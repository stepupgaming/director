//! Finished video files for QA: one ffprobe pass, selected frames decoded
//! in frame order, and the audio stream as 48 kHz float stereo.

use crate::error::{ErrorCode, HostError};
use director_core::video_wrapper::{parse_frame_rate, FFmpegDriver};
use serde_json::Value;
use std::io::{BufReader, Read};
use std::path::Path;
use std::process::{Child, ChildStdout, Stdio};

/// Sample rate the audio stream decodes at.
pub(super) const AUDIO_RATE: u32 = 48_000;

fn invalid(message: String) -> HostError {
    HostError::new(ErrorCode::InvalidArgument, message)
}

/// The first video stream and the container of a file.
#[derive(Clone, Debug)]
pub(super) struct Probe {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration: f64,
    /// Audio streams in the file.
    pub audio: usize,
}

pub(super) fn probe(path: &Path) -> Result<Probe, HostError> {
    let unreadable = |reason: String| invalid(format!("video {}: {reason}", path.display()));
    if !path.is_file() {
        return Err(unreadable("not a file".into()));
    }
    let output = director_core::video_wrapper::ffprobe()
        .args(["-v", "error", "-show_entries", "stream=codec_type,width,height,avg_frame_rate,r_frame_rate:format=duration", "-of", "json"])
        .arg(path)
        .output()
        .map_err(|error| unreadable(format!("ffprobe: {error}")))?;
    let json: Value = serde_json::from_slice(&output.stdout).map_err(|error| unreadable(error.to_string()))?;
    let streams = json["streams"].as_array().cloned().unwrap_or_default();
    let video = streams.iter().find(|stream| stream["codec_type"] == "video").ok_or_else(|| unreadable("no video stream".into()))?;
    let size = |key: &str| video[key].as_u64().filter(|value| *value > 0).map(|value| value as u32);
    let (Some(width), Some(height)) = (size("width"), size("height")) else {
        return Err(unreadable("no frame size".into()));
    };
    let fps = ["avg_frame_rate", "r_frame_rate"]
        .iter()
        .find_map(|key| video[*key].as_str().and_then(parse_frame_rate))
        .ok_or_else(|| unreadable("no frame rate".into()))?;
    let duration = json["format"]["duration"]
        .as_str()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| unreadable("no duration".into()))?;
    Ok(Probe {
        width,
        height,
        fps,
        duration,
        audio: streams.iter().filter(|stream| stream["codec_type"] == "audio").count(),
    })
}

/// What [`Video::next`] returns per frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Pixels {
    /// The full-range luma plane.
    Luma,
    /// Straight RGBA.
    Rgba,
}

/// Selected frames of a video, in frame order.
pub(super) struct Video {
    child: Child,
    reader: BufReader<ChildStdout>,
    width: u32,
    height: u32,
    frame_len: usize,
    keep: usize,
}

impl Video {
    /// Decodes the frames showing at `times` on the video's clock (constant
    /// frame rate), with each frame index and its time, deduplicated.
    pub(super) fn open(path: &Path, probe: &Probe, times: &[f64], pixels: Pixels) -> Result<(Self, Vec<(u64, f64)>), HostError> {
        let mut frames: Vec<(u64, f64)> = Vec::new();
        for &time in times {
            let index = (time * probe.fps + 1e-6).floor() as u64;
            if frames.last().is_none_or(|(last, _)| *last < index) {
                frames.push((index, index as f64 / probe.fps));
            }
        }
        let select: Vec<String> = frames.iter().map(|(index, _)| format!("eq(n\\,{index})")).collect();
        let format = match pixels {
            Pixels::Luma => "yuv420p",
            Pixels::Rgba => "rgba",
        };
        let mut child = FFmpegDriver::command()
            .args(["-hide_banner", "-nostats", "-loglevel", "error", "-i"])
            .arg(path)
            .arg("-vf")
            .arg(format!("select='{}',scale=out_range=full", select.join("+")))
            .args(["-fps_mode", "passthrough", "-f", "rawvideo", "-pix_fmt", format, "-"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| invalid(format!("decoding {}: {error}", path.display())))?;
        let stdout = child.stdout.take().ok_or_else(|| invalid("ffmpeg decoder stdout unavailable".into()))?;
        let (width, height) = (probe.width as usize, probe.height as usize);
        let (frame_len, keep) = match pixels {
            Pixels::Luma => (width * height + 2 * width.div_ceil(2) * height.div_ceil(2), width * height),
            Pixels::Rgba => (width * height * 4, width * height * 4),
        };
        let video = Self {
            child,
            reader: BufReader::new(stdout),
            width: probe.width,
            height: probe.height,
            frame_len,
            keep,
        };
        Ok((video, frames))
    }

    pub(super) fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The next selected frame, which `open` listed at `index`.
    pub(super) fn next(&mut self, index: u64) -> Result<Vec<u8>, HostError> {
        let mut buffer = vec![0u8; self.frame_len];
        self.reader.read_exact(&mut buffer).map_err(|_| invalid(format!("the video ends before frame {index}")))?;
        buffer.truncate(self.keep);
        Ok(buffer)
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The first audio stream as interleaved stereo at [`AUDIO_RATE`].
pub(super) fn audio(path: &Path) -> Result<Vec<f32>, HostError> {
    let output = FFmpegDriver::command()
        .args(["-hide_banner", "-nostats", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-map", "0:a:0", "-vn", "-f", "f32le", "-ac", "2", "-ar"])
        .arg(AUDIO_RATE.to_string())
        .arg("-")
        .stderr(Stdio::null())
        .output()
        .map_err(|error| invalid(format!("decoding the audio of {}: {error}", path.display())))?;
    if !output.status.success() {
        return Err(invalid(format!("decoding the audio of {} failed", path.display())));
    }
    Ok(output.stdout.as_chunks::<4>().0.iter().map(|bytes| f32::from_le_bytes(*bytes)).collect())
}
