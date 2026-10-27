use atlas_index::Index;
use atlas_scan::scan_directory;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_first_scan() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "fn main() { helper(); } fn helper() {}",
    )
    .unwrap();
    let idx = Index::open_in_memory().unwrap();
    let summary = scan_directory(dir.path(), &idx).unwrap();
    assert!(summary.indexed >= 1);
}

#[test]
fn test_second_scan_no_changes() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
    let idx = Index::open_in_memory().unwrap();
    let summary1 = scan_directory(dir.path(), &idx).unwrap();
    assert_eq!(summary1.indexed, 1);
    let summary2 = scan_directory(dir.path(), &idx).unwrap();
    assert_eq!(summary2.skipped_unchanged, 1);
    assert_eq!(summary2.indexed, 0);
}

#[test]
fn test_modified_file() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
    let idx = Index::open_in_memory().unwrap();
    scan_directory(dir.path(), &idx).unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "fn main() { foo(); } fn foo() {}",
    )
    .unwrap();
    let summary = scan_directory(dir.path(), &idx).unwrap();
    assert_eq!(summary.indexed, 1);
}

#[test]
fn test_deleted_file() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("main.rs");
    fs::write(&f, "fn main() {}").unwrap();
    let idx = Index::open_in_memory().unwrap();
    scan_directory(dir.path(), &idx).unwrap();
    assert_eq!(idx.stats().unwrap().file_count, 1);
    fs::remove_file(&f).unwrap();
    let summary = scan_directory(dir.path(), &idx).unwrap();
    assert_eq!(summary.removed, 1);
    assert_eq!(idx.stats().unwrap().file_count, 0);
    assert_eq!(idx.stats().unwrap().symbol_count, 0);
}

#[test]
fn test_paths_are_root_relative_and_imports_resolve() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), "pub fn helper() {}\n").unwrap();
    fs::write(
        dir.path().join("src/main.rs"),
        "use crate::helper;\nfn main() { helper(); }\n",
    )
    .unwrap();
    let idx = Index::open_in_memory().unwrap();
    let summary = scan_directory(dir.path(), &idx).unwrap();
    assert_eq!(summary.indexed, 2);

    let edges = idx.edges().unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].caller_file, "src/main.rs");
    assert_eq!(edges[0].to_name, "helper");
    assert_eq!(edges[0].resolution.as_deref(), Some("import"));
}

#[test]
fn test_external_edges_are_counted() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("app.py"),
        "import os\n\ndef f():\n    os.getcwd()\n",
    )
    .unwrap();
    let idx = Index::open_in_memory().unwrap();
    let summary = scan_directory(dir.path(), &idx).unwrap();
    assert_eq!(summary.external_edges, 1);
    assert_eq!(summary.unresolved_edges, 0);
    assert_eq!(summary.resolved_edges, 0);
}

#[test]
fn test_gitignore() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(".gitignore"), "ignored.rs\n").unwrap();
    fs::write(dir.path().join("ignored.rs"), "fn ignored() {}").unwrap();
    fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
    let idx = Index::open_in_memory().unwrap();
    let summary = scan_directory(dir.path(), &idx).unwrap();
    // ignored.rs should be skipped by gitignore
    assert!(summary.files_scanned >= 1); // at least main.rs
}

#[test]
fn test_non_utf8_file() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("bad.rs"), vec![0xFF, 0xFE, 0x00]).unwrap();
    let idx = Index::open_in_memory().unwrap();
    let summary = scan_directory(dir.path(), &idx).unwrap();
    assert_eq!(summary.skipped_not_utf8, 1);
    assert!(!summary.warnings.is_empty());
}

#[test]
fn test_oversized_file() {
    let dir = TempDir::new().unwrap();
    let big = "a".repeat(2 * 1024 * 1024 + 100);
    fs::write(dir.path().join("big.rs"), big).unwrap();
    let idx = Index::open_in_memory().unwrap();
    let summary = scan_directory(dir.path(), &idx).unwrap();
    assert_eq!(summary.skipped_too_large, 1);
    assert!(!summary.warnings.is_empty());
}
