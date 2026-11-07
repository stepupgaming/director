//! Audio input: media through the `ffmpeg` CLI, and in-memory samples through `bark-asr`'s
//! downmix and FFT resampler, both to 16 kHz mono.

use std::path::Path;
use std::process::Command;

use crate::AsrError;

/// The sample rate every model reads.
pub const SAMPLE_RATE: u32 = bark_asr::audio::RATE;

/// Interleaved samples at any rate and channel count.
#[derive(Debug, Clone, Copy)]
pub struct Audio<'a> {
    pub samples: &'a [f32],
    pub sample_rate: u32,
    pub channels: u16,
}

/// Decodes the first audio stream of `path` (any container ffmpeg reads, video included) to
/// 16 kHz mono through the `ffmpeg` CLI. Sample 0 is the first decoded sample.
pub fn decode(path: &Path) -> Result<Vec<f32>, AsrError> {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(path)
        .args(["-map", "0:a:0", "-ac", "1", "-ar", "16000", "-f", "f32le", "-"])
        .output()
        .map_err(|source| AsrError::Spawn { tool: "ffmpeg", source })?;
    if !output.status.success() {
        return Err(AsrError::Tool {
            tool: "ffmpeg",
            path: path.display().to_string(),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(output.stdout.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect())
}

/// Averages the channels of `audio` and resamples the result to 16 kHz with the resampler
/// `bark-asr` measured against the authors' soxr path. A sound at `t` seconds stays at `t`.
pub fn to_mono_16k(audio: Audio) -> Result<Vec<f32>, AsrError> {
    let channels = usize::from(audio.channels);
    if channels == 0 || audio.sample_rate == 0 {
        return Err(AsrError::Invalid(format!("audio needs channels and a sample rate, got {channels} at {} Hz", audio.sample_rate)));
    }
    if !audio.samples.len().is_multiple_of(channels) {
        return Err(AsrError::Invalid(format!("{} samples do not split into {channels} channels", audio.samples.len())));
    }
    bark_asr::audio::to_model_input(audio.samples, channels, audio.sample_rate).map_err(|e| AsrError::Resample(format!("{e:#}")))
}
