use graphswarm::mcp::{tool_definitions, McpResponse};

#[test]
fn tool_definitions_valid() {
    let tools = tool_definitions();
    assert_eq!(tools.len(), 6);
    for tool in &tools {
        assert!(!tool.name.is_empty());
        assert!(!tool.description.is_empty());
    }
}

#[test]
fn protocol_round_trip() {
    use graphswarm::mcp::ContentBlock;
    let block = ContentBlock::text("hello");
    let resp = McpResponse::tool_result(Some(serde_json::json!(1)), vec![block]);
    let json = serde_json::to_string(&resp).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["jsonrpc"], "2.0");
    assert_eq!(v["result"]["content"][0]["type"], "text");
}
