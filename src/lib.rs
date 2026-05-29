pub mod error;
pub mod indexer;
pub mod storage;
pub mod tracker;
pub mod query;
pub mod mcp;
pub mod cli;
pub mod utils;

/// Re-exports for ergonomic library usage.
pub mod prelude {
    pub use crate::indexer::CodeIndexer;
    pub use crate::query::QueryEngine;
    pub use crate::tracker::ActionTracker;
    pub use crate::mcp::McpServer;
    pub use crate::error::{Error, Result};
}
