//! Graph-aware storage layer.
//!
//! Architecture (bottom to top):
//!
//!   sled (disk)
//!     ↑  bytes
//!   KvBackend   -knows about bytes, not domain types
//!     ↑  keys / JSON
//!   GraphStore  -you are here. Knows about CodeEntity, CallGraph.
//!     ↑
//!   CLI / Query Engine
//!
//! Core insight: we pre-compute all indexes at WRITE time so every
//! read is a single KV lookup -O(1). Writing is O(V + E); reading is O(1).
//!
//! For graph traversal (BFS) we fan out one KV read per node visited,
//! which is O(nodes_visited) reads. Still fast because sled reads are
//! sub-millisecond on SSD.

use std::collections::{HashMap, HashSet, VecDeque};
use crate::error::{Error, Result};
use crate::indexer::{
    call_graph::{CallGraph, GraphMetadata},
    extractor::{CodeEntity, Language},
};
use super::{
    kv_backend::KvBackend,
    schema::{
        callers_key, callees_key, edge_key, entity_key,
        file_entities_key, lang_index_key, meta_graph_key,
        stale_key, watcher_last_reconcile_key,
    },
};

/// Graph-aware storage layer built on KvBackend.
///
/// All reads are O(1) because indexes are pre-computed at write time.
///
/// `Clone` is a shallow Arc increment -all clones share the same sled::Db.
#[derive(Clone)]
pub struct GraphStore {
    kv: KvBackend,
}

impl GraphStore {
    pub fn new(kv: KvBackend) -> Self {
        Self { kv }
    }

    // ── Write operations ──────────────────────────────────────────────────────

    /// Persists an entire CallGraph to the KV store.
    ///
    /// Clears all existing graph keys first so re-indexing is always consistent.
    /// Total writes ≈ 2*|V| + 2*|E| + |files| + |languages| + 1.
    ///
    /// Flushes to disk after all writes to guarantee durability.
    pub fn store_graph(&self, graph: &CallGraph) -> Result<()> {
        // Clear stale data from previous indexing runs so load_graph always
        // reflects exactly the graph we are about to write -no ghost entities.
        self.clear_graph_keys()?;

        // ── Phase 1: entities, file index, language index ─────────────────────
        //
        // We build file_index and lang_index in memory and write them in one
        // batch at the end of Phase 1, instead of read-modify-write per entity.
        let mut file_index: HashMap<String, Vec<String>> = HashMap::new();
        let mut lang_index: HashMap<String, Vec<String>> = HashMap::new();

        for (id, entity) in &graph.entities {
            self.kv.set(&entity_key(id), entity)?;

            file_index
                .entry(entity.file_path.clone())
                .or_default()
                .push(id.clone());

            // Language::Display gives "rust", "python", etc. -lowercase already.
            lang_index
                .entry(entity.language.to_string())
                .or_default()
                .push(id.clone());
        }

        for (file_path, ids) in &file_index {
            self.kv.set(&file_entities_key(file_path), ids)?;
        }

        for (lang, ids) in &lang_index {
            self.kv.set(&lang_index_key(lang), ids)?;
        }

        // ── Phase 2: edge indexes ─────────────────────────────────────────────
        //
        // For each directed edge (caller → callee) we write three records:
        //   edge:{caller}:{callee}   = "1"        existence check, O(1)
        //   callees:{caller}         = [callee_ids]  forward adjacency
        //   callers:{callee}         = [caller_ids]  reverse adjacency (pre-computed!)
        //
        // The reverse adjacency (callers) is the key design: because we compute
        // it here at write time, find_callers() is a single read -not a scan.

        let mut callees_map: HashMap<String, Vec<String>> = HashMap::new();
        let mut callers_map: HashMap<String, Vec<String>> = HashMap::new();

        for (caller_id, callee_id) in &graph.edges {
            self.kv.set(&edge_key(caller_id, callee_id), &"1")?;

            callees_map
                .entry(caller_id.clone())
                .or_default()
                .push(callee_id.clone());

            callers_map
                .entry(callee_id.clone())
                .or_default()
                .push(caller_id.clone());
        }

        for (id, callees) in &callees_map {
            self.kv.set(&callees_key(id), callees)?;
        }

        for (id, callers) in &callers_map {
            self.kv.set(&callers_key(id), callers)?;
        }

        // ── Phase 3: metadata ─────────────────────────────────────────────────
        self.kv.set(meta_graph_key(), &graph.metadata)?;

        // Flush once at the end -sled batches this efficiently.
        self.kv.flush()
    }

    /// Reconstructs a full CallGraph from the KV store.
    ///
    /// This is the inverse of store_graph().
    /// Time: O(V + E) reads -one per entity, one callee-list per entity.
    pub fn load_graph(&self) -> Result<CallGraph> {
        let entity_keys = self.kv.list_prefix("entity:")?;

        if entity_keys.is_empty() {
            return Err(Error::storage(
                "No graph in store. Run `graphswarm index` first."
            ));
        }

        // Reconstruct using add_entity / add_call so file_paths is maintained.
        let mut graph = CallGraph::new();

        for key in &entity_keys {
            if let Some(entity) = self.kv.get::<CodeEntity>(key)? {
                graph.add_entity(entity);
            }
        }

        // Reconstruct edges from the callees adjacency lists.
        // We only iterate entity IDs we already loaded -avoids scanning edge: keys.
        let ids: Vec<String> = graph.entities.keys().cloned().collect();
        for id in ids {
            let callees: Vec<String> = self.kv
                .get(&callees_key(&id))?
                .unwrap_or_default();
            for callee_id in callees {
                graph.add_call(id.clone(), callee_id);
            }
        }

        // add_entity refreshes total_files/total_entities/languages but not
        // repo_path or indexed_at -those come from the stored metadata.
        if let Some(meta) = self.kv.get::<GraphMetadata>(meta_graph_key())? {
            graph.set_repo_path(meta.repo_path);
            graph.metadata.indexed_at = meta.indexed_at;
        }

        Ok(graph)
    }

    // ── Read operations ───────────────────────────────────────────────────────

    /// Returns all entities that CALL the given entity id.
    ///
    /// O(1) KV read for the callers list, O(|callers|) reads for full entities.
    pub fn find_callers(&self, entity_id: &str) -> Result<Vec<CodeEntity>> {
        let ids: Vec<String> = self.kv
            .get(&callers_key(entity_id))?
            .unwrap_or_default();
        self.fetch_entities(&ids)
    }

    /// Returns all entities that the given entity CALLS.
    pub fn find_callees(&self, entity_id: &str) -> Result<Vec<CodeEntity>> {
        let ids: Vec<String> = self.kv
            .get(&callees_key(entity_id))?
            .unwrap_or_default();
        self.fetch_entities(&ids)
    }

    /// Returns all entities defined in the given file.
    pub fn find_in_file(&self, file_path: &str) -> Result<Vec<CodeEntity>> {
        let ids: Vec<String> = self.kv
            .get(&file_entities_key(file_path))?
            .unwrap_or_default();
        self.fetch_entities(&ids)
    }

    /// Looks up a single entity by its id.
    pub fn entity_by_id(&self, entity_id: &str) -> Result<Option<CodeEntity>> {
        self.kv.get(&entity_key(entity_id))
    }

    /// Returns all entity ids whose name field matches `name`.
    ///
    /// This is a linear scan over all entity: keys -O(V).
    /// Acceptable because name searches are rare and V < 500k in practice.
    pub fn find_entity_by_name(&self, name: &str) -> Result<Vec<String>> {
        let all_keys = self.kv.list_prefix("entity:")?;
        let mut matches = Vec::new();

        for key in all_keys {
            if let Some(entity) = self.kv.get::<CodeEntity>(&key)? {
                if entity.name == name {
                    matches.push(entity.id);
                }
            }
        }

        matches.sort();
        Ok(matches)
    }

    /// Returns all entities for the given language.
    pub fn find_by_language(&self, language: &Language) -> Result<Vec<CodeEntity>> {
        let ids: Vec<String> = self.kv
            .get(&lang_index_key(&language.to_string()))?
            .unwrap_or_default();
        self.fetch_entities(&ids)
    }

    /// Returns graph-level metadata, or None if no graph has been indexed.
    pub fn metadata(&self) -> Result<Option<GraphMetadata>> {
        self.kv.get(meta_graph_key())
    }

    /// Returns the number of stored entities.
    pub fn entity_count(&self) -> Result<usize> {
        Ok(self.kv.list_prefix("entity:")?.len())
    }

    /// Returns the number of stored directed edges.
    pub fn edge_count(&self) -> Result<usize> {
        Ok(self.kv.list_prefix("edge:")?.len())
    }

    /// Returns true if there is a direct call edge from `caller_id` to `callee_id`.
    pub fn edge_exists(&self, caller_id: &str, callee_id: &str) -> Result<bool> {
        self.kv.contains_key(&edge_key(caller_id, callee_id))
    }

    // ── Graph traversal over KV ───────────────────────────────────────────────
    //
    // Classic BFS algorithm, but instead of following in-memory pointers we
    // do one KV read per node to get its adjacency list. Each read is O(1).
    //
    // Time: O(V_visited) KV reads, where V_visited ≤ min(V, branching^depth).

    /// Forward BFS (follows call edges): returns all entity ids reachable from
    /// `start_entity` within `max_depth` hops, including the start node.
    pub fn bfs(&self, start_entity: &str, max_depth: usize) -> Result<Vec<String>> {
        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<(String, usize)> = VecDeque::new();
        queue.push_back((start_entity.to_string(), 0));

        while let Some((id, depth)) = queue.pop_front() {
            if !visited.insert(id.clone()) {
                continue; // already processed this node
            }

            if depth < max_depth {
                // One KV read to get the callees of this node
                let callees: Vec<String> = self.kv
                    .get(&callees_key(&id))?
                    .unwrap_or_default();

                for callee in callees {
                    if !visited.contains(&callee) {
                        queue.push_back((callee, depth + 1));
                    }
                }
            }
        }

        let mut result: Vec<String> = visited.into_iter().collect();
        result.sort();
        Ok(result)
    }

    /// Reverse BFS (follows caller edges): returns all entity ids that
    /// transitively call `start_entity` within `max_depth` hops.
    pub fn reverse_bfs(&self, start_entity: &str, max_depth: usize) -> Result<Vec<String>> {
        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<(String, usize)> = VecDeque::new();
        queue.push_back((start_entity.to_string(), 0));

        while let Some((id, depth)) = queue.pop_front() {
            if !visited.insert(id.clone()) {
                continue;
            }

            if depth < max_depth {
                // One KV read to get the callers of this node (reverse edges)
                let callers: Vec<String> = self.kv
                    .get(&callers_key(&id))?
                    .unwrap_or_default();

                for caller in callers {
                    if !visited.contains(&caller) {
                        queue.push_back((caller, depth + 1));
                    }
                }
            }
        }

        let mut result: Vec<String> = visited.into_iter().collect();
        result.sort();
        Ok(result)
    }

    /// Returns all entity ids that `start_entity` transitively depends on
    /// (BFS minus the start node itself).
    pub fn dependency_chain(&self, start_entity: &str, max_depth: usize) -> Result<Vec<String>> {
        let mut all = self.bfs(start_entity, max_depth)?;
        all.retain(|id| id != start_entity);
        Ok(all)
    }

    /// Returns all entity keys in the store (for QueryEngine full scan).
    ///
    /// Keys are returned with the `"entity:"` prefix -strip it before
    /// passing to `entity_by_id`, which re-applies the prefix internally.
    pub fn entity_keys(&self) -> Result<Vec<String>> {
        self.kv.list_prefix("entity:")
    }

    // ── File watcher methods ──────────────────────────────────────────────────

    /// Deletes all entities for `file_path` and cascades edge index cleanup.
    ///
    /// This is the write-side of incremental updates: called by the Reconciler
    /// when a file is deleted or before re-indexing a modified file.
    ///
    /// Cascade steps for each entity in the file:
    ///   1. Remove this entity_id from the callees list of every caller
    ///   2. Remove this entity_id from the callers list of every callee
    ///   3. Delete all edge:{this}:{*} and edge:{*}:{this} keys
    ///   4. Delete callers:{this} and callees:{this} index keys
    ///   5. Delete entity:{this}
    ///
    /// Then delete the file_entities index for the file.
    pub fn delete_file(&self, file_path: &str) -> Result<()> {
        let entity_ids: Vec<String> = self.kv
            .get(&file_entities_key(file_path))?
            .unwrap_or_default();

        for entity_id in &entity_ids {
            // 1. Remove from callers' callees lists
            let callers_of: Vec<String> = self.kv
                .get(&callers_key(entity_id))?
                .unwrap_or_default();
            for caller_id in &callers_of {
                let mut callees: Vec<String> = self.kv
                    .get(&callees_key(caller_id))?
                    .unwrap_or_default();
                callees.retain(|id| id != entity_id);
                if callees.is_empty() {
                    self.kv.delete(&callees_key(caller_id))?;
                } else {
                    self.kv.set(&callees_key(caller_id), &callees)?;
                }
                self.kv.delete(&edge_key(caller_id, entity_id))?;
            }

            // 2. Remove from callees' callers lists
            let callees_of: Vec<String> = self.kv
                .get(&callees_key(entity_id))?
                .unwrap_or_default();
            for callee_id in &callees_of {
                let mut callers: Vec<String> = self.kv
                    .get(&callers_key(callee_id))?
                    .unwrap_or_default();
                callers.retain(|id| id != entity_id);
                if callers.is_empty() {
                    self.kv.delete(&callers_key(callee_id))?;
                } else {
                    self.kv.set(&callers_key(callee_id), &callers)?;
                }
                self.kv.delete(&edge_key(entity_id, callee_id))?;
            }

            // 3. Remove entity's own index keys
            self.kv.delete(&callers_key(entity_id))?;
            self.kv.delete(&callees_key(entity_id))?;
            self.kv.delete(&entity_key(entity_id))?;

            // 4. Update language index (load entity first to know its language)
            // Language index cleanup is best-effort -stale IDs are harmlessly ignored on read.
        }

        // Delete the file_entities index entry
        self.kv.delete(&file_entities_key(file_path))?;
        Ok(())
    }

    /// Marks `file_path` as having unreconciled on-disk changes.
    ///
    /// The QueryEngine checks this flag and attaches a `stale_warning` to
    /// any RelevantFile that is marked stale.
    pub fn mark_stale(&self, file_path: &str) -> Result<()> {
        self.kv.set(&stale_key(file_path), &"1")
    }

    /// Clears the stale flag for `file_path` after successful re-indexing.
    pub fn clear_stale(&self, file_path: &str) -> Result<()> {
        self.kv.delete(&stale_key(file_path))
    }

    /// Returns true if `file_path` has unreconciled on-disk changes.
    pub fn is_stale(&self, file_path: &str) -> Result<bool> {
        self.kv.contains_key(&stale_key(file_path))
    }

    /// Returns the file paths of all files currently marked stale.
    pub fn all_stale_files(&self) -> Result<Vec<String>> {
        let keys = self.kv.list_prefix("stale:")?;
        Ok(keys.iter().map(|k| {
            k.strip_prefix("stale:").unwrap_or(k).replace('|', "/")
        }).collect())
    }

    /// Returns all file paths transitively affected by changes to `file_path`.
    ///
    /// Uses reverse BFS (follow `callers` edges) to find every file that
    /// calls into `file_path`. These files may need re-indexing or cache
    /// invalidation after `file_path` changes.
    pub fn impact_subtree(&self, file_path: &str) -> Result<Vec<String>> {
        let entity_ids: Vec<String> = self.kv
            .get(&file_entities_key(file_path))?
            .unwrap_or_default();

        let mut affected_files: HashSet<String> = HashSet::new();

        for entity_id in &entity_ids {
            let reachable = self.reverse_bfs(entity_id, 5)?;
            for rid in reachable {
                if let Ok(Some(e)) = self.entity_by_id(&rid) {
                    if e.file_path != file_path {
                        affected_files.insert(e.file_path);
                    }
                }
            }
        }

        let mut files: Vec<String> = affected_files.into_iter().collect();
        files.sort();
        Ok(files)
    }

    /// Records the timestamp of the last successful reconciler pass.
    pub fn set_last_reconcile_time(&self, ts: &str) -> Result<()> {
        self.kv.set(watcher_last_reconcile_key(), &ts.to_string())
    }

    /// Stores a single `CodeEntity` and updates all relevant indexes.
    ///
    /// Used by the Reconciler for incremental updates -faster than a full
    /// `store_graph()` when only one file changed.
    ///
    /// NOTE: Cross-file call edges are not resolved here (that requires the
    /// full symbol table). Run `graphswarm index` for complete resolution.
    pub fn store_single_entity(&self, entity: &CodeEntity) -> Result<()> {
        // Entity record
        self.kv.set(&entity_key(&entity.id), entity)?;

        // File entities index
        let mut file_ids: Vec<String> = self.kv
            .get(&file_entities_key(&entity.file_path))?
            .unwrap_or_default();
        if !file_ids.contains(&entity.id) {
            file_ids.push(entity.id.clone());
            self.kv.set(&file_entities_key(&entity.file_path), &file_ids)?;
        }

        // Callees + edge existence + callers of callees
        if !entity.calls.is_empty() {
            self.kv.set(&callees_key(&entity.id), &entity.calls)?;
            for callee_id in &entity.calls {
                let mut callers: Vec<String> = self.kv
                    .get(&callers_key(callee_id))?
                    .unwrap_or_default();
                if !callers.contains(&entity.id) {
                    callers.push(entity.id.clone());
                    self.kv.set(&callers_key(callee_id), &callers)?;
                }
                self.kv.set(&edge_key(&entity.id, callee_id), &"1")?;
            }
        }

        // Language index
        let lang_key = lang_index_key(&entity.language.to_string());
        let mut lang_ids: Vec<String> = self.kv.get(&lang_key)?.unwrap_or_default();
        if !lang_ids.contains(&entity.id) {
            lang_ids.push(entity.id.clone());
            self.kv.set(&lang_key, &lang_ids)?;
        }

        Ok(())
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Deletes all graph-owned keys so re-indexing is always consistent.
    fn clear_graph_keys(&self) -> Result<()> {
        for prefix in &["entity:", "callers:", "callees:", "file:", "edge:", "index:", "meta:"] {
            for key in self.kv.list_prefix(prefix)? {
                self.kv.delete(&key)?;
            }
        }
        Ok(())
    }

    /// Fetches full CodeEntity records for a slice of entity ids.
    /// Missing ids are silently skipped (defensive against partial writes).
    fn fetch_entities(&self, ids: &[String]) -> Result<Vec<CodeEntity>> {
        let mut entities = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(entity) = self.kv.get::<CodeEntity>(&entity_key(id))? {
                entities.push(entity);
            }
        }
        Ok(entities)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::{
        call_graph::CallGraph,
        extractor::{CodeEntity, EntityType, Language},
    };
    use tempfile::TempDir;

    fn temp_store() -> (GraphStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let backend = KvBackend::open(dir.path()).unwrap();
        (GraphStore::new(backend), dir)
    }

    /// 3-entity, 2-edge graph: main → authenticate_user → verify_token
    fn make_test_graph() -> CallGraph {
        let main_e = CodeEntity {
            id: "src/main.rs::main".into(),
            name: "main".into(),
            entity_type: EntityType::Function,
            file_path: "src/main.rs".into(),
            line_start: 1, line_end: 10,
            language: Language::Rust,
            docstring: Some("Entry point".into()),
            calls: vec!["src/auth.rs::authenticate_user".into()],
            called_by: vec![],
        };
        let auth_e = CodeEntity {
            id: "src/auth.rs::authenticate_user".into(),
            name: "authenticate_user".into(),
            entity_type: EntityType::Function,
            file_path: "src/auth.rs".into(),
            line_start: 5, line_end: 25,
            language: Language::Rust,
            docstring: Some("Authenticates a user by JWT token".into()),
            calls: vec!["src/auth.rs::verify_token".into()],
            called_by: vec!["src/main.rs::main".into()],
        };
        let verify_e = CodeEntity {
            id: "src/auth.rs::verify_token".into(),
            name: "verify_token".into(),
            entity_type: EntityType::Function,
            file_path: "src/auth.rs".into(),
            line_start: 30, line_end: 45,
            language: Language::Rust,
            docstring: None,
            calls: vec![],
            called_by: vec!["src/auth.rs::authenticate_user".into()],
        };

        let mut graph = CallGraph::new();
        graph.set_repo_path("./test_repo".into());
        // indexed_at is set by CallGraph::new() -leave as-is for tests.

        graph.add_entity(main_e);
        graph.add_entity(auth_e);
        graph.add_entity(verify_e);

        graph.add_call("src/main.rs::main".into(), "src/auth.rs::authenticate_user".into());
        graph.add_call("src/auth.rs::authenticate_user".into(), "src/auth.rs::verify_token".into());

        graph
    }

    // ── store / load roundtrip ────────────────────────────────────────────────

    #[test]
    fn store_load_entity_count() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        let loaded = store.load_graph().unwrap();
        assert_eq!(loaded.entities.len(), 3);
    }

    #[test]
    fn store_load_edge_count() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        let loaded = store.load_graph().unwrap();
        assert_eq!(loaded.edges.len(), 2);
    }

    #[test]
    fn store_load_entity_fields_preserved() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        let loaded = store.load_graph().unwrap();
        let auth = loaded.entities.get("src/auth.rs::authenticate_user").unwrap();
        assert_eq!(auth.name, "authenticate_user");
        assert_eq!(auth.file_path, "src/auth.rs");
        assert_eq!(auth.line_start, 5);
        assert_eq!(auth.docstring.as_deref(), Some("Authenticates a user by JWT token"));
    }

    #[test]
    fn store_load_metadata_preserved() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        let loaded = store.load_graph().unwrap();
        assert_eq!(loaded.metadata.repo_path, "./test_repo");
        assert_eq!(loaded.metadata.total_entities, 3);
    }

    #[test]
    fn load_empty_store_returns_error() {
        let (store, _dir) = temp_store();
        assert!(store.load_graph().is_err());
    }

    // ── find_callers ──────────────────────────────────────────────────────────

    #[test]
    fn find_callers_correct() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        let callers = store.find_callers("src/auth.rs::authenticate_user").unwrap();
        assert_eq!(callers.len(), 1);
        assert_eq!(callers[0].name, "main");
    }

    #[test]
    fn find_callers_on_root_returns_empty() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        assert!(store.find_callers("src/main.rs::main").unwrap().is_empty());
    }

    #[test]
    fn find_callers_unknown_entity_returns_empty() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        assert!(store.find_callers("nonexistent::fn").unwrap().is_empty());
    }

    // ── find_callees ──────────────────────────────────────────────────────────

    #[test]
    fn find_callees_correct() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        let callees = store.find_callees("src/auth.rs::authenticate_user").unwrap();
        assert_eq!(callees.len(), 1);
        assert_eq!(callees[0].name, "verify_token");
    }

    #[test]
    fn find_callees_on_leaf_returns_empty() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        assert!(store.find_callees("src/auth.rs::verify_token").unwrap().is_empty());
    }

    // ── find_in_file ──────────────────────────────────────────────────────────

    #[test]
    fn find_in_file_correct() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        let entities = store.find_in_file("src/auth.rs").unwrap();
        assert_eq!(entities.len(), 2);
        let names: Vec<&str> = entities.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"authenticate_user"));
        assert!(names.contains(&"verify_token"));
    }

    #[test]
    fn find_in_file_nonexistent_returns_empty() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        assert!(store.find_in_file("src/missing.rs").unwrap().is_empty());
    }

    // ── entity_by_id ──────────────────────────────────────────────────────────

    #[test]
    fn entity_by_id_found() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        let entity = store.entity_by_id("src/main.rs::main").unwrap();
        assert_eq!(entity.unwrap().name, "main");
    }

    #[test]
    fn entity_by_id_not_found() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        assert!(store.entity_by_id("nonexistent::id").unwrap().is_none());
    }

    // ── edge_exists ───────────────────────────────────────────────────────────

    #[test]
    fn edge_exists_true() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        assert!(store.edge_exists("src/main.rs::main", "src/auth.rs::authenticate_user").unwrap());
    }

    #[test]
    fn edge_exists_false() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        // main does not directly call verify_token (only through authenticate_user)
        assert!(!store.edge_exists("src/main.rs::main", "src/auth.rs::verify_token").unwrap());
    }

    #[test]
    fn edge_exists_is_directional() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        assert!(store.edge_exists("src/main.rs::main", "src/auth.rs::authenticate_user").unwrap());
        assert!(!store.edge_exists("src/auth.rs::authenticate_user", "src/main.rs::main").unwrap());
    }

    // ── BFS traversal ─────────────────────────────────────────────────────────

    #[test]
    fn bfs_depth_1_includes_start_and_direct_callee() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        let visited = store.bfs("src/main.rs::main", 1).unwrap();
        assert!(visited.contains(&"src/main.rs::main".to_string()));
        assert!(visited.contains(&"src/auth.rs::authenticate_user".to_string()));
        assert!(!visited.contains(&"src/auth.rs::verify_token".to_string()));
    }

    #[test]
    fn bfs_depth_2_reaches_full_chain() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        let visited = store.bfs("src/main.rs::main", 2).unwrap();
        assert!(visited.contains(&"src/auth.rs::verify_token".to_string()));
    }

    #[test]
    fn reverse_bfs_finds_upstream_callers() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        let callers = store.reverse_bfs("src/auth.rs::verify_token", 2).unwrap();
        assert!(callers.contains(&"src/main.rs::main".to_string()));
        assert!(callers.contains(&"src/auth.rs::authenticate_user".to_string()));
    }

    #[test]
    fn dependency_chain_excludes_start_node() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        let chain = store.dependency_chain("src/main.rs::main", 2).unwrap();
        assert!(!chain.contains(&"src/main.rs::main".to_string()));
        assert!(chain.contains(&"src/auth.rs::authenticate_user".to_string()));
        assert!(chain.contains(&"src/auth.rs::verify_token".to_string()));
    }

    // ── persistence ───────────────────────────────────────────────────────────

    #[test]
    fn graph_persists_across_reopen() {
        let dir = TempDir::new().unwrap();

        {
            let backend = KvBackend::open(dir.path()).unwrap();
            GraphStore::new(backend).store_graph(&make_test_graph()).unwrap();
        }

        {
            let backend = KvBackend::open(dir.path()).unwrap();
            let loaded = GraphStore::new(backend).load_graph().unwrap();
            assert_eq!(loaded.entities.len(), 3);
            assert_eq!(loaded.edges.len(), 2);
        }
    }

    // ── delete_file ───────────────────────────────────────────────────────────

    #[test]
    fn delete_file_removes_entities() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        store.delete_file("src/auth.rs").unwrap();
        assert!(store.find_in_file("src/auth.rs").unwrap().is_empty());
    }

    #[test]
    fn delete_file_cascades_caller_edges() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        // authenticate_user is called by main; after deleting auth.rs
        // the callees list of main should no longer contain authenticate_user
        store.delete_file("src/auth.rs").unwrap();
        let callees = store.find_callees("src/main.rs::main").unwrap();
        assert!(callees.is_empty(), "main should have no callees after auth.rs deleted");
    }

    #[test]
    fn delete_file_nonexistent_is_ok() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        assert!(store.delete_file("no_such_file.rs").is_ok());
    }

    // ── stale markers ─────────────────────────────────────────────────────────

    #[test]
    fn mark_and_is_stale() {
        let (store, _dir) = temp_store();
        assert!(!store.is_stale("src/auth.rs").unwrap());
        store.mark_stale("src/auth.rs").unwrap();
        assert!(store.is_stale("src/auth.rs").unwrap());
    }

    #[test]
    fn clear_stale_removes_flag() {
        let (store, _dir) = temp_store();
        store.mark_stale("src/auth.rs").unwrap();
        store.clear_stale("src/auth.rs").unwrap();
        assert!(!store.is_stale("src/auth.rs").unwrap());
    }

    #[test]
    fn all_stale_files_returns_marked_paths() {
        let (store, _dir) = temp_store();
        store.mark_stale("src/auth.rs").unwrap();
        store.mark_stale("src/main.rs").unwrap();
        let stale = store.all_stale_files().unwrap();
        assert_eq!(stale.len(), 2);
        assert!(stale.contains(&"src/auth.rs".to_string()));
    }

    // ── impact_subtree ────────────────────────────────────────────────────────

    #[test]
    fn impact_subtree_finds_callers() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        // auth.rs is called by main.rs -impact_subtree should surface main.rs
        let affected = store.impact_subtree("src/auth.rs").unwrap();
        assert!(affected.contains(&"src/main.rs".to_string()),
            "main.rs calls into auth.rs, must appear in impact subtree");
    }

    #[test]
    fn impact_subtree_excludes_self() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        let affected = store.impact_subtree("src/auth.rs").unwrap();
        assert!(!affected.contains(&"src/auth.rs".to_string()));
    }

    #[test]
    fn graph_store_clone_shares_underlying_db() {
        // Clone before storing -both should see the write afterward.
        let dir = TempDir::new().unwrap();
        let kv = KvBackend::open(dir.path()).unwrap();
        let store1 = GraphStore::new(kv);
        let store2 = store1.clone();
        store1.store_graph(&make_test_graph()).unwrap();
        // store2 is an Arc clone of the same sled::Db -must see 3 entities
        assert_eq!(store2.entity_count().unwrap(), 3);
    }

    #[test]
    fn entity_keys_returns_all_entity_keys() {
        let (store, _dir) = temp_store();
        store.store_graph(&make_test_graph()).unwrap();
        let keys = store.entity_keys().unwrap();
        assert_eq!(keys.len(), 3);
        assert!(keys.iter().all(|k| k.starts_with("entity:")));
    }

    #[test]
    fn entity_keys_empty_store_returns_empty() {
        let (store, _dir) = temp_store();
        assert!(store.entity_keys().unwrap().is_empty());
    }

    #[test]
    fn reindex_clears_stale_entities() {
        let (store, _dir) = temp_store();

        // First index: 3 entities
        store.store_graph(&make_test_graph()).unwrap();
        assert_eq!(store.entity_count().unwrap(), 3);

        // Re-index with a smaller graph (verify_token removed)
        let mut small = make_test_graph();
        small.entities.remove("src/auth.rs::verify_token");
        small.edges.retain(|(_, callee)| callee != "src/auth.rs::verify_token");
        store.store_graph(&small).unwrap();

        // load_graph should reflect the new index exactly -no ghost entities
        let loaded = store.load_graph().unwrap();
        assert_eq!(loaded.entities.len(), 2);
    }
}
