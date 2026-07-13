//! # GraphSwarm
//!
//! GraphSwarm builds a queryable call graph for a codebase and exposes it to
//! AI coding agents (Claude Code, Cursor, Codex, ...) through an embedded
//! key-value store and an MCP (Model Context Protocol) stdio server.
//!
//! ## Pipeline
//!
//! ```text
//! source files
//!     │  CodeIndexer::index_directory()
//!     ▼
//! CallGraph             (entities + call edges, in memory)
//!     │  GraphStore::store_graph()
//!     ▼
//! sled key-value store  (on disk, .graphswarm/db)
//!     │  QueryEngine::query("...", top_k)
//!     ▼
//! Vec<RelevantFile>     (ranked by relevance)
//! ```
//!
//! The same `GraphStore` also backs `McpServer`, which exposes the graph to
//! AI agents as MCP tools: `query_graph`, `get_callers`, `get_callees`,
//! `shortest_path`, `explain_entity`, and `find_tests`.
//!
//! ## Quick example
//!
//! ```
//! use graphswarm::prelude::*;
//!
//! # fn main() -> Result<()> {
//! let dir = tempfile::tempdir()?;
//! std::fs::write(
//!     dir.path().join("main.rs"),
//!     "fn main() { helper(); }\nfn helper() {}\n",
//! )?;
//!
//! // 1. Parse source files into a call graph.
//! let indexer = CodeIndexer::new("auto")?;
//! let graph = indexer.index_directory(dir.path(), &[])?;
//!
//! // 2. Persist the graph to an embedded KV store.
//! let kv = KvBackend::open(&dir.path().join("db"))?;
//! let store = GraphStore::new(kv.clone());
//! store.store_graph(&graph)?;
//!
//! // 3. Run a natural-language query over the indexed code.
//! let engine = QueryEngine::new(store, History::new(kv));
//! let results = engine.query("helper", 5)?;
//! assert!(!results.is_empty());
//! # Ok(())
//! # }
//! ```

/// Command-line interface: argument parsing and subcommand implementations
/// (`index`, `query`, `server`, `export`, `install`).
pub mod cli;
/// Crate-wide error type and `Result` alias.
pub mod error;
/// Source parsing and call-graph construction (tree-sitter based).
pub mod indexer;
/// MCP (Model Context Protocol) stdio server and tool implementations.
pub mod mcp;
/// Natural-language query engine over the indexed call graph.
pub mod query;
/// Embedded sled-backed key-value store and graph-aware queries.
pub mod storage;
/// Agent action logging and history (recency signal for queries).
pub mod tracker;
/// Shared configuration and logging setup.
pub mod utils;
/// File-system watcher and incremental graph reconciler.
pub mod watcher;

/// Re-exports for ergonomic library usage.
///
/// `use graphswarm::prelude::*;` brings in everything needed for the
/// canonical index -> store -> query pipeline shown in the crate-level
/// example above.
pub mod prelude {
    pub use crate::error::{Error, Result};
    /// Summary metadata about an indexed `CallGraph`: repo path, indexing
    /// time, and entity/file/language counts.
    pub use crate::indexer::call_graph::GraphMetadata;
    /// The programming language a parsed `CodeEntity` belongs to.
    pub use crate::indexer::extractor::Language;
    pub use crate::indexer::{CallGraph, CodeEntity, CodeIndexer, EntityType};
    pub use crate::mcp::McpServer;
    pub use crate::query::{QueryEngine, RelevantFile};
    pub use crate::storage::{GraphStore, KvBackend};
    pub use crate::tracker::{ActionTracker, History};
}
