use crate::error::Result;
use dashmap::DashMap;

/// In-process lock-free KV store.
/// TODO: swap with KV-SWARM networked backend in Part 2.
pub struct KvBackend {
    data: DashMap<String, String>,
}

impl KvBackend {
    pub fn new() -> Self {
        Self { data: DashMap::new() }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.data.get(key).map(|v| v.value().clone())
    }

    pub fn set(&self, key: String, value: String) {
        self.data.insert(key, value);
    }

    pub fn delete(&self, key: &str) -> bool {
        self.data.remove(key).is_some()
    }

    pub fn list_prefix(&self, prefix: &str) -> Vec<String> {
        self.data.iter()
            .filter(|entry| entry.key().starts_with(prefix))
            .map(|entry| entry.key().clone())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl Default for KvBackend {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_delete() {
        let kv = KvBackend::new();
        kv.set("k1".into(), "v1".into());
        assert_eq!(kv.get("k1"), Some("v1".into()));
        assert!(kv.delete("k1"));
        assert_eq!(kv.get("k1"), None);
    }

    #[test]
    fn list_prefix() {
        let kv = KvBackend::new();
        kv.set("graph:a".into(), "1".into());
        kv.set("graph:b".into(), "2".into());
        kv.set("action:x".into(), "3".into());
        assert_eq!(kv.list_prefix("graph:").len(), 2);
    }
}
