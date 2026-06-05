# GraphSwarm

**Graph-aware persistent call graph for AI coding agents — index once, query in milliseconds.**

---

## What It Does

GraphSwarm reads your source files using tree-sitter, extracts every function, class, method, and import as a `CodeEntity`, detects call relationships, and stores a bidirectional `CallGraph` in an embedded sled KV store with no external database required. A query engine scores files against natural language queries using four weighted signals — name match, graph distance, recency, and docstring coverage — and returns ranked results with explanations. A file watcher keeps the graph in sync automatically, re-indexing only the files that changed and marking stale results so agents always know when data is fresh. An MCP (Model Context Protocol) server exposes the full graph over stdio JSON-RPC 2.0, giving AI agents five queryable tools they can call without any configuration.

---

## Installation

```bash
git clone https://github.com/dhrish-s/graphswarm
cd graphswarm
cargo build --release
```

The binary is at `target/release/graphswarm` (7.3 MB, self-contained, no runtime dependencies).

```bash
./target/release/graphswarm --help
```

---

## Quickstart

**Step 1 — Index your project**

```bash
graphswarm index ./your-project
```

This parses all `.rs`, `.py`, `.js`, and `.ts` files, extracts entities and call edges, and persists the call graph to `.graphswarm_db/` in the project root. Re-run after significant changes.

**Step 2 — Query the graph**

```bash
# Natural language — returns ranked files with scores and reasons
graphswarm query "authentication flow"

# Structured graph queries
graphswarm query callers src/auth.rs::authenticate_user
graphswarm query bfs src/main.rs::main 3
```

**Step 3 — Export a visual graph**

```bash
graphswarm export
```

Opens `graphswarm-out/graph.html` in any browser for an interactive force-directed visualization. D3.js is bundled inline — the graph renders with no internet connection.

**Step 4 — Install the skill file for your AI coding tool**

```bash
graphswarm install                      # writes to ~/.claude/skills/graphswarm/
graphswarm install --project .          # writes to .claude/skills/graphswarm/ in the current project
graphswarm install --platform cursor    # writes .cursor/rules/graphswarm.mdc
graphswarm install --platform all       # installs for Claude Code, Cursor, and Codex
```

The skill file tells the AI agent to query GraphSwarm before answering questions about the codebase.

**Step 5 — Start the MCP server**

```bash
graphswarm server             # stdio MCP server, static graph
graphswarm server --watch     # same, plus live file watcher
```

With `--watch`, the reconciler runs in the background and updates the graph within seconds of a file save. AI agents receive a `stale_warning` in query results during the brief re-indexing window.

---

## MCP Tools

The MCP server exposes five tools via JSON-RPC 2.0 over stdio.

| Tool | Description | Parameters |
|---|---|---|
| `query_graph` | Rank files by relevance to a natural language query | `query: string`, `top_k: int` (default 10) |
| `get_callers` | Find every entity that directly calls the given entity | `entity_id: string` |
| `get_callees` | Find every entity the given entity directly calls | `entity_id: string` |
| `shortest_path` | Return the shortest call chain between two entities | `from: string`, `to: string` |
| `explain_entity` | Return full details for one entity (type, file, lines, docstring, calls, callers) | `entity_id: string` |

Entity IDs follow the format `file_path::entity_name`, for example `src/auth.rs::authenticate_user`.

---

## Relevance Scoring

Each file is scored against a query using four signals combined with fixed weights:

```
relevance(file, query) =
    0.4 × name_score      — token match between query tokens and entity name
  + 0.3 × graph_score     — call graph distance, exponential decay (1.0 / 0.7 / 0.4 / 0.2 / 0.0)
  + 0.2 × recency_score   — half-life decay at 3 600 s using real access timestamps
  + 0.1 × docstring_score — token match in entity docstring
```

Every signal is in `[0.0, 1.0]`. Tokens are lowercased and split on whitespace, underscores, hyphens, colons, dots, and slashes, so `"authenticate user"` matches `authenticate_user` naturally.

The per-file score is the **maximum** entity score within that file, not the average. A file with one highly relevant function and nine unrelated ones is correctly surfaced at the top, not buried by the average.

If the file watcher has detected a change that has not yet been reconciled, the result includes a `stale_warning` field indicating that the data may be slightly out of date.

---

## File Watcher

Start the server with `--watch` to enable incremental graph updates:

```bash
graphswarm server --watch
```

The watcher uses the `notify` crate with a **500 ms debounce window**. Editors typically perform multiple write operations (write to temp, rename to target) when saving; the debounce window lets those operations complete before re-indexing begins, avoiding partial reads.

The reconciler handles four event types: **modified**, **created**, **deleted**, and **renamed**. For modifications and creations it marks the file stale, deletes its existing entities with full edge cascade, re-parses the file, stores the new entities, then clears the stale flag. For deletions it marks the file stale and removes entities permanently. Only the changed file is re-parsed — the rest of the graph is untouched.

`impact_subtree()` uses reverse BFS to identify all files that transitively call into the changed file. These can be queued for re-indexing in a future pass if cross-file edge accuracy is required. The graph is typically updated within five seconds of a file save. During the re-indexing window, query results include a `stale_warning` so agents can factor freshness into their reasoning.

---

## Supported Languages

| Language | Extracted entities |
|---|---|
| Rust | functions, `impl` methods, `struct` / `trait` declarations, `use` imports |
| Python | functions, classes, methods, `import` / `from … import` statements |
| JavaScript | function declarations, arrow functions, classes, methods, ES module imports |
| TypeScript | same as JavaScript plus type-annotated constructs |

Go support is planned for Phase 7 — the `Language::Go` variant exists in the enum and the extension mapping is reserved.

---

## KV Schema

All keys live in a single sled B-tree. Paths containing `/` or `\` are encoded with `|` to avoid key hierarchy ambiguity.

```
entity:{entity_id}               → JSON CodeEntity
callers:{entity_id}              → JSON Vec<String>   (reverse edges, pre-computed)
callees:{entity_id}              → JSON Vec<String>   (forward edges)
file:{encoded_path}:entities     → JSON Vec<String>   (entity ids in this file)
edge:{caller_id}:{callee_id}     → "1"                (O(1) existence check)
meta:graph                       → JSON GraphMetadata (repo path, counts, languages)
index:lang:{language}            → JSON Vec<String>   (entity ids by language)
stale:{encoded_path}             → "1"                (cleared after re-index)
history:recent:{rfc3339}:{uuid}  → file_path          (time-ordered, newest-last)
history:count:{encoded_path}     → JSON FileAccessCount
watcher:last_reconcile           → RFC3339 timestamp
```

Reverse edges (`callers:`) are pre-computed at write time so `find_callers()` is a single O(1) KV read regardless of graph size.

---

## Architecture

**Layer 1 — Parser.** The `src/indexer/` module uses tree-sitter grammars for Rust, Python, JavaScript, and TypeScript to parse source files into `CodeEntity` records (id, name, type, file path, line range, language, docstring, calls, callers). A two-pass algorithm resolves both intra-file and cross-file call edges using an import symbol table, building a `CallGraph` that captures the full dependency structure of the repository.

**Layer 2 — Storage.** The `src/storage/` module wraps sled, an embedded B-tree KV store, through a thin `KvBackend` that serializes values as JSON. `GraphStore` pre-computes bidirectional edge indexes at write time — writing is O(V + E), but every subsequent read is O(1). BFS and reverse BFS fan out one KV read per visited node, making traversal fast on SSD even for large graphs. `delete_file()` cascades edge cleanup across all cross-file references.

**Layer 3 — Tracker.** The `src/tracker/` module logs every agent file access through a bounded async Tokio mpsc channel. The background writer drains the channel to sled without blocking the query path. `History` provides `recent_files()`, `frequent_files()`, and `file_last_accessed()`, which extracts exact timestamps from time-ordered KV keys to give the query engine real elapsed seconds rather than an approximation.

**Layer 4 — Query Engine.** The `src/query/` module scores every entity in the graph against the query using the four-signal formula, groups results by file, takes the maximum score per file, sorts descending, and returns the top-K `RelevantFile` records. The graph distance signal uses an O(degree) approximation (depth ≤ 2) instead of full BFS per entity, keeping query latency well under 1 ms for warm graphs. Stale warnings are attached after ranking.

**Layer 5 — MCP Server.** The `src/mcp/` module implements JSON-RPC 2.0 over stdio. The server runs a blocking read-process-write loop: one line in, one line out, stdout flushed immediately after every response. With `--watch`, the MCP server runs in `tokio::task::spawn_blocking`, the file watcher on a `std::thread`, and the reconciler as a `tokio::spawn`; `tokio::select!` exits cleanly when any task terminates.

---

## Performance

| Operation | Target | Measured |
|---|---|---|
| Index 100-file repo | < 5 s | run `cargo bench` to measure |
| Index single file | < 100 ms | run `cargo bench` to measure |
| Query warm (p99) | < 1 ms | run `cargo bench` to measure |
| BFS depth 3 | < 100 ms | run `cargo bench` to measure |
| `action log()` call | < 1 μs | run `cargo bench` to measure |
| Binary size | < 20 MB | **7.3 MB** |

Run `cargo bench` to generate full HTML reports with variance analysis in `target/criterion/`. The benchmark suite covers indexing speed, query latency (warm vs cold), graph traversal (BFS, reverse BFS, `find_callers`, `find_in_file`), and action logging throughput.

---

## Development

```bash
cargo test                    # 266 tests, 0 failures
cargo clippy -- -D warnings   # 0 warnings
cargo bench                   # Criterion HTML reports in target/criterion/
cargo build --release         # 7.3 MB self-contained binary
```

**Module structure:**

```
src/indexer/   — tree-sitter parser, entity extractor, CallGraph
src/storage/   — sled KV backend, GraphStore, schema, watcher keys
src/tracker/   — async ActionLogger (Tokio mpsc), History queries
src/query/     — 4-signal relevance engine, ranker, RelevantFile
src/mcp/       — JSON-RPC 2.0 server, 5 tool handlers, protocol types
src/watcher/   — FileWatcher (notify), Reconciler, dirty queue
src/cli/       — 5 subcommands: index, query, server, export, install
benches/       — Criterion benchmarks: indexing, query, traversal, logging
tests/         — integration tests (6), watcher tests (9), module tests
```

CI runs on every push and pull request to `main` and `dev`:

```
build → test → clippy → fmt → binary size gate (< 20 MB)
```

See `.github/workflows/ci.yml`.

---

## Architecture Decisions

**Why sled over RocksDB or SQLite?**
Sled is a pure-Rust embedded B-tree with no C dependencies, which simplifies cross-compilation and keeps the binary fully self-contained. It provides sorted key iteration (enabling time-ordered prefix scans for history), ACID-like durability, and async-flush semantics at a level appropriate for a developer tool. RocksDB adds build complexity with no benefit at GraphSwarm's scale; SQLite would require schema migrations and cannot efficiently serve the prefix-scan patterns the tracker relies on.

**Why pre-compute reverse edges at write time?**
`find_callers()` is the hottest read path — agents ask "who calls this function?" far more often than they modify the graph. Pre-computing the reverse adjacency list at index time turns that read into a single O(1) KV lookup regardless of graph size. The cost is paid once at write time (O(V + E)) and amortized across every subsequent query.

**Why max score per file instead of average?**
A file containing one highly relevant function and nine irrelevant utility functions should rank near the top, not get buried by the average of its entities' scores. Agents typically navigate to the specific function they need, so surfacing the file is the right behavior. Max-per-file correctly models the question "is there anything in this file worth looking at?" while average answers the much weaker question "how relevant is the file on average?"

**Why a Tokio mpsc channel for action logging?**
Logging agent file accesses to disk on the query hot path would add unbounded latency spikes whenever sled flushes. A bounded mpsc channel makes each `log()` call a submicrosecond channel send; the background task drains to disk asynchronously. The channel capacity ensures a slow disk cannot stall the agent even during heavy I/O.

**Why stdio MCP instead of HTTP?**
Stdio eliminates every configuration problem: no port selection, no localhost binding, no firewall rules, no auth tokens. The MCP client (the AI agent's host) spawns GraphSwarm as a subprocess and owns its lifetime — when the agent exits, the server exits. For the common single-user developer workflow this is strictly simpler than HTTP with no tradeoff. An HTTP mode is planned for Phase 7 to support remote and multi-user deployments.

**Why 500 ms debounce on the file watcher?**
Most text editors write files in multiple OS operations: write to a temp path, flush, then rename atomically to the final path. Without debouncing, GraphSwarm would receive two or three raw events and potentially parse an incomplete file on the first one. A 500 ms window absorbs the full editor save sequence and triggers exactly one reconcile pass on the complete, final file.

---

## Roadmap

**Phases 1–6: Complete ✅**

The core indexer, storage layer, action tracker, query engine, MCP server, file watcher, JS/TS parser support, real benchmarks, and integration tests are all shipped.

**Phase 7 (planned):**

- **Go language parser** — tree-sitter-go grammar, function and method extraction, import resolution
- **HTTP MCP mode** — optional HTTP transport for remote agents and multi-user team deployments
- **KV-SWARM multi-tier storage** — tiered cache (GPU memory → DRAM → disk) for very large repositories
- **Web dashboard** — browser-based graph explorer built on the existing `graph.html` export
- **Semantic similarity** — optional embedding-based scoring alongside the existing token-match signals, for queries where keyword overlap is insufficient

---

## License

MIT. See [LICENSE](LICENSE).
