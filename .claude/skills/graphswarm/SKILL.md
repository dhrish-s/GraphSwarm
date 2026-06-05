# GraphSwarm -Code Graph Intelligence

GraphSwarm has indexed this repository into a queryable knowledge graph.
Before answering questions about this codebase, query the graph first.

## How to use

Start the MCP server:
  graphswarm server

Then use these tools:

| Tool | When to use |
|---|---|
| `query_graph` | "what files are relevant to X?" |
| `get_callers` | "what calls function Y?" |
| `get_callees` | "what does function Y call?" |
| `shortest_path` | "how does A reach B?" |
| `explain_entity` | "full details about entity Z" |

## When to re-index

Run `graphswarm index ./` when:
- You've made significant code changes
- New files have been added or deleted
- Query results feel stale

## Query examples

```
query_graph: "authentication flow"
query_graph: "database connection handling"
get_callers: "src/auth.rs::verify_token"
shortest_path from "src/main.rs::main" to "src/db.rs::query"
explain_entity: "src/auth.rs::authenticate_user"
```
