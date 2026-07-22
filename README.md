GraphSwarm indexes your codebase into a queryable call graph and exposes it to AI coding assistants via a skill file. Instead of reading hundreds of files and guessing, your AI editor queries the graph -  finding exactly which files are relevant, what calls what, and how everything connects. It understands Rust, Python, JavaScript, TypeScript, and Go out of the box, persists the graph to an embedded database so queries are instant, and ships as a single self-contained binary with no runtime dependencies. Works with Claude Code, Cursor, and any MCP-compatible editor.

---

## Quick Start

**Step 1 -  Download or build the binary**

Option A: Download a pre-built binary from the [GitHub Releases page](https://github.com/dhrish-s/GraphSwarm/releases/tag/v0.2.0) -  no Rust required.

- Windows: download `graphswarm.exe`
- Linux/Mac: download `graphswarm`, then run `chmod +x graphswarm`

Option B: Build from source (requires Rust 1.75+)

```bash
git clone https://github.com/dhrish-s/GraphSwarm
cd GraphSwarm
cargo build --release
# Binary will be at target/release/graphswarm  (or .exe on Windows)
```

---

**Step 2 -  Add to PATH (do once)**

Windows -  run Command Prompt as Administrator:

```cmd
setx PATH "%PATH%;C:\path\to\graphswarm\folder"
```

Linux/Mac -  add to `~/.bashrc` or `~/.zshrc`:

```bash
export PATH="$PATH:/path/to/graphswarm/folder"
```

After adding, open a new terminal and verify:

```bash
graphswarm --version
```

---

**Step 3 -  Index your project**

Always kill any running GraphSwarm processes before indexing to avoid database lock errors.

Windows:

```cmd
taskkill /F /IM graphswarm.exe
```

Linux/Mac:

```bash
pkill -f graphswarm
```

Then index from your project root:

```bash
graphswarm index . --exclude target,venv,node_modules,dist,build,__pycache__,.next,.graphswarm
```

Wait for **both** of these lines to appear before continuing:

```
Graph persisted to: .graphswarm/db
Action tracker started.
```

If either line is missing, kill all processes and reindex.

---

**Step 4 -  Install skill files**

```bash
graphswarm install --project .
```

`--platform all` is the default, so this writes four files into your project:

- `.claude/skills/graphswarm/SKILL.md` -  Claude Code
- `.cursor/rules/graphswarm.mdc` -  Cursor
- `AGENTS.md` -  Codex agents
- `.graphswarm/config.toml` -  records the project's absolute path, so MCP hosts that launch `graphswarm server` from a different working directory can still find the right database

Pass `--platform claude`, `--platform cursor`, or `--platform codex` to install for a single editor only.

---

**Step 5 -  Open your AI editor**

Open Claude Code, Cursor, or any MCP-compatible editor in this folder. The skill file is read automatically -  no further configuration needed.

---

**Step 6 -  Ask anything**

Your AI editor now has full call graph awareness. Try asking:

- "Find files related to authentication"
- "What calls the store_graph function?"
- "How does main reach the MCP server?"

---

## Getting Started on a New Machine

Cloning this repo onto a new machine should get you to a working `graphswarm` binary with minimal manual steps. Two paths:

### Option A: Dev Container (recommended if you use VS Code + Docker)

1. Install [Docker](https://www.docker.com/products/docker-desktop/) and the [Dev Containers VS Code extension](https://marketplace.visualstudio.com/items?itemName=ms-vscode-remote.remote-containers).
2. Open the cloned folder in VS Code and choose **"Reopen in Container"** when prompted (or run the **Dev Containers: Reopen in Container** command).
3. The container uses a pinned Rust image (`rust:1.85-bookworm`) and automatically runs `cargo build --release`, then verifies the binary with `--version` as part of `postCreateCommand`. If either step fails, the container log will show the error instead of silently succeeding.

### Option B: Manual setup script

Mac/Linux:

```bash
git clone https://github.com/dhrish-s/GraphSwarm
cd GraphSwarm
./setup.sh
```

Windows (PowerShell):

```powershell
git clone https://github.com/dhrish-s/GraphSwarm
cd GraphSwarm
.\setup.ps1
```

Each script:

- Checks whether `cargo`/`rustc` are already installed; if not, installs Rust via [rustup](https://rustup.rs/) non-interactively.
- Runs `rustup show` to confirm the toolchain is active.
- Runs `cargo build --release`.
- Confirms the binary exists at `target/release/graphswarm` (or `.exe` on Windows) and runs `graphswarm --version` to prove it actually executes, not just compiles.

Both scripts are safe to re-run if something fails partway through.

### Troubleshooting

- **`rustup` install fails or hangs**: check your network/proxy settings, then try the manual install from [rustup.rs](https://rustup.rs/) directly. If a partial install left `~/.cargo` or `~/.rustup` in a bad state, remove those directories and re-run the setup script.
- **Dev container fails to build**: make sure Docker is installed and the Docker daemon is actually running (`docker info` should succeed, not error). If Docker isn't available on your machine, use Option B (the manual setup script) instead.

---

## How It Works

GraphSwarm reads your source files using a fast native parser. It extracts every function, method, class, and import as a named entity, then detects which functions call which others. The result is a complete bidirectional call graph of your entire codebase, built in a single pass.

The call graph is persisted to an embedded database inside your project at `.graphswarm/db/`. Subsequent queries are instant -  the graph survives process restarts with no re-indexing needed unless your code changes. The optional file watcher updates individual files incrementally as you edit, keeping the graph current without a full reindex.

When your AI editor asks a question, GraphSwarm scores every entity using four signals: name match, call graph distance, recency, and docstring content. Results are ranked by file, highest score first, so the most relevant code surfaces at the top every time.

Six tools expose the graph to your AI editor. The editor calls them automatically when you ask questions about your code -  no manual commands needed. Each tool speaks the Model Context Protocol, so it works with any MCP-compatible host out of the box.

---

## MCP Tools

| Tool | What it does |
|------|-------------|
| `query_graph` | Find the most relevant files for a natural language query |
| `get_callers` | Find everything that calls a specific function |
| `get_callees` | Find everything a specific function calls |
| `shortest_path` | Find the shortest call chain between two functions |
| `explain_entity` | Get full details about any function or method |
| `find_tests` | List every detected test, or find the tests that cover a given function |

Tools that take a function name use entity IDs in the format `file_path::function_name`, or `file_path::StructName::method_name` for methods on structs. Example: `src/auth.rs::authenticate_user`. Use forward slashes on all platforms -  GraphSwarm normalizes automatically on Windows.

---

## CLI Reference

```bash
# ── Index ─────────────────────────────────────────────────────────
# Index a project from its root directory
graphswarm index .

# Exclude library and build folders (recommended)
graphswarm index . --exclude target,venv,node_modules,dist,build,__pycache__,.next,.graphswarm

# ── Query ─────────────────────────────────────────────────────────
# Query without starting the server (quick check)
graphswarm query "authentication flow"
graphswarm query "database layer"

# ── Server ────────────────────────────────────────────────────────
# Start MCP server (reads requests from stdin, writes to stdout)
graphswarm server

# Start MCP server with live file watcher
graphswarm server --watch

# Start MCP server over HTTP instead of stdio (POST /mcp, GET /health).
# Binds to 127.0.0.1 only -ignores --watch.
graphswarm server --http
graphswarm server --http --port 8080   # default port is 3000

# ── Export ────────────────────────────────────────────────────────
# Export graph.json, graph.html, and GRAPH_REPORT.md into graphswarm-out/
graphswarm export .

# ── Install ───────────────────────────────────────────────────────
# Install skill files for all editors (--platform all is the default)
graphswarm install --project .

# Install for a specific editor only
graphswarm install --project . --platform claude   # Claude Code
graphswarm install --project . --platform cursor   # Cursor
graphswarm install --project . --platform codex    # AGENTS.md only

# Install to home directory (available in all projects)
graphswarm install
```

---

## Supported Languages

| Language | Status |
|----------|--------|
| Rust | ✅ Full support |
| Python | ✅ Full support |
| JavaScript | ✅ Full support |
| TypeScript | ✅ Full support |
| Go | ✅ Full support |

---

## Multiple Projects

Each project gets its own `.graphswarm/db/` database inside its root directory. Switching projects means running `graphswarm index .` in that project's root -  there is nothing global to configure. The server always reads from the `.graphswarm/db/` in the directory where it was started, so running it from the right folder is all that is needed. Projects are fully independent: indexing one has no effect on any other.

---

## Troubleshooting

**"Graph not indexed" error**

The database was not written correctly or is missing.

Windows:

```powershell
Remove-Item -Recurse -Force .graphswarm
graphswarm index .
```

Linux/Mac:

```bash
rm -rf .graphswarm
graphswarm index .
```

---

**DB lock error during index**

Another graphswarm process is running and holding the database lock.

Windows:

```cmd
taskkill /F /IM graphswarm.exe
```

Linux/Mac:

```bash
pkill -f graphswarm
```

Then reindex immediately.

---

**Index appears to succeed but DB is missing**

The "Graph persisted" confirmation line never appeared. Both of these lines must appear after indexing:

```
Graph persisted to: .graphswarm/db
Action tracker started.
```

If either is missing, kill all processes and reindex.

---

**Query results include library code**

Third-party libraries are being indexed. Re-index with exclusions:

```bash
graphswarm index . --exclude target,venv,node_modules,dist,build,__pycache__,.next,.graphswarm
```

---

**`graphswarm` not recognized as a command**

The binary is not in PATH. Add the folder containing the binary to your PATH (see Step 2 above), then open a new terminal and verify:

```bash
graphswarm --version
```

---

**`graph.html` shows nothing in the browser**

Export was run before indexing, or from the wrong directory. Run the export from the project root after indexing:

```bash
graphswarm export .
```

Then open `graphswarm-out/graph.html` in a browser.

---

## Roadmap

**v0.2.0 -  Current release**

- Call graph indexing for Rust, Python, JavaScript, TypeScript, and Go
- 6 MCP tools: `query_graph`, `get_callers`, `get_callees`, `shortest_path`, `explain_entity`, `find_tests`
- Test-function detection and test-coverage mapping (`find_tests`)
- HTTP transport for MCP (`graphswarm server --http --port <n>`), in addition to stdio
- Top-K pre-filtering for fast queries on large graphs
- `.graphswarm/config.toml`, written by `graphswarm install`, so MCP hosts can locate the project database regardless of their working directory
- Clean public library API (`graphswarm` can be used as a Rust crate, not just a CLI)
- File watcher for live graph updates
- D3.js visual graph export (works offline)
- Skill file installation for Claude Code, Cursor, and Codex (`--platform all` by default)
- Pre-built release binaries for Windows, Linux, and macOS (Intel + Apple Silicon)
- 290 tests, 0 warnings, 7.9 MB binary

**v0.1.0 -  Previous release**

- Call graph indexing for Rust, Python, JavaScript, and TypeScript
- 5 MCP tools: `query_graph`, `get_callers`, `get_callees`, `shortest_path`, `explain_entity`
- File watcher for live graph updates
- D3.js visual graph export (works offline)
- Skill file installation for Claude Code, Cursor, and Codex
- 266 tests, 0 warnings, 7.3 MB binary

---

## Build Stats

- Tests: 290 passing, 0 failed
- Warnings: 0
- Binary size: 7.9 MB
- CI: GitHub Actions
- License: MIT

---

## License

MIT -  see [LICENSE](LICENSE) file.
