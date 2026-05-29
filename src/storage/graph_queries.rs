use super::kv_backend::KvBackend;
use crate::error::Result;

/// Graph query operations backed by KV store.
/// TODO: implement in Part 2.
pub struct GraphQueries<'a> {
    kv: &'a KvBackend,
}

impl<'a> GraphQueries<'a> {
    pub fn new(kv: &'a KvBackend) -> Self {
        Self { kv }
    }

    pub fn find_callers(&self, _entity_id: &str) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    pub fn find_callees(&self, _entity_id: &str) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    pub fn find_entities_in_file(&self, _file: &str) -> Result<Vec<String>> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_queries() {
        let kv = KvBackend::new();
        let gq = GraphQueries::new(&kv);
        assert!(gq.find_callers("x").unwrap().is_empty());
    }
}
