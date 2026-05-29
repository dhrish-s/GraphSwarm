pub mod parser;
pub mod call_graph;
pub mod extractor;

pub use parser::CodeParser;
pub use call_graph::CallGraph;
pub use extractor::{CodeEntity, EntityType};

use crate::error::Result;
use std::path::Path;

/// Top-level indexer that parses a repo and produces a CallGraph.
pub struct CodeIndexer {
    parser: CodeParser,
}

impl CodeIndexer {
    pub fn new(language: &str) -> Result<Self> {
        Ok(Self { parser: CodeParser::new(language)? })
    }

    /// Index every source file under `path` and build a call graph.
    /// TODO: walk directory, call parser, wire up edges in Part 1.
    pub fn index_directory(&self, _path: impl AsRef<Path>) -> Result<CallGraph> {
        Ok(CallGraph::new())
    }

    pub fn language(&self) -> &str {
        self.parser.language()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_indexer() {
        assert!(CodeIndexer::new("python").is_ok());
        assert!(CodeIndexer::new("cobol").is_err());
    }
}
