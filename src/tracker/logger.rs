use super::action_log::AgentAction;
use super::history::ActionHistory;
use parking_lot::Mutex;
use std::sync::Arc;

/// Lock-free-ish logger backed by an in-memory history.
/// TODO: persist to KV backend in Part 3.
pub struct ActionLogger {
    history: Arc<Mutex<ActionHistory>>,
}

impl ActionLogger {
    pub fn new() -> Self {
        Self { history: Arc::new(Mutex::new(ActionHistory::new())) }
    }

    pub fn log(&self, action: AgentAction) {
        self.history.lock().push(action);
    }

    pub fn history(&self) -> Arc<Mutex<ActionHistory>> {
        Arc::clone(&self.history)
    }
}

impl Default for ActionLogger {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn log_and_retrieve() {
        let logger = ActionLogger::new();
        logger.log(AgentAction::FileRead {
            file: "x.py".into(),
            timestamp: Utc::now(),
            context_window: 1000,
            reason: None,
        });
        assert_eq!(logger.history().lock().len(), 1);
    }
}
