//! Example: index a Flask repository.
//!
//! Usage: cargo run --example index_flask_repo -- /path/to/flask

use graphswarm::indexer::CodeIndexer;

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let indexer = CodeIndexer::new("python")?;
    let graph = indexer.index_directory(&path)?;
    println!("Indexed {} entities across {} files",
        graph.entity_count(), graph.files().len());
    Ok(())
}
