use crate::indexer::CallGraph;
use crate::error::Result;
use super::RelevantFile;

/// Main query engine that combines graph + history to recommend files.
pub struct QueryEngine {
    graph: CallGraph,
}

impl QueryEngine {
    pub fn new(graph: CallGraph) -> Result<Self> {
        Ok(Self { graph })
    }

    /// Return the most relevant files for a task description.
    /// TODO: wire up scoring in Part 4.
    pub async fn query_relevant_files(
        &self,
        _task: &str,
        _current_file: Option<&str>,
        _top_k: usize,
    ) -> Result<Vec<RelevantFile>> {
        Ok(Vec::new())
    }

    pub fn get_dependents(&self, _file: &str) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    pub fn get_dependencies(&self, _file: &str) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    pub fn graph(&self) -> &CallGraph { &self.graph }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_query() {
        let engine = QueryEngine::new(CallGraph::new()).unwrap();
        let r = engine.query_relevant_files("anything", None, 5).await.unwrap();
        assert!(r.is_empty());
    }
}
