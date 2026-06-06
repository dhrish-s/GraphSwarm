//! Install command: write skill files for AI coding platforms.
//!
//! Behavior:
//!   --project <PATH>  → write files under <PATH>
//!   (no flag)         → write files under the user's home directory
//!
//! Platform-specific paths relative to the chosen base:
//!   Claude Code  → <base>/.claude/skills/graphswarm/SKILL.md
//!   Cursor       → <base>/.cursor/rules/graphswarm.mdc
//!   Codex        → <base>/AGENTS.md (appended, idempotent)

use crate::error::Result;
use clap::Args;
use std::path::{Path, PathBuf};

#[derive(Args)]
pub struct InstallCommand {
    /// Target platform: claude (default), cursor, codex, all
    #[arg(long, default_value = "claude")]
    pub platform: String,

    /// Install into this project directory (default: user home directory)
    #[arg(long, value_name = "PATH")]
    pub project: Option<String>,
}

impl InstallCommand {
    pub async fn execute(&self) -> Result<()> {
        let base = self.resolve_base()?;

        match self.platform.as_str() {
            "claude" | "claude-code" => self.install_claude(&base)?,
            "cursor" => self.install_cursor(&base)?,
            "codex" => self.install_codex(&base)?,
            "all" => {
                self.install_claude(&base)?;
                self.install_cursor(&base)?;
                self.install_codex(&base)?;
            }
            p => {
                println!("Unknown platform: '{p}'");
                println!("Supported: claude, cursor, codex, all");
                return Ok(());
            }
        }

        if self.project.is_some() {
            println!("\nInstalled to project: {}", base.display());
        } else {
            println!(
                "\nInstalled to home: {}/.claude/skills/graphswarm/",
                base.display()
            );
        }
        println!("Start the MCP server with: graphswarm server");
        Ok(())
    }

    /// Resolves the base installation directory.
    ///
    /// - `--project <PATH>` → use that path (created if it doesn't exist)
    /// - No flag            → user home directory (USERPROFILE on Windows, HOME on Unix)
    fn resolve_base(&self) -> Result<PathBuf> {
        if let Some(p) = &self.project {
            let path = PathBuf::from(p);
            std::fs::create_dir_all(&path).map_err(|e| {
                crate::error::Error::storage(format!(
                    "Cannot create project dir '{}': {e}",
                    path.display()
                ))
            })?;
            Ok(path)
        } else {
            let home = std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .map_err(|_| {
                    crate::error::Error::config(
                        "Cannot find home directory. Set the HOME or USERPROFILE env var.",
                    )
                })?;
            Ok(PathBuf::from(home))
        }
    }

    fn install_claude(&self, base: &Path) -> Result<()> {
        let dir = base.join(".claude/skills/graphswarm");
        std::fs::create_dir_all(&dir).map_err(|e| {
            crate::error::Error::storage(format!("Cannot create dir '{}': {e}", dir.display()))
        })?;
        let path = dir.join("SKILL.md");
        std::fs::write(&path, self.claude_skill_content())
            .map_err(|e| crate::error::Error::storage(format!("Cannot write SKILL.md: {e}")))?;
        println!("  Claude Code: {}", path.display());
        Ok(())
    }

    fn install_cursor(&self, base: &Path) -> Result<()> {
        let dir = base.join(".cursor/rules");
        std::fs::create_dir_all(&dir).map_err(|e| {
            crate::error::Error::storage(format!("Cannot create dir '{}': {e}", dir.display()))
        })?;
        let path = dir.join("graphswarm.mdc");
        std::fs::write(&path, self.cursor_rules_content()).map_err(|e| {
            crate::error::Error::storage(format!("Cannot write graphswarm.mdc: {e}"))
        })?;
        println!("  Cursor: {}", path.display());
        Ok(())
    }

    fn install_codex(&self, base: &Path) -> Result<()> {
        let path = base.join("AGENTS.md");
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        if !existing.contains("## GraphSwarm") {
            let mut content = existing;
            content.push_str(&self.codex_agents_content());
            std::fs::write(&path, content).map_err(|e| {
                crate::error::Error::storage(format!("Cannot write AGENTS.md: {e}"))
            })?;
        }
        println!("  Codex: {}", path.display());
        Ok(())
    }

    fn claude_skill_content(&self) -> String {
        r#"# GraphSwarm — Code Graph Intelligence

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
"#
        .into()
    }

    fn cursor_rules_content(&self) -> String {
        r#"---
description: GraphSwarm code graph intelligence
globs: ["**/*.rs", "**/*.py", "**/*.ts", "**/*.go"]
alwaysApply: true
---

# GraphSwarm Rules

This repository is indexed by GraphSwarm. Before answering questions about
the codebase, use the GraphSwarm MCP tools to query the call graph.

## Available tools

- `query_graph` -find relevant files for a natural language query
- `get_callers` -who calls a specific function?
- `get_callees` -what does a function call?
- `shortest_path` -shortest call chain between two entities
- `explain_entity` -full details about a code entity

## Starting the server

```bash
graphswarm server
```

## Re-indexing

```bash
graphswarm index ./
```
"#
        .into()
    }

    fn codex_agents_content(&self) -> String {
        r#"
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
"#
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn install_project_claude_writes_skill_md() {
        let dir = TempDir::new().unwrap();
        let cmd = InstallCommand {
            platform: "claude".into(),
            project: Some(dir.path().to_str().unwrap().to_string()),
        };
        cmd.execute().await.unwrap();
        let path = dir.path().join(".claude/skills/graphswarm/SKILL.md");
        assert!(path.exists(), "SKILL.md must exist at {}", path.display());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("GraphSwarm"));
    }

    #[tokio::test]
    async fn install_project_cursor_writes_mdc() {
        let dir = TempDir::new().unwrap();
        let cmd = InstallCommand {
            platform: "cursor".into(),
            project: Some(dir.path().to_str().unwrap().to_string()),
        };
        cmd.execute().await.unwrap();
        let path = dir.path().join(".cursor/rules/graphswarm.mdc");
        assert!(path.exists(), ".cursor/rules/graphswarm.mdc must exist");
    }

    #[tokio::test]
    async fn install_project_all_platforms() {
        let dir = TempDir::new().unwrap();
        let cmd = InstallCommand {
            platform: "all".into(),
            project: Some(dir.path().to_str().unwrap().to_string()),
        };
        cmd.execute().await.unwrap();
        assert!(dir
            .path()
            .join(".claude/skills/graphswarm/SKILL.md")
            .exists());
        assert!(dir.path().join(".cursor/rules/graphswarm.mdc").exists());
        assert!(dir.path().join("AGENTS.md").exists());
    }

    #[tokio::test]
    async fn install_project_codex_idempotent() {
        let dir = TempDir::new().unwrap();
        let cmd = InstallCommand {
            platform: "codex".into(),
            project: Some(dir.path().to_str().unwrap().to_string()),
        };
        // Run twice -AGENTS.md must not contain ## GraphSwarm twice
        cmd.execute().await.unwrap();
        cmd.execute().await.unwrap();
        let content = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        let count = content.matches("## GraphSwarm").count();
        assert_eq!(count, 1, "## GraphSwarm section must appear exactly once");
    }
}
