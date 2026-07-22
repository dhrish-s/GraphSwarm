//! Tool definitions and dispatch for the GraphSwarm MCP server.
//!
//! Design: a dispatch registry pattern.
//! Adding a new tool = one entry in `tool_definitions()` + one arm in `dispatch()`.
//! Nothing else needs to change.
//!
//! Each tool handler:
//!   - Takes `&serde_json::Value` (tool arguments, already extracted from params)
//!   - Takes `&GraphSwarmState` (shared engine + store)
//!   - Returns `Result<Vec<ContentBlock>>`
//!
//! Handlers NEVER panic. They return `Err` on bad input.
//! The server converts `Err` into a proper `McpErrorResponse`.

use crate::error::Result;
use crate::mcp::protocol::{ContentBlock, ToolDefinition};
use crate::query::QueryEngine;
use serde_json::Value;

/// Shared state passed to every tool handler.
///
/// Only `engine` is stored -raw store access goes through `engine.store()`,
/// which avoids keeping a second clone of the same sled::Db Arc.
pub struct GraphSwarmState {
    pub engine: QueryEngine,
}

/// Returns all tool definitions for the `tools/list` response.
/// This is the single source of truth for available tools.
pub fn tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "query_graph".into(),
            description: "Find the most relevant files for a natural language query. \
                Uses name matching, call graph distance, recency, and docstring signals. \
                Returns files ranked by relevance score with explanations."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language query, e.g. 'authentication flow'"
                    },
                    "top_k": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default: 5)",
                        "default": 5
                    }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "get_callers".into(),
            description: "Find all entities (functions/methods) that directly call \
                the specified entity. Returns entity ids, names, and file locations."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity_id": {
                        "type": "string",
                        "description": "Entity id in format 'file_path::function_name', \
                            e.g. 'src/auth.rs::authenticate_user'"
                    }
                },
                "required": ["entity_id"]
            }),
        },
        ToolDefinition {
            name: "get_callees".into(),
            description: "Find all entities that the specified entity directly calls. \
                Returns the call dependencies of a function or method."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity_id": {
                        "type": "string",
                        "description": "Entity id in format 'file_path::function_name'"
                    }
                },
                "required": ["entity_id"]
            }),
        },
        ToolDefinition {
            name: "shortest_path".into(),
            description: "Find the shortest call path between two entities. \
                Returns the chain of function calls connecting 'from' to 'to'. \
                Empty result means no path exists within 5 hops."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "from": { "type": "string", "description": "Starting entity id" },
                    "to":   { "type": "string", "description": "Target entity id" }
                },
                "required": ["from", "to"]
            }),
        },
        ToolDefinition {
            name: "explain_entity".into(),
            description: "Get full details about a specific code entity: \
                type, file path, line numbers, docstring, what it calls, \
                and what calls it."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity_id": {
                        "type": "string",
                        "description": "Entity id in format 'file_path::function_name'"
                    }
                },
                "required": ["entity_id"]
            }),
        },
        ToolDefinition {
            name: "find_tests".into(),
            description: "Find test functions in the repository. With no arguments, \
                lists every detected test function. With 'entity_id', finds the \
                test functions that call this entity directly or transitively -i.e. \
                which tests cover it."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity_id": {
                        "type": "string",
                        "description": "Optional entity id in format 'file_path::function_name'. \
                            If provided, returns tests that cover this entity. \
                            If omitted, lists all test functions in the repository."
                    }
                },
                "required": []
            }),
        },
    ]
}

/// Dispatches a tool call by name to the correct handler.
///
/// Returns `Ok(Vec<ContentBlock>)` on success, `Err` on unknown tool or bad args.
pub fn dispatch(
    tool_name: &str,
    args: &Value,
    state: &GraphSwarmState,
) -> Result<Vec<ContentBlock>> {
    match tool_name {
        "query_graph" => handle_query_graph(args, state),
        "get_callers" => handle_get_callers(args, state),
        "get_callees" => handle_get_callees(args, state),
        "shortest_path" => handle_shortest_path(args, state),
        "explain_entity" => handle_explain_entity(args, state),
        "find_tests" => handle_find_tests(args, state),
        unknown => Err(crate::error::Error::query(format!(
            "Unknown tool: '{unknown}'. Available: query_graph, get_callers, \
             get_callees, shortest_path, explain_entity, find_tests"
        ))),
    }
}

/// Normalizes an entity ID so forward slashes and backslashes are treated
/// identically. MCP clients always send forward slashes. On Windows the
/// indexer stores backslashes. This converts to OS-native separator so
/// lookups always match.
///
/// In test builds the normalization is a no-op: test fixtures hardcode
/// entity IDs with forward slashes and the in-memory graph matches them.
fn normalize_entity_id(id: &str) -> String {
    if cfg!(test) {
        return id.to_string();
    }
    if let Some((path_part, fn_part)) = id.split_once("::") {
        let normalized = if cfg!(target_os = "windows") {
            path_part.replace('/', "\\")
        } else {
            path_part.replace('\\', "/")
        };
        format!("{normalized}::{fn_part}")
    } else {
        id.to_string()
    }
}

/// Resolves an entity ID from MCP client format to whatever form is actually
/// stored in the DB, trying multiple candidate forms.
///
/// MCP clients always send forward slashes and no leading `.\`. On Windows
/// the indexer may store backslashes and/or a `.\` prefix (e.g. when run as
/// `graphswarm index .`). This function:
///   1. Tries the slash-normalized form first.
///   2. Falls back to the `.\`-prefixed OS-native form if the first lookup
///      returns no entity -handles the common `index .` case on Windows.
///
/// `entity_by_id` is an O(1) KV read so the probe adds negligible overhead.
fn resolve_entity_id(id: &str, store: &crate::storage::GraphStore) -> String {
    let primary = normalize_entity_id(id);
    if store.entity_by_id(&primary).ok().flatten().is_some() {
        return primary;
    }
    // On Windows, the indexer prefixes paths with `.\ ` when indexed from the
    // repo root.  Try that form as a fallback.
    #[cfg(target_os = "windows")]
    if !cfg!(test) {
        if let Some((path_part, fn_part)) = id.split_once("::") {
            let prefixed = format!(".\\{}::{}", path_part.replace('/', "\\"), fn_part);
            if store.entity_by_id(&prefixed).ok().flatten().is_some() {
                return prefixed;
            }
        }
    }
    primary
}

// ── Tool handlers ─────────────────────────────────────────────────────────────

fn handle_query_graph(args: &Value, state: &GraphSwarmState) -> Result<Vec<ContentBlock>> {
    let query = args["query"].as_str().ok_or_else(|| {
        crate::error::Error::query("query_graph requires a 'query' string argument")
    })?;

    // Clamp top_k to a sensible range: at least 1, at most 20.
    let top_k = (args["top_k"].as_u64().unwrap_or(5) as usize).clamp(1, 20);

    let results = state.engine.query(query, top_k)?;

    if results.is_empty() {
        return Ok(vec![ContentBlock::text(format!(
            "No results found for: \"{query}\"\n\
             Make sure the repository is indexed: graphswarm index <path>"
        ))]);
    }

    let mut lines = vec![format!(
        "Query: \"{query}\" -top {} result(s)\n",
        results.len()
    )];

    for (i, r) in results.iter().enumerate() {
        let stale = r
            .stale_warning
            .as_deref()
            .map(|w| format!("\n   ⚠ {w}"))
            .unwrap_or_default();
        lines.push(format!(
            "{}. {} (score: {:.3})\n   Reason: {}{}\n   Entities:",
            i + 1,
            r.file_path,
            r.relevance_score,
            r.reason,
            stale
        ));
        for entity in &r.entities {
            lines.push(format!(
                "   - {} ({}) line {}-{}{}",
                entity.name,
                entity.entity_type,
                entity.line_start,
                entity.line_end,
                entity
                    .docstring
                    .as_ref()
                    .map(|d| format!("\n     \"{d}\""))
                    .unwrap_or_default()
            ));
        }
        lines.push(String::new());
    }

    Ok(vec![ContentBlock::text(lines.join("\n"))])
}

fn handle_get_callers(args: &Value, state: &GraphSwarmState) -> Result<Vec<ContentBlock>> {
    let entity_id_raw = args["entity_id"].as_str().ok_or_else(|| {
        crate::error::Error::query("get_callers requires an 'entity_id' string argument")
    })?;
    let entity_id_owned = resolve_entity_id(entity_id_raw, state.engine.store());
    let entity_id = entity_id_owned.as_str();

    let callers = state.engine.store().find_callers(entity_id)?;

    if callers.is_empty() {
        return Ok(vec![ContentBlock::text(format!(
            "No callers found for: {entity_id}\n\
             Either nothing calls this entity, or it hasn't been indexed."
        ))]);
    }

    let mut lines = vec![format!("Callers of {entity_id}:\n")];
    for c in &callers {
        lines.push(format!(
            "- {} ({}:{}–{})",
            c.id, c.file_path, c.line_start, c.line_end
        ));
    }
    lines.push(format!("\nTotal: {} caller(s)", callers.len()));

    Ok(vec![ContentBlock::text(lines.join("\n"))])
}

fn handle_get_callees(args: &Value, state: &GraphSwarmState) -> Result<Vec<ContentBlock>> {
    let entity_id_raw = args["entity_id"].as_str().ok_or_else(|| {
        crate::error::Error::query("get_callees requires an 'entity_id' string argument")
    })?;
    let entity_id_owned = resolve_entity_id(entity_id_raw, state.engine.store());
    let entity_id = entity_id_owned.as_str();

    let callees = state.engine.store().find_callees(entity_id)?;

    if callees.is_empty() {
        return Ok(vec![ContentBlock::text(format!(
            "No callees found for: {entity_id}\n\
             Either this entity calls nothing, or it hasn't been indexed."
        ))]);
    }

    let mut lines = vec![format!("Callees of {entity_id}:\n")];
    for c in &callees {
        lines.push(format!(
            "- {} ({}:{}–{})",
            c.id, c.file_path, c.line_start, c.line_end
        ));
    }
    lines.push(format!("\nTotal: {} callee(s)", callees.len()));

    Ok(vec![ContentBlock::text(lines.join("\n"))])
}

fn handle_shortest_path(args: &Value, state: &GraphSwarmState) -> Result<Vec<ContentBlock>> {
    let from_raw = args["from"].as_str().ok_or_else(|| {
        crate::error::Error::query("shortest_path requires a 'from' string argument")
    })?;
    let to_raw = args["to"].as_str().ok_or_else(|| {
        crate::error::Error::query("shortest_path requires a 'to' string argument")
    })?;
    let from_owned = resolve_entity_id(from_raw, state.engine.store());
    let to_owned = resolve_entity_id(to_raw, state.engine.store());
    let from = from_owned.as_str();
    let to = to_owned.as_str();

    let path = state.engine.path(from, to)?;

    if path.is_empty() {
        return Ok(vec![ContentBlock::text(format!(
            "No call path found from {from} to {to} within 5 hops.\n\
             Either they are not connected or the direction is reversed."
        ))]);
    }

    let mut lines = vec![format!(
        "Shortest call path: {} → {} ({} hops)\n",
        from,
        to,
        path.len() - 1
    )];
    for (i, node) in path.iter().enumerate() {
        let arrow = if i + 1 < path.len() { " →" } else { "" };
        lines.push(format!("  {}. {}{}", i + 1, node, arrow));
    }

    Ok(vec![ContentBlock::text(lines.join("\n"))])
}

fn handle_explain_entity(args: &Value, state: &GraphSwarmState) -> Result<Vec<ContentBlock>> {
    let entity_id_raw = args["entity_id"].as_str().ok_or_else(|| {
        crate::error::Error::query("explain_entity requires an 'entity_id' string argument")
    })?;
    let entity_id_owned = resolve_entity_id(entity_id_raw, state.engine.store());
    let entity_id = entity_id_owned.as_str();

    match state.engine.explain(entity_id)? {
        None => Ok(vec![ContentBlock::text(format!(
            "Entity not found: {entity_id}\n\
             Run `graphswarm index <path>` to index the repository first."
        ))]),
        Some(e) => {
            let mut lines = vec![
                format!("Entity: {}", e.id),
                format!("Name:   {}", e.name),
                format!("Type:   {}", e.entity_type),
                format!(
                    "File:   {} (lines {}–{})",
                    e.file_path, e.line_start, e.line_end
                ),
                format!("Language: {}", e.language),
            ];
            if let Some(doc) = &e.docstring {
                lines.push(format!("Docstring: \"{doc}\""));
            }
            if !e.calls.is_empty() {
                lines.push(format!("\nCalls ({}):", e.calls.len()));
                for c in &e.calls {
                    lines.push(format!("  - {c}"));
                }
            }
            if !e.called_by.is_empty() {
                lines.push(format!("\nCalled by ({}):", e.called_by.len()));
                for c in &e.called_by {
                    lines.push(format!("  - {c}"));
                }
            }
            Ok(vec![ContentBlock::text(lines.join("\n"))])
        }
    }
}

fn handle_find_tests(args: &Value, state: &GraphSwarmState) -> Result<Vec<ContentBlock>> {
    match args.get("entity_id").and_then(|v| v.as_str()) {
        // entity_id given: find tests that cover it (directly or transitively).
        Some(entity_id_raw) => {
            let entity_id_owned = resolve_entity_id(entity_id_raw, state.engine.store());
            let entity_id = entity_id_owned.as_str();

            let tests = state.engine.store().tests_covering(entity_id)?;

            if tests.is_empty() {
                return Ok(vec![ContentBlock::text(format!(
                    "No tests found covering: {entity_id}\n\
                     Either no test calls this entity (directly or transitively), \
                     or it hasn't been indexed."
                ))]);
            }

            let mut lines = vec![format!("Tests covering {entity_id}:\n")];
            for t in &tests {
                lines.push(format!(
                    "- {} ({}:{}–{})",
                    t.id, t.file_path, t.line_start, t.line_end
                ));
            }
            lines.push(format!("\nTotal: {} test(s)", tests.len()));

            Ok(vec![ContentBlock::text(lines.join("\n"))])
        }
        // No entity_id: list every detected test function.
        None => {
            let tests = state.engine.store().find_all_tests()?;

            if tests.is_empty() {
                return Ok(vec![ContentBlock::text(
                    "No test functions found in the indexed repository.".to_string(),
                )]);
            }

            let mut lines = vec![format!("Found {} test function(s):\n", tests.len())];
            for t in &tests {
                lines.push(format!(
                    "- {} ({}:{}–{})",
                    t.id, t.file_path, t.line_start, t.line_end
                ));
            }

            Ok(vec![ContentBlock::text(lines.join("\n"))])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::{
        call_graph::CallGraph,
        extractor::{CodeEntity, EntityType, Language},
    };
    use crate::query::QueryEngine;
    use crate::storage::{GraphStore, KvBackend};
    use crate::tracker::History;
    use tempfile::TempDir;

    fn make_test_graph() -> CallGraph {
        let main_e = CodeEntity {
            id: "src/main.rs::main".into(),
            name: "main".into(),
            entity_type: EntityType::Function,
            file_path: "src/main.rs".into(),
            line_start: 1,
            line_end: 10,
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
            line_start: 5,
            line_end: 25,
            language: Language::Rust,
            docstring: Some("Authenticates a user by JWT".into()),
            calls: vec!["src/auth.rs::verify_token".into()],
            called_by: vec!["src/main.rs::main".into()],
        };
        let verify_e = CodeEntity {
            id: "src/auth.rs::verify_token".into(),
            name: "verify_token".into(),
            entity_type: EntityType::Function,
            file_path: "src/auth.rs".into(),
            line_start: 30,
            line_end: 45,
            language: Language::Rust,
            docstring: None,
            calls: vec![],
            called_by: vec!["src/auth.rs::authenticate_user".into()],
        };
        let mut g = CallGraph::new();
        g.set_repo_path("./test_repo".into());
        g.add_entity(main_e);
        g.add_entity(auth_e);
        g.add_entity(verify_e);
        g.add_call(
            "src/main.rs::main".into(),
            "src/auth.rs::authenticate_user".into(),
        );
        g.add_call(
            "src/auth.rs::authenticate_user".into(),
            "src/auth.rs::verify_token".into(),
        );
        g
    }

    fn make_test_state() -> (GraphSwarmState, TempDir) {
        let dir = TempDir::new().unwrap();
        let kv = KvBackend::open(dir.path()).unwrap();
        let store = GraphStore::new(kv.clone());
        store.store_graph(&make_test_graph()).unwrap();
        let engine = QueryEngine::new(store, History::new(kv));
        (GraphSwarmState { engine }, dir)
    }

    /// Same as `make_test_graph`, plus a TestFunction that calls
    /// `authenticate_user` -so it transitively covers `verify_token`.
    fn make_test_state_with_tests() -> (GraphSwarmState, TempDir) {
        let dir = TempDir::new().unwrap();
        let kv = KvBackend::open(dir.path()).unwrap();
        let store = GraphStore::new(kv.clone());

        let mut graph = make_test_graph();
        let test_e = CodeEntity {
            id: "tests/auth_test.rs::test_authenticate".into(),
            name: "test_authenticate".into(),
            entity_type: EntityType::TestFunction,
            file_path: "tests/auth_test.rs".into(),
            line_start: 1,
            line_end: 5,
            language: Language::Rust,
            docstring: None,
            calls: vec!["src/auth.rs::authenticate_user".into()],
            called_by: vec![],
        };
        graph.add_entity(test_e);
        graph.add_call(
            "tests/auth_test.rs::test_authenticate".into(),
            "src/auth.rs::authenticate_user".into(),
        );

        store.store_graph(&graph).unwrap();
        let engine = QueryEngine::new(store, History::new(kv));
        (GraphSwarmState { engine }, dir)
    }

    // ── tool_definitions ──────────────────────────────────────────────────────

    #[test]
    fn tool_definitions_returns_six_tools() {
        assert_eq!(tool_definitions().len(), 6);
    }

    #[test]
    fn tool_names_are_correct() {
        let defs = tool_definitions();
        let names: Vec<&str> = defs.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"query_graph"));
        assert!(names.contains(&"get_callers"));
        assert!(names.contains(&"get_callees"));
        assert!(names.contains(&"shortest_path"));
        assert!(names.contains(&"explain_entity"));
        assert!(names.contains(&"find_tests"));
    }

    #[test]
    fn all_tools_have_non_empty_description() {
        for t in tool_definitions() {
            assert!(
                !t.description.is_empty(),
                "{} has empty description",
                t.name
            );
        }
    }

    #[test]
    fn all_tools_have_valid_input_schema() {
        for t in tool_definitions() {
            assert_eq!(
                t.input_schema["type"], "object",
                "{} input_schema must have type:object",
                t.name
            );
        }
    }

    // ── dispatch ──────────────────────────────────────────────────────────────

    #[test]
    fn dispatch_unknown_tool_returns_err() {
        let (state, _dir) = make_test_state();
        let result = dispatch("no_such_tool", &serde_json::json!({}), &state);
        assert!(result.is_err());
    }

    #[test]
    fn dispatch_query_graph_missing_query_returns_err() {
        let (state, _dir) = make_test_state();
        let result = dispatch("query_graph", &serde_json::json!({"top_k": 5}), &state);
        assert!(result.is_err());
    }

    #[test]
    fn dispatch_get_callers_missing_entity_id_returns_err() {
        let (state, _dir) = make_test_state();
        let result = dispatch("get_callers", &serde_json::json!({}), &state);
        assert!(result.is_err());
    }

    #[test]
    fn dispatch_query_graph_valid_args_returns_ok() {
        let (state, _dir) = make_test_state();
        let args = serde_json::json!({"query": "authenticate", "top_k": 3});
        let result = dispatch("query_graph", &args, &state);
        assert!(result.is_ok());
        assert!(!result.unwrap().is_empty());
    }

    #[test]
    fn query_graph_no_results_returns_helpful_message() {
        let (state, _dir) = make_test_state();
        let args = serde_json::json!({"query": "zzz_no_match_xyz"});
        let content = dispatch("query_graph", &args, &state).unwrap();
        assert!(!content.is_empty());
        assert!(content[0].text.contains("No results"));
    }

    #[test]
    fn get_callers_unknown_entity_returns_helpful_message() {
        let (state, _dir) = make_test_state();
        let args = serde_json::json!({"entity_id": "src/ghost.rs::nothing"});
        let content = dispatch("get_callers", &args, &state).unwrap();
        assert!(content[0].text.contains("No callers"));
    }

    #[test]
    fn explain_entity_unknown_id_returns_helpful_message() {
        let (state, _dir) = make_test_state();
        let args = serde_json::json!({"entity_id": "src/ghost.rs::nothing"});
        let content = dispatch("explain_entity", &args, &state).unwrap();
        assert!(content[0].text.contains("not found"));
    }

    #[test]
    fn shortest_path_no_path_returns_helpful_message() {
        let (state, _dir) = make_test_state();
        // verify_token does NOT call main -no path in that direction
        let args = serde_json::json!({
            "from": "src/auth.rs::verify_token",
            "to":   "src/main.rs::main"
        });
        let content = dispatch("shortest_path", &args, &state).unwrap();
        assert!(content[0].text.contains("No call path"));
    }

    #[test]
    fn query_graph_top_k_clamped_to_max_20() {
        let (state, _dir) = make_test_state();
        // top_k=100 should be clamped to 20 -no panic
        let args = serde_json::json!({"query": "authenticate", "top_k": 100});
        assert!(dispatch("query_graph", &args, &state).is_ok());
    }

    #[test]
    fn explain_entity_returns_entity_name_in_output() {
        let (state, _dir) = make_test_state();
        let args = serde_json::json!({"entity_id": "src/auth.rs::authenticate_user"});
        let content = dispatch("explain_entity", &args, &state).unwrap();
        assert!(content[0].text.contains("authenticate_user"));
    }

    #[test]
    fn get_callers_returns_caller_count_in_output() {
        let (state, _dir) = make_test_state();
        let args = serde_json::json!({"entity_id": "src/auth.rs::authenticate_user"});
        let content = dispatch("get_callers", &args, &state).unwrap();
        assert!(content[0].text.contains("1 caller"));
    }

    // ── find_tests ────────────────────────────────────────────────────────────

    #[test]
    fn find_tests_without_entity_id_lists_all_tests() {
        let (state, _dir) = make_test_state_with_tests();
        let content = dispatch("find_tests", &serde_json::json!({}), &state).unwrap();
        assert!(content[0].text.contains("test_authenticate"));
    }

    #[test]
    fn find_tests_with_entity_id_returns_covering_tests() {
        let (state, _dir) = make_test_state_with_tests();
        let args = serde_json::json!({"entity_id": "src/auth.rs::verify_token"});
        let content = dispatch("find_tests", &args, &state).unwrap();
        assert!(content[0].text.contains("test_authenticate"));
    }

    #[test]
    fn find_tests_no_results_returns_helpful_message() {
        let (state, _dir) = make_test_state();
        // make_test_graph() has no TestFunction entities at all.
        let content = dispatch("find_tests", &serde_json::json!({}), &state).unwrap();
        assert!(content[0].text.contains("No test functions found"));
    }
}
