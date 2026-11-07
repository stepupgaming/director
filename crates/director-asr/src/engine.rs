//! The Bark engine: a model and a graph set from the [`Store`], run by `bark-asr` on ONNX
//! Runtime, reading long audio piece by piece with progress and cancellation.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::store::{Graphs, Model, Store};
use crate::{AsrError, Transcript, Word, SAMPLE_RATE};

/// A loaded model, ready to transcribe 16 kHz mono samples.
pub struct Engine {
    bark: bark_asr::Bark,
    model: Model,
    graphs: Graphs,
    max_piece: f64,
}

impl Engine {
    /// Verifies the model's weights and the graph set in `store`, then builds the ONNX Runtime
    /// sessions. `threads` 0 lets ONNX Runtime choose.
    pub fn open(store: &Store, model: Model, graphs: Graphs, threads: usize) -> Result<Self, AsrError> {
        if model == Model::BarkPro {
            return Err(AsrError::Unsupported(format!("{model} ships BF16 safetensors, which bark-asr does not load yet")));
        }
        let weights = store.model(model)?;
        let dir = store.graphs(graphs)?;
        Self::load(&weights, &dir, model, graphs, threads)
    }

    /// Loads weights and graphs from any paths, skipping the store's hash checks.
    pub fn load(weights: &Path, graphs_dir: &Path, model: Model, graphs: Graphs, threads: usize) -> Result<Self, AsrError> {
        let bark = bark_asr::Bark::load(weights, graphs_dir, threads).map_err(|e| AsrError::Engine(format!("{e:#}")))?;
        Ok(Self { bark, model, graphs, max_piece: bark_asr::model::MAX_PIECE })
    }

    pub fn model(&self) -> Model {
        self.model
    }

    pub fn graphs(&self) -> Graphs {
        self.graphs
    }

    /// The longest piece, in seconds, read in one pass (default 30). Longer audio is cut at its
    /// quietest points into pieces between a third of this and this long.
    pub fn set_max_piece(&mut self, seconds: f64) {
        self.max_piece = seconds.max(1.0);
    }

    /// Transcribes 16 kHz mono samples. Word times are seconds from sample 0. `progress` gets
    /// the seconds read so far and the total after each piece; `cancel` is checked before each
    /// piece.
    pub fn transcribe(&mut self, samples: &[f32], progress: &mut dyn FnMut(f64, f64), cancel: &AtomicBool) -> Result<Transcript, AsrError> {
        let rate = f64::from(SAMPLE_RATE);
        let total = samples.len() as f64 / rate;
        self.bark.set_max_piece(self.max_piece);
        let mut words: Vec<Word> = Vec::new();
        for piece in bark_asr::audio::segments(samples, self.max_piece, self.max_piece / 3.0) {
            if cancel.load(Ordering::Relaxed) {
                return Err(AsrError::Cancelled);
            }
            let offset = piece.start as f64 / rate;
            let done = piece.end as f64 / rate;
            let heard = self.bark.transcribe(&samples[piece]).map_err(|e| AsrError::Engine(format!("{e:#}")))?;
            for w in heard.words {
                let (start, end) = (w.start + offset, w.end + offset);
                // Punctuation opening a piece belongs to the last word of the piece before.
                if let Some(prev) = words.last_mut().filter(|_| w.text.chars().all(|c| !c.is_alphanumeric())) {
                    prev.text.push_str(&w.text);
                    continue;
                }
                words.push(Word { text: w.text, start, end });
            }
            progress(done, total);
        }
        Ok(Transcript { model: self.model, words })
    }
}
