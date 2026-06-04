//! MCP (Model Context Protocol) JSON-RPC 2.0 types.
//!
//! MCP uses JSON-RPC 2.0 over stdio. Every request has:
//!   jsonrpc: "2.0"  (always)
//!   id:      number | string | null
//!   method:  string
//!   params:  optional object
//!
//! Every response has either `result` or `error`, never both.
//!
//! Why stdio instead of HTTP?
//!   - No port conflicts, no firewall issues
//!   - Claude Code spawns the MCP server as a subprocess
//!   - When Claude Code exits, the server exits automatically
//!   - Zero configuration — no IP, no port, no auth

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An incoming JSON-RPC 2.0 request from the MCP client.
#[derive(Debug, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    /// Can be a number, string, or null. Null id = notification (no response expected).
    pub id: Option<Value>,
    pub method: String,
    /// Tool-specific parameters; absent for methods like initialize.
    pub params: Option<Value>,
}

/// A successful JSON-RPC 2.0 response.
#[derive(Debug, Serialize)]
pub struct McpResponse {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub result: Value,
}

/// A failed JSON-RPC 2.0 response.
#[derive(Debug, Serialize)]
pub struct McpErrorResponse {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub error: McpError,
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Serialize)]
pub struct McpError {
    pub code: i32,
    pub message: String,
}

/// Standard JSON-RPC 2.0 error codes, plus our custom ones.
pub mod error_codes {
    /// The method does not exist or is not available.
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Invalid method parameter(s).
    pub const INVALID_PARAMS: i32 = -32602;
    /// Internal JSON-RPC error.
    pub const INTERNAL_ERROR: i32 = -32603;
    /// Server-defined: graph not indexed yet.
    pub const NOT_INDEXED: i32 = -32000;
}

/// A single content block in a tool result.
/// MCP tools return an array of content blocks.
/// We only use "text" — GraphSwarm doesn't produce images or other media.
#[derive(Debug, Serialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

/// Tool definition for the `tools/list` response.
#[derive(Debug, Serialize, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

impl McpResponse {
    /// Creates a successful response wrapping tool content blocks.
    pub fn tool_result(id: Option<Value>, content: Vec<ContentBlock>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: serde_json::json!({ "content": content }),
        }
    }

    /// Creates the `initialize` response with server capabilities.
    pub fn initialize(id: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "graphswarm",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        }
    }

    /// Creates the `tools/list` response.
    pub fn tools_list(id: Option<Value>, tools: Vec<ToolDefinition>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: serde_json::json!({ "tools": tools }),
        }
    }

    /// Creates an empty result (used for notifications that have no response body).
    pub fn empty(id: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: serde_json::json!({}),
        }
    }
}

impl McpErrorResponse {
    pub fn new(id: Option<Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            error: McpError { code, message: message.into() },
        }
    }

    pub fn method_not_found(id: Option<Value>, method: &str) -> Self {
        Self::new(id, error_codes::METHOD_NOT_FOUND, format!("Method not found: {method}"))
    }

    pub fn invalid_params(id: Option<Value>, detail: impl Into<String>) -> Self {
        Self::new(id, error_codes::INVALID_PARAMS, detail)
    }

    pub fn not_indexed(id: Option<Value>) -> Self {
        Self::new(
            id,
            error_codes::NOT_INDEXED,
            "Graph not indexed. Run `graphswarm index <path>` first.",
        )
    }

    pub fn internal(id: Option<Value>, detail: impl Into<String>) -> Self {
        Self::new(id, error_codes::INTERNAL_ERROR, detail)
    }
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        Self { content_type: "text".into(), text: text.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_contains_protocol_version() {
        let r = McpResponse::initialize(Some(serde_json::json!(1)));
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["result"]["protocolVersion"], "2024-11-05");
    }

    #[test]
    fn initialize_contains_server_name() {
        let r = McpResponse::initialize(Some(serde_json::json!(1)));
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["result"]["serverInfo"]["name"], "graphswarm");
    }

    #[test]
    fn tool_result_content_type_is_text() {
        let block = ContentBlock::text("hello");
        let r = McpResponse::tool_result(Some(serde_json::json!(1)), vec![block]);
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["result"]["content"][0]["type"], "text");
    }

    #[test]
    fn error_method_not_found_code() {
        let e = McpErrorResponse::method_not_found(None, "foo");
        assert_eq!(e.error.code, -32601);
    }

    #[test]
    fn error_not_indexed_code() {
        let e = McpErrorResponse::not_indexed(None);
        assert_eq!(e.error.code, -32000);
    }

    #[test]
    fn error_invalid_params_code() {
        let e = McpErrorResponse::invalid_params(None, "bad args");
        assert_eq!(e.error.code, -32602);
    }

    #[test]
    fn request_deserializes_from_json() {
        let json = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"query_graph","arguments":{"query":"auth"}}}"#;
        let req: McpRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.method, "tools/call");
        assert_eq!(req.jsonrpc, "2.0");
    }

    #[test]
    fn response_serializes_to_valid_json() {
        let r = McpResponse::initialize(Some(serde_json::json!(42)));
        let s = serde_json::to_string(&r).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
    }

    #[test]
    fn id_can_be_number_string_or_null() {
        let r1 = McpResponse::empty(Some(serde_json::json!(1)));
        let r2 = McpResponse::empty(Some(serde_json::json!("abc")));
        let r3 = McpResponse::empty(None);

        assert_eq!(serde_json::to_value(&r1).unwrap()["id"], 1);
        assert_eq!(serde_json::to_value(&r2).unwrap()["id"], "abc");
        assert!(serde_json::to_value(&r3).unwrap()["id"].is_null());
    }

    #[test]
    fn content_block_text_sets_type() {
        let b = ContentBlock::text("hello");
        assert_eq!(b.content_type, "text");
        assert_eq!(b.text, "hello");
    }
}
