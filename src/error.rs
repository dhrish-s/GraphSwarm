use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Index error: {0}")]
    Index(String),

    #[error("Query error: {0}")]
    Query(String),

    #[error("Parser error: {0}")]
    Parser(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("MCP error: {0}")]
    Mcp(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Not found: {0}")]
    NotFound(String),

    // Separate variant for serde serialization failures inside the storage layer.
    // Distinguishing "disk I/O failed" (Storage) from "value couldn't be encoded"
    // (Serialization) makes error messages clearer and lets callers handle them differently.
    #[error("Serialization error: {0}")]
    Serialization(String),
}

impl Error {
    pub fn index(msg: impl Into<String>) -> Self { Error::Index(msg.into()) }
    pub fn query(msg: impl Into<String>) -> Self { Error::Query(msg.into()) }
    pub fn parser(msg: impl Into<String>) -> Self { Error::Parser(msg.into()) }
    pub fn storage(msg: impl Into<String>) -> Self { Error::Storage(msg.into()) }
    pub fn mcp(msg: impl Into<String>) -> Self { Error::Mcp(msg.into()) }
    pub fn config(msg: impl Into<String>) -> Self { Error::Config(msg.into()) }
    pub fn not_found(msg: impl Into<String>) -> Self { Error::NotFound(msg.into()) }
    pub fn serialization(msg: impl Into<String>) -> Self { Error::Serialization(msg.into()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let e = Error::index("bad index");
        assert_eq!(format!("{e}"), "Index error: bad index");
    }

    #[test]
    fn result_alias() {
        let ok: Result<i32> = Ok(1);
        assert!(ok.is_ok());
        let err: Result<i32> = Err(Error::parser("fail"));
        assert!(err.is_err());
    }
}
