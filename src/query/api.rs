//! Public QueryEngine -the single entry point for all query operations.
//!
//! The QueryEngine orchestrates:
//!   1. GraphStore (Part 2) -call graph lookups
//!   2. History (Part 3)    -access history for recency signal
//!   3. Relevance scoring   -four signals combined
//!   4. Ranker              -group by file, sort, top-K
//!
//! The query() method:
//!   - Scans all entities via GraphStore
//!   - Scores each entity against the query using all four signals
//!   - Groups by file and returns top-K RelevantFile results
//!
//! Design: QueryEngine owns both GraphStore and History.
//! Both are Arc-backed (KvBackend is Arc<sled::Db>) so constructing
//! QueryEngine with cloned backends costs only two Arc increments.

use std::collections::{HashMap, HashSet, VecDeque};

use chrono::Utc;

use crate::error::Result;
use crate::indexer::extractor::CodeEntity;
use crate::storage::graph_queries::GraphStore;
use crate::tracker::history::History;
use super::mod_types::RelevantFile;
use super::ranker::{build_reason, rank_files, ScoredEntity};
use super::relevance::{docstring_score, graph_score, name_score, recency_score};

/// Weights for the four relevance signals. Must sum to 1.0.
const W_NAME:      f64 = 0.4;
const W_GRAPH:     f64 = 0.3;
const W_RECENCY:   f64 = 0.2;
const W_DOCSTRING: f64 = 0.1;

/// Public query interface for GraphSwarm.
pub struct QueryEngine {
    store:   GraphStore,
    history: History,
}

impl QueryEngine {
    /// Creates a new QueryEngine backed by the given store and history.
    ///
    /// Both are cheap to clone -sled::Db is Arc-backed internally.
    pub fn new(store: GraphStore, history: History) -> Self {
        Self { store, history }
    }

    /// Finds the top-K most relevant files for a natural language query.
    ///
    /// Algorithm:
    /// 1. Precompute recency map: for each recently-accessed file, call
    ///    `history.file_last_accessed()` to get its real timestamp and compute
    ///    elapsed seconds. This is O(50 * k) for k history records.
    /// 2. List all entity keys from GraphStore
    /// 3. Score each entity against the query with four weighted signals
    /// 4. Filter zero-score entities (they add noise, not signal)
    /// 5. Rank by file, sort descending, return top-K
    ///
    /// Empty query or top_k=0 returns an empty vec (not an error).
    pub fn query(&self, q: &str, top_k: usize) -> Result<Vec<RelevantFile>> {
        let q = q.trim();
        if q.is_empty() || top_k == 0 {
            return Ok(Vec::new());
        }

        // Precompute recency signal: file_path → elapsed_seconds_since_last_access.
        // We call file_last_accessed() for each of the top-50 recently-accessed files.
        // Each call scans history:recent: (O(k)) -total O(50*k).
        // The result is O(1) lookups during scoring.
        let recency: HashMap<String, f64> = {
            let now = Utc::now();
            let recent_files = self.history.recent_files(50).unwrap_or_default();
            let mut map = HashMap::with_capacity(recent_files.len());
            for file_path in recent_files {
                if let Ok(Some(ts)) = self.history.file_last_accessed(&file_path) {
                    let elapsed = (now - ts).num_seconds().max(0) as f64;
                    map.insert(file_path, elapsed);
                }
            }
            map
        };

        // entity_keys() returns "entity:<id>" strings.
        let entity_keys = self.store.entity_keys()?;
        let mut scored = Vec::with_capacity(entity_keys.len() / 4 + 1);

        for key in &entity_keys {
            // Strip "entity:" prefix -entity_by_id re-applies it internally.
            let id = key.strip_prefix("entity:").unwrap_or(key);
            if let Some(entity) = self.store.entity_by_id(id)? {
                let score = self.score_entity(&entity, q, &recency);
                if score > 0.0 {
                    let distance = self.graph_distance_to_query(&entity, q);
                    let secs_ago = recency.get(&entity.file_path).copied();
                    let reason   = build_reason(&entity, q, distance, secs_ago);
                    scored.push(ScoredEntity { entity, score, reason });
                }
            }
        }

        Ok(rank_files(scored, top_k))
    }

    /// Returns a reference to the underlying `GraphStore`.
    ///
    /// Used by `McpServer` tool handlers that need raw store access
    /// (e.g. `find_callers`, `find_callees`) without holding a second clone.
    pub fn store(&self) -> &GraphStore {
        &self.store
    }

    /// Returns full details about a single entity by id.
    ///
    /// Used by the MCP `explain_entity` tool.
    pub fn explain(&self, entity_id: &str) -> Result<Option<CodeEntity>> {
        self.store.entity_by_id(entity_id)
    }

    /// Finds the shortest call path between two entities.
    ///
    /// Returns entity ids from `from` to `to`, or an empty vec if no path
    /// exists within 5 hops.
    pub fn path(&self, from: &str, to: &str) -> Result<Vec<String>> {
        let reachable = self.store.bfs(from, 5)?;
        if !reachable.contains(&to.to_string()) {
            return Ok(Vec::new());
        }
        self.bfs_path(from, to, 5)
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Computes the combined relevance score for one entity.
    ///
    /// Final score = W_NAME*name + W_GRAPH*graph + W_RECENCY*recency + W_DOCSTRING*doc
    fn score_entity(&self, entity: &CodeEntity, query: &str, recency: &HashMap<String, f64>) -> f64 {
        // Signal 1: name match (weight 0.4)
        // We also score against file_path so "src" matches "src/auth.rs".
        let s_name = name_score(&entity.name, query)
            .max(name_score(&entity.file_path, query));

        // Signal 2: graph distance (weight 0.3)
        // O(degree) approximation; full BFS would be O(V*E) -too slow.
        let s_graph = graph_score(self.approx_graph_distance(entity, query));

        // Signal 3: recency (weight 0.2) -real elapsed seconds from precomputed map.
        let s_recency = recency_score(recency.get(&entity.file_path).copied());

        // Signal 4: docstring (weight 0.1)
        let s_doc = docstring_score(entity.docstring.as_deref(), query);

        W_NAME * s_name + W_GRAPH * s_graph + W_RECENCY * s_recency + W_DOCSTRING * s_doc
    }

    /// Approximates the graph distance from `entity` to the nearest name-matching node.
    ///
    /// Checks only distance 0 (self), 1 (direct neighbors), and 2 (neighbor's neighbors).
    /// This is O(degree) per entity, not O(V*E) like full BFS.
    ///
    /// APPROXIMATION NOTE: this misses matches beyond 2 hops. Full BFS per
    /// entity would be O(V*E) -prohibitive on large graphs. The approximation
    /// is good enough for scoring and can be refined in Part 6 if needed.
    ///
    /// Returns usize::MAX when no match is found → graph_score returns 0.0.
    fn approx_graph_distance(&self, entity: &CodeEntity, query: &str) -> usize {
        // Distance 0: does this entity itself match?
        if name_score(&entity.name, query) > 0.0
            || name_score(&entity.file_path, query) > 0.0
        {
            return 0;
        }

        // Distance 1: does any direct callee or caller match by name?
        for callee_id in &entity.calls {
            let callee_name = callee_id.split("::").last().unwrap_or(callee_id);
            if name_score(callee_name, query) > 0.0 {
                return 1;
            }
        }
        for caller_id in &entity.called_by {
            let caller_name = caller_id.split("::").last().unwrap_or(caller_id);
            if name_score(caller_name, query) > 0.0 {
                return 1;
            }
        }

        // Distance 2: does any callee's callee match?
        for callee_id in &entity.calls {
            if let Ok(Some(callee)) = self.store.entity_by_id(callee_id) {
                for callee2_id in &callee.calls {
                    let name = callee2_id.split("::").last().unwrap_or(callee2_id);
                    if name_score(name, query) > 0.0 {
                        return 2;
                    }
                }
            }
        }

        usize::MAX
    }

    /// Returns the min graph distance to a name-matching node, or None if not nearby.
    fn graph_distance_to_query(&self, entity: &CodeEntity, query: &str) -> Option<usize> {
        let d = self.approx_graph_distance(entity, query);
        if d == usize::MAX { None } else { Some(d) }
    }

    /// BFS with parent tracking to reconstruct the actual call path from `from` to `to`.
    fn bfs_path(&self, from: &str, to: &str, max_depth: usize) -> Result<Vec<String>> {
        let mut parent: HashMap<String, String> = HashMap::new();
        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<(String, usize)> = VecDeque::new();

        queue.push_back((from.to_string(), 0));
        visited.insert(from.to_string());

        while let Some((current, depth)) = queue.pop_front() {
            if current == to {
                // Reconstruct path by following parent pointers back to start.
                let mut path = vec![current.clone()];
                let mut node = current;
                while let Some(p) = parent.get(&node) {
                    path.push(p.clone());
                    node = p.clone();
                }
                path.reverse();
                return Ok(path);
            }

            if depth >= max_depth {
                continue;
            }

            if let Ok(Some(entity)) = self.store.entity_by_id(&current) {
                for callee_id in &entity.calls {
                    if visited.insert(callee_id.clone()) {
                        parent.insert(callee_id.clone(), current.clone());
                        queue.push_back((callee_id.clone(), depth + 1));
                    }
                }
            }
        }

        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::{
        call_graph::CallGraph,
        extractor::{CodeEntity, EntityType, Language},
    };
    use crate::storage::kv_backend::KvBackend;
    use tempfile::TempDir;

    /// Standard 3-entity graph: main → authenticate_user → verify_token
    fn make_test_graph() -> CallGraph {
        let main_e = CodeEntity {
            id:          "src/main.rs::main".into(),
            name:        "main".into(),
            entity_type: EntityType::Function,
            file_path:   "src/main.rs".into(),
            line_start:  1, line_end: 10,
            language:    Language::Rust,
            docstring:   Some("Entry point".into()),
            calls:       vec!["src/auth.rs::authenticate_user".into()],
            called_by:   vec![],
        };
        let auth_e = CodeEntity {
            id:          "src/auth.rs::authenticate_user".into(),
            name:        "authenticate_user".into(),
            entity_type: EntityType::Function,
            file_path:   "src/auth.rs".into(),
            line_start:  5, line_end: 25,
            language:    Language::Rust,
            docstring:   Some("Authenticates a user by JWT token".into()),
            calls:       vec!["src/auth.rs::verify_token".into()],
            called_by:   vec!["src/main.rs::main".into()],
        };
        let verify_e = CodeEntity {
            id:          "src/auth.rs::verify_token".into(),
            name:        "verify_token".into(),
            entity_type: EntityType::Function,
            file_path:   "src/auth.rs".into(),
            line_start:  30, line_end: 45,
            language:    Language::Rust,
            docstring:   None,
            calls:       vec![],
            called_by:   vec!["src/auth.rs::authenticate_user".into()],
        };

        let mut graph = CallGraph::new();
        graph.set_repo_path("./test_repo".into());
        graph.add_entity(main_e);
        graph.add_entity(auth_e);
        graph.add_entity(verify_e);
        graph.add_call("src/main.rs::main".into(), "src/auth.rs::authenticate_user".into());
        graph.add_call("src/auth.rs::authenticate_user".into(), "src/auth.rs::verify_token".into());
        graph
    }

    fn make_test_engine() -> (QueryEngine, TempDir) {
        let dir = TempDir::new().unwrap();
        let kv = KvBackend::open(dir.path()).unwrap();
        let store = GraphStore::new(kv.clone());
        let history = History::new(kv);
        store.store_graph(&make_test_graph()).unwrap();
        (QueryEngine::new(store, history), dir)
    }

    // ── query() ───────────────────────────────────────────────────────────────

    #[test]
    fn query_empty_string_returns_empty() {
        let (engine, _dir) = make_test_engine();
        assert!(engine.query("", 10).unwrap().is_empty());
    }

    #[test]
    fn query_whitespace_only_returns_empty() {
        let (engine, _dir) = make_test_engine();
        assert!(engine.query("   ", 10).unwrap().is_empty());
    }

    #[test]
    fn query_top_k_zero_returns_empty() {
        let (engine, _dir) = make_test_engine();
        assert!(engine.query("authenticate", 0).unwrap().is_empty());
    }

    #[test]
    fn query_main_top_result_is_main_rs() {
        let (engine, _dir) = make_test_engine();
        let results = engine.query("main", 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].file_path, "src/main.rs");
    }

    #[test]
    fn query_authenticate_top_result_is_auth_rs() {
        let (engine, _dir) = make_test_engine();
        let results = engine.query("authenticate", 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].file_path, "src/auth.rs");
    }

    #[test]
    fn query_verify_top_result_is_auth_rs() {
        let (engine, _dir) = make_test_engine();
        let results = engine.query("verify", 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].file_path, "src/auth.rs");
    }

    #[test]
    fn query_top_k_one_returns_exactly_one() {
        let (engine, _dir) = make_test_engine();
        let results = engine.query("authenticate", 1).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn query_top_k_larger_than_results_returns_all() {
        let (engine, _dir) = make_test_engine();
        let results = engine.query("authenticate", 100).unwrap();
        // At most 2 distinct files (main.rs, auth.rs)
        assert!(results.len() <= 2);
    }

    #[test]
    fn query_results_sorted_descending() {
        let (engine, _dir) = make_test_engine();
        let results = engine.query("authenticate", 10).unwrap();
        for w in results.windows(2) {
            assert!(w[0].relevance_score >= w[1].relevance_score,
                "scores must be non-increasing");
        }
    }

    #[test]
    fn query_scores_in_zero_one_range() {
        let (engine, _dir) = make_test_engine();
        let results = engine.query("authenticate", 10).unwrap();
        for r in &results {
            assert!(r.relevance_score >= 0.0 && r.relevance_score <= 1.0,
                "score out of range: {}", r.relevance_score);
        }
    }

    #[test]
    fn query_reasons_non_empty() {
        let (engine, _dir) = make_test_engine();
        let results = engine.query("authenticate", 10).unwrap();
        for r in &results {
            assert!(!r.reason.is_empty(), "reason must not be empty");
        }
    }

    #[test]
    fn query_results_have_non_empty_entities_list() {
        let (engine, _dir) = make_test_engine();
        let results = engine.query("authenticate", 10).unwrap();
        for r in &results {
            assert!(!r.entities.is_empty(),
                "{} has empty entities list", r.file_path);
        }
    }

    #[test]
    fn query_no_matching_entities_returns_empty() {
        let (engine, _dir) = make_test_engine();
        let results = engine.query("zzz_no_match_xyz", 10).unwrap();
        assert!(results.is_empty());
    }

    // ── explain() ─────────────────────────────────────────────────────────────

    #[test]
    fn explain_returns_correct_entity() {
        let (engine, _dir) = make_test_engine();
        let e = engine.explain("src/auth.rs::authenticate_user").unwrap();
        assert!(e.is_some());
        assert_eq!(e.unwrap().name, "authenticate_user");
    }

    #[test]
    fn explain_returns_none_for_unknown_id() {
        let (engine, _dir) = make_test_engine();
        assert!(engine.explain("src/unknown.rs::ghost").unwrap().is_none());
    }

    // ── path() ────────────────────────────────────────────────────────────────

    #[test]
    fn path_connected_entities_returns_non_empty() {
        let (engine, _dir) = make_test_engine();
        let p = engine.path("src/main.rs::main", "src/auth.rs::verify_token").unwrap();
        assert!(!p.is_empty());
    }

    #[test]
    fn path_includes_start_and_end() {
        let (engine, _dir) = make_test_engine();
        let p = engine.path("src/main.rs::main", "src/auth.rs::authenticate_user").unwrap();
        assert!(!p.is_empty());
        assert_eq!(p.first().unwrap(), "src/main.rs::main");
        assert_eq!(p.last().unwrap(),  "src/auth.rs::authenticate_user");
    }

    #[test]
    fn path_unconnected_entities_returns_empty() {
        let (engine, _dir) = make_test_engine();
        // verify_token does not call main -reverse direction has no path
        let p = engine.path("src/auth.rs::verify_token", "src/main.rs::main").unwrap();
        assert!(p.is_empty());
    }
}
