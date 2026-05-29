use criterion::{criterion_group, criterion_main, Criterion};

fn bench_bfs(c: &mut Criterion) {
    c.bench_function("bfs_empty_graph", |b| {
        let graph = graphswarm::indexer::CallGraph::new();
        b.iter(|| graph.bfs("nonexistent", 3));
    });
}

criterion_group!(benches, bench_bfs);
criterion_main!(benches);
