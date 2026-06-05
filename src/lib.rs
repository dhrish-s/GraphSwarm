pub mod cli;
pub mod error;
pub mod indexer;
pub mod mcp;
pub mod query;
pub mod storage;
pub mod tracker;
pub mod utils;
pub mod watcher;

/// Re-exports for ergonomic library usage.
pub mod prelude {
    pub use crate::error::{Error, Result};
    pub use crate::indexer::CodeIndexer;
    pub use crate::mcp::McpServer;
    pub use crate::query::QueryEngine;
    pub use crate::tracker::ActionTracker;
}
