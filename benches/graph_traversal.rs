// Benchmark: graph traversal operations
//
// Targets:
//   bench_bfs_depth3        < 100 ms
//   bench_reverse_bfs_depth3 < 100 ms
//   bench_find_callers       < 1 ms
//   bench_find_in_file       < 5 ms

use criterion::{criterion_group, criterion_main, Criterion};
use graphswarm::indexer::{
    call_graph::CallGraph,
    extractor::{CodeEntity, EntityType, Language},
};
use graphswarm::storage::{GraphStore, KvBackend};
use tempfile::TempDir;

/// Builds a 1 000-entity graph with branching factor ~5.
/// Returns TempDir (keeps DB alive) + store + root entity id.
fn setup_branching_graph() -> (TempDir, GraphStore, String) {
    let dir  = TempDir::new().unwrap();
    let kv   = KvBackend::open(dir.path()).unwrap();
    let store = GraphStore::new(kv);

    let mut graph = CallGraph::new();
    graph.set_repo_path("./bench".into());

    // Build a 5-ary tree: 1 root → 5 children → 25 grandchildren → 125 → 250 → 500 leaves
    let mut id_counter = 0usize;
    let mut make_id = || { let id = id_counter; id_counter += 1; id };

    fn build_tree(
        graph: &mut CallGraph,
        parent_id: Option<String>,
        depth: usize,
        branching: usize,
        counter: &mut usize,
    ) -> String {
        let id   = *counter;
        *counter += 1;
        let name = format!("func_{id}");
        let file = format!("src/mod_{}.rs", id % 20);
        let full_id = format!("{file}::{name}");

        graph.add_entity(CodeEntity {
            id: full_id.clone(), name,
            entity_type: EntityType::Function,
            file_path: file,
            line_start: 1, line_end: 5,
            language: Language::Rust,
            docstring: None, calls: vec![], called_by: vec![],
        });

        if let Some(pid) = parent_id {
            graph.add_call(pid, full_id.clone());
        }

        if depth > 0 {
            for _ in 0..branching {
                build_tree(graph, Some(full_id.clone()), depth - 1, branching, counter);
            }
        }

        full_id
    }

    let mut cnt = 0usize;
    let root = build_tree(&mut graph, None, 4, 5, &mut cnt);
    store.store_graph(&graph).unwrap();

    (dir, store, root)
}

fn bench_bfs_depth3(c: &mut Criterion) {
    let (_dir, store, root) = setup_branching_graph();

    c.bench_function("bfs_depth3", |b| {
        b.iter(|| {
            let _ = store.bfs(&root, 3).unwrap();
        });
    });
}

fn bench_reverse_bfs_depth3(c: &mut Criterion) {
    let (_dir, store, root) = setup_branching_graph();
    // Use a leaf (BFS from root to depth=4 returns leaves)
    let leaves = store.bfs(&root, 4).unwrap();
    let leaf = leaves.last().cloned().unwrap_or(root);

    c.bench_function("reverse_bfs_depth3", |b| {
        b.iter(|| {
            let _ = store.reverse_bfs(&leaf, 3).unwrap();
        });
    });
}

fn bench_find_callers(c: &mut Criterion) {
    let (_dir, store, root) = setup_branching_graph();
    // The root's children are its callees; pick one as the target
    let children = store.find_callees(&root).unwrap();
    let target = children.first().map(|e| e.id.clone()).unwrap_or(root);

    c.bench_function("find_callers", |b| {
        b.iter(|| {
            let _ = store.find_callers(&target).unwrap();
        });
    });
}

fn bench_find_in_file(c: &mut Criterion) {
    // Build a graph where src/mod_0.rs has ~50 entities
    let dir  = TempDir::new().unwrap();
    let kv   = KvBackend::open(dir.path()).unwrap();
    let store = GraphStore::new(kv);

    let mut graph = CallGraph::new();
    graph.set_repo_path(".".into());
    for i in 0..50usize {
        graph.add_entity(CodeEntity {
            id: format!("src/mod_0.rs::func_{i}"),
            name: format!("func_{i}"),
            entity_type: EntityType::Function,
            file_path: "src/mod_0.rs".into(),
            line_start: (i * 5 + 1) as u32,
            line_end:   (i * 5 + 4) as u32,
            language: Language::Rust,
            docstring: None, calls: vec![], called_by: vec![],
        });
    }
    store.store_graph(&graph).unwrap();

    c.bench_function("find_in_file_50_entities", |b| {
        b.iter(|| {
            let _ = store.find_in_file("src/mod_0.rs").unwrap();
        });
    });
}

criterion_group!(
    benches,
    bench_bfs_depth3,
    bench_reverse_bfs_depth3,
    bench_find_callers,
    bench_find_in_file,
);
criterion_main!(benches);
