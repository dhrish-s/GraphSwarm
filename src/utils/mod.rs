//! Shared configuration and logging setup.
//!
//! `Config` loads `.graphswarm/config.toml` (written by `graphswarm
//! install`), used to recover the repo root when GraphSwarm is launched as
//! an MCP subprocess from a different working directory. `setup_logging`
//! initializes the `tracing`/`env_logger` subscriber for the CLI.

pub mod config;
pub mod logger;

pub use config::Config;
pub use logger::setup_logging;
