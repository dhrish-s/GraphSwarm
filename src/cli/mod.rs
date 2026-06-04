pub mod assets;
pub mod export_cmd;
pub mod index_cmd;
pub mod install_cmd;
pub mod query_cmd;
pub mod server_cmd;

pub use export_cmd::ExportCommand;
pub use index_cmd::IndexCommand;
pub use install_cmd::InstallCommand;
pub use query_cmd::QueryCommand;
pub use server_cmd::ServerCommand;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "graphswarm")]
#[command(about = "Graph-aware memory for AI coding agents")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Index a repository into the call graph database
    Index(IndexCommand),
    /// Query the indexed graph (natural language or structured)
    Query(QueryCommand),
    /// Start the MCP stdio server for Claude Code / Cursor
    Server(ServerCommand),
    /// Export graph.json, graph.html, and GRAPH_REPORT.md
    Export(ExportCommand),
    /// Install skill files for Claude Code, Cursor, or Codex
    Install(InstallCommand),
}
