use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionType { FileRead, FileEdit, Error, TestRun }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TestResult { Pass, Fail, Skip }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentAction {
    FileRead {
        file: String,
        timestamp: DateTime<Utc>,
        context_window: usize,
        reason: Option<String>,
    },
    FileEdit {
        file: String,
        timestamp: DateTime<Utc>,
        diff: String,
        test_result: TestResult,
        lines_changed: usize,
        functions_affected: Vec<String>,
    },
    Error {
        timestamp: DateTime<Utc>,
        file: String,
        line: usize,
        message: String,
    },
    TestRun {
        timestamp: DateTime<Utc>,
        test_file: String,
        passed: bool,
        duration_ms: u64,
    },
}

impl AgentAction {
    pub fn action_type(&self) -> ActionType {
        match self {
            Self::FileRead { .. } => ActionType::FileRead,
            Self::FileEdit { .. } => ActionType::FileEdit,
            Self::Error { .. } => ActionType::Error,
            Self::TestRun { .. } => ActionType::TestRun,
        }
    }

    pub fn file(&self) -> &str {
        match self {
            Self::FileRead { file, .. }
            | Self::FileEdit { file, .. }
            | Self::Error { file, .. } => file,
            Self::TestRun { test_file, .. } => test_file,
        }
    }

    pub fn timestamp(&self) -> DateTime<Utc> {
        match self {
            Self::FileRead { timestamp, .. }
            | Self::FileEdit { timestamp, .. }
            | Self::Error { timestamp, .. }
            | Self::TestRun { timestamp, .. } => *timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_read() -> AgentAction {
        AgentAction::FileRead {
            file: "a.py".into(),
            timestamp: Utc::now(),
            context_window: 4096,
            reason: None,
        }
    }

    #[test]
    fn action_type_matches() {
        assert_eq!(sample_read().action_type(), ActionType::FileRead);
    }

    #[test]
    fn file_accessor() {
        assert_eq!(sample_read().file(), "a.py");
    }

    #[test]
    fn serialization_roundtrip() {
        let a = sample_read();
        let json = serde_json::to_string(&a).unwrap();
        let _: AgentAction = serde_json::from_str(&json).unwrap();
    }
}
