use clap::Args;
use crate::error::Result;
use crate::mcp::McpServer;
use crate::storage::{GraphStore, KvBackend};
use crate::watcher::{FileWatcher, Reconciler};
use std::path::PathBuf;

#[derive(Args)]
pub struct ServerCommand {
    /// Path to repository root (where .graphswarm_db lives)
    #[arg(long, default_value = ".")]
    pub path: String,

    /// Also start a file watcher for incremental graph updates
    #[arg(long)]
    pub watch: bool,

    /// Log level
    #[arg(long, default_value = "info")]
    pub log_level: String,
}

impl ServerCommand {
    pub async fn execute(&self) -> Result<()> {
        let repo_root = PathBuf::from(&self.path);
        let db_path   = repo_root.join(".graphswarm_db");
        let server    = McpServer::new(db_path.clone());

        if !self.watch {
            // Plain stdio MCP server (existing behavior).
            server.run()
        } else {
            self.run_with_watcher(server, db_path, repo_root).await
        }
    }

    async fn run_with_watcher(
        &self,
        server: McpServer,
        db_path: PathBuf,
        repo_root: PathBuf,
    ) -> Result<()> {
        eprintln!("[graphswarm] MCP server + file watcher starting");

        // Channel for FileEvents: capacity 256 prevents blocking the watcher
        // if the reconciler falls behind.
        let (tx, rx) = tokio::sync::mpsc::channel::<crate::watcher::FileEvent>(256);

        // FileWatcher runs on a dedicated std thread (notify is synchronous).
        let watcher_root = repo_root.clone();
        let watcher_task = std::thread::spawn(move || {
            let watcher = FileWatcher::new(watcher_root);
            if let Err(e) = watcher.start(tx) {
                eprintln!("[graphswarm watcher] fatal: {e}");
            }
        });

        // Reconciler is an async task.
        let kv      = KvBackend::open(&db_path)?;
        let store   = GraphStore::new(kv);
        let reconciler = Reconciler::new(rx, store, repo_root);
        let reconciler_handle = tokio::spawn(async move { reconciler.run().await });

        // MCP server blocks on stdio; run it in a blocking thread.
        let server_handle = tokio::task::spawn_blocking(move || server.run());

        eprintln!("[graphswarm] MCP server started with file watcher");

        // Wait for either the server or reconciler to finish.
        // If the server exits (stdin closed), we shut down.
        // If the reconciler panics, we log and exit.
        tokio::select! {
            result = server_handle => {
                if let Ok(Err(e)) = result {
                    eprintln!("[graphswarm server] exited with error: {e}");
                }
            }
            result = reconciler_handle => {
                if let Err(e) = result {
                    eprintln!("[graphswarm reconciler] task error: {e}");
                }
            }
        }

        // The watcher thread will stop when the tx end drops (reconciler stopped).
        drop(watcher_task);
        Ok(())
    }
}
