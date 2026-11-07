//! Speech to text for Director.
//!
//! Media decodes to 16 kHz mono ([`audio`]), long recordings split into overlapping
//! windows cut at quiet points ([`chunk`]), the Underdog Bark 3.0 models live in a
//! hash-checked cache ([`store`]), and word-timed transcripts become SRT, VTT or JSON
//! ([`subtitle`]). Times are `f64` seconds of the source media, the unit of
//! `CaptionWord::start_sec`.

pub mod audio;
pub mod chunk;
pub mod store;
pub mod subtitle;

use std::io;

use serde::{Deserialize, Serialize};

pub use audio::{Audio, SAMPLE_RATE};
pub use chunk::{Window, WindowOptions};
pub use store::{Model, Status, Store};
pub use subtitle::{Cue, CueOptions};

/// One recognized word.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Word {
    /// The word as the model wrote it, punctuation attached.
    pub text: String,
    /// Seconds of the source media where the word starts.
    pub start: f64,
    /// Seconds of the source media where the word ends.
    pub end: f64,
    /// The model's probability for the word, from 0 to 1.
    pub confidence: f32,
    /// A filler, false start or repeat that the Clean model left out.
    #[serde(default)]
    pub disfluent: bool,
}

impl Word {
    /// The time halfway through the word.
    pub fn midpoint(&self) -> f64 {
        (self.start + self.end) / 2.0
    }
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
    #[error("{model} is not installed; run `director-engine asr fetch {model}`")]
    NotInstalled { model: Model },
    #[error("{path} has SHA-256 {actual}, expected {expected}")]
    Hash { path: String, actual: String, expected: String },
}

impl AsrError {
    pub(crate) fn io(path: &std::path::Path, source: io::Error) -> Self {
        Self::Io { path: path.display().to_string(), source }
    }
}
