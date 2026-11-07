//! Audio input: media through the `ffmpeg` CLI, and in-memory samples through a downmix and
//! an FFT resampler, both to 16 kHz mono.

use std::path::Path;
use std::process::Command;

use rubato::{FftFixedInOut, Resampler};

use crate::AsrError;

/// The sample rate every model reads.
pub const SAMPLE_RATE: u32 = 16_000;

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

/// Averages the channels of `audio` and resamples the result to 16 kHz.
pub fn to_mono_16k(audio: Audio) -> Result<Vec<f32>, AsrError> {
    let channels = usize::from(audio.channels);
    if channels == 0 || audio.sample_rate == 0 {
        return Err(AsrError::Invalid(format!("audio needs channels and a sample rate, got {channels} at {} Hz", audio.sample_rate)));
    }
    if !audio.samples.len().is_multiple_of(channels) {
        return Err(AsrError::Invalid(format!("{} samples do not split into {channels} channels", audio.samples.len())));
    }
    let mono: Vec<f32> = if channels == 1 {
        audio.samples.to_vec()
    } else {
        audio.samples.chunks_exact(channels).map(|frame| frame.iter().sum::<f32>() / channels as f32).collect()
    };
    resample(&mono, audio.sample_rate)
}

/// Resamples mono samples at `rate` to 16 kHz. The output holds `len * 16000 / rate` samples,
/// rounded, with the resampler's delay removed, so a sound at `t` seconds stays at `t`.
pub fn resample(mono: &[f32], rate: u32) -> Result<Vec<f32>, AsrError> {
    if rate == SAMPLE_RATE || mono.is_empty() {
        return Ok(mono.to_vec());
    }
    let mut resampler = FftFixedInOut::<f32>::new(rate as usize, SAMPLE_RATE as usize, 1024, 1)
        .map_err(|e| AsrError::Resample(e.to_string()))?;
    let delay = resampler.output_delay();
    let want = ((mono.len() as u64 * u64::from(SAMPLE_RATE) + u64::from(rate) / 2) / u64::from(rate)) as usize;
    let mut out = Vec::with_capacity(delay + want + 2048);
    let mut chunk = Vec::new();
    let mut pos = 0;
    while out.len() < delay + want {
        let n = resampler.input_frames_next();
        chunk.clear();
        if pos < mono.len() {
            chunk.extend_from_slice(&mono[pos..(pos + n).min(mono.len())]);
        }
        chunk.resize(n, 0.0);
        pos += n;
        let block = resampler.process(&[&chunk[..]], None).map_err(|e| AsrError::Resample(e.to_string()))?;
        out.extend_from_slice(&block[0]);
    }
    out.truncate(delay + want);
    out.drain(..delay);
    Ok(out)
}
