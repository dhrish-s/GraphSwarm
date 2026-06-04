//! Install command: write skill files for AI coding platforms.
//!
//! After running `graphswarm install`, the AI agent knows to:
//!   1. Query GraphSwarm before answering codebase questions
//!   2. How to format queries
//!   3. When to re-index
//!
//! Platform-specific paths:
//!   Claude Code  → .claude/skills/graphswarm/SKILL.md
//!   Cursor       → .cursor/rules/graphswarm.mdc
//!   Codex        → AGENTS.md (append section)
//!   Default      → Claude Code

use clap::Args;
use crate::error::Result;
use std::path::PathBuf;

#[derive(Args)]
pub struct InstallCommand {
    /// Target platform: claude (default), cursor, codex, all
    #[arg(long, default_value = "claude")]
    pub platform: String,

    /// Install into the current project directory (default: current dir)
    #[arg(long)]
    pub project: bool,
}

impl InstallCommand {
    pub async fn execute(&self) -> Result<()> {
        match self.platform.as_str() {
            "claude" | "claude-code" => self.install_claude()?,
            "cursor"                 => self.install_cursor()?,
            "codex"                  => self.install_codex()?,
            "all" => {
                self.install_claude()?;
                self.install_cursor()?;
                self.install_codex()?;
            }
            p => {
                println!("Unknown platform: '{p}'");
                println!("Supported: claude, cursor, codex, all");
                return Ok(());
            }
        }
        println!("\nInstall complete. Start the MCP server with: graphswarm server");
        Ok(())
    }

    fn install_claude(&self) -> Result<()> {
        let dir = PathBuf::from(".claude/skills/graphswarm");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("SKILL.md");
        std::fs::write(&path, self.claude_skill_content()).map_err(|e| {
            crate::error::Error::storage(format!("Cannot write SKILL.md: {e}"))
        })?;
        println!("  Claude Code: {}", path.display());
        Ok(())
    }

    fn install_cursor(&self) -> Result<()> {
        let dir = PathBuf::from(".cursor/rules");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("graphswarm.mdc");
        std::fs::write(&path, self.cursor_rules_content()).map_err(|e| {
            crate::error::Error::storage(format!("Cannot write graphswarm.mdc: {e}"))
        })?;
        println!("  Cursor: {}", path.display());
        Ok(())
    }

    fn install_codex(&self) -> Result<()> {
        let path = PathBuf::from("AGENTS.md");
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        // Avoid appending twice
        if !existing.contains("## GraphSwarm") {
            let mut content = existing;
            content.push_str(&self.codex_agents_content());
            std::fs::write(&path, content).map_err(|e| {
                crate::error::Error::storage(format!("Cannot write AGENTS.md: {e}"))
            })?;
        }
        println!("  Codex: AGENTS.md");
        Ok(())
    }

    fn claude_skill_content(&self) -> String {
        r#"# GraphSwarm — Code Graph Intelligence

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

- `query_graph` — find relevant files for a natural language query
- `get_callers` — who calls a specific function?
- `get_callees` — what does a function call?
- `shortest_path` — shortest call chain between two entities
- `explain_entity` — full details about a code entity

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
