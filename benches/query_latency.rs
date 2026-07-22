// Benchmark: query engine latency
//
// Targets:
//   bench_query_warm (p99) < 1 ms
//   bench_query_cold       document cold/warm difference
//
// Note: warm means the sled database is already open and cached in memory;
// cold means we reopen it per iteration (simulates a fresh process).

use criterion::{criterion_group, criterion_main, Criterion};
use graphswarm::indexer::{
    call_graph::CallGraph,
    extractor::{CodeEntity, EntityType, Language},
};
use graphswarm::query::QueryEngine;
use graphswarm::storage::{GraphStore, KvBackend};
use graphswarm::tracker::History;
use tempfile::TempDir;

/// Builds a 1 000-entity call graph seeded into a sled DB.
/// Returns the TempDir (keeps DB alive while the test runs).
fn setup_large_graph() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("bench_db");

    let kv = KvBackend::open(&db).unwrap();
    let store = GraphStore::new(kv);

    let mut graph = CallGraph::new();
    graph.set_repo_path("./bench".into());

    // 20 files × 50 entities each = 1 000 entities
    for file_idx in 0..20usize {
        let file = format!("src/module_{file_idx}.rs");
        for fn_idx in 0..50usize {
            let id = format!("{file}::func_{fn_idx}");
            let name = format!("func_{fn_idx}");
            let docstring = if fn_idx == 0 {
                Some(format!("Authenticates user in module {file_idx}"))
            } else {
                None
            };
            let calls = if fn_idx + 1 < 50 {
                vec![format!("{file}::func_{}", fn_idx + 1)]
            } else {
                vec![]
            };
            graph.add_entity(CodeEntity {
                id,
                name,
                entity_type: EntityType::Function,
                file_path: file.clone(),
                line_start: (fn_idx * 5 + 1) as u32,
                line_end: (fn_idx * 5 + 4) as u32,
                language: Language::Rust,
                docstring,
                calls: calls.clone(),
                called_by: vec![],
            });
            for callee in calls {
                graph.add_call(format!("{file}::func_{fn_idx}"), callee);
            }
        }
    }

    store.store_graph(&graph).unwrap();
    (dir, db)
}

fn bench_query_warm(c: &mut Criterion) {
    // DB opened once outside the measured loop -all reads hit the OS page cache.
    let (_dir, db) = setup_large_graph();
    let kv = KvBackend::open(&db).unwrap();
    let store = GraphStore::new(kv.clone());
    let history = History::new(kv);
    let engine = QueryEngine::new(store, history);

    c.bench_function("query_warm", |b| {
        b.iter(|| {
            let _ = engine.query("authenticate", 10).unwrap();
        });
    });
}

fn bench_query_cold(c: &mut Criterion) {
    // DB reopened each iteration -simulates a cold process start.
    // This is significantly slower than warm because sled must load B-tree pages.
    let (_dir, db) = setup_large_graph();

    c.bench_function("query_cold", |b| {
        b.iter(|| {
            let kv = KvBackend::open(&db).unwrap();
            let store = GraphStore::new(kv.clone());
            let history = History::new(kv);
            let engine = QueryEngine::new(store, history);
            let _ = engine.query("authenticate", 10).unwrap();
        });
    });
}

criterion_group!(benches, bench_query_warm, bench_query_cold);
criterion_main!(benches);
