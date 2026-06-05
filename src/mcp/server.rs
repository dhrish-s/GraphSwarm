//! MCP stdio server for GraphSwarm.
//!
//! The server runs a simple read-process-write loop:
//!
//!   1. Read one line from stdin (one JSON-RPC message)
//!   2. Parse as McpRequest
//!   3. Dispatch to handler (initialize, tools/list, tools/call)
//!   4. Serialize response as JSON
//!   5. Write one line to stdout + flush immediately
//!   6. Repeat until stdin closes
//!
//! Why line-delimited JSON?
//!   MCP clients send exactly one JSON object per line. Reading a full
//!   line before parsing guarantees a complete JSON object each time.
//!   Simpler and more reliable than a streaming JSON parser.
//!
//! Why flush after every write?
//!   stdout is line-buffered by default in Rust. Without an explicit
//!   flush the MCP client may block indefinitely waiting for bytes that
//!   are sitting in our buffer. Always flush immediately.
//!
//! Thread safety: McpServer is single-threaded.
//! Tool calls are synchronous. The ActionLogger background task runs in
//! a Tokio runtime -the server itself uses blocking stdio I/O, which
//! is correct because MCP clients send one request at a time.

use crate::error::Result;
use crate::mcp::protocol::{McpErrorResponse, McpRequest, McpResponse};
use crate::mcp::tools::{dispatch, tool_definitions, GraphSwarmState};
use crate::query::QueryEngine;
use crate::storage::{GraphStore, KvBackend};
use crate::tracker::History;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

/// MCP stdio server.
///
/// Reads JSON-RPC requests from stdin, writes responses to stdout.
/// Runs until stdin is closed (MCP client exits).
pub struct McpServer {
    /// Path to the `.graphswarm_db` sled directory.
    db_path: PathBuf,
    /// HTTP port -unused for stdio transport, kept for the HTTP future (Part 7).
    pub port: u16,
}

impl McpServer {
    /// Creates a new McpServer that will load the graph from `db_path`.
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        Self {
            db_path: db_path.into(),
            port: 3000,
        }
    }

    /// Runs the MCP stdio server until stdin closes.
    ///
    /// Blocking call -loops forever until the client disconnects.
    pub fn run(&self) -> Result<()> {
        let state = self.open_state();

        let stdin = io::stdin();
        let stdout = io::stdout();
        let mut out = stdout.lock();

        eprintln!("[graphswarm] MCP server ready on stdio");

        for line in stdin.lock().lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue; // ignore blank lines between messages
            }

            let response_json = match serde_json::from_str::<McpRequest>(trimmed) {
                Err(e) => serde_json::to_string(&McpErrorResponse::new(
                    None,
                    -32700,
                    format!("Parse error: {e}"),
                ))
                .unwrap_or_default(),
                Ok(req) => {
                    // Notifications (no id) don't require a response in JSON-RPC 2.0.
                    // We still produce one here because some MCP hosts expect it;
                    // clients that follow the spec will simply ignore the null-id reply.
                    let val = self.handle_request(req, state.as_ref());
                    serde_json::to_string(&val).unwrap_or_default()
                }
            };

            if writeln!(out, "{response_json}").is_err() {
                break;
            }
            if out.flush().is_err() {
                break;
            }
        }

        eprintln!("[graphswarm] MCP server stopped");
        Ok(())
    }

    /// Handles a single MCP request and returns the JSON-serializable response.
    pub fn handle_request(
        &self,
        req: McpRequest,
        state: Option<&GraphSwarmState>,
    ) -> serde_json::Value {
        match req.method.as_str() {
            // ── Handshake ──────────────────────────────────────────────────────
            // Client says hello; we return our capabilities and protocol version.
            "initialize" => serde_json::to_value(McpResponse::initialize(req.id)).unwrap(),

            // ── Notification: client acknowledges init ─────────────────────────
            // JSON-RPC notifications have no id -clients ignore our reply.
            "notifications/initialized" => serde_json::to_value(McpResponse::empty(None)).unwrap(),

            // ── Tool discovery ─────────────────────────────────────────────────
            "tools/list" => {
                serde_json::to_value(McpResponse::tools_list(req.id, tool_definitions())).unwrap()
            }

            // ── Tool execution ─────────────────────────────────────────────────
            "tools/call" => {
                let params = match req.params.as_ref() {
                    Some(p) => p,
                    None => {
                        return serde_json::to_value(McpErrorResponse::invalid_params(
                            req.id,
                            "Missing params",
                        ))
                        .unwrap()
                    }
                };

                let tool_name = match params["name"].as_str() {
                    Some(n) => n,
                    None => {
                        return serde_json::to_value(McpErrorResponse::invalid_params(
                            req.id,
                            "Missing tool name",
                        ))
                        .unwrap()
                    }
                };

                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or(serde_json::json!({}));

                let state = match state {
                    Some(s) => s,
                    None => {
                        return serde_json::to_value(McpErrorResponse::not_indexed(req.id)).unwrap()
                    }
                };

                match dispatch(tool_name, &args, state) {
                    Ok(content) => {
                        serde_json::to_value(McpResponse::tool_result(req.id, content)).unwrap()
                    }
                    Err(e) => {
                        serde_json::to_value(McpErrorResponse::internal(req.id, e.to_string()))
                            .unwrap()
                    }
                }
            }

            // ── Unknown method ─────────────────────────────────────────────────
            method => {
                serde_json::to_value(McpErrorResponse::method_not_found(req.id, method)).unwrap()
            }
        }
    }

    /// Opens the sled database and builds `GraphSwarmState`.
    ///
    /// Returns `None` if the database doesn't exist (not indexed yet).
    /// The server continues running but tool calls return a "not indexed" error.
    fn open_state(&self) -> Option<GraphSwarmState> {
        if !self.db_path.exists() {
            eprintln!(
                "[graphswarm] Warning: database not found at {}. \
                 Run `graphswarm index <path>` first.",
                self.db_path.display()
            );
            return None;
        }

        let kv = KvBackend::open(&self.db_path).ok()?;
        let engine = QueryEngine::new(GraphStore::new(kv.clone()), History::new(kv));
        // GraphStore is accessible via engine.store() -no second clone needed.
        Some(GraphSwarmState { engine })
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

// Helper: build a minimal McpRequest for tests without going through serde.
#[cfg(test)]
fn make_req(id: serde_json::Value, method: &str, params: Option<serde_json::Value>) -> McpRequest {
    McpRequest {
        jsonrpc: "2.0".into(),
        id: Some(id),
        method: method.into(),
        params,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::{
        call_graph::CallGraph,
        extractor::{CodeEntity, EntityType, Language},
    };
    use crate::storage::KvBackend;
    use tempfile::TempDir;

    fn temp_server() -> (McpServer, TempDir) {
        let dir = TempDir::new().unwrap();
        let server = McpServer::new(dir.path().join(".graphswarm_db"));
        (server, dir)
    }

    fn make_test_graph() -> CallGraph {
        let main_e = CodeEntity {
            id: "src/main.rs::main".into(),
            name: "main".into(),
            entity_type: EntityType::Function,
            file_path: "src/main.rs".into(),
            line_start: 1,
            line_end: 10,
            language: Language::Rust,
            docstring: None,
            calls: vec!["src/auth.rs::authenticate_user".into()],
            called_by: vec![],
        };
        let auth_e = CodeEntity {
            id: "src/auth.rs::authenticate_user".into(),
            name: "authenticate_user".into(),
            entity_type: EntityType::Function,
            file_path: "src/auth.rs".into(),
            line_start: 5,
            line_end: 25,
            language: Language::Rust,
            docstring: None,
            calls: vec![],
            called_by: vec!["src/main.rs::main".into()],
        };
        let mut g = CallGraph::new();
        g.set_repo_path("./test".into());
        g.add_entity(main_e);
        g.add_entity(auth_e);
        g.add_call(
            "src/main.rs::main".into(),
            "src/auth.rs::authenticate_user".into(),
        );
        g
    }

    fn make_state_with_graph(dir: &TempDir) -> GraphSwarmState {
        let db_path = dir.path().join(".graphswarm_db");
        let kv = KvBackend::open(&db_path).unwrap();
        let store = GraphStore::new(kv.clone());
        store.store_graph(&make_test_graph()).unwrap();
        let engine = QueryEngine::new(store, History::new(kv));
        GraphSwarmState { engine }
    }

    // ── McpServer::new / port ─────────────────────────────────────────────────

    #[test]
    fn server_new_does_not_panic() {
        let (_server, _dir) = temp_server();
    }

    #[test]
    fn default_port_is_3000() {
        let (server, _dir) = temp_server();
        assert_eq!(server.port(), 3000);
    }

    // ── handle_request ────────────────────────────────────────────────────────

    #[test]
    fn initialize_returns_protocol_version() {
        let (server, _dir) = temp_server();
        let req = make_req(serde_json::json!(1), "initialize", None);
        let v = server.handle_request(req, None);
        assert_eq!(v["result"]["protocolVersion"], "2024-11-05");
    }

    #[test]
    fn tools_list_returns_five_tools() {
        let (server, _dir) = temp_server();
        let req = make_req(serde_json::json!(1), "tools/list", None);
        let v = server.handle_request(req, None);
        assert_eq!(v["result"]["tools"].as_array().unwrap().len(), 5);
    }

    #[test]
    fn unknown_method_returns_error_code_32601() {
        let (server, _dir) = temp_server();
        let req = make_req(serde_json::json!(1), "foo/bar", None);
        let v = server.handle_request(req, None);
        assert_eq!(v["error"]["code"], -32601);
    }

    #[test]
    fn tools_call_with_no_state_returns_32000() {
        let (server, _dir) = temp_server();
        let req = make_req(
            serde_json::json!(1),
            "tools/call",
            Some(serde_json::json!({
                "name": "query_graph",
                "arguments": {"query": "auth"}
            })),
        );
        let v = server.handle_request(req, None);
        assert_eq!(v["error"]["code"], -32000);
    }

    #[test]
    fn tools_call_missing_params_returns_32602() {
        let (server, _dir) = temp_server();
        let req = make_req(serde_json::json!(1), "tools/call", None);
        let v = server.handle_request(req, None);
        assert_eq!(v["error"]["code"], -32602);
    }

    #[test]
    fn tools_call_unknown_tool_returns_error() {
        let dir = TempDir::new().unwrap();
        let server = McpServer::new(dir.path().join(".graphswarm_db"));
        let state = make_state_with_graph(&dir);
        let req = make_req(
            serde_json::json!(1),
            "tools/call",
            Some(serde_json::json!({
                "name": "no_such_tool",
                "arguments": {}
            })),
        );
        let v = server.handle_request(req, Some(&state));
        assert!(
            v.get("error").is_some(),
            "unknown tool must produce an error response"
        );
    }

    #[test]
    fn notifications_initialized_returns_empty_result() {
        let (server, _dir) = temp_server();
        let req = McpRequest {
            jsonrpc: "2.0".into(),
            id: None,
            method: "notifications/initialized".into(),
            params: None,
        };
        let v = server.handle_request(req, None);
        // No error; result is the empty object
        assert!(v.get("error").is_none());
    }

    #[test]
    fn response_always_contains_jsonrpc_2_0() {
        let (server, _dir) = temp_server();
        for method in &["initialize", "tools/list", "unknown_method"] {
            let req = make_req(serde_json::json!(1), method, None);
            let v = server.handle_request(req, None);
            assert_eq!(v["jsonrpc"], "2.0", "method {method} missing jsonrpc field");
        }
    }

    #[test]
    fn tools_call_valid_query_returns_result() {
        let dir = TempDir::new().unwrap();
        let server = McpServer::new(dir.path().join(".graphswarm_db"));
        let state = make_state_with_graph(&dir);
        let req = make_req(
            serde_json::json!(1),
            "tools/call",
            Some(serde_json::json!({
                "name": "query_graph",
                "arguments": {"query": "authenticate", "top_k": 3}
            })),
        );
        let v = server.handle_request(req, Some(&state));
        // Should have result, not error
        assert!(
            v.get("result").is_some(),
            "valid query must return a result"
        );
    }
}
