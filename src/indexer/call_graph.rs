use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use super::extractor::{CodeEntity, Language};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphMetadata {
    pub repo_path: String,
    pub indexed_at: DateTime<Utc>,
    pub total_files: usize,
    pub total_entities: usize,
    pub languages: Vec<Language>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallGraph {
    pub entities: HashMap<String, CodeEntity>,
    pub edges: Vec<(String, String)>,
    pub metadata: GraphMetadata,

    #[serde(skip)]
    file_paths: HashSet<String>,
}

impl CallGraph {
    pub fn new() -> Self {
        Self {
            entities: HashMap::new(),
            edges: Vec::new(),
            metadata: GraphMetadata {
                repo_path: String::new(),
                indexed_at: Utc::now(),
                total_files: 0,
                total_entities: 0,
                languages: Vec::new(),
            },
            file_paths: HashSet::new(),
        }
    }

    fn refresh_metadata(&mut self) {
        let languages: HashSet<Language> = self.entities.values().map(|e| e.language).collect();
        self.metadata.total_files = self.file_paths.len();
        self.metadata.total_entities = self.entities.len();
        self.metadata.languages = languages.into_iter().collect();
    }

    pub fn set_repo_path(&mut self, repo_path: String) {
        self.metadata.repo_path = repo_path;
    }

    pub fn add_entity(&mut self, entity: CodeEntity) {
        let id = entity.id.clone();
        self.file_paths.insert(entity.file_path.clone());
        self.entities.insert(id.clone(), entity);
        self.refresh_metadata();
    }

    pub fn add_call(&mut self, from: String, to: String) {
        if !self.edges.contains(&(from.clone(), to.clone())) {
            self.edges.push((from.clone(), to.clone()));
        }
        self.refresh_metadata();
    }

    pub fn get_entity(&self, id: &str) -> Option<&CodeEntity> {
        self.entities.get(id)
    }

    pub fn get_callees(&self, id: &str) -> Vec<&CodeEntity> {
        self.edges.iter()
            .filter(|(caller, _)| caller == id)
            .filter_map(|(_, callee)| self.entities.get(callee))
            .collect()
    }

    pub fn get_callers(&self, id: &str) -> Vec<&CodeEntity> {
        self.edges.iter()
            .filter(|(_, callee)| callee == id)
            .filter_map(|(caller, _)| self.entities.get(caller))
            .collect()
    }

    pub fn get_entities_in_file(&self, file_path: &str) -> Vec<&CodeEntity> {
        self.entities.values().filter(|e| e.file_path == file_path).collect()
    }

    pub fn files(&self) -> Vec<String> {
        self.file_paths.iter().cloned().collect()
    }

    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn bfs(&self, start_id: &str, max_depth: usize) -> HashSet<String> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back((start_id.to_string(), 0));

        while let Some((id, depth)) = queue.pop_front() {
            if !visited.insert(id.clone()) {
                continue;
            }

            if depth < max_depth {
                for (caller, callee) in &self.edges {
                    if caller == &id && !visited.contains(callee) {
                        queue.push_back((callee.clone(), depth + 1));
                    }
                }
            }
        }

        visited
    }

    pub fn reverse_bfs(&self, start_id: &str, max_depth: usize) -> HashSet<String> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back((start_id.to_string(), 0));

        while let Some((id, depth)) = queue.pop_front() {
            if !visited.insert(id.clone()) {
                continue;
            }

            if depth < max_depth {
                for (caller, callee) in &self.edges {
                    if callee == &id && !visited.contains(caller) {
                        queue.push_back((caller.clone(), depth + 1));
                    }
                }
            }
        }

        visited
    }

    pub fn stats(&self) -> GraphMetadata {
        self.metadata.clone()
    }
}

impl Default for CallGraph {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::extractor::{EntityType, Language, CodeEntity};

    fn entity(id: &str, file_path: &str) -> CodeEntity {
        CodeEntity::new(
            id.into(),
            id.into(),
            EntityType::Function,
            file_path.into(),
            1,
            1,
            Language::Python,
            None,
        )
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
