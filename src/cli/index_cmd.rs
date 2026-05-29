use clap::Args;
use crate::error::Result;
use crate::indexer::CodeIndexer;

#[derive(Args)]
pub struct IndexCommand {
    /// Path to the repository to index
    pub path: String,

    /// Programming language (python, javascript, auto)
    #[arg(long, default_value = "auto")]
    pub language: String,

    /// Comma-separated exclude patterns
    #[arg(long, value_delimiter = ',')]
    pub exclude: Option<Vec<String>>,

    /// Output index file
    #[arg(short, long, default_value = ".graphswarm/index.db")]
    pub output: String,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,
}

impl IndexCommand {
    pub async fn execute(&self) -> Result<()> {
        if self.verbose {
            println!("Language : {}", self.language);
            println!("Output  : {}", self.output);
            if let Some(excl) = &self.exclude {
                println!("Exclude : {:?}", excl);
            }
        }

        let indexer = CodeIndexer::new(&self.language)?;
        let graph = indexer.index_directory(&self.path)?;

        println!("📊 Index Complete");
        println!("├── Files     : {}", graph.files().len());
        println!("├── Entities  : {}", graph.entity_count());
        println!("└── Edges     : {}", graph.edge_count());

        // TODO: serialize graph to self.output in Part 2
        Ok(())
    }
}
