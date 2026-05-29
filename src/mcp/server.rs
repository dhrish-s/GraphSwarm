use crate::error::Result;

pub struct McpServer {
    port: u16,
}

impl McpServer {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    /// Start the MCP server. TODO: implement with axum in Part 5.
    pub async fn start(&self) -> Result<()> {
        println!("GraphSwarm MCP Server listening on http://localhost:{}", self.port);
        println!("Available tools: query_context, log_action, get_dependents, get_dependencies");
        // TODO: axum router + tool handlers
        Ok(())
    }

    pub fn port(&self) -> u16 { self.port }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_port() {
        let s = McpServer::new(3000);
        assert_eq!(s.port(), 3000);
    }
}
