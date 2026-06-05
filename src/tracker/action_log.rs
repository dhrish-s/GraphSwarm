//! Core data types for the GraphSwarm action tracker.
//!
//! An AgentAction is the fundamental unit: one record per thing the
//! AI agent does. We log these to KV-SWARM so the query engine can
//! use access history to boost relevance scoring.
//!
//! Design principle: AgentAction is a plain data struct with no
//! behavior -just fields and serde. All logic lives in logger.rs
//! and history.rs. This keeps the type easy to test and serialize.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// The type of agent action being logged.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ActionType {
    /// Agent read a file (to understand context)
    FileRead,
    /// Agent edited/modified a file
    FileEdit,
    /// Agent invoked a specific function via MCP tool call
    FunctionCall,
    /// Agent ran tests
    TestRun,
    /// An operation failed with an error
    Error,
}

impl std::fmt::Display for ActionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionType::FileRead => write!(f, "file_read"),
            ActionType::FileEdit => write!(f, "file_edit"),
            ActionType::FunctionCall => write!(f, "function_call"),
            ActionType::TestRun => write!(f, "test_run"),
            ActionType::Error => write!(f, "error"),
        }
    }
}

/// A single action performed by an AI coding agent.
///
/// This is the fundamental unit of the tracker. Every file read,
/// every edit, every test run becomes one AgentAction in the log.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentAction {
    /// Unique identifier for this action (UUID v4).
    pub id: Uuid,
    /// What kind of action was performed.
    pub action_type: ActionType,
    /// The file path this action touched.
    /// Use forward slashes, relative to repo root.
    pub file_path: String,
    /// The specific entity (function/class) involved, if known.
    /// e.g. "src/auth.rs::authenticate_user"
    pub entity_id: Option<String>,
    /// When this action occurred (UTC).
    pub timestamp: DateTime<Utc>,
    /// Extra context: error message, line numbers, edit size, etc.
    /// serde_json::Value lets us store arbitrary per-action-type data
    /// without a separate struct for every ActionType variant.
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Summary of access frequency for a single file.
/// Used by the query engine to boost frequently-touched files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAccessCount {
    /// File path (relative to repo root).
    pub file_path: String,
    /// Total number of times this file was accessed.
    pub count: u64,
    /// Most recent access timestamp.
    pub last_accessed: DateTime<Utc>,
}

impl AgentAction {
    /// Creates a new AgentAction with a fresh UUID and the current UTC timestamp.
    ///
    /// # Example
    /// ```ignore
    /// let action = AgentAction::new(ActionType::FileRead, "src/auth.rs");
    /// ```
    pub fn new(action_type: ActionType, file_path: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            action_type,
            file_path: file_path.into(),
            entity_id: None,
            timestamp: Utc::now(),
            metadata: HashMap::new(),
        }
    }

    /// Attaches an entity id to this action (builder pattern).
    ///
    /// Builder pattern: returns `Self` so calls can be chained:
    /// `AgentAction::new(...).with_entity("src/auth.rs::login")`
    pub fn with_entity(mut self, entity_id: impl Into<String>) -> Self {
        self.entity_id = Some(entity_id.into());
        self
    }

    /// Attaches an arbitrary metadata key-value pair to this action.
    ///
    /// Uses `serde_json::Value` so any JSON-compatible data fits:
    /// numbers, strings, booleans, arrays, or nested objects.
    pub fn with_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    /// Returns `true` if this action represents an error.
    pub fn is_error(&self) -> bool {
        self.action_type == ActionType::Error
    }

    /// Returns the RFC3339 timestamp string for use as a KV key prefix.
    ///
    /// RFC3339 format ("2025-06-01T12:34:56.789Z") sorts lexicographically
    /// in chronological order. We use this as a key prefix so sled's
    /// lexicographic scan order gives us time order for free.
    pub fn timestamp_key(&self) -> String {
        self.timestamp.to_rfc3339()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn new_creates_unique_uuids() {
        let a = AgentAction::new(ActionType::FileRead, "a.rs");
        let b = AgentAction::new(ActionType::FileRead, "a.rs");
        assert_ne!(a.id, b.id, "each call must mint a distinct UUID");
    }

    #[test]
    fn new_timestamp_is_recent() {
        let before = Utc::now();
        let action = AgentAction::new(ActionType::FileRead, "a.rs");
        let after = Utc::now();
        assert!(action.timestamp >= before);
        assert!(action.timestamp <= after);
    }

    #[test]
    fn with_entity_sets_entity_id() {
        let action =
            AgentAction::new(ActionType::FileRead, "a.rs").with_entity("src/auth.rs::authenticate");
        assert_eq!(
            action.entity_id.as_deref(),
            Some("src/auth.rs::authenticate")
        );
    }

    #[test]
    fn with_entity_is_chainable() {
        // with_entity returns Self -both builder calls must succeed
        let action = AgentAction::new(ActionType::FileEdit, "b.rs")
            .with_entity("mod::fn")
            .with_metadata("lines_changed", serde_json::json!(42));
        assert!(action.entity_id.is_some());
        assert!(action.metadata.contains_key("lines_changed"));
    }

    #[test]
    fn with_metadata_inserts_key_value() {
        let action = AgentAction::new(ActionType::Error, "c.rs")
            .with_metadata("message", serde_json::json!("disk full"));
        assert_eq!(
            action.metadata.get("message"),
            Some(&serde_json::Value::String("disk full".into()))
        );
    }

    #[test]
    fn is_error_true_for_error_type() {
        assert!(AgentAction::new(ActionType::Error, "x.rs").is_error());
    }

    #[test]
    fn is_error_false_for_non_error_types() {
        assert!(!AgentAction::new(ActionType::FileRead, "x.rs").is_error());
        assert!(!AgentAction::new(ActionType::FileEdit, "x.rs").is_error());
        assert!(!AgentAction::new(ActionType::FunctionCall, "x.rs").is_error());
        assert!(!AgentAction::new(ActionType::TestRun, "x.rs").is_error());
    }

    #[test]
    fn timestamp_key_is_valid_rfc3339() {
        let action = AgentAction::new(ActionType::FileRead, "a.rs");
        let key = action.timestamp_key();
        assert!(
            DateTime::parse_from_rfc3339(&key).is_ok(),
            "timestamp_key must be parseable as RFC3339: {key}"
        );
    }

    #[test]
    fn timestamp_key_sorts_chronologically() {
        // Sleep a couple of ms to ensure the two timestamps are strictly ordered
        let a1 = AgentAction::new(ActionType::FileRead, "a.rs");
        thread::sleep(Duration::from_millis(2));
        let a2 = AgentAction::new(ActionType::FileRead, "b.rs");
        // RFC3339 strings sort lexicographically == chronologically
        assert!(
            a1.timestamp_key() < a2.timestamp_key(),
            "earlier action must have a lexicographically smaller timestamp_key"
        );
    }

    #[test]
    fn action_type_display() {
        assert_eq!(format!("{}", ActionType::FileRead), "file_read");
        assert_eq!(format!("{}", ActionType::FileEdit), "file_edit");
        assert_eq!(format!("{}", ActionType::FunctionCall), "function_call");
        assert_eq!(format!("{}", ActionType::TestRun), "test_run");
        assert_eq!(format!("{}", ActionType::Error), "error");
    }

    #[test]
    fn agent_action_json_roundtrip() {
        let action = AgentAction::new(ActionType::FileEdit, "src/main.rs");
        let json = serde_json::to_string(&action).unwrap();
        let decoded: AgentAction = serde_json::from_str(&json).unwrap();
        assert_eq!(action.id, decoded.id);
        assert_eq!(action.file_path, decoded.file_path);
        assert_eq!(action.action_type, decoded.action_type);
    }

    #[test]
    fn agent_action_full_fields_roundtrip() {
        let action = AgentAction::new(ActionType::FunctionCall, "src/lib.rs")
            .with_entity("src/lib.rs::my_fn")
            .with_metadata("context_window", serde_json::json!(4096))
            .with_metadata("duration_ms", serde_json::json!(12));
        let json = serde_json::to_string(&action).unwrap();
        let decoded: AgentAction = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.entity_id.as_deref(), Some("src/lib.rs::my_fn"));
        assert_eq!(decoded.metadata["context_window"], serde_json::json!(4096));
        assert_eq!(decoded.metadata["duration_ms"], serde_json::json!(12));
    }

    #[test]
    fn metadata_stores_json_value() {
        let action = AgentAction::new(ActionType::TestRun, "tests/main_test.rs")
            .with_metadata("passed", serde_json::json!(true))
            .with_metadata("count", serde_json::json!(42));
        assert_eq!(action.metadata["passed"], serde_json::json!(true));
        assert_eq!(action.metadata["count"], serde_json::json!(42));
    }

    #[test]
    fn file_access_count_json_roundtrip() {
        let fac = FileAccessCount {
            file_path: "src/auth.rs".into(),
            count: 7,
            last_accessed: Utc::now(),
        };
        let json = serde_json::to_string(&fac).unwrap();
        let decoded: FileAccessCount = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.file_path, "src/auth.rs");
        assert_eq!(decoded.count, 7);
    }
}
