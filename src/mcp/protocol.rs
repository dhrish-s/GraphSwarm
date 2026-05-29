use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRequest {
    pub tool: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl McpResponse {
    pub fn ok(result: serde_json::Value) -> Self {
        Self { success: true, result: Some(result), error: None }
    }
    pub fn err(msg: impl Into<String>) -> Self {
        Self { success: false, result: None, error: Some(msg.into()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_response() {
        let r = McpResponse::ok(serde_json::json!({"files": []}));
        assert!(r.success);
        assert!(r.error.is_none());
    }

    #[test]
    fn err_response() {
        let r = McpResponse::err("boom");
        assert!(!r.success);
        assert_eq!(r.error.as_deref(), Some("boom"));
    }

    #[test]
    fn request_deserialize() {
        let json = r#"{"tool":"query_context","input":{"task":"fix bug"}}"#;
        let req: McpRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.tool, "query_context");
    }
}
