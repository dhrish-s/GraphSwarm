use graphswarm::query::QueryEngine;
use graphswarm::storage::{GraphStore, KvBackend};
use graphswarm::tracker::History;
use tempfile::TempDir;

#[test]
fn query_empty_graph_returns_empty() {
    let dir = TempDir::new().unwrap();
    let kv = KvBackend::open(dir.path()).unwrap();
    let store = GraphStore::new(kv.clone());
    let history = History::new(kv);
    let engine = QueryEngine::new(store, history);

    // No graph stored → no entities → empty results (not an error)
    let results = engine.query("anything", 5).unwrap();
    assert!(results.is_empty());
}
