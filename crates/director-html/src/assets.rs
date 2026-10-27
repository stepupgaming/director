use crate::CompileError;
use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// Records dependencies while packing, or resolves a closed set of embedded assets.
pub(crate) struct AssetStore {
    base: Option<PathBuf>,
    record: bool,
    bytes: Mutex<BTreeMap<String, Arc<Vec<u8>>>>,
    media: Mutex<Option<tempfile::TempDir>>,
    dependencies: Mutex<BTreeSet<PathBuf>>,
}

pub(crate) struct AssetSnapshot {
    pub references: BTreeMap<String, String>,
    pub blobs: BTreeMap<String, Arc<str>>,
}

impl AssetStore {
    pub fn local(base: &Path, record: bool) -> Self {
        Self {
            base: Some(base.into()),
            record,
            bytes: Mutex::new(BTreeMap::new()),
            media: Mutex::new(None),
            dependencies: Mutex::new(BTreeSet::new()),
        }
    }

    pub fn frozen(
        references: &BTreeMap<String, String>,
        blobs: &BTreeMap<String, Arc<str>>,
    ) -> Result<Self, CompileError> {
        let mut decoded = BTreeMap::new();
        for (digest, encoded) in blobs {
            let bytes = STANDARD
                .decode(encoded.as_bytes())
                .map_err(|e| CompileError::Input(format!("asset {digest}: {e}")))?;
            if hash(&bytes) != *digest {
                return Err(CompileError::Input(format!(
                    "asset {digest}: SHA-256 mismatch"
                )));
            }
            decoded.insert(digest, Arc::new(bytes));
        }
        let mut bytes = BTreeMap::new();
        for (reference, digest) in references {
            let blob = decoded.get(digest).ok_or_else(|| {
                CompileError::Input(format!("asset '{reference}': blob {digest} missing"))
            })?;
            bytes.insert(reference.clone(), blob.clone());
        }
        Ok(Self {
            base: None,
            record: false,
            bytes: Mutex::new(bytes),
            media: Mutex::new(None),
            dependencies: Mutex::new(BTreeSet::new()),
        })
    }

    pub fn read(&self, source: &str) -> Result<Vec<u8>, CompileError> {
        if let Some(data) = source.strip_prefix("data:") {
            let (meta, data) = data
                .split_once(',')
                .ok_or_else(|| CompileError::Input("invalid data URI".into()))?;
            if !meta.ends_with(";base64") {
                return Err(CompileError::Input("data URIs must use base64".into()));
            }
            return STANDARD
                .decode(data)
                .map_err(|e| CompileError::Input(format!("data URI: {e}")));
        }
        if source.contains("://") {
            return Err(CompileError::Input(format!(
                "remote asset '{source}' must be downloaded before native compilation"
            )));
        }
        if let Some(bytes) = self.bytes.lock().map_err(lock_error)?.get(source).cloned() {
            return Ok((*bytes).clone());
        }
        let base = self.base.as_ref().ok_or_else(|| {
            CompileError::Input(format!("project asset '{source}' is not bundled"))
        })?;
        let bytes = std::fs::read(base.join(source)).map_err(|e| {
            CompileError::Input(format!("asset {}: {e}", base.join(source).display()))
        })?;
        self.dependencies
            .lock()
            .map_err(lock_error)?
            .insert(base.join(source));
        if self.record {
            self.bytes
                .lock()
                .map_err(lock_error)?
                .insert(source.into(), Arc::new(bytes.clone()));
        }
        Ok(bytes)
    }

    /// The bytes bundled under `reference`, else the file at `path`, which a
    /// packing store records under `reference`. A remote script resolves to a
    /// local copy this way.
    pub fn read_as(&self, reference: &str, path: Option<&Path>) -> Result<Vec<u8>, CompileError> {
        if let Some(bytes) = self.bytes.lock().map_err(lock_error)?.get(reference).cloned() {
            return Ok((*bytes).clone());
        }
        let path = path.ok_or_else(|| CompileError::Input(format!("'{reference}' has no local copy")))?;
        let bytes = std::fs::read(path).map_err(|e| CompileError::Input(format!("{}: {e}", path.display())))?;
        self.dependencies.lock().map_err(lock_error)?.insert(path.into());
        if self.record {
            self.bytes
                .lock()
                .map_err(lock_error)?
                .insert(reference.into(), Arc::new(bytes.clone()));
        }
        Ok(bytes)
    }

    pub fn media_path(&self, source: &str) -> Result<PathBuf, CompileError> {
        if let Some(base) = &self.base {
            if !source.starts_with("data:") && !source.contains("://") {
                let path = base.join(source);
                if !path.is_file() {
                    return Err(CompileError::Input(format!(
                        "video file missing: {}",
                        path.display()
                    )));
                }
                self.dependencies
                    .lock()
                    .map_err(lock_error)?
                    .insert(path.clone());
                if self.record {
                    self.read(source)?;
                }
                return Ok(path);
            }
        }
        let bytes = self.read(source)?;
        let mut media = self.media.lock().map_err(lock_error)?;
        if media.is_none() {
            *media = Some(
                tempfile::tempdir()
                    .map_err(|e| CompileError::Input(format!("media cache: {e}")))?,
            );
        }
        let path = media.as_ref().unwrap().path().join(hash(&bytes));
        if !path.exists() {
            std::fs::write(&path, bytes)
                .map_err(|e| CompileError::Input(format!("media cache: {e}")))?;
        }
        Ok(path)
    }

    pub fn snapshot(&self) -> Result<AssetSnapshot, CompileError> {
        let mut references = BTreeMap::new();
        let mut blobs = BTreeMap::new();
        for (reference, bytes) in self.bytes.lock().map_err(lock_error)?.iter() {
            let digest = hash(bytes);
            references.insert(reference.clone(), digest.clone());
            blobs
                .entry(digest)
                .or_insert_with(|| Arc::from(STANDARD.encode(bytes.as_ref())));
        }
        Ok(AssetSnapshot { references, blobs })
    }

    pub fn dependencies(&self) -> Vec<PathBuf> {
        self.dependencies
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .cloned()
            .collect()
    }
}

impl director_core::AssetLoader for AssetStore {
    fn load_bytes(&self, path: &str) -> anyhow::Result<Vec<u8>> {
        if path.starts_with("fonts/") {
            let key = format!("director:{path}");
            if let Some(bytes) = self.bytes.lock().map_err(lock_error)?.get(&key).cloned() {
                return Ok((*bytes).clone());
            }
            if self.base.is_none() {
                anyhow::bail!("bundled font '{path}' is missing from project");
            }
            let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../director-core/assets")
                .join(path);
            let bytes = std::fs::read(&bundled)?;
            self.dependencies
                .lock()
                .map_err(lock_error)?
                .insert(bundled);
            if self.record {
                self.bytes
                    .lock()
                    .map_err(lock_error)?
                    .insert(key, Arc::new(bytes.clone()));
            }
            return Ok(bytes);
        }
        Ok(self.read(path)?)
    }

    fn media_path(&self, path: &str) -> Option<PathBuf> {
        AssetStore::media_path(self, path).ok()
    }
}

fn lock_error<T>(_: std::sync::PoisonError<T>) -> CompileError {
    CompileError::Input("project asset lock poisoned".into())
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
