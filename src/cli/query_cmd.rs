use clap::Args;
use std::path::PathBuf;
use crate::error::{Error, Result};
use crate::indexer::CodeIndexer;
use crate::storage::{GraphQueries, KvBackend};

#[derive(Args)]
pub struct QueryCommand {
    /// Query expression, e.g. callers payment.py::process_payment
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
        let indexer = CodeIndexer::new("auto")?;
        let graph = indexer.index_directory(&repo_root)?;
        let kv = KvBackend::new();
        let query_engine = GraphQueries::new(&kv).with_graph(&graph);

        let tokens: Vec<&str> = self.query.iter().map(String::as_str).collect();
        if tokens.is_empty() {
            return Err(Error::query("Missing query. Use callers, callees, file, entity, bfs, reverse_bfs, or dependency_chain."));
        }

        match tokens.as_slice() {
            ["callers", entity_id] => self.print_callers(entity_id, &graph, &query_engine),
            ["callees", entity_id] => self.print_callees(entity_id, &graph, &query_engine),
            ["file", file_path] => self.print_file_entities(file_path, &query_engine),
            ["entity", name] => self.print_entity_by_name(name, &query_engine),
            ["bfs", entity_id] => self.print_bfs(entity_id, 3, &query_engine),
            ["bfs", entity_id, depth] => self.print_bfs(entity_id, Self::parse_depth(depth)?, &query_engine),
            ["reverse_bfs", entity_id] => self.print_reverse_bfs(entity_id, 3, &query_engine),
            ["reverse_bfs", entity_id, depth] => self.print_reverse_bfs(entity_id, Self::parse_depth(depth)?, &query_engine),
            ["dependency_chain", entity_id] => self.print_dependency_chain(entity_id, 3, &query_engine),
            ["dependency_chain", entity_id, depth] => self.print_dependency_chain(entity_id, Self::parse_depth(depth)?, &query_engine),
            _ => Err(Error::query(format!("Unsupported query: '{}'", self.query.join(" ")))),
        }
    }

    fn resolve_repo_root(index_path: &str) -> Result<PathBuf> {
        let path = PathBuf::from(index_path);
        if path.is_dir() {
            return Ok(path);
        }

        if let Some(parent) = path.parent() {
            if parent.file_name().and_then(|name| name.to_str()) == Some(".graphswarm") {
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
        depth.parse::<usize>().map_err(|_| Error::query(format!("Invalid depth '{}'. Must be a positive integer.", depth)))
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

    fn print_callers(&self, entity_id: &str, graph: &crate::indexer::call_graph::CallGraph, engine: &GraphQueries<'_>) -> Result<()> {
        if graph.get_entity(entity_id).is_none() {
            return Err(Error::not_found(format!("Entity not found: {}", entity_id)));
        }

        let callers = engine.find_callers(entity_id)?;
        self.print_entity_header(entity_id);
        self.print_list("Callers", &callers);
        Ok(())
    }

    fn print_callees(&self, entity_id: &str, graph: &crate::indexer::call_graph::CallGraph, engine: &GraphQueries<'_>) -> Result<()> {
        if graph.get_entity(entity_id).is_none() {
            return Err(Error::not_found(format!("Entity not found: {}", entity_id)));
        }

        let callees = engine.find_callees(entity_id)?;
        self.print_entity_header(entity_id);
        self.print_list("Callees", &callees);
        Ok(())
    }

    fn print_file_entities(&self, file_path: &str, engine: &GraphQueries<'_>) -> Result<()> {
        let entities = engine.find_entities_in_file(file_path)?;
        println!("File path: {}\n", file_path);
        self.print_list("Entities", &entities);
        Ok(())
    }

    fn print_entity_by_name(&self, name: &str, engine: &GraphQueries<'_>) -> Result<()> {
        let matches = engine.find_entity_by_name(name)?;
        println!("Entity name: {}\n", name);
        self.print_list("Matching entities", &matches);
        Ok(())
    }

    fn print_bfs(&self, entity_id: &str, max_depth: usize, engine: &GraphQueries<'_>) -> Result<()> {
        let visited = engine.bfs(entity_id, max_depth)?;
        println!("BFS starting from {} (max depth {})\n", entity_id, max_depth);
        self.print_list("Reachable entities", &visited);
        Ok(())
    }

    fn print_reverse_bfs(&self, entity_id: &str, max_depth: usize, engine: &GraphQueries<'_>) -> Result<()> {
        let visited = engine.reverse_bfs(entity_id, max_depth)?;
        println!("Reverse BFS starting from {} (max depth {})\n", entity_id, max_depth);
        self.print_list("Caller graph", &visited);
        Ok(())
    }

    fn print_dependency_chain(&self, entity_id: &str, max_depth: usize, engine: &GraphQueries<'_>) -> Result<()> {
        let chain = engine.dependency_chain(entity_id, max_depth)?;
        println!("Dependency chain from {} (max depth {})\n", entity_id, max_depth);
        self.print_list("Dependent entities", &chain);
        Ok(())
    }
}
