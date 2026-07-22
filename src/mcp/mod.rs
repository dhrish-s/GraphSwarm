//! MCP (Model Context Protocol) server for GraphSwarm.
//!
//! Exposes GraphSwarm's code graph to AI agents (Claude Code, Cursor, etc.)
//! via the MCP stdio protocol -JSON-RPC 2.0 over stdin/stdout.
//!
//! Six tools:
//!   query_graph    -natural language → ranked relevant files
//!   get_callers    -who calls this entity?
//!   get_callees    -what does this entity call?
//!   shortest_path  -call chain from A to B
//!   explain_entity -full entity details
//!   find_tests     -list tests, or find tests covering an entity
//!
//! Usage:
//!   graphswarm server           # start MCP server on stdio
//!   graphswarm install          # write Claude Code skill file
//!   graphswarm export           # write graph.json + graph.html + GRAPH_REPORT.md

pub mod protocol;
pub mod server;
pub mod tools;

pub use protocol::{ContentBlock, McpErrorResponse, McpRequest, McpResponse, ToolDefinition};
pub use server::McpServer;
pub use tools::{tool_definitions, GraphSwarmState};
