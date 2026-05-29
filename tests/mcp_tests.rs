use graphswarm::mcp::*;

#[test]
fn tool_definitions_valid() {
    let tools = McpTool::all();
    assert_eq!(tools.len(), 4);
    for tool in &tools {
        assert!(!tool.name.is_empty());
        assert!(!tool.description.is_empty());
    }
}

#[test]
fn protocol_round_trip() {
    let resp = McpResponse::ok(serde_json::json!({"status": "ok"}));
    let json = serde_json::to_string(&resp).unwrap();
    let back: McpResponse = serde_json::from_str(&json).unwrap();
    assert!(back.success);
}
