use super::action_log::AgentAction;

pub struct ActionHistory {
    actions: Vec<AgentAction>,
}

impl ActionHistory {
    pub fn new() -> Self { Self { actions: Vec::new() } }

    pub fn push(&mut self, action: AgentAction) {
        self.actions.push(action);
    }

    pub fn recent(&self, n: usize) -> &[AgentAction] {
        let start = self.actions.len().saturating_sub(n);
        &self.actions[start..]
    }

    pub fn len(&self) -> usize { self.actions.len() }
    pub fn is_empty(&self) -> bool { self.actions.is_empty() }
    pub fn clear(&mut self) { self.actions.clear(); }
    pub fn all(&self) -> &[AgentAction] { &self.actions }
}

impl Default for ActionHistory {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn recent_returns_last_n() {
        let mut h = ActionHistory::new();
        for i in 0..5 {
            h.push(AgentAction::FileRead {
                file: format!("{i}.py"),
                timestamp: Utc::now(),
                context_window: 100,
                reason: None,
            });
        }
        assert_eq!(h.recent(2).len(), 2);
        assert_eq!(h.recent(2)[0].file(), "3.py");
    }

    #[test]
    fn clear_empties() {
        let mut h = ActionHistory::new();
        h.push(AgentAction::Error {
            timestamp: Utc::now(), file: "x".into(), line: 1, message: "e".into(),
        });
        h.clear();
        assert!(h.is_empty());
    }
}
