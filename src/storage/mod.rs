//! Storage layer for GraphSwarm.
//!
//! Layer hierarchy (bottom to top):
//!
//!   sled (disk)
//!     ↑
//!   KvBackend  — thin sled wrapper, knows about bytes
//!     ↑
//!   GraphStore — knows about CodeEntity, CallGraph; pre-computes indexes
//!     ↑
//!   CLI / Query Engine (Part 4)

pub mod graph_queries;
pub mod kv_backend;
pub mod schema;

pub use graph_queries::GraphStore;
pub use kv_backend::KvBackend;
// schema functions are used via crate::storage::schema:: — no re-export needed.
