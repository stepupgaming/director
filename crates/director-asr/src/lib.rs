//! Speech to text for Director.
//!
//! Media decodes to 16 kHz mono ([`audio`]), the Underdog Bark 3.0 models and the `bark-asr`
//! graphs live in a hash-checked cache ([`store`]), the [`Engine`] runs them on ONNX Runtime
//! through `bark-asr`, and word-timed transcripts become SRT, VTT or JSON ([`subtitle`]).
//! Times are `f64` seconds of the source media, the unit of `CaptionWord::start_sec`.

pub mod audio;
pub mod engine;
pub mod store;
pub mod subtitle;

use std::io;

use serde::{Deserialize, Serialize};

pub use audio::{Audio, SAMPLE_RATE};
pub use engine::Engine;
pub use store::{Asset, Graphs, Model, Status, Store};
pub use subtitle::{Cue, CueOptions};

/// One recognized word.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Word {
    /// The word as the model wrote it, punctuation attached.
    pub text: String,
    /// Seconds of the source media where the word's first token was emitted.
    pub start: f64,
    /// Seconds of the source media where the word's last token ends.
    pub end: f64,
}

/// The words of one recording and the model that heard them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transcript {
    pub model: Model,
    pub words: Vec<Word>,
}

impl Transcript {
    /// The words joined by spaces.
    pub fn text(&self) -> String {
        self.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
    }
}

/// A failure decoding audio, managing models or transcribing.
#[derive(Debug, thiserror::Error)]
pub enum AsrError {
    #[error("running {tool}: {source}")]
    Spawn { tool: &'static str, source: io::Error },
    #[error("{tool} failed on {path}: {message}")]
    Tool { tool: &'static str, path: String, message: String },
    #[error("{path}: {source}")]
    Io { path: String, source: io::Error },
    #[error("{0}")]
    Invalid(String),
    #[error("resampling: {0}")]
    Resample(String),
    #[error("unknown model {0}; known models: bark-3.0, bark-3.0-plus, bark-3.0-clean, bark-3.0-pro")]
    UnknownModel(String),
    #[error("{what} is not installed; run `{fetch}`")]
    NotInstalled { what: String, fetch: String },
    #[error("{path} has SHA-256 {actual}, expected {expected}")]
    Hash { path: String, actual: String, expected: String },
    #[error("bark-asr: {0}")]
    Engine(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("transcription cancelled")]
    Cancelled,
}

impl AsrError {
    pub(crate) fn io(path: &std::path::Path, source: io::Error) -> Self {
        Self::Io { path: path.display().to_string(), source }
    }
}
