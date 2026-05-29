use std::collections::{HashMap, HashSet, VecDeque};
use super::extractor::CodeEntity;

pub struct CallGraph {
    entities: HashMap<String, CodeEntity>,
    edges: HashMap<String, Vec<String>>,
    reverse_edges: HashMap<String, Vec<String>>,
    files: HashSet<String>,
}

impl CallGraph {
    pub fn new() -> Self {
        Self {
            entities: HashMap::new(),
            edges: HashMap::new(),
            reverse_edges: HashMap::new(),
            files: HashSet::new(),
        }
    }

    pub fn add_entity(&mut self, entity: CodeEntity) {
        let id = entity.id.clone();
        self.files.insert(entity.file.clone());
        self.entities.insert(id.clone(), entity);
        self.edges.entry(id.clone()).or_default();
        self.reverse_edges.entry(id).or_default();
    }

    pub fn add_call(&mut self, from: String, to: String) {
        self.edges.entry(from.clone()).or_default().push(to.clone());
        self.reverse_edges.entry(to).or_default().push(from);
    }

    pub fn get_entity(&self, id: &str) -> Option<&CodeEntity> {
        self.entities.get(id)
    }

    pub fn get_callees(&self, id: &str) -> Vec<&CodeEntity> {
        self.edges.get(id)
            .map(|ids| ids.iter().filter_map(|i| self.entities.get(i)).collect())
            .unwrap_or_default()
    }

    pub fn get_callers(&self, id: &str) -> Vec<&CodeEntity> {
        self.reverse_edges.get(id)
            .map(|ids| ids.iter().filter_map(|i| self.entities.get(i)).collect())
            .unwrap_or_default()
    }

    pub fn get_entities_in_file(&self, file: &str) -> Vec<&CodeEntity> {
        self.entities.values().filter(|e| e.file == file).collect()
    }

    pub fn files(&self) -> Vec<String> {
        self.files.iter().cloned().collect()
    }

    pub fn entities(&self) -> &HashMap<String, CodeEntity> {
        &self.entities
    }

    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.values().map(|v| v.len()).sum()
    }

    pub fn bfs(&self, start_id: &str, max_depth: usize) -> HashSet<String> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back((start_id.to_string(), 0usize));

        while let Some((id, depth)) = queue.pop_front() {
            if !visited.insert(id.clone()) { continue; }
            if depth < max_depth {
                if let Some(neighbours) = self.edges.get(&id) {
                    for n in neighbours {
                        if !visited.contains(n) {
                            queue.push_back((n.clone(), depth + 1));
                        }
                    }
                }
            }
        }
        visited
    }

    pub fn reverse_bfs(&self, start_id: &str, max_depth: usize) -> HashSet<String> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back((start_id.to_string(), 0usize));

        while let Some((id, depth)) = queue.pop_front() {
            if !visited.insert(id.clone()) { continue; }
            if depth < max_depth {
                if let Some(neighbours) = self.reverse_edges.get(&id) {
                    for n in neighbours {
                        if !visited.contains(n) {
                            queue.push_back((n.clone(), depth + 1));
                        }
                    }
                }
            }
        }
        visited
    }
}

impl Default for CallGraph {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::extractor::EntityType;

    fn entity(id: &str, file: &str) -> CodeEntity {
        CodeEntity::new(id.into(), id.into(), file.into(), EntityType::Function, format!("def {id}()"), 1)
    }

    #[test]
    fn add_and_lookup() {
        let mut g = CallGraph::new();
        g.add_entity(entity("a", "a.py"));
        assert!(g.get_entity("a").is_some());
        assert_eq!(g.entity_count(), 1);
    }

    #[test]
    fn edges() {
        let mut g = CallGraph::new();
        g.add_entity(entity("a", "a.py"));
        g.add_entity(entity("b", "b.py"));
        g.add_call("a".into(), "b".into());
        assert_eq!(g.get_callees("a").len(), 1);
        assert_eq!(g.get_callers("b").len(), 1);
        assert_eq!(g.edge_count(), 1);
    }

    #[test]
    fn bfs_depth() {
        let mut g = CallGraph::new();
        g.add_entity(entity("a", "a.py"));
        g.add_entity(entity("b", "b.py"));
        g.add_entity(entity("c", "c.py"));
        g.add_call("a".into(), "b".into());
        g.add_call("b".into(), "c".into());

        let d1 = g.bfs("a", 1);
        assert!(d1.contains("a") && d1.contains("b") && !d1.contains("c"));

        let d2 = g.bfs("a", 2);
        assert!(d2.contains("c"));
    }

    #[test]
    fn files_list() {
        let mut g = CallGraph::new();
        g.add_entity(entity("a", "x.py"));
        g.add_entity(entity("b", "y.py"));
        assert_eq!(g.files().len(), 2);
    }
}
