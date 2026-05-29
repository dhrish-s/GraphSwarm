# GraphSwarm Architecture

## System Overview

GraphSwarm is a four-layered system:

```
Layer 4  MCP Server          ← Agent interface (Claude Code)
Layer 3  Query Engine        ← Intelligence (scoring + ranking)
Layer 2  Action Tracker      ← Awareness (logs + learning)
Layer 1  KV Storage + Index  ← Foundation (parsing + persistence)
```

## Data Models

### CodeEntity

```rust
struct CodeEntity {
    id: String,              // "payment.py::process_payment"
    name: String,            // "process_payment"
    file: String,            // "payment.py"
    entity_type: EntityType, // Function | Class | Method | Module
    signature: String,       // "def process_payment(user_id, amount)"
    calls: Vec<String>,      // IDs of functions this calls
    called_by: Vec<String>,  // IDs of functions calling this
    imports: Vec<String>,
    imported_by: Vec<String>,
    line_number: usize,
    metadata: HashMap<String, String>,
}
```

### CallGraph

Directed graph stored as two adjacency lists (forward edges + reverse edges) plus entity lookup by ID. Supports BFS/DFS traversal with configurable depth.

### AgentAction

```rust
enum AgentAction {
    FileRead  { file, timestamp, context_window, reason? }
    FileEdit  { file, timestamp, diff, test_result, lines_changed, functions_affected }
    Error     { timestamp, file, line, message }
    TestRun   { timestamp, test_file, passed, duration_ms }
}
```

### RelevantFile

Query result returned to the agent:

```rust
struct RelevantFile {
    file: String,
    relevance_score: f32,        // 0.0 – 1.0
    reason: String,              // human-readable explanation
    dependencies: Vec<String>,
    dependents: Vec<String>,
    suggested_functions: Vec<String>,
}
```

## Relevance Scoring

```
score(file) =
    semantic_match(file, task)     × 0.4
  + recency(file, history)        × 0.3
  + error_correlation(file, hist) × 0.2
  + dependency_importance(file)   × 0.1
```

- **Semantic:** keyword overlap between task description and file/entity names
- **Recency:** exponential decay (7-day half-life) from last agent touch
- **Errors:** normalised count of recent errors in this file
- **Dependencies:** normalised count of dependents (degree centrality)

## KV Schema

```
callgraph:{entity_id}          → CodeEntity (JSON)
import_graph:{file_path}       → ImportGraph (JSON)
file_index:{file_path}         → [entity_ids] (JSON)
agent:action:{timestamp_nanos} → AgentAction (JSON)
agent:task:{task_id}           → TaskRecord (JSON)
```

## MCP Tools

| Tool | Input | Output |
|------|-------|--------|
| `query_context` | task, current_file?, top_k? | ranked file list |
| `log_action` | action_type, file, result? | ack |
| `get_dependents` | file | [files] |
| `get_dependencies` | file | [files] |

## Performance Targets

| Operation | Target |
|-----------|--------|
| Index 50-file repo | < 5 s |
| Graph query | < 10 ms |
| Action log write | < 1 μs |
| Relevance query | < 50 ms |
| Memory (50 files) | < 10 MB |
