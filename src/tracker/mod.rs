//! Action tracking layer for GraphSwarm.
//!
//! The tracker has two sides:
//!
//!   **WRITE side** -`ActionLogger` (`logger.rs`)
//!   - async, non-blocking
//!   - uses a Tokio `mpsc` channel to decouple logging from the query path
//!   - background task drains the channel and writes to KV
//!
//!   **READ side** -`History` (`history.rs`)
//!   - synchronous KV prefix scans
//!   - `recent_files()`, `frequent_files()`, `recent_errors()`
//!   - used by Part 4's query engine to boost file relevance
//!
//!   **DATA side** -`AgentAction`, `ActionType`, `FileAccessCount` (`action_log.rs`)
//!   - plain data structs with no behaviour -only serde and builder methods
//!
//! ## Usage in the query engine (Part 4)
//! ```ignore
//! let history = History::new(kv.clone());
//! let recent  = history.recent_files(5)?;
//! // boost relevance score for any file in `recent`
//! ```

pub mod action_log;
pub mod history;
pub mod logger;

pub use action_log::{ActionType, AgentAction, FileAccessCount};
pub use history::History;
pub use logger::ActionLogger;

/// Backward-compatibility alias used by `lib.rs::prelude`.
/// `ActionTracker` and `ActionLogger` are the same type.
pub type ActionTracker = ActionLogger;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::kv_backend::KvBackend;
    use tempfile::TempDir;

    /// Verify the ActionTracker type alias compiles and behaves like ActionLogger.
    #[tokio::test]
    async fn action_tracker_alias_works() {
        let dir = TempDir::new().unwrap();
        let kv = KvBackend::open(dir.path()).unwrap();
        // ActionTracker == ActionLogger -must accept the same constructor
        let tracker: ActionTracker = ActionTracker::new(kv);
        // Must be able to log without error
        tracker.log_file_read("src/auth.rs").await.unwrap();
    }
}
