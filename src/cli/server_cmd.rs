use crate::error::Result;
use crate::mcp::McpServer;
use crate::storage::{GraphStore, KvBackend};
use crate::watcher::{FileWatcher, Reconciler};
use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
pub struct ServerCommand {
    /// Path to repository root (where .graphswarm/db lives)
    #[arg(long, default_value = ".")]
    pub path: String,

    /// Also start a file watcher for incremental graph updates
    #[arg(long)]
    pub watch: bool,

    /// Serve the MCP protocol over HTTP (POST /mcp, GET /health) instead of
    /// stdio. Binds to 127.0.0.1 only -ignores --watch.
    #[arg(long)]
    pub http: bool,

    /// Port to bind when --http is set.
    #[arg(long, default_value_t = 3000)]
    pub port: u16,

    /// Log level
    #[arg(long, default_value = "info")]
    pub log_level: String,
}

impl ServerCommand {
    pub async fn execute(&self) -> Result<()> {
        let repo_root = find_repo_root(&self.path);
        let db_path = repo_root.join(".graphswarm").join("db");
        let server = McpServer::new(db_path.clone());

        if self.http {
            return server.run_http(self.port).await;
        }

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
        let kv = KvBackend::open(&db_path)?;
        let store = GraphStore::new(kv);
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

/// Determines the repository root containing `.graphswarm/db`.
///
/// MCP hosts (e.g. Claude Code) launch `graphswarm server` as a subprocess
/// using their own working directory, not the project's. When `--path`
/// is left at its default (`.`), this falls back to reading
/// `repo_root` from `.graphswarm/config.toml` (written by `graphswarm
/// install`) so the server can still find the right database.
fn find_repo_root(explicit_path: &str) -> PathBuf {
    // If --path was explicitly set to something other than ".",
    // trust it completely
    if explicit_path != "." {
        return PathBuf::from(explicit_path);
    }
    // Check if config.toml exists in current directory
    let config_path = PathBuf::from(".graphswarm").join("config.toml");
    if config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("repo_root") {
                    if let Some((_, val)) = trimmed.split_once('=') {
                        let root = val.trim().trim_matches('"');
                        let p = PathBuf::from(root);
                        if p.exists() {
                            return p;
                        }
                    }
                }
            }
        }
    }
    // Fall back to current directory
    PathBuf::from(".")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    /// Process-wide cwd is global mutable state shared across cargo test's
    /// parallel test threads. Holding this for the lifetime of every
    /// `CwdGuard` serializes all cwd-changing tests in this module so one
    /// test's `set_current_dir` can never run while another is mid-test.
    static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Restores the process working directory on drop, even on panic,
    /// so a failing assertion in a cwd-changing test doesn't leak its
    /// changed cwd into other tests running in the same process.
    struct CwdGuard {
        original: PathBuf,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl CwdGuard {
        fn change_to(path: &Path) -> Self {
            let lock = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let original = std::env::current_dir().unwrap();
            std::env::set_current_dir(path).unwrap();
            CwdGuard {
                original,
                _lock: lock,
            }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.original);
        }
    }

    #[test]
    fn find_repo_root_explicit_path_is_trusted() {
        let result = find_repo_root("/some/explicit/path");
        assert_eq!(result, PathBuf::from("/some/explicit/path"));
    }

    #[test]
    fn find_repo_root_uses_config_toml_in_cwd() {
        let dir = TempDir::new().unwrap();
        let repo_root = dir.path().canonicalize().unwrap();
        let config_dir = repo_root.join(".graphswarm");
        std::fs::create_dir_all(&config_dir).unwrap();
        let root_str = repo_root.to_string_lossy().replace('\\', "/");
        std::fs::write(
            config_dir.join("config.toml"),
            format!("[graphswarm]\nrepo_root = \"{root_str}\"\nversion = \"0.2.0\"\n"),
        )
        .unwrap();

        let _guard = CwdGuard::change_to(&repo_root);
        let found = find_repo_root(".");
        // find_repo_root must resolve to the path stored in config.toml
        // (not necessarily byte-identical to the raw canonicalize() output -
        // on Windows canonicalize() returns a `\\?\` verbatim path, while
        // config.toml stores it with forward slashes).
        assert_eq!(found, PathBuf::from(&root_str));
        assert!(found.exists());
    }

    #[test]
    fn find_repo_root_handles_equals_sign_in_path() {
        // Directory names may legally contain '=' on Unix-like systems (only
        // '/' and NUL are forbidden). split('=').nth(1) would truncate the
        // value at the first '=' inside the path itself; split_once('=')
        // must keep everything after the key's own '='.
        let dir = TempDir::new().unwrap();
        let repo_root = dir.path().join("my=project");
        std::fs::create_dir_all(&repo_root).unwrap();
        let repo_root = repo_root.canonicalize().unwrap();
        let config_dir = repo_root.join(".graphswarm");
        std::fs::create_dir_all(&config_dir).unwrap();
        let root_str = repo_root.to_string_lossy().replace('\\', "/");
        std::fs::write(
            config_dir.join("config.toml"),
            format!("[graphswarm]\nrepo_root = \"{root_str}\"\nversion = \"0.2.0\"\n"),
        )
        .unwrap();

        let _guard = CwdGuard::change_to(&repo_root);
        let found = find_repo_root(".");
        assert_eq!(found, PathBuf::from(&root_str));
        assert!(found.exists());
    }

    #[test]
    fn find_repo_root_falls_back_to_dot_without_config() {
        let dir = TempDir::new().unwrap();
        let _guard = CwdGuard::change_to(dir.path());
        let found = find_repo_root(".");
        assert_eq!(found, PathBuf::from("."));
    }

    // ── --http / --port flags ────────────────────────────────────────────────

    #[test]
    fn server_command_http_and_port_default_values() {
        use crate::cli::{Cli, Commands};
        use clap::Parser;

        let cli = Cli::try_parse_from(["graphswarm", "server"]).unwrap();
        match cli.command {
            Commands::Server(cmd) => {
                assert!(!cmd.http);
                assert_eq!(cmd.port, 3000);
            }
            _ => panic!("expected Server command"),
        }
    }

    #[test]
    fn server_command_parses_http_and_port_flags() {
        use crate::cli::{Cli, Commands};
        use clap::Parser;

        let cli =
            Cli::try_parse_from(["graphswarm", "server", "--http", "--port", "8080"]).unwrap();
        match cli.command {
            Commands::Server(cmd) => {
                assert!(cmd.http);
                assert_eq!(cmd.port, 8080);
            }
            _ => panic!("expected Server command"),
        }
    }
}
