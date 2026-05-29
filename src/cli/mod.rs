pub mod index_cmd;
pub mod query_cmd;
pub mod server_cmd;

pub use index_cmd::IndexCommand;
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
    /// Index a repository
    Index(IndexCommand),
    /// Query the index for relevant files
    Query(QueryCommand),
    /// Start the MCP server
    Server(ServerCommand),
}
