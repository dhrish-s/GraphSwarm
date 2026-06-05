use graphswarm::indexer::*;
use tempfile::tempdir;

#[test]
fn indexer_creates_empty_graph() {
    let dir = tempdir().unwrap();
    let indexer = CodeIndexer::new("python").unwrap();
    let graph = indexer.index_directory(dir.path(), &[]).unwrap();
    assert_eq!(graph.entity_count(), 0);
}
