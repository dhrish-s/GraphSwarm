//! Thin wrapper around the sled embedded key-value store.
//!
//! Design decisions:
//!
//! 1. Values are JSON-serialized (serde_json). Slightly slower than binary
//!    formats (bincode, postcard) but human-readable when debugging the store.
//!    For our scale (< 500k entities), the overhead is irrelevant.
//!
//! 2. All operations return Result<T>. "Key not found" is Ok(None), not Err.
//!    Only storage failures (disk full, corruption) return Err.
//!
//! 3. KvBackend is Clone because sled::Db is Arc-backed internally — cloning
//!    just increments a reference count. Safe to share across threads.
//!
//! 4. Writes are flushed asynchronously by sled. Call flush() after bulk
//!    operations (like store_graph) to guarantee durability.

use crate::error::{Error, Result};
use serde::{de::DeserializeOwned, Serialize};
use std::path::Path;

/// Embedded key-value store backed by sled.
///
/// Cheap to clone — the underlying sled::Db uses Arc internally.
#[derive(Clone)]
pub struct KvBackend {
    db: sled::Db,
}

impl KvBackend {
    /// Opens (or creates) a sled database at the given directory path.
    ///
    /// Idempotent: safe to call multiple times with the same path.
    /// If the directory doesn't exist, sled creates it.
    pub fn open(path: &Path) -> Result<Self> {
        let db = sled::open(path).map_err(|e| {
            Error::storage(format!("Failed to open KV store at {}: {}", path.display(), e))
        })?;
        Ok(Self { db })
    }

    /// Serializes `value` as JSON and stores it under `key`.
    pub fn set<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        // serde_json::to_vec writes bytes directly — slightly faster than
        // to_string because it avoids allocating an intermediate String.
        let bytes = serde_json::to_vec(value).map_err(|e| {
            Error::serialization(format!("Failed to serialize value for key '{}': {}", key, e))
        })?;

        self.db.insert(key.as_bytes(), bytes).map_err(|e| {
            Error::storage(format!("Failed to write key '{}': {}", key, e))
        })?;

        Ok(())
    }

    /// Retrieves and deserializes a value by key.
    ///
    /// Returns Ok(None) if the key doesn't exist — this is NOT an error.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let maybe_bytes = self.db.get(key.as_bytes()).map_err(|e| {
            Error::storage(format!("Failed to read key '{}': {}", key, e))
        })?;

        match maybe_bytes {
            None => Ok(None),
            Some(bytes) => {
                // sled::IVec derefs to [u8], so &bytes gives us &[u8].
                let value = serde_json::from_slice(&bytes).map_err(|e| {
                    Error::serialization(format!(
                        "Failed to deserialize value for key '{}': {}",
                        key, e
                    ))
                })?;
                Ok(Some(value))
            }
        }
    }

    /// Deletes a key. Returns Ok(()) whether or not the key existed.
    pub fn delete(&self, key: &str) -> Result<()> {
        self.db.remove(key.as_bytes()).map_err(|e| {
            Error::storage(format!("Failed to delete key '{}': {}", key, e))
        })?;
        Ok(())
    }

    /// Returns all keys that start with `prefix`, sorted lexicographically.
    ///
    /// sled stores keys in sorted byte order (B-tree), so prefix scans are
    /// efficient — O(log n + k) where k is the number of matching keys.
    pub fn list_prefix(&self, prefix: &str) -> Result<Vec<String>> {
        let mut keys = Vec::new();

        for result in self.db.scan_prefix(prefix.as_bytes()) {
            let (key_bytes, _) = result.map_err(|e| {
                Error::storage(format!("Failed to scan prefix '{}': {}", prefix, e))
            })?;

            // Our keys are always valid UTF-8 — we construct them ourselves.
            let key = String::from_utf8(key_bytes.to_vec()).map_err(|e| {
                Error::storage(format!("Key is not valid UTF-8: {}", e))
            })?;

            keys.push(key);
        }

        Ok(keys)
    }

    /// Returns the total number of keys in the store.
    pub fn len(&self) -> usize {
        self.db.len()
    }

    /// Returns true if the store contains no keys.
    pub fn is_empty(&self) -> bool {
        self.db.is_empty()
    }

    /// Flushes pending writes to disk synchronously.
    ///
    /// sled normally flushes in the background. Call this after a bulk write
    /// to guarantee the data is durable before returning to the caller.
    pub fn flush(&self) -> Result<()> {
        // flush() returns the number of bytes flushed — we discard it.
        self.db.flush().map_err(|e| {
            Error::storage(format!("Failed to flush KV store: {}", e))
        })?;
        Ok(())
    }

    /// Returns true if the given key exists without deserializing its value.
    /// Faster than get() when you only need existence.
    pub fn contains_key(&self, key: &str) -> Result<bool> {
        self.db.contains_key(key.as_bytes()).map_err(|e| {
            Error::storage(format!("Failed to check key '{}': {}", key, e))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_backend() -> (KvBackend, TempDir) {
        let dir = TempDir::new().expect("failed to create temp dir");
        let backend = KvBackend::open(dir.path()).expect("failed to open KV backend");
        (backend, dir)
    }

    #[test]
    fn open_creates_new_store() {
        let (_backend, _dir) = temp_backend();
        // Reaching here without panic means the store opened successfully.
    }

    #[test]
    fn set_get_string_roundtrip() {
        let (backend, _dir) = temp_backend();
        backend.set("key:1", &"hello world").unwrap();
        let result: Option<String> = backend.get("key:1").unwrap();
        assert_eq!(result, Some("hello world".to_string()));
    }

    #[test]
    fn set_get_vec_roundtrip() {
        let (backend, _dir) = temp_backend();
        let ids = vec!["id1".to_string(), "id2".to_string()];
        backend.set("callers:fn", &ids).unwrap();
        let result: Option<Vec<String>> = backend.get("callers:fn").unwrap();
        assert_eq!(result.unwrap(), ids);
    }

    #[test]
    fn get_missing_key_returns_none() {
        let (backend, _dir) = temp_backend();
        let result: Option<String> = backend.get("does:not:exist").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn delete_removes_key() {
        let (backend, _dir) = temp_backend();
        backend.set("k", &"v").unwrap();
        assert!(backend.contains_key("k").unwrap());
        backend.delete("k").unwrap();
        let result: Option<String> = backend.get("k").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn delete_nonexistent_key_is_ok() {
        let (backend, _dir) = temp_backend();
        backend.delete("never:existed").unwrap();
    }

    #[test]
    fn list_prefix_filters_correctly() {
        let (backend, _dir) = temp_backend();
        backend.set("entity:a", &"1").unwrap();
        backend.set("entity:b", &"2").unwrap();
        backend.set("callers:a", &"3").unwrap();

        let keys = backend.list_prefix("entity:").unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.iter().all(|k| k.starts_with("entity:")));
    }

    #[test]
    fn list_prefix_no_match_returns_empty() {
        let (backend, _dir) = temp_backend();
        backend.set("entity:a", &"1").unwrap();
        let keys = backend.list_prefix("nomatch:").unwrap();
        assert!(keys.is_empty());
    }

    #[test]
    fn len_tracks_count() {
        let (backend, _dir) = temp_backend();
        assert_eq!(backend.len(), 0);
        backend.set("k1", &"v1").unwrap();
        backend.set("k2", &"v2").unwrap();
        assert_eq!(backend.len(), 2);
        backend.delete("k1").unwrap();
        assert_eq!(backend.len(), 1);
    }

    #[test]
    fn data_persists_across_reopen() {
        let dir = TempDir::new().unwrap();

        {
            let backend = KvBackend::open(dir.path()).unwrap();
            backend.set("persistent:key", &"still here").unwrap();
            backend.flush().unwrap();
            // backend drops here, closing the database handle
        }

        {
            let backend2 = KvBackend::open(dir.path()).unwrap();
            let result: Option<String> = backend2.get("persistent:key").unwrap();
            assert_eq!(result, Some("still here".to_string()));
        }
    }

    #[test]
    fn set_get_complex_struct() {
        #[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
        struct Inner {
            x: u32,
            tags: Vec<String>,
        }

        let (backend, _dir) = temp_backend();
        let val = Inner { x: 42, tags: vec!["a".into(), "b".into()] };
        backend.set("complex:key", &val).unwrap();
        let result: Option<Inner> = backend.get("complex:key").unwrap();
        assert_eq!(result.unwrap(), val);
    }
}
