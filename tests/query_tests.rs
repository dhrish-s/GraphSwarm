use graphswarm::query::*;
use graphswarm::indexer::CallGraph;

#[tokio::test]
async fn query_empty_graph() {
    let engine = QueryEngine::new(CallGraph::new()).unwrap();
    let results = engine.query_relevant_files("anything", None, 5).await.unwrap();
    assert!(results.is_empty());
}
