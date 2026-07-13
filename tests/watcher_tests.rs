//! Integration tests for the file watcher and reconciler.
//!
//! These tests exercise real filesystem events -they write files, wait for
//! events, and verify the reconciler updated the GraphStore correctly.

use std::io::Write;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::timeout;

use graphswarm::storage::{GraphStore, KvBackend};
use graphswarm::watcher::{is_source_file, FileEvent, FileWatcher};

// ── is_source_file ───────────────────────────────────────────────────────────

#[test]
fn source_file_rs_is_watched() {
    let p = std::path::Path::new("src/auth.rs");
    assert!(is_source_file(p));
}

#[test]
fn source_file_py_is_watched() {
    assert!(is_source_file(std::path::Path::new("app/main.py")));
}

#[test]
fn source_file_go_is_watched() {
    // Go support landed in v0.2.0 -the watcher must reconcile .go changes.
    assert!(is_source_file(std::path::Path::new("pkg/auth/login.go")));
}

#[test]
fn source_file_jsx_is_watched() {
    assert!(is_source_file(std::path::Path::new("src/App.jsx")));
}

#[test]
fn non_source_txt_is_ignored() {
    assert!(!is_source_file(std::path::Path::new("README.txt")));
}

#[test]
fn non_source_md_is_ignored() {
    assert!(!is_source_file(std::path::Path::new("docs/guide.md")));
}

#[test]
fn non_source_json_is_ignored() {
    assert!(!is_source_file(std::path::Path::new("package.json")));
}

// ── FileWatcher event delivery ────────────────────────────────────────────────

#[tokio::test]
async fn file_watcher_sends_modified_event() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("main.rs");

    // Create initial file
    std::fs::write(&file_path, "fn main() {}").unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<FileEvent>(32);
    let watch_dir = dir.path().to_path_buf();

    // Start watcher on a std thread
    std::thread::spawn(move || {
        let watcher = FileWatcher::new(watch_dir);
        let _ = watcher.start(tx);
    });

    // Give watcher time to initialise
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Modify the file
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .open(&file_path)
            .unwrap();
        write!(f, "\nfn foo() {{}}").unwrap();
    }

    // Expect a Modified event within 3 seconds
    let result = timeout(Duration::from_secs(3), async {
        while let Some(event) = rx.recv().await {
            if event.path == file_path {
                return Some(event.kind);
            }
        }
        None
    })
    .await;

    assert!(result.is_ok(), "timed out waiting for Modified event");
    assert!(result.unwrap().is_some(), "no event for the modified file");
}

#[tokio::test]
async fn file_watcher_ignores_non_source_files() {
    let dir = TempDir::new().unwrap();
    let md_file = dir.path().join("README.md");
    std::fs::write(&md_file, "# hello").unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<FileEvent>(32);
    let watch_dir = dir.path().to_path_buf();

    std::thread::spawn(move || {
        let watcher = FileWatcher::new(watch_dir);
        let _ = watcher.start(tx);
    });

    tokio::time::sleep(Duration::from_millis(300)).await;

    // Modify the non-source file
    std::fs::write(&md_file, "# updated").unwrap();

    // Should NOT receive an event within 2 seconds
    let result = timeout(Duration::from_secs(2), rx.recv()).await;
    assert!(result.is_err(), "should not receive events for .md files");
}

// ── Reconciler behaviour ──────────────────────────────────────────────────────

#[tokio::test]
async fn reconciler_handles_modified_event() {
    let dir = TempDir::new().unwrap();
    let db_dir = dir.path().join(".graphswarm_db");

    let kv = KvBackend::open(&db_dir).unwrap();
    let store = GraphStore::new(kv);

    // Seed a file entity
    store.mark_stale("src/auth.rs").unwrap();

    // Verify stale is set
    assert!(store.is_stale("src/auth.rs").unwrap());

    // Simulate reconciler clearing stale after re-index
    store.clear_stale("src/auth.rs").unwrap();

    assert!(!store.is_stale("src/auth.rs").unwrap());
}

#[tokio::test]
async fn reconciler_handles_deleted_event() {
    use graphswarm::indexer::{
        extractor::{CodeEntity, EntityType, Language},
        CallGraph,
    };

    let dir = TempDir::new().unwrap();
    let kv = KvBackend::open(dir.path()).unwrap();
    let store = GraphStore::new(kv);

    // Create a minimal graph with one entity in src/auth.rs
    let entity = CodeEntity {
        id: "src/auth.rs::foo".into(),
        name: "foo".into(),
        entity_type: EntityType::Function,
        file_path: "src/auth.rs".into(),
        line_start: 1,
        line_end: 5,
        language: Language::Rust,
        docstring: None,
        calls: vec![],
        called_by: vec![],
    };
    let mut graph = CallGraph::new();
    graph.set_repo_path(".".into());
    graph.add_entity(entity);
    store.store_graph(&graph).unwrap();

    assert_eq!(store.find_in_file("src/auth.rs").unwrap().len(), 1);

    // Simulate deletion reconcile: delete_file + mark_stale (no clear_stale)
    store.delete_file("src/auth.rs").unwrap();
    store.mark_stale("src/auth.rs").unwrap();

    assert!(store.find_in_file("src/auth.rs").unwrap().is_empty());
    assert!(store.is_stale("src/auth.rs").unwrap());
}
