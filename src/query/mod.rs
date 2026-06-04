//! Query engine for GraphSwarm.
//!
//! Answers the question: "Given a natural language query, which files are most relevant?"
//!
//! Four signals combined with fixed weights:
//!   name match    (0.4) — does the entity name match the query tokens?
//!   graph distance (0.3) — is this entity near a name-matching entity?
//!   recency        (0.2) — did the agent touch this file recently?
//!   docstring      (0.1) — does the documentation mention the query?
//!
//! Entry point: [`QueryEngine::query(q, top_k)`] → [`Vec<RelevantFile>`]

pub mod api;
pub mod ranker;
pub mod relevance;

/// Internal type module — `RelevantFile` is shared by both `api.rs` and
/// `ranker.rs`. Defining it here as a sibling avoids any circular imports.
pub(crate) mod mod_types {
    use crate::indexer::extractor::CodeEntity;
    use serde::{Deserialize, Serialize};

    /// A file with its relevance score and the reason it was selected.
    ///
    /// This is the primary return type of `QueryEngine::query()`.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct RelevantFile {
        /// File path relative to repo root (e.g. `"src/auth.rs"`)
        pub file_path: String,
        /// Overall relevance score: 0.0 (irrelevant) to 1.0 (perfect match)
        pub relevance_score: f64,
        /// Human-readable explanation: "name match: authenticate_user"
        pub reason: String,
        /// Specific entities in this file that matched the query.
        pub entities: Vec<CodeEntity>,
        /// Present when this file has unreconciled on-disk changes.
        /// The watcher marks files stale until re-indexing completes.
        pub stale_warning: Option<String>,
    }
}

pub use api::QueryEngine;
pub use mod_types::RelevantFile;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relevant_file_fields_accessible() {
        let f = RelevantFile {
            file_path:       "x.py".into(),
            relevance_score: 0.8,
            reason:          "name match: foo".into(),
            entities:        Vec::new(),
            stale_warning:   None,
        };
        assert_eq!(f.relevance_score, 0.8);
        assert_eq!(f.file_path, "x.py");
        assert!(f.stale_warning.is_none());
    }

    #[test]
    fn relevant_file_stale_warning_field() {
        let f = RelevantFile {
            file_path:       "y.rs".into(),
            relevance_score: 0.5,
            reason:          "graph match".into(),
            entities:        Vec::new(),
            stale_warning:   Some("File has pending changes".into()),
        };
        assert!(f.stale_warning.is_some());
    }
}
