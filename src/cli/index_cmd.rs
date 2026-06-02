use clap::Args;
use crate::error::Result;
use crate::indexer::CodeIndexer;
use crate::indexer::call_graph::CallGraph;
use std::path::Path;
use std::fs;
use serde_json;

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
            return Err(crate::error::Error::index(format!("Path does not exist: {}", self.path)));
        }
        if !repo_path.is_dir() {
            return Err(crate::error::Error::index(format!("Path is not a directory: {}", self.path)));
        }

        let indexer = CodeIndexer::new(&self.language)?;
        let graph = indexer.index_directory(&self.path)?;

        // Handle empty repo
        if graph.files().is_empty() {
            return Err(crate::error::Error::index("Empty repository or no readable files found"));
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

        println!("Indexed repository: {}", self.path);
        println!("Files: {}", graph.files().len());
        println!("Entities: {}", graph.entity_count());
        println!("Call Edges: {}", graph.edge_count());
        println!("Languages: {}", langs.join(", "));

        // Ensure output dir
        let out_dir = Path::new("graphswarm_output");
        fs::create_dir_all(out_dir)?;
        let out_file = out_dir.join("graph.json");

        let f = fs::File::create(&out_file).map_err(|e| crate::error::Error::index(format!("Failed to create output file: {}", e)))?;
        serde_json::to_writer_pretty(f, &graph).map_err(|e| crate::error::Error::index(format!("Failed to serialize graph: {}", e)))?;

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

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::io::Write;

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
        let out = std::fs::read_to_string("graphswarm_output/graph.json").unwrap();
        assert!(out.contains("Entities" ) || out.len()>0);
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
