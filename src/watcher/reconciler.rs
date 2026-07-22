//! Incremental reconciler: consumes FileEvents and keeps the graph in sync.
//!
//! For each event:
//!   Modified / Created →
//!     1. mark_stale(path)         -query results warn about this file
//!     2. delete_file(path)        -remove stale entities + cascaded edges
//!     3. re-index the file        -parse, extract entities
//!     4. merge new entities       -store_file_entities()
//!     5. clear_stale(path)        -remove stale warning
//!
//!   Deleted →
//!     1. mark_stale(path)
//!     2. delete_file(path)
//!     (no re-index -file is gone)
//!
//!   Renamed →
//!     1. delete_file(old path, inferred from event)  -may be the same path
//!     2. treat new path as Modified

use std::path::{Path, PathBuf};

use chrono::Utc;
use tokio::sync::mpsc::Receiver;

use super::{EventKind, FileEvent};
use crate::error::Result;
use crate::indexer::CodeIndexer;
use crate::storage::GraphStore;

/// Consumes `FileEvent`s and incrementally updates the `GraphStore`.
pub struct Reconciler {
    rx: Receiver<FileEvent>,
    store: GraphStore,
    /// Reserved for Phase 7 cross-file edge resolution after incremental re-index.
    _repo_root: PathBuf,
}

impl Reconciler {
    pub fn new(rx: Receiver<FileEvent>, store: GraphStore, repo_root: PathBuf) -> Self {
        Self {
            rx,
            store,
            _repo_root: repo_root,
        }
    }

    /// Runs the reconcile loop until the channel closes.
    ///
    /// This is an async function -it should be spawned as a Tokio task.
    pub async fn run(mut self) {
        eprintln!("[graphswarm reconciler] started");

        while let Some(event) = self.rx.recv().await {
            let path_str = event.path.to_string_lossy().to_string();
            eprintln!("[graphswarm reconciler] {:?} {}", event.kind, path_str);

            let result = match event.kind {
                // The watcher infers Created as Modified (both mean "file exists now").
                EventKind::Created | EventKind::Modified => self.reconcile_modified(&event.path),
                EventKind::Deleted => self.reconcile_deleted(&event.path),
                EventKind::Renamed => self.reconcile_modified(&event.path),
            };

            if let Err(e) = result {
                eprintln!("[graphswarm reconciler] error reconciling {path_str}: {e}");
            }

            // Record the last successful reconcile time.
            let _ = self.store.set_last_reconcile_time(&Utc::now().to_rfc3339());
        }

        eprintln!("[graphswarm reconciler] stopped");
    }

    fn reconcile_modified(&self, path: &Path) -> Result<()> {
        let path_str = path.to_string_lossy().to_string();

        // Step 1: mark stale so live queries show the warning immediately.
        self.store.mark_stale(&path_str)?;

        // Step 2: remove existing entities (with edge cascade).
        self.store.delete_file(&path_str)?;

        // Step 3: re-index the single file.
        // We re-use CodeIndexer but only index this one file. Because the
        // indexer operates on directories, we index the parent and filter
        // to entities from this file. For single-file accuracy we build a
        // mini graph from just this file.
        if !path.exists() {
            // File may have been deleted between event and now -treat as deletion.
            return Ok(());
        }

        let indexer = CodeIndexer::new("auto")?;
        let content = std::fs::read_to_string(path)?;
        let entities = indexer
            .parser_ref()
            .parse_source(&path_str, &content)
            .unwrap_or_default();

        // Step 4: store the new entities.
        for entity in &entities {
            self.store.store_single_entity(entity)?;
        }

        // Step 5: clear stale now that re-index succeeded.
        self.store.clear_stale(&path_str)?;

        eprintln!(
            "[graphswarm reconciler] reconciled {} ({} entities)",
            path_str,
            entities.len()
        );
        Ok(())
    }

    fn reconcile_deleted(&self, path: &Path) -> Result<()> {
        let path_str = path.to_string_lossy().to_string();
        self.store.mark_stale(&path_str)?;
        self.store.delete_file(&path_str)?;
        // Don't clear_stale -the file is gone so there's nothing to re-index.
        // Queries will show the warning, reminding users the file was removed.
        eprintln!("[graphswarm reconciler] deleted {path_str}");
        Ok(())
    }
}
