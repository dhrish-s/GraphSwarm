use graphswarm::indexer::*;

#[test]
fn indexer_creates_empty_graph() {
    let indexer = CodeIndexer::new("python").unwrap();
    let graph = indexer.index_directory(".").unwrap();
    // Stub indexer returns empty graph
    assert_eq!(graph.entity_count(), 0);
}
