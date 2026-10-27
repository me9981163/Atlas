use atlas_core::{EdgeKind, FileSymbols, Import, RawEdge, Symbol, SymbolKind};
use atlas_index::{AccuracyReport, Index};
use rusqlite::Connection;
use tempfile::TempDir;

fn make_symbol(name: &str, qname: &str, file: &str, line: usize) -> Symbol {
    Symbol {
        name: name.to_string(),
        qualified_name: qname.to_string(),
        kind: SymbolKind::Function,
        file: file.to_string(),
        line,
        column: 0,
        signature: None,
        doc: None,
    }
}

fn make_edge(from: &str, to_name: &str, line: usize) -> RawEdge {
    RawEdge {
        from: from.to_string(),
        to_name: to_name.to_string(),
        kind: EdgeKind::Calls,
        line,
    }
}

fn make_import(module: &str, name: Option<&str>, alias: Option<&str>) -> Import {
    Import {
        module: module.to_string(),
        name: name.map(|s| s.to_string()),
        alias: alias.map(|s| s.to_string()),
        line: 1,
    }
}

fn file(
    path: &str,
    symbols: Vec<Symbol>,
    edges: Vec<RawEdge>,
    imports: Vec<Import>,
) -> FileSymbols {
    FileSymbols {
        file: path.to_string(),
        symbols,
        edges,
        imports,
    }
}

fn resolution_of(idx: &Index, caller_file: &str, to_name: &str) -> Option<String> {
    idx.edges()
        .unwrap()
        .into_iter()
        .find(|edge| edge.caller_file == caller_file && edge.to_name == to_name)
        .unwrap_or_else(|| {
            panic!(
                "no edge {} -> {} in {:?}",
                caller_file,
                to_name,
                idx.edges()
            )
        })
        .resolution
}

fn callee_of(idx: &Index, caller_file: &str, to_name: &str) -> Option<String> {
    idx.edges()
        .unwrap()
        .into_iter()
        .find(|edge| edge.caller_file == caller_file && edge.to_name == to_name)
        .unwrap()
        .callee_qualified_name
}

#[test]
fn rust_import_resolves_crate_item() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "src/lib.rs",
        "rust",
        "h1",
        &file(
            "src/lib.rs",
            vec![make_symbol("helper", "helper", "src/lib.rs", 1)],
            vec![],
            vec![],
        ),
    )
    .unwrap();
    idx.replace_file(
        "src/main.rs",
        "rust",
        "h2",
        &file(
            "src/main.rs",
            vec![make_symbol("main", "main", "src/main.rs", 3)],
            vec![make_edge("main", "helper", 4)],
            vec![make_import("crate", Some("helper"), None)],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    assert_eq!(
        resolution_of(&idx, "src/main.rs", "helper").as_deref(),
        Some("import")
    );
    assert_eq!(
        callee_of(&idx, "src/main.rs", "helper").as_deref(),
        Some("helper")
    );
}

#[test]
fn rust_import_resolves_module_path() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "src/foo.rs",
        "rust",
        "h1",
        &file(
            "src/foo.rs",
            vec![make_symbol("bar", "bar", "src/foo.rs", 1)],
            vec![],
            vec![],
        ),
    )
    .unwrap();
    idx.replace_file(
        "src/lib.rs",
        "rust",
        "h2",
        &file(
            "src/lib.rs",
            vec![make_symbol("main", "main", "src/lib.rs", 3)],
            vec![make_edge("main", "bar", 4)],
            vec![make_import("crate::foo", Some("bar"), None)],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    assert_eq!(
        resolution_of(&idx, "src/lib.rs", "bar").as_deref(),
        Some("import")
    );
}

#[test]
fn rust_module_import_resolves_qualified_call() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "src/foo.rs",
        "rust",
        "h1",
        &file(
            "src/foo.rs",
            vec![make_symbol("bar", "bar", "src/foo.rs", 1)],
            vec![],
            vec![],
        ),
    )
    .unwrap();
    idx.replace_file(
        "src/lib.rs",
        "rust",
        "h2",
        &file(
            "src/lib.rs",
            vec![make_symbol("main", "main", "src/lib.rs", 3)],
            vec![make_edge("main", "foo::bar", 4)],
            vec![make_import("crate", Some("foo"), None)],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    assert_eq!(
        resolution_of(&idx, "src/lib.rs", "foo::bar").as_deref(),
        Some("import")
    );
}

#[test]
fn rust_workspace_crate_without_src_resolves() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "crates/core/flags/parse.rs",
        "rust",
        "h1",
        &file(
            "crates/core/flags/parse.rs",
            vec![make_symbol(
                "parse_low_raw",
                "parse_low_raw",
                "crates/core/flags/parse.rs",
                1,
            )],
            vec![],
            vec![],
        ),
    )
    .unwrap();
    idx.replace_file(
        "crates/core/flags/defs.rs",
        "rust",
        "h2",
        &file(
            "crates/core/flags/defs.rs",
            vec![make_symbol(
                "caller",
                "caller",
                "crates/core/flags/defs.rs",
                1,
            )],
            vec![make_edge("caller", "parse_low_raw", 2)],
            vec![make_import(
                "crate::flags::parse",
                Some("parse_low_raw"),
                None,
            )],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    assert_eq!(
        resolution_of(&idx, "crates/core/flags/defs.rs", "parse_low_raw").as_deref(),
        Some("import")
    );
}

#[test]
fn rust_external_import_is_classified() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "src/lib.rs",
        "rust",
        "h1",
        &file(
            "src/lib.rs",
            vec![make_symbol("main", "main", "src/lib.rs", 3)],
            vec![make_edge("main", "HashMap::new", 4)],
            vec![make_import("std::collections", Some("HashMap"), None)],
        ),
    )
    .unwrap();
    let stats = idx.resolve_edges().unwrap();

    assert_eq!(stats.external, 1);
    assert_eq!(
        resolution_of(&idx, "src/lib.rs", "HashMap::new").as_deref(),
        Some("external")
    );
    assert!(callee_of(&idx, "src/lib.rs", "HashMap::new").is_none());
}

#[test]
fn same_file_and_unique_name_resolution() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "a.rs",
        "rust",
        "h1",
        &file(
            "a.rs",
            vec![
                make_symbol("caller", "caller", "a.rs", 1),
                make_symbol("helper", "helper", "a.rs", 5),
            ],
            vec![make_edge("caller", "helper", 2)],
            vec![],
        ),
    )
    .unwrap();
    idx.replace_file(
        "b.rs",
        "rust",
        "h2",
        &file(
            "b.rs",
            vec![make_symbol("caller2", "caller2", "b.rs", 1)],
            vec![make_edge("caller2", "other_helper", 2)],
            vec![],
        ),
    )
    .unwrap();
    idx.replace_file(
        "c.rs",
        "rust",
        "h3",
        &file(
            "c.rs",
            vec![make_symbol("other_helper", "other_helper", "c.rs", 1)],
            vec![],
            vec![],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    assert_eq!(
        resolution_of(&idx, "a.rs", "helper").as_deref(),
        Some("same_file")
    );
    assert_eq!(
        resolution_of(&idx, "b.rs", "other_helper").as_deref(),
        Some("unique_name")
    );
}

#[test]
fn ambiguous_edges_stay_unresolved() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "a.rs",
        "rust",
        "h1",
        &file(
            "a.rs",
            vec![make_symbol("caller", "caller", "a.rs", 1)],
            vec![make_edge("caller", "dup", 2)],
            vec![],
        ),
    )
    .unwrap();
    for path in ["b.rs", "c.rs"] {
        idx.replace_file(
            path,
            "rust",
            "h",
            &file(
                path,
                vec![make_symbol("dup", "dup", path, 1)],
                vec![],
                vec![],
            ),
        )
        .unwrap();
    }
    let stats = idx.resolve_edges().unwrap();

    assert_eq!(stats.linked, 0);
    assert_eq!(stats.ambiguous, 1);
    assert_eq!(stats.unresolved, 1);
    assert!(resolution_of(&idx, "a.rs", "dup").is_none());
}

#[test]
fn python_absolute_import_resolution() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "pkg/util.py",
        "python",
        "h1",
        &file(
            "pkg/util.py",
            vec![make_symbol("helper", "helper", "pkg/util.py", 1)],
            vec![],
            vec![],
        ),
    )
    .unwrap();
    idx.replace_file(
        "app.py",
        "python",
        "h2",
        &file(
            "app.py",
            vec![make_symbol("main", "main", "app.py", 3)],
            vec![make_edge("main", "helper", 4)],
            vec![make_import("pkg.util", Some("helper"), None)],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    assert_eq!(
        resolution_of(&idx, "app.py", "helper").as_deref(),
        Some("import")
    );
}

#[test]
fn python_relative_import_resolution() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "pkg/a.py",
        "python",
        "h1",
        &file(
            "pkg/a.py",
            vec![make_symbol("helper", "helper", "pkg/a.py", 1)],
            vec![],
            vec![],
        ),
    )
    .unwrap();
    idx.replace_file(
        "pkg/b.py",
        "python",
        "h2",
        &file(
            "pkg/b.py",
            vec![make_symbol("main", "main", "pkg/b.py", 3)],
            vec![make_edge("main", "helper", 4)],
            vec![make_import(".a", Some("helper"), None)],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    assert_eq!(
        resolution_of(&idx, "pkg/b.py", "helper").as_deref(),
        Some("import")
    );
}

#[test]
fn python_module_import_resolution() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "pkg/util.py",
        "python",
        "h1",
        &file(
            "pkg/util.py",
            vec![make_symbol("helper", "helper", "pkg/util.py", 1)],
            vec![],
            vec![],
        ),
    )
    .unwrap();
    idx.replace_file(
        "app.py",
        "python",
        "h2",
        &file(
            "app.py",
            vec![make_symbol("main", "main", "app.py", 3)],
            vec![make_edge("main", "pkg.util.helper", 4)],
            vec![make_import("pkg.util", None, None)],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    assert_eq!(
        resolution_of(&idx, "app.py", "pkg.util.helper").as_deref(),
        Some("import")
    );
}

#[test]
fn python_glob_import_resolution() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "pkg/util.py",
        "python",
        "h1",
        &file(
            "pkg/util.py",
            vec![make_symbol("helper", "helper", "pkg/util.py", 1)],
            vec![],
            vec![],
        ),
    )
    .unwrap();
    idx.replace_file(
        "app.py",
        "python",
        "h2",
        &file(
            "app.py",
            vec![make_symbol("main", "main", "app.py", 3)],
            vec![make_edge("main", "helper", 4)],
            vec![make_import("pkg.util", Some("*"), None)],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    assert_eq!(
        resolution_of(&idx, "app.py", "helper").as_deref(),
        Some("import")
    );
}

#[test]
fn python_external_import_is_classified() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "app.py",
        "python",
        "h1",
        &file(
            "app.py",
            vec![make_symbol("main", "main", "app.py", 3)],
            vec![make_edge("main", "os.getcwd", 4)],
            vec![make_import("os", None, None)],
        ),
    )
    .unwrap();
    let stats = idx.resolve_edges().unwrap();

    assert_eq!(stats.external, 1);
    assert_eq!(
        resolution_of(&idx, "app.py", "os.getcwd").as_deref(),
        Some("external")
    );
}

#[test]
fn typescript_relative_import_resolution() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "src/util.ts",
        "typescript",
        "h1",
        &file(
            "src/util.ts",
            vec![make_symbol("helper", "helper", "src/util.ts", 1)],
            vec![],
            vec![],
        ),
    )
    .unwrap();
    idx.replace_file(
        "src/app.ts",
        "typescript",
        "h2",
        &file(
            "src/app.ts",
            vec![make_symbol("main", "main", "src/app.ts", 3)],
            vec![make_edge("main", "helper", 4)],
            vec![make_import("./util", Some("helper"), None)],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    assert_eq!(
        resolution_of(&idx, "src/app.ts", "helper").as_deref(),
        Some("import")
    );
}

#[test]
fn typescript_index_file_resolution() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "src/foo/index.ts",
        "typescript",
        "h1",
        &file(
            "src/foo/index.ts",
            vec![make_symbol("helper", "helper", "src/foo/index.ts", 1)],
            vec![],
            vec![],
        ),
    )
    .unwrap();
    idx.replace_file(
        "src/app.ts",
        "typescript",
        "h2",
        &file(
            "src/app.ts",
            vec![make_symbol("main", "main", "src/app.ts", 3)],
            vec![make_edge("main", "helper", 4)],
            vec![make_import("./foo", Some("helper"), None)],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    assert_eq!(
        resolution_of(&idx, "src/app.ts", "helper").as_deref(),
        Some("import")
    );
}

#[test]
fn typescript_default_import_resolution() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "src/util.ts",
        "typescript",
        "h1",
        &file(
            "src/util.ts",
            vec![make_symbol("helper", "helper", "src/util.ts", 1)],
            vec![],
            vec![],
        ),
    )
    .unwrap();
    idx.replace_file(
        "src/app.ts",
        "typescript",
        "h2",
        &file(
            "src/app.ts",
            vec![make_symbol("main", "main", "src/app.ts", 3)],
            vec![make_edge("main", "helper", 4)],
            vec![make_import("./util", None, Some("helper"))],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    assert_eq!(
        resolution_of(&idx, "src/app.ts", "helper").as_deref(),
        Some("import")
    );
}

#[test]
fn typescript_external_import_is_classified() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "src/app.ts",
        "typescript",
        "h1",
        &file(
            "src/app.ts",
            vec![make_symbol("main", "main", "src/app.ts", 3)],
            vec![make_edge("main", "z.string", 4)],
            vec![make_import("zod", Some("z"), None)],
        ),
    )
    .unwrap();
    let stats = idx.resolve_edges().unwrap();

    assert_eq!(stats.external, 1);
    assert_eq!(
        resolution_of(&idx, "src/app.ts", "z.string").as_deref(),
        Some("external")
    );
}

#[test]
fn resolution_does_not_cross_languages() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "src/lib.rs",
        "rust",
        "h1",
        &file(
            "src/lib.rs",
            vec![make_symbol("helper", "helper", "src/lib.rs", 1)],
            vec![],
            vec![],
        ),
    )
    .unwrap();
    idx.replace_file(
        "app.py",
        "python",
        "h2",
        &file(
            "app.py",
            vec![make_symbol("helper", "helper", "app.py", 1)],
            vec![],
            vec![],
        ),
    )
    .unwrap();
    idx.replace_file(
        "src/main.rs",
        "rust",
        "h3",
        &file(
            "src/main.rs",
            vec![make_symbol("main", "main", "src/main.rs", 3)],
            vec![make_edge("main", "helper", 4)],
            vec![],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    let edge = idx
        .edges()
        .unwrap()
        .into_iter()
        .find(|edge| edge.caller_file == "src/main.rs")
        .unwrap();
    assert_eq!(edge.resolution.as_deref(), Some("unique_name"));
    assert_eq!(edge.callee_qualified_name.as_deref(), Some("helper"));
}

#[test]
fn rust_internal_glob_does_not_shadow_same_file() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "src/lib.rs",
        "rust",
        "h1",
        &file(
            "src/lib.rs",
            vec![],
            vec![],
            vec![make_import("super", None, None)],
        ),
    )
    .unwrap();
    idx.replace_file(
        "src/module.rs",
        "rust",
        "h2",
        &file(
            "src/module.rs",
            vec![
                make_symbol("caller", "caller", "src/module.rs", 1),
                make_symbol("helper", "helper", "src/module.rs", 5),
            ],
            vec![make_edge("caller", "helper", 2)],
            vec![make_import("super", None, None)],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    assert_eq!(
        resolution_of(&idx, "src/module.rs", "helper").as_deref(),
        Some("same_file")
    );
}

#[test]
fn external_glob_does_not_claim_local_calls() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "app.py",
        "python",
        "h1",
        &file(
            "app.py",
            vec![
                make_symbol("caller", "caller", "app.py", 1),
                make_symbol("helper", "helper", "app.py", 5),
            ],
            vec![make_edge("caller", "helper", 2)],
            vec![make_import("typing", Some("*"), None)],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    assert_eq!(
        resolution_of(&idx, "app.py", "helper").as_deref(),
        Some("same_file")
    );
}

#[test]
fn rust_missing_internal_import_does_not_fall_back() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "src/lib.rs",
        "rust",
        "h1",
        &file(
            "src/lib.rs",
            vec![make_symbol("helper", "helper", "src/lib.rs", 1)],
            vec![],
            vec![],
        ),
    )
    .unwrap();
    idx.replace_file(
        "src/main.rs",
        "rust",
        "h2",
        &file(
            "src/main.rs",
            vec![make_symbol("main", "main", "src/main.rs", 3)],
            vec![make_edge("main", "helper", 4)],
            vec![make_import("crate::missing", Some("helper"), None)],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    assert!(resolution_of(&idx, "src/main.rs", "helper").is_none());
}

#[test]
fn python_missing_relative_import_does_not_fall_back() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "pkg/a.py",
        "python",
        "h1",
        &file(
            "pkg/a.py",
            vec![make_symbol("helper", "helper", "pkg/a.py", 1)],
            vec![],
            vec![],
        ),
    )
    .unwrap();
    idx.replace_file(
        "pkg/b.py",
        "python",
        "h2",
        &file(
            "pkg/b.py",
            vec![make_symbol("main", "main", "pkg/b.py", 3)],
            vec![make_edge("main", "helper", 4)],
            vec![make_import(".missing", Some("helper"), None)],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    assert!(resolution_of(&idx, "pkg/b.py", "helper").is_none());
}

#[test]
fn typescript_missing_relative_import_does_not_fall_back() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "src/other.ts",
        "typescript",
        "h1",
        &file(
            "src/other.ts",
            vec![make_symbol("helper", "helper", "src/other.ts", 1)],
            vec![],
            vec![],
        ),
    )
    .unwrap();
    idx.replace_file(
        "src/app.ts",
        "typescript",
        "h2",
        &file(
            "src/app.ts",
            vec![make_symbol("main", "main", "src/app.ts", 3)],
            vec![make_edge("main", "helper", 4)],
            vec![make_import("./missing", Some("helper"), None)],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    assert!(resolution_of(&idx, "src/app.ts", "helper").is_none());
}

#[test]
fn rust_method_call_is_not_matched_to_an_import() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "src/lib.rs",
        "rust",
        "h1",
        &file(
            "src/lib.rs",
            vec![
                make_symbol("caller", "caller", "src/lib.rs", 1),
                make_symbol("read", "read", "src/lib.rs", 5),
            ],
            vec![make_edge("caller", "buf.read", 2)],
            vec![make_import("std::fs", Some("read"), None)],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    // `buf.read()` is a method call, not the imported `std::fs::read`.
    assert_eq!(
        resolution_of(&idx, "src/lib.rs", "buf.read").as_deref(),
        Some("same_file")
    );
}

#[test]
fn name_matching_is_case_sensitive() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "a.rs",
        "rust",
        "h1",
        &file(
            "a.rs",
            vec![
                make_symbol("caller", "caller", "a.rs", 1),
                make_symbol("Helper", "Helper", "a.rs", 5),
            ],
            vec![make_edge("caller", "helper", 2)],
            vec![],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    assert!(resolution_of(&idx, "a.rs", "helper").is_none());
}

#[test]
fn accuracy_report_counts_and_top_unresolved() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "src/lib.rs",
        "rust",
        "h1",
        &file(
            "src/lib.rs",
            vec![make_symbol("helper", "helper", "src/lib.rs", 1)],
            vec![],
            vec![],
        ),
    )
    .unwrap();
    idx.replace_file(
        "src/main.rs",
        "rust",
        "h2",
        &file(
            "src/main.rs",
            vec![make_symbol("main", "main", "src/main.rs", 3)],
            vec![
                make_edge("main", "helper", 4),
                make_edge("main", "HashMap::new", 5),
            ],
            vec![
                make_import("crate", Some("helper"), None),
                make_import("std::collections", Some("HashMap"), None),
            ],
        ),
    )
    .unwrap();
    idx.replace_file(
        "a.rs",
        "rust",
        "h3",
        &file(
            "a.rs",
            vec![
                make_symbol("caller", "caller", "a.rs", 1),
                make_symbol("local", "local", "a.rs", 5),
            ],
            vec![make_edge("caller", "local", 2)],
            vec![],
        ),
    )
    .unwrap();
    idx.replace_file(
        "b.rs",
        "rust",
        "h4",
        &file(
            "b.rs",
            vec![make_symbol("caller2", "caller2", "b.rs", 1)],
            vec![
                make_edge("caller2", "missing", 2),
                make_edge("caller2", "missing", 3),
                make_edge("caller2", "other", 4),
            ],
            vec![],
        ),
    )
    .unwrap();
    idx.replace_file(
        "c.rs",
        "rust",
        "h5",
        &file(
            "c.rs",
            vec![make_symbol("other", "other", "c.rs", 1)],
            vec![],
            vec![],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    let report: AccuracyReport = idx.accuracy_report().unwrap();
    assert_eq!(report.edges, 6);
    assert_eq!(report.resolved, 3);
    assert_eq!(report.import, 1);
    assert_eq!(report.same_file, 1);
    assert_eq!(report.unique_name, 1);
    assert_eq!(report.external, 1);
    assert_eq!(report.unresolved, 2);
    assert_eq!(report.top_unresolved.len(), 1);
    assert_eq!(report.top_unresolved[0].name, "missing");
    assert_eq!(report.top_unresolved[0].count, 2);
}

#[test]
fn v1_database_migrates_to_v4() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("atlas.db");

    {
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE files (
                id INTEGER PRIMARY KEY,
                path TEXT UNIQUE NOT NULL,
                language TEXT,
                content_hash TEXT
            );
            CREATE TABLE symbols (
                id INTEGER PRIMARY KEY,
                file_id INTEGER NOT NULL,
                name TEXT NOT NULL,
                qualified_name TEXT NOT NULL,
                kind TEXT NOT NULL,
                file_path TEXT NOT NULL,
                line INTEGER NOT NULL,
                column_no INTEGER NOT NULL,
                FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
            );
            CREATE TABLE edges (
                id INTEGER PRIMARY KEY,
                file_id INTEGER NOT NULL,
                from_qualified TEXT NOT NULL,
                to_name TEXT NOT NULL,
                kind TEXT NOT NULL,
                line INTEGER NOT NULL,
                dst_symbol_id INTEGER,
                FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE,
                FOREIGN KEY(dst_symbol_id) REFERENCES symbols(id)
            );
            CREATE TABLE schema_version (version INTEGER NOT NULL);
            INSERT INTO schema_version (rowid, version) VALUES (1, 1);
            PRAGMA user_version = 1;

            INSERT INTO files (id, path, language, content_hash)
                VALUES (1, 'a.rs', 'rust', 'h1');
            INSERT INTO symbols (id, file_id, name, qualified_name, kind, file_path, line, column_no)
                VALUES (1, 1, 'foo', 'foo', 'Function', 'a.rs', 1, 0);
            INSERT INTO edges (id, file_id, from_qualified, to_name, kind, line)
                VALUES (1, 1, 'foo', 'bar', 'Calls', 2);
            "#,
        )
        .unwrap();
    }

    let idx = Index::open(&db_path).unwrap();
    assert_eq!(idx.stats().unwrap().symbol_count, 1);
    assert_eq!(idx.stats().unwrap().edge_count, 1);

    let conn = Connection::open(&db_path).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 4);

    let has_resolution: bool = {
        let mut stmt = conn.prepare("PRAGMA table_info(edges)").unwrap();
        let names: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        names.iter().any(|name| name == "resolution")
    };
    assert!(has_resolution);

    let symbol_columns: Vec<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(symbols)").unwrap();
        stmt.query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    assert!(symbol_columns.iter().any(|name| name == "signature"));
    assert!(symbol_columns.iter().any(|name| name == "doc"));

    let imports_table: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'imports'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(imports_table, 1);

    let fts_table: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'symbol_fts'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(fts_table, 1);
    drop(conn);

    // Existing symbols are searchable after the FTS index is rebuilt.
    let results = idx.search("foo", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].qualified_name, "foo");

    // The migrated database still accepts new data and re-resolution.
    idx.replace_file(
        "b.rs",
        "rust",
        "h2",
        &file(
            "b.rs",
            vec![make_symbol("caller", "caller", "b.rs", 1)],
            vec![make_edge("caller", "foo", 2)],
            vec![],
        ),
    )
    .unwrap();
    idx.resolve_edges().unwrap();
    assert!(idx.stats().unwrap().resolved_edge_count >= 1);
}

#[test]
fn v2_database_migrates_to_v4() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("atlas.db");

    {
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE files (
                id INTEGER PRIMARY KEY,
                path TEXT UNIQUE NOT NULL,
                language TEXT,
                content_hash TEXT
            );
            CREATE TABLE symbols (
                id INTEGER PRIMARY KEY,
                file_id INTEGER NOT NULL,
                name TEXT NOT NULL,
                qualified_name TEXT NOT NULL,
                kind TEXT NOT NULL,
                file_path TEXT NOT NULL,
                line INTEGER NOT NULL,
                column_no INTEGER NOT NULL,
                FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
            );
            CREATE TABLE edges (
                id INTEGER PRIMARY KEY,
                file_id INTEGER NOT NULL,
                from_qualified TEXT NOT NULL,
                to_name TEXT NOT NULL,
                kind TEXT NOT NULL,
                line INTEGER NOT NULL,
                dst_symbol_id INTEGER,
                resolution TEXT,
                FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE,
                FOREIGN KEY(dst_symbol_id) REFERENCES symbols(id)
            );
            CREATE TABLE imports (
                id INTEGER PRIMARY KEY,
                file_id INTEGER NOT NULL,
                module TEXT NOT NULL,
                name TEXT,
                alias TEXT,
                line INTEGER NOT NULL,
                FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
            );
            CREATE TABLE schema_version (version INTEGER NOT NULL);
            INSERT INTO schema_version (rowid, version) VALUES (1, 2);
            PRAGMA user_version = 2;

            INSERT INTO files (id, path, language, content_hash)
                VALUES (1, 'legacy.rs', 'rust', 'h1');
            INSERT INTO symbols (id, file_id, name, qualified_name, kind, file_path, line, column_no)
                VALUES (1, 1, 'legacy', 'legacy', 'Function', 'legacy.rs', 1, 0);
            "#,
        )
        .unwrap();
    }

    let idx = Index::open(&db_path).unwrap();
    assert_eq!(idx.stats().unwrap().symbol_count, 1);
    assert_eq!(idx.search("legacy", 10).unwrap().len(), 1);

    let conn = Connection::open(&db_path).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 4);
    drop(conn);

    idx.replace_file(
        "new.rs",
        "rust",
        "h2",
        &file(
            "new.rs",
            vec![Symbol {
                name: "documented".to_string(),
                qualified_name: "documented".to_string(),
                kind: SymbolKind::Function,
                file: "new.rs".to_string(),
                line: 1,
                column: 0,
                signature: Some("pub fn documented()".to_string()),
                doc: Some("Newly indexed documentation.".to_string()),
            }],
            vec![],
            vec![],
        ),
    )
    .unwrap();
    let results = idx.search("documentation", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].signature.as_deref(), Some("pub fn documented()"));
}
