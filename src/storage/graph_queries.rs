use super::kv_backend::KvBackend;
use crate::error::Result;
use crate::indexer::call_graph::CallGraph;

/// Graph query operations. For now this can use the in-process `KvBackend`,
/// but callers may attach an in-memory `CallGraph` for fast queries.
pub struct GraphQueries<'a> {
    kv: &'a KvBackend,
    graph: Option<&'a CallGraph>,
}

impl<'a> GraphQueries<'a> {
    pub fn new(kv: &'a KvBackend) -> Self {
        Self { kv, graph: None }
    }

    pub fn with_graph(mut self, graph: &'a CallGraph) -> Self {
        self.graph = Some(graph);
        self
    }

    pub fn find_callers(&self, entity_id: &str) -> Result<Vec<String>> {
        if let Some(g) = self.graph {
            let callers = g.get_callers(entity_id).into_iter().map(|e| e.id.clone()).collect();
            return Ok(callers);
        }
        Ok(Vec::new())
    }

    pub fn find_callees(&self, entity_id: &str) -> Result<Vec<String>> {
        if let Some(g) = self.graph {
            let callees = g.get_callees(entity_id).into_iter().map(|e| e.id.clone()).collect();
            return Ok(callees);
        }
        Ok(Vec::new())
    }

    pub fn find_entities_in_file(&self, file: &str) -> Result<Vec<String>> {
        if let Some(g) = self.graph {
            let ents = g.get_entities_in_file(file).into_iter().map(|e| e.id.clone()).collect();
            return Ok(ents);
        }
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
