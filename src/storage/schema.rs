//! Key-value schema for GraphSwarm's persistent storage layer.
//!
//! ALL keys in the system are constructed here -never as raw string literals
//! elsewhere. This makes the key space a single source of truth: if a key
//! format changes, you change it here and nowhere else.
//!
//! Key layout:
//!
//!   entity:{entity_id}                 → JSON-encoded CodeEntity
//!   callers:{entity_id}                → JSON Vec<String> of entity ids that CALL this entity
//!   callees:{entity_id}                → JSON Vec<String> of entity ids this entity CALLS
//!   file:{file_path_encoded}:entities  → JSON Vec<String> of entity ids in this file
//!   edge:{caller_id}:{callee_id}       → "1" (existence marker, lets us answer "does A call B?" in O(1))
//!   meta:graph                         → JSON GraphMetadata (singleton)
//!   index:lang:{language}              → JSON Vec<String> of entity ids for that language
//!
//! File paths are encoded: "/" and "\" become "|" to avoid key ambiguity.

/// Key for a full CodeEntity record.
///
/// list_prefix("entity:") returns all stored entities.
pub fn entity_key(entity_id: &str) -> String {
    format!("entity:{}", entity_id)
}

/// Key for the list of entity ids that CALL the given entity (reverse edges).
///
/// Pre-computed at write time so that find_callers() is one read, not a graph scan.
pub fn callers_key(entity_id: &str) -> String {
    format!("callers:{}", entity_id)
}

/// Key for the list of entity ids that the given entity CALLS (forward edges).
pub fn callees_key(entity_id: &str) -> String {
    format!("callees:{}", entity_id)
}

/// Key for the list of entity ids that live in a given file.
///
/// File paths are encoded: slashes become "|" so "src/auth.rs" doesn't look
/// like a nested key hierarchy in the key space.
pub fn file_entities_key(file_path: &str) -> String {
    let encoded = file_path.replace(['/', '\\'], "|");
    format!("file:{}:entities", encoded)
}

/// Key for an edge existence marker.
///
/// edge_key("A", "B") answers "does A call B?" in one O(1) lookup.
/// This is directional: edge_key("A","B") != edge_key("B","A").
pub fn edge_key(caller_id: &str, callee_id: &str) -> String {
    format!("edge:{}:{}", caller_id, callee_id)
}

/// Singleton key for graph-level metadata (repo_path, indexed_at, entity/file counts).
pub fn meta_graph_key() -> &'static str {
    "meta:graph"
}

/// Key for the list of entity ids for a given language.
///
/// Uses the Language Display string ("rust", "python", etc.) lowercased.
pub fn lang_index_key(language: &str) -> String {
    format!("index:lang:{}", language.to_lowercase())
}

// ── Tracker schema ────────────────────────────────────────────────────────
//
// Time-series key design (used by history:recent: and history:error:):
//
//   sled stores keys in sorted lexicographic byte order.
//   RFC3339 timestamps ("2025-06-01T12:34:56Z") sort lexicographically
//   in chronological order, so prefixing a key with a timestamp lets us
//   retrieve records in time order from a single prefix scan -no sort step.
//
//   Key format:  "history:recent:{rfc3339}:{uuid}"
//   Example:
//     "history:recent:2025-06-01T10:00:00Z:uuid-a"  ← earlier (smaller key)
//     "history:recent:2025-06-01T11:00:00Z:uuid-b"  ← later   (larger key)
//
//   list_prefix("history:recent:") returns keys ascending (oldest first).
//   Reversing the result gives newest-first -the natural read order for
//   "what did the agent touch most recently?"
//
//   The UUID suffix guarantees uniqueness even if two actions share the
//   same millisecond timestamp.

/// Key for a full AgentAction record.
/// list_prefix("action:") returns all stored actions.
pub fn action_key(action_id: &str) -> String {
    format!("action:{}", action_id)
}

/// Time-ordered key for recent file access history.
///
/// Key format: "history:recent:{rfc3339}:{uuid}"
/// sled scans return keys in ascending byte order, which equals
/// ascending chronological order for RFC3339 timestamps.
/// Reverse the scan result to get newest-first.
pub fn history_recent_key(timestamp: &str, action_id: &str) -> String {
    format!("history:recent:{}:{}", timestamp, action_id)
}

/// Key for per-file access frequency counter.
///
/// File path separators (/ and \) are replaced with | so the path
/// cannot be confused with other key hierarchies in the store.
pub fn history_count_key(file_path: &str) -> String {
    let encoded = file_path.replace(['/', '\\'], "|");
    format!("history:count:{}", encoded)
}

// ── File watcher schema ───────────────────────────────────────────────────
//
// Stale markers: files that have on-disk changes not yet reconciled.
//   stale:{encoded_path}  → "1"  (presence = stale; deletion = clean)
//
// Reconciler heartbeat (last time the watcher loop ran successfully):
//   watcher:last_reconcile → RFC3339 timestamp string

/// Key marking a source file as having unreconciled on-disk changes.
///
/// Written by the Reconciler when a change is detected, cleared after
/// re-indexing succeeds. Query results include a warning while this is set.
pub fn stale_key(file_path: &str) -> String {
    let encoded = file_path.replace(['/', '\\'], "|");
    format!("stale:{encoded}")
}

/// Singleton key for the last successful reconciler pass timestamp.
pub fn watcher_last_reconcile_key() -> &'static str {
    "watcher:last_reconcile"
}

/// Time-ordered key for error actions only.
///
/// Mirrors history_recent_key but scoped to errors so recent_errors()
/// can scan only error records -no filtering needed on the read path.
pub fn history_error_key(timestamp: &str, action_id: &str) -> String {
    format!("history:error:{}:{}", timestamp, action_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_key_format() {
        assert_eq!(entity_key("src/auth.rs::login"), "entity:src/auth.rs::login");
        assert!(entity_key("x").starts_with("entity:"));
    }

    #[test]
    fn callers_callees_keys() {
        assert_eq!(callers_key("a::b"), "callers:a::b");
        assert_eq!(callees_key("a::b"), "callees:a::b");
    }

    #[test]
    fn file_entities_key_encodes_separators() {
        let k = file_entities_key("src/auth.rs");
        // Forward slash must be encoded so it doesn't create key hierarchy ambiguity
        assert!(!k.contains("src/auth"));
        assert!(k.contains("src|auth"));
        assert!(k.ends_with(":entities"));
    }

    #[test]
    fn file_entities_key_encodes_backslashes() {
        let k = file_entities_key("src\\auth.rs");
        assert!(!k.contains('\\'));
        assert!(k.contains('|'));
    }

    #[test]
    fn edge_key_is_directional() {
        let ab = edge_key("A", "B");
        let ba = edge_key("B", "A");
        assert_ne!(ab, ba);
        assert!(ab.starts_with("edge:"));
    }

    #[test]
    fn meta_graph_key_is_stable() {
        assert_eq!(meta_graph_key(), "meta:graph");
        assert_eq!(meta_graph_key(), meta_graph_key());
    }

    #[test]
    fn lang_index_key_lowercased() {
        assert_eq!(lang_index_key("Rust"), lang_index_key("rust"));
        assert_eq!(lang_index_key("PYTHON"), lang_index_key("python"));
    }

    // ── tracker key tests ─────────────────────────────────────────────────

    #[test]
    fn action_key_format() {
        let k = action_key("550e8400-e29b-41d4-a716-446655440000");
        assert!(k.starts_with("action:"));
        assert_eq!(k, "action:550e8400-e29b-41d4-a716-446655440000");
    }

    #[test]
    fn history_recent_key_format() {
        let k = history_recent_key("2025-06-01T12:00:00Z", "uuid-1");
        assert!(k.starts_with("history:recent:"));
        assert!(k.contains("2025-06-01T12:00:00Z"));
        assert!(k.ends_with(":uuid-1"));
    }

    #[test]
    fn history_recent_key_is_time_ordered() {
        // Earlier RFC3339 timestamp → lexicographically smaller key
        let earlier = history_recent_key("2025-06-01T10:00:00Z", "uuid-a");
        let later   = history_recent_key("2025-06-01T11:00:00Z", "uuid-b");
        assert!(earlier < later, "history:recent keys must sort chronologically");
    }

    #[test]
    fn history_count_key_encodes_separators() {
        let k1 = history_count_key("src/auth.rs");
        assert!(!k1.contains('/'), "forward slash must be encoded");
        assert!(k1.contains('|'));
        assert!(k1.starts_with("history:count:"));

        let k2 = history_count_key("src\\auth.rs");
        assert!(!k2.contains('\\'), "backslash must be encoded");
        assert!(k2.contains('|'));
    }

    #[test]
    fn stale_key_encodes_path() {
        let k = stale_key("src/auth.rs");
        assert!(k.starts_with("stale:"));
        assert!(!k.contains('/'));
        assert!(k.contains('|'));
    }

    #[test]
    fn watcher_last_reconcile_key_is_stable() {
        assert_eq!(watcher_last_reconcile_key(), "watcher:last_reconcile");
    }

    #[test]
    fn history_error_key_format() {
        let k = history_error_key("2025-06-01T12:00:00Z", "uuid-err");
        assert!(k.starts_with("history:error:"));
        assert!(k.contains("2025-06-01T12:00:00Z"));
        assert!(k.ends_with(":uuid-err"));
    }
}
