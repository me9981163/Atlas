use atlas_core::FileSymbols;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Other: {0}")]
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveStats {
    pub linked: usize,
    pub ambiguous: usize,
    pub unresolved: usize,
    pub external: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallSite {
    pub caller_qualified_name: String,
    pub caller_file: String,
    pub caller_line: usize,
    pub callee_qualified_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactEntry {
    pub symbol_id: i64,
    pub symbol_qualified_name: String,
    pub file: String,
    pub line: usize,
    pub depth: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub symbol_count: usize,
    pub edge_count: usize,
    pub resolved_edge_count: usize,
    pub unresolved_edge_count: usize,
    pub external_edge_count: usize,
    pub file_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionCount {
    pub resolution: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnresolvedName {
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeInfo {
    pub caller_file: String,
    pub caller_qualified_name: String,
    pub line: usize,
    pub to_name: String,
    pub resolution: Option<String>,
    pub callee_qualified_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: i64,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
    pub signature: Option<String>,
    pub doc: Option<String>,
    /// FTS5 bm25 score. Lower is a better match.
    pub score: f64,
}

/// A symbol whose embedding is missing or out of date for a model. `text` is
/// exactly what the caller must embed; `text_hash` is stored so later runs can
/// skip unchanged symbols without re-embedding them.
#[derive(Debug, Clone)]
pub struct EmbeddingCandidate {
    pub symbol_id: i64,
    pub text: String,
    pub text_hash: String,
}

/// One freshly computed embedding to store.
#[derive(Debug, Clone)]
pub struct EmbeddingInput {
    pub symbol_id: i64,
    pub text: String,
    pub vector: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingStats {
    pub model: String,
    pub total_symbols: usize,
    /// Rows present for this model, including stale ones.
    pub embedded_symbols: usize,
    /// Rows whose text hash still matches the current symbol text.
    pub current_symbols: usize,
    pub stale_symbols: usize,
    pub dim: Option<usize>,
}

/// A semantic search hit. `similarity` is cosine similarity in `[-1, 1]`;
/// higher is a better match. Unlike FTS5 bm25 scores it is not comparable
/// across different queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticResult {
    pub id: i64,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
    pub signature: Option<String>,
    pub doc: Option<String>,
    pub similarity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolDetails {
    pub id: i64,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub signature: Option<String>,
    pub doc: Option<String>,
}

/// One end of a call edge, as shown in the desktop app. `symbol_id` is
/// `None` for an unresolved or external callee, where no definition is known.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedSymbol {
    pub symbol_id: Option<i64>,
    pub name: String,
    pub qualified_name: String,
    pub kind: Option<String>,
    pub file: String,
    pub line: usize,
    pub call_line: usize,
    pub resolution: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: i64,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
    /// Shortest distance from the root, in call-graph hops.
    pub depth: u32,
    /// `root`, `caller`, `callee`, or `both`.
    pub side: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: i64,
    pub to: i64,
    pub resolution: Option<String>,
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// True when the node cap stopped the traversal. The graph is then a
    /// deterministic subset, not the complete neighborhood.
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccuracyReport {
    pub files: usize,
    pub symbols: usize,
    pub edges: usize,
    pub resolved: usize,
    pub same_file: usize,
    pub import: usize,
    pub unique_name: usize,
    pub legacy_resolved: usize,
    pub external: usize,
    pub unresolved: usize,
    pub resolution_counts: Vec<ResolutionCount>,
    pub top_unresolved: Vec<UnresolvedName>,
}

pub struct Index {
    conn: Mutex<Connection>,
}

const SCHEMA_VERSION: i64 = 4;

/// Documentation is often much longer than the embedding model's context.
/// Keeping a bounded prefix makes indexing predictable and keeps the stored
/// text hash stable across platforms.
const EMBEDDING_DOC_CHARS: usize = 1_000;

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);

CREATE TABLE IF NOT EXISTS files (
    id INTEGER PRIMARY KEY,
    path TEXT UNIQUE NOT NULL,
    language TEXT,
    content_hash TEXT
);

CREATE TABLE IF NOT EXISTS symbols (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    qualified_name TEXT NOT NULL,
    kind TEXT NOT NULL,
    file_path TEXT NOT NULL,
    line INTEGER NOT NULL,
    column_no INTEGER NOT NULL,
    signature TEXT,
    doc TEXT,
    FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS edges (
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

CREATE TABLE IF NOT EXISTS imports (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL,
    module TEXT NOT NULL,
    name TEXT,
    alias TEXT,
    line INTEGER NOT NULL,
    FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
);

INSERT OR REPLACE INTO schema_version (rowid, version) VALUES (1, 4);
PRAGMA user_version = 4;
"#;

const INDEX_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS imports (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL,
    module TEXT NOT NULL,
    name TEXT,
    alias TEXT,
    line INTEGER NOT NULL,
    FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
CREATE INDEX IF NOT EXISTS idx_symbols_qualified_name ON symbols(qualified_name);
CREATE INDEX IF NOT EXISTS idx_symbols_file_id ON symbols(file_id);
CREATE INDEX IF NOT EXISTS idx_edges_dst_symbol_id ON edges(dst_symbol_id);
CREATE INDEX IF NOT EXISTS idx_edges_file_id ON edges(file_id);
CREATE INDEX IF NOT EXISTS idx_imports_file_id ON imports(file_id);

CREATE TABLE IF NOT EXISTS symbol_embeddings (
    symbol_id INTEGER NOT NULL,
    model TEXT NOT NULL,
    dim INTEGER NOT NULL,
    text_hash TEXT NOT NULL,
    vector BLOB NOT NULL,
    PRIMARY KEY(symbol_id, model),
    FOREIGN KEY(symbol_id) REFERENCES symbols(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_symbol_embeddings_model
    ON symbol_embeddings(model);

CREATE VIRTUAL TABLE IF NOT EXISTS symbol_fts USING fts5(
    name,
    qualified_name,
    signature,
    doc,
    content='symbols',
    content_rowid='id',
    tokenize='unicode61'
);

CREATE TRIGGER IF NOT EXISTS symbols_ai AFTER INSERT ON symbols BEGIN
    INSERT INTO symbol_fts(rowid, name, qualified_name, signature, doc)
    VALUES (new.id, new.name, new.qualified_name, new.signature, new.doc);
END;

CREATE TRIGGER IF NOT EXISTS symbols_ad AFTER DELETE ON symbols BEGIN
    INSERT INTO symbol_fts(symbol_fts, rowid, name, qualified_name, signature, doc)
    VALUES ('delete', old.id, old.name, old.qualified_name, old.signature, old.doc);
END;

CREATE TRIGGER IF NOT EXISTS symbols_au AFTER UPDATE ON symbols BEGIN
    INSERT INTO symbol_fts(symbol_fts, rowid, name, qualified_name, signature, doc)
    VALUES ('delete', old.id, old.name, old.qualified_name, old.signature, old.doc);
    INSERT INTO symbol_fts(rowid, name, qualified_name, signature, doc)
    VALUES (new.id, new.name, new.qualified_name, new.signature, new.doc);
END;
"#;

impl Index {
    pub fn open(path: &Path) -> Result<Self, IndexError> {
        let conn = Connection::open(path)?;
        let idx = Index {
            conn: Mutex::new(conn),
        };
        idx.init()?;
        Ok(idx)
    }

    pub fn open_in_memory() -> Result<Self, IndexError> {
        let conn = Connection::open_in_memory()?;
        let idx = Index {
            conn: Mutex::new(conn),
        };
        idx.init()?;
        Ok(idx)
    }

    fn init(&self) -> Result<(), IndexError> {
        let mut conn = self.conn.lock().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        init_schema(&mut conn)
    }

    pub fn replace_file(
        &self,
        path: &str,
        language: &str,
        content_hash: &str,
        analysis: &FileSymbols,
    ) -> Result<(), IndexError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        // Delete old data for this file
        let file_id: Option<i64> = tx
            .query_row(
                "SELECT id FROM files WHERE path = ?1",
                params![path],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(fid) = file_id {
            tx.execute("DELETE FROM imports WHERE file_id = ?1", params![fid])?;
            tx.execute("DELETE FROM edges WHERE file_id = ?1", params![fid])?;
            tx.execute("DELETE FROM symbols WHERE file_id = ?1", params![fid])?;
            tx.execute("DELETE FROM files WHERE id = ?1", params![fid])?;
        }
        // Insert new file
        tx.execute(
            "INSERT INTO files (path, language, content_hash) VALUES (?1, ?2, ?3)",
            params![path, language, content_hash],
        )?;
        let file_id: i64 = tx.last_insert_rowid();
        // Insert symbols
        for sym in &analysis.symbols {
            tx.execute(
                "INSERT INTO symbols (file_id, name, qualified_name, kind, file_path, line, column_no, signature, doc) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    file_id,
                    sym.name,
                    sym.qualified_name,
                    format!("{:?}", sym.kind),
                    sym.file,
                    sym.line as i64,
                    sym.column as i64,
                    sym.signature,
                    sym.doc,
                ],
            )?;
        }
        // Insert imports
        for import in &analysis.imports {
            tx.execute(
                "INSERT INTO imports (file_id, module, name, alias, line) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    file_id,
                    import.module,
                    import.name,
                    import.alias,
                    import.line as i64,
                ],
            )?;
        }
        // Insert edges. Resolution is filled in by resolve_edges().
        for edge in &analysis.edges {
            tx.execute(
                "INSERT INTO edges (file_id, from_qualified, to_name, kind, line) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    file_id,
                    edge.from,
                    edge.to_name,
                    format!("{:?}", edge.kind),
                    edge.line as i64,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn file_hash(&self, path: &str) -> Result<Option<String>, IndexError> {
        let conn = self.conn.lock().unwrap();
        let res = conn
            .query_row(
                "SELECT content_hash FROM files WHERE path = ?1",
                params![path],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(res)
    }

    pub fn all_file_paths(&self) -> Result<Vec<String>, IndexError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT path FROM files ORDER BY path")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn remove_file(&self, path: &str) -> Result<(), IndexError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM files WHERE path = ?1", params![path])?;
        Ok(())
    }

    /// Recompute every edge. Import-aware resolution runs first, then the
    /// conservative name-based rules. Every result is written to
    /// `edges.resolution` so the accuracy report can explain it.
    pub fn resolve_edges(&self) -> Result<ResolveStats, IndexError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        tx.execute(
            "UPDATE edges SET dst_symbol_id = NULL, resolution = NULL",
            [],
        )?;

        let mut file_stmt = tx.prepare("SELECT id, path, language FROM files")?;
        let files: HashMap<i64, FileInfo> = file_stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    FileInfo {
                        path: row.get(1)?,
                        language: row.get(2)?,
                    },
                ))
            })?
            .collect::<Result<HashMap<_, _>, _>>()?;
        drop(file_stmt);

        let all_files: HashSet<String> = files.values().map(|f| f.path.clone()).collect();
        let mut rust_modules: HashMap<(String, Vec<String>), Vec<String>> = HashMap::new();
        for info in files.values() {
            if info.language.as_deref() == Some("rust") {
                if let Some(module) = rust_module_of(&info.path) {
                    rust_modules
                        .entry((module.crate_root, module.path))
                        .or_default()
                        .push(info.path.clone());
                }
            }
        }

        // Rust module paths are derived from file paths; compute them once
        // instead of for every import/edge pair.
        let rust_caller_modules: HashMap<i64, Option<RustModulePath>> = files
            .iter()
            .filter(|(_, info)| info.language.as_deref() == Some("rust"))
            .map(|(id, info)| (*id, rust_module_of(&info.path)))
            .collect();

        let mut import_stmt =
            tx.prepare("SELECT file_id, module, name, alias FROM imports ORDER BY id")?;
        let mut imports_by_file: HashMap<i64, Vec<ImportRow>> = HashMap::new();
        let import_rows = import_stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                ImportRow {
                    module: row.get(1)?,
                    name: row.get(2)?,
                    alias: row.get(3)?,
                },
            ))
        })?;
        for row in import_rows {
            let (file_id, import) = row?;
            imports_by_file.entry(file_id).or_default().push(import);
        }
        drop(import_stmt);

        let mut edge_stmt =
            tx.prepare("SELECT id, to_name, from_qualified, file_id FROM edges ORDER BY id")?;
        let edges: Vec<(i64, String, String, i64)> = edge_stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(edge_stmt);

        // Load symbols once and resolve in memory. Running a LIKE query per
        // edge dominated indexing time on repositories with many edges.
        let symbol_maps = SymbolMaps::load(&tx)?;
        let mut update_stmt =
            tx.prepare("UPDATE edges SET dst_symbol_id = ?1, resolution = ?2 WHERE id = ?3")?;
        let mut external_stmt = tx.prepare(
            "UPDATE edges SET dst_symbol_id = NULL, resolution = 'external' WHERE id = ?1",
        )?;

        let mut linked = 0;
        let mut ambiguous = 0;
        let mut unresolved = 0;
        let mut external = 0;

        for (edge_id, to_name, _from_qualified, file_id) in edges {
            let caller = match files.get(&file_id) {
                Some(info) => info,
                None => {
                    unresolved += 1;
                    continue;
                }
            };
            let language = caller.language.as_deref().unwrap_or("");

            let mut external_match = false;
            let mut unresolved_import = false;
            let mut internal_candidates: Vec<(String, String)> = Vec::new();
            let mut wildcard_candidates: Vec<(String, String)> = Vec::new();
            let caller_rust_module = rust_caller_modules.get(&file_id).and_then(|m| m.as_ref());
            if let Some(caller_imports) = imports_by_file.get(&file_id) {
                for import in caller_imports {
                    if !import_may_match(import, &to_name, language) {
                        continue;
                    }
                    match resolve_import_match(
                        import,
                        &to_name,
                        caller,
                        caller_rust_module,
                        &rust_modules,
                        &all_files,
                    ) {
                        Some(ImportMatch::External) => external_match = true,
                        Some(ImportMatch::Unresolved) => unresolved_import = true,
                        Some(ImportMatch::Internal(candidates)) => {
                            internal_candidates.extend(candidates);
                        }
                        Some(ImportMatch::WildcardInternal(candidates)) => {
                            wildcard_candidates.extend(candidates);
                        }
                        None => {}
                    }
                }
            }

            if !internal_candidates.is_empty() {
                match symbol_maps.find_unique_symbol(&internal_candidates, language) {
                    SymbolLookup::One(symbol_id) => {
                        update_stmt.execute(params![symbol_id, "import", edge_id])?;
                        linked += 1;
                    }
                    SymbolLookup::Many => {
                        ambiguous += 1;
                        unresolved += 1;
                    }
                    SymbolLookup::None => {
                        // An explicit import pointed somewhere we could not
                        // find the symbol. Do not fall back to a global name
                        // match: that would guess a different definition.
                        unresolved += 1;
                    }
                }
                continue;
            }

            if !wildcard_candidates.is_empty() {
                // A glob import can explain the call, but if the symbol is not
                // in the globbed module, fall through to normal name matching
                // instead of declaring the call unresolved. Glob imports are
                // too broad to shadow a concrete definition.
                if let SymbolLookup::One(symbol_id) =
                    symbol_maps.find_unique_symbol(&wildcard_candidates, language)
                {
                    update_stmt.execute(params![symbol_id, "import", edge_id])?;
                    linked += 1;
                    continue;
                }
            }

            if external_match {
                external_stmt.execute(params![edge_id])?;
                external += 1;
                continue;
            }

            if unresolved_import {
                // The import matched, but its target is not in the index.
                // Falling back to a global name match would guess.
                unresolved += 1;
                continue;
            }

            let candidates = symbol_maps.fallback_candidates(&to_name, language);

            if candidates.is_empty() {
                unresolved += 1;
                continue;
            }
            if candidates.len() == 1 {
                let (id, _, file) = &candidates[0];
                let resolution = if file == &caller.path {
                    "same_file"
                } else {
                    "unique_name"
                };
                update_stmt.execute(params![id, resolution, edge_id])?;
                linked += 1;
                continue;
            }
            let same_file: Vec<_> = candidates
                .iter()
                .filter(|(_, _, file)| file == &caller.path)
                .collect();
            if same_file.len() == 1 {
                let (id, _, _) = same_file[0];
                update_stmt.execute(params![id, "same_file", edge_id])?;
                linked += 1;
            } else {
                // Never guess between equally plausible definitions.
                ambiguous += 1;
                unresolved += 1;
            }
        }

        drop(update_stmt);
        drop(external_stmt);
        tx.commit()?;

        Ok(ResolveStats {
            linked,
            ambiguous,
            unresolved,
            external,
        })
    }

    /// Every edge with its resolution, in deterministic source order. Useful
    /// for tests, reports, and future UI views.
    pub fn edges(&self) -> Result<Vec<EdgeInfo>, IndexError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT f.path, e.from_qualified, e.line, e.to_name, e.resolution, s.qualified_name
             FROM edges e
             JOIN files f ON e.file_id = f.id
             LEFT JOIN symbols s ON e.dst_symbol_id = s.id
             ORDER BY f.path, e.line, e.id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(EdgeInfo {
                caller_file: row.get(0)?,
                caller_qualified_name: row.get(1)?,
                line: row.get(2)?,
                to_name: row.get(3)?,
                resolution: row.get(4)?,
                callee_qualified_name: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Search symbol names, signatures and documentation using FTS5.
    /// Query text is treated as plain terms, not as FTS5 syntax.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, IndexError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let Some(query) = fts_query(query) else {
            return Ok(Vec::new());
        };
        let conn = self.conn.lock().unwrap();
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut stmt = conn.prepare(
            "SELECT s.id, s.name, s.qualified_name, s.kind, s.file_path, s.line,
                    s.signature, s.doc, bm25(symbol_fts) AS score
             FROM symbol_fts
             JOIN symbols s ON s.id = symbol_fts.rowid
             WHERE symbol_fts MATCH ?1
             ORDER BY bm25(symbol_fts), s.file_path, s.line, s.id
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit], |row| {
            Ok(SearchResult {
                id: row.get(0)?,
                name: row.get(1)?,
                qualified_name: row.get(2)?,
                kind: row.get(3)?,
                file: row.get(4)?,
                line: row.get(5)?,
                signature: row.get(6)?,
                doc: row.get(7)?,
                score: row.get(8)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Symbols that need an embedding for `model`: either no row exists yet,
    /// or the stored row was produced from different text. The embedding text
    /// includes name, kind, signature, documentation and location, so a symbol
    /// moving files also invalidates its vector.
    pub fn embedding_candidates(&self, model: &str) -> Result<Vec<EmbeddingCandidate>, IndexError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT s.id, s.name, s.qualified_name, s.kind, s.signature, s.doc,
                    s.file_path, f.language, e.text_hash, e.dim, length(e.vector)
             FROM symbols s
             JOIN files f ON f.id = s.file_id
             LEFT JOIN symbol_embeddings e
                    ON e.symbol_id = s.id AND e.model = ?1
             ORDER BY s.id",
        )?;
        let rows = stmt.query_map(params![model], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<i64>>(9)?,
                row.get::<_, Option<i64>>(10)?,
            ))
        })?;

        let mut candidates = Vec::new();
        for row in rows {
            let (
                symbol_id,
                name,
                qualified_name,
                kind,
                signature,
                doc,
                file_path,
                language,
                stored_hash,
                dim,
                blob_len,
            ) = row?;
            let text = embedding_text(
                &name,
                &qualified_name,
                &kind,
                signature.as_deref(),
                doc.as_deref(),
                &file_path,
                language.as_deref(),
            );
            let text_hash = embedding_text_hash(&text);
            let current = match (stored_hash, dim, blob_len) {
                (Some(stored), Some(dim), Some(len)) if dim > 0 => {
                    stored == text_hash && len == dim * 4
                }
                _ => false,
            };
            if !current {
                candidates.push(EmbeddingCandidate {
                    symbol_id,
                    text,
                    text_hash,
                });
            }
        }
        Ok(candidates)
    }

    pub fn embedding_stats(&self, model: &str) -> Result<EmbeddingStats, IndexError> {
        let conn = self.conn.lock().unwrap();
        let total_symbols: i64 =
            conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))?;
        let embedded_symbols: i64 = conn.query_row(
            "SELECT COUNT(*) FROM symbol_embeddings WHERE model = ?1",
            params![model],
            |r| r.get(0),
        )?;
        let dim: Option<i64> = conn
            .query_row(
                "SELECT dim FROM symbol_embeddings WHERE model = ?1 LIMIT 1",
                params![model],
                |r| r.get(0),
            )
            .optional()?;
        drop(conn);

        let stale_symbols = self.embedding_candidates(model)?.len();
        let total_symbols = total_symbols as usize;
        Ok(EmbeddingStats {
            model: model.to_string(),
            total_symbols,
            embedded_symbols: embedded_symbols as usize,
            current_symbols: total_symbols.saturating_sub(stale_symbols),
            stale_symbols,
            dim: dim.map(|value| value as usize),
        })
    }

    /// Store freshly computed vectors. Every input is validated before the
    /// transaction commits, so a bad batch cannot leave a half-updated index.
    pub fn upsert_embeddings(
        &self,
        model: &str,
        inputs: &[EmbeddingInput],
    ) -> Result<usize, IndexError> {
        if model.trim().is_empty() {
            return Err(IndexError::Other(
                "embedding model must not be empty".into(),
            ));
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let mut stored = 0usize;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO symbol_embeddings (symbol_id, model, dim, text_hash, vector)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(symbol_id, model) DO UPDATE SET
                     dim = excluded.dim,
                     text_hash = excluded.text_hash,
                     vector = excluded.vector",
            )?;
            for input in inputs {
                if input.vector.is_empty() || !input.vector.iter().all(|value| value.is_finite()) {
                    return Err(IndexError::Other(format!(
                        "invalid embedding for symbol {}: vector must be non-empty and finite",
                        input.symbol_id
                    )));
                }
                let exists: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM symbols WHERE id = ?1)",
                    params![input.symbol_id],
                    |row| row.get(0),
                )?;
                if !exists {
                    return Err(IndexError::Other(format!(
                        "cannot store an embedding for missing symbol {}",
                        input.symbol_id
                    )));
                }
                let dim = input.vector.len() as i64;
                let text_hash = embedding_text_hash(&input.text);
                let blob = vector_to_blob(&input.vector);
                stmt.execute(params![input.symbol_id, model, dim, text_hash, blob])?;
                stored += 1;
            }
        }
        tx.commit()?;
        Ok(stored)
    }

    /// Rank every embedded symbol by cosine similarity to `query`. Atlas uses
    /// brute force on purpose: symbol counts are small enough that a scan is
    /// fast, and it avoids an approximate-nearest-neighbour index that would
    /// silently miss results.
    pub fn semantic_search(
        &self,
        model: &str,
        query: &[f32],
        limit: usize,
    ) -> Result<Vec<SemanticResult>, IndexError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        if query.is_empty() || !query.iter().all(|value| value.is_finite()) {
            return Err(IndexError::Other(
                "query embedding must be non-empty and finite".into(),
            ));
        }
        let query_norm = query.iter().map(|value| value * value).sum::<f32>().sqrt();
        if query_norm < 1e-9 {
            return Err(IndexError::Other(
                "query embedding must not be the zero vector".into(),
            ));
        }

        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT s.id, s.name, s.qualified_name, s.kind, s.file_path, s.line,
                    s.signature, s.doc, e.dim, e.vector
             FROM symbol_embeddings e
             JOIN symbols s ON s.id = e.symbol_id
             WHERE e.model = ?1",
        )?;
        let rows = stmt.query_map(params![model], |row| {
            Ok((
                SemanticResult {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    qualified_name: row.get(2)?,
                    kind: row.get(3)?,
                    file: row.get(4)?,
                    line: row.get(5)?,
                    signature: row.get(6)?,
                    doc: row.get(7)?,
                    similarity: 0.0,
                },
                row.get::<_, i64>(8)?,
                row.get::<_, Vec<u8>>(9)?,
            ))
        })?;

        let mut results = Vec::new();
        for row in rows {
            let (mut result, dim, blob) = row?;
            if dim <= 0 || dim as usize != query.len() {
                continue;
            }
            let Some(vector) = blob_to_vector(&blob) else {
                continue;
            };
            if vector.len() != query.len() {
                continue;
            }
            let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
            if !norm.is_finite() || norm < 1e-9 {
                continue;
            }
            let dot = query
                .iter()
                .zip(&vector)
                .map(|(left, right)| left * right)
                .sum::<f32>();
            let similarity = (dot / (query_norm * norm)).clamp(-1.0, 1.0);
            if !similarity.is_finite() {
                continue;
            }
            result.similarity = similarity;
            results.push(result);
        }
        drop(stmt);

        results.sort_by(|left, right| {
            right
                .similarity
                .total_cmp(&left.similarity)
                .then_with(|| left.file.cmp(&right.file))
                .then_with(|| left.line.cmp(&right.line))
                .then_with(|| left.id.cmp(&right.id))
        });
        results.truncate(limit);
        Ok(results)
    }

    /// Fetch one symbol by its database id. The desktop app uses ids so two
    /// symbols with the same name are never conflated.
    pub fn symbol_details(&self, id: i64) -> Result<Option<SymbolDetails>, IndexError> {
        let conn = self.conn.lock().unwrap();
        let details = conn
            .query_row(
                "SELECT id, name, qualified_name, kind, file_path, line, column_no,
                        signature, doc
                 FROM symbols WHERE id = ?1",
                params![id],
                |row| {
                    Ok(SymbolDetails {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        qualified_name: row.get(2)?,
                        kind: row.get(3)?,
                        file: row.get(4)?,
                        line: row.get(5)?,
                        column: row.get(6)?,
                        signature: row.get(7)?,
                        doc: row.get(8)?,
                    })
                },
            )
            .optional()?;
        Ok(details)
    }

    /// First symbols in deterministic source order. Used by the desktop app
    /// to show something useful before the user types a search.
    pub fn list_symbols(&self, limit: usize) -> Result<Vec<SymbolDetails>, IndexError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().unwrap();
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut stmt = conn.prepare(
            "SELECT id, name, qualified_name, kind, file_path, line, column_no,
                    signature, doc
             FROM symbols
             ORDER BY file_path, line, id
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(SymbolDetails {
                id: row.get(0)?,
                name: row.get(1)?,
                qualified_name: row.get(2)?,
                kind: row.get(3)?,
                file: row.get(4)?,
                line: row.get(5)?,
                column: row.get(6)?,
                signature: row.get(7)?,
                doc: row.get(8)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Callers of exactly one symbol id. Unresolved incoming calls cannot be
    /// attributed to a definition and therefore cannot appear here.
    pub fn callers_of_symbol(&self, id: i64) -> Result<Vec<RelatedSymbol>, IndexError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT s.id, s.name, s.qualified_name, s.kind, f.path, s.line,
                    e.line, e.resolution
             FROM edges e
             JOIN files f ON f.id = e.file_id
             JOIN symbols s ON s.file_id = e.file_id
                            AND s.qualified_name = e.from_qualified
             WHERE e.dst_symbol_id = ?1
             ORDER BY f.path, e.line, e.id",
        )?;
        let rows = stmt.query_map(params![id], |row| {
            Ok(RelatedSymbol {
                symbol_id: Some(row.get(0)?),
                name: row.get(1)?,
                qualified_name: row.get(2)?,
                kind: Some(row.get(3)?),
                file: row.get(4)?,
                line: row.get(5)?,
                call_line: row.get(6)?,
                resolution: row.get(7)?,
            })
        })?;
        let mut seen: HashSet<(i64, String, usize, usize)> = HashSet::new();
        let mut results = Vec::new();
        for row in rows {
            let row = row?;
            let key = (
                row.symbol_id.unwrap_or_default(),
                row.file.clone(),
                row.line,
                row.call_line,
            );
            if seen.insert(key) {
                results.push(row);
            }
        }
        Ok(results)
    }

    /// Outgoing calls of exactly one symbol id. Unlike callers, this includes
    /// unresolved and external calls so the app can report them honestly.
    pub fn callees_of_symbol(&self, id: i64) -> Result<Vec<RelatedSymbol>, IndexError> {
        let conn = self.conn.lock().unwrap();
        let source: Option<(i64, String)> = conn
            .query_row(
                "SELECT file_id, qualified_name FROM symbols WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((file_id, qualified_name)) = source else {
            return Ok(Vec::new());
        };
        let mut stmt = conn.prepare(
            "SELECT e.dst_symbol_id, COALESCE(s.name, e.to_name),
                    COALESCE(s.qualified_name, e.to_name), s.kind,
                    COALESCE(s.file_path, f.path), COALESCE(s.line, e.line),
                    e.line, e.resolution
             FROM edges e
             JOIN files f ON f.id = e.file_id
             LEFT JOIN symbols s ON s.id = e.dst_symbol_id
             WHERE e.file_id = ?1 AND e.from_qualified = ?2
             ORDER BY e.line, e.id",
        )?;
        let rows = stmt.query_map(params![file_id, qualified_name], |row| {
            Ok(RelatedSymbol {
                symbol_id: row.get(0)?,
                name: row.get(1)?,
                qualified_name: row.get(2)?,
                kind: row.get(3)?,
                file: row.get(4)?,
                line: row.get(5)?,
                call_line: row.get(6)?,
                resolution: row.get(7)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn callers_of(&self, name: &str) -> Result<Vec<CallSite>, IndexError> {
        let conn = self.conn.lock().unwrap();
        // Find symbol(s) matching name
        let mut stmt = conn.prepare(
            "SELECT id, qualified_name FROM symbols WHERE qualified_name = ?1 OR name = ?2 ORDER BY qualified_name",
        )?;
        let symbols: Vec<(i64, String)> = stmt
            .query_map(params![name, name], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        if symbols.is_empty() {
            return Ok(vec![]);
        }
        // If multiple symbols match, we need to be careful - but for now return all callers of any matching symbol
        let mut all_callers = Vec::new();
        for (sym_id, _callee_qname) in symbols {
            let mut stmt = conn.prepare(
                "SELECT e.from_qualified, f.path, e.line, s.qualified_name 
                 FROM edges e 
                 JOIN files f ON e.file_id = f.id 
                 JOIN symbols s ON e.dst_symbol_id = s.id 
                 WHERE e.dst_symbol_id = ?1 
                 ORDER BY f.path, e.line",
            )?;
            let rows = stmt.query_map(params![sym_id], |row| {
                Ok(CallSite {
                    caller_qualified_name: row.get(0)?,
                    caller_file: row.get(1)?,
                    caller_line: row.get(2)?,
                    callee_qualified_name: row.get(3)?,
                })
            })?;
            for r in rows {
                all_callers.push(r?);
            }
            drop(stmt);
        }
        all_callers.sort_by(|a, b| {
            a.caller_file
                .cmp(&b.caller_file)
                .then(a.caller_line.cmp(&b.caller_line))
        });
        Ok(all_callers)
    }

    pub fn callees_of(&self, name: &str) -> Result<Vec<CallSite>, IndexError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT e.to_name, f.path, e.line, COALESCE(s.qualified_name, e.to_name) as callee_q
             FROM edges e 
             JOIN files f ON e.file_id = f.id 
             LEFT JOIN symbols s ON e.dst_symbol_id = s.id
             JOIN symbols caller ON caller.qualified_name = e.from_qualified AND caller.file_id = e.file_id
             WHERE caller.qualified_name = ?1 OR caller.name = ?2
             ORDER BY f.path, e.line",
        )?;
        let mut results = Vec::new();
        let rows = stmt.query_map(params![name, name], |row| {
            Ok(CallSite {
                caller_qualified_name: name.to_string(),
                caller_file: row.get(1)?,
                caller_line: row.get(2)?,
                callee_qualified_name: row.get(3)?,
            })
        })?;
        for r in rows {
            results.push(r?);
        }
        results.sort_by(|a, b| {
            a.caller_file
                .cmp(&b.caller_file)
                .then(a.caller_line.cmp(&b.caller_line))
        });
        Ok(results)
    }

    /// Transitive callers of every symbol matching `name`. The CLI keeps this
    /// name-based entry point; the desktop app uses `impact_of_symbol` so two
    /// same-named definitions are never merged.
    pub fn impact_of(&self, name: &str, max_depth: u32) -> Result<Vec<ImpactEntry>, IndexError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id FROM symbols WHERE qualified_name = ?1 OR name = ?2 ORDER BY id")?;
        let ids: Vec<i64> = stmt
            .query_map(params![name, name], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        drop(stmt);

        let mut merged: Vec<ImpactEntry> = Vec::new();
        let mut positions: HashMap<(String, String, usize), usize> = HashMap::new();
        for id in ids {
            for entry in impact_of_id(&conn, id, max_depth)? {
                let key = (
                    entry.symbol_qualified_name.clone(),
                    entry.file.clone(),
                    entry.line,
                );
                if let Some(&position) = positions.get(&key) {
                    if entry.depth < merged[position].depth {
                        merged[position].depth = entry.depth;
                    }
                } else {
                    positions.insert(key, merged.len());
                    merged.push(entry);
                }
            }
        }
        merged.sort_by(|a, b| {
            a.depth
                .cmp(&b.depth)
                .then(a.symbol_qualified_name.cmp(&b.symbol_qualified_name))
                .then(a.file.cmp(&b.file))
                .then(a.line.cmp(&b.line))
        });
        Ok(merged)
    }

    pub fn impact_of_symbol(
        &self,
        id: i64,
        max_depth: u32,
    ) -> Result<Vec<ImpactEntry>, IndexError> {
        let conn = self.conn.lock().unwrap();
        impact_of_id(&conn, id, max_depth)
    }

    /// Bounded, deterministic call-graph neighborhood. Callers are expanded
    /// upstream and callees downstream; only resolved edges are included,
    /// because an unresolved call has no destination symbol to draw.
    pub fn graph_for_symbol(
        &self,
        id: i64,
        max_depth: u32,
        max_nodes: usize,
    ) -> Result<Option<GraphData>, IndexError> {
        let conn = self.conn.lock().unwrap();
        let mut nodes: HashMap<i64, GraphNode> = HashMap::new();
        {
            let mut stmt = conn
                .prepare("SELECT id, name, qualified_name, kind, file_path, line FROM symbols")?;
            let rows = stmt.query_map([], |row| {
                Ok(GraphNode {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    qualified_name: row.get(2)?,
                    kind: row.get(3)?,
                    file: row.get(4)?,
                    line: row.get(5)?,
                    depth: 0,
                    side: String::new(),
                })
            })?;
            for row in rows {
                let node = row?;
                nodes.insert(node.id, node);
            }
        }
        if !nodes.contains_key(&id) {
            return Ok(None);
        }

        let mut outgoing: HashMap<i64, Vec<GraphEdge>> = HashMap::new();
        let mut incoming: HashMap<i64, Vec<GraphEdge>> = HashMap::new();
        let mut seen_edges: HashSet<(i64, i64, String, usize)> = HashSet::new();
        {
            let mut stmt = conn.prepare(
                "SELECT src.id, e.dst_symbol_id, e.resolution, f.path, e.line
                 FROM edges e
                 JOIN files f ON f.id = e.file_id
                 JOIN symbols src ON src.file_id = e.file_id
                                  AND src.qualified_name = e.from_qualified
                 WHERE e.dst_symbol_id IS NOT NULL
                 ORDER BY f.path, e.line, e.id",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, usize>(4)?,
                ))
            })?;
            for row in rows {
                let (from, to, resolution, file, line) = row?;
                if !nodes.contains_key(&to) {
                    continue;
                }
                let key = (from, to, resolution.clone().unwrap_or_default(), line);
                if !seen_edges.insert(key) {
                    continue;
                }
                let edge = GraphEdge {
                    from,
                    to,
                    resolution,
                    file,
                    line,
                };
                outgoing.entry(from).or_default().push(edge.clone());
                incoming.entry(to).or_default().push(edge);
            }
        }

        let max_nodes = max_nodes.max(1);
        let mut selected: HashMap<i64, (u32, String)> = HashMap::new();
        selected.insert(id, (0, "root".to_string()));
        let mut queue: VecDeque<(i64, u32, String)> = VecDeque::new();
        queue.push_back((id, 0, "root".to_string()));
        let mut truncated = false;

        'bfs: while let Some((node_id, depth, side)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }
            let (expand_outgoing, expand_incoming) = match side.as_str() {
                "caller" => (false, true),
                "callee" => (true, false),
                _ => (true, true),
            };

            if expand_outgoing {
                if let Some(edges) = outgoing.get(&node_id) {
                    let mut neighbors: Vec<(String, i64)> = edges
                        .iter()
                        .map(|edge| {
                            (
                                nodes
                                    .get(&edge.to)
                                    .map(|node| node.qualified_name.clone())
                                    .unwrap_or_default(),
                                edge.to,
                            )
                        })
                        .collect();
                    neighbors.sort();
                    neighbors.dedup_by_key(|(_, neighbor)| *neighbor);
                    for (_, neighbor) in neighbors {
                        if !add_graph_node(
                            &mut selected,
                            &mut queue,
                            neighbor,
                            depth + 1,
                            "callee",
                            max_nodes,
                            &mut truncated,
                        ) {
                            break 'bfs;
                        }
                    }
                }
            }

            if expand_incoming {
                if let Some(edges) = incoming.get(&node_id) {
                    let mut neighbors: Vec<(String, i64)> = edges
                        .iter()
                        .map(|edge| {
                            (
                                nodes
                                    .get(&edge.from)
                                    .map(|node| node.qualified_name.clone())
                                    .unwrap_or_default(),
                                edge.from,
                            )
                        })
                        .collect();
                    neighbors.sort();
                    neighbors.dedup_by_key(|(_, neighbor)| *neighbor);
                    for (_, neighbor) in neighbors {
                        if !add_graph_node(
                            &mut selected,
                            &mut queue,
                            neighbor,
                            depth + 1,
                            "caller",
                            max_nodes,
                            &mut truncated,
                        ) {
                            break 'bfs;
                        }
                    }
                }
            }
        }

        let mut result_nodes: Vec<GraphNode> = selected
            .iter()
            .filter_map(|(node_id, (depth, side))| {
                nodes.get(node_id).cloned().map(|mut node| {
                    node.depth = *depth;
                    node.side = side.clone();
                    node
                })
            })
            .collect();
        result_nodes.sort_by(|a, b| {
            side_rank(&a.side)
                .cmp(&side_rank(&b.side))
                .then(a.depth.cmp(&b.depth))
                .then(a.qualified_name.cmp(&b.qualified_name))
                .then(a.file.cmp(&b.file))
                .then(a.line.cmp(&b.line))
                .then(a.id.cmp(&b.id))
        });

        let selected_ids: HashSet<i64> = selected.keys().copied().collect();
        let mut result_edges: Vec<GraphEdge> = outgoing
            .values()
            .flatten()
            .filter(|edge| selected_ids.contains(&edge.from) && selected_ids.contains(&edge.to))
            .cloned()
            .collect();
        result_edges.sort_by(|a, b| {
            let a_from = &nodes[&a.from];
            let b_from = &nodes[&b.from];
            let a_to = &nodes[&a.to];
            let b_to = &nodes[&b.to];
            a_from
                .qualified_name
                .cmp(&b_from.qualified_name)
                .then(a_to.qualified_name.cmp(&b_to.qualified_name))
                .then(a.file.cmp(&b.file))
                .then(a.line.cmp(&b.line))
        });

        Ok(Some(GraphData {
            nodes: result_nodes,
            edges: result_edges,
            truncated,
        }))
    }

    pub fn stats(&self) -> Result<IndexStats, IndexError> {
        let conn = self.conn.lock().unwrap();
        let symbol_count: i64 = conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))?;
        let edge_count: i64 = conn.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))?;
        let resolved_edge_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM edges WHERE dst_symbol_id IS NOT NULL",
            [],
            |r| r.get(0),
        )?;
        let external_edge_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM edges WHERE dst_symbol_id IS NULL AND resolution = 'external'",
            [],
            |r| r.get(0),
        )?;
        let unresolved_edge_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM edges WHERE dst_symbol_id IS NULL AND COALESCE(resolution, '') <> 'external'",
            [],
            |r| r.get(0),
        )?;
        let file_count: i64 = conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
        Ok(IndexStats {
            symbol_count: symbol_count as usize,
            edge_count: edge_count as usize,
            resolved_edge_count: resolved_edge_count as usize,
            unresolved_edge_count: unresolved_edge_count as usize,
            external_edge_count: external_edge_count as usize,
            file_count: file_count as usize,
        })
    }

    /// Explain how edges were resolved, and list the names that most often
    /// stayed unresolved. This is the data behind `atlas report`.
    pub fn accuracy_report(&self) -> Result<AccuracyReport, IndexError> {
        let conn = self.conn.lock().unwrap();
        let files: i64 = conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
        let symbols: i64 = conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))?;
        let edges: i64 = conn.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))?;
        let resolved: i64 = conn.query_row(
            "SELECT COUNT(*) FROM edges WHERE dst_symbol_id IS NOT NULL",
            [],
            |r| r.get(0),
        )?;

        let mut counts: HashMap<String, usize> = HashMap::new();
        {
            let mut stmt = conn.prepare(
                "SELECT CASE
                     WHEN dst_symbol_id IS NOT NULL THEN COALESCE(resolution, 'legacy_resolved')
                     ELSE COALESCE(resolution, 'unresolved')
                 END AS label,
                 COUNT(*)
                 FROM edges
                 GROUP BY label",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            for row in rows {
                let (label, count) = row?;
                counts.insert(label, count as usize);
            }
        }

        let order = [
            "import",
            "same_file",
            "unique_name",
            "legacy_resolved",
            "external",
            "unresolved",
        ];
        let resolution_counts: Vec<ResolutionCount> = order
            .iter()
            .map(|label| ResolutionCount {
                resolution: (*label).to_string(),
                count: *counts.get(*label).unwrap_or(&0),
            })
            .collect();

        let top_unresolved: Vec<UnresolvedName> = {
            let mut stmt = conn.prepare(
                "SELECT to_name, COUNT(*) AS c
                 FROM edges
                 WHERE dst_symbol_id IS NULL AND COALESCE(resolution, '') <> 'external'
                 GROUP BY to_name
                 ORDER BY c DESC, to_name ASC
                 LIMIT 20",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(UnresolvedName {
                    name: row.get(0)?,
                    count: row.get(1)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        Ok(AccuracyReport {
            files: files as usize,
            symbols: symbols as usize,
            edges: edges as usize,
            resolved: resolved as usize,
            same_file: *counts.get("same_file").unwrap_or(&0),
            import: *counts.get("import").unwrap_or(&0),
            unique_name: *counts.get("unique_name").unwrap_or(&0),
            legacy_resolved: *counts.get("legacy_resolved").unwrap_or(&0),
            external: *counts.get("external").unwrap_or(&0),
            unresolved: *counts.get("unresolved").unwrap_or(&0),
            resolution_counts,
            top_unresolved,
        })
    }
}

fn impact_of_id(
    conn: &Connection,
    id: i64,
    max_depth: u32,
) -> Result<Vec<ImpactEntry>, IndexError> {
    // The path carries its own delimiters so the first id is checked like
    // every other one. This terminates on cycles instead of relying on the
    // depth limit alone.
    let mut stmt = conn.prepare(
        r#"
        WITH RECURSIVE impact(sym_id, depth, path) AS (
            SELECT id, 0, ',' || id || ','
            FROM symbols
            WHERE id = ?1
            UNION ALL
            SELECT src.id, i.depth + 1, i.path || src.id || ','
            FROM impact i
            JOIN edges e ON e.dst_symbol_id = i.sym_id
            JOIN symbols src ON src.file_id = e.file_id
                             AND src.qualified_name = e.from_qualified
            WHERE i.depth < ?2
              AND instr(i.path, ',' || src.id || ',') = 0
        )
        SELECT s.id, s.qualified_name, s.file_path, s.line, MIN(impact.depth) AS d
        FROM impact
        JOIN symbols s ON s.id = impact.sym_id
        WHERE impact.depth > 0
        GROUP BY s.id, s.qualified_name, s.file_path, s.line
        ORDER BY d, s.qualified_name, s.file_path, s.line
        "#,
    )?;
    let rows = stmt.query_map(params![id, max_depth as i64], |row| {
        Ok(ImpactEntry {
            symbol_id: row.get(0)?,
            symbol_qualified_name: row.get(1)?,
            file: row.get(2)?,
            line: row.get(3)?,
            depth: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn add_graph_node(
    selected: &mut HashMap<i64, (u32, String)>,
    queue: &mut VecDeque<(i64, u32, String)>,
    node_id: i64,
    depth: u32,
    side: &str,
    max_nodes: usize,
    truncated: &mut bool,
) -> bool {
    if let Some((_, existing_side)) = selected.get_mut(&node_id) {
        if existing_side != "root" && existing_side != side {
            *existing_side = "both".to_string();
        }
        return true;
    }
    if selected.len() >= max_nodes {
        *truncated = true;
        return false;
    }
    selected.insert(node_id, (depth, side.to_string()));
    queue.push_back((node_id, depth, side.to_string()));
    true
}

fn side_rank(side: &str) -> u8 {
    match side {
        "root" => 0,
        "caller" => 1,
        "callee" => 2,
        _ => 3,
    }
}

/// Build the text that is embedded for a symbol. The order matters: the
/// declaration and signature come first so they survive the model's token
/// limit even when a doc comment is long.
fn embedding_text(
    name: &str,
    qualified_name: &str,
    kind: &str,
    signature: Option<&str>,
    doc: Option<&str>,
    file_path: &str,
    language: Option<&str>,
) -> String {
    let mut parts = vec![format!("{kind} {qualified_name}")];
    if name != qualified_name {
        parts.push(format!("name: {name}"));
    }
    if let Some(signature) = signature.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(format!("signature: {signature}"));
    }
    if let Some(doc) = doc.map(str::trim).filter(|value| !value.is_empty()) {
        let truncated: String = doc.chars().take(EMBEDDING_DOC_CHARS).collect();
        parts.push(format!("doc: {truncated}"));
    }
    parts.push(format!("file: {file_path}"));
    if let Some(language) = language.filter(|value| !value.is_empty()) {
        parts.push(format!("language: {language}"));
    }
    parts.join("\n")
}

fn embedding_text_hash(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

fn vector_to_blob(vector: &[f32]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        blob.extend_from_slice(&value.to_le_bytes());
    }
    blob
}

fn blob_to_vector(blob: &[u8]) -> Option<Vec<f32>> {
    if blob.is_empty() || !blob.len().is_multiple_of(4) {
        return None;
    }
    let (chunks, _) = blob.as_chunks::<4>();
    let mut vector = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        vector.push(f32::from_le_bytes(*chunk));
    }
    Some(vector)
}

fn init_schema(conn: &mut Connection) -> Result<(), IndexError> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let has_files: bool = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'files'",
        [],
        |row| row.get::<_, i64>(0),
    )? > 0;

    if !has_files {
        conn.execute_batch(SCHEMA_SQL)?;
        conn.execute_batch(INDEX_SQL)?;
        return Ok(());
    }

    if version > SCHEMA_VERSION {
        return Err(IndexError::Other(format!(
            "database schema version {version} is newer than supported version \
             {SCHEMA_VERSION}; delete the database and re-index"
        )));
    }

    if version >= SCHEMA_VERSION {
        // Make sure indexes and FTS objects exist even if a database was
        // created by an earlier build of the same schema version.
        let had_fts: bool = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'symbol_fts'",
            [],
            |row| row.get::<_, i64>(0),
        )? > 0;
        conn.execute_batch(INDEX_SQL)?;
        if !had_fts {
            conn.execute("INSERT INTO symbol_fts(symbol_fts) VALUES('rebuild')", [])?;
        }
        return Ok(());
    }

    // v1/v2/v3 -> v4. Old databases are upgraded in place; no data is
    // discarded. The FTS index is rebuilt from the existing symbols. The
    // embeddings table starts empty: vectors are model-specific and must be
    // generated by `atlas embed` after the upgrade.
    let tx = conn.transaction()?;
    let edge_columns = table_columns(&tx, "edges")?;
    if !edge_columns.iter().any(|name| name == "resolution") {
        tx.execute("ALTER TABLE edges ADD COLUMN resolution TEXT", [])?;
    }
    let symbol_columns = table_columns(&tx, "symbols")?;
    if !symbol_columns.iter().any(|name| name == "signature") {
        tx.execute("ALTER TABLE symbols ADD COLUMN signature TEXT", [])?;
    }
    if !symbol_columns.iter().any(|name| name == "doc") {
        tx.execute("ALTER TABLE symbols ADD COLUMN doc TEXT", [])?;
    }
    tx.execute_batch(INDEX_SQL)?;
    tx.execute_batch(
        "INSERT INTO symbol_fts(symbol_fts) VALUES('rebuild');
         CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);
         INSERT OR REPLACE INTO schema_version (rowid, version) VALUES (1, 4);
         PRAGMA user_version = 4;",
    )?;
    tx.commit()?;
    Ok(())
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>, IndexError> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(names)
}

#[derive(Debug, Clone)]
struct FileInfo {
    path: String,
    language: Option<String>,
}

#[derive(Debug, Clone)]
struct ImportRow {
    module: String,
    name: Option<String>,
    alias: Option<String>,
}

#[derive(Debug)]
enum ImportMatch {
    External,
    /// The call matches an import, but the target is not in the repository
    /// (for example a relative Python/TypeScript import to a missing file).
    /// The edge must stay unresolved instead of falling back to a global
    /// name match.
    Unresolved,
    Internal(Vec<(String, String)>),
    /// A glob import (`use foo::*`, `from foo import *`). It can suggest a
    /// target, but it binds every name, so a miss must not shadow ordinary
    /// name matching. External glob imports are ignored entirely because
    /// attributing an arbitrary call to them would be a guess.
    WildcardInternal(Vec<(String, String)>),
}

enum SymbolLookup {
    None,
    One(i64),
    Many,
}

/// Cheap filter before the full import resolution: only imports that could
/// bind the first segment of the call are relevant. This keeps resolution
/// linear-ish on repositories with many imports per file.
fn import_may_match(import: &ImportRow, to_name: &str, language: &str) -> bool {
    match language {
        "rust" => match import.name.as_deref() {
            Some(name) => {
                let binding = import.alias.as_deref().unwrap_or(name);
                to_name.split("::").next() == Some(binding)
            }
            // Glob import: only plain calls can be attributed to it.
            None => !to_name.contains("::") && !to_name.contains('.'),
        },
        "python" => match import.name.as_deref() {
            Some("*") => !to_name.contains('.'),
            Some(name) => {
                let binding = import.alias.as_deref().unwrap_or(name);
                to_name.split('.').next() == Some(binding)
            }
            None => {
                let binding = import
                    .alias
                    .as_deref()
                    .unwrap_or_else(|| import.module.split('.').next().unwrap_or(""));
                to_name.split('.').next() == Some(binding)
            }
        },
        "typescript" => match import.name.as_deref() {
            Some("*") => import
                .alias
                .as_deref()
                .is_some_and(|alias| to_name.split('.').next() == Some(alias)),
            Some(name) => {
                let binding = import.alias.as_deref().unwrap_or(name);
                to_name.split('.').next() == Some(binding)
            }
            None => import
                .alias
                .as_deref()
                .is_some_and(|alias| to_name.split('.').next() == Some(alias)),
        },
        _ => false,
    }
}

fn resolve_import_match(
    import: &ImportRow,
    to_name: &str,
    caller: &FileInfo,
    caller_rust_module: Option<&RustModulePath>,
    rust_modules: &HashMap<(String, Vec<String>), Vec<String>>,
    all_files: &HashSet<String>,
) -> Option<ImportMatch> {
    match caller.language.as_deref() {
        Some("rust") => {
            let module = caller_rust_module?;
            resolve_rust_import(import, to_name, module, rust_modules)
        }
        Some("python") => resolve_python_import(import, to_name, &caller.path, all_files),
        Some("typescript") => resolve_ts_import(import, to_name, &caller.path, all_files),
        _ => None,
    }
}

struct SymbolRow {
    id: i64,
    qualified_name: String,
    file_path: String,
    language: String,
}

/// In-memory indexes over the symbol table. Resolution runs once per file
/// batch and touches every edge, so querying SQLite per edge is too slow.
#[derive(Default)]
struct SymbolMaps {
    all: Vec<SymbolRow>,
    by_name: HashMap<String, Vec<usize>>,
    by_qualified: HashMap<String, Vec<usize>>,
    by_suffix: HashMap<String, Vec<usize>>,
    by_file_name: HashMap<(String, String), Vec<usize>>,
    by_file_qualified: HashMap<(String, String), Vec<usize>>,
    by_file_suffix: HashMap<(String, String), Vec<usize>>,
}

impl SymbolMaps {
    fn load(conn: &Connection) -> Result<Self, IndexError> {
        let mut stmt = conn.prepare(
            "SELECT s.id, s.name, s.qualified_name, s.file_path, COALESCE(f.language, '')
             FROM symbols s JOIN files f ON s.file_id = f.id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut maps = SymbolMaps::default();
        for row in rows {
            let (id, name, qualified_name, file_path, language) = row?;
            let idx = maps.all.len();
            let suffix = last_name(&qualified_name).to_string();
            maps.by_name.entry(name.clone()).or_default().push(idx);
            maps.by_qualified
                .entry(qualified_name.clone())
                .or_default()
                .push(idx);
            maps.by_suffix.entry(suffix.clone()).or_default().push(idx);
            maps.by_file_name
                .entry((file_path.clone(), name))
                .or_default()
                .push(idx);
            maps.by_file_qualified
                .entry((file_path.clone(), qualified_name.clone()))
                .or_default()
                .push(idx);
            maps.by_file_suffix
                .entry((file_path.clone(), suffix))
                .or_default()
                .push(idx);
            maps.all.push(SymbolRow {
                id,
                qualified_name,
                file_path,
                language,
            });
        }
        Ok(maps)
    }

    fn find_unique_symbol(&self, candidates: &[(String, String)], language: &str) -> SymbolLookup {
        let mut ids: HashSet<i64> = HashSet::new();
        for (file, symbol) in candidates {
            let key = (file.clone(), symbol.clone());
            for map in [
                &self.by_file_name,
                &self.by_file_qualified,
                &self.by_file_suffix,
            ] {
                if let Some(indices) = map.get(&key) {
                    for idx in indices {
                        let row = &self.all[*idx];
                        if row.language == language {
                            ids.insert(row.id);
                        }
                    }
                }
            }
        }
        if ids.is_empty() {
            return SymbolLookup::None;
        }
        if ids.len() == 1 {
            return SymbolLookup::One(*ids.iter().next().unwrap());
        }
        // Several symbols in the target file may share a name (for example two
        // impl blocks). Prefer the top-level definition when there is exactly one.
        let mut exact: HashSet<i64> = HashSet::new();
        for (file, symbol) in candidates {
            let key = (file.clone(), symbol.clone());
            if let Some(indices) = self.by_file_qualified.get(&key) {
                for idx in indices {
                    let row = &self.all[*idx];
                    if row.language == language {
                        exact.insert(row.id);
                    }
                }
            }
        }
        if exact.len() == 1 {
            SymbolLookup::One(*exact.iter().next().unwrap())
        } else {
            SymbolLookup::Many
        }
    }

    fn fallback_candidates(&self, to_name: &str, language: &str) -> Vec<(i64, String, String)> {
        let suffix = last_name(to_name);
        let mut ids: HashSet<usize> = HashSet::new();
        for (map, key) in [
            (&self.by_qualified, to_name),
            (&self.by_name, to_name),
            (&self.by_name, suffix),
            (&self.by_suffix, suffix),
        ] {
            if let Some(indices) = map.get(key) {
                ids.extend(indices.iter().copied());
            }
        }
        let mut out: Vec<(i64, String, String)> = ids
            .into_iter()
            .filter_map(|idx| {
                let row = &self.all[idx];
                (row.language == language)
                    .then(|| (row.id, row.qualified_name.clone(), row.file_path.clone()))
            })
            .collect();
        out.sort_by_key(|(id, _, _)| *id);
        out
    }
}

fn last_name(name: &str) -> &str {
    if let Some(pos) = name.rfind("::") {
        &name[pos + 2..]
    } else if let Some(pos) = name.rfind('.') {
        &name[pos + 1..]
    } else {
        name
    }
}

// ---------------------------------------------------------------------------
// Rust
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct RustModulePath {
    crate_root: String,
    path: Vec<String>,
}

enum RustModuleResolution {
    Internal(Vec<String>),
    External,
    Missing,
}

/// Best-effort mapping from a repository-relative Rust file to its module
/// path. The crate root is the nearest `src` directory. `lib.rs`, `main.rs`
/// and `mod.rs` are the module itself; other files contribute their stem.
/// Limits: workspace members are handled independently, and `src/bin/*`
/// binaries are treated as modules under `src` rather than separate crates.
fn rust_module_of(file: &str) -> Option<RustModulePath> {
    let normalized = file.replace('\\', "/");
    let (crate_root, rest): (String, &str) = if let Some(pos) = normalized.rfind("/src/") {
        (normalized[..pos + 4].to_string(), &normalized[pos + 5..])
    } else if let Some(rest) = normalized.strip_prefix("src/") {
        ("src".to_string(), rest)
    } else if let Some(after_crates) = normalized.strip_prefix("crates/") {
        // Workspace crates often live at `crates/<name>/` without a `src`
        // directory (for example ripgrep's `crates/core`). Treat the first
        // two segments as the crate root.
        match after_crates.split_once('/') {
            Some((name, rest)) if !name.is_empty() && !rest.is_empty() => {
                (format!("crates/{}", name), rest)
            }
            _ => (String::new(), normalized.as_str()),
        }
    } else {
        (String::new(), normalized.as_str())
    };
    if rest.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = rest.split('/').map(|s| s.to_string()).collect();
    let last = parts.last()?.strip_suffix(".rs")?.to_string();
    parts.pop();
    if last != "lib" && last != "main" && last != "mod" {
        parts.push(last);
    }
    Some(RustModulePath {
        crate_root,
        path: parts,
    })
}

fn rust_resolve_module(
    module: &str,
    caller: &RustModulePath,
    modules: &HashMap<(String, Vec<String>), Vec<String>>,
) -> RustModuleResolution {
    let segments: Vec<&str> = if module.is_empty() {
        Vec::new()
    } else {
        module.split("::").collect()
    };
    let mut base: Vec<String> = Vec::new();
    let mut index = 0;
    let mut explicit_internal = false;
    if let Some(first) = segments.first() {
        match *first {
            "crate" => {
                explicit_internal = true;
                index = 1;
            }
            "self" => {
                explicit_internal = true;
                base = caller.path.clone();
                index = 1;
            }
            "super" => {
                explicit_internal = true;
                base = caller.path.clone();
                while index < segments.len() && segments[index] == "super" {
                    if base.pop().is_none() {
                        return RustModuleResolution::Missing;
                    }
                    index += 1;
                }
            }
            _ => {}
        }
    }
    for segment in &segments[index..] {
        base.push((*segment).to_string());
    }
    let key = (caller.crate_root.clone(), base);
    if let Some(files) = modules.get(&key) {
        RustModuleResolution::Internal(files.clone())
    } else if explicit_internal {
        RustModuleResolution::Missing
    } else {
        RustModuleResolution::External
    }
}

fn resolve_rust_import(
    import: &ImportRow,
    to_name: &str,
    caller: &RustModulePath,
    modules: &HashMap<(String, Vec<String>), Vec<String>>,
) -> Option<ImportMatch> {
    match import.name.as_deref() {
        None => {
            // Glob import (`use crate::foo::*;`): the call may name any item
            // from that module.
            let (target_module, symbol) = match to_name.rsplit_once("::") {
                Some((prefix, last)) => (join_module(&import.module, prefix), last),
                None => (import.module.clone(), to_name),
            };
            match rust_resolve_module(&target_module, caller, modules) {
                RustModuleResolution::Internal(files) => Some(ImportMatch::WildcardInternal(
                    files
                        .into_iter()
                        .map(|file| (file, symbol.to_string()))
                        .collect(),
                )),
                _ => match rust_resolve_module(&import.module, caller, modules) {
                    RustModuleResolution::Internal(files) => Some(ImportMatch::WildcardInternal(
                        files
                            .into_iter()
                            .map(|file| (file, symbol.to_string()))
                            .collect(),
                    )),
                    // An external glob could name anything; attributing the
                    // call to it would be a guess.
                    RustModuleResolution::External => None,
                    RustModuleResolution::Missing => Some(ImportMatch::Unresolved),
                },
            }
        }
        Some(name) => {
            let local = import.alias.as_deref().unwrap_or(name);
            if to_name == local {
                return match rust_resolve_module(&import.module, caller, modules) {
                    RustModuleResolution::Internal(files) => Some(ImportMatch::Internal(
                        files
                            .into_iter()
                            .map(|file| (file, name.to_string()))
                            .collect(),
                    )),
                    RustModuleResolution::External => Some(ImportMatch::External),
                    RustModuleResolution::Missing => Some(ImportMatch::Unresolved),
                };
            }
            // Qualified call through the imported item, e.g. `bar::baz()` or
            // `Type::new()`.
            let rest = to_name.strip_prefix(&format!("{}::", local))?;
            let (rest_prefix, symbol) = match rest.rsplit_once("::") {
                Some((prefix, last)) => (prefix, last),
                None => ("", rest),
            };
            let item_module = join_module(&import.module, name);
            let mut candidates = Vec::new();
            // The imported name may be a module (`bar::baz`) or a type
            // (`Bar::new`); try both interpretations.
            for base in [item_module.as_str(), import.module.as_str()] {
                let target = join_module(base, rest_prefix);
                if let RustModuleResolution::Internal(files) =
                    rust_resolve_module(&target, caller, modules)
                {
                    for file in files {
                        candidates.push((file, symbol.to_string()));
                    }
                }
            }
            candidates.sort();
            candidates.dedup();
            if !candidates.is_empty() {
                return Some(ImportMatch::Internal(candidates));
            }
            match rust_resolve_module(&import.module, caller, modules) {
                RustModuleResolution::External => Some(ImportMatch::External),
                RustModuleResolution::Missing => Some(ImportMatch::Unresolved),
                RustModuleResolution::Internal(_) => None,
            }
        }
    }
}

fn join_module(left: &str, right: &str) -> String {
    if left.is_empty() {
        right.to_string()
    } else if right.is_empty() {
        left.to_string()
    } else {
        format!("{}::{}", left, right)
    }
}

// ---------------------------------------------------------------------------
// Python
// ---------------------------------------------------------------------------

fn resolve_python_import(
    import: &ImportRow,
    to_name: &str,
    caller_file: &str,
    all_files: &HashSet<String>,
) -> Option<ImportMatch> {
    let module = import.module.as_str();
    match import.name.as_deref() {
        Some("*") => {
            // `from pkg import *`: the call may name any item from pkg.
            let (prefix, symbol) = split_last_dot(to_name);
            let target_module = join_dot(module, prefix);
            if let Some(files) = python_module_files(&target_module, caller_file, all_files) {
                return Some(ImportMatch::WildcardInternal(
                    files
                        .into_iter()
                        .map(|file| (file, symbol.to_string()))
                        .collect(),
                ));
            }
            if let Some(files) = python_module_files(module, caller_file, all_files) {
                return Some(ImportMatch::WildcardInternal(
                    files
                        .into_iter()
                        .map(|file| (file, symbol.to_string()))
                        .collect(),
                ));
            }
            if module.starts_with('.') {
                Some(ImportMatch::Unresolved)
            } else {
                // External glob: cannot attribute the call without guessing.
                None
            }
        }
        Some(name) => {
            let local = import.alias.as_deref().unwrap_or(name);
            if to_name == local {
                let mut candidates = Vec::new();
                if let Some(files) = python_module_files(module, caller_file, all_files) {
                    for file in files {
                        candidates.push((file, name.to_string()));
                    }
                }
                for file in python_submodule_files(module, name, caller_file, all_files) {
                    candidates.push((file, name.to_string()));
                }
                if candidates.is_empty() {
                    python_missing_match(module)
                } else {
                    Some(ImportMatch::Internal(candidates))
                }
            } else if let Some(rest) = to_name.strip_prefix(&format!("{}.", local)) {
                let (rest_prefix, symbol) = split_last_dot(rest);
                let item_module = join_dot(module, name);
                let mut candidates = Vec::new();
                for base in [
                    join_dot(&item_module, rest_prefix),
                    join_dot(module, rest_prefix),
                    module.to_string(),
                ] {
                    if let Some(files) = python_module_files(&base, caller_file, all_files) {
                        for file in files {
                            candidates.push((file, symbol.to_string()));
                        }
                    }
                }
                for file in python_submodule_files(module, name, caller_file, all_files) {
                    candidates.push((file, symbol.to_string()));
                }
                candidates.sort();
                candidates.dedup();
                if candidates.is_empty() {
                    python_missing_match(module)
                } else {
                    Some(ImportMatch::Internal(candidates))
                }
            } else {
                None
            }
        }
        None => {
            // `import a.b` / `import a.b as c`: a module import. Python binds
            // the first segment unless an alias is given.
            let binding = import
                .alias
                .as_deref()
                .unwrap_or_else(|| module.split('.').next().unwrap_or(module));
            let rest = to_name.strip_prefix(&format!("{}.", binding))?;
            let module_segments: Vec<&str> = if module.is_empty() {
                Vec::new()
            } else {
                module.split('.').collect()
            };
            let mut rest_segments: Vec<&str> = rest.split('.').collect();
            if import.alias.is_none() {
                let module_tail = &module_segments[1.min(module_segments.len())..];
                if rest_segments.len() < module_tail.len()
                    || rest_segments[..module_tail.len()] != *module_tail
                {
                    // The call path does not go through the imported module.
                    return None;
                }
                rest_segments.drain(..module_tail.len());
            }
            if rest_segments.is_empty() {
                return None;
            }
            let symbol = rest_segments.pop().unwrap();
            let mut target_segments = module_segments;
            target_segments.extend(rest_segments);
            let target_module = target_segments.join(".");
            match python_module_files(&target_module, caller_file, all_files) {
                Some(files) => Some(ImportMatch::Internal(
                    files
                        .into_iter()
                        .map(|file| (file, symbol.to_string()))
                        .collect(),
                )),
                None => python_missing_match(module),
            }
        }
    }
}

fn python_missing_match(module: &str) -> Option<ImportMatch> {
    if module.is_empty() || module.starts_with('.') {
        Some(ImportMatch::Unresolved)
    } else {
        Some(ImportMatch::External)
    }
}

fn is_package_only(module: &str) -> bool {
    !module.is_empty() && module.chars().all(|c| c == '.')
}

fn python_module_base(module: &str, caller_file: &str) -> Option<PathBuf> {
    if module.starts_with('.') {
        let dots = module.chars().take_while(|c| *c == '.').count();
        let rest = &module[dots..];
        let mut base = Path::new(caller_file).parent()?.to_path_buf();
        for _ in 1..dots {
            base = base.parent()?.to_path_buf();
        }
        if !rest.is_empty() {
            base = base.join(rest.replace('.', "/"));
        }
        Some(base)
    } else if module.is_empty() {
        None
    } else {
        Some(PathBuf::from(module.replace('.', "/")))
    }
}

fn python_module_files(
    module: &str,
    caller_file: &str,
    all_files: &HashSet<String>,
) -> Option<Vec<String>> {
    let base = python_module_base(module, caller_file)?;
    let mut candidates = Vec::new();
    if is_package_only(module) {
        push_candidate(&mut candidates, base.join("__init__.py"), all_files);
    } else {
        push_candidate(&mut candidates, base.with_extension("py"), all_files);
        push_candidate(&mut candidates, base.join("__init__.py"), all_files);
    }
    if candidates.is_empty() {
        None
    } else {
        Some(candidates)
    }
}

fn python_submodule_files(
    module: &str,
    name: &str,
    caller_file: &str,
    all_files: &HashSet<String>,
) -> Vec<String> {
    let base = match python_module_base(module, caller_file) {
        Some(base) => base,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    push_candidate(&mut out, base.join(format!("{}.py", name)), all_files);
    push_candidate(&mut out, base.join(name).join("__init__.py"), all_files);
    out
}

fn split_last_dot(path: &str) -> (&str, &str) {
    match path.rsplit_once('.') {
        Some((prefix, last)) => (prefix, last),
        None => ("", path),
    }
}

fn join_dot(left: &str, right: &str) -> String {
    if left.is_empty() {
        right.to_string()
    } else if right.is_empty() {
        left.to_string()
    } else {
        format!("{}.{}", left, right)
    }
}

// ---------------------------------------------------------------------------
// TypeScript
// ---------------------------------------------------------------------------

fn resolve_ts_import(
    import: &ImportRow,
    to_name: &str,
    caller_file: &str,
    all_files: &HashSet<String>,
) -> Option<ImportMatch> {
    let module_files = ts_module_files(&import.module, caller_file, all_files);
    let is_relative = import.module.starts_with('.') || import.module.starts_with('/');
    let external_or_none = || {
        if is_relative {
            Some(ImportMatch::Unresolved)
        } else {
            Some(ImportMatch::External)
        }
    };
    match import.name.as_deref() {
        Some("*") => {
            // `import * as ns from "./mod"; ns.foo()`
            let local = import.alias.as_deref()?;
            let rest = to_name.strip_prefix(&format!("{}.", local))?;
            let symbol = last_dot_segment(rest);
            match module_files {
                Some(files) => Some(ImportMatch::Internal(
                    files
                        .into_iter()
                        .map(|file| (file, symbol.to_string()))
                        .collect(),
                )),
                None => external_or_none(),
            }
        }
        Some(name) => {
            let local = import.alias.as_deref().unwrap_or(name);
            if to_name == local {
                match module_files {
                    Some(files) => Some(ImportMatch::Internal(
                        files
                            .into_iter()
                            .map(|file| (file, name.to_string()))
                            .collect(),
                    )),
                    None => external_or_none(),
                }
            } else if let Some(rest) = to_name.strip_prefix(&format!("{}.", local)) {
                let symbol = last_dot_segment(rest);
                match module_files {
                    Some(files) => Some(ImportMatch::Internal(
                        files
                            .into_iter()
                            .map(|file| (file, symbol.to_string()))
                            .collect(),
                    )),
                    None => external_or_none(),
                }
            } else {
                None
            }
        }
        None => {
            // Default import (`import foo from "./foo"`) or a side-effect
            // import with no local binding.
            let local = import.alias.as_deref()?;
            if to_name == local {
                match module_files {
                    Some(files) => Some(ImportMatch::Internal(
                        files
                            .into_iter()
                            .map(|file| (file, local.to_string()))
                            .collect(),
                    )),
                    None => external_or_none(),
                }
            } else if let Some(rest) = to_name.strip_prefix(&format!("{}.", local)) {
                let symbol = last_dot_segment(rest);
                match module_files {
                    Some(files) => Some(ImportMatch::Internal(
                        files
                            .into_iter()
                            .map(|file| (file, symbol.to_string()))
                            .collect(),
                    )),
                    None => external_or_none(),
                }
            } else {
                None
            }
        }
    }
}

fn last_dot_segment(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or(path)
}

fn ts_module_files(
    module: &str,
    caller_file: &str,
    all_files: &HashSet<String>,
) -> Option<Vec<String>> {
    if !module.starts_with('.') && !module.starts_with('/') {
        return None;
    }
    let base = if module.starts_with('/') {
        PathBuf::from(module.trim_start_matches('/'))
    } else {
        let parent = Path::new(caller_file)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        parent.join(module)
    };
    let normalized = normalize_relative_path(&base)?;
    let normalized_str = normalized.to_string_lossy().replace('\\', "/");
    // TypeScript source sometimes imports `./foo.js` while the file on disk
    // is `foo.ts`; strip the JS extension before trying TS candidates.
    let base = if normalized_str.ends_with(".js") {
        PathBuf::from(normalized_str.trim_end_matches(".js"))
    } else {
        normalized
    };
    let mut candidates = Vec::new();
    let base_str = base.to_string_lossy().replace('\\', "/");
    if base_str.ends_with(".ts") || base_str.ends_with(".tsx") {
        push_candidate(&mut candidates, base.clone(), all_files);
    } else {
        push_candidate(&mut candidates, base.with_extension("ts"), all_files);
        push_candidate(&mut candidates, base.with_extension("tsx"), all_files);
        push_candidate(&mut candidates, base.join("index.ts"), all_files);
        push_candidate(&mut candidates, base.join("index.tsx"), all_files);
    }
    if candidates.is_empty() {
        None
    } else {
        Some(candidates)
    }
}

fn normalize_relative_path(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::Normal(part) => out.push(part),
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

fn push_candidate(out: &mut Vec<String>, path: PathBuf, all_files: &HashSet<String>) {
    let path = path.to_string_lossy().replace('\\', "/");
    if all_files.contains(&path) {
        out.push(path);
    }
}

/// Turn arbitrary user input into a safe FTS5 query. Each whitespace-separated
/// term is quoted so FTS operators cannot change the meaning, and the last
/// token of each term is a prefix so `parse` also finds `parse_file`.
fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split_whitespace()
        .filter_map(|term| {
            let term = term.trim_matches(|c: char| {
                !(c.is_alphanumeric() || matches!(c, '_' | '$' | ':' | '.' | '#'))
            });
            if !term.chars().any(char::is_alphanumeric) {
                return None;
            }
            Some(format!("\"{}\"*", term.replace('"', "\"\"")))
        })
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" AND "))
    }
}
