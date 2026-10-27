use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn atlas() -> Command {
    Command::new(env!("CARGO_BIN_EXE_atlas"))
}

#[test]
fn search_command_indexes_and_finds_docs() {
    let repo = TempDir::new().unwrap();
    let db_dir = TempDir::new().unwrap();
    fs::write(
        repo.path().join("lib.rs"),
        "/// Parse a file.\npub fn parse_file() {}\n",
    )
    .unwrap();
    let db = db_dir.path().join("atlas.db");

    let index = atlas()
        .args([
            "index",
            repo.path().to_str().unwrap(),
            "--db",
            db.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        index.status.success(),
        "index failed: {}",
        String::from_utf8_lossy(&index.stderr)
    );

    let output = atlas()
        .args(["search", "parse", "--db", db.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "search failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("parse_file"));
    assert!(stdout.contains("pub fn parse_file()"));
    assert!(stdout.contains("Parse a file."));
}

#[test]
fn search_command_json_returns_an_empty_array() {
    let repo = TempDir::new().unwrap();
    let db_dir = TempDir::new().unwrap();
    fs::write(repo.path().join("lib.rs"), "pub fn parse_file() {}\n").unwrap();
    let db = db_dir.path().join("atlas.db");

    let index = atlas()
        .args([
            "index",
            repo.path().to_str().unwrap(),
            "--db",
            db.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(index.status.success());

    let output = atlas()
        .args([
            "search",
            "definitely-not-present",
            "--db",
            db.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "[]");
}
