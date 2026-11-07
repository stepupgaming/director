//! `ffprobe` metadata and decoded frames through an `ffmpeg` rawvideo pipe.
//! Ported from rea-video `media.py` (Apache-2.0, see NOTICE).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

/// A failure reading the video.
#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("running {tool}: {source}")]
    Spawn { tool: &'static str, source: std::io::Error },
    #[error("{tool} failed on {path}: {message}")]
    Tool { tool: &'static str, path: String, message: String },
    #[error("{0} has no video stream")]
    NoVideo(String),
    #[error("reading {path}: {source}")]
    Io { path: String, source: std::io::Error },
}

/// What `ffprobe` reports about a video's first video stream and container.
#[derive(Debug, Clone)]
pub struct VideoInfo {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration: f64,
    pub nb_frames: u64,
    pub vcodec: String,
    pub has_audio: bool,
    /// Display rotation in degrees from the stream's side data or `rotate` tag.
    pub rotation: u32,
    /// The whole `ffprobe -show_format -show_streams` document.
    pub raw: Value,
}

fn ratio(value: Option<&Value>) -> f64 {
    let Some(text) = value.and_then(Value::as_str) else { return 0.0 };
    match text.split_once('/') {
        Some((a, b)) => match (a.parse::<f64>(), b.parse::<f64>()) {
            (Ok(a), Ok(b)) if b != 0.0 => a / b,
            _ => 0.0,
        },
        None => text.parse().unwrap_or(0.0),
    }
}

fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

impl VideoInfo {
    /// Probes `path` with `ffprobe`.
    pub fn probe(path: &Path) -> Result<Self, MediaError> {
        let output = Command::new("ffprobe")
            .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"])
            .arg(path)
            .output()
            .map_err(|source| MediaError::Spawn { tool: "ffprobe", source })?;
        if !output.status.success() {
            return Err(MediaError::Tool {
                tool: "ffprobe",
                path: path.display().to_string(),
                message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        let raw: Value = serde_json::from_slice(&output.stdout).map_err(|e| MediaError::Tool {
            tool: "ffprobe",
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        Self::from_probe(path, raw)
    }

    /// Reads an `ffprobe` JSON document.
    pub fn from_probe(path: &Path, raw: Value) -> Result<Self, MediaError> {
        let empty = Vec::new();
        let streams = raw.get("streams").and_then(Value::as_array).unwrap_or(&empty);
        let is_video = |s: &&Value| {
            s.get("codec_type").and_then(Value::as_str) == Some("video")
                && s.pointer("/disposition/attached_pic").and_then(Value::as_i64).unwrap_or(0) == 0
        };
        let v = streams.iter().find(is_video).ok_or_else(|| MediaError::NoVideo(path.display().to_string()))?;
        let format = raw.get("format").cloned().unwrap_or(Value::Null);
        let mut fps = ratio(v.get("avg_frame_rate"));
        if fps <= 0.0 {
            fps = ratio(v.get("r_frame_rate"));
        }
        if !(fps > 0.0 && fps.is_finite()) {
            fps = 30.0;
        }
        let duration = number(v.get("duration")).or_else(|| number(format.get("duration"))).unwrap_or(0.0);
        let nb_frames = number(v.get("nb_frames"))
            .filter(|&n| n > 0.0)
            .unwrap_or((duration * fps).round())
            .max(0.0) as u64;
        let mut rotation = 0;
        if let Some(list) = v.get("side_data_list").and_then(Value::as_array) {
            for sd in list {
                if let Some(r) = number(sd.get("rotation")) {
                    rotation = (r.abs() as i64).rem_euclid(360) as u32;
                }
            }
        }
        if let Some(r) = v.pointer("/tags/rotate").and_then(|t| t.as_str()).and_then(|t| t.parse::<i64>().ok()) {
            rotation = r.rem_euclid(360) as u32;
        }
        let has_audio = streams.iter().any(|s| s.get("codec_type").and_then(Value::as_str) == Some("audio"));
        Ok(Self {
            path: path.to_path_buf(),
            width: number(v.get("width")).unwrap_or(0.0) as u32,
            height: number(v.get("height")).unwrap_or(0.0) as u32,
            fps,
            duration,
            nb_frames,
            vcodec: v.get("codec_name").and_then(Value::as_str).unwrap_or("").to_string(),
            has_audio,
            rotation,
            raw,
        })
    }

    /// Width and height as shown, after rotation.
    pub fn display_size(&self) -> (u32, u32) {
        if matches!(self.rotation, 90 | 270) {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }

    /// The nearest common aspect ratio name, else `W:H`.
    pub fn aspect_ratio(&self) -> String {
        let (w, h) = self.display_size();
        if w == 0 || h == 0 {
            return "unknown".into();
        }
        let r = w as f64 / h as f64;
        let named = [("9:16", 9.0 / 16.0), ("3:4", 0.75), ("6:7", 6.0 / 7.0), ("1:1", 1.0), ("4:3", 4.0 / 3.0), ("16:9", 16.0 / 9.0), ("21:9", 21.0 / 9.0)];
        named
            .iter()
            .find(|(_, v)| (r - v).abs() / v < 0.04)
            .map(|(n, _)| n.to_string())
            .unwrap_or_else(|| format!("{w}:{h}"))
    }

    /// The even output size of frames scaled to `width` in display orientation.
    pub fn scaled_size(&self, width: u32) -> (u32, u32) {
        let (w, h) = self.display_size();
        let mut width = if w > 0 { width.min(w) } else { width };
        width -= width % 2;
        let mut height = if w > 0 { (h as f64 * width as f64 / w as f64).round() as u32 } else { width };
        height -= height % 2;
        (width.max(2), height.max(2))
    }
}

/// Decodes every frame of the video as packed RGB at `width` (area scaling),
/// passing frames through without frame rate conversion so duplicate frames
/// stay visible, and hands each to `each`. Returns the frame count.
pub fn for_each_frame(info: &VideoInfo, width: u32, mut each: impl FnMut(&[u8])) -> Result<usize, MediaError> {
    let (w, h) = info.scaled_size(width);
    let mut child = Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(&info.path)
        .args(["-an", "-sn", "-vf", &format!("scale={w}:{h}:flags=area"), "-fps_mode", "passthrough"])
        .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| MediaError::Spawn { tool: "ffmpeg", source })?;
    // Drain stderr on its own thread so a damaged file's error stream cannot fill the pipe and stall stdout.
    let mut stderr = child.stderr.take().expect("piped stderr");
    let errors = std::thread::spawn(move || {
        let mut text = Vec::new();
        let _ = stderr.read_to_end(&mut text);
        text
    });
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut frame = vec![0u8; (w * h * 3) as usize];
    let mut count = 0;
    let read = loop {
        match stdout.read_exact(&mut frame) {
            Ok(()) => {
                count += 1;
                each(&frame);
            }
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break Ok(()),
            Err(e) => break Err(e),
        }
    };
    drop(stdout);
    let _ = child.kill();
    let _ = child.wait();
    let errors = errors.join().unwrap_or_default();
    read.map_err(|source| MediaError::Io { path: info.path.display().to_string(), source })?;
    if count == 0 {
        let text = String::from_utf8_lossy(&errors);
        let tail: String = text.trim().chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect();
        return Err(MediaError::Tool {
            tool: "ffmpeg",
            path: info.path.display().to_string(),
            message: if tail.is_empty() { "decoded no frames".into() } else { tail },
        });
    }
    Ok(count)
}

/// Real timestamps (seconds from the first frame) of `n` passed-through
/// frames, read from packet pts without decoding. `None` when they match
/// `i / fps` within half a frame (constant frame rate) or when the packet
/// count differs from the decoded frame count.
pub fn frame_times(path: &Path, n: usize, fps: f64) -> Option<Vec<f64>> {
    if n < 2 || fps <= 0.0 {
        return None;
    }
    let output = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "packet=pts_time", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut pts: Vec<f64> = text
        .split_whitespace()
        .map(|s| s.trim().trim_end_matches(','))
        .filter(|s| !s.is_empty() && *s != "N/A")
        .filter_map(|s| s.parse().ok())
        .collect();
    if pts.len() != n {
        return None;
    }
    pts.sort_by(f64::total_cmp);
    let first = pts[0];
    let times: Vec<f64> = pts.iter().map(|p| p - first).collect();
    let off = times.iter().enumerate().map(|(i, t)| (t - i as f64 / fps).abs()).fold(0.0, f64::max);
    (off > 0.5 / fps).then_some(times)
}
