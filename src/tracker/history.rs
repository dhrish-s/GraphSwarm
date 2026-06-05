//! Query layer for agent action history stored in KV-SWARM.
//!
//! `History` is the READ side of the tracker.
//! `ActionLogger` (logger.rs) is the WRITE side.
//!
//! All queries are prefix scans over the sled B-tree, which are O(log n + k)
//! where k = matching records. For typical history sizes (< 10k records per
//! session) every call completes in well under 1ms.
//!
//! Design: `History` owns a `KvBackend`. Since `KvBackend` is Arc-backed
//! (cloning it only increments a reference count), multiple parts of the
//! system can hold independent `History` or `ActionLogger` instances that
//! all read/write the same underlying sled database.

use std::collections::HashSet;

use chrono::{DateTime, Utc};

use crate::error::Result;
use crate::storage::kv_backend::KvBackend;
use crate::storage::schema::{action_key, history_count_key};
use crate::tracker::action_log::{AgentAction, FileAccessCount};

/// Read-only query layer over the persisted action history.
pub struct History {
    kv: KvBackend,
}

impl History {
    pub fn new(kv: KvBackend) -> Self {
        Self { kv }
    }

    /// Returns the `n` most recently accessed **unique** file paths, newest first.
    ///
    /// Algorithm:
    /// 1. Prefix-scan `"history:recent:"` -sled returns keys in ascending
    ///    byte order, which equals ascending chronological order for RFC3339 keys.
    /// 2. Reverse the key list so we iterate newest-first.
    /// 3. Collect distinct file paths until we have `n`.
    ///
    /// Complexity: O(k) where k = total history records (< 10k typical).
    pub fn recent_files(&self, n: usize) -> Result<Vec<String>> {
        if n == 0 {
            return Ok(Vec::new());
        }

        let mut seen: HashSet<String> = HashSet::new();
        let mut result = Vec::new();
        let keys = self.kv.list_prefix("history:recent:")?;

        // Reverse: sled returns ascending order, we want newest first
        for key in keys.iter().rev() {
            if let Some(file_path) = self.kv.get::<String>(key)? {
                // HashSet::insert returns true only if the value was newly inserted
                if seen.insert(file_path.clone()) {
                    result.push(file_path);
                    if result.len() >= n {
                        break;
                    }
                }
            }
        }

        Ok(result)
    }

    /// Returns the `n` most frequently accessed files, sorted by count descending.
    ///
    /// Algorithm:
    /// 1. Prefix-scan `"history:count:"` -one `FileAccessCount` per unique file.
    /// 2. Sort by count descending; use `last_accessed` as a tiebreaker.
    /// 3. Truncate to `n`.
    ///
    /// Complexity: O(V log V) where V = unique files accessed. For V < 1000 this
    /// is microseconds.
    pub fn frequent_files(&self, n: usize) -> Result<Vec<FileAccessCount>> {
        if n == 0 {
            return Ok(Vec::new());
        }

        let keys = self.kv.list_prefix("history:count:")?;
        let mut counts: Vec<FileAccessCount> = Vec::with_capacity(keys.len());

        for key in &keys {
            if let Some(fac) = self.kv.get::<FileAccessCount>(key)? {
                counts.push(fac);
            }
        }

        // Sort descending by count, then descending by last_accessed as tiebreaker
        counts.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| b.last_accessed.cmp(&a.last_accessed))
        });

        counts.truncate(n);
        Ok(counts)
    }

    /// Returns the `n` most recent **error** actions, newest first.
    ///
    /// Error actions are indexed separately under `"history:error:"` so this
    /// query scans only errors -no filtering over the full history needed.
    pub fn recent_errors(&self, n: usize) -> Result<Vec<AgentAction>> {
        if n == 0 {
            return Ok(Vec::new());
        }

        let keys = self.kv.list_prefix("history:error:")?;
        let mut errors = Vec::new();

        // Reverse: ascending key order = ascending time, we want newest first
        for key in keys.iter().rev() {
            if let Some(action) = self.kv.get::<AgentAction>(key)? {
                errors.push(action);
                if errors.len() >= n {
                    break;
                }
            }
        }

        Ok(errors)
    }

    /// Returns the total number of recorded actions (`action:*` keys).
    pub fn total_actions(&self) -> Result<usize> {
        Ok(self.kv.list_prefix("action:")?.len())
    }

    /// Returns a single action by its UUID string, or `None` if not found.
    pub fn get_action(&self, action_id: &str) -> Result<Option<AgentAction>> {
        self.kv.get(&action_key(action_id))
    }

    /// Returns the most recent UTC timestamp at which the agent accessed `file_path`,
    /// or `None` if the file has never been logged.
    ///
    /// Algorithm: scan all `history:recent:` keys (time-ordered by prefix), load
    /// only entries whose value matches `file_path`, extract the RFC3339 timestamp
    /// embedded in the key, and return the maximum.
    ///
    /// Key format: `history:recent:{rfc3339}:{uuid}`
    /// UUID contains only hyphens -no colons -so `rfind(':')` reliably splits
    /// the timestamp from the UUID suffix.
    ///
    /// Complexity: O(k) where k = total history records.
    pub fn file_last_accessed(&self, file_path: &str) -> Result<Option<DateTime<Utc>>> {
        let keys = self.kv.list_prefix("history:recent:")?;
        let mut latest: Option<DateTime<Utc>> = None;

        for key in &keys {
            if let Some(path) = self.kv.get::<String>(key)? {
                if path == file_path {
                    // Strip prefix, then split at the rightmost ':' to isolate the
                    // timestamp. UUID is always 36 chars and contains no colons,
                    // so rfind(':') finds the separator between timestamp and UUID.
                    if let Some(after) = key.strip_prefix("history:recent:") {
                        if let Some(colon_idx) = after.rfind(':') {
                            let ts_str = &after[..colon_idx];
                            if let Ok(dt) = DateTime::parse_from_rfc3339(ts_str) {
                                let dt_utc = dt.with_timezone(&Utc);
                                if latest.is_none() || dt_utc > *latest.as_ref().unwrap() {
                                    latest = Some(dt_utc);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(latest)
    }

    /// Returns the recorded access count for `file_path`, or `0` if the file
    /// has never been logged.
    pub fn file_access_count(&self, file_path: &str) -> Result<u64> {
        let fac: Option<FileAccessCount> = self.kv.get(&history_count_key(file_path))?;
        Ok(fac.map(|f| f.count).unwrap_or(0))
    }

    /// Clears all action history from the KV store.
    ///
    /// Only removes tracker-owned prefixes (`action:`, `history:*`).
    /// Graph data (`entity:`, `edge:`, etc.) is untouched.
    pub fn clear(&self) -> Result<()> {
        for prefix in &[
            "action:",
            "history:recent:",
            "history:count:",
            "history:error:",
        ] {
            for key in self.kv.list_prefix(prefix)? {
                self.kv.delete(&key)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::kv_backend::KvBackend;
    use crate::storage::schema::{
        action_key, history_count_key, history_error_key, history_recent_key,
    };
    use crate::tracker::action_log::{ActionType, AgentAction, FileAccessCount};
    use chrono::{Duration, Utc};
    use std::collections::HashMap;
    use tempfile::TempDir;
    use uuid::Uuid;

    /// Creates a KvBackend seeded with 5 known actions across 3 files.
    ///
    /// Access summary (chronological order t1 < t2 < t3 < t4 < t5):
    ///   file_a.rs -3 accesses (t1, t3, t5)  -most frequent, most recent
    ///   file_b.rs -1 access  (t2)
    ///   file_c.rs -1 access  (t4)            -only error action
    fn seeded_history() -> (History, TempDir) {
        let dir = TempDir::new().unwrap();
        let kv = KvBackend::open(dir.path()).unwrap();

        let now = Utc::now();
        let t1 = now - Duration::seconds(500);
        let t2 = now - Duration::seconds(400);
        let t3 = now - Duration::seconds(300);
        let t4 = now - Duration::seconds(200);
        let t5 = now - Duration::seconds(100);

        // (timestamp, file, action_type, is_error)
        let specs: Vec<(chrono::DateTime<Utc>, &str, ActionType, bool)> = vec![
            (t1, "file_a.rs", ActionType::FileRead, false),
            (t2, "file_b.rs", ActionType::FileRead, false),
            (t3, "file_a.rs", ActionType::FileEdit, false),
            (t4, "file_c.rs", ActionType::Error, true),
            (t5, "file_a.rs", ActionType::FileRead, false),
        ];

        for (ts, file, atype, is_err) in specs {
            let action = AgentAction {
                id: Uuid::new_v4(),
                action_type: atype,
                file_path: file.to_string(),
                entity_id: None,
                timestamp: ts,
                metadata: HashMap::new(),
            };

            // Full record under action:{uuid}
            kv.set(&action_key(&action.id.to_string()), &action)
                .unwrap();
            // Recency index: value is the file path string
            kv.set(
                &history_recent_key(&ts.to_rfc3339(), &action.id.to_string()),
                &action.file_path,
            )
            .unwrap();
            // Error index: only written for error actions
            if is_err {
                kv.set(
                    &history_error_key(&ts.to_rfc3339(), &action.id.to_string()),
                    &action,
                )
                .unwrap();
            }
        }

        // Per-file frequency counters
        let counts = [
            FileAccessCount {
                file_path: "file_a.rs".into(),
                count: 3,
                last_accessed: t5,
            },
            FileAccessCount {
                file_path: "file_b.rs".into(),
                count: 1,
                last_accessed: t2,
            },
            FileAccessCount {
                file_path: "file_c.rs".into(),
                count: 1,
                last_accessed: t4,
            },
        ];
        for c in &counts {
            kv.set(&history_count_key(&c.file_path), c).unwrap();
        }

        (History::new(kv), dir)
    }

    // ── recent_files ─────────────────────────────────────────────────────────

    #[test]
    fn recent_files_returns_n_most_recent_unique() {
        let (history, _dir) = seeded_history();
        let files = history.recent_files(3).unwrap();
        assert_eq!(files.len(), 3);
        assert!(files.contains(&"file_a.rs".to_string()));
        assert!(files.contains(&"file_b.rs".to_string()));
        assert!(files.contains(&"file_c.rs".to_string()));
    }

    #[test]
    fn recent_files_deduplicates() {
        // file_a.rs was accessed 3 times but must appear only once
        let (history, _dir) = seeded_history();
        let files = history.recent_files(10).unwrap();
        let count = files.iter().filter(|f| f.as_str() == "file_a.rs").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn recent_files_zero_returns_empty() {
        let (history, _dir) = seeded_history();
        assert!(history.recent_files(0).unwrap().is_empty());
    }

    #[test]
    fn recent_files_empty_store_returns_empty() {
        let dir = TempDir::new().unwrap();
        let history = History::new(KvBackend::open(dir.path()).unwrap());
        assert!(history.recent_files(10).unwrap().is_empty());
    }

    #[test]
    fn recent_files_newest_first() {
        // t5 (most recent) touches file_a.rs -it must be element [0]
        let (history, _dir) = seeded_history();
        let files = history.recent_files(2).unwrap();
        assert_eq!(files[0], "file_a.rs");
    }

    #[test]
    fn recent_files_more_than_available_returns_all() {
        let (history, _dir) = seeded_history();
        // Only 3 unique files exist; asking for 100 must return exactly 3
        let files = history.recent_files(100).unwrap();
        assert_eq!(files.len(), 3);
    }

    // ── frequent_files ────────────────────────────────────────────────────────

    #[test]
    fn frequent_files_sorted_by_count_descending() {
        let (history, _dir) = seeded_history();
        let counts = history.frequent_files(3).unwrap();
        // file_a.rs has count=3, others have count=1
        assert_eq!(counts[0].file_path, "file_a.rs");
        assert_eq!(counts[0].count, 3);
        // Verify descending order throughout the list
        assert!(counts.windows(2).all(|w| w[0].count >= w[1].count));
    }

    #[test]
    fn frequent_files_top_one_is_most_accessed() {
        let (history, _dir) = seeded_history();
        let top = history.frequent_files(1).unwrap();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].file_path, "file_a.rs");
    }

    #[test]
    fn frequent_files_zero_returns_empty() {
        let (history, _dir) = seeded_history();
        assert!(history.frequent_files(0).unwrap().is_empty());
    }

    #[test]
    fn frequent_files_empty_store_returns_empty() {
        let dir = TempDir::new().unwrap();
        let history = History::new(KvBackend::open(dir.path()).unwrap());
        assert!(history.frequent_files(10).unwrap().is_empty());
    }

    // ── recent_errors ─────────────────────────────────────────────────────────

    #[test]
    fn recent_errors_returns_only_errors() {
        let (history, _dir) = seeded_history();
        let errors = history.recent_errors(10).unwrap();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].is_error());
    }

    #[test]
    fn recent_errors_newest_first() {
        // Build a fresh store with two error actions at different times
        let dir = TempDir::new().unwrap();
        let kv = KvBackend::open(dir.path()).unwrap();
        let now = Utc::now();
        let t1 = now - Duration::seconds(200); // older
        let t2 = now - Duration::seconds(100); // newer

        for (ts, file) in [(t1, "old_error.rs"), (t2, "new_error.rs")] {
            let action = AgentAction {
                id: Uuid::new_v4(),
                action_type: ActionType::Error,
                file_path: file.to_string(),
                entity_id: None,
                timestamp: ts,
                metadata: HashMap::new(),
            };
            kv.set(&action_key(&action.id.to_string()), &action)
                .unwrap();
            kv.set(
                &history_error_key(&ts.to_rfc3339(), &action.id.to_string()),
                &action,
            )
            .unwrap();
        }

        let history = History::new(kv);
        let errors = history.recent_errors(10).unwrap();
        // Newest error (new_error.rs at t2) must be first
        assert_eq!(errors[0].file_path, "new_error.rs");
    }

    #[test]
    fn recent_errors_empty_when_no_errors() {
        let dir = TempDir::new().unwrap();
        let kv = KvBackend::open(dir.path()).unwrap();
        // Write only a non-error action -no history:error: keys written
        let action = AgentAction {
            id: Uuid::new_v4(),
            action_type: ActionType::FileRead,
            file_path: "a.rs".into(),
            entity_id: None,
            timestamp: Utc::now(),
            metadata: HashMap::new(),
        };
        kv.set(&action_key(&action.id.to_string()), &action)
            .unwrap();
        kv.set(
            &history_recent_key(&action.timestamp.to_rfc3339(), &action.id.to_string()),
            &action.file_path,
        )
        .unwrap();

        let history = History::new(kv);
        assert!(history.recent_errors(10).unwrap().is_empty());
    }

    #[test]
    fn recent_errors_n_one_returns_most_recent() {
        let (history, _dir) = seeded_history();
        let errors = history.recent_errors(1).unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file_path, "file_c.rs");
    }

    // ── total_actions / get_action ────────────────────────────────────────────

    #[test]
    fn total_actions_correct() {
        let (history, _dir) = seeded_history();
        // seeded_history writes 5 action:{uuid} keys
        assert_eq!(history.total_actions().unwrap(), 5);
    }

    #[test]
    fn get_action_returns_correct_action() {
        let dir = TempDir::new().unwrap();
        let kv = KvBackend::open(dir.path()).unwrap();
        let action = AgentAction {
            id: Uuid::new_v4(),
            action_type: ActionType::FileRead,
            file_path: "src/auth.rs".into(),
            entity_id: Some("src/auth.rs::login".into()),
            timestamp: Utc::now(),
            metadata: HashMap::new(),
        };
        let action_id = action.id.to_string();
        kv.set(&action_key(&action_id), &action).unwrap();

        let history = History::new(kv);
        let retrieved = history.get_action(&action_id).unwrap().unwrap();
        assert_eq!(retrieved.file_path, "src/auth.rs");
        assert_eq!(retrieved.entity_id.as_deref(), Some("src/auth.rs::login"));
    }

    #[test]
    fn get_action_returns_none_for_unknown_id() {
        let dir = TempDir::new().unwrap();
        let history = History::new(KvBackend::open(dir.path()).unwrap());
        assert!(history
            .get_action("00000000-0000-0000-0000-000000000000")
            .unwrap()
            .is_none());
    }

    // ── file_last_accessed ────────────────────────────────────────────────────

    #[test]
    fn file_last_accessed_returns_none_for_unaccessed_file() {
        let (history, _dir) = seeded_history();
        assert!(history
            .file_last_accessed("never_touched.rs")
            .unwrap()
            .is_none());
    }

    #[test]
    fn file_last_accessed_returns_some_for_accessed_file() {
        let (history, _dir) = seeded_history();
        // file_a.rs was accessed at t1, t3, t5 -must find a timestamp
        assert!(history.file_last_accessed("file_a.rs").unwrap().is_some());
    }

    #[test]
    fn file_last_accessed_returns_most_recent_of_multiple_accesses() {
        let (history, _dir) = seeded_history();
        // file_a.rs accesses: t1 (-500s), t3 (-300s), t5 (-100s)
        // Must return the timestamp closest to now (≈ t5)
        let now = Utc::now();
        let ts = history.file_last_accessed("file_a.rs").unwrap().unwrap();
        let elapsed = (now - ts).num_seconds();
        // t5 was now - 100s; allow ±30s for test timing jitter
        assert!(
            elapsed >= 70 && elapsed <= 130,
            "expected ~100s elapsed for file_a.rs, got {elapsed}s"
        );
    }

    // ── file_access_count ─────────────────────────────────────────────────────

    #[test]
    fn file_access_count_zero_for_never_accessed() {
        let (history, _dir) = seeded_history();
        assert_eq!(history.file_access_count("never_touched.rs").unwrap(), 0);
    }

    #[test]
    fn file_access_count_correct_for_accessed_file() {
        let (history, _dir) = seeded_history();
        assert_eq!(history.file_access_count("file_a.rs").unwrap(), 3);
        assert_eq!(history.file_access_count("file_b.rs").unwrap(), 1);
    }

    // ── clear ────────────────────────────────────────────────────────────────

    #[test]
    fn clear_empties_all_history() {
        let (history, _dir) = seeded_history();
        history.clear().unwrap();
        assert!(history.recent_files(10).unwrap().is_empty());
        assert!(history.frequent_files(10).unwrap().is_empty());
        assert!(history.recent_errors(10).unwrap().is_empty());
        assert_eq!(history.total_actions().unwrap(), 0);
    }

    #[test]
    fn clear_then_recent_files_empty() {
        let (history, _dir) = seeded_history();
        history.clear().unwrap();
        assert!(history.recent_files(10).unwrap().is_empty());
    }

    #[test]
    fn clear_then_total_actions_zero() {
        let (history, _dir) = seeded_history();
        history.clear().unwrap();
        assert_eq!(history.total_actions().unwrap(), 0);
    }
}
