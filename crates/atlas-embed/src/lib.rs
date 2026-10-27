use model2vec_rs::model::StaticModel;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{BufWriter, Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The only semantic model Atlas supports in this beta. The revision and file
/// hashes are pinned so a download is reproducible and cannot silently change.
pub const DEFAULT_MODEL: &str = "minishlab/potion-code-16M-v2";
pub const DEFAULT_MODEL_DIM: usize = 256;
pub const MODEL_REVISION: &str = "e9d2a44ca6a05ac6685f3b23709ea57eb7352d5b";
pub const MODEL_LICENSE: &str = "MIT";

pub struct ModelFile {
    pub name: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

pub const MODEL_FILES: [ModelFile; 3] = [
    ModelFile {
        name: "config.json",
        bytes: 59,
        sha256: "148e5691a6fcc553437156859701fba017a1ba5d340b170f17e0f3668fb861a7",
    },
    ModelFile {
        name: "tokenizer.json",
        bytes: 1_024_340,
        sha256: "107bbdcbad4bff1d299b7a4c3a2fb17c52890688b7dd0e4c9deab79d3c4f3d45",
    },
    ModelFile {
        name: "model.safetensors",
        bytes: 32_490_072,
        sha256: "75cf7a6c2171b230ad19b1e7d8e0b1aee86da5a02af8e7cacedd9921d227623c",
    },
];

pub fn model_download_bytes() -> u64 {
    MODEL_FILES.iter().map(|file| file.bytes).sum()
}

#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error(
        "Unsupported semantic model '{0}'. Atlas currently supports \
         minishlab/potion-code-16M-v2."
    )]
    UnsupportedModel(String),
    #[error(
        "Semantic model '{model}' is not installed at {path}. \
         Run `atlas model download` first."
    )]
    NotInstalled { model: String, path: String },
    #[error("Could not load semantic model: {0}")]
    Model(String),
    #[error("Download failed: {0}")]
    Download(String),
    #[error("Checksum mismatch for {file}: expected {expected}, got {actual}")]
    HashMismatch {
        file: String,
        expected: String,
        actual: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelStatus {
    pub model: String,
    pub path: String,
    pub installed: bool,
    pub size_bytes: u64,
    pub dim: usize,
}

#[derive(Debug, Clone, Default)]
pub struct VerifyReport {
    pub missing: Vec<String>,
    pub mismatched: Vec<String>,
}

impl VerifyReport {
    pub fn ok(&self) -> bool {
        self.missing.is_empty() && self.mismatched.is_empty()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    pub file: String,
    pub downloaded: u64,
    pub total: u64,
    pub file_index: usize,
    pub file_count: usize,
}

#[derive(Debug, Clone)]
pub struct DownloadOutcome {
    pub downloaded: usize,
    pub skipped: usize,
    pub path: PathBuf,
}

pub struct ModelStore {
    root: PathBuf,
}

impl ModelStore {
    /// Platform cache directory, unless `ATLAS_MODEL_DIR` overrides it.
    pub fn platform() -> Result<Self, EmbedError> {
        if let Some(root) = std::env::var_os("ATLAS_MODEL_DIR") {
            return Ok(Self::new(PathBuf::from(root)));
        }
        let cache = dirs::cache_dir().ok_or_else(|| {
            EmbedError::Model(
                "could not determine a cache directory; set ATLAS_MODEL_DIR".to_string(),
            )
        })?;
        Ok(Self::new(cache.join("atlas").join("models")))
    }

    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn model_dir(&self, model: &str) -> Result<PathBuf, EmbedError> {
        validate_model(model)?;
        Ok(self.root.join(model))
    }

    pub fn status(&self, model: &str) -> Result<ModelStatus, EmbedError> {
        let dir = self.model_dir(model)?;
        let mut size_bytes = 0;
        let mut installed = true;
        for file in MODEL_FILES {
            match fs::metadata(dir.join(file.name)) {
                Ok(metadata) if metadata.is_file() => {
                    size_bytes += metadata.len();
                    if metadata.len() != file.bytes {
                        installed = false;
                    }
                }
                _ => installed = false,
            }
        }
        Ok(ModelStatus {
            model: model.to_string(),
            path: dir.display().to_string(),
            installed,
            size_bytes,
            dim: DEFAULT_MODEL_DIM,
        })
    }

    pub fn verify(&self, model: &str) -> Result<VerifyReport, EmbedError> {
        let dir = self.model_dir(model)?;
        let mut report = VerifyReport::default();
        for file in MODEL_FILES {
            let path = dir.join(file.name);
            if !path.is_file() {
                report.missing.push(file.name.to_string());
                continue;
            }
            let actual = sha256_file(&path)?;
            if actual != file.sha256 {
                report.mismatched.push(file.name.to_string());
            }
        }
        Ok(report)
    }

    /// Download missing or corrupt model files. This is the only function in
    /// Atlas that performs a network request; callers must ask the user first.
    pub fn download(
        &self,
        model: &str,
        progress: &mut dyn FnMut(DownloadProgress),
    ) -> Result<DownloadOutcome, EmbedError> {
        let dir = self.model_dir(model)?;
        fs::create_dir_all(&dir)?;

        if self.verify(model)?.ok() {
            return Ok(DownloadOutcome {
                downloaded: 0,
                skipped: MODEL_FILES.len(),
                path: dir,
            });
        }

        let agent = ureq::AgentBuilder::new()
            .user_agent(concat!("atlas/", env!("CARGO_PKG_VERSION")))
            .build();
        let mut downloaded = 0;
        let mut skipped = 0;

        for (index, file) in MODEL_FILES.iter().enumerate() {
            let path = dir.join(file.name);
            let part = dir.join(format!("{}.part", file.name));
            if path.is_file() && sha256_file(&path)? == file.sha256 {
                skipped += 1;
                continue;
            }

            let _ = fs::remove_file(&part);
            let url = format!(
                "https://huggingface.co/{model}/resolve/{MODEL_REVISION}/{}",
                file.name
            );
            let response = agent
                .get(&url)
                .call()
                .map_err(|error| EmbedError::Download(format!("{url}: {error}")))?;
            let total = response
                .header("Content-Length")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(file.bytes);

            progress(DownloadProgress {
                file: file.name.to_string(),
                downloaded: 0,
                total,
                file_index: index + 1,
                file_count: MODEL_FILES.len(),
            });

            let mut reader = response.into_reader();
            let mut writer = BufWriter::new(fs::File::create(&part)?);
            let mut hasher = Sha256::new();
            let mut received = 0u64;
            let mut last_reported = 0u64;
            let mut buffer = [0u8; 64 * 1024];

            loop {
                let read = reader.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                writer.write_all(&buffer[..read])?;
                hasher.update(&buffer[..read]);
                received += read as u64;
                // Throttle IPC/progress callbacks on large files.
                if received == total || received - last_reported >= 256 * 1024 {
                    progress(DownloadProgress {
                        file: file.name.to_string(),
                        downloaded: received,
                        total,
                        file_index: index + 1,
                        file_count: MODEL_FILES.len(),
                    });
                    last_reported = received;
                }
            }
            writer.flush()?;
            drop(writer);

            let actual = hex(hasher.finalize().as_slice());
            if actual != file.sha256 {
                let _ = fs::remove_file(&part);
                return Err(EmbedError::HashMismatch {
                    file: file.name.to_string(),
                    expected: file.sha256.to_string(),
                    actual,
                });
            }
            if path.exists() {
                fs::remove_file(&path)?;
            }
            fs::rename(&part, &path)?;
            downloaded += 1;
        }

        Ok(DownloadOutcome {
            downloaded,
            skipped,
            path: dir,
        })
    }

    pub fn load(&self, model: &str) -> Result<Embedder, EmbedError> {
        let status = self.status(model)?;
        if !status.installed {
            return Err(EmbedError::NotInstalled {
                model: model.to_string(),
                path: status.path,
            });
        }
        if !self.verify(model)?.ok() {
            return Err(EmbedError::Model(format!(
                "model files in {} are corrupt; run `atlas model download` again",
                status.path
            )));
        }
        let dir = self.model_dir(model)?;
        let inner = StaticModel::from_pretrained(&dir, None, None, None).map_err(|error| {
            EmbedError::Model(format!("failed to load {}: {error}", dir.display()))
        })?;
        let embedder = Embedder {
            model: model.to_string(),
            inner,
        };
        let probe = embedder.encode_one("atlas")?;
        if probe.len() != DEFAULT_MODEL_DIM {
            return Err(EmbedError::Model(format!(
                "model dimension {} does not match the expected {DEFAULT_MODEL_DIM}",
                probe.len()
            )));
        }
        Ok(embedder)
    }
}

#[derive(Debug)]
pub struct Embedder {
    model: String,
    inner: StaticModel,
}

impl Embedder {
    pub fn model_id(&self) -> &str {
        &self.model
    }

    pub fn dim(&self) -> usize {
        DEFAULT_MODEL_DIM
    }

    /// Encode and L2-normalize vectors. The model config already normalizes,
    /// but normalizing here keeps the stored vectors independent of that
    /// implementation detail.
    pub fn encode(&self, texts: &[String], batch_size: usize) -> Result<Vec<Vec<f32>>, EmbedError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let batch_size = batch_size.clamp(1, 4096);
        let encoded = catch_unwind(AssertUnwindSafe(|| {
            self.inner.encode_with_args(texts, Some(512), batch_size)
        }))
        .map_err(|_| {
            EmbedError::Model("the embedding model panicked while tokenizing input".to_string())
        })?;
        if encoded.len() != texts.len() {
            return Err(EmbedError::Model(format!(
                "model returned {} vectors for {} inputs",
                encoded.len(),
                texts.len()
            )));
        }

        let mut vectors = Vec::with_capacity(encoded.len());
        for mut vector in encoded {
            if vector.len() != DEFAULT_MODEL_DIM {
                return Err(EmbedError::Model(format!(
                    "model returned dimension {} instead of {DEFAULT_MODEL_DIM}",
                    vector.len()
                )));
            }
            if !vector.iter().all(|value| value.is_finite()) {
                return Err(EmbedError::Model(
                    "model returned a non-finite embedding".to_string(),
                ));
            }
            let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
            if !norm.is_finite() || norm < 1e-9 {
                return Err(EmbedError::Model(
                    "model returned an empty embedding".to_string(),
                ));
            }
            for value in &mut vector {
                *value /= norm;
            }
            vectors.push(vector);
        }
        Ok(vectors)
    }

    pub fn encode_one(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let mut vectors = self.encode(&[text.to_string()], 1)?;
        vectors
            .pop()
            .ok_or_else(|| EmbedError::Model("model returned no embedding".to_string()))
    }
}

fn validate_model(model: &str) -> Result<(), EmbedError> {
    if model == DEFAULT_MODEL {
        Ok(())
    } else {
        Err(EmbedError::UnsupportedModel(model.to_string()))
    }
}

fn sha256_file(path: &Path) -> Result<String, EmbedError> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(hasher.finalize().as_slice()))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

/// Load the model once per process and reuse it. Semantic search runs on every
/// query in the desktop app; reloading 32 MB of weights for each keystroke
/// would make the feature unusable.
pub fn load_cached(model: &str) -> Result<&'static Embedder, EmbedError> {
    static CACHED: OnceLock<Embedder> = OnceLock::new();
    if let Some(embedder) = CACHED.get() {
        if embedder.model_id() == model {
            return Ok(embedder);
        }
        return Err(EmbedError::UnsupportedModel(model.to_string()));
    }
    let embedder = default_store()?.load(model)?;
    let _ = CACHED.set(embedder);
    CACHED
        .get()
        .ok_or_else(|| EmbedError::Model("could not cache the embedding model".to_string()))
}

/// The cache is process-global in practice, so avoid recomputing it for every
/// status check in the desktop app. Tests construct their own store instead.
pub fn default_store() -> Result<&'static ModelStore, EmbedError> {
    static STORE: OnceLock<ModelStore> = OnceLock::new();
    if let Some(store) = STORE.get() {
        return Ok(store);
    }
    let store = ModelStore::platform()?;
    let _ = STORE.set(store);
    STORE
        .get()
        .ok_or_else(|| EmbedError::Model("could not initialize the model cache".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store() -> (TempDir, ModelStore) {
        let dir = TempDir::new().unwrap();
        let store = ModelStore::new(dir.path().to_path_buf());
        (dir, store)
    }

    #[test]
    fn unsupported_model_is_rejected() {
        let (_dir, store) = store();
        let error = store.status("../escape").unwrap_err();
        assert!(matches!(error, EmbedError::UnsupportedModel(_)));
    }

    #[test]
    fn missing_model_reports_install_command() {
        let (_dir, store) = store();
        let error = store.load(DEFAULT_MODEL).unwrap_err();
        assert!(error.to_string().contains("atlas model download"));
    }

    #[test]
    fn status_requires_all_files() {
        let (_dir, store) = store();
        let status = store.status(DEFAULT_MODEL).unwrap();
        assert!(!status.installed);
        assert_eq!(status.dim, DEFAULT_MODEL_DIM);
        let dir = store.model_dir(DEFAULT_MODEL).unwrap();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("config.json"), b"{}").unwrap();
        assert!(!store.status(DEFAULT_MODEL).unwrap().installed);
    }

    #[test]
    fn verify_reports_wrong_hash_and_missing_files() {
        let (_dir, store) = store();
        let dir = store.model_dir(DEFAULT_MODEL).unwrap();
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("config.json"),
            vec![b'x'; MODEL_FILES[0].bytes as usize],
        )
        .unwrap();
        let report = store.verify(DEFAULT_MODEL).unwrap();
        assert!(report.mismatched.contains(&"config.json".to_string()));
        assert!(report.missing.contains(&"tokenizer.json".to_string()));
        assert!(!report.ok());
    }
}
