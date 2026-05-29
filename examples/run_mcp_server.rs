//! Example: start the MCP server.
//!
//! Usage: cargo run --example run_mcp_server

use graphswarm::mcp::McpServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = McpServer::new(3000);
    server.start().await?;
    Ok(())
}
