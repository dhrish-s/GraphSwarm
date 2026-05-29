use clap::Args;
use crate::error::Result;

#[derive(Args)]
pub struct QueryCommand {
    /// Task description to query
    pub query: String,

    /// Path to index file
    #[arg(long, default_value = ".graphswarm/index.db")]
    pub index: String,

    /// Number of results
    #[arg(short = 'k', long, default_value = "10")]
    pub top_k: usize,

    /// Output format: json, pretty, minimal
    #[arg(short, long, default_value = "pretty")]
    pub format: String,
}

impl QueryCommand {
    pub async fn execute(&self) -> Result<()> {
        println!("🔍 Querying: \"{}\"", self.query);
        println!("   Index : {}", self.index);
        println!("   Top-K : {}", self.top_k);
        println!("   Format: {}", self.format);

        // TODO: load graph from index, run query engine in Part 4
        println!("\n(query engine not yet implemented — see ROADMAP.md Part 4)");
        Ok(())
    }
}
