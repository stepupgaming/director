//! The Underdog Bark 3.0 models: pinned Hugging Face files, a cache directory, SHA-256 checks
//! and an explicit fetch through the `curl` CLI.

use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::AsrError;

/// One of the four Bark 3.0 models. All four are 0.6 B English post-trainings of
/// `nvidia/parakeet-tdt-0.6b-v3` and share one graph set in `bark-rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Model {
    #[serde(rename = "bark-3.0")]
    Bark,
    #[serde(rename = "bark-3.0-plus")]
    BarkPlus,
    #[serde(rename = "bark-3.0-clean")]
    BarkClean,
    #[serde(rename = "bark-3.0-pro")]
    BarkPro,
}

/// The weight file a model needs, as Hugging Face lists it at the pinned revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelFile {
    pub name: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

impl Model {
    pub const ALL: [Model; 4] = [Model::Bark, Model::BarkPlus, Model::BarkClean, Model::BarkPro];

    /// The name commands and documents use.
    pub fn id(self) -> &'static str {
        match self {
            Model::Bark => "bark-3.0",
            Model::BarkPlus => "bark-3.0-plus",
            Model::BarkClean => "bark-3.0-clean",
            Model::BarkPro => "bark-3.0-pro",
        }
    }

    pub fn repo(self) -> &'static str {
        match self {
            Model::Bark => "ConwayResearch/Underdog-Bark-3.0",
            Model::BarkPlus => "ConwayResearch/Underdog-Bark-3.0-Plus",
            Model::BarkClean => "ConwayResearch/Underdog-Bark-3.0-Clean",
            Model::BarkPro => "ConwayResearch/Underdog-Bark-3.0-Pro",
        }
    }

    /// The repository commit the file and hash were read from on 2026-11-07.
    pub fn revision(self) -> &'static str {
        match self {
            Model::Bark => "e7a641c0fdd4cdf1818dde62a98c26e11d65c061",
            Model::BarkPlus => "28a7d4234146b0f01c11e0c6b72dfa29028a4758",
            Model::BarkClean => "73775471a9410b75ceb2401e837861dac12bd76d",
            Model::BarkPro => "d87f288a9a0b0cdb92228057e168ce575b300a6b",
        }
    }

    pub fn file(self) -> ModelFile {
        match self {
            Model::Bark => ModelFile {
                name: "model.bark",
                size: 163_398_645,
                sha256: "c18de9418d73cbeb80b8c1287b392d480a56e191ef4a81d1baaa1b02e66d92ed",
            },
            Model::BarkPlus => ModelFile {
                name: "model.bark",
                size: 179_524_845,
                sha256: "fdd9f1052e4fdead07d71e8cd2138813eb0c209fd6ff8b1b6f47737e1e501857",
            },
            Model::BarkClean => ModelFile {
                name: "model.bark",
                size: 179_458_613,
                sha256: "b67ca90d94b79a36af4616f1cb3f241884925de7aa9f3e673a8d5c6c4cfd3436",
            },
            Model::BarkPro => ModelFile {
                name: "model.safetensors",
                size: 1_254_196_636,
                sha256: "6f84b33e63ebd1851e910e8b2fe299d29f5e0efc6f57e84d92d2f79af464e0e5",
            },
        }
    }

    /// The weights' license. Director ships no weights; commercial use needs a license from
    /// Conway (license@conway.tech).
    pub fn license(self) -> &'static str {
        "CC-BY-NC-4.0"
    }

    pub fn description(self) -> &'static str {
        match self {
            Model::Bark => "verbatim, the default",
            Model::BarkPlus => "verbatim, more accurate",
            Model::BarkClean => "drops fillers, false starts and repeats",
            Model::BarkPro => "BF16 weights, the most accurate",
        }
    }

    /// The download URL at the pinned revision.
    pub fn url(self) -> String {
        format!("https://huggingface.co/{}/resolve/{}/{}", self.repo(), self.revision(), self.file().name)
    }
}

impl fmt::Display for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

impl FromStr for Model {
    type Err = AsrError;

    fn from_str(s: &str) -> Result<Self, AsrError> {
        Model::ALL.into_iter().find(|m| m.id() == s).ok_or_else(|| AsrError::UnknownModel(s.to_string()))
    }
}

/// What the cache holds for a model, judged by file size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Missing,
    /// A file with the wrong size, such as an interrupted download.
    WrongSize(u64),
    /// The right size; [`Store::verify`] checks the hash.
    Present,
}

/// A cache directory holding `<model id>/<file name>` for each fetched model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `DIRECTOR_ASR_CACHE`, else `director/asr` under the platform cache directory:
    /// `%LOCALAPPDATA%` on Windows, `~/Library/Caches` on macOS, `$XDG_CACHE_HOME` or
    /// `~/.cache` elsewhere.
    pub fn default_root() -> Option<PathBuf> {
        if let Some(dir) = std::env::var_os("DIRECTOR_ASR_CACHE") {
            return Some(PathBuf::from(dir));
        }
        let home = || std::env::var_os("HOME").map(PathBuf::from);
        let base = if cfg!(windows) {
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
        } else if cfg!(target_os = "macos") {
            home().map(|h| h.join("Library").join("Caches"))
        } else {
            std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| home().map(|h| h.join(".cache")))
        };
        base.map(|b| b.join("director").join("asr"))
    }

    /// The store at [`Store::default_root`].
    pub fn open_default() -> Result<Self, AsrError> {
        Self::default_root()
            .map(Self::new)
            .ok_or_else(|| AsrError::Invalid("no cache directory; set DIRECTOR_ASR_CACHE".to_string()))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where the model's weight file lives once fetched.
    pub fn path(&self, model: Model) -> PathBuf {
        self.root.join(model.id()).join(model.file().name)
    }

    pub fn status(&self, model: Model) -> Status {
        match fs::metadata(self.path(model)) {
            Err(_) => Status::Missing,
            Ok(meta) if meta.len() == model.file().size => Status::Present,
            Ok(meta) => Status::WrongSize(meta.len()),
        }
    }

    /// Checks the fetched file's size and SHA-256 and returns its path.
    pub fn verify(&self, model: Model) -> Result<PathBuf, AsrError> {
        let path = self.path(model);
        match self.status(model) {
            Status::Missing => return Err(AsrError::NotInstalled { model }),
            Status::WrongSize(size) => {
                return Err(AsrError::Invalid(format!(
                    "{} holds {size} bytes, expected {}; run `director-engine asr fetch {model}` again",
                    path.display(),
                    model.file().size
                )))
            }
            Status::Present => {}
        }
        check_hash(&path, model.file().sha256)?;
        Ok(path)
    }

    /// Downloads the model's weight file at its pinned revision through `curl`, resuming a
    /// partial `.part` file, and moves it into place once its hash matches. A file already in
    /// place with the right hash downloads nothing.
    pub fn fetch(&self, model: Model) -> Result<PathBuf, AsrError> {
        let path = self.path(model);
        if self.status(model) == Status::Present && check_hash(&path, model.file().sha256).is_ok() {
            return Ok(path);
        }
        let dir = path.parent().expect("model paths have a parent");
        fs::create_dir_all(dir).map_err(|e| AsrError::io(dir, e))?;
        let part = path.with_extension(format!("{}.part", path.extension().and_then(|e| e.to_str()).unwrap_or("")));
        let output = Command::new("curl")
            .args(["--location", "--fail", "--retry", "3", "--continue-at", "-", "--output"])
            .arg(&part)
            .arg(model.url())
            .output()
            .map_err(|source| AsrError::Spawn { tool: "curl", source })?;
        if !output.status.success() {
            return Err(AsrError::Tool {
                tool: "curl",
                path: model.url(),
                message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        if let Err(e) = check_hash(&part, model.file().sha256) {
            let _ = fs::remove_file(&part);
            return Err(e);
        }
        fs::rename(&part, &path).map_err(|e| AsrError::io(&path, e))?;
        Ok(path)
    }
}

fn check_hash(path: &Path, expected: &str) -> Result<(), AsrError> {
    let actual = sha256_file(path)?;
    if actual == expected {
        Ok(())
    } else {
        Err(AsrError::Hash { path: path.display().to_string(), actual, expected: expected.to_string() })
    }
}

/// The lowercase hex SHA-256 of a file, read in 1 MiB blocks.
pub fn sha256_file(path: &Path) -> Result<String, AsrError> {
    let mut file = fs::File::open(path).map_err(|e| AsrError::io(path, e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).map_err(|e| AsrError::io(path, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
