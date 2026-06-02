//! Key-value schema for GraphSwarm's persistent storage layer.
//!
//! ALL keys in the system are constructed here — never as raw string literals
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
    let encoded = file_path.replace('/', "|").replace('\\', "|");
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
}
