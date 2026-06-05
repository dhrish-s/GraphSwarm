# GraphSwarm

Indexes your codebase into a call graph and exposes it to AI coding assistants via MCP.

## Quick Start

**Windows**

```cmd
cd your-project
graphswarm index .
graphswarm install --project . --platform all
graphswarm server --watch
```

**Linux/Mac**

```bash
cd your-project
graphswarm index .
graphswarm install --project . --platform all
graphswarm server --watch
```

Open Claude Code, Cursor, or any MCP-compatible editor. GraphSwarm is now live.

## What It Does

GraphSwarm reads your source files and builds a bidirectional call graph showing which functions call which. The graph is persisted to an embedded database inside your project, so subsequent queries are instant. A file watcher updates the graph in real time as you edit code. Five MCP tools expose the graph to any AI editor that supports the Model Context Protocol.

## Installation

### Option A -Download pre-built binary

Download from the [GitHub Releases page][RELEASES_URL].

- Windows: `graphswarm.exe`
- Linux/Mac: `graphswarm`

No Rust required.

### Option B -Build from source

Requires Rust 1.75 or later.

```bash
git clone https://github.com/YOUR_USERNAME/GraphSwarm
cd GraphSwarm
cargo build --release
```

### Add to PATH

**Windows** (run as Administrator):

```cmd
setx PATH "%PATH%;D:\path\to\GraphSwarm\target\release"
```

**Linux/Mac** (add to `~/.bashrc` or `~/.zshrc`):

```bash
export PATH="$PATH:/path/to/GraphSwarm/target/release"
```

## Using on a Project

1. **Index the project**

   ```bash
   graphswarm index .
   ```

2. **Install skill files**

   ```bash
   graphswarm install --project . --platform all
   ```

   Writes:

   - `.claude/skills/graphswarm/SKILL.md` -Claude Code
   - `.cursor/rules/graphswarm.mdc` -Cursor
   - `AGENTS.md` -Codex agents

3. **Start the server**

   ```bash
   graphswarm server --watch
   ```

4. **Open your AI editor** -it reads the skill files automatically.

5. **Export a visual graph** (optional)

   ```bash
   graphswarm export .
   ```

   Then open `graphswarm-out/graph.html` in a browser.

## MCP Tools

| Tool | Input | When to use |
|------|-------|-------------|
| `query_graph` | natural language query | Find relevant files for a topic |
| `get_callers` | entity_id | What calls this function? |
| `get_callees` | entity_id | What does this function call? |
| `shortest_path` | two entity_ids | How does A reach B? |
| `explain_entity` | entity_id | Full details about a function |

Entity IDs follow the format `file_path::function_name`. Example: `src/auth.rs::authenticate_user`. Use forward slashes on all platforms -GraphSwarm normalizes automatically on Windows.

## CLI Reference

```bash
# Index a repository (run from project root)
graphswarm index .

# Query without starting the server
graphswarm query "authentication flow"
graphswarm query "database layer" --index .graphswarm/db

# Start MCP server
graphswarm server
graphswarm server --watch        # with live file watcher

# Export visual graph
graphswarm export .

# Install skill files
graphswarm install --project .                     # Claude Code only
graphswarm install --project . --platform all      # all editors
graphswarm install --project . --platform cursor   # Cursor only
graphswarm install --project . --platform codex    # AGENTS.md only
graphswarm install                                 # write to home dir

# Build
cargo build --release
```

## Multiple Projects

Each project gets its own `.graphswarm/db/` database inside its root directory. Run `graphswarm index .` from that project's root to build or refresh its graph. The server always reads from the `.graphswarm/db/` in the directory where it was started. Projects are fully independent -indexing one project has no effect on another.

## Supported Languages

| Language | Status |
|----------|--------|
| Rust | ✅ Supported |
| Python | ✅ Supported |
| JavaScript | ✅ Supported |
| TypeScript | ✅ Supported |
| Go | 🔜 Planned (v0.2.0) |

## Troubleshooting

**Server returns "Graph not indexed"**

Delete the database and re-index:

```powershell
# Windows
Remove-Item -Recurse -Force .graphswarm
graphswarm index .
```

```bash
# Linux/Mac
rm -rf .graphswarm
graphswarm index .
```

**`query --index` flag needed**

This flag is only needed if you indexed a subdirectory instead of the project root. Always run `graphswarm index .` from the project root to avoid this.

**`graph.html` shows nothing on first open**

Make sure you ran `graphswarm export .` from the project root after indexing.

## Build Stats

- Tests: 266 passing
- Warnings: 0
- Binary size: 7.3 MB
- CI: GitHub Actions (`.github/workflows/ci.yml`)

## License

MIT
