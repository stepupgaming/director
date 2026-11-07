//! Shot breakdown of a reference video: the measured "video DNA" a
//! replication step works from. This first stage reads the cuts and
//! transitions and the container's provenance; later stages add captions,
//! audio, faces and embeddings.
//!
//! Shot detection and the provenance scan are ported from rea-video
//! (Apache-2.0); see NOTICE.

pub mod media;
pub mod probe;
pub mod shots;
pub mod track;

use std::path::Path;
use std::time::Instant;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use media::{MediaError, VideoInfo};
pub use probe::{provenance, Provenance};
pub use shots::{detect_shots, Shot, ShotOptions, Transition};
pub use track::FrameTrack;

/// The `format` value of a breakdown document.
pub const FORMAT: &str = "director-breakdown/1";

/// A breakdown document, written as `breakdown.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Breakdown {
    pub format: String,
    pub source: Source,
    pub shots: Vec<Shot>,
    pub provenance: Provenance,
}

/// The analysed video.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Source {
    pub path: String,
    pub width: u32,
    pub height: u32,
    /// Display rotation in degrees.
    pub rotation: u32,
    /// The nearest common aspect ratio, such as `9:16`.
    pub aspect_ratio: String,
    pub fps: f64,
    /// Seconds.
    pub duration: f64,
    /// Decoded frames.
    pub frames: usize,
    pub video_codec: String,
    pub has_audio: bool,
    /// Frame times come from packet timestamps because they stray from `frame / fps`.
    pub variable_frame_rate: bool,
}

/// Wall-clock seconds per stage.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Timings {
    pub decode: f64,
    pub detect: f64,
    pub probe: f64,
}

/// Analyses the video at `path`.
pub fn analyze(path: &Path, options: &ShotOptions) -> Result<(Breakdown, Timings), MediaError> {
    let t0 = Instant::now();
    let info = VideoInfo::probe(path)?;
    let track = FrameTrack::read(&info)?;
    let t1 = Instant::now();
    let shots = detect_shots(&track, options);
    let t2 = Instant::now();
    let provenance = provenance(&info);
    let t3 = Instant::now();
    let source = Source {
        path: path.display().to_string(),
        width: info.width,
        height: info.height,
        rotation: info.rotation,
        aspect_ratio: info.aspect_ratio(),
        fps: info.fps,
        duration: info.duration,
        frames: track.n(),
        video_codec: info.vcodec.clone(),
        has_audio: info.has_audio,
        variable_frame_rate: track.times.is_some(),
    };
    let timings = Timings {
        decode: (t1 - t0).as_secs_f64(),
        detect: (t2 - t1).as_secs_f64(),
        probe: (t3 - t2).as_secs_f64(),
    };
    Ok((Breakdown { format: FORMAT.into(), source, shots, provenance }, timings))
}

/// The JSON schema of a breakdown document.
pub fn schema() -> schemars::schema::RootSchema {
    schemars::schema_for!(Breakdown)
}
