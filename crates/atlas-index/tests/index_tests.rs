use atlas_core::{EdgeKind, FileSymbols, RawEdge, Symbol, SymbolKind};
use atlas_index::{EmbeddingInput, Index};
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

#[test]
fn test_insert_replace_remove() {
    let idx = Index::open_in_memory().unwrap();
    let analysis = FileSymbols {
        file: "test.rs".to_string(),
        symbols: vec![make_symbol("foo", "foo", "test.rs", 1)],
        edges: vec![],
        imports: Vec::new(),
    };
    idx.replace_file("test.rs", "rust", "hash1", &analysis)
        .unwrap();
    assert_eq!(idx.stats().unwrap().symbol_count, 1);
    idx.remove_file("test.rs").unwrap();
    assert_eq!(idx.stats().unwrap().symbol_count, 0);
}

#[test]
fn test_atomic_rollback() {
    let idx = Index::open_in_memory().unwrap();
    let analysis = FileSymbols {
        file: "test.rs".to_string(),
        symbols: vec![make_symbol("foo", "foo", "test.rs", 1)],
        edges: vec![],
        imports: Vec::new(),
    };
    idx.replace_file("test.rs", "rust", "hash1", &analysis)
        .unwrap();
    let stats_before = idx.stats().unwrap();
    assert_eq!(stats_before.symbol_count, 1);
}

#[test]
fn test_idempotent_reindexing() {
    let idx = Index::open_in_memory().unwrap();
    let analysis = FileSymbols {
        file: "test.rs".to_string(),
        symbols: vec![make_symbol("foo", "foo", "test.rs", 1)],
        edges: vec![],
        imports: Vec::new(),
    };
    idx.replace_file("test.rs", "rust", "hash1", &analysis)
        .unwrap();
    idx.replace_file("test.rs", "rust", "hash1", &analysis)
        .unwrap();
    assert_eq!(idx.stats().unwrap().symbol_count, 1);
}

#[test]
fn test_file_hash() {
    let idx = Index::open_in_memory().unwrap();
    let analysis = FileSymbols {
        file: "test.rs".to_string(),
        symbols: vec![],
        edges: vec![],
        imports: Vec::new(),
    };
    assert!(idx.file_hash("test.rs").unwrap().is_none());
    idx.replace_file("test.rs", "rust", "abc123", &analysis)
        .unwrap();
    assert_eq!(idx.file_hash("test.rs").unwrap().unwrap(), "abc123");
}

#[test]
fn test_resolution_rule1() {
    let idx = Index::open_in_memory().unwrap();
    let analysis1 = FileSymbols {
        file: "a.rs".to_string(),
        symbols: vec![make_symbol("foo", "foo", "a.rs", 1)],
        edges: vec![],
        imports: Vec::new(),
    };
    idx.replace_file("a.rs", "rust", "h1", &analysis1).unwrap();
    let analysis_caller = FileSymbols {
        file: "b.rs".to_string(),
        symbols: vec![make_symbol("bar", "bar", "b.rs", 5)],
        edges: vec![RawEdge {
            from: "bar".to_string(),
            to_name: "foo".to_string(),
            kind: EdgeKind::Calls,
            line: 10,
        }],
        imports: Vec::new(),
    };
    idx.replace_file("b.rs", "rust", "h2", &analysis_caller)
        .unwrap();
    let stats = idx.resolve_edges().unwrap();
    assert!(stats.linked >= 1);
}

#[test]
fn test_callers_callees() {
    let idx = Index::open_in_memory().unwrap();
    let analysis1 = FileSymbols {
        file: "a.rs".to_string(),
        symbols: vec![make_symbol("foo", "foo", "a.rs", 1)],
        edges: vec![],
        imports: Vec::new(),
    };
    idx.replace_file("a.rs", "rust", "h1", &analysis1).unwrap();
    let analysis2 = FileSymbols {
        file: "b.rs".to_string(),
        symbols: vec![make_symbol("bar", "bar", "b.rs", 5)],
        edges: vec![RawEdge {
            from: "bar".to_string(),
            to_name: "foo".to_string(),
            kind: EdgeKind::Calls,
            line: 10,
        }],
        imports: Vec::new(),
    };
    idx.replace_file("b.rs", "rust", "h2", &analysis2).unwrap();
    idx.resolve_edges().unwrap();
    let callers = idx.callers_of("foo").unwrap();
    assert_eq!(callers.len(), 1);
    let callees = idx.callees_of("bar").unwrap();
    assert_eq!(callees.len(), 1);
}

#[test]
fn test_impact_with_cycles() {
    let idx = Index::open_in_memory().unwrap();
    let analysis1 = FileSymbols {
        file: "a.rs".to_string(),
        symbols: vec![
            make_symbol("a", "a", "a.rs", 1),
            make_symbol("b", "b", "a.rs", 5),
        ],
        edges: vec![
            RawEdge {
                from: "a".to_string(),
                to_name: "b".to_string(),
                kind: EdgeKind::Calls,
                line: 2,
            },
            RawEdge {
                from: "b".to_string(),
                to_name: "a".to_string(),
                kind: EdgeKind::Calls,
                line: 6,
            },
        ],
        imports: Vec::new(),
    };
    idx.replace_file("a.rs", "rust", "h1", &analysis1).unwrap();
    idx.resolve_edges().unwrap();
    let _impact = idx.impact_of("a", 10).unwrap();
}

#[test]
fn test_empty_index() {
    let idx = Index::open_in_memory().unwrap();
    assert_eq!(idx.stats().unwrap().symbol_count, 0);
    assert_eq!(idx.callers_of("foo").unwrap().len(), 0);
    assert_eq!(idx.callees_of("foo").unwrap().len(), 0);
    assert_eq!(idx.impact_of("foo", 5).unwrap().len(), 0);
}

fn documented_symbol(
    name: &str,
    qname: &str,
    file: &str,
    line: usize,
    signature: Option<&str>,
    doc: Option<&str>,
) -> Symbol {
    Symbol {
        name: name.to_string(),
        qualified_name: qname.to_string(),
        kind: SymbolKind::Function,
        file: file.to_string(),
        line,
        column: 0,
        signature: signature.map(str::to_string),
        doc: doc.map(str::to_string),
    }
}

fn replace_symbols(idx: &Index, path: &str, symbols: Vec<Symbol>) {
    idx.replace_file(
        path,
        "rust",
        "hash",
        &FileSymbols {
            file: path.to_string(),
            symbols,
            edges: Vec::new(),
            imports: Vec::new(),
        },
    )
    .unwrap();
}

#[test]
fn search_matches_name_signature_and_doc() {
    let idx = Index::open_in_memory().unwrap();
    replace_symbols(
        &idx,
        "src/parser.rs",
        vec![documented_symbol(
            "parse_file",
            "parser::parse_file",
            "src/parser.rs",
            10,
            Some("pub fn parse_file(path: &Path) -> Result<Ast>"),
            Some("Parse one source file."),
        )],
    );
    replace_symbols(
        &idx,
        "src/render.rs",
        vec![documented_symbol(
            "render",
            "render",
            "src/render.rs",
            3,
            Some("pub fn render(graph: &Graph)"),
            Some("Render the dependency graph."),
        )],
    );

    let by_name = idx.search("parse", 10).unwrap();
    assert_eq!(by_name.len(), 1);
    assert_eq!(by_name[0].qualified_name, "parser::parse_file");
    assert_eq!(
        by_name[0].signature.as_deref(),
        Some("pub fn parse_file(path: &Path) -> Result<Ast>")
    );
    assert_eq!(by_name[0].doc.as_deref(), Some("Parse one source file."));

    let by_doc = idx.search("dependency graph", 10).unwrap();
    assert_eq!(by_doc.len(), 1);
    assert_eq!(by_doc[0].name, "render");

    let by_signature = idx.search("Ast", 10).unwrap();
    assert_eq!(by_signature.len(), 1);
    assert_eq!(by_signature[0].name, "parse_file");
}

#[test]
fn search_order_is_deterministic() {
    let idx = Index::open_in_memory().unwrap();
    replace_symbols(
        &idx,
        "b.rs",
        vec![documented_symbol(
            "beta",
            "beta",
            "b.rs",
            1,
            None,
            Some("shared token"),
        )],
    );
    replace_symbols(
        &idx,
        "a.rs",
        vec![documented_symbol(
            "alpha",
            "alpha",
            "a.rs",
            1,
            None,
            Some("shared token"),
        )],
    );

    let results = idx.search("shared", 10).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].name, "alpha");
    assert_eq!(results[1].name, "beta");
}

#[test]
fn search_index_tracks_replace_and_remove() {
    let idx = Index::open_in_memory().unwrap();
    replace_symbols(
        &idx,
        "a.rs",
        vec![documented_symbol(
            "foo",
            "foo",
            "a.rs",
            1,
            None,
            Some("oldtoken"),
        )],
    );
    assert_eq!(idx.search("oldtoken", 10).unwrap().len(), 1);

    replace_symbols(
        &idx,
        "a.rs",
        vec![documented_symbol(
            "foo",
            "foo",
            "a.rs",
            1,
            None,
            Some("newtoken"),
        )],
    );
    assert!(idx.search("oldtoken", 10).unwrap().is_empty());
    assert_eq!(idx.search("newtoken", 10).unwrap().len(), 1);

    idx.remove_file("a.rs").unwrap();
    assert!(idx.search("newtoken", 10).unwrap().is_empty());
}

#[test]
fn search_treats_input_as_plain_text() {
    let idx = Index::open_in_memory().unwrap();
    replace_symbols(
        &idx,
        "a.rs",
        vec![documented_symbol(
            "foo",
            "foo",
            "a.rs",
            1,
            None,
            Some("needle"),
        )],
    );
    replace_symbols(
        &idx,
        "b.rs",
        vec![documented_symbol(
            "bar",
            "bar",
            "b.rs",
            1,
            None,
            Some("needle"),
        )],
    );

    for query in [
        "\"",
        "*",
        "(",
        "foo OR bar",
        "-foo",
        "NEAR(foo bar)",
        "foo*",
        "foo:bar",
    ] {
        idx.search(query, 10).unwrap();
    }
    assert!(idx.search("\"", 10).unwrap().is_empty());
    assert!(idx.search("*", 10).unwrap().is_empty());
    assert_eq!(idx.search("foo*", 10).unwrap()[0].name, "foo");
    assert!(idx.search("OR", 10).unwrap().is_empty());
}

#[test]
fn search_respects_limit_and_empty_query() {
    let idx = Index::open_in_memory().unwrap();
    replace_symbols(
        &idx,
        "a.rs",
        vec![
            documented_symbol("foo", "foo", "a.rs", 1, None, Some("shared")),
            documented_symbol("bar", "bar", "a.rs", 2, None, Some("shared")),
        ],
    );

    assert_eq!(idx.search("shared", 1).unwrap().len(), 1);
    assert!(idx.search("", 10).unwrap().is_empty());
    assert!(idx.search("   ", 10).unwrap().is_empty());
    assert!(idx.search("shared", 0).unwrap().is_empty());
}

#[test]
fn id_based_queries_do_not_merge_same_named_symbols() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "a.rs",
        "rust",
        "h1",
        &FileSymbols {
            file: "a.rs".to_string(),
            symbols: vec![
                make_symbol("foo", "a::foo", "a.rs", 1),
                make_symbol("caller_a", "a::caller_a", "a.rs", 5),
            ],
            edges: vec![RawEdge {
                from: "a::caller_a".to_string(),
                to_name: "foo".to_string(),
                kind: EdgeKind::Calls,
                line: 6,
            }],
            imports: Vec::new(),
        },
    )
    .unwrap();
    idx.replace_file(
        "b.rs",
        "rust",
        "h2",
        &FileSymbols {
            file: "b.rs".to_string(),
            symbols: vec![
                make_symbol("foo", "b::foo", "b.rs", 1),
                make_symbol("caller_b", "b::caller_b", "b.rs", 5),
            ],
            edges: vec![RawEdge {
                from: "b::caller_b".to_string(),
                to_name: "foo".to_string(),
                kind: EdgeKind::Calls,
                line: 6,
            }],
            imports: Vec::new(),
        },
    )
    .unwrap();
    idx.resolve_edges().unwrap();

    let symbols = idx.list_symbols(20).unwrap();
    let by_name = |qname: &str| {
        symbols
            .iter()
            .find(|symbol| symbol.qualified_name == qname)
            .unwrap()
            .id
    };
    let foo_a = by_name("a::foo");
    let foo_b = by_name("b::foo");
    let caller_a = by_name("a::caller_a");
    let caller_b = by_name("b::caller_b");

    let callers_a = idx.callers_of_symbol(foo_a).unwrap();
    assert_eq!(callers_a.len(), 1);
    assert_eq!(callers_a[0].qualified_name, "a::caller_a");
    let callers_b = idx.callers_of_symbol(foo_b).unwrap();
    assert_eq!(callers_b.len(), 1);
    assert_eq!(callers_b[0].qualified_name, "b::caller_b");

    let callees = idx.callees_of_symbol(caller_a).unwrap();
    assert_eq!(callees.len(), 1);
    assert_eq!(callees[0].symbol_id, Some(foo_a));
    assert_eq!(callees[0].qualified_name, "a::foo");
    assert_eq!(callees[0].call_line, 6);
    assert_eq!(callees[0].resolution.as_deref(), Some("same_file"));

    let impact_a = idx.impact_of_symbol(foo_a, 10).unwrap();
    assert_eq!(impact_a.len(), 1);
    assert_eq!(impact_a[0].symbol_id, caller_a);
    assert_eq!(impact_a[0].symbol_qualified_name, "a::caller_a");
    let impact_b = idx.impact_of_symbol(foo_b, 10).unwrap();
    assert_eq!(impact_b.len(), 1);
    assert_eq!(impact_b[0].symbol_id, caller_b);
    assert_eq!(impact_b[0].symbol_qualified_name, "b::caller_b");
}

#[test]
fn graph_neighborhood_has_callers_callees_and_depths() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "graph.rs",
        "rust",
        "h1",
        &FileSymbols {
            file: "graph.rs".to_string(),
            symbols: vec![
                make_symbol("a", "a", "graph.rs", 1),
                make_symbol("b", "b", "graph.rs", 5),
                make_symbol("c", "c", "graph.rs", 9),
                make_symbol("d", "d", "graph.rs", 13),
            ],
            edges: vec![
                RawEdge {
                    from: "a".to_string(),
                    to_name: "b".to_string(),
                    kind: EdgeKind::Calls,
                    line: 2,
                },
                RawEdge {
                    from: "b".to_string(),
                    to_name: "c".to_string(),
                    kind: EdgeKind::Calls,
                    line: 6,
                },
                RawEdge {
                    from: "d".to_string(),
                    to_name: "b".to_string(),
                    kind: EdgeKind::Calls,
                    line: 14,
                },
            ],
            imports: Vec::new(),
        },
    )
    .unwrap();
    idx.resolve_edges().unwrap();
    let b_id = idx
        .list_symbols(20)
        .unwrap()
        .into_iter()
        .find(|symbol| symbol.name == "b")
        .unwrap()
        .id;

    let graph = idx.graph_for_symbol(b_id, 2, 20).unwrap().unwrap();
    assert!(!graph.truncated);
    assert_eq!(graph.nodes.len(), 4);
    assert_eq!(graph.edges.len(), 3);

    let node = |name: &str| graph.nodes.iter().find(|node| node.name == name).unwrap();
    assert_eq!(node("b").side, "root");
    assert_eq!(node("b").depth, 0);
    assert_eq!(node("a").side, "caller");
    assert_eq!(node("a").depth, 1);
    assert_eq!(node("d").side, "caller");
    assert_eq!(node("c").side, "callee");
    assert_eq!(node("c").depth, 1);
    assert_eq!(graph.edges[0].from, node("a").id);
    assert_eq!(graph.edges[0].to, node("b").id);
    assert_eq!(graph.edges[1].from, node("b").id);
    assert_eq!(graph.edges[1].to, node("c").id);
    assert_eq!(graph.edges[2].from, node("d").id);
    assert_eq!(graph.edges[2].to, node("b").id);
}

#[test]
fn graph_respects_depth_and_node_limits() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "chain.rs",
        "rust",
        "h1",
        &FileSymbols {
            file: "chain.rs".to_string(),
            symbols: vec![
                make_symbol("a", "a", "chain.rs", 1),
                make_symbol("b", "b", "chain.rs", 5),
                make_symbol("c", "c", "chain.rs", 9),
            ],
            edges: vec![
                RawEdge {
                    from: "a".to_string(),
                    to_name: "b".to_string(),
                    kind: EdgeKind::Calls,
                    line: 2,
                },
                RawEdge {
                    from: "b".to_string(),
                    to_name: "c".to_string(),
                    kind: EdgeKind::Calls,
                    line: 6,
                },
            ],
            imports: Vec::new(),
        },
    )
    .unwrap();
    idx.resolve_edges().unwrap();
    let a_id = idx
        .list_symbols(20)
        .unwrap()
        .into_iter()
        .find(|symbol| symbol.name == "a")
        .unwrap()
        .id;

    let shallow = idx.graph_for_symbol(a_id, 0, 20).unwrap().unwrap();
    assert_eq!(shallow.nodes.len(), 1);
    assert_eq!(shallow.nodes[0].side, "root");
    assert!(shallow.edges.is_empty());

    let limited = idx.graph_for_symbol(a_id, 5, 2).unwrap().unwrap();
    assert!(limited.truncated);
    assert_eq!(limited.nodes.len(), 2);
    assert!(limited.nodes.iter().any(|node| node.name == "b"));
}

#[test]
fn callees_include_unresolved_calls() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "u.rs",
        "rust",
        "h1",
        &FileSymbols {
            file: "u.rs".to_string(),
            symbols: vec![make_symbol("caller", "caller", "u.rs", 1)],
            edges: vec![RawEdge {
                from: "caller".to_string(),
                to_name: "missing".to_string(),
                kind: EdgeKind::Calls,
                line: 5,
            }],
            imports: Vec::new(),
        },
    )
    .unwrap();
    idx.resolve_edges().unwrap();
    let caller_id = idx.list_symbols(1).unwrap()[0].id;

    let callees = idx.callees_of_symbol(caller_id).unwrap();
    assert_eq!(callees.len(), 1);
    assert_eq!(callees[0].symbol_id, None);
    assert_eq!(callees[0].name, "missing");
    assert_eq!(callees[0].resolution, None);
    assert_eq!(callees[0].call_line, 5);
    assert_eq!(callees[0].file, "u.rs");
}

#[test]
fn impact_terminates_on_cycles_without_returning_the_root() {
    let idx = Index::open_in_memory().unwrap();
    idx.replace_file(
        "cycle.rs",
        "rust",
        "h1",
        &FileSymbols {
            file: "cycle.rs".to_string(),
            symbols: vec![
                make_symbol("a", "a", "cycle.rs", 1),
                make_symbol("b", "b", "cycle.rs", 5),
            ],
            edges: vec![
                RawEdge {
                    from: "a".to_string(),
                    to_name: "b".to_string(),
                    kind: EdgeKind::Calls,
                    line: 2,
                },
                RawEdge {
                    from: "b".to_string(),
                    to_name: "a".to_string(),
                    kind: EdgeKind::Calls,
                    line: 6,
                },
            ],
            imports: Vec::new(),
        },
    )
    .unwrap();
    idx.resolve_edges().unwrap();
    let a_id = idx
        .list_symbols(20)
        .unwrap()
        .into_iter()
        .find(|symbol| symbol.name == "a")
        .unwrap()
        .id;

    let impact = idx.impact_of_symbol(a_id, 10).unwrap();
    assert_eq!(impact.len(), 1);
    assert_eq!(impact[0].symbol_qualified_name, "b");
    assert_eq!(impact[0].depth, 1);

    let by_name = idx.impact_of("a", 10).unwrap();
    assert_eq!(by_name.len(), 1);
    assert_eq!(by_name[0].symbol_qualified_name, "b");
}

#[test]
fn newer_database_is_rejected_with_a_clear_message() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("atlas.db");
    {
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE files (id INTEGER PRIMARY KEY);
             PRAGMA user_version = 99;",
        )
        .unwrap();
    }

    match Index::open(&db_path) {
        Ok(_) => panic!("newer database should not open"),
        Err(error) => {
            let message = error.to_string();
            assert!(message.contains("newer than supported version 4"));
            assert!(message.contains("delete the database and re-index"));
        }
    }
}

#[test]
fn fresh_database_uses_schema_v4() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("atlas.db");
    {
        let _index = Index::open(&db_path).unwrap();
    }
    let conn = Connection::open(&db_path).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 4);
    let embeddings: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'symbol_embeddings'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(embeddings, 1);
}

#[test]
fn v3_database_migrates_to_v4_without_losing_symbols() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("atlas.db");
    {
        let idx = Index::open(&db_path).unwrap();
        let analysis = FileSymbols {
            file: "old.rs".to_string(),
            symbols: vec![make_symbol("old", "old", "old.rs", 1)],
            edges: vec![],
            imports: Vec::new(),
        };
        idx.replace_file("old.rs", "rust", "hash", &analysis)
            .unwrap();
    }
    {
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "DROP TABLE symbol_embeddings;
             PRAGMA user_version = 3;",
        )
        .unwrap();
    }

    let idx = Index::open(&db_path).unwrap();
    assert_eq!(idx.stats().unwrap().symbol_count, 1);
    let candidates = idx.embedding_candidates("model").unwrap();
    assert_eq!(candidates.len(), 1);
    assert!(candidates[0].text.contains("old.rs"));

    drop(idx);
    let conn = Connection::open(&db_path).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 4);
    let embeddings: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'symbol_embeddings'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(embeddings, 1);
}

#[test]
fn embedding_candidates_describe_symbols_and_become_current() {
    let idx = Index::open_in_memory().unwrap();
    let mut symbol = make_symbol("parse_args", "cli::parse_args", "src/cli.rs", 12);
    symbol.signature = Some("fn parse_args(args: &[String]) -> Options".to_string());
    symbol.doc = Some("Parse command line arguments and return options.".to_string());
    let analysis = FileSymbols {
        file: "src/cli.rs".to_string(),
        symbols: vec![symbol],
        edges: vec![],
        imports: Vec::new(),
    };
    idx.replace_file("src/cli.rs", "rust", "hash", &analysis)
        .unwrap();

    let candidates = idx.embedding_candidates("test-model").unwrap();
    assert_eq!(candidates.len(), 1);
    let text = &candidates[0].text;
    assert!(text.contains("parse_args"));
    assert!(text.contains("cli::parse_args"));
    assert!(text.contains("fn parse_args"));
    assert!(text.contains("Parse command line arguments"));
    assert!(text.contains("src/cli.rs"));
    assert!(text.contains("language: rust"));

    let stats = idx.embedding_stats("test-model").unwrap();
    assert_eq!(stats.total_symbols, 1);
    assert_eq!(stats.embedded_symbols, 0);
    assert_eq!(stats.stale_symbols, 1);
    assert_eq!(stats.current_symbols, 0);

    let stored = idx
        .upsert_embeddings(
            "test-model",
            &[EmbeddingInput {
                symbol_id: candidates[0].symbol_id,
                text: candidates[0].text.clone(),
                vector: vec![1.0, 0.0, 0.0],
            }],
        )
        .unwrap();
    assert_eq!(stored, 1);
    assert!(idx.embedding_candidates("test-model").unwrap().is_empty());

    let stats = idx.embedding_stats("test-model").unwrap();
    assert_eq!(stats.embedded_symbols, 1);
    assert_eq!(stats.current_symbols, 1);
    assert_eq!(stats.stale_symbols, 0);
    assert_eq!(stats.dim, Some(3));
}

#[test]
fn changing_symbol_text_makes_its_embedding_stale() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("atlas.db");
    let idx = Index::open(&db_path).unwrap();
    let analysis = FileSymbols {
        file: "lib.rs".to_string(),
        symbols: vec![make_symbol("work", "work", "lib.rs", 1)],
        edges: vec![],
        imports: Vec::new(),
    };
    idx.replace_file("lib.rs", "rust", "hash", &analysis)
        .unwrap();
    let candidate = idx.embedding_candidates("m").unwrap().pop().unwrap();
    idx.upsert_embeddings(
        "m",
        &[EmbeddingInput {
            symbol_id: candidate.symbol_id,
            text: candidate.text,
            vector: vec![1.0, 0.0],
        }],
    )
    .unwrap();
    assert_eq!(idx.embedding_stats("m").unwrap().stale_symbols, 0);

    {
        let conn = Connection::open(&db_path).unwrap();
        conn.execute(
            "UPDATE symbols SET doc = 'new documentation' WHERE id = ?1",
            [candidate.symbol_id],
        )
        .unwrap();
    }

    let stats = idx.embedding_stats("m").unwrap();
    assert_eq!(stats.current_symbols, 0);
    assert_eq!(stats.stale_symbols, 1);
    assert_eq!(idx.embedding_candidates("m").unwrap().len(), 1);
}

#[test]
fn semantic_search_ranks_by_cosine_similarity() {
    let idx = Index::open_in_memory().unwrap();
    let analysis = FileSymbols {
        file: "search.rs".to_string(),
        symbols: vec![
            make_symbol("parse_args", "parse_args", "search.rs", 1),
            make_symbol("send_request", "send_request", "search.rs", 5),
            make_symbol("render_graph", "render_graph", "search.rs", 9),
        ],
        edges: vec![],
        imports: Vec::new(),
    };
    idx.replace_file("search.rs", "rust", "hash", &analysis)
        .unwrap();

    let candidates = idx.embedding_candidates("m").unwrap();
    let mut inputs = Vec::new();
    for candidate in &candidates {
        let vector = if candidate.text.contains("parse_args") {
            vec![1.0, 0.0]
        } else if candidate.text.contains("send_request") {
            vec![0.8, 0.6]
        } else {
            vec![0.0, 1.0]
        };
        inputs.push(EmbeddingInput {
            symbol_id: candidate.symbol_id,
            text: candidate.text.clone(),
            vector,
        });
    }
    idx.upsert_embeddings("m", &inputs).unwrap();

    let results = idx.semantic_search("m", &[1.0, 0.0], 10).unwrap();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].name, "parse_args");
    assert_eq!(results[1].name, "send_request");
    assert_eq!(results[2].name, "render_graph");
    assert!((results[0].similarity - 1.0).abs() < 1e-6);
    assert!((results[1].similarity - 0.8).abs() < 1e-6);
    assert!((results[2].similarity - 0.0).abs() < 1e-6);

    let limited = idx.semantic_search("m", &[1.0, 0.0], 1).unwrap();
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].name, "parse_args");
    assert!(idx
        .semantic_search("other-model", &[1.0, 0.0], 10)
        .unwrap()
        .is_empty());
}

#[test]
fn reindexing_removes_old_embeddings() {
    let idx = Index::open_in_memory().unwrap();
    let analysis = FileSymbols {
        file: "lib.rs".to_string(),
        symbols: vec![make_symbol("work", "work", "lib.rs", 1)],
        edges: vec![],
        imports: Vec::new(),
    };
    idx.replace_file("lib.rs", "rust", "hash1", &analysis)
        .unwrap();
    let candidate = idx.embedding_candidates("m").unwrap().pop().unwrap();
    idx.upsert_embeddings(
        "m",
        &[EmbeddingInput {
            symbol_id: candidate.symbol_id,
            text: candidate.text,
            vector: vec![1.0, 0.0],
        }],
    )
    .unwrap();
    assert_eq!(idx.embedding_stats("m").unwrap().embedded_symbols, 1);

    idx.replace_file("lib.rs", "rust", "hash2", &analysis)
        .unwrap();
    let stats = idx.embedding_stats("m").unwrap();
    assert_eq!(stats.embedded_symbols, 0);
    assert_eq!(stats.stale_symbols, 1);
}

#[test]
fn invalid_embeddings_are_rejected_atomically() {
    let idx = Index::open_in_memory().unwrap();
    let analysis = FileSymbols {
        file: "lib.rs".to_string(),
        symbols: vec![
            make_symbol("a", "a", "lib.rs", 1),
            make_symbol("b", "b", "lib.rs", 2),
        ],
        edges: vec![],
        imports: Vec::new(),
    };
    idx.replace_file("lib.rs", "rust", "hash", &analysis)
        .unwrap();
    let candidates = idx.embedding_candidates("m").unwrap();

    let error = idx
        .upsert_embeddings(
            "m",
            &[
                EmbeddingInput {
                    symbol_id: candidates[0].symbol_id,
                    text: candidates[0].text.clone(),
                    vector: vec![1.0, 0.0],
                },
                EmbeddingInput {
                    symbol_id: candidates[1].symbol_id,
                    text: candidates[1].text.clone(),
                    vector: vec![f32::NAN, 0.0],
                },
            ],
        )
        .unwrap_err();
    assert!(error.to_string().contains("non-empty and finite"));
    assert_eq!(idx.embedding_stats("m").unwrap().embedded_symbols, 0);

    let error = idx
        .upsert_embeddings(
            "m",
            &[EmbeddingInput {
                symbol_id: 9999,
                text: "missing".to_string(),
                vector: vec![1.0, 0.0],
            }],
        )
        .unwrap_err();
    assert!(error.to_string().contains("missing symbol 9999"));
}
