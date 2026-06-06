# GraphSwarm — Code Graph Intelligence

GraphSwarm has indexed this repository into a queryable call graph.
Before answering questions about this codebase, use GraphSwarm first.

## Step 0 — Find the binary
Check in this order and use whichever works:
  Windows:
    where graphswarm
    dir target\release\graphswarm.exe
  Linux/Mac:
    which graphswarm
    ls target/release/graphswarm

Use graphswarm if in PATH, otherwise use:
  Windows: ./target/release/graphswarm.exe
  Linux:   ./target/release/graphswarm

## Step 1 — Kill any running graphswarm processes
Always do this before indexing or querying to avoid DB lock errors.
  Windows: taskkill /F /IM graphswarm.exe 2>nul
  Linux:   pkill -f graphswarm 2>/dev/null

## Step 2 — Check if DB exists
  Windows: dir .graphswarm\db
  Linux:   ls .graphswarm/db

## Step 3 — If DB does not exist, index first
  graphswarm index . --exclude target,venv,node_modules,dist,build,__pycache__,.next,.graphswarm

Wait for BOTH of these lines to appear before continuing:
  Graph persisted to: .\.graphswarm\db
  Action tracker started.

If either line is missing, the DB was not written correctly.
Kill all graphswarm processes and reindex.

## Step 4 — Query using JSON-RPC pipe
IMPORTANT: Do not start the server as a background process.
Pipe a single JSON-RPC request. The server starts, answers, and exits.

Windows PowerShell:
  '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"TOOL_NAME","arguments":{ARGS}}}' | graphswarm server --path .

Linux/Mac:
  echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"TOOL_NAME","arguments":{ARGS}}}' | graphswarm server --path .

If graphswarm is not in PATH replace it with:
  Windows: ./target/release/graphswarm.exe
  Linux:   ./target/release/graphswarm

## Available tools

| Tool | Arguments | When to use |
|------|-----------|-------------|
| query_graph | query (string), top_k (int, default 5) | Find relevant files for a topic |
| get_callers | entity_id (string) | What calls this function? |
| get_callees | entity_id (string) | What does this function call? |
| shortest_path | from (string), to (string) | How does A reach B? |
| explain_entity | entity_id (string) | Full details about a function |

## Entity ID format
  file_path::function_name
  file_path::StructName::method_name   (for methods on structs)

Examples:
  src/auth.rs::authenticate_user
  src/storage/graph_queries.rs::GraphStore::store_graph
  src/mcp/server.rs::McpServer::handle_request

Use forward slashes on all platforms. GraphSwarm normalizes on Windows.

## Ready-to-use examples

query_graph (Windows PowerShell):
  '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"query_graph","arguments":{"query":"authentication flow","top_k":5}}}' | graphswarm server --path .

get_callers (Windows PowerShell):
  '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_callers","arguments":{"entity_id":"src/storage/graph_queries.rs::GraphStore::store_graph"}}}' | graphswarm server --path .

explain_entity (Windows PowerShell):
  '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"explain_entity","arguments":{"entity_id":"src/mcp/server.rs::McpServer::run"}}}' | graphswarm server --path .

query_graph (Linux/Mac):
  echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"query_graph","arguments":{"query":"authentication flow","top_k":5}}}' | graphswarm server --path .

## Troubleshooting

Problem: DB lock error during index
Fix:
  Windows: taskkill /F /IM graphswarm.exe
  Linux:   pkill -f graphswarm
  Then reindex immediately.

Problem: Graph persisted line missing after index
Fix: DB was not written. Kill all processes and reindex.

Problem: Empty results from query
Fix: Re-index with correct exclusions:
  graphswarm index . --exclude target,venv,node_modules,dist,build,__pycache__,.next,.graphswarm

## When to re-index
Run this when files have changed:
  graphswarm index . --exclude target,venv,node_modules,dist,build,__pycache__,.next,.graphswarm
