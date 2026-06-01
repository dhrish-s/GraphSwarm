# GraphSwarm

> The production Rust binary that maps your codebase into a queryable knowledge graph -
> so AI coding agents stop reading 200 files and start asking smart questions.

## Why GraphSwarm?

- Single binary. Zero pip, zero Python, zero PATH setup.
- `/graphswarm .` → indexes your repo → graph ready in seconds
- Sub-millisecond queries via KV-SWARM (not a flat JSON file)
- Works with Claude Code, Cursor, Codex, and any MCP-compatible agent
- 10-50x faster than Python-based alternatives (Rust + tree-sitter natively)

## Quick Start

# Install (single binary)
```bash
cargo install graphswarm
```

# Index your repo
```bash
graphswarm index ./my-project
```

# Query the graph
```bash
graphswarm query "what calls authenticate_user?"
```

# Install skill for Claude Code
```bash
graphswarm install
```

# Start MCP server
```bash
graphswarm server --port 3000
```

## Output

```bash
graphswarm-out/
├── graph.json        ← full call graph (commit this to git)
├── graph.html        ← interactive browser visualization
└── GRAPH_REPORT.md   ← key concepts, surprising connections, suggested questions
```

## Performance

| Metric                                     | Target                         |
|-------------------------------------------|--------------------------------|
| Index 100-file repo                       | < 5 seconds                    |
| Single entity query                       | < 1 ms (p99)                   |
| Full graph traversal (50 files)           | < 100 ms                       |
| Memory overhead (50-file repo)            | < 100 MB                       |
| Binary size                               | < 20 MB                        |
