use clap::Args;
use crate::error::Result;
use crate::mcp::McpServer;

#[derive(Args)]
pub struct ServerCommand {
    /// Server port
    #[arg(short, long, default_value = "3000")]
    pub port: u16,

    /// Path to index file
    #[arg(long, default_value = ".graphswarm/index.db")]
    pub index: String,

    /// Log level
    #[arg(long, default_value = "info")]
    pub log_level: String,
}

impl ServerCommand {
    pub async fn execute(&self) -> Result<()> {
        let server = McpServer::new(self.port);
        server.start().await
    }
}
