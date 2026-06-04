//! Example: query the index for relevant files.
//!
//! Run `graphswarm index .` first, then:
//!   cargo run --example query_api

use graphswarm::prelude::*;
use graphswarm::storage::{GraphStore, KvBackend};
use graphswarm::tracker::History;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db_path = std::path::Path::new(".graphswarm_db");
    let kv      = KvBackend::open(db_path)?;
    let store   = GraphStore::new(kv.clone());
    let history = History::new(kv);

    let engine  = QueryEngine::new(store, history);
    let results = engine.query("Fix payment timeout bug", 5)?;

    if results.is_empty() {
        println!("No results -run `graphswarm index .` first.");
    }
    for r in results {
        println!("{}: {:.3}  -{}", r.file_path, r.relevance_score, r.reason);
    }
    Ok(())
}
