pub mod relevance;
pub mod ranker;
pub mod api;

use serde::{Deserialize, Serialize};

pub use api::QueryEngine;
pub use ranker::RankedResult;
pub use relevance::RelevanceScorer;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelevantFile {
    pub file_path: String,
    pub relevance_score: f64,
    pub reason: String,
    pub entities: Vec<String>,
}

impl RelevantFile {
    pub fn new(file_path: String, score: f64, reason: String) -> Self {
        Self {
            file_path,
            relevance_score: score,
            reason,
            entities: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relevant_file_creation() {
        let r = RelevantFile::new("x.py".into(), 0.8, "match".into());
        assert_eq!(r.relevance_score, 0.8);
        assert_eq!(r.file_path, "x.py");
    }
}
