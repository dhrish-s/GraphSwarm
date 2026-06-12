//! MCP server for GraphSwarm -stdio and HTTP transports.
//!
//! ## stdio transport (`run`)
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
//!
//! ## HTTP transport (`run_http`)
//!
//! `run_http` exposes the same JSON-RPC 2.0 protocol over `POST /mcp`
//! (plus `GET /health`) using axum, binding to `127.0.0.1` only -
//! GraphSwarm has no authentication, so the HTTP transport is for local
//! tooling that can't speak stdio, not for exposing the graph on a
//! network. Both transports share `dispatch_request` for parsing and
//! dispatch, so a malformed request gets the same `-32700 Parse error`
//! response on either one.

use crate::error::{Error, Result};
use crate::mcp::protocol::{McpErrorResponse, McpRequest, McpResponse};
use crate::mcp::tools::{dispatch, tool_definitions, GraphSwarmState};
use crate::query::QueryEngine;
use crate::storage::{GraphStore, KvBackend};
use crate::tracker::History;
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;

/// MCP stdio server.
///
/// Reads JSON-RPC requests from stdin, writes responses to stdout.
/// Runs until stdin is closed (MCP client exits).
pub struct McpServer {
    /// Path to the `.graphswarm_db` sled directory.
    db_path: PathBuf,
    /// Default HTTP port for `run_http`, used when the CLI doesn't override it.
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

            // Notifications (no id) don't require a response in JSON-RPC 2.0.
            // We still produce one here because some MCP hosts expect it;
            // clients that follow the spec will simply ignore the null-id reply.
            let response_json =
                serde_json::to_string(&self.dispatch_request(trimmed, state.as_ref()))
                    .unwrap_or_default();

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

    /// Parses a raw JSON-RPC request string and dispatches it via `handle_request`.
    ///
    /// Shared by both transports: `run()` calls this once per stdin line, and
    /// `run_http()`'s `/mcp` handler calls this once per POST body. Centralizing
    /// the parse step here means malformed JSON gets the same `-32700 Parse
    /// error` JSON-RPC response regardless of transport.
    fn dispatch_request(&self, raw: &str, state: Option<&GraphSwarmState>) -> serde_json::Value {
        match serde_json::from_str::<McpRequest>(raw.trim()) {
            Err(e) => serde_json::to_value(McpErrorResponse::new(
                None,
                -32700,
                format!("Parse error: {e}"),
            ))
            .unwrap(),
            Ok(req) => self.handle_request(req, state),
        }
    }

    /// Runs the MCP server over HTTP, serving the same JSON-RPC 2.0 protocol
    /// as `run()` at `POST /mcp`, plus `GET /health` for liveness checks.
    ///
    /// Binds to `127.0.0.1:port` -localhost only. Blocking call -runs until
    /// the process is killed or the listener errors.
    pub async fn run_http(self, port: u16) -> Result<()> {
        let addr = format!("127.0.0.1:{port}");
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| Error::mcp(format!("Failed to bind to {addr}: {e}")))?;

        eprintln!("[graphswarm] MCP server ready on http://{addr}/mcp");
        self.serve(listener).await
    }

    /// Builds the axum router and serves it on `listener` until the process exits.
    ///
    /// Split out from `run_http` so tests can bind to an OS-assigned port
    /// (`127.0.0.1:0`), read the real address back with `local_addr()`, and
    /// hand the bound listener here.
    async fn serve(self, listener: tokio::net::TcpListener) -> Result<()> {
        let graph_state = self.open_state();
        let state: HttpState = Arc::new((self, graph_state));

        let app = Router::new()
            .route("/mcp", post(handle_mcp_post))
            .route("/health", get(|| async { "ok" }))
            .with_state(state);

        axum::serve(listener, app)
            .await
            .map_err(|e| Error::mcp(format!("HTTP server error: {e}")))
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

/// Shared state for the HTTP transport: the server (for `dispatch_request`)
/// plus the loaded graph state (`None` if the repo hasn't been indexed yet).
/// `Arc` lets axum clone the state per request without requiring
/// `GraphSwarmState: Clone`.
type HttpState = Arc<(McpServer, Option<GraphSwarmState>)>;

/// `POST /mcp` -handles one JSON-RPC 2.0 request, same protocol as stdio.
async fn handle_mcp_post(State(state): State<HttpState>, body: String) -> Json<serde_json::Value> {
    let (server, graph_state) = state.as_ref();
    Json(server.dispatch_request(&body, graph_state.as_ref()))
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
    fn tools_list_returns_six_tools() {
        let (server, _dir) = temp_server();
        let req = make_req(serde_json::json!(1), "tools/list", None);
        let v = server.handle_request(req, None);
        assert_eq!(v["result"]["tools"].as_array().unwrap().len(), 6);
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

    // ── dispatch_request ─────────────────────────────────────────────────────

    #[test]
    fn dispatch_request_parse_error_returns_32700() {
        let (server, _dir) = temp_server();
        let v = server.dispatch_request("not json", None);
        assert_eq!(v["error"]["code"], -32700);
    }

    #[test]
    fn dispatch_request_valid_request_dispatches_to_handle_request() {
        let (server, _dir) = temp_server();
        let raw = r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
        let v = server.dispatch_request(raw, None);
        assert_eq!(v["result"]["protocolVersion"], "2024-11-05");
    }

    // ── HTTP transport ────────────────────────────────────────────────────────

    /// Sends a raw HTTP/1.1 request over a fresh TCP connection and returns the
    /// full response text (status line, headers, and body).
    async fn http_request(addr: std::net::SocketAddr, request: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    }

    /// Extracts the body from a raw HTTP response (everything after the blank
    /// line that ends the headers).
    fn response_body(raw_response: &str) -> &str {
        raw_response.split("\r\n\r\n").nth(1).unwrap_or("")
    }

    /// Binds to an OS-assigned port, spawns `server.serve()` on it, and
    /// returns the address tests can connect to.
    async fn spawn_http_server(server: McpServer) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(server.serve(listener));
        addr
    }

    #[tokio::test]
    async fn run_http_serves_initialize_over_mcp_endpoint() {
        let dir = TempDir::new().unwrap();
        let server = McpServer::new(dir.path().join(".graphswarm_db"));
        let addr = spawn_http_server(server).await;

        let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
        let request = format!(
            "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );

        let response = http_request(addr, &request).await;
        let v: serde_json::Value = serde_json::from_str(response_body(&response)).unwrap();
        assert_eq!(v["result"]["protocolVersion"], "2024-11-05");
    }

    #[tokio::test]
    async fn run_http_tools_list_returns_six_tools() {
        let dir = TempDir::new().unwrap();
        let server = McpServer::new(dir.path().join(".graphswarm_db"));
        let addr = spawn_http_server(server).await;

        let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
        let request = format!(
            "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );

        let response = http_request(addr, &request).await;
        let v: serde_json::Value = serde_json::from_str(response_body(&response)).unwrap();
        assert_eq!(v["result"]["tools"].as_array().unwrap().len(), 6);
    }

    #[tokio::test]
    async fn run_http_health_endpoint_returns_ok() {
        let dir = TempDir::new().unwrap();
        let server = McpServer::new(dir.path().join(".graphswarm_db"));
        let addr = spawn_http_server(server).await;

        let request = "GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
        let response = http_request(addr, request).await;

        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response_body(&response).contains("ok"));
    }
}
