//! File watcher + incremental reconciler for GraphSwarm.
//!
//! When `graphswarm server --watch` is active, GraphSwarm keeps the indexed
//! call graph in sync with on-disk source changes without requiring a full
//! re-index.
//!
//! Architecture:
//!
//!   FileWatcher  ─── FileEvent ──▶  mpsc channel
//!                                        │
//!                                   Reconciler
//!                                        │
//!                                   GraphStore (sled)
//!
//! FileWatcher uses `notify-debouncer-mini` (wraps `notify 6.x`) to
//! receive OS filesystem events with a 500ms debounce window.
//!
//! Reconciler reads from the channel and for each event:
//!   Modified / Created → mark_stale + delete_file + re-index + clear_stale
//!   Deleted            → mark_stale + delete_file (no re-index)
//!   Renamed            → delete old + re-index new

pub mod file_watcher;
pub mod reconciler;

pub use file_watcher::FileWatcher;
pub use reconciler::Reconciler;

use std::path::PathBuf;

/// A filesystem change event produced by the FileWatcher.
#[derive(Debug, Clone)]
pub struct FileEvent {
    pub kind: EventKind,
    /// Absolute path of the changed file.
    pub path: PathBuf,
}

/// The kind of filesystem change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    /// File was created (new file appeared).
    Created,
    /// File was modified.
    Modified,
    /// File was deleted.
    Deleted,
    /// File was renamed: `path` is the new path.
    Renamed,
}

/// Source file extensions that GraphSwarm indexes.
///
/// Files with other extensions (e.g. .txt, .md, .json) are ignored by the
/// watcher to avoid spurious re-indexing. Must stay in sync with the
/// extension lists in `indexer::parser` -every extension the parser can
/// index should be watched, or `--watch` serves stale data for those files.
pub const WATCHED_EXTENSIONS: &[&str] = &[
    "rs", "py", "js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts", "go",
];

/// Returns true if `path` has an extension that GraphSwarm can index.
pub fn is_source_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| WATCHED_EXTENSIONS.contains(&ext))
        .unwrap_or(false)
}
