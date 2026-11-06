//! Decoding a video's luma through the `ffmpeg` and `ffprobe` CLIs.

use std::path::Path;
use std::process::Command;

use crate::image::Gray;

/// A video's frames as luma, at most `max_width` wide.
pub struct Footage {
    /// The source size in pixels.
    pub width: u32,
    pub height: u32,
    /// Frames per second.
    pub fps: f64,
    /// Every frame in order, downscaled when the source is wider than asked.
    pub frames: Vec<Gray>,
    /// The regions the solve leaves out, at the frames' size: luma above
    /// half marks a pixel to ignore, such as a moving car or water. Frame k
    /// reads matte k, and frames past the matte's end read its last frame,
    /// so a still image covers the whole shot. Empty for none.
    pub matte: Vec<Gray>,
}

impl Footage {
    /// Reads the matte at `path`, a video or an image, at the frames' size.
    pub fn read_matte(&mut self, path: &Path) -> Result<(), String> {
        let first = self.frames.first().ok_or("footage without frames takes no matte")?;
        self.matte = decode(path, first.width as u32, first.height as u32)?;
        Ok(())
    }
}

/// Reads every frame of `path` as luma. Footage wider than `max_width` is
/// scaled down with area averaging, which keeps the solve's field of view.
pub fn read_footage(path: &Path, max_width: u32) -> Result<Footage, String> {
    let probe = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height,avg_frame_rate", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .map_err(|e| format!("running ffprobe: {e}"))?;
    if !probe.status.success() {
        return Err(format!("ffprobe could not read {}: {}", path.display(), String::from_utf8_lossy(&probe.stderr).trim()));
    }
    let text = String::from_utf8_lossy(&probe.stdout);
    let fields: Vec<&str> = text.trim().split(',').map(str::trim).collect();
    let [width, height, rate] = fields[..] else {
        return Err(format!("{} has no video stream", path.display()));
    };
    let parse = |s: &str| s.parse::<u32>().map_err(|_| format!("{} reports a size of {text}", path.display()));
    let (width, height) = (parse(width)?, parse(height)?);
    let fps = match rate.split_once('/') {
        Some((n, d)) => n.parse::<f64>().unwrap_or(0.0) / d.parse::<f64>().unwrap_or(1.0),
        None => rate.parse().unwrap_or(0.0),
    };
    if width == 0 || height == 0 || fps.is_nan() || fps <= 0.0 {
        return Err(format!("{} reports {width}x{height} at {rate} fps", path.display()));
    }
    let (w, h) = if width > max_width {
        (max_width, ((height as f64 * max_width as f64 / width as f64).round() as u32).max(1))
    } else {
        (width, height)
    };
    let frames = decode(path, w, h)?;
    Ok(Footage { width, height, fps, frames, matte: Vec::new() })
}

/// Every frame of `path` as luma scaled to `w` by `h` with area averaging.
fn decode(path: &Path, w: u32, h: u32) -> Result<Vec<Gray>, String> {
    let decoded = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-vf", &format!("scale={w}:{h}:flags=area"), "-f", "rawvideo", "-pix_fmt", "gray", "-"])
        .output()
        .map_err(|e| format!("running ffmpeg: {e}"))?;
    if !decoded.status.success() {
        return Err(format!("ffmpeg could not decode {}: {}", path.display(), String::from_utf8_lossy(&decoded.stderr).trim()));
    }
    let size = (w * h) as usize;
    let frames = decoded
        .stdout
        .chunks_exact(size)
        .map(|frame| Gray::new(w as usize, h as usize, frame.iter().map(|&v| v as f32).collect()))
        .collect::<Vec<_>>();
    if frames.is_empty() {
        return Err(format!("{} decoded no frames", path.display()));
    }
    Ok(frames)
}
