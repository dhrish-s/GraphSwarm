# GraphSwarm Architecture

## System Overview

GraphSwarm is built as a four-layer system that separates storage, tracking, intelligence, and agent integration.

```
┌─────────────────────────────────────────────────┐
│ Layer 4: MCP Server + Agent Integration         │
│ "What files should I load?"                    │
└────────────────────┬────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────┐
│ Layer 3: Query Engine & Learning                │
│ "Combine code graph + execution history"       │
└────────────────────┬────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────┐
│ Layer 2: Action Tracker                         │
│ "Log reads, edits, errors, test results"       │
└────────────────────┬────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────┐
│ Layer 1: KV Storage Backend                     │
│ "Fast, lock-free distributed KV store"         │
└─────────────────────────────────────────────────┘
```

## Layer 1 - KV Storage Backend

- Stores graph entities, file indexes, call relationships, and action history.
- Uses hierarchical string keys with `:` separators only.
- Values are JSON-serialized with `serde_json`.

### Key schema

- `entity:{entity_id}` → `CodeEntity`
- `file:{file_path}:entities` → `Vec<String>`
- `call:{caller_id}:{callee_id}` → `"1"`
- `callers:{entity_id}` → `Vec<String>`
- `callees:{entity_id}` → `Vec<String>`
- `meta:graph` → `GraphMetadata`
- `index:lang:{language}` → `Vec<String>`
- `index:file:{file_path}` → `Vec<String>`

## Layer 2 - Action Tracker

- Records agent actions without blocking the query path.
- Persists action history in KV storage.
- Supports file reads, file edits, function calls, test runs, and errors.

## Layer 3 - Query Engine

- Ranks files and entities by relevance to a natural language query.
- Uses graph distance, entity name match, docstring content, and recent activity.
- Returns `RelevantFile` results with a score and explanation.

## Layer 4 - MCP Server

- Exposes GraphSwarm through MCP to Claude Code and other agents.
- Defines tools for graph queries, callers/callees, shortest paths, and entity explanations.
- Runs over stdio or HTTP depending on deployment.

## Data Models

### CodeEntity

A code entity is a named item extracted from source,
including identifiers, position, language, and call relationships.

### CallGraph

A directed graph of entities and edges, with metadata about repo path, indexed time,
file count, entity count, and languages.

### AgentAction

Structured agent interactions stored for history-aware relevance.

### RelevantFile

Query results that include file path, relevance score, explanation, and related entities.

## Performance Targets

| Operation | Target |
|-----------|--------|
| Index 100-file repo | < 5 seconds |
| Single entity query | < 1 ms (p99) |
| Full graph traversal (50 files) | < 100 ms |
| Memory overhead (50-file repo) | < 100 MB |
| Binary size | < 20 MB |
