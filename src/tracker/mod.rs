pub mod action_log;
pub mod logger;
pub mod history;

pub use action_log::{AgentAction, ActionType, TestResult};
pub use logger::ActionLogger;
pub use history::ActionHistory;

/// Convenience wrapper around ActionLogger.
pub struct ActionTracker {
    logger: ActionLogger,
}

impl ActionTracker {
    pub fn new() -> Self { Self { logger: ActionLogger::new() } }

    pub fn log(&self, action: AgentAction) {
        self.logger.log(action);
    }

    pub fn recent(&self, n: usize) -> Vec<AgentAction> {
        self.logger.history().lock().recent(n).to_vec()
    }
}

impl Default for ActionTracker {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn tracker_round_trip() {
        let t = ActionTracker::new();
        t.log(AgentAction::FileRead {
            file: "a.py".into(), timestamp: Utc::now(), context_window: 100, reason: None,
        });
        assert_eq!(t.recent(10).len(), 1);
    }
}
