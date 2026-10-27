use atlas_embed::{
    default_store, model_download_bytes, DownloadProgress, EmbedError, ModelStore, DEFAULT_MODEL,
    MODEL_LICENSE, MODEL_REVISION,
};
use atlas_index::{EmbeddingInput, Index};
use atlas_scan::scan_directory;
use clap::{Parser, Subcommand};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;

#[derive(Parser)]
#[command(name = "atlas")]
#[command(about = "Atlas code indexer", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Index a repository
    Index {
        #[arg(value_name = "PATH")]
        path: PathBuf,
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Show index statistics
    Stats {
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Show how call edges were resolved and the most common unresolved names
    Report {
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Show callers of a symbol
    Callers {
        #[arg(value_name = "NAME")]
        name: String,
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Show callees of a symbol
    Callees {
        #[arg(value_name = "NAME")]
        name: String,
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Show impact of changing a symbol
    Impact {
        #[arg(value_name = "NAME")]
        name: String,
        #[arg(long, default_value = "5")]
        depth: u32,
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Search symbol names, signatures and documentation
    Search {
        #[arg(value_name = "QUERY")]
        query: String,
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "20")]
        limit: usize,
        /// Rank results by semantic similarity instead of FTS5 tokens
        #[arg(long)]
        semantic: bool,
        /// Semantic model to use (only with --semantic)
        #[arg(long, default_value_t = DEFAULT_MODEL.to_string())]
        model: String,
    },
    /// Compute semantic embeddings for indexed symbols
    Embed {
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long, default_value_t = DEFAULT_MODEL.to_string())]
        model: String,
        #[arg(long, default_value = "64")]
        batch_size: usize,
    },
    /// Manage the local semantic-search model
    Model {
        #[command(subcommand)]
        command: ModelCommands,
    },
}

#[derive(Subcommand)]
enum ModelCommands {
    /// Download the semantic model (explicit, one-time network access)
    Download {
        #[arg(long, default_value_t = DEFAULT_MODEL.to_string())]
        model: String,
        /// Skip the interactive confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Show whether the semantic model is installed and verified
    Status {
        #[arg(long, default_value_t = DEFAULT_MODEL.to_string())]
        model: String,
    },
}

fn get_db_path(repo_path: &Path, db: Option<PathBuf>) -> PathBuf {
    match db {
        Some(p) => p,
        None => {
            let mut p = repo_path.to_path_buf();
            if !p.is_dir() && p.exists() {
                p = p.parent().unwrap_or(repo_path).to_path_buf();
            }
            let atlas_dir = p.join(".atlas");
            let _ = fs::create_dir_all(&atlas_dir);
            atlas_dir.join("atlas.db")
        }
    }
}

fn main() {
    // Rust ignores SIGPIPE by default, so writing to a closed pipe turns
    // `println!` into an EPIPE panic (`atlas report | head`). Restore the
    // default disposition so Atlas dies silently, like other Unix filters.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let cli = Cli::parse();
    match cli.command {
        Commands::Index { path, db } => {
            let db_path = get_db_path(&path, db);
            let index = match Index::open(&db_path) {
                Ok(idx) => idx,
                Err(e) => {
                    eprintln!("Error opening database: {}", e);
                    process::exit(1);
                }
            };
            match scan_directory(&path, &index) {
                Ok(summary) => {
                    println!("Indexed repository at {}", path.display());
                    println!("Files scanned: {}", summary.files_scanned);
                    println!("Indexed: {}", summary.indexed);
                    println!("Skipped unchanged: {}", summary.skipped_unchanged);
                    println!("Skipped too large: {}", summary.skipped_too_large);
                    println!("Skipped not UTF-8: {}", summary.skipped_not_utf8);
                    println!("Removed: {}", summary.removed);
                    println!("Symbols: {}", summary.symbols);
                    println!(
                        "Edges: {} ({} resolved, {} external, {} unresolved)",
                        summary.edges,
                        summary.resolved_edges,
                        summary.external_edges,
                        summary.unresolved_edges
                    );
                    if summary.unresolved_edges > 0 {
                        println!(
                            "{} call sites could not be resolved to a definition",
                            summary.unresolved_edges
                        );
                    }
                    println!("Elapsed: {}ms", summary.elapsed_time_ms);
                    if !summary.warnings.is_empty() {
                        for w in summary.warnings {
                            eprintln!("Warning: {}: {}", w.path, w.message);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error scanning: {}", e);
                    process::exit(1);
                }
            }
        }
        Commands::Stats { db } => {
            let db_path = db.unwrap_or_else(|| PathBuf::from(".atlas/atlas.db"));
            let index = match Index::open(&db_path) {
                Ok(idx) => idx,
                Err(e) => {
                    eprintln!("Error opening database: {}", e);
                    process::exit(1);
                }
            };
            match index.stats() {
                Ok(stats) => {
                    println!("Symbols: {}", stats.symbol_count);
                    println!("Edges: {}", stats.edge_count);
                    println!("Resolved edges: {}", stats.resolved_edge_count);
                    println!("External edges: {}", stats.external_edge_count);
                    println!("Unresolved edges: {}", stats.unresolved_edge_count);
                    println!("Files: {}", stats.file_count);
                    if stats.unresolved_edge_count > 0 {
                        println!(
                            "{} call sites could not be resolved to a definition",
                            stats.unresolved_edge_count
                        );
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(1);
                }
            }
        }
        Commands::Report { db, json } => {
            let db_path = db.unwrap_or_else(|| PathBuf::from(".atlas/atlas.db"));
            let index = match Index::open(&db_path) {
                Ok(idx) => idx,
                Err(e) => {
                    eprintln!("Error opening database: {}", e);
                    process::exit(1);
                }
            };
            match index.accuracy_report() {
                Ok(report) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&report).unwrap());
                        return;
                    }
                    let percent = |count: usize| {
                        if report.edges == 0 {
                            0.0
                        } else {
                            100.0 * count as f64 / report.edges as f64
                        }
                    };
                    println!("Atlas accuracy report");
                    println!("Files: {}", report.files);
                    println!("Symbols: {}", report.symbols);
                    println!("Edges: {}", report.edges);
                    println!(
                        "Resolved: {} ({:.1}%)",
                        report.resolved,
                        percent(report.resolved)
                    );
                    for count in &report.resolution_counts {
                        if matches!(
                            count.resolution.as_str(),
                            "import" | "same_file" | "unique_name"
                        ) {
                            println!("  {}: {}", count.resolution, count.count);
                        }
                    }
                    println!(
                        "External: {} ({:.1}%)",
                        report.external,
                        percent(report.external)
                    );
                    println!(
                        "Unresolved: {} ({:.1}%)",
                        report.unresolved,
                        percent(report.unresolved)
                    );
                    if !report.top_unresolved.is_empty() {
                        println!("Top unresolved names:");
                        for (i, name) in report.top_unresolved.iter().enumerate() {
                            println!("  {}. {} ({})", i + 1, name.name, name.count);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(1);
                }
            }
        }
        Commands::Callers { name, db, json } => {
            let db_path = db.unwrap_or_else(|| PathBuf::from(".atlas/atlas.db"));
            let index = match Index::open(&db_path) {
                Ok(idx) => idx,
                Err(e) => {
                    eprintln!("Error opening database: {}", e);
                    process::exit(1);
                }
            };
            match index.callers_of(&name) {
                Ok(callers) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&callers).unwrap());
                        return;
                    }
                    if callers.is_empty() {
                        println!("No callers found for '{}'", name);
                        process::exit(0);
                    }
                    for c in callers {
                        println!(
                            "{} {}:{}",
                            c.caller_qualified_name, c.caller_file, c.caller_line
                        );
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(1);
                }
            }
        }
        Commands::Callees { name, db, json } => {
            let db_path = db.unwrap_or_else(|| PathBuf::from(".atlas/atlas.db"));
            let index = match Index::open(&db_path) {
                Ok(idx) => idx,
                Err(e) => {
                    eprintln!("Error opening database: {}", e);
                    process::exit(1);
                }
            };
            match index.callees_of(&name) {
                Ok(callees) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&callees).unwrap());
                        return;
                    }
                    if callees.is_empty() {
                        println!("No callees found for '{}'", name);
                        process::exit(0);
                    }
                    for c in callees {
                        println!(
                            "{} {}:{}",
                            c.callee_qualified_name, c.caller_file, c.caller_line
                        );
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(1);
                }
            }
        }
        Commands::Impact {
            name,
            depth,
            db,
            json,
        } => {
            let db_path = db.unwrap_or_else(|| PathBuf::from(".atlas/atlas.db"));
            let index = match Index::open(&db_path) {
                Ok(idx) => idx,
                Err(e) => {
                    eprintln!("Error opening database: {}", e);
                    process::exit(1);
                }
            };
            match index.impact_of(&name, depth) {
                Ok(impact) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&impact).unwrap());
                        return;
                    }
                    if impact.is_empty() {
                        println!(
                            "No impact found for '{}' (or no callers transitively)",
                            name
                        );
                        process::exit(0);
                    }
                    for i in impact {
                        println!(
                            "depth={} {} {}:{}",
                            i.depth, i.symbol_qualified_name, i.file, i.line
                        );
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(1);
                }
            }
        }
        Commands::Search {
            query,
            db,
            json,
            limit,
            semantic,
            model,
        } => {
            let db_path = db.unwrap_or_else(|| PathBuf::from(".atlas/atlas.db"));
            let index = match Index::open(&db_path) {
                Ok(idx) => idx,
                Err(e) => {
                    eprintln!("Error opening database: {}", e);
                    process::exit(1);
                }
            };
            if semantic {
                run_semantic_search(&index, &model, &query, limit, json);
                return;
            }
            match index.search(&query, limit) {
                Ok(results) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&results).unwrap());
                        return;
                    }
                    if results.is_empty() {
                        println!("No matches found for '{}'", query);
                        process::exit(0);
                    }
                    for result in results {
                        println!(
                            "{} {}:{} [{}]",
                            result.qualified_name, result.file, result.line, result.kind
                        );
                        if let Some(signature) = &result.signature {
                            println!("  {}", signature);
                        }
                        if let Some(doc) = &result.doc {
                            if let Some(first) = doc.lines().next() {
                                println!("  {}", first);
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(1);
                }
            }
        }
        Commands::Embed {
            db,
            model,
            batch_size,
        } => {
            let db_path = db.unwrap_or_else(|| PathBuf::from(".atlas/atlas.db"));
            let index = match Index::open(&db_path) {
                Ok(idx) => idx,
                Err(e) => {
                    eprintln!("Error opening database: {}", e);
                    process::exit(1);
                }
            };
            run_embed(&index, &model, batch_size);
        }
        Commands::Model { command } => match command {
            ModelCommands::Download { model, yes } => download_model(&model, yes),
            ModelCommands::Status { model } => model_status(&model),
        },
    }
}

fn model_store() -> &'static ModelStore {
    match default_store() {
        Ok(store) => store,
        Err(error) => {
            eprintln!("Error: {}", error);
            process::exit(1);
        }
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

fn confirm(prompt: &str) -> bool {
    print!("{} [y/N] ", prompt);
    let _ = io::stdout().flush();
    let mut answer = String::new();
    match io::stdin().read_line(&mut answer) {
        Ok(_) => matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes"),
        Err(_) => false,
    }
}

fn download_model(model: &str, assume_yes: bool) {
    let store = model_store();
    let status = match store.status(model) {
        Ok(status) => status,
        Err(error) => {
            eprintln!("Error: {}", error);
            process::exit(1);
        }
    };
    match store.verify(model) {
        Ok(report) if report.ok() => {
            println!("Semantic model '{}' is already installed.", model);
            println!("Path: {}", status.path);
            println!("Size: {}", format_bytes(model_download_bytes()));
            println!("Checksums: verified");
            return;
        }
        Ok(_) => {}
        Err(error) => {
            eprintln!("Error: {}", error);
            process::exit(1);
        }
    }

    println!("Semantic model: {}", model);
    println!("License: {}", MODEL_LICENSE);
    println!("Revision: {}", MODEL_REVISION);
    println!(
        "Source: https://huggingface.co/{}/resolve/{}",
        model, MODEL_REVISION
    );
    println!(
        "Download size: {} ({} bytes)",
        format_bytes(model_download_bytes()),
        model_download_bytes()
    );
    println!("Cache path: {}", status.path);
    println!("This is the only network operation Atlas performs. It runs once and");
    println!("the model then works offline.");
    if !assume_yes && !confirm("Download the model now?") {
        println!("Download cancelled.");
        return;
    }

    let mut last_file = String::new();
    let mut outcome = None;
    let mut error: Option<EmbedError> = None;
    {
        let mut on_progress = |progress: DownloadProgress| {
            if progress.file != last_file {
                if !last_file.is_empty() {
                    println!();
                }
                last_file = progress.file.clone();
            }
            let percent = if progress.total == 0 {
                0.0
            } else {
                100.0 * progress.downloaded as f64 / progress.total as f64
            };
            print!(
                "\r  [{}/{}] {}: {} / {} ({:.1}%)",
                progress.file_index,
                progress.file_count,
                progress.file,
                format_bytes(progress.downloaded),
                format_bytes(progress.total),
                percent
            );
            let _ = io::stdout().flush();
        };
        match store.download(model, &mut on_progress) {
            Ok(result) => outcome = Some(result),
            Err(failure) => error = Some(failure),
        }
    }
    if !last_file.is_empty() {
        println!();
    }
    if let Some(error) = error {
        eprintln!("Error: {}", error);
        process::exit(1);
    }
    if let Some(outcome) = outcome {
        println!(
            "Model ready: {} downloaded, {} already present.",
            outcome.downloaded, outcome.skipped
        );
        println!("Path: {}", outcome.path.display());
    }
}

fn model_status(model: &str) {
    let store = model_store();
    let status = match store.status(model) {
        Ok(status) => status,
        Err(error) => {
            eprintln!("Error: {}", error);
            process::exit(1);
        }
    };
    println!("Model: {}", status.model);
    println!("Path: {}", status.path);
    println!("Installed: {}", if status.installed { "yes" } else { "no" });
    println!("Size on disk: {}", format_bytes(status.size_bytes));
    println!("Expected size: {}", format_bytes(model_download_bytes()));
    println!("Dimensions: {}", status.dim);
    println!("License: {}", MODEL_LICENSE);
    match store.verify(model) {
        Ok(report) if report.ok() => println!("Checksums: verified"),
        Ok(report) => {
            println!("Checksums: NOT verified");
            if !report.missing.is_empty() {
                println!("  Missing: {}", report.missing.join(", "));
            }
            if !report.mismatched.is_empty() {
                println!("  Mismatched: {}", report.mismatched.join(", "));
            }
        }
        Err(error) => {
            eprintln!("Error: {}", error);
            process::exit(1);
        }
    }
}

fn run_embed(index: &Index, model: &str, batch_size: usize) {
    let store = model_store();
    let embedder = match store.load(model) {
        Ok(embedder) => embedder,
        Err(error) => {
            eprintln!("Error: {}", error);
            process::exit(1);
        }
    };
    let candidates = match index.embedding_candidates(model) {
        Ok(candidates) => candidates,
        Err(error) => {
            eprintln!("Error: {}", error);
            process::exit(1);
        }
    };
    if candidates.is_empty() {
        match index.embedding_stats(model) {
            Ok(stats) => {
                println!(
                    "Embeddings are up to date: {} of {} symbols embedded with {}.",
                    stats.current_symbols, stats.total_symbols, stats.model
                );
            }
            Err(error) => {
                eprintln!("Error: {}", error);
                process::exit(1);
            }
        }
        return;
    }

    println!(
        "Embedding {} of {} symbols with {} ({} dimensions)...",
        candidates.len(),
        index
            .embedding_stats(model)
            .map(|stats| stats.total_symbols)
            .unwrap_or(candidates.len()),
        model,
        embedder.dim()
    );
    let batch_size = batch_size.clamp(1, 512);
    let mut done = 0usize;
    for batch in candidates.chunks(batch_size) {
        let texts: Vec<String> = batch.iter().map(|item| item.text.clone()).collect();
        let vectors = match embedder.encode(&texts, batch_size) {
            Ok(vectors) => vectors,
            Err(error) => {
                eprintln!("Error: {}", error);
                process::exit(1);
            }
        };
        let inputs: Vec<EmbeddingInput> = batch
            .iter()
            .zip(vectors)
            .map(|(candidate, vector)| EmbeddingInput {
                symbol_id: candidate.symbol_id,
                text: candidate.text.clone(),
                vector,
            })
            .collect();
        match index.upsert_embeddings(model, &inputs) {
            Ok(stored) => done += stored,
            Err(error) => {
                eprintln!("Error storing embeddings: {}", error);
                process::exit(1);
            }
        }
        println!("  {}/{}", done, candidates.len());
    }
    match index.embedding_stats(model) {
        Ok(stats) => {
            println!(
                "Done: {} current embeddings for {} ({} stale).",
                stats.current_symbols, stats.model, stats.stale_symbols
            );
            if stats.stale_symbols > 0 {
                println!(
                    "{} symbols still need embeddings; run `atlas embed` again.",
                    stats.stale_symbols
                );
            }
        }
        Err(error) => {
            eprintln!("Error: {}", error);
            process::exit(1);
        }
    }
}

fn run_semantic_search(index: &Index, model: &str, query: &str, limit: usize, json: bool) {
    let stats = match index.embedding_stats(model) {
        Ok(stats) => stats,
        Err(error) => {
            eprintln!("Error: {}", error);
            process::exit(1);
        }
    };
    if stats.total_symbols == 0 {
        println!("No symbols indexed yet. Run `atlas index <path>` first.");
        return;
    }
    if stats.embedded_symbols == 0 {
        eprintln!(
            "No embeddings found for '{}'. Run `atlas embed` first.",
            model
        );
        process::exit(1);
    }
    if stats.stale_symbols > 0 {
        eprintln!(
            "Warning: {} symbols have stale or missing embeddings; run `atlas embed` to refresh them.",
            stats.stale_symbols
        );
    }

    let store = model_store();
    let embedder = match store.load(model) {
        Ok(embedder) => embedder,
        Err(error) => {
            eprintln!("Error: {}", error);
            process::exit(1);
        }
    };
    let query_vector = match embedder.encode_one(query) {
        Ok(vector) => vector,
        Err(error) => {
            eprintln!("Error encoding query: {}", error);
            process::exit(1);
        }
    };
    let results = match index.semantic_search(model, &query_vector, limit) {
        Ok(results) => results,
        Err(error) => {
            eprintln!("Error: {}", error);
            process::exit(1);
        }
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&results).unwrap());
        return;
    }
    if results.is_empty() {
        println!("No semantic matches found for '{}'", query);
        return;
    }
    for result in results {
        println!(
            "{:.4} {} {}:{} [{}]",
            result.similarity, result.qualified_name, result.file, result.line, result.kind
        );
        if let Some(signature) = &result.signature {
            println!("  {}", signature);
        }
        if let Some(doc) = &result.doc {
            if let Some(first) = doc.lines().next() {
                println!("  {}", first);
            }
        }
    }
}
