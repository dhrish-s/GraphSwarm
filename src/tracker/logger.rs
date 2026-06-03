//! Async non-blocking action logger for GraphSwarm.
//!
//! ## The core problem
//!
//! Writing to disk takes microseconds to milliseconds. If we write
//! synchronously on the query path, every query gets slower. Our target
//! is < 1ms query latency — we cannot afford disk I/O there.
//!
//! ## Solution: decouple logging with a Tokio channel
//!
//! How `tokio::sync::mpsc` works ("multi-producer, single-consumer"):
//!
//!   - Many callers can SEND actions into the channel (tx = transmitter).
//!   - One background task RECEIVES from the channel (rx = receiver).
//!   - Sending is non-blocking — it returns in ~100ns.
//!   - Receiving blocks only the background task, never the caller.
//!
//!   Timeline:
//!
//! ```text
//!   Caller (query path):  log(action) -> tx.send(action) -> returns instantly
//!                                              | (channel buffer, capacity 1000)
//!   Background task:                   <- rx.recv() -> kv.set(...) -> done
//! ```
//!
//!   The caller never waits for the KV write. The channel absorbs the gap.
//!
//! ## Channel capacity
//!
//! We use a bounded channel with capacity 1000. If the background task
//! falls behind by > 1000 actions, `send()` will block — this provides
//! backpressure and prevents unbounded memory growth. At 10 agent
//! actions/second, the buffer lasts 100 seconds before backpressure kicks in.
//!
//! ## Shutdown
//!
//! When the last `ActionLogger` clone is dropped, `tx` is dropped, which
//! closes the channel. The background task's `rx.recv()` returns `None` and
//! it exits cleanly — no explicit shutdown signal needed.

use crate::error::Result;
use crate::storage::kv_backend::KvBackend;
use crate::storage::schema::{
    action_key, history_count_key, history_error_key, history_recent_key,
};
use crate::tracker::action_log::{AgentAction, ActionType, FileAccessCount};
use tokio::sync::mpsc;

/// Buffered actions between the caller and the background write task.
/// At 10 actions/second, this gives ~100 seconds of headroom before backpressure.
const CHANNEL_CAPACITY: usize = 1000;

/// Async non-blocking action logger.
///
/// Cheap to clone — each clone holds only an `mpsc::Sender`, which is
/// internally reference-counted. All clones share the same background task
/// and the same KV database.
#[derive(Clone)]
pub struct ActionLogger {
    /// Sender half of the bounded Tokio channel.
    /// Cloning the sender is O(1) — it just increments an Arc counter.
    tx: mpsc::Sender<AgentAction>,
}

impl ActionLogger {
    /// Creates a new `ActionLogger` and spawns the background write task.
    ///
    /// The background task owns the `KvBackend` and `rx` (the receiver).
    /// It runs until all `ActionLogger` instances (including clones) are dropped.
    ///
    /// # Panics
    /// Panics if called outside a Tokio runtime (no `#[tokio::main]` context).
    pub fn new(kv: KvBackend) -> Self {
        // mpsc::channel returns (sender, receiver).
        // The sender (tx) lives in ActionLogger; the receiver (rx) goes to the task.
        let (tx, rx) = mpsc::channel::<AgentAction>(CHANNEL_CAPACITY);

        // tokio::spawn creates a concurrent task — like a lightweight green thread.
        // It takes ownership of rx and kv. The future returned by background_writer
        // is polled by the Tokio runtime without blocking the calling thread.
        tokio::spawn(background_writer(rx, kv));

        Self { tx }
    }

    /// Logs an action asynchronously.
    ///
    /// Places the action into the channel and returns immediately (~100ns).
    /// The actual KV write happens in the background task.
    ///
    /// # Errors
    /// Returns `Err` only if the background task has crashed and the
    /// channel is closed — an extremely rare condition.
    pub async fn log(&self, action: AgentAction) -> Result<()> {
        // send() is async because a bounded channel blocks when full.
        // With capacity=1000 and typical agent speed (~10 actions/s),
        // this will effectively never block in practice.
        self.tx.send(action).await.map_err(|e| {
            crate::error::Error::tracker(format!(
                "Action logger channel closed (background task may have panicked): {}",
                e
            ))
        })
    }

    /// Convenience: log a `FileRead` action.
    pub async fn log_file_read(&self, file_path: &str) -> Result<()> {
        self.log(AgentAction::new(ActionType::FileRead, file_path)).await
    }

    /// Convenience: log a `FileEdit` action.
    pub async fn log_file_edit(&self, file_path: &str) -> Result<()> {
        self.log(AgentAction::new(ActionType::FileEdit, file_path)).await
    }

    /// Convenience: log an `Error` action with a message in metadata.
    pub async fn log_error(&self, file_path: &str, message: &str) -> Result<()> {
        let action = AgentAction::new(ActionType::Error, file_path)
            .with_metadata("message", serde_json::Value::String(message.to_string()));
        self.log(action).await
    }
}

/// Background task: drains the channel and writes each action to KV.
///
/// Runs until the channel closes (all `ActionLogger` senders dropped).
///
/// Each action produces up to 4 KV writes (same "pre-compute at write time"
/// pattern as the graph storage layer):
///   1. `action:{uuid}`                     — full record for `get_action()`
///   2. `history:recent:{ts}:{uuid}`        — file path for `recent_files()`
///   3. `history:count:{file_path}`         — frequency counter for `frequent_files()`
///   4. `history:error:{ts}:{uuid}`         — only if `action.is_error()`
///
/// Errors are printed to stderr but do NOT stop the task — a transient
/// disk error on one action must not kill logging for all future actions.
async fn background_writer(mut rx: mpsc::Receiver<AgentAction>, kv: KvBackend) {
    // `rx.recv()` returns None when every sender (tx) has been dropped.
    // The while-let loop exits cleanly at that point — no explicit shutdown needed.
    while let Some(action) = rx.recv().await {
        // ── Write 1: full action record ──────────────────────────────────────
        if let Err(e) = kv.set(&action_key(&action.id.to_string()), &action) {
            eprintln!("[graphswarm tracker] failed to write action {}: {}", action.id, e);
            continue; // skip remaining writes for this action; move to next
        }

        // ── Write 2: recency index ────────────────────────────────────────────
        // Key sorts chronologically so recent_files() just reverses the scan.
        let recent_key = history_recent_key(
            &action.timestamp.to_rfc3339(),
            &action.id.to_string(),
        );
        if let Err(e) = kv.set(&recent_key, &action.file_path) {
            eprintln!("[graphswarm tracker] failed to write recency index: {}", e);
        }

        // ── Write 3: frequency counter (read-modify-write) ───────────────────
        update_file_count(&kv, &action);

        // ── Write 4: error index (conditional) ───────────────────────────────
        if action.is_error() {
            let error_key = history_error_key(
                &action.timestamp.to_rfc3339(),
                &action.id.to_string(),
            );
            if let Err(e) = kv.set(&error_key, &action) {
                eprintln!("[graphswarm tracker] failed to write error index: {}", e);
            }
        }
    }
    // Channel closed → task exits. Tokio cleans up the task automatically.
}

/// Atomically-ish increments the per-file access counter in KV.
///
/// This is a non-atomic read-modify-write. If two background tasks updated
/// the same file concurrently (impossible here — we have one background task),
/// the last writer would win. For a relevance *hint*, last-write-wins is fine.
///
/// `kv.get` returns `None` on first access → we start the counter at 0.
fn update_file_count(kv: &KvBackend, action: &AgentAction) {
    let count_key = history_count_key(&action.file_path);

    // Read current counter or initialise a fresh one
    let mut fac: FileAccessCount = kv
        .get::<FileAccessCount>(&count_key)
        .unwrap_or(None)
        .unwrap_or_else(|| FileAccessCount {
            file_path:     action.file_path.clone(),
            count:         0,
            last_accessed: action.timestamp,
        });

    fac.count += 1;
    fac.last_accessed = action.timestamp;

    if let Err(e) = kv.set(&count_key, &fac) {
        eprintln!("[graphswarm tracker] failed to update file count: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::kv_backend::KvBackend;
    use crate::tracker::history::History;
    use std::time::Duration;
    use tempfile::TempDir;

    /// Returns (ActionLogger, History, TempDir) all pointing at the same sled DB.
    ///
    /// `KvBackend` is Arc-backed — cloning gives a second handle to the same db.
    fn open_logger_and_history() -> (ActionLogger, History, TempDir) {
        let dir = TempDir::new().unwrap();
        let kv = KvBackend::open(dir.path()).unwrap();
        let logger = ActionLogger::new(kv.clone());
        let history = History::new(kv);
        (logger, history, dir)
    }

    /// Sleeps long enough for the background task to drain the channel and
    /// commit all pending KV writes.
    ///
    /// Why is this necessary? `log()` returns after placing the action in the
    /// channel. The actual sled write happens in `background_writer`, which
    /// runs as a separate Tokio task. Without a brief sleep, the test's
    /// assertions might run before the background task has written anything.
    async fn flush_background() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // ── basic smoke tests ────────────────────────────────────────────────────

    #[tokio::test]
    async fn logger_new_does_not_panic() {
        let dir = TempDir::new().unwrap();
        let kv = KvBackend::open(dir.path()).unwrap();
        let _logger = ActionLogger::new(kv);
        // Reaching here without panic = success
    }

    #[tokio::test]
    async fn log_returns_ok() {
        let (logger, _history, _dir) = open_logger_and_history();
        let action = AgentAction::new(ActionType::FileRead, "src/auth.rs");
        assert!(logger.log(action).await.is_ok());
    }

    // ── convenience helpers ──────────────────────────────────────────────────

    #[tokio::test]
    async fn log_file_read_visible_in_history() {
        let (logger, history, _dir) = open_logger_and_history();
        logger.log_file_read("src/auth.rs").await.unwrap();
        // Give the background task time to write before we query
        flush_background().await;
        let files = history.recent_files(10).unwrap();
        assert!(files.contains(&"src/auth.rs".to_string()));
    }

    #[tokio::test]
    async fn log_file_edit_visible_in_history() {
        let (logger, history, _dir) = open_logger_and_history();
        logger.log_file_edit("src/main.rs").await.unwrap();
        flush_background().await;
        let files = history.recent_files(10).unwrap();
        assert!(files.contains(&"src/main.rs".to_string()));
    }

    #[tokio::test]
    async fn log_error_visible_in_recent_errors() {
        let (logger, history, _dir) = open_logger_and_history();
        logger.log_error("src/lib.rs", "compile error").await.unwrap();
        flush_background().await;
        let errors = history.recent_errors(10).unwrap();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].is_error());
        assert_eq!(errors[0].file_path, "src/lib.rs");
    }

    // ── deduplication / counting ─────────────────────────────────────────────

    #[tokio::test]
    async fn same_file_multiple_times_deduplicates_in_recent_files() {
        let (logger, history, _dir) = open_logger_and_history();
        logger.log_file_read("src/auth.rs").await.unwrap();
        logger.log_file_read("src/auth.rs").await.unwrap();
        logger.log_file_read("src/auth.rs").await.unwrap();
        flush_background().await;
        let files = history.recent_files(10).unwrap();
        let occurrences = files.iter().filter(|f| f.as_str() == "src/auth.rs").count();
        assert_eq!(occurrences, 1, "recent_files must deduplicate");
    }

    #[tokio::test]
    async fn same_file_multiple_times_increments_count() {
        let (logger, history, _dir) = open_logger_and_history();
        logger.log_file_read("src/auth.rs").await.unwrap();
        logger.log_file_read("src/auth.rs").await.unwrap();
        logger.log_file_read("src/auth.rs").await.unwrap();
        flush_background().await;
        assert_eq!(history.file_access_count("src/auth.rs").unwrap(), 3);
    }

    // ── clone semantics ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn cloned_logger_logs_successfully() {
        // Both clones share the same channel → both are visible in history
        let (logger, history, _dir) = open_logger_and_history();
        let logger2 = logger.clone();
        logger.log_file_read("a.rs").await.unwrap();
        logger2.log_file_read("b.rs").await.unwrap();
        flush_background().await;
        let files = history.recent_files(10).unwrap();
        assert!(files.contains(&"a.rs".to_string()));
        assert!(files.contains(&"b.rs".to_string()));
    }

    // ── background task writes full record ───────────────────────────────────

    #[tokio::test]
    async fn background_task_writes_action_to_kv() {
        let (logger, history, _dir) = open_logger_and_history();
        let action = AgentAction::new(ActionType::FileRead, "src/auth.rs");
        let action_id = action.id.to_string();
        logger.log(action).await.unwrap();
        flush_background().await;
        let retrieved = history.get_action(&action_id).unwrap();
        assert!(retrieved.is_some(), "full action record must be in KV");
        assert_eq!(retrieved.unwrap().file_path, "src/auth.rs");
    }

    // ── limit / filter ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn recent_files_n_limit_respected() {
        let (logger, history, _dir) = open_logger_and_history();
        logger.log_file_read("a.rs").await.unwrap();
        logger.log_file_read("b.rs").await.unwrap();
        logger.log_file_read("c.rs").await.unwrap();
        flush_background().await;
        let files = history.recent_files(2).unwrap();
        assert_eq!(files.len(), 2);
    }

    #[tokio::test]
    async fn non_error_does_not_appear_in_recent_errors() {
        let (logger, history, _dir) = open_logger_and_history();
        logger.log_file_read("a.rs").await.unwrap();
        logger.log_error("b.rs", "fail").await.unwrap();
        flush_background().await;
        let errors = history.recent_errors(10).unwrap();
        // Only the error action must appear
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file_path, "b.rs");
    }

    // ── total_actions ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn total_actions_correct() {
        let (logger, history, _dir) = open_logger_and_history();
        for i in 0..5_u32 {
            logger.log_file_read(&format!("file{}.rs", i)).await.unwrap();
        }
        flush_background().await;
        assert_eq!(history.total_actions().unwrap(), 5);
    }

    // ── clean shutdown ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn drop_logger_does_not_panic() {
        let dir = TempDir::new().unwrap();
        let kv = KvBackend::open(dir.path()).unwrap();
        {
            let logger = ActionLogger::new(kv);
            logger.log_file_read("x.rs").await.unwrap();
            // logger is dropped here:
            //   tx drops → channel closes → background_writer exits cleanly
        }
        // Reaching here without panic = success
    }
}
