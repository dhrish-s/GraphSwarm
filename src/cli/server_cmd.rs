use clap::Args;
use crate::error::Result;
use crate::mcp::McpServer;
use std::path::PathBuf;

#[derive(Args)]
pub struct ServerCommand {
    /// Path to repository root (where .graphswarm_db lives)
    #[arg(long, default_value = ".")]
    pub path: String,

    /// Log level
    #[arg(long, default_value = "info")]
    pub log_level: String,
}

impl ServerCommand {
    pub async fn execute(&self) -> Result<()> {
        let db_path = PathBuf::from(&self.path).join(".graphswarm_db");
        let server  = McpServer::new(db_path);
        // run() blocks until stdin closes (MCP client exits)
        server.run()
    }
}
