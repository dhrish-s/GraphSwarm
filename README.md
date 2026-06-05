# GraphSwarm

**Persistent call graph for AI coding agents — index once, query in milliseconds over MCP.**

---

## What It Does

GraphSwarm reads your source files using tree-sitter, extracts every function, class, method, and import as a `CodeEntity`, detects call relationships, and stores a bidirectional `CallGraph` in a persistent sled KV store with no external database required. A four-signal query engine scores every entity against natural language queries using name match, graph distance, recency, and docstring coverage, then returns ranked files with scores and explanations. A file watcher keeps the graph current automatically, re-indexing only the files that changed within 500ms of a save and marking stale results so agents know when data is momentarily out of date. An MCP server exposes the full graph over stdio JSON-RPC 2.0, giving agents five queryable tools with no port configuration, no authentication, and no external processes to manage.

---

## Installation

```bash
git clone https://github.com/dhrish-s/graphswarm
cd GraphSwarm
cargo build --release
```

| Platform | Binary path |
|---|---|
| Windows | `target\release\graphswarm.exe` |
| Linux / Mac | `target/release/graphswarm` |

Verify the build:

```bash
# Windows
target\release\graphswarm.exe --help

# Linux / Mac
./target/release/graphswarm --help
```

The binary is self-contained (7.3 MB) with no runtime dependencies.

---

## Quickstart

### Step 1 — Index your project

```bash
# Windows
target\release\graphswarm.exe index .\src

# Linux / Mac
./target/release/graphswarm index ./src
```

The call graph is written to `<path>/.graphswarm/db/`. Run this once after cloning, then again after significant refactors.

### Step 2 — Query the graph

```bash
# Windows
target\release\graphswarm.exe query "authentication flow"
target\release\graphswarm.exe query "storage layer"
target\release\graphswarm.exe query "error handling"

# Linux / Mac
./target/release/graphswarm query "authentication flow"
./target/release/graphswarm query "storage layer"
./target/release/graphswarm query "error handling"
```

Returns ranked files with relevance scores (0.0–1.0), matched entity names, and a human-readable reason string explaining which signals fired.

### Step 3 — Export a visual graph

```bash
# Windows
target\release\graphswarm.exe export .\src

# Linux / Mac
./target/release/graphswarm export ./src
```

Writes three files to `graphswarm-out/`:

- `graph.html` — interactive force-directed visualization (D3.js v7.9.0 bundled inline, works fully offline, no CDN required)
- `graph.json` — full serialized `CallGraph` for machine consumption
- `GRAPH_REPORT.md` — god nodes, largest files, cross-module edges, suggested queries

### Step 4 — Install the skill file

```bash
# Windows — install to current project directory
target\release\graphswarm.exe install --project .

# Windows — install to home directory
target\release\graphswarm.exe install

# Linux / Mac — install to current project directory
./target/release/graphswarm install --project .

# Linux / Mac — install to home directory
./target/release/graphswarm install
```

`--project .` writes to `.claude/skills/graphswarm/` inside the current directory. Without the flag, writes to `~/.claude/skills/graphswarm/`. The skill file instructs the AI agent to query GraphSwarm before answering questions about the codebase.

### Step 5 — Start the MCP server

Static mode (re-run `index` manually when code changes):

```bash
# Windows
target\release\graphswarm.exe server

# Linux / Mac
./target/release/graphswarm server
```

With file watcher (graph auto-updates within 5 seconds of any file save):

```bash
# Windows
target\release\graphswarm.exe server --watch

# Linux / Mac
./target/release/graphswarm server --watch
```

---

## MCP Tools

The server exposes five tools over JSON-RPC 2.0. Entity IDs use the format `file_path::entity_name`, for example `src/auth.rs::authenticate_user`.

| Tool | Description | Required params | Optional params | Returns |
|---|---|---|---|---|
| `query_graph` | Natural language query → ranked relevant files | `query` (string) | `top_k` (int, default 10, max 20) | Ranked list of files with `relevance_score`, `reason`, `entities`, and `stale_warning` if the file has pending watcher changes |
| `get_callers` | Find all entities that call a given entity | `entity_id` (string) | — | List of caller entity IDs and names |
| `get_callees` | Find all entities that a given entity calls | `entity_id` (string) | — | List of callee entity IDs and names |
| `shortest_path` | Find the shortest call chain between two entities | `from` (string), `to` (string) | — | Ordered list of entity IDs forming the call chain |
| `explain_entity` | Full details for one entity | `entity_id` (string) | — | Complete `CodeEntity` with type, file, line range, docstring, callers, and callees |

---

## Relevance Scoring

Each query is scored against every entity in the graph using four signals with fixed weights:

```
relevance = 0.4 × name_score      (token match between query tokens and entity name)
          + 0.3 × graph_score     (call graph distance, exponential decay)
          + 0.2 × recency_score   (half-life decay using real access timestamps)
          + 0.1 × docstring_score (token match in entity docstring)
```

All signals are in `[0.0, 1.0]`. Tokens are lowercased and split on whitespace, underscores, hyphens, colons, dots, and slashes, so `"authenticate user"` matches `authenticate_user`.

**Graph distance decay:**

| Distance | Score |
|---|---|
| 0 — exact entity match | 1.0 |
| 1 — direct neighbor | 0.7 |
| 2 — neighbor's neighbor | 0.4 |
| 3+ | 0.0 |

**Per-file score** is the maximum entity score within that file, not the average. A file containing one highly relevant function and nine unrelated ones ranks correctly near the top rather than being buried by its average.

**`stale_warning`** appears in `query_graph` results when the file watcher has detected a change that has not yet been re-indexed. Agents can use this field to decide whether to re-run the query after a brief wait.

---

## File Watcher

The watcher uses the `notify` crate with a **500ms debounce window**. On `server --watch`, a `FileWatcher` thread monitors the entire repository root for changes to `.rs`, `.py`, `.js`, `.ts`, and `.tsx` files, converts OS-level events to typed `FileEvent` values, and forwards them to a `Reconciler` task over a Tokio mpsc channel. Only changed files are re-indexed — the rest of the graph is untouched.

Each event type triggers a specific reconcile sequence. For **modified** and **created** files: the file is marked stale, its existing entities are deleted with full edge cascade, the file is re-parsed, the new entities are stored, and the stale flag is cleared. For **deleted** files: the file is marked stale and all its entities and cross-references are permanently removed from the graph. For **renamed** files: the old path is treated as deleted and the new path is indexed as created.

When a file changes, `impact_subtree()` uses reverse BFS to identify every file that transitively calls into the changed file. These are the files whose call edges may now be stale. The current reconciler re-indexes the changed file immediately; the impact subtree is available for a future pass if cross-file edge accuracy is required. This keeps incremental updates fast even on large repositories because only the affected portion of the graph is touched.

---

## Supported Languages

| Language | Extensions | Extracts |
|---|---|---|
| Rust | `.rs` | functions, structs, impl methods, traits, `use` imports |
| Python | `.py` | functions, classes, methods, `import` / `from … import` statements |
| JavaScript | `.js` `.jsx` `.mjs` | function declarations, arrow functions, classes, methods, ES module imports |
| TypeScript | `.ts` `.tsx` `.mts` | same as JavaScript plus type-annotated constructs |
| Go | `.go` | planned — enum variant exists, parser not yet active |

---

## KV Schema

All data lives in a single sled B-tree at `<repo_root>/.graphswarm/db/`. Path separators and special characters are encoded with `|` to avoid key hierarchy collisions.

```
entity:{entity_id}                → JSON CodeEntity
callers:{entity_id}               → JSON Vec<String>  (pre-computed reverse edges)
callees:{entity_id}               → JSON Vec<String>  (pre-computed forward edges)
file:{path_encoded}:entities      → JSON Vec<String>  (entity IDs in this file)
edge:{caller}:{callee}            → "1"               (O(1) existence check)
meta:graph                        → JSON GraphMetadata
index:lang:{language}             → JSON Vec<String>  (entity IDs by language)
stale:{file_path}                 → "1"               (cleared after re-index)
watcher:last_reconcile            → RFC3339 timestamp
history:recent:{rfc3339}:{uuid}   → file_path string  (time-ordered, newest-last)
history:count:{file_path}         → JSON FileAccessCount
history:error:{rfc3339}:{uuid}    → JSON AgentAction  (errors only)
action:{uuid}                     → JSON AgentAction  (full record)
```

Reverse edges (`callers:` and `callees:`) are pre-computed at write time. Every read operation — including `find_callers`, `find_callees`, BFS traversal, and `explain_entity` — is a bounded number of O(1) KV lookups regardless of graph size.

---

## Architecture

**Layer 1 — Parser (`src/indexer/`).** tree-sitter grammars for Rust, Python, JavaScript, and TypeScript parse each source file into a concrete syntax tree. The extractor walks the AST to collect `CodeEntity` records (id, name, type, file path, line range, language, docstring, calls). A two-pass algorithm resolves intra-file and cross-file call edges using an import symbol table, producing a `CallGraph` that captures the full dependency structure of the repository.

**Layer 2 — Storage (`src/storage/`).** sled is a pure-Rust embedded B-tree KV store with no C FFI and no external process. `store_graph()` pre-computes both `callers:{id}` and `callees:{id}` at write time — O(E) cost at index, O(1) cost at every subsequent read. `delete_file()` cascades correctly, updating all cross-file callers and callees lists before removing entity and edge keys. The `KvBackend` abstraction is designed to support a future multi-tier storage upgrade.

**Layer 3 — Tracker (`src/tracker/`).** Every file read and edit is logged through `ActionLogger`, which places actions into a bounded Tokio mpsc channel (capacity 1000) and returns in under 1 µs. A background task owns the receiver and performs all KV writes without blocking the query path. `History` provides `recent_files()`, `frequent_files()`, and `file_last_accessed()`, the last of which extracts exact RFC3339 timestamps from time-ordered KV keys to give the query engine real elapsed seconds for recency scoring.

**Layer 4 — Query Engine (`src/query/`).** Four pure scoring functions (no I/O, no side effects) are combined with fixed weights. `rank_files()` groups scored entities by file, takes the maximum score per file, sorts descending, and returns the top-K `RelevantFile` records. `QueryEngine::query()` checks stale flags for every result file and appends a `stale_warning` where applicable. The graph distance signal uses an O(degree) approximation checking depth ≤ 2 neighbors rather than full BFS per entity.

**Layer 5 — MCP Server (`src/mcp/`).** Line-delimited JSON-RPC 2.0 over stdio: one request line in, one response line out, stdout flushed after every response. The server is spawned as a subprocess by the AI agent's host process — no port selection, no firewall rules, no authentication tokens. Five tool handlers dispatch to the query engine and graph store through a single `GraphSwarmState` struct. With `--watch`, the MCP server runs in `tokio::task::spawn_blocking`, the file watcher on a `std::thread`, and the reconciler as a `tokio::spawn` task, with a `tokio::select!` loop that exits cleanly when any component terminates.

**Layer 6 — File Watcher (`src/watcher/`).** `notify-debouncer-mini` with a 500ms debounce window sits on a dedicated `std::thread` because the `notify` crate is synchronous. It converts OS-level filesystem events to typed `FileEvent` values and sends them to the `Reconciler` over a Tokio mpsc channel. The reconciler processes each event atomically: modified files go through a mark-stale → delete → reparse → store → clear-stale sequence, and deleted files trigger cascade removal of all entities and cross-file edge references.

---

## Performance

| Operation | Target | Measured |
|---|---|---|
| Index 100-file repo | < 5 s | 49 ms |
| Index single file | < 100 ms | 878 µs |
| BFS depth 3 (1 000-node graph) | < 100 ms | 101 µs |
| Reverse BFS depth 3 | < 100 ms | 3.96 µs |
| `find_callers` | < 1 ms | 2.90 µs |
| `find_in_file` (50 entities) | < 5 ms | 102 µs |
| Query warm (500-entity graph) | < 1 ms | 9.6 ms * |
| Binary size | < 20 MB | 7.3 MB |

\* `query_warm` scans all entities in the graph to score them (O(V)). For the 500-entity benchmark graph this is 9.6 ms. The bottleneck is the full `entity_keys()` scan through sled, not the scoring math. A top-K pre-filter is planned for v0.2.

Run `cargo bench` to regenerate full HTML reports with variance analysis in `target/criterion/`.

---

## Development

```bash
cargo test                    # 266 tests, 0 failed
cargo clippy -- -D warnings   # 0 warnings
cargo bench                   # Criterion HTML reports in target/criterion/
cargo build --release         # 7.3 MB self-contained binary
```

**Module structure:**

```
src/indexer/   — tree-sitter parser, entity extractor, call graph
src/storage/   — sled KV backend, graph queries, schema
src/tracker/   — async action logger, history queries
src/query/     — 4-signal relevance engine, ranker, QueryEngine
src/mcp/       — JSON-RPC 2.0 server, 5 tool handlers
src/watcher/   — file watcher (notify), reconciler, event types
src/cli/       — 5 subcommands: index, query, server, export, install
src/utils/     — tracing setup, config
benches/       — Criterion benchmarks for all layers
tests/         — integration tests, watcher tests, module tests
```

CI runs on every push and pull request to `main` and `dev`:

```
build → test → clippy → fmt → binary size gate (< 20 MB)
```

See `.github/workflows/ci.yml`.

---

## Architecture Decisions

**Why sled over RocksDB or SQLite?** Sled is pure Rust with no C FFI, which means a single `cargo build --release` produces a fully self-contained binary on any platform without system library dependencies. It provides sorted key iteration (required for the time-ordered `history:recent:` prefix scans), ACID-like durability, and performance appropriate for a developer tool. RocksDB adds C++ build complexity with no benefit at GraphSwarm's scale; SQLite would require schema migrations and cannot efficiently serve the prefix-scan patterns the tracker relies on.

**Why pre-compute reverse edges at write time?** `find_callers()` is the hottest read path — agents ask "who calls this function?" far more often than the graph is modified. Pre-computing both `callers:{id}` and `callees:{id}` at index time turns every caller lookup into a single O(1) KV read regardless of graph size. The cost is paid once at write time (O(E)) and amortized across every subsequent query.

**Why max score per file instead of average?** A file containing one highly relevant function and nine unrelated utilities should rank near the top, not be penalized by the average of its entities' scores. Max-per-file correctly models the question "is there anything in this file worth examining?" while averaging answers the much weaker question "how relevant is the file on average?" — which consistently buries useful files behind large ones with many mediocre entities.

**Why a Tokio mpsc channel for action logging?** Writing to sled on the query hot path would add unbounded latency spikes whenever the OS flushes dirty pages. A bounded mpsc channel makes each `log()` call a sub-microsecond channel send; the background task drains to disk asynchronously. The bounded capacity of 1000 provides backpressure if the disk falls behind, preventing unbounded memory growth without ever blocking the query path under normal workloads.

**Why stdio MCP instead of HTTP?** Stdio eliminates every configuration problem: no port selection, no localhost binding, no firewall rules, no authentication tokens. The MCP client spawns GraphSwarm as a subprocess and owns its lifetime — when the client exits, GraphSwarm exits. For the single-user developer workflow this is strictly simpler than HTTP with no tradeoffs. An HTTP transport mode is planned for Phase 7 to support remote and multi-user deployments.

**Why 500ms debounce on the file watcher?** Most text editors perform multiple OS-level write operations when saving a file: write to a temporary path, sync, then rename atomically to the final path. Without debouncing, GraphSwarm would receive two or three raw events per save and potentially read a partial or empty file on the first event. A 500ms window absorbs the full editor save sequence and triggers exactly one reconcile pass on the complete, final file.

**Why is D3.js bundled inline?** Including D3.js via a CDN URL means the exported `graph.html` fails to render in any offline or air-gapped environment and produces a network request to a third-party server every time the file is opened. Bundling D3.js v7.9.0 as a compile-time string literal (279 KB minified, embedded via `include_str!`) makes the exported graph a single fully self-contained HTML file that renders correctly with no network access required, indefinitely, regardless of CDN availability.

---

## .gitignore

Add the following to your project's `.gitignore`:

```gitignore
**/.graphswarm/       # database written by the index command
graphswarm-out/       # export output (graph.html, graph.json, GRAPH_REPORT.md)
graphswarm_output/    # graph.json written during indexing
target/criterion/     # Criterion benchmark HTML reports
```

---

## Roadmap

**Phases 1–6: Complete**

The core indexer, storage layer, action tracker, query engine, MCP server, file watcher, JavaScript and TypeScript parser support, Criterion benchmarks, CI pipeline, and integration tests are all shipped.

**Phase 7 (planned):**

- **Go language parser** — the `Language::Go` enum variant and extension mapping already exist; the tree-sitter-go grammar is ready to wire in
- **HTTP MCP transport** — optional HTTP mode for remote agents and multi-user team deployments where stdio subprocess spawning is not available
- **KV-SWARM multi-tier storage** — tiered cache with a GPU hot tier, DRAM warm tier, and sled cold tier for very large repositories where the full entity set does not fit in memory
- **Top-K pre-filter in query engine** — an inverted index on entity name tokens to eliminate the O(V) full scan and bring `query_warm` latency under 1 ms
- **Web dashboard** — browser-based interactive graph explorer extending the existing `graph.html` export
- **Semantic similarity scoring** — optional embedding-based signal alongside the existing token-match signals for queries where keyword overlap is insufficient

---

## License

MIT. See [LICENSE](LICENSE).
