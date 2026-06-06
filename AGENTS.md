
## GraphSwarm

This repository is indexed by GraphSwarm for call-graph-aware queries.

### Starting the MCP server

```bash
graphswarm server
```

### Querying the graph

```bash
graphswarm query "authentication flow"
graphswarm query callers src/auth.rs::verify_token
graphswarm query bfs src/main.rs::main 3
```

### Re-indexing after changes

```bash
graphswarm index ./
```
