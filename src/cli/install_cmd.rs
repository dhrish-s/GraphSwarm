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

use clap::Args;
use crate::error::Result;
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
            "cursor"                 => self.install_cursor(&base)?,
            "codex"                  => self.install_codex(&base)?,
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
            println!("\nInstalled to home: {}/.claude/skills/graphswarm/", base.display());
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
                    "Cannot create project dir '{}': {e}", path.display()
                ))
            })?;
            Ok(path)
        } else {
            let home = std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .map_err(|_| crate::error::Error::config(
                    "Cannot find home directory. Set the HOME or USERPROFILE env var."
                ))?;
            Ok(PathBuf::from(home))
        }
    }

    fn install_claude(&self, base: &Path) -> Result<()> {
        let dir = base.join(".claude/skills/graphswarm");
        std::fs::create_dir_all(&dir).map_err(|e| {
            crate::error::Error::storage(format!("Cannot create dir '{}': {e}", dir.display()))
        })?;
        let path = dir.join("SKILL.md");
        std::fs::write(&path, self.claude_skill_content()).map_err(|e| {
            crate::error::Error::storage(format!("Cannot write SKILL.md: {e}"))
        })?;
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
        r#"# GraphSwarm -Code Graph Intelligence

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
"#.into()
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
"#.into()
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
"#.into()
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
            project:  Some(dir.path().to_str().unwrap().to_string()),
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
            project:  Some(dir.path().to_str().unwrap().to_string()),
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
            project:  Some(dir.path().to_str().unwrap().to_string()),
        };
        cmd.execute().await.unwrap();
        assert!(dir.path().join(".claude/skills/graphswarm/SKILL.md").exists());
        assert!(dir.path().join(".cursor/rules/graphswarm.mdc").exists());
        assert!(dir.path().join("AGENTS.md").exists());
    }

    #[tokio::test]
    async fn install_project_codex_idempotent() {
        let dir = TempDir::new().unwrap();
        let cmd = InstallCommand {
            platform: "codex".into(),
            project:  Some(dir.path().to_str().unwrap().to_string()),
        };
        // Run twice -AGENTS.md must not contain ## GraphSwarm twice
        cmd.execute().await.unwrap();
        cmd.execute().await.unwrap();
        let content = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        let count = content.matches("## GraphSwarm").count();
        assert_eq!(count, 1, "## GraphSwarm section must appear exactly once");
    }
}
