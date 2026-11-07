//! Per-frame signals on 96 px thumbnails: HSV histogram distance, gray
//! difference, the changed fraction and mean luma. Ported from rea-video
//! `shots.py` `build_track` (Apache-2.0, see NOTICE). Gray and HSV follow
//! OpenCV's 8-bit fixed-point conversions so the numbers match the reference.

use std::path::Path;

use crate::media::{for_each_frame, frame_times, MediaError, VideoInfo};

/// Thumbnail width in pixels.
pub const THUMB_W: u32 = 96;

const HIST_BINS: usize = 16 * 4 * 4;

/// Signals for every frame of a video.
#[derive(Debug, Clone, Default)]
pub struct FrameTrack {
    pub fps: f64,
    pub width: usize,
    pub height: usize,
    /// Bhattacharyya distance to the previous frame's histogram, 0 to 1.
    pub hist_dist: Vec<f64>,
    /// Mean absolute gray difference to the previous frame, 0 to 1.
    pub pix_diff: Vec<f64>,
    /// Fraction of thumbnail pixels that changed noticeably.
    pub changed: Vec<f64>,
    /// Mean luma, 0 to 255.
    pub luma: Vec<f64>,
    /// Gray thumbnails.
    pub thumbs: Vec<Vec<u8>>,
    /// Standard deviation of each gray thumbnail.
    pub thumb_std: Vec<f64>,
    /// Per-frame pts in seconds for variable frame rate video, else `None` (frame / fps).
    pub times: Option<Vec<f64>>,
}

impl FrameTrack {
    pub fn n(&self) -> usize {
        self.luma.len()
    }

    /// Decodes the video at `THUMB_W` and builds its track.
    pub fn read(info: &VideoInfo) -> Result<Self, MediaError> {
        let (w, h) = info.scaled_size(THUMB_W);
        let fps = if info.fps > 0.0 { info.fps } else { 30.0 };
        let mut builder = TrackBuilder::new(fps, w as usize, h as usize);
        for_each_frame(info, THUMB_W, |rgb| builder.push(rgb))?;
        let mut track = builder.finish();
        track.times = frame_times(Path::new(&info.path), track.n(), fps);
        Ok(track)
    }
}

/// OpenCV's division tables for 8-bit RGB to HSV with a hue range of 180.
struct HsvTables {
    sdiv: [i32; 256],
    hdiv: [i32; 256],
}

impl HsvTables {
    fn new() -> Self {
        let mut sdiv = [0; 256];
        let mut hdiv = [0; 256];
        for i in 1..256 {
            sdiv[i] = ((255 << 12) as f64 / i as f64).round() as i32;
            hdiv[i] = ((180 << 12) as f64 / (6.0 * i as f64)).round() as i32;
        }
        Self { sdiv, hdiv }
    }

    /// The [16, 4, 4] HSV histogram bin of one pixel.
    fn bin(&self, r: i32, g: i32, b: i32) -> usize {
        let v = r.max(g).max(b);
        let diff = v - r.min(g).min(b);
        let s = (diff * self.sdiv[v as usize] + (1 << 11)) >> 12;
        let h = if v == r {
            g - b
        } else if v == g {
            b - r + 2 * diff
        } else {
            r - g + 4 * diff
        };
        let mut h = (h * self.hdiv[diff as usize] + (1 << 11)) >> 12;
        if h < 0 {
            h += 180;
        }
        let hb = ((h.clamp(0, 179) * 16) / 180) as usize;
        hb * 16 + (s as usize >> 6) * 4 + (v as usize >> 6)
    }
}

/// OpenCV `COLOR_RGB2GRAY` on 8-bit input.
fn gray(r: u8, g: u8, b: u8) -> u8 {
    ((r as u32 * 4899 + g as u32 * 9617 + b as u32 * 1868 + 8192) >> 14) as u8
}

fn mean_std(values: &[u8]) -> (f64, f64) {
    let n = values.len().max(1) as f64;
    let mean = values.iter().map(|&v| v as f64).sum::<f64>() / n;
    let var = values.iter().map(|&v| (v as f64 - mean).powi(2)).sum::<f64>() / n;
    (mean, var.sqrt())
}

/// Builds a `FrameTrack` one RGB frame at a time.
pub struct TrackBuilder {
    track: FrameTrack,
    tables: HsvTables,
    prev_hist: Option<[f32; HIST_BINS]>,
}

impl TrackBuilder {
    pub fn new(fps: f64, width: usize, height: usize) -> Self {
        Self { track: FrameTrack { fps, width, height, ..Default::default() }, tables: HsvTables::new(), prev_hist: None }
    }

    /// Adds one packed RGB frame of the builder's size.
    pub fn push(&mut self, rgb: &[u8]) {
        let pixels = self.track.width * self.track.height;
        let mut g = Vec::with_capacity(pixels);
        let mut hist = [0f32; HIST_BINS];
        for &[r, g8, b] in rgb.as_chunks::<3>().0.iter().take(pixels) {
            g.push(gray(r, g8, b));
            hist[self.tables.bin(r as i32, g8 as i32, b as i32)] += 1.0;
        }
        let total: f32 = hist.iter().sum();
        if total > 0.0 {
            hist.iter_mut().for_each(|h| *h /= total);
        }
        let (mean, std) = mean_std(&g);
        let t = &mut self.track;
        match (&self.prev_hist, t.thumbs.last()) {
            (Some(prev_hist), Some(prev)) => {
                t.hist_dist.push(bhattacharyya(prev_hist, &hist));
                let prev_std = *t.thumb_std.last().unwrap();
                let thr = 6f64.max(0.35 * std.min(prev_std));
                let (mut sum, mut over) = (0f64, 0usize);
                for (&a, &b) in g.iter().zip(prev) {
                    let d = (a as f64 - b as f64).abs();
                    sum += d;
                    over += (d > thr) as usize;
                }
                t.pix_diff.push(sum / g.len() as f64 / 255.0);
                t.changed.push(over as f64 / g.len() as f64);
            }
            _ => {
                t.hist_dist.push(0.0);
                t.pix_diff.push(0.0);
                t.changed.push(0.0);
            }
        }
        t.luma.push(mean);
        t.thumb_std.push(std);
        t.thumbs.push(g);
        self.prev_hist = Some(hist);
    }

    pub fn finish(self) -> FrameTrack {
        self.track
    }
}

/// OpenCV `HISTCMP_BHATTACHARYYA`.
fn bhattacharyya(a: &[f32], b: &[f32]) -> f64 {
    let (s1, s2): (f64, f64) = (a.iter().map(|&v| v as f64).sum(), b.iter().map(|&v| v as f64).sum());
    let sum: f64 = a.iter().zip(b).map(|(&x, &y)| (x as f64 * y as f64).sqrt()).sum();
    let scale = if s1 * s2 > f32::EPSILON as f64 { 1.0 / (s1 * s2).sqrt() } else { 1.0 };
    (1.0 - sum * scale).max(0.0).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gray_matches_opencv_weights() {
        assert_eq!(gray(255, 255, 255), 255);
        assert_eq!(gray(255, 0, 0), 76);
        assert_eq!(gray(0, 255, 0), 150);
        assert_eq!(gray(0, 0, 255), 29);
    }

    #[test]
    fn hue_bins_cover_primaries() {
        let t = HsvTables::new();
        // Red hue 0, green 60, blue 120 in OpenCV's 0-180 range; full saturation and value.
        assert_eq!(t.bin(255, 0, 0) / 16, 0);
        assert_eq!(t.bin(0, 255, 0) / 16, 5);
        assert_eq!(t.bin(0, 0, 255) / 16, 10);
        assert_eq!(t.bin(0, 0, 255) % 16, 15);
    }

    #[test]
    fn identical_histograms_are_zero_apart() {
        let mut h = [0f32; HIST_BINS];
        h[3] = 0.5;
        h[7] = 0.5;
        assert!(bhattacharyya(&h, &h) < 1e-6);
        let mut other = [0f32; HIST_BINS];
        other[9] = 1.0;
        assert!((bhattacharyya(&h, &other) - 1.0).abs() < 1e-6);
    }
}
