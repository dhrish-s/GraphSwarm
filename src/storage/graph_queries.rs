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
            let mut callers: Vec<String> = g.get_callers(entity_id).into_iter().map(|e| e.id.clone()).collect();
            callers.sort();
            return Ok(callers);
        }
        Ok(Vec::new())
    }

    pub fn find_callees(&self, entity_id: &str) -> Result<Vec<String>> {
        if let Some(g) = self.graph {
            let mut callees: Vec<String> = g.get_callees(entity_id).into_iter().map(|e| e.id.clone()).collect();
            callees.sort();
            return Ok(callees);
        }
        Ok(Vec::new())
    }

    pub fn find_entities_in_file(&self, file: &str) -> Result<Vec<String>> {
        if let Some(g) = self.graph {
            let mut ents: Vec<String> = g.get_entities_in_file(file).into_iter().map(|e| e.id.clone()).collect();
            ents.sort();
            return Ok(ents);
        }
        Ok(Vec::new())
    }

    pub fn find_entity_by_name(&self, name: &str) -> Result<Vec<String>> {
        if let Some(g) = self.graph {
            let mut matches: Vec<String> = g.entities.values()
                .filter(|entity| entity.name == name)
                .map(|entity| entity.id.clone())
                .collect();
            matches.sort();
            return Ok(matches);
        }
        Ok(Vec::new())
    }

    pub fn bfs(&self, start_entity: &str, max_depth: usize) -> Result<Vec<String>> {
        if let Some(g) = self.graph {
            let mut visited: Vec<String> = g.bfs(start_entity, max_depth).into_iter().collect();
            visited.sort();
            return Ok(visited);
        }
        Ok(Vec::new())
    }

    pub fn reverse_bfs(&self, start_entity: &str, max_depth: usize) -> Result<Vec<String>> {
        if let Some(g) = self.graph {
            let mut visited: Vec<String> = g.reverse_bfs(start_entity, max_depth).into_iter().collect();
            visited.sort();
            return Ok(visited);
        }
        Ok(Vec::new())
    }

    pub fn dependency_chain(&self, start_entity: &str, max_depth: usize) -> Result<Vec<String>> {
        if let Some(g) = self.graph {
            let mut chain: Vec<String> = g.bfs(start_entity, max_depth)
                .into_iter()
                .filter(|id| id != start_entity)
                .collect();
            chain.sort();
            return Ok(chain);
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::call_graph::CallGraph;

    fn entity(id: &str, file_path: &str) -> crate::indexer::extractor::CodeEntity {
        crate::indexer::extractor::CodeEntity::new(
            id.into(),
            id.split("::").last().unwrap_or_default().into(),
            crate::indexer::extractor::EntityType::Function,
            file_path.into(),
            1,
            1,
            crate::indexer::extractor::Language::Python,
            None,
        )
    }

    #[test]
    fn empty_queries() {
        let kv = KvBackend::new();
        let gq = GraphQueries::new(&kv);
        assert!(gq.find_callers("x").unwrap().is_empty());
    }

    #[test]
    fn same_file_caller_callee_lookup() {
        let kv = KvBackend::new();
        let mut graph = CallGraph::new();
        graph.add_entity(entity("a.py::foo", "a.py"));
        graph.add_entity(entity("a.py::bar", "a.py"));
        graph.add_call("a.py::bar".into(), "a.py::foo".into());

        let gq = GraphQueries::new(&kv).with_graph(&graph);
        assert_eq!(gq.find_callers("a.py::foo").unwrap(), vec!["a.py::bar"]);
        assert_eq!(gq.find_callees("a.py::bar").unwrap(), vec!["a.py::foo"]);
    }

    #[test]
    fn cross_file_caller_callee_lookup() {
        let kv = KvBackend::new();
        let mut graph = CallGraph::new();
        graph.add_entity(entity("payment.py::process_payment", "payment.py"));
        graph.add_entity(entity("checkout.py::checkout", "checkout.py"));
        graph.add_call("checkout.py::checkout".into(), "payment.py::process_payment".into());

        let gq = GraphQueries::new(&kv).with_graph(&graph);
        assert_eq!(gq.find_callers("payment.py::process_payment").unwrap(), vec!["checkout.py::checkout"]);
        assert_eq!(gq.find_callees("checkout.py::checkout").unwrap(), vec!["payment.py::process_payment"]);
    }

    #[test]
    fn bfs_traversal_returns_dependency_chain() {
        let kv = KvBackend::new();
        let mut graph = CallGraph::new();
        graph.add_entity(entity("a.py::a", "a.py"));
        graph.add_entity(entity("b.py::b", "b.py"));
        graph.add_entity(entity("c.py::c", "c.py"));
        graph.add_call("a.py::a".into(), "b.py::b".into());
        graph.add_call("b.py::b".into(), "c.py::c".into());

        let gq = GraphQueries::new(&kv).with_graph(&graph);
        let chain = gq.dependency_chain("a.py::a", 2).unwrap();
        assert_eq!(chain, vec!["b.py::b", "c.py::c"]);
    }

    #[test]
    fn bfs_traversal_includes_start_and_callees() {
        let kv = KvBackend::new();
        let mut graph = CallGraph::new();
        graph.add_entity(entity("a.py::a", "a.py"));
        graph.add_entity(entity("b.py::b", "b.py"));
        graph.add_entity(entity("c.py::c", "c.py"));
        graph.add_call("a.py::a".into(), "b.py::b".into());
        graph.add_call("b.py::b".into(), "c.py::c".into());

        let gq = GraphQueries::new(&kv).with_graph(&graph);
        let visited = gq.bfs("a.py::a", 1).unwrap();
        assert_eq!(visited, vec!["a.py::a", "b.py::b"]);
    }

    #[test]
    fn reverse_bfs_traversal_returns_callers() {
        let kv = KvBackend::new();
        let mut graph = CallGraph::new();
        graph.add_entity(entity("a.py::a", "a.py"));
        graph.add_entity(entity("b.py::b", "b.py"));
        graph.add_entity(entity("c.py::c", "c.py"));
        graph.add_call("a.py::a".into(), "b.py::b".into());
        graph.add_call("a.py::a".into(), "c.py::c".into());

        let gq = GraphQueries::new(&kv).with_graph(&graph);
        let callers = gq.reverse_bfs("b.py::b", 1).unwrap();
        assert_eq!(callers, vec!["a.py::a", "b.py::b"]);
    }

    #[test]
    fn entity_lookup_by_file_and_name() {
        let kv = KvBackend::new();
        let mut graph = CallGraph::new();
        graph.add_entity(entity("a.py::run", "a.py"));
        graph.add_entity(entity("b.py::run", "b.py"));
        graph.add_entity(entity("b.py::helper", "b.py"));

        let gq = GraphQueries::new(&kv).with_graph(&graph);
        assert_eq!(gq.find_entities_in_file("b.py").unwrap(), vec!["b.py::helper", "b.py::run"]);
        assert_eq!(gq.find_entity_by_name("run").unwrap(), vec!["a.py::run", "b.py::run"]);
    }

    #[test]
    fn missing_entity_handling_returns_empty() {
        let kv = KvBackend::new();
        let gq = GraphQueries::new(&kv);
        assert!(gq.find_callees("nonexistent").unwrap().is_empty());
        assert!(gq.bfs("nonexistent", 3).unwrap().is_empty());
        assert!(gq.find_entity_by_name("missing").unwrap().is_empty());
    }
}
