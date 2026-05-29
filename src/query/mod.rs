pub mod relevance;
pub mod ranker;
pub mod api;

pub use api::QueryEngine;
pub use ranker::RankedResult;
pub use relevance::RelevanceScorer;

#[derive(Debug, Clone)]
pub struct RelevantFile {
    pub file: String,
    pub relevance_score: f32,
    pub reason: String,
    pub dependencies: Vec<String>,
    pub dependents: Vec<String>,
    pub suggested_functions: Vec<String>,
}

impl RelevantFile {
    pub fn new(file: String, score: f32, reason: String) -> Self {
        Self {
            file, relevance_score: score, reason,
            dependencies: Vec::new(), dependents: Vec::new(),
            suggested_functions: Vec::new(),
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
    }
}
