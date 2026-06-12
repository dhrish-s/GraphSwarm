use crate::error::Result;
use crate::indexer::extractor::EntityType;
use crate::indexer::CodeIndexer;
use crate::storage::{GraphStore, KvBackend};
use crate::tracker::ActionLogger;
use clap::Args;
use serde_json;
use std::fs;
use std::path::Path;

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

        // Validate path
        let repo_path = Path::new(&self.path);
        if !repo_path.exists() {
            return Err(crate::error::Error::index(format!(
                "Path does not exist: {}",
                self.path
            )));
        }
        if !repo_path.is_dir() {
            return Err(crate::error::Error::index(format!(
                "Path is not a directory: {}",
                self.path
            )));
        }

        let indexer = CodeIndexer::new(&self.language)?;
        let exclude_patterns = self.exclude.clone().unwrap_or_default();
        let graph = indexer.index_directory(&self.path, &exclude_patterns)?;

        // Handle empty repo
        if graph.files().is_empty() {
            return Err(crate::error::Error::index(
                "Empty repository or no readable files found",
            ));
        }

        // Handle unsupported files (files present but no entities)
        if graph.entity_count() == 0 {
            return Err(crate::error::Error::index(
                "No code entities found; repository may contain unsupported files",
            ));
        }

        // Print summary
        let langs: Vec<String> = graph
            .metadata
            .languages
            .iter()
            .map(|l| format!("{}", l))
            .collect();

        let test_count = graph
            .entities
            .values()
            .filter(|e| e.entity_type == EntityType::TestFunction)
            .count();

        println!("Indexed repository: {}", self.path);
        println!("Files: {}", graph.files().len());
        println!("Entities: {}", graph.entity_count());
        println!("Call Edges: {}", graph.edge_count());
        println!("Tests: {}", test_count);
        println!("Languages: {}", langs.join(", "));

        // Ensure output dir (co-located with the indexed repo, not the
        // invocation cwd -keeps this deterministic under parallel tests
        // and predictable when graphswarm is invoked from another directory).
        let out_dir = repo_path.join("graphswarm_output");
        fs::create_dir_all(&out_dir)?;
        let out_file = out_dir.join("graph.json");

        let f = fs::File::create(&out_file).map_err(|e| {
            crate::error::Error::index(format!("Failed to create output file: {}", e))
        })?;
        serde_json::to_writer_pretty(f, &graph)
            .map_err(|e| crate::error::Error::index(format!("Failed to serialize graph: {}", e)))?;

        // Also write to provided output path (best-effort)
        if !self.output.is_empty() {
            if let Some(parent) = Path::new(&self.output).parent() {
                fs::create_dir_all(parent).ok();
            }
            if let Ok(f2) = fs::File::create(&self.output) {
                let _ = serde_json::to_writer_pretty(f2, &graph);
            }
        }

        println!("Wrote graph to: {}", out_file.to_string_lossy());

        // Persist the graph to the KV store so `graphswarm query` can read
        // it without re-indexing.  The store lives at <repo>/.graphswarm/db/.
        let db_path = repo_path.join(".graphswarm").join("db");
        let kv = KvBackend::open(&db_path)?;
        let store = GraphStore::new(kv.clone());
        store.store_graph(&graph)?;
        println!("Graph persisted to: {}", db_path.display());

        // Start the action tracker background task.
        // ActionLogger::new() spawns a Tokio task -it is zero-cost to start.
        // The task runs until the process exits, logging every agent action
        // to the same sled database that holds the call graph.
        let logger = ActionLogger::new(kv);
        // Log the index operation itself as the first tracked action.
        // .ok() -a tracker failure must never crash the indexer.
        logger.log_file_read(&self.path).await.ok();
        println!("Action tracker started.");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[tokio::test]
    async fn successful_indexing_creates_graph() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("a.py");
        let mut f = std::fs::File::create(&p).unwrap();
        writeln!(f, "def foo():\n    pass\n").unwrap();

        let cmd = IndexCommand {
            path: dir.path().to_string_lossy().to_string(),
            language: "auto".into(),
            exclude: None,
            output: "graphswarm_output/test.json".into(),
            verbose: false,
        };

        let res = cmd.execute().await;
        assert!(res.is_ok());
        let out_file = dir.path().join("graphswarm_output").join("graph.json");
        let out = std::fs::read_to_string(out_file).unwrap();
        assert!(out.contains("Entities") || !out.is_empty());
    }

    #[tokio::test]
    async fn invalid_path_returns_error() {
        let cmd = IndexCommand {
            path: "nonexistent_path_xyz".into(),
            language: "auto".into(),
            exclude: None,
            output: "".into(),
            verbose: false,
        };

        let res = cmd.execute().await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn empty_repo_returns_error() {
        let dir = tempdir().unwrap();
        let cmd = IndexCommand {
            path: dir.path().to_string_lossy().to_string(),
            language: "auto".into(),
            exclude: None,
            output: "".into(),
            verbose: false,
        };

        let res = cmd.execute().await;
        assert!(res.is_err());
    }
}
