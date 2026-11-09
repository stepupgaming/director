//! Speech to text for `add_caption(#{ media })`. Core links no recognizer: a
//! host installs one with [`set_transcriber`], as `director-engine` does when
//! built with its `asr` feature.

use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::node::caption::TimedWord;

/// A media file to transcribe, with an optional model and graph set id such
/// as `bark-3.0-plus` and `int8`. `None` lets the transcriber choose.
#[derive(Clone, Copy, Debug)]
pub struct TranscribeRequest<'a> {
    pub media: &'a Path,
    pub model: Option<&'a str>,
    pub graphs: Option<&'a str>,
}

/// Returns the words spoken in the request's media, timed in its seconds.
pub type Transcriber = dyn Fn(&TranscribeRequest<'_>) -> Result<Vec<TimedWord>, String> + Send + Sync;

static TRANSCRIBER: RwLock<Option<Arc<Transcriber>>> = RwLock::new(None);

/// Installs the process-wide transcriber, replacing any earlier one.
pub fn set_transcriber(transcriber: Arc<Transcriber>) {
    *TRANSCRIBER.write().unwrap_or_else(|e| e.into_inner()) = Some(transcriber);
}

/// True when a transcriber is installed.
pub fn has_transcriber() -> bool {
    TRANSCRIBER.read().unwrap_or_else(|e| e.into_inner()).is_some()
}

/// Runs the installed transcriber. Without one it fails with a message naming
/// the ways to get word times.
pub fn transcribe(request: &TranscribeRequest<'_>) -> Result<Vec<TimedWord>, String> {
    let transcriber = TRANSCRIBER.read().unwrap_or_else(|e| e.into_inner()).clone();
    match transcriber {
        Some(transcriber) => transcriber(request),
        None => Err(format!(
            "no speech to text in this host to transcribe {}: render with a director-engine built with `--features asr`, or pass `file: \"words.json\"` from `director-engine asr transcribe --format json`",
            request.media.display()
        )),
    }
}
