//! Example: start the MCP stdio server.
//!
//! Run `graphswarm index .` first, then:
//!   cargo run --example run_mcp_server

use graphswarm::mcp::McpServer;

fn main() -> anyhow::Result<()> {
    let server = McpServer::new(".graphswarm_db");
    server.run()?;
    Ok(())
}
