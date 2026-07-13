//! Real end-to-end integration tests for GraphSwarm.
//!
//! Each test creates a real on-disk file structure, runs the indexer,
//! persists to a sled KV store, and verifies query / traversal results.

use std::io::Write;
use tempfile::TempDir;

use graphswarm::indexer::CodeIndexer;
use graphswarm::mcp::protocol::McpRequest;
use graphswarm::mcp::server::McpServer;
use graphswarm::query::QueryEngine;
use graphswarm::storage::{GraphStore, KvBackend};
use graphswarm::tracker::History;

// ── helpers ───────────────────────────────────────────────────────────────────

fn write_file(dir: &TempDir, name: &str, content: &str) -> String {
    let path = dir.path().join(name);
    let mut f = std::fs::File::create(&path).unwrap();
    write!(f, "{content}").unwrap();
    path.to_string_lossy().to_string()
}

// ── Test 1: index Rust files and query ────────────────────────────────────────

#[test]
fn integration_index_and_query_rust_files() {
    let dir = TempDir::new().unwrap();

    // Write 3 Rust files with known relationships:  main → auth → db
    write_file(
        &dir,
        "main.rs",
        "fn main() { authenticate(); }\nfn authenticate() {}\n",
    );
    write_file(&dir, "auth.rs", "fn authenticate_user() { db_query(); }\n");
    write_file(&dir, "db.rs", "fn db_query() {}\n");

    let indexer = CodeIndexer::new("auto").unwrap();
    let graph = indexer.index_directory(dir.path(), &[]).unwrap();

    let kv = KvBackend::open(&dir.path().join("db")).unwrap();
    let store = GraphStore::new(kv.clone());
    store.store_graph(&graph).unwrap();

    let engine = QueryEngine::new(store, History::new(kv));
    let results = engine.query("authenticate", 5).unwrap();

    assert!(!results.is_empty(), "expected results for 'authenticate'");
    // auth.rs should be in the top 3 results
    let top3_paths: Vec<&str> = results
        .iter()
        .take(3)
        .map(|r| r.file_path.as_str())
        .collect();
    assert!(
        top3_paths.iter().any(|p| p.contains("auth")),
        "auth.rs not in top 3: {top3_paths:?}"
    );

    // All scores must be in [0, 1]
    for r in &results {
        assert!(
            r.relevance_score >= 0.0 && r.relevance_score <= 1.0,
            "score out of range: {}",
            r.relevance_score
        );
    }
}

// ── Test 2: caller chain traversal ───────────────────────────────────────────

#[test]
fn integration_caller_chain_traversal() {
    use graphswarm::indexer::{
        call_graph::CallGraph,
        extractor::{CodeEntity, EntityType, Language},
    };

    let dir = TempDir::new().unwrap();
    let kv = KvBackend::open(dir.path()).unwrap();

    // Build graph: A → B → C
    let mut graph = CallGraph::new();
    graph.set_repo_path(".".into());
    for (id, calls, called_by) in [
        ("file.rs::a", vec!["file.rs::b"], vec![]),
        ("file.rs::b", vec!["file.rs::c"], vec!["file.rs::a"]),
        ("file.rs::c", vec![], vec!["file.rs::b"]),
    ] {
        graph.add_entity(CodeEntity {
            id: id.into(),
            name: id.split("::").last().unwrap().to_string(),
            entity_type: EntityType::Function,
            file_path: "file.rs".into(),
            line_start: 1,
            line_end: 5,
            language: Language::Rust,
            docstring: None,
            calls: calls.iter().map(|s| s.to_string()).collect(),
            called_by: called_by.iter().map(|s| s.to_string()).collect(),
        });
    }
    graph.add_call("file.rs::a".into(), "file.rs::b".into());
    graph.add_call("file.rs::b".into(), "file.rs::c".into());

    let store = GraphStore::new(kv.clone());
    store.store_graph(&graph).unwrap();

    // get_callers(C) → B
    let callers_of_c = store.find_callers("file.rs::c").unwrap();
    assert_eq!(callers_of_c.len(), 1);
    assert_eq!(callers_of_c[0].id, "file.rs::b");

    // get_callers(B) → A
    let callers_of_b = store.find_callers("file.rs::b").unwrap();
    assert_eq!(callers_of_b.len(), 1);
    assert_eq!(callers_of_b[0].id, "file.rs::a");

    // shortest_path A → C  = [A, B, C]
    let engine = QueryEngine::new(store, History::new(kv));
    let path = engine.path("file.rs::a", "file.rs::c").unwrap();
    assert_eq!(path, vec!["file.rs::a", "file.rs::b", "file.rs::c"]);
}

// ── Test 3: delete_file cascades edges ───────────────────────────────────────

#[test]
fn integration_delete_file_cascade() {
    use graphswarm::indexer::{
        call_graph::CallGraph,
        extractor::{CodeEntity, EntityType, Language},
    };

    let dir = TempDir::new().unwrap();
    let kv = KvBackend::open(dir.path()).unwrap();
    let store = GraphStore::new(kv);

    let mut graph = CallGraph::new();
    graph.set_repo_path(".".into());

    // main.rs calls auth.rs which calls db.rs
    for (id, file, calls) in [
        ("main.rs::run", "main.rs", vec!["auth.rs::login"]),
        ("auth.rs::login", "auth.rs", vec!["db.rs::query"]),
        ("db.rs::query", "db.rs", vec![]),
    ] {
        graph.add_entity(CodeEntity {
            id: id.into(),
            name: id.split("::").last().unwrap().to_string(),
            entity_type: EntityType::Function,
            file_path: file.into(),
            line_start: 1,
            line_end: 5,
            language: Language::Rust,
            docstring: None,
            calls: calls.iter().map(|s| s.to_string()).collect(),
            called_by: vec![],
        });
    }
    graph.add_call("main.rs::run".into(), "auth.rs::login".into());
    graph.add_call("auth.rs::login".into(), "db.rs::query".into());
    store.store_graph(&graph).unwrap();

    // Delete the middle file
    store.delete_file("auth.rs").unwrap();

    // auth.rs entities are gone
    assert!(store.find_in_file("auth.rs").unwrap().is_empty());

    // main.rs::run's callees list should no longer contain auth.rs::login
    let callees_of_run = store.find_callees("main.rs::run").unwrap();
    let has_auth = callees_of_run.iter().any(|e| e.id == "auth.rs::login");
    assert!(
        !has_auth,
        "auth.rs::login should be removed from main.rs::run's callees"
    );
}

// ── Test 4: MCP tools/call query_graph round-trip ────────────────────────────

#[test]
fn integration_mcp_tools_call_roundtrip() {
    use graphswarm::indexer::{
        call_graph::CallGraph,
        extractor::{CodeEntity, EntityType, Language},
    };
    use graphswarm::mcp::tools::GraphSwarmState;

    let dir = TempDir::new().unwrap();
    let kv = KvBackend::open(dir.path()).unwrap();
    let store = GraphStore::new(kv.clone());

    let mut graph = CallGraph::new();
    graph.set_repo_path(".".into());
    graph.add_entity(CodeEntity {
        id: "src/auth.rs::authenticate_user".into(),
        name: "authenticate_user".into(),
        entity_type: EntityType::Function,
        file_path: "src/auth.rs".into(),
        line_start: 1,
        line_end: 10,
        language: Language::Rust,
        docstring: Some("Authenticates a user".into()),
        calls: vec![],
        called_by: vec![],
    });
    store.store_graph(&graph).unwrap();

    let engine = QueryEngine::new(store, History::new(kv));
    let state = GraphSwarmState { engine };
    let server = McpServer::new(dir.path().join("db"));

    let req = McpRequest {
        jsonrpc: "2.0".into(),
        id: Some(serde_json::json!(1)),
        method: "tools/call".into(),
        params: Some(serde_json::json!({
            "name": "query_graph",
            "arguments": { "query": "authenticate", "top_k": 5 }
        })),
    };

    let v = server.handle_request(req, Some(&state));

    // Must be a valid JSON-RPC 2.0 response
    assert_eq!(v["jsonrpc"], "2.0");
    assert_eq!(v["id"], 1);
    assert!(v.get("result").is_some(), "expected result, got: {v}");
    // Content must be present
    assert!(v["result"]["content"].as_array().is_some());
}

// ── Test 5: MCP tools/list returns exactly 6 tools ───────────────────────────

#[test]
fn integration_mcp_tools_list() {
    let dir = TempDir::new().unwrap();
    let server = McpServer::new(dir.path().join("db"));

    let req = McpRequest {
        jsonrpc: "2.0".into(),
        id: Some(serde_json::json!(1)),
        method: "tools/list".into(),
        params: None,
    };

    let v = server.handle_request(req, None);
    assert_eq!(v["jsonrpc"], "2.0");
    let tools = v["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 6, "expected exactly 6 tools");

    // Every tool must have name, description, inputSchema
    for tool in tools {
        assert!(tool["name"].as_str().is_some());
        assert!(tool["description"].as_str().is_some());
        assert!(tool.get("input_schema").is_some() || tool.get("inputSchema").is_some());
    }
}

// ── Test 6: stale flag round-trip through QueryEngine ────────────────────────

#[test]
fn integration_stale_flag_round_trip() {
    use graphswarm::indexer::{
        call_graph::CallGraph,
        extractor::{CodeEntity, EntityType, Language},
    };

    let dir = TempDir::new().unwrap();
    let kv = KvBackend::open(dir.path()).unwrap();
    let store = GraphStore::new(kv.clone());

    let mut graph = CallGraph::new();
    graph.set_repo_path(".".into());
    graph.add_entity(CodeEntity {
        id: "src/auth.rs::authenticate".into(),
        name: "authenticate".into(),
        entity_type: EntityType::Function,
        file_path: "src/auth.rs".into(),
        line_start: 1,
        line_end: 5,
        language: Language::Rust,
        docstring: None,
        calls: vec![],
        called_by: vec![],
    });
    store.store_graph(&graph).unwrap();

    // Mark auth.rs stale
    store.mark_stale("src/auth.rs").unwrap();

    // Query -stale_warning must be Some
    let engine = QueryEngine::new(store.clone(), History::new(kv.clone()));
    let results = engine.query("authenticate", 5).unwrap();
    assert!(!results.is_empty());
    let auth_result = results
        .iter()
        .find(|r| r.file_path.contains("auth"))
        .unwrap();
    assert!(
        auth_result.stale_warning.is_some(),
        "expected stale_warning, got None"
    );

    // Clear stale
    store.clear_stale("src/auth.rs").unwrap();

    // Query again -stale_warning must be None
    let engine2 = QueryEngine::new(store, History::new(kv));
    let results2 = engine2.query("authenticate", 5).unwrap();
    let auth2 = results2
        .iter()
        .find(|r| r.file_path.contains("auth"))
        .unwrap();
    assert!(
        auth2.stale_warning.is_none(),
        "expected no stale_warning after clear, got Some"
    );
}
