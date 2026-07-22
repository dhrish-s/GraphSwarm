# GraphSwarm v0.2.0 Handoff

This document summarizes the work done for the v0.2.0 release, phase by
phase. Every phase was implemented with new tests, then passed the same
verification gate before the next phase began:

```bash
cargo test                  # all suites, zero regressions
cargo clippy -- -D warnings # 0 warnings
cargo fmt --check           # clean
```

Final state: **290 lib tests** (up from 266 in v0.1.0), all 6 integration
test binaries passing, 0 clippy warnings, fmt clean, **7.9 MB** release
binary (well under the 20 MB target).

---

## Phase 1 -  MCP subprocess path resolution (`config.toml`)

**Problem:** When an MCP host (Claude Code, Cursor, ...) launches
`graphswarm server` as a subprocess, the subprocess's working directory is
the *host's*, not the project's. `graphswarm server` defaults `--path` to
`.`, so it was looking for `.graphswarm/db` in the wrong place and failing
with "Graph not indexed."

**Fix:**
- `graphswarm install` now writes `.graphswarm/config.toml`, containing the
  project's **absolute** repo root (`src/cli/install_cmd.rs`,
  `InstallCommand::write_config`).
- `find_repo_root()` in `src/cli/server_cmd.rs`: when `--path` is left at
  its default `.`, it now reads `repo_root` from
  `.graphswarm/config.toml` in the current directory before falling back to
  `.`. An explicit `--path` is still trusted completely.
- `graphswarm install`'s default `--platform` changed from `claude` to
  `all`, so a plain `graphswarm install --project .` writes the skill files
  for every supported editor **and** `config.toml`.

---

## Phase 2 -  Top-K pre-filter for `query_warm`

**Problem:** `QueryEngine::query()` scored **every** entity in the graph on
4 signals (name, call-graph distance, recency, docstring). The recency and
docstring signals each require a sled read + JSON deserialize per entity
(`entity_by_id`), so this was O(V) sled reads per query -slow on large
graphs.

**Fix (`src/query/api.rs`):**
- New private `pre_filter(query, top_k)`: scores every entity **key**
  (cheap, no sled reads -just the trailing `::name` segment) using
  `name_score_tokens`, sorts, and keeps the top `max(top_k * 4, 20)`
  candidates.
- `query()` now calls `pre_filter()` first, then runs the full 4-signal
  scoring only on that shortlist.
- The `* 4` over-fetch (with a floor of 20) exists so an entity with a weak
  name match but a strong graph/recency/docstring signal can still surface
  in the final top-K -pre-filtering on name alone shouldn't cause rank
  inversion.
- For small graphs (fewer entities than the limit), `pre_filter` returns
  every key -behaviorally identical to the old full scan.
- New helper `name_score_tokens` / `tokenize` extracted in
  `src/query/relevance.rs` so the query string is tokenized once, not on
  every one of the V iterations.

---

## Phase 3 -  Test detection, coverage mapping, and `find_tests`

**Problem:** GraphSwarm could tell you what calls what, but not "what tests
exist?" or "what tests cover this function?" -both important for an AI
agent deciding what to run after a change.

**Fix:**
- New `EntityType::TestFunction` variant (`src/indexer/extractor.rs`),
  detected per-language during parsing (`src/indexer/parser.rs`):
  - Rust: `#[test]` / `#[tokio::test]` attributes
  - Python: function name starts with `test_` (pytest convention)
  - JS/TS: function name starts with `test` (Jest-style)
  - Go: function signature `func TestXxx(t *testing.T)` (requires
    `testing` import) -see Phase 5 below
- `GraphStore::find_all_tests()` (`src/storage/graph_queries.rs`): linear
  scan returning every `TestFunction` entity. O(V), but "what tests exist"
  is an occasional query, not a hot path.
- `GraphStore::tests_covering(entity_id)`: reverse-BFS over the call graph
  (same traversal as `impact_subtree`) to find every entity that
  transitively calls `entity_id`, filtered to `TestFunction`s. A test that
  calls a helper that calls the target still "covers" it.
- New MCP tool **`find_tests`** (`src/mcp/tools.rs`, 6th tool): with no
  arguments, lists every detected test; with `entity_id`, returns the tests
  covering it.

---

## Phase 4 -  Public library API (`src/lib.rs`)

**Problem:** GraphSwarm was usable as a CLI but not cleanly as a Rust
library -module-level docs were sparse and the `prelude` was incomplete.
This phase was documentation/API-surface only, **no logic changes**.

**Fix:**
- Crate-level doc comment on `src/lib.rs` describing the
  index → store → query pipeline, with a runnable example (doctest) that
  indexes a tiny in-memory repo, persists it, and queries it end-to-end.
- Every top-level module (`cli`, `error`, `indexer`, `mcp`, `query`,
  `storage`, `tracker`, `utils`, `watcher`) now has a one-line doc comment
  explaining its role.
- `prelude` expanded to re-export `CallGraph`, `CodeEntity`, `EntityType`,
  `GraphStore`, `KvBackend`, and `History` alongside the existing
  `CodeIndexer`, `McpServer`, `QueryEngine`, `ActionTracker` -so
  `use graphswarm::prelude::*;` is enough for the canonical pipeline.
- `src/error.rs`, `src/mcp/mod.rs`, `src/utils/mod.rs`, `src/indexer/mod.rs`
  all gained module-level doc comments.

---

## Phase 5 -  Go language support

**Problem:** GraphSwarm understood Rust, Python, JavaScript, and
TypeScript, but not Go.

**Fix (`src/indexer/parser.rs`):**
- Added `tree-sitter-go = "0.20"` dependency.
- Entity extraction: `func Name(...)` → `Function`;
  `func (recv *Type) Name(...)` → `Method` with id
  `<file>::<Type>::<Name>` (receiver type extracted via
  `go_receiver_type`).
- Test detection: `has_go_test_signature` checks for a `func TestXxx(t
  *testing.T)` signature **and** a `testing` import -a function named
  `TestHelper()` with no `*testing.T` parameter is a plain `Function`, not
  a `TestFunction`.
- Call extraction: same-file calls resolve via last-segment fallback, so
  `g.Greet()` (a selector expression) resolves to `Greeter::Greet`.
- Import extraction handles both single (`import "fmt"`) and grouped
  (`import (...)`) forms, including aliases (`myalias "path/to/pkg"`).
- 6 new tests covering functions, pointer-receiver methods, test detection
  (positive and negative), call resolution, and import extraction.

---

## Phase 6 -  HTTP transport for MCP

**Problem:** The MCP server only spoke JSON-RPC over stdio. Some hosts and
debugging workflows want to talk to it over HTTP instead.

**Fix (`src/mcp/server.rs`, `src/cli/server_cmd.rs`):**
- Extracted `dispatch_request(&self, raw: &str, state: Option<&GraphSwarmState>)
  -> serde_json::Value` as the single source of truth for "parse this raw
  JSON-RPC string, return -32700 on parse error, otherwise dispatch to
  `handle_request`." Both the stdio loop (`run`) and the new HTTP handler
  call this -identical behavior on both transports by construction.
- New `McpServer::run_http(self, port: u16)`: starts an `axum` server with
  two routes:
  - `POST /mcp` -body is a raw JSON-RPC request, handled via
    `dispatch_request`.
  - `GET /health` -plain liveness check.
- **Binds to `127.0.0.1` only**, never `0.0.0.0` -GraphSwarm has no
  authentication, and the embedded graph can expose a project's source
  structure. This is a deliberate security default for a local dev tool.
- New CLI flags on `graphswarm server`: `--http` (switch to HTTP transport,
  ignores `--watch`) and `--port` (default `3000`).
- 5 new `mcp::server` tests (dispatch parse-error/success paths, plus
  end-to-end HTTP tests using raw `tokio::net::TcpStream` requests against
  an OS-assigned port -no new HTTP-client dependency needed) and 2 new
  `cli::server_cmd` tests for the `--http`/`--port` flags.

---

## Phase 7 -  GitHub Actions release workflow

**New file:** `.github/workflows/release.yml`

- Triggers on tags matching `v*.*.*`.
- Build matrix (4 native builds, no cross-compilation toolchain needed):
  - `ubuntu-latest` → `x86_64-unknown-linux-gnu`
  - `windows-latest` → `x86_64-pc-windows-msvc`
  - `macos-13` (Intel runner) → `x86_64-apple-darwin`
  - `macos-latest` (Apple Silicon runner) → `aarch64-apple-darwin`
- Each job builds with `cargo build --release --target <target>`, packages
  the binary (`.tar.gz` on Unix, `.zip` on Windows), and uploads it as a
  build artifact.
- A final `release` job downloads all 4 artifacts and runs
  `gh release create <tag> ... --generate-notes` using the built-in
  `GITHUB_TOKEN` -no third-party release action.
- The existing `.github/workflows/ci.yml` (test/clippy/fmt/binary-size on
  push and PR) is unchanged.

**Not done as part of this phase:** actually pushing a `v*.*.*` tag to
trigger the workflow -that's a `git push` of a tag, a shared-state action
left for the developer to do explicitly.

---

## Incidental fix: flaky `successful_indexing_creates_graph` test

While running the final verification gate repeatedly, the lib test suite
intermittently failed (1 in ~3 runs) on
`cli::index_cmd::tests::successful_indexing_creates_graph`, with a
"file not found" panic reading `graphswarm_output/graph.json`.

**Root cause:** `IndexCommand::execute()` wrote its `graphswarm_output/`
directory **relative to the process's current working directory**. Two
*other* pre-existing tests in `cli::server_cmd.rs`
(`find_repo_root_uses_config_toml_in_cwd`,
`find_repo_root_falls_back_to_dot_without_config`) temporarily change the
process-wide cwd via `std::env::set_current_dir` (restored on drop). Since
`cargo test` runs tests in parallel within one process, and cwd is global
to the whole process, the index test could write its output under one
test's cwd and then look for it under another -a classic test-isolation
race, pre-dating all 7 phases.

**Fix (`src/cli/index_cmd.rs`):** `graphswarm_output/` is now created
relative to the **indexed repo path** (`repo_path.join("graphswarm_output")`)
instead of the process cwd. This directory was already
`.gitignore`'d and isn't part of the documented CLI surface (the documented
export workflow is `graphswarm export .` → `graphswarm-out/`), so this is
an internal, behavior-preserving-for-real-usage fix: when you run
`graphswarm index .` from a project root (the normal case), the output
still lands in `./graphswarm_output/` as before. Verified clean across 5
consecutive full test runs after the fix.

---

## What's out of scope for v0.2.0

Per the original plan, the following were explicitly **not** built in this
release: web dashboard/UI, Docker sandbox, evolution engine, multi-agent
support, user accounts/auth, cloud deployment, VSCode extension. These are
candidates for a future, separate "GraphSwarm-Eval" benchmark platform
project.

---

## Remaining manual steps (not done by this handoff)

1. Review the diffs (`git diff`) and the new files
   (`.github/workflows/release.yml`, this `HANDOFF.md`).
2. Commit the changes.
3. Tag and push `v0.2.0` to trigger `.github/workflows/release.yml` and
   publish the binaries referenced by the README's
   [Releases page link](https://github.com/dhrish-s/GraphSwarm/releases/tag/v0.2.0).
