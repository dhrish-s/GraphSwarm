// Benchmark: ActionLogger throughput and History query latency
//
// Targets:
//   bench_action_log_throughput  < 1 μs per log() call (channel send, not disk)
//   bench_history_recent_files   < 5 ms

use chrono::{Duration, Utc};
use criterion::{criterion_group, criterion_main, Criterion};
use graphswarm::storage::schema::{action_key, history_count_key, history_recent_key};
use graphswarm::storage::KvBackend;
use graphswarm::tracker::action_log::{ActionType, AgentAction, FileAccessCount};
use graphswarm::tracker::{ActionLogger, History};
use std::collections::HashMap;
use tempfile::TempDir;
use uuid::Uuid;

fn bench_action_log_throughput(c: &mut Criterion) {
    // The logger uses an mpsc channel internally. Each log() is a channel send
    // which should be well under 1 μs. Disk I/O happens on the background task.
    let dir = TempDir::new().unwrap();
    let kv = KvBackend::open(dir.path()).unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    // ActionLogger::new calls tokio::spawn internally; must run inside the runtime.
    let logger = rt.block_on(async { ActionLogger::new(kv) });

    c.bench_function("action_log_file_read", |b| {
        b.iter(|| {
            rt.block_on(async {
                let _ = logger.log_file_read("src/auth.rs").await;
            });
        });
    });
}

fn bench_history_recent_files(c: &mut Criterion) {
    // Pre-seed a history with 500 entries across 50 unique files.
    let dir = TempDir::new().unwrap();
    let kv = KvBackend::open(dir.path()).unwrap();

    let now = Utc::now();
    for i in 0..500usize {
        let ts = now - Duration::seconds(i as i64);
        let file = format!("src/module_{}.rs", i % 50);
        let action = AgentAction {
            id: Uuid::new_v4(),
            action_type: ActionType::FileRead,
            file_path: file.clone(),
            entity_id: None,
            timestamp: ts,
            metadata: HashMap::new(),
        };
        kv.set(&action_key(&action.id.to_string()), &action)
            .unwrap();
        kv.set(
            &history_recent_key(&ts.to_rfc3339(), &action.id.to_string()),
            &file,
        )
        .unwrap();
    }

    // Per-file frequency counters
    for i in 0..50usize {
        let file = format!("src/module_{i}.rs");
        let fac = FileAccessCount {
            file_path: file.clone(),
            count: 10,
            last_accessed: now - Duration::seconds(i as i64),
        };
        kv.set(&history_count_key(&file), &fac).unwrap();
    }

    let history = History::new(kv);

    c.bench_function("history_recent_files_50", |b| {
        b.iter(|| {
            let _ = history.recent_files(50).unwrap();
        });
    });
}

criterion_group!(
    benches,
    bench_action_log_throughput,
    bench_history_recent_files
);
criterion_main!(benches);
