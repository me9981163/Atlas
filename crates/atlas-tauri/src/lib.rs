use atlas_embed::{
    default_store, load_cached, DownloadProgress, EmbedError, ModelStatus, DEFAULT_MODEL,
    MODEL_LICENSE,
};
use atlas_index::{
    AccuracyReport, EmbeddingStats, GraphData, ImpactEntry, Index, IndexStats, RelatedSymbol,
    SearchResult, SemanticResult, SymbolDetails,
};
use atlas_scan::{scan_directory, ScanSummary};
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tauri::{Emitter, State};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Message(String),
    #[error("Database error: {0}")]
    Index(#[from] atlas_index::IndexError),
    #[error("Scan error: {0}")]
    Scan(String),
    #[error("File system error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Semantic model error: {0}")]
    Embed(#[from] EmbedError),
}

#[derive(Debug, Clone, Serialize)]
pub struct RepositoryInfo {
    pub root: String,
    pub db_path: String,
    pub indexed: bool,
    pub stats: IndexStats,
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexOutcome {
    pub summary: ScanSummary,
    pub stats: IndexStats,
}

/// Everything the desktop app needs to explain the semantic index state:
/// whether the model is installed, where it lives, and how many symbols have
/// current vectors.
#[derive(Debug, Clone, Serialize)]
pub struct SemanticStatus {
    pub model: String,
    pub model_license: String,
    pub model_installed: bool,
    pub model_path: String,
    pub model_size_bytes: u64,
    pub model_dim: usize,
    pub total_symbols: usize,
    pub embedded_symbols: usize,
    pub current_symbols: usize,
    pub stale_symbols: usize,
    pub embedding_dim: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbedProgress {
    pub done: usize,
    pub total: usize,
}

struct Session {
    root: PathBuf,
    db_path: PathBuf,
    index: Index,
}

impl Session {
    fn open(root: PathBuf) -> Result<Self, AppError> {
        let atlas_dir = root.join(".atlas");
        fs::create_dir_all(&atlas_dir)?;
        let db_path = atlas_dir.join("atlas.db");
        let index = Index::open(&db_path)?;
        Ok(Self {
            root,
            db_path,
            index,
        })
    }

    fn info(&self) -> Result<RepositoryInfo, AppError> {
        let stats = self.index.stats()?;
        Ok(RepositoryInfo {
            root: self.root.display().to_string(),
            db_path: self.db_path.display().to_string(),
            indexed: stats.file_count > 0,
            stats,
        })
    }

    fn index(&self) -> Result<IndexOutcome, AppError> {
        let summary = scan_directory(&self.root, &self.index)
            .map_err(|error| AppError::Scan(error.to_string()))?;
        let stats = self.index.stats()?;
        Ok(IndexOutcome { summary, stats })
    }
}

/// Application state shared by all Tauri commands. The session is replaced
/// atomically when a new repository is opened, so a failed open leaves the
/// previous repository usable.
#[derive(Default)]
pub struct AtlasState {
    session: RwLock<Option<Arc<Session>>>,
}

impl AtlasState {
    pub fn open(&self, path: &str) -> Result<RepositoryInfo, AppError> {
        let root = validate_repository_path(path)?;
        let session = Session::open(root)?;
        let info = session.info()?;
        *self.session.write().unwrap() = Some(Arc::new(session));
        Ok(info)
    }

    pub fn close(&self) {
        *self.session.write().unwrap() = None;
    }

    pub fn info(&self) -> Result<Option<RepositoryInfo>, AppError> {
        let session = self.session.read().unwrap().clone();
        session.as_ref().map(|session| session.info()).transpose()
    }

    pub fn index(&self) -> Result<IndexOutcome, AppError> {
        self.session()?.index()
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, AppError> {
        Ok(self.session()?.index.search(query, limit.clamp(1, 200))?)
    }

    pub fn list_symbols(&self, limit: usize) -> Result<Vec<SymbolDetails>, AppError> {
        Ok(self.session()?.index.list_symbols(limit.clamp(1, 200))?)
    }

    pub fn symbol(&self, id: i64) -> Result<SymbolDetails, AppError> {
        self.session()?
            .index
            .symbol_details(id)?
            .ok_or_else(|| missing_symbol(id))
    }

    pub fn callers(&self, id: i64) -> Result<Vec<RelatedSymbol>, AppError> {
        Ok(self.session()?.index.callers_of_symbol(id)?)
    }

    pub fn callees(&self, id: i64) -> Result<Vec<RelatedSymbol>, AppError> {
        Ok(self.session()?.index.callees_of_symbol(id)?)
    }

    pub fn impact(&self, id: i64, depth: u32) -> Result<Vec<ImpactEntry>, AppError> {
        Ok(self
            .session()?
            .index
            .impact_of_symbol(id, depth.clamp(1, 20))?)
    }

    pub fn graph(&self, id: i64, depth: u32, limit: usize) -> Result<GraphData, AppError> {
        self.session()?
            .index
            .graph_for_symbol(id, depth.clamp(0, 10), limit.clamp(1, 200))?
            .ok_or_else(|| missing_symbol(id))
    }

    pub fn report(&self) -> Result<AccuracyReport, AppError> {
        Ok(self.session()?.index.accuracy_report()?)
    }

    pub fn semantic_status(&self) -> Result<SemanticStatus, AppError> {
        let session = self.session()?;
        let store = default_store()?;
        let model = store.status(DEFAULT_MODEL)?;
        let stats = session.index.embedding_stats(DEFAULT_MODEL)?;
        Ok(SemanticStatus {
            model: model.model,
            model_license: MODEL_LICENSE.to_string(),
            model_installed: model.installed,
            model_path: model.path,
            model_size_bytes: model.size_bytes,
            model_dim: model.dim,
            total_symbols: stats.total_symbols,
            embedded_symbols: stats.embedded_symbols,
            current_symbols: stats.current_symbols,
            stale_symbols: stats.stale_symbols,
            embedding_dim: stats.dim,
        })
    }

    fn session(&self) -> Result<Arc<Session>, AppError> {
        self.session
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| AppError::Message("No repository is open.".to_string()))
    }
}

fn missing_symbol(id: i64) -> AppError {
    AppError::Message(format!(
        "Symbol {id} was not found. Re-index the repository and try again."
    ))
}

fn validate_repository_path(path: &str) -> Result<PathBuf, AppError> {
    let path = path.trim();
    if path.is_empty() {
        return Err(AppError::Message(
            "Repository path is empty. Enter the path to a directory.".to_string(),
        ));
    }
    let candidate = PathBuf::from(path);
    if !candidate.exists() {
        return Err(AppError::Message(format!(
            "Repository path does not exist: {}",
            candidate.display()
        )));
    }
    let root = candidate.canonicalize()?;
    if !root.is_dir() {
        return Err(AppError::Message(format!(
            "Repository path is not a directory: {}",
            root.display()
        )));
    }
    Ok(root)
}

#[tauri::command]
fn open_repository(state: State<'_, AtlasState>, path: String) -> Result<RepositoryInfo, String> {
    state.open(&path).map_err(|error| error.to_string())
}

#[tauri::command]
fn close_repository(state: State<'_, AtlasState>) {
    state.close();
}

#[tauri::command]
fn repository_info(state: State<'_, AtlasState>) -> Result<Option<RepositoryInfo>, String> {
    state.info().map_err(|error| error.to_string())
}

#[tauri::command]
async fn index_repository(state: State<'_, AtlasState>) -> Result<IndexOutcome, String> {
    let session = state.session().map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || session.index())
        .await
        .map_err(|error| format!("Indexing task failed: {error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn search_symbols(
    state: State<'_, AtlasState>,
    query: String,
    limit: usize,
) -> Result<Vec<SearchResult>, String> {
    state
        .search(&query, limit)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_symbols(state: State<'_, AtlasState>, limit: usize) -> Result<Vec<SymbolDetails>, String> {
    state.list_symbols(limit).map_err(|error| error.to_string())
}

#[tauri::command]
fn symbol_details(state: State<'_, AtlasState>, id: i64) -> Result<SymbolDetails, String> {
    state.symbol(id).map_err(|error| error.to_string())
}

#[tauri::command]
fn callers(state: State<'_, AtlasState>, id: i64) -> Result<Vec<RelatedSymbol>, String> {
    state.callers(id).map_err(|error| error.to_string())
}

#[tauri::command]
fn callees(state: State<'_, AtlasState>, id: i64) -> Result<Vec<RelatedSymbol>, String> {
    state.callees(id).map_err(|error| error.to_string())
}

#[tauri::command]
fn impact(state: State<'_, AtlasState>, id: i64, depth: u32) -> Result<Vec<ImpactEntry>, String> {
    state.impact(id, depth).map_err(|error| error.to_string())
}

#[tauri::command]
fn symbol_graph(
    state: State<'_, AtlasState>,
    id: i64,
    depth: u32,
    limit: usize,
) -> Result<GraphData, String> {
    state
        .graph(id, depth, limit)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn accuracy_report(state: State<'_, AtlasState>) -> Result<AccuracyReport, String> {
    state.report().map_err(|error| error.to_string())
}

#[tauri::command]
fn semantic_status(state: State<'_, AtlasState>) -> Result<SemanticStatus, String> {
    state.semantic_status().map_err(|error| error.to_string())
}

/// Download the semantic model. This is the only command that touches the
/// network; the UI must confirm with the user before invoking it.
#[tauri::command]
async fn download_model(app: tauri::AppHandle) -> Result<ModelStatus, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<ModelStatus, String> {
        let store = default_store().map_err(|error| error.to_string())?;
        let mut on_progress = |progress: DownloadProgress| {
            let _ = app.emit("semantic-download-progress", progress);
        };
        store
            .download(DEFAULT_MODEL, &mut on_progress)
            .map_err(|error| error.to_string())?;
        let report = store
            .verify(DEFAULT_MODEL)
            .map_err(|error| error.to_string())?;
        if !report.ok() {
            return Err(
                "The downloaded model failed checksum verification. Try again.".to_string(),
            );
        }
        store
            .status(DEFAULT_MODEL)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Model download task failed: {error}"))?
}

#[tauri::command]
async fn embed_repository(
    app: tauri::AppHandle,
    state: State<'_, AtlasState>,
) -> Result<EmbeddingStats, String> {
    let session = state.session().map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || -> Result<EmbeddingStats, String> {
        let candidates = session
            .index
            .embedding_candidates(DEFAULT_MODEL)
            .map_err(|error| error.to_string())?;
        let total = candidates.len();
        if total == 0 {
            return session
                .index
                .embedding_stats(DEFAULT_MODEL)
                .map_err(|error| error.to_string());
        }

        let embedder = load_cached(DEFAULT_MODEL).map_err(|error| error.to_string())?;
        let mut done = 0usize;
        let _ = app.emit("semantic-embed-progress", EmbedProgress { done, total });
        for batch in candidates.chunks(64) {
            let texts: Vec<String> = batch.iter().map(|item| item.text.clone()).collect();
            let vectors = embedder
                .encode(&texts, 64)
                .map_err(|error| error.to_string())?;
            let inputs: Vec<atlas_index::EmbeddingInput> = batch
                .iter()
                .zip(vectors)
                .map(|(candidate, vector)| atlas_index::EmbeddingInput {
                    symbol_id: candidate.symbol_id,
                    text: candidate.text.clone(),
                    vector,
                })
                .collect();
            done += session
                .index
                .upsert_embeddings(DEFAULT_MODEL, &inputs)
                .map_err(|error| error.to_string())?;
            let _ = app.emit("semantic-embed-progress", EmbedProgress { done, total });
        }
        session
            .index
            .embedding_stats(DEFAULT_MODEL)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Embedding task failed: {error}"))?
}

#[tauri::command]
async fn semantic_search(
    state: State<'_, AtlasState>,
    query: String,
    limit: usize,
) -> Result<Vec<SemanticResult>, String> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let session = state.session().map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<SemanticResult>, String> {
        let stats = session
            .index
            .embedding_stats(DEFAULT_MODEL)
            .map_err(|error| error.to_string())?;
        if stats.total_symbols == 0 {
            return Ok(Vec::new());
        }
        if stats.embedded_symbols == 0 {
            return Err(
                "No semantic embeddings for this repository. Build the semantic index first."
                    .to_string(),
            );
        }
        let embedder = load_cached(DEFAULT_MODEL).map_err(|error| error.to_string())?;
        let vector = embedder
            .encode_one(&query)
            .map_err(|error| error.to_string())?;
        session
            .index
            .semantic_search(DEFAULT_MODEL, &vector, limit.clamp(1, 200))
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Semantic search task failed: {error}"))?
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AtlasState::default())
        .invoke_handler(tauri::generate_handler![
            open_repository,
            close_repository,
            repository_info,
            index_repository,
            search_symbols,
            list_symbols,
            symbol_details,
            callers,
            callees,
            impact,
            symbol_graph,
            accuracy_report,
            semantic_status,
            download_model,
            embed_repository,
            semantic_search,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Atlas");
}
