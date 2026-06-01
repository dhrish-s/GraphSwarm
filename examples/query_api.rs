//! Example: query the index for relevant files.
//!
//! Usage: cargo run --example query_api

use graphswarm::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let indexer = CodeIndexer::new("python")?;
    let graph = indexer.index_directory(".")?;
    let engine = QueryEngine::new(graph)?;

    let results = engine.query_relevant_files(
        "Fix payment timeout bug", None, 5,
    ).await?;

    if results.is_empty() {
        println!("No results (query engine not yet implemented)");
    }
    for r in results {
        println!("{}: {:.2} - {}", r.file, r.relevance_score, r.reason);
    }
    Ok(())
}
