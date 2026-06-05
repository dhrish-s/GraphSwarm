// Benchmark: indexing speed
//
// Targets:
//   bench_index_100_files  < 5 s
//   bench_index_1_file     < 100 ms

use criterion::{criterion_group, criterion_main, Criterion};
use graphswarm::indexer::CodeIndexer;
use std::io::Write;
use tempfile::TempDir;

/// Generates `n` synthetic Rust source files in `dir`.
/// Each file defines 10 functions with intra-file call edges.
fn generate_rust_files(dir: &TempDir, n: usize) {
    for i in 0..n {
        let path = dir.path().join(format!("module_{i}.rs"));
        let mut f = std::fs::File::create(&path).unwrap();
        for j in 0..10usize {
            // Each function calls the next one (circular except last)
            let calls = if j + 1 < 10 {
                format!("    func_{i}_{};", j + 1)
            } else {
                String::new()
            };
            writeln!(f, "fn func_{i}_{j}() {{\n{calls}\n}}").unwrap();
        }
    }
}

fn bench_index_100_files(c: &mut Criterion) {
    let dir = TempDir::new().unwrap();
    generate_rust_files(&dir, 100);
    let path = dir.path().to_path_buf();

    c.bench_function("index_100_files", |b| {
        b.iter(|| {
            let indexer = CodeIndexer::new("auto").unwrap();
            let _graph = indexer.index_directory(&path, &[]).unwrap();
        });
    });
}

fn bench_index_1_file(c: &mut Criterion) {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("single.rs");
    {
        let mut f = std::fs::File::create(&file).unwrap();
        for i in 0..50usize {
            writeln!(f, "fn func_{i}() {{}}").unwrap();
        }
    }
    let path = dir.path().to_path_buf();

    c.bench_function("index_1_file", |b| {
        b.iter(|| {
            let indexer = CodeIndexer::new("auto").unwrap();
            let _graph = indexer.index_directory(&path, &[]).unwrap();
        });
    });
}

criterion_group!(benches, bench_index_100_files, bench_index_1_file);
criterion_main!(benches);
