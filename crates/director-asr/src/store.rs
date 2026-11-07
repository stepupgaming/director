//! The Underdog Bark 3.0 models and the `bark-asr` graphs that run them: pinned files, a cache
//! directory, SHA-256 checks, an explicit fetch through the `curl` CLI and an import of files
//! already on disk.

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
/// `nvidia/parakeet-tdt-0.6b-v3` and share one graph set in `bark-asr`.
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

    /// The weights' license at the pinned revision. Director ships no weights; commercial use
    /// needs a license from Conway (license@conway.tech).
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

    /// The weight file as a store asset at `<id>/<file name>`.
    pub fn asset(self) -> Asset {
        let file = self.file();
        Asset {
            label: format!("{self} ({})", file.name),
            path: PathBuf::from(self.id()).join(file.name),
            url: self.url(),
            size: file.size,
            sha256: file.sha256,
            fetch: format!("director-engine asr fetch {self}"),
        }
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

/// The `bark-rs` commit the graphs and their hashes were read from on 2026-11-07, as published
/// in `bark-asr` 0.1.0.
pub const BARK_RS_COMMIT: &str = "12da3350b4a74a8c9efb07a981d047481c0569a1";

/// Which `bark-asr` graph set runs the model. Both build their weights from the model file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Graphs {
    /// 2-bit ternary MatMulNBits terms that reproduce the stored weights bit for bit: the
    /// official transcript at 1.4 GB peak memory.
    #[default]
    Exact,
    /// 8-bit weights with int8 activations: about 3x faster on desktop, and 0.11% of words
    /// differ on the `bark-rs` clips.
    Int8,
}

impl Graphs {
    pub const ALL: [Graphs; 2] = [Graphs::Exact, Graphs::Int8];

    pub fn id(self) -> &'static str {
        match self {
            Graphs::Exact => "exact",
            Graphs::Int8 => "int8",
        }
    }

    /// The folder in the `bark-rs` repository.
    fn repo_dir(self) -> &'static str {
        match self {
            Graphs::Exact => "graphs",
            Graphs::Int8 => "graphs-int8",
        }
    }

    fn files(self) -> [(&'static str, u64, &'static str); 5] {
        const DECODER: (&str, u64, &str) = ("decoder.onnx", 4_230, "660b7678bbc367b9d6c108cc818c4adcfd90c5151d0f8a9773662ea92708226c");
        const JOINT: (&str, u64, &str) = ("joint.onnx", 514, "133d2859f22b836645105204003b7ce3ea98eba0fe52f7702f0c5ea9954b885e");
        const VOCAB: (&str, u64, &str) = ("vocab.json", 115_825, "e308483a6d60bac0c250354eda7a74a0943a31458a48fd19130a7c13c1638d26");
        match self {
            Graphs::Exact => [
                ("encoder.onnx", 1_999_078, "d53fb8caad5dcd0ba54a067d89b2ba212818ccf241826d3cf12def29883ac3f1"),
                DECODER,
                JOINT,
                ("manifest.json", 241_273, "252a3b9c09e80d9711d2e90f000e5ff5429dca70ac823737be516d6995e5d9ff"),
                VOCAB,
            ],
            Graphs::Int8 => [
                ("encoder.onnx", 1_868_992, "685217591eefb259e8d353b9c59e088a08886583ccac4ca2fb81bdc9eb24a4ee"),
                DECODER,
                JOINT,
                ("manifest.json", 133_689, "83d84bc8d2df73b70f8f2391993897603815605fc40ebdb79ec30aea1e2c4d6d"),
                VOCAB,
            ],
        }
    }

    /// The folder under the store root that holds this set.
    pub fn dir(self) -> PathBuf {
        PathBuf::from(format!("bark-rs-{}", &BARK_RS_COMMIT[..7])).join(self.id())
    }

    /// The set's five files (encoder, decoder, joint, manifest, vocabulary) as store assets.
    pub fn assets(self) -> Vec<Asset> {
        self.files()
            .into_iter()
            .map(|(name, size, sha256)| Asset {
                label: format!("{} graphs ({name})", self.id()),
                path: self.dir().join(name),
                url: format!("https://raw.githubusercontent.com/stepupgaming/bark-rs/{BARK_RS_COMMIT}/{}/{name}", self.repo_dir()),
                size,
                sha256,
                fetch: format!("director-engine asr fetch <model> --graphs {}", self.id()),
            })
            .collect()
    }
}

impl fmt::Display for Graphs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

impl FromStr for Graphs {
    type Err = AsrError;

    fn from_str(s: &str) -> Result<Self, AsrError> {
        Graphs::ALL.into_iter().find(|g| g.id() == s).ok_or_else(|| AsrError::Invalid(format!("unknown graphs {s}; use exact or int8")))
    }
}

/// One file the store holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    /// What error messages call it.
    pub label: String,
    /// Where it lives under the store root.
    pub path: PathBuf,
    /// Where [`Store::fetch`] downloads it from.
    pub url: String,
    pub size: u64,
    pub sha256: &'static str,
    /// The command that installs it, for error messages.
    pub fetch: String,
}

/// What the cache holds for an asset, judged by file size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Missing,
    /// A file with the wrong size, such as an interrupted download.
    WrongSize(u64),
    /// The right size; [`Store::verify`] checks the hash.
    Present,
}

/// A cache directory holding `<model id>/<file name>` for each model and
/// `bark-rs-<commit>/<graphs>/` for each graph set.
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

    /// Where the asset lives once installed.
    pub fn path(&self, asset: &Asset) -> PathBuf {
        self.root.join(&asset.path)
    }

    pub fn status(&self, asset: &Asset) -> Status {
        match fs::metadata(self.path(asset)) {
            Err(_) => Status::Missing,
            Ok(meta) if meta.len() == asset.size => Status::Present,
            Ok(meta) => Status::WrongSize(meta.len()),
        }
    }

    /// Checks the installed file's size and SHA-256 and returns its path.
    pub fn verify(&self, asset: &Asset) -> Result<PathBuf, AsrError> {
        let path = self.path(asset);
        match self.status(asset) {
            Status::Missing => return Err(AsrError::NotInstalled { what: asset.label.clone(), fetch: asset.fetch.clone() }),
            Status::WrongSize(size) => {
                return Err(AsrError::Invalid(format!(
                    "{} holds {size} bytes, expected {}; run `{}` again",
                    path.display(),
                    asset.size,
                    asset.fetch
                )))
            }
            Status::Present => {}
        }
        check_hash(&path, asset.sha256)?;
        Ok(path)
    }

    /// Verifies the model's weight file and returns its path.
    pub fn model(&self, model: Model) -> Result<PathBuf, AsrError> {
        self.verify(&model.asset())
    }

    /// Verifies every file of the graph set and returns its folder.
    pub fn graphs(&self, graphs: Graphs) -> Result<PathBuf, AsrError> {
        for asset in graphs.assets() {
            self.verify(&asset)?;
        }
        Ok(self.root.join(graphs.dir()))
    }

    /// Downloads the asset through `curl`, resuming a partial `.part` file, and moves it into
    /// place once its hash matches. A file already in place with the right hash downloads
    /// nothing.
    pub fn fetch(&self, asset: &Asset) -> Result<PathBuf, AsrError> {
        let path = self.path(asset);
        if self.status(asset) == Status::Present && check_hash(&path, asset.sha256).is_ok() {
            return Ok(path);
        }
        let part = part_path(&path)?;
        let output = Command::new("curl")
            .args(["--location", "--fail", "--silent", "--show-error", "--retry", "3", "--continue-at", "-", "--output"])
            .arg(&part)
            .arg(&asset.url)
            .output()
            .map_err(|source| AsrError::Spawn { tool: "curl", source })?;
        if !output.status.success() {
            return Err(AsrError::Tool {
                tool: "curl",
                path: asset.url.clone(),
                message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        if let Err(e) = check_hash(&part, asset.sha256) {
            let _ = fs::remove_file(&part);
            return Err(e);
        }
        fs::rename(&part, &path).map_err(|e| AsrError::io(&path, e))?;
        Ok(path)
    }

    /// Installs a copy of the asset already on disk, such as a model downloaded by hand. The
    /// source must match the pinned size and hash. The store hard-links it when both sit on
    /// one volume and copies it otherwise.
    pub fn import(&self, asset: &Asset, source: &Path) -> Result<PathBuf, AsrError> {
        let size = fs::metadata(source).map_err(|e| AsrError::io(source, e))?.len();
        if size != asset.size {
            return Err(AsrError::Invalid(format!("{} holds {size} bytes, {} has {}", source.display(), asset.label, asset.size)));
        }
        check_hash(source, asset.sha256)?;
        let path = self.path(asset);
        let part = part_path(&path)?;
        let _ = fs::remove_file(&part);
        if fs::hard_link(source, &part).is_err() {
            fs::copy(source, &part).map_err(|e| AsrError::io(&part, e))?;
        }
        if path.exists() {
            fs::remove_file(&path).map_err(|e| AsrError::io(&path, e))?;
        }
        fs::rename(&part, &path).map_err(|e| AsrError::io(&path, e))?;
        Ok(path)
    }
}

/// `<file>.<ext>.part` beside the target, with the target's folder created.
fn part_path(path: &Path) -> Result<PathBuf, AsrError> {
    let dir = path.parent().expect("store paths have a parent");
    fs::create_dir_all(dir).map_err(|e| AsrError::io(dir, e))?;
    Ok(path.with_extension(format!("{}.part", path.extension().and_then(|e| e.to_str()).unwrap_or(""))))
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
