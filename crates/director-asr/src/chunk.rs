//! Long recordings as overlapping windows cut at quiet points, and the words of those windows
//! joined back into one list.

use crate::audio::SAMPLE_RATE;
use crate::Word;

/// 10 ms of 16 kHz audio.
const FRAME: usize = 160;
/// Frames on each side of the 210 ms mean that ranks cut points, so a pause between words
/// wins over a stop consonant inside one.
const SMOOTH: usize = 10;

/// How [`windows`] splits a recording.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowOptions {
    /// Longest window in seconds, overlap included.
    pub max_len: f64,
    /// Seconds two neighbouring windows share, split evenly around the cut.
    pub overlap: f64,
    /// Seconds before the latest possible cut searched for the quietest point.
    pub search: f64,
}

impl Default for WindowOptions {
    /// 60 s windows, the clip length `bark-rs` matches the official model on, sharing 4 s and
    /// cutting at the quietest point of the last 10 s.
    fn default() -> Self {
        Self { max_len: 60.0, overlap: 4.0, search: 10.0 }
    }
}

/// A stretch of 16 kHz samples to transcribe on its own. Words whose midpoint falls in
/// `keep_start..keep_end` come from this window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub start: usize,
    pub end: usize,
    pub keep_start: usize,
    pub keep_end: usize,
}

impl Window {
    /// Seconds of the recording where the window starts; add it to times the model reports.
    pub fn offset(&self) -> f64 {
        self.start as f64 / f64::from(SAMPLE_RATE)
    }
}

fn seconds_to_samples(seconds: f64) -> usize {
    (seconds.max(0.0) * f64::from(SAMPLE_RATE)).round() as usize
}

/// RMS of each 10 ms frame, the last one partial.
pub fn frame_rms(samples: &[f32]) -> Vec<f32> {
    samples.chunks(FRAME).map(|f| (f.iter().map(|s| s * s).sum::<f32>() / f.len() as f32).sqrt()).collect()
}

/// Splits 16 kHz samples into windows of at most `max_len` seconds. Each cut lands at the
/// quietest 210 ms of the `search` seconds before the latest cut that keeps the window in
/// bounds. Keep ranges tile the recording, and each window reaches half the overlap past them.
pub fn windows(samples: &[f32], options: &WindowOptions) -> Vec<Window> {
    let n = samples.len();
    let half = seconds_to_samples(options.overlap) / 2;
    let max = seconds_to_samples(options.max_len).max(2 * half + 2 * FRAME);
    let stride = max - 2 * half;
    let search = seconds_to_samples(options.search).min(stride / 2);
    if n <= max {
        return vec![Window { start: 0, end: n, keep_start: 0, keep_end: n }];
    }
    let rms = frame_rms(samples);
    let mut prefix = vec![0f64; rms.len() + 1];
    for (i, r) in rms.iter().enumerate() {
        prefix[i + 1] = prefix[i] + f64::from(*r);
    }
    let quiet = |f: usize| {
        let (lo, hi) = (f.saturating_sub(SMOOTH), (f + SMOOTH + 1).min(rms.len()));
        (prefix[hi] - prefix[lo]) / (hi - lo) as f64
    };
    let mut cuts = vec![0];
    loop {
        let from = *cuts.last().unwrap();
        if n <= from + max - if from == 0 { 0 } else { half } {
            break;
        }
        let latest = from + stride;
        let earliest = latest - search;
        let best = (earliest / FRAME..=latest / FRAME)
            .min_by(|a, b| quiet(*a).total_cmp(&quiet(*b)))
            .unwrap_or(latest / FRAME);
        cuts.push((best * FRAME + FRAME / 2).clamp(earliest, latest));
    }
    cuts.push(n);
    cuts.windows(2)
        .map(|c| Window { start: c[0].saturating_sub(half), end: (c[1] + half).min(n), keep_start: c[0], keep_end: c[1] })
        .collect()
}

/// Joins the words of each window, already in recording seconds, keeping each word from the
/// window whose keep range holds its midpoint. The last window also keeps words ending at
/// the recording's end.
pub fn stitch(parts: Vec<(Window, Vec<Word>)>) -> Vec<Word> {
    let count = parts.len();
    let rate = f64::from(SAMPLE_RATE);
    let mut out = Vec::new();
    for (i, (window, words)) in parts.into_iter().enumerate() {
        let (lo, hi) = (window.keep_start as f64 / rate, window.keep_end as f64 / rate);
        let last = i + 1 == count;
        out.extend(words.into_iter().filter(|w| {
            let mid = w.midpoint();
            mid >= lo && (mid < hi || (last && mid <= hi))
        }));
    }
    out
}
