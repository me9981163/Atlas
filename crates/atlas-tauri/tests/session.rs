use atlas_tauri::AtlasState;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn write_repository() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "/// Helper.\npub fn helper() {}\n\npub fn caller() {\n    helper();\n}\n",
    )
    .unwrap();
    dir
}

#[test]
fn open_index_search_and_query_a_repository() {
    let repo = write_repository();
    let state = AtlasState::default();

    let opened = state.open(repo.path().to_str().unwrap()).unwrap();
    assert!(!opened.indexed);
    assert_eq!(opened.stats.symbol_count, 0);
    // Path::ends_with compares components, so it is separator-agnostic on Windows.
    assert!(Path::new(&opened.db_path).ends_with(".atlas/atlas.db"));
    assert!(repo.path().join(".atlas/atlas.db").is_file());

    let outcome = state.index().unwrap();
    assert_eq!(outcome.summary.files_scanned, 1);
    assert_eq!(outcome.summary.indexed, 1);
    assert_eq!(outcome.summary.symbols, 2);
    assert_eq!(outcome.summary.edges, 1);
    assert_eq!(outcome.summary.resolved_edges, 1);
    assert_eq!(outcome.summary.unresolved_edges, 0);
    assert_eq!(outcome.stats.symbol_count, 2);

    let info = state.info().unwrap().unwrap();
    assert!(info.indexed);
    assert_eq!(info.stats.symbol_count, 2);
    assert_eq!(info.stats.file_count, 1);

    let results = state.search("helper", 10).unwrap();
    assert_eq!(results.len(), 1);
    let helper = state.symbol(results[0].id).unwrap();
    assert_eq!(helper.qualified_name, "helper");
    assert_eq!(helper.doc.as_deref(), Some("Helper."));
    assert!(helper
        .signature
        .as_deref()
        .unwrap()
        .contains("pub fn helper"));

    let symbols = state.list_symbols(10).unwrap();
    assert_eq!(symbols.len(), 2);
    let caller = symbols
        .iter()
        .find(|symbol| symbol.name == "caller")
        .unwrap();

    let callers = state.callers(helper.id).unwrap();
    assert_eq!(callers.len(), 1);
    assert_eq!(callers[0].symbol_id, Some(caller.id));
    assert_eq!(callers[0].qualified_name, "caller");
    assert_eq!(callers[0].call_line, 5);

    let callees = state.callees(caller.id).unwrap();
    assert_eq!(callees.len(), 1);
    assert_eq!(callees[0].symbol_id, Some(helper.id));
    assert_eq!(callees[0].resolution.as_deref(), Some("same_file"));

    let impact = state.impact(helper.id, 5).unwrap();
    assert_eq!(impact.len(), 1);
    assert_eq!(impact[0].symbol_id, caller.id);

    let graph = state.graph(helper.id, 2, 20).unwrap();
    assert_eq!(graph.nodes.len(), 2);
    assert_eq!(graph.edges.len(), 1);
    let root = graph
        .nodes
        .iter()
        .find(|node| node.id == helper.id)
        .unwrap();
    assert_eq!(root.side, "root");
    let caller_node = graph
        .nodes
        .iter()
        .find(|node| node.id == caller.id)
        .unwrap();
    assert_eq!(caller_node.side, "caller");

    let report = state.report().unwrap();
    assert_eq!(report.edges, 1);
    assert_eq!(report.resolved, 1);
    assert_eq!(report.unresolved, 0);
}

#[test]
fn reindexing_is_incremental_and_picks_up_changes() {
    let repo = write_repository();
    let state = AtlasState::default();
    state.open(repo.path().to_str().unwrap()).unwrap();

    let first = state.index().unwrap();
    assert_eq!(first.summary.indexed, 1);
    let second = state.index().unwrap();
    assert_eq!(second.summary.indexed, 0);
    assert_eq!(second.summary.skipped_unchanged, 1);

    fs::write(
        repo.path().join("lib.rs"),
        "pub fn helper() {}\npub fn caller() { helper(); }\npub fn extra() {}\n",
    )
    .unwrap();
    let third = state.index().unwrap();
    assert_eq!(third.summary.indexed, 1);
    assert_eq!(third.summary.symbols, 3);
}

#[test]
fn close_removes_the_session_and_queries_fail_clearly() {
    let repo = write_repository();
    let state = AtlasState::default();
    state.open(repo.path().to_str().unwrap()).unwrap();
    state.index().unwrap();

    state.close();
    assert!(state.info().unwrap().is_none());
    let error = state.search("helper", 10).unwrap_err().to_string();
    assert!(error.contains("No repository is open"));
    let error = state.index().unwrap_err().to_string();
    assert!(error.contains("No repository is open"));
}

#[test]
fn invalid_repository_paths_are_rejected() {
    let state = AtlasState::default();
    assert!(state.open("").unwrap_err().to_string().contains("empty"));

    let dir = TempDir::new().unwrap();
    let file = dir.path().join("file.rs");
    fs::write(&file, "pub fn f() {}\n").unwrap();
    let error = state.open(file.to_str().unwrap()).unwrap_err().to_string();
    assert!(error.contains("not a directory"));

    let missing = dir.path().join("missing");
    let error = state
        .open(missing.to_str().unwrap())
        .unwrap_err()
        .to_string();
    assert!(error.contains("does not exist"));
}

#[test]
fn a_newer_database_is_rejected() {
    let repo = TempDir::new().unwrap();
    let atlas_dir = repo.path().join(".atlas");
    fs::create_dir_all(&atlas_dir).unwrap();
    let db_path = atlas_dir.join("atlas.db");
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE files (id INTEGER PRIMARY KEY);
             PRAGMA user_version = 99;",
        )
        .unwrap();
    }

    let state = AtlasState::default();
    let error = state
        .open(repo.path().to_str().unwrap())
        .unwrap_err()
        .to_string();
    assert!(error.contains("newer than supported version"));
}

#[test]
fn missing_symbol_ids_report_a_clear_error() {
    let repo = write_repository();
    let state = AtlasState::default();
    state.open(repo.path().to_str().unwrap()).unwrap();
    state.index().unwrap();

    let error = state.symbol(9_999_999).unwrap_err().to_string();
    assert!(error.contains("was not found"));
    assert!(error.contains("Re-index"));
    assert!(state.graph(9_999_999, 2, 20).is_err());
}
