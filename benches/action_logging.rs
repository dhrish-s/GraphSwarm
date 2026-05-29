use criterion::{criterion_group, criterion_main, Criterion};

fn bench_logging(c: &mut Criterion) {
    c.bench_function("log_action", |b| {
        let tracker = graphswarm::tracker::ActionTracker::new();
        b.iter(|| {
            tracker.log(graphswarm::tracker::AgentAction::FileRead {
                file: "bench.py".into(),
                timestamp: chrono::Utc::now(),
                context_window: 4096,
                reason: None,
            });
        });
    });
}

criterion_group!(benches, bench_logging);
criterion_main!(benches);
