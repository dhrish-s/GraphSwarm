//! Filesystem event watcher with 500ms debounce.
//!
//! Uses `notify-debouncer-mini` v0.4 which wraps `notify 6.x`.
//! The debouncer coalesces rapid OS events (e.g. a save that writes then renames)
//! into a single logical event per 500ms window.
//!
//! In debouncer v0.4 the `DebouncedEvent` exposes only `DebouncedEventKind::Any`
//! or `AnyContinuous` -it does not expose the underlying notify event type.
//! We infer Created/Modified vs Deleted by checking whether the path still
//! exists on disk after the debounce window expires.
//!
//! Why debounce?
//! Editors often write a file in multiple steps (write tmp, rename to target).
//! Without debouncing the reconciler would receive several partial-write events
//! and potentially re-index an incomplete file. 500ms gives the editor time to finish.

use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use notify_debouncer_mini::new_debouncer;
use notify_debouncer_mini::notify::RecursiveMode;

use super::{is_source_file, EventKind, FileEvent};
use crate::error::{Error, Result};

/// Watches a directory tree and sends `FileEvent`s through the given channel.
pub struct FileWatcher {
    root: PathBuf,
}

impl FileWatcher {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Starts watching `self.root` on the **current thread** (blocking).
    ///
    /// Sends `FileEvent`s to `tx` for every source-file change detected.
    /// Returns when `tx` is closed (the reconciler has shut down).
    ///
    /// Call from a dedicated std thread:
    /// ```rust,ignore
    /// let (tx, rx) = tokio::sync::mpsc::channel(256);
    /// std::thread::spawn(move || watcher.start(tx));
    /// ```
    pub fn start(&self, tx: tokio::sync::mpsc::Sender<FileEvent>) -> Result<()> {
        let (raw_tx, raw_rx) = std_mpsc::channel();

        let mut debouncer = new_debouncer(Duration::from_millis(500), raw_tx)
            .map_err(|e| Error::storage(format!("Failed to create file watcher: {e}")))?;

        debouncer
            .watcher()
            .watch(&self.root, RecursiveMode::Recursive)
            .map_err(|e| Error::storage(format!("Failed to watch {}: {e}", self.root.display())))?;

        for result in raw_rx {
            match result {
                Ok(events) => {
                    for event in events {
                        let path = event.path.clone();

                        // Ignore non-source files (.md, .json, .lock, etc.)
                        if !is_source_file(&path) {
                            continue;
                        }

                        // debouncer v0.4 gives us DebouncedEventKind::Any/AnyContinuous - 
                        // we infer the semantic kind from whether the file still exists.
                        let kind = if path.exists() {
                            EventKind::Modified
                        } else {
                            EventKind::Deleted
                        };

                        if tx.blocking_send(FileEvent { kind, path }).is_err() {
                            return Ok(()); // receiver dropped
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[graphswarm watcher] notify error: {e:?}");
                }
            }
        }

        Ok(())
    }
}
