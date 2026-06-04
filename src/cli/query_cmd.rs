use clap::Args;
use std::path::PathBuf;
use crate::error::{Error, Result};
use crate::query::QueryEngine;
use crate::storage::{GraphStore, KvBackend};
use crate::tracker::History;

#[derive(Args)]
pub struct QueryCommand {
    /// Query expression, e.g. "authenticate" or "callers src/auth.rs::verify_token"
    pub query: Vec<String>,

    /// Path to index file or repository root
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
        let repo_root = Self::resolve_repo_root(&self.index)?;

        // Open the existing KV store -no re-indexing.
        // Run `graphswarm index <path>` first to populate the store.
        let db_path = repo_root.join(".graphswarm_db");
        let kv    = KvBackend::open(&db_path)?;
        let store = GraphStore::new(kv.clone());
        let history = History::new(kv);

        let tokens: Vec<&str> = self.query.iter().map(String::as_str).collect();
        if tokens.is_empty() {
            return Err(Error::query(
                "Missing query. Try: callers, callees, file, entity, bfs, or a natural-language phrase.",
            ));
        }

        match tokens.as_slice() {
            // ── Structured graph queries ──────────────────────────────────────
            ["callers", entity_id]  => self.print_callers(entity_id, &store),
            ["callees", entity_id]  => self.print_callees(entity_id, &store),
            ["file", file_path]     => self.print_file_entities(file_path, &store),
            ["entity", name]        => self.print_entity_by_name(name, &store),
            ["bfs",  entity_id]     => self.print_bfs(entity_id, 3, &store),
            ["bfs",  entity_id, depth]
                => self.print_bfs(entity_id, Self::parse_depth(depth)?, &store),
            ["reverse_bfs", entity_id]
                => self.print_reverse_bfs(entity_id, 3, &store),
            ["reverse_bfs", entity_id, depth]
                => self.print_reverse_bfs(entity_id, Self::parse_depth(depth)?, &store),
            ["dependency_chain", entity_id]
                => self.print_dependency_chain(entity_id, 3, &store),
            ["dependency_chain", entity_id, depth]
                => self.print_dependency_chain(entity_id, Self::parse_depth(depth)?, &store),

            // ── Natural language query via QueryEngine ────────────────────────
            _ => {
                let q = self.query.join(" ");
                let engine = QueryEngine::new(store, history);
                let results = engine.query(&q, self.top_k)?;

                if results.is_empty() {
                    println!("No results for: \"{}\"", q);
                    println!("Tip: run `graphswarm index <path>` first to populate the graph.");
                    return Ok(());
                }

                for result in &results {
                    println!(
                        "{:.3}  {}  -{}",
                        result.relevance_score, result.file_path, result.reason
                    );
                    for entity in &result.entities {
                        println!("       {}  ({})", entity.name, entity.entity_type);
                    }
                }
                Ok(())
            }
        }
    }

    fn resolve_repo_root(index_path: &str) -> Result<PathBuf> {
        let path = PathBuf::from(index_path);
        if path.is_dir() {
            return Ok(path);
        }

        if let Some(parent) = path.parent() {
            if parent.file_name().and_then(|n| n.to_str()) == Some(".graphswarm") {
                if let Some(repo_root) = parent.parent() {
                    return Ok(repo_root.to_path_buf());
                }
            }
            if parent.exists() {
                return Ok(parent.to_path_buf());
            }
        }

        std::env::current_dir().map_err(Error::from)
    }

    fn parse_depth(depth: &str) -> Result<usize> {
        depth.parse::<usize>().map_err(|_| {
            Error::query(format!("Invalid depth '{}'. Must be a positive integer.", depth))
        })
    }

    fn print_entity_header(&self, entity_id: &str) {
        println!("Entity:\n{}\n", entity_id);
    }

    fn print_list(&self, heading: &str, items: &[String]) {
        println!("{}:\n", heading);
        if items.is_empty() {
            println!("  (none)\n");
            return;
        }
        for item in items {
            println!("- {}", item);
        }
        println!();
    }

    fn print_callers(&self, entity_id: &str, store: &GraphStore) -> Result<()> {
        if store.entity_by_id(entity_id)?.is_none() {
            return Err(Error::not_found(format!("Entity not found: {}", entity_id)));
        }
        let callers: Vec<String> = store.find_callers(entity_id)?
            .into_iter().map(|e| e.id).collect();
        self.print_entity_header(entity_id);
        self.print_list("Callers", &callers);
        Ok(())
    }

    fn print_callees(&self, entity_id: &str, store: &GraphStore) -> Result<()> {
        if store.entity_by_id(entity_id)?.is_none() {
            return Err(Error::not_found(format!("Entity not found: {}", entity_id)));
        }
        let callees: Vec<String> = store.find_callees(entity_id)?
            .into_iter().map(|e| e.id).collect();
        self.print_entity_header(entity_id);
        self.print_list("Callees", &callees);
        Ok(())
    }

    fn print_file_entities(&self, file_path: &str, store: &GraphStore) -> Result<()> {
        let entities: Vec<String> = store.find_in_file(file_path)?
            .into_iter().map(|e| e.id).collect();
        println!("File path: {}\n", file_path);
        self.print_list("Entities", &entities);
        Ok(())
    }

    fn print_entity_by_name(&self, name: &str, store: &GraphStore) -> Result<()> {
        let matches = store.find_entity_by_name(name)?;
        println!("Entity name: {}\n", name);
        self.print_list("Matching entities", &matches);
        Ok(())
    }

    fn print_bfs(&self, entity_id: &str, max_depth: usize, store: &GraphStore) -> Result<()> {
        let visited = store.bfs(entity_id, max_depth)?;
        println!("BFS starting from {} (max depth {})\n", entity_id, max_depth);
        self.print_list("Reachable entities", &visited);
        Ok(())
    }

    fn print_reverse_bfs(&self, entity_id: &str, max_depth: usize, store: &GraphStore) -> Result<()> {
        let visited = store.reverse_bfs(entity_id, max_depth)?;
        println!("Reverse BFS starting from {} (max depth {})\n", entity_id, max_depth);
        self.print_list("Caller graph", &visited);
        Ok(())
    }

    fn print_dependency_chain(&self, entity_id: &str, max_depth: usize, store: &GraphStore) -> Result<()> {
        let chain = store.dependency_chain(entity_id, max_depth)?;
        println!("Dependency chain from {} (max depth {})\n", entity_id, max_depth);
        self.print_list("Dependent entities", &chain);
        Ok(())
    }
}
