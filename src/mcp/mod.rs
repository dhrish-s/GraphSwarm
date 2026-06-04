//! MCP (Model Context Protocol) server for GraphSwarm.
//!
//! Exposes GraphSwarm's code graph to AI agents (Claude Code, Cursor, etc.)
//! via the MCP stdio protocol — JSON-RPC 2.0 over stdin/stdout.
//!
//! Five tools:
//!   query_graph    — natural language → ranked relevant files
//!   get_callers    — who calls this entity?
//!   get_callees    — what does this entity call?
//!   shortest_path  — call chain from A to B
//!   explain_entity — full entity details
//!
//! Usage:
//!   graphswarm server           # start MCP server on stdio
//!   graphswarm install          # write Claude Code skill file
//!   graphswarm export           # write graph.json + graph.html + GRAPH_REPORT.md

pub mod protocol;
pub mod server;
pub mod tools;

pub use server::McpServer;
pub use tools::{tool_definitions, GraphSwarmState};
pub use protocol::{McpRequest, McpResponse, McpErrorResponse, ContentBlock, ToolDefinition};
