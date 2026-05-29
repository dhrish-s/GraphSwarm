use criterion::{criterion_group, criterion_main, Criterion};

fn bench_index(_c: &mut Criterion) {
    // TODO: benchmark indexer on a real repo
}

criterion_group!(benches, bench_index);
criterion_main!(benches);
