use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

impl McpTool {
    pub fn query_context() -> Self {
        Self {
            name: "query_context".into(),
            description: "Get relevant files for a task".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "What the agent is trying to do" },
                    "current_file": { "type": "string" },
                    "top_k": { "type": "integer", "default": 10 }
                },
                "required": ["task"]
            }),
        }
    }

    pub fn log_action() -> Self {
        Self {
            name: "log_action".into(),
            description: "Log what the agent just did".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "action_type": { "type": "string", "enum": ["FileRead","FileEdit","Error","TestRun"] },
                    "file": { "type": "string" },
                    "result": { "type": "string", "enum": ["PASS","FAIL","SKIP"] }
                },
                "required": ["action_type","file"]
            }),
        }
    }

    pub fn get_dependents() -> Self {
        Self {
            name: "get_dependents".into(),
            description: "What files depend on this file?".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "file": { "type": "string" } },
                "required": ["file"]
            }),
        }
    }

    pub fn get_dependencies() -> Self {
        Self {
            name: "get_dependencies".into(),
            description: "What files does this file depend on?".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "file": { "type": "string" } },
                "required": ["file"]
            }),
        }
    }

    pub fn all() -> Vec<Self> {
        vec![Self::query_context(), Self::log_action(), Self::get_dependents(), Self::get_dependencies()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_tools() {
        assert_eq!(McpTool::all().len(), 4);
    }

    #[test]
    fn tool_names() {
        let names: Vec<String> = McpTool::all().into_iter().map(|t| t.name).collect();
        assert!(names.contains(&"query_context".to_string()));
        assert!(names.contains(&"log_action".to_string()));
    }
}
