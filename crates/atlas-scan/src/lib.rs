use atlas_index::Index;
use atlas_parser::language::get_language_for_path;
use atlas_parser::python::PythonExtractor;
use atlas_parser::typescript::TypeScriptExtractor;
use atlas_parser::RustExtractor;
use rayon::prelude::*;
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone, Serialize)]
pub struct ScanWarning {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanSummary {
    pub files_scanned: usize,
    pub indexed: usize,
    pub skipped_unchanged: usize,
    pub skipped_too_large: usize,
    pub skipped_not_utf8: usize,
    pub removed: usize,
    pub symbols: usize,
    pub edges: usize,
    pub resolved_edges: usize,
    pub external_edges: usize,
    pub unresolved_edges: usize,
    pub elapsed_time_ms: u128,
    pub warnings: Vec<ScanWarning>,
}

fn is_supported_file(path: &Path) -> bool {
    get_language_for_path(path).is_some()
}

fn detect_language(path: &Path) -> Option<&'static str> {
    get_language_for_path(path).map(|language| language.name)
}

fn compute_hash(content: &[u8]) -> String {
    blake3::hash(content).to_hex().to_string()
}

/// Store paths relative to the scan root with forward slashes. Module and
/// import resolution depend on stable, root-relative paths.
fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn scan_directory(
    root: &Path,
    index: &Index,
) -> Result<ScanSummary, Box<dyn std::error::Error>> {
    let start = Instant::now();
    let mut warnings = Vec::new();

    // Walk with ignore crate to respect .gitignore
    let mut walker = ignore::WalkBuilder::new(root);
    walker.git_ignore(true);
    walker.git_global(true);
    walker.git_exclude(true);
    let walker = walker.build();

    let mut files_to_process: Vec<(PathBuf, String)> = Vec::new();
    let mut seen_paths: HashSet<String> = HashSet::new();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.file_type().is_some_and(|ft| ft.is_file()) {
            let path = entry.into_path();
            if is_supported_file(&path) {
                let relative = relative_path(root, &path);
                if seen_paths.insert(relative.clone()) {
                    files_to_process.push((path, relative));
                }
            }
        }
    }

    let mut skipped_too_large = 0;
    let mut skipped_not_utf8 = 0;
    let mut skipped_unchanged = 0;
    let mut indexed = 0;
    let mut removed = 0;

    // Read and parse in parallel. Each worker gets one extractor per language.
    let results: Vec<_> = files_to_process
        .par_iter()
        .map_init(
            || {
                (
                    RustExtractor::new(),
                    PythonExtractor::new(),
                    TypeScriptExtractor::new(),
                )
            },
            |extractors, (path, relative)| {
                let metadata = match fs::metadata(path) {
                    Ok(m) => m,
                    Err(e) => {
                        return Err((relative.clone(), format!("metadata error: {}", e)));
                    }
                };
                // Skip files larger than 2 MiB
                if metadata.len() > 2 * 1024 * 1024 {
                    return Err((relative.clone(), "too_large".to_string()));
                }
                let content_bytes = match fs::read(path) {
                    Ok(b) => b,
                    Err(e) => {
                        return Err((relative.clone(), format!("read error: {}", e)));
                    }
                };
                let content = match String::from_utf8(content_bytes.clone()) {
                    Ok(s) => s,
                    Err(_) => {
                        return Err((relative.clone(), "not_utf8".to_string()));
                    }
                };
                let hash = compute_hash(&content_bytes);
                let language = match detect_language(path) {
                    Some(language) => language,
                    None => return Err((relative.clone(), "unsupported".to_string())),
                };
                let analysis = match language {
                    "rust" => extractors.0.extract(relative, &content),
                    "python" => extractors.1.extract(relative, &content),
                    "typescript" => extractors.2.extract(relative, &content),
                    _ => return Err((relative.clone(), "unsupported".to_string())),
                };
                Ok((relative.clone(), language, hash, analysis))
            },
        )
        .collect();

    for result in results {
        match result {
            Err((relative, message)) => {
                if message == "too_large" {
                    skipped_too_large += 1;
                    warnings.push(ScanWarning {
                        path: relative.clone(),
                        message: "File larger than 2 MiB, skipped".to_string(),
                    });
                } else if message == "not_utf8" {
                    skipped_not_utf8 += 1;
                    warnings.push(ScanWarning {
                        path: relative.clone(),
                        message: "File is not valid UTF-8, skipped".to_string(),
                    });
                } else {
                    warnings.push(ScanWarning {
                        path: relative.clone(),
                        message,
                    });
                }
                // If this file used to be indexed, its old data is stale.
                if index.file_hash(&relative)?.is_some() {
                    index.remove_file(&relative)?;
                    removed += 1;
                }
            }
            Ok((relative, language, hash, analysis)) => {
                if let Some(stored_hash) = index.file_hash(&relative)? {
                    if stored_hash == hash {
                        skipped_unchanged += 1;
                        continue;
                    }
                }
                if let Err(e) = index.replace_file(&relative, language, &hash, &analysis) {
                    warnings.push(ScanWarning {
                        path: relative,
                        message: format!("index error: {}", e),
                    });
                    continue;
                }
                indexed += 1;
            }
        }
    }

    // Remove index entries for files that no longer exist on disk (or are no
    // longer supported).
    for path in index.all_file_paths()? {
        if !seen_paths.contains(&path) {
            index.remove_file(&path)?;
            removed += 1;
        }
    }

    // Resolve once after all files are written.
    let resolve_stats = index.resolve_edges()?;
    let stats = index.stats()?;

    Ok(ScanSummary {
        files_scanned: files_to_process.len(),
        indexed,
        skipped_unchanged,
        skipped_too_large,
        skipped_not_utf8,
        removed,
        symbols: stats.symbol_count,
        edges: stats.edge_count,
        resolved_edges: resolve_stats.linked,
        external_edges: resolve_stats.external,
        unresolved_edges: resolve_stats.unresolved,
        elapsed_time_ms: start.elapsed().as_millis(),
        warnings,
    })
}
