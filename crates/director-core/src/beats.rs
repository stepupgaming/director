//! # Beats Module
//!
//! Beat grids: the tempo, beats, bars and drop of a music track, found ahead
//! of time so cuts can land on them.
//!
//! `detect` follows the dynamic programming beat tracker of Ellis (2007):
//! a spectral flux onset envelope, a tempo from its autocorrelation, then the
//! beat sequence that best balances onset strength against steady spacing.
//! Bars start on the beat phase that carries the most bass onsets, and the
//! drop is the bar with the largest rise in energy over the bars before it.

use realfft::RealFftPlanner;
use serde::{Deserialize, Serialize};

/// Analysis window and hop in samples.
const WINDOW: usize = 1024;
const HOP: usize = 256;
/// Upper edge of the kick band, Hz.
const BASS_HZ: f64 = 150.0;
/// How strongly beat spacing holds to the tempo period.
const TIGHTNESS: f64 = 100.0;
/// Gaussian smoothing of the onset envelope for the tempo search, frames.
const SMOOTH: f64 = 2.0;
/// Tempo prior: a log-normal around 120 BPM, one octave wide.
const PRIOR_BPM: f64 = 120.0;
/// A drop raises bar energy at least this many times over the bars before it.
const DROP_RISE: f64 = 2.0;

/// Beat positions count from the first downbeat: position 0 is the first
/// downbeat, bar `n` starts at position `n * beats_per_bar`, and beats
/// before the first downbeat have negative positions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BeatGrid {
    pub bpm: f64,
    pub beats_per_bar: u32,
    /// Beat times in seconds, increasing, at least two.
    pub beats: Vec<f64>,
    /// Index in `beats` of position 0.
    pub origin: usize,
    /// Position of the drop, a whole bar.
    pub drop: Option<f64>,
}

/// What `detect` assumes about the music.
#[derive(Debug, Clone, PartialEq)]
pub struct BeatOptions {
    pub beats_per_bar: u32,
    /// A known tempo. Unset, the tempo is searched within `bpm_range`, and
    /// half or double the musical tempo is a valid reading.
    pub bpm: Option<f64>,
    pub bpm_range: (f64, f64),
}

impl Default for BeatOptions {
    fn default() -> Self {
        Self {
            beats_per_bar: 4,
            bpm: None,
            bpm_range: (70.0, 185.0),
        }
    }
}

impl BeatGrid {
    /// A constant tempo with its first downbeat at `offset` seconds.
    pub fn fixed(bpm: f64, offset: f64, beats_per_bar: u32) -> Result<Self, String> {
        if !(bpm.is_finite() && bpm > 0.0) {
            return Err(format!("bpm must be finite and positive (got {bpm})"));
        }
        if !offset.is_finite() || beats_per_bar == 0 {
            return Err("beat offset must be finite and beats per bar positive".into());
        }
        Ok(Self {
            bpm,
            beats_per_bar,
            beats: vec![offset, offset + 60.0 / bpm],
            origin: 0,
            drop: None,
        })
    }

    /// Seconds at beat `position`, interpolating between beats and
    /// continuing the edge intervals past either end.
    pub fn time(&self, position: f64) -> f64 {
        let b = &self.beats;
        let index = position + self.origin as f64;
        let k = (index.floor().max(0.0) as usize).min(b.len() - 2);
        b[k] + (b[k + 1] - b[k]) * (index - k as f64)
    }

    /// Beat position at `time` seconds, the inverse of `time`.
    pub fn position(&self, time: f64) -> f64 {
        let b = &self.beats;
        let k = b.partition_point(|&t| t <= time).saturating_sub(1).min(b.len() - 2);
        k as f64 + (time - b[k]) / (b[k + 1] - b[k]) - self.origin as f64
    }

    /// Times of the detected beats that start a bar.
    pub fn downbeats(&self) -> Vec<f64> {
        let n = self.beats_per_bar as usize;
        self.beats
            .iter()
            .enumerate()
            .filter(|(i, _)| i % n == self.origin % n)
            .map(|(_, t)| *t)
            .collect()
    }
}

/// Per-frame features of the analysis: onset flux, bass onset flux and power.
struct Features {
    flux: Vec<f64>,
    bass: Vec<f64>,
    power: Vec<f64>,
    fps: f64,
}

fn features(mono: &[f32], sample_rate: u32) -> Features {
    let frames = mono.len() / HOP + 1;
    let fft = RealFftPlanner::<f64>::new().plan_fft_forward(WINDOW);
    let window: Vec<f64> = (0..WINDOW)
        .map(|i| 0.5 * (1.0 - (2.0 * std::f64::consts::PI * i as f64 / WINDOW as f64).cos()))
        .collect();
    let bass_bins = (BASS_HZ * WINDOW as f64 / sample_rate as f64) as usize + 1;
    let mut input = fft.make_input_vec();
    let mut output = fft.make_output_vec();
    let mut previous = vec![0.0; WINDOW / 2 + 1];
    let mut current = vec![0.0; WINDOW / 2 + 1];
    let (mut flux, mut bass, mut power) = (vec![0.0; frames], vec![0.0; frames], vec![0.0; frames]);
    for f in 0..frames {
        // Frames centre on `f * HOP`, zero padded past either end.
        let start = (f * HOP) as isize - (WINDOW / 2) as isize;
        for (i, x) in input.iter_mut().enumerate() {
            let at = start + i as isize;
            *x = if at >= 0 && (at as usize) < mono.len() {
                mono[at as usize] as f64 * window[i]
            } else {
                0.0
            };
        }
        fft.process(&mut input, &mut output).expect("FFT sizes match the plan");
        let (mut sum, mut low, mut energy) = (0.0, 0.0, 0.0);
        for (k, bin) in output.iter().enumerate() {
            let magnitude = bin.norm() / (WINDOW as f64 / 4.0);
            energy += magnitude * magnitude;
            current[k] = (100.0 * magnitude).ln_1p();
            let rise = (current[k] - previous[k]).max(0.0);
            sum += rise;
            if (1..bass_bins).contains(&k) {
                low += rise;
            }
        }
        flux[f] = sum / output.len() as f64;
        bass[f] = low;
        power[f] = energy;
        std::mem::swap(&mut previous, &mut current);
    }
    Features {
        flux,
        bass,
        power,
        fps: sample_rate as f64 / HOP as f64,
    }
}

/// The onset envelope: flux above its half-second local mean, in units of
/// its standard deviation.
fn onset_envelope(flux: &[f64], fps: f64) -> Option<Vec<f64>> {
    let k = (fps * 0.5) as usize;
    let mut prefix = vec![0.0; flux.len() + 1];
    for (i, v) in flux.iter().enumerate() {
        prefix[i + 1] = prefix[i] + v;
    }
    let onset: Vec<f64> = (0..flux.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(k), (i + k + 1).min(flux.len()));
            // The window counts the zero padding past either end, as a centred convolution does.
            (flux[i] - (prefix[b] - prefix[a]) / (2 * k + 1) as f64).max(0.0)
        })
        .collect();
    let mean = onset.iter().sum::<f64>() / onset.len() as f64;
    let deviation = (onset.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / onset.len() as f64).sqrt();
    (deviation > 1e-12).then(|| onset.iter().map(|v| v / deviation).collect())
}

/// The tempo period in frames: the autocorrelation lag with the best
/// harmonic sum under the tempo prior.
fn tempo_period(onset: &[f64], fps: f64, (low_bpm, high_bpm): (f64, f64)) -> Option<f64> {
    let radius = (3.0 * SMOOTH) as isize;
    let kernel: Vec<f64> = (-radius..=radius).map(|i| (-0.5 * (i as f64 / SMOOTH).powi(2)).exp()).collect();
    let total: f64 = kernel.iter().sum();
    let n = onset.len();
    let smooth: Vec<f64> = (0..n as isize)
        .map(|i| {
            kernel
                .iter()
                .enumerate()
                .filter_map(|(j, w)| onset.get((i + j as isize - radius).max(-1) as usize).map(|v| v * w))
                .sum::<f64>()
                / total
        })
        .collect();
    let shortest = (60.0 * fps / high_bpm).ceil() as usize;
    let longest = (60.0 * fps / low_bpm).floor() as usize;
    if shortest < 2 || longest <= shortest || 4 * longest + 1 >= n {
        return None;
    }
    let correlation: Vec<f64> = (0..=4 * longest + 1)
        .map(|lag| smooth[..n - lag].iter().zip(&smooth[lag..]).map(|(a, b)| a * b).sum::<f64>() / (n - lag) as f64)
        .collect();
    let peak = |lag: usize| correlation[lag - 1].max(correlation[lag]).max(correlation[lag + 1]);
    let scores: Vec<f64> = (shortest..=longest)
        .map(|lag| {
            let bpm = 60.0 * fps / lag as f64;
            let prior = (-0.5 * (bpm / PRIOR_BPM).log2().powi(2)).exp();
            (peak(lag) + 0.5 * peak(2 * lag) + 0.25 * peak(4 * lag)) * prior
        })
        .collect();
    let best = (0..scores.len()).max_by(|a, b| scores[*a].total_cmp(&scores[*b]))?;
    let mut period = (shortest + best) as f64;
    if best > 0 && best + 1 < scores.len() {
        let (a, b, c) = (scores[best - 1], scores[best], scores[best + 1]);
        let curve = a - 2.0 * b + c;
        if curve < 0.0 {
            period += 0.5 * (a - c) / curve;
        }
    }
    Some(period)
}

/// The beat frames: Ellis's dynamic program over the onset envelope. A beat
/// starts a new chain when no predecessor adds to its score, so silence
/// before the music leaves no beats behind it.
fn track(onset: &[f64], period: f64) -> Vec<usize> {
    let n = onset.len();
    let mut score = vec![0.0; n];
    let mut back = vec![usize::MAX; n];
    for t in 0..n {
        let first = (t as f64 - 2.0 * period).round().max(0.0) as usize;
        let last = ((t as f64 - period / 2.0).round().max(0.0) as usize).min(t.saturating_sub(1));
        let mut best: Option<(f64, usize)> = None;
        if t > 0 {
            for (tau, prior) in score.iter().enumerate().take(last + 1).skip(first) {
                let gap = ((t - tau) as f64 / period).ln();
                let value = prior - TIGHTNESS * gap * gap;
                if best.is_none_or(|(b, _)| value > b) {
                    best = Some((value, tau));
                }
            }
        }
        score[t] = onset[t];
        if let Some((value, tau)) = best.filter(|(value, _)| *value > 0.0) {
            score[t] += value;
            back[t] = tau;
        }
    }
    let tail = (2.0 * period).ceil() as usize;
    let mut beat = (n.saturating_sub(tail)..n).max_by(|a, b| score[*a].total_cmp(&score[*b])).unwrap_or(0);
    let mut beats = vec![beat];
    while back[beat] != usize::MAX {
        beat = back[beat];
        beats.push(beat);
    }
    beats.reverse();
    beats
}

/// Find the beat grid of `samples`, interleaved over `channels`.
pub fn detect(samples: &[f32], channels: usize, sample_rate: u32, options: &BeatOptions) -> Result<BeatGrid, String> {
    let (low, high) = options.bpm_range;
    if channels == 0 || sample_rate == 0 || options.beats_per_bar == 0 {
        return Err("beat detection needs channels, a sample rate and beats per bar".into());
    }
    if !(low.is_finite() && high.is_finite() && 0.0 < low && low < high) {
        return Err(format!("invalid bpm range {low}..{high}"));
    }
    if options.bpm.is_some_and(|bpm| !(bpm.is_finite() && bpm > 0.0)) {
        return Err("bpm must be finite and positive".into());
    }
    let mono: Vec<f32> = samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();
    let features = features(&mono, sample_rate);
    let fps = features.fps;
    let onset = onset_envelope(&features.flux, fps).ok_or("the music has no onsets to find beats in")?;
    let period = match options.bpm {
        Some(bpm) => 60.0 * fps / bpm,
        None => tempo_period(&onset, fps, options.bpm_range)
            .ok_or("the music is too short to find its tempo")?,
    };
    let mut frames = track(&onset, period);
    // Trim beats at either end whose onsets are much weaker than the music's.
    let strength = |b: usize| onset[b.saturating_sub(2)..(b + 3).min(onset.len())].iter().copied().fold(0.0, f64::max);
    let strengths: Vec<f64> = frames.iter().map(|b| strength(*b)).collect();
    let mut sorted = strengths.clone();
    sorted.sort_by(f64::total_cmp);
    let floor = 0.2 * sorted[sorted.len() / 2];
    let keep_from = strengths.iter().position(|s| *s >= floor).unwrap_or(0);
    let keep_to = strengths.iter().rposition(|s| *s >= floor).map_or(0, |i| i + 1);
    frames = frames[keep_from..keep_to].to_vec();
    if frames.len() < 2 {
        return Err("the music has fewer than two beats".into());
    }
    // Place each beat on its onset peak, interpolated between frames.
    let beats: Vec<f64> = frames
        .iter()
        .map(|&b| {
            let (lo, hi) = (b.saturating_sub(2).max(1), (b + 2).min(onset.len() - 2));
            if lo > hi {
                return b as f64 / fps;
            }
            let m = (lo..=hi).max_by(|x, y| onset[*x].total_cmp(&onset[*y])).unwrap_or(b);
            let (a, c, d) = (onset[m - 1], onset[m], onset[m + 1]);
            let curve = a - 2.0 * c + d;
            let shift = if curve < 0.0 { (0.5 * (a - d) / curve).clamp(-0.5, 0.5) } else { 0.0 };
            (m as f64 + shift) / fps
        })
        .collect();
    let n = options.beats_per_bar as usize;
    // Bars start on the beat phase with the most bass onsets.
    let bass_at = |b: usize| features.bass[b.saturating_sub(1)..(b + 3).min(features.bass.len())].iter().sum::<f64>();
    let origin = (0..n.min(frames.len()))
        .max_by(|a, b| {
            let sum = |p: usize| frames.iter().skip(p).step_by(n).map(|f| bass_at(*f)).sum::<f64>();
            sum(*a).total_cmp(&sum(*b))
        })
        .unwrap_or(0);
    // Mean power per whole bar, then the bar with the largest rise.
    let bars: Vec<f64> = (origin..)
        .step_by(n)
        .take_while(|i| i + n < frames.len())
        .map(|i| {
            let span = &features.power[frames[i]..frames[i + n]];
            span.iter().sum::<f64>() / span.len().max(1) as f64
        })
        .collect();
    let mut drop = None;
    let mut best = DROP_RISE;
    for k in 1..bars.len() {
        let after = bars[k..(k + 2).min(bars.len())].iter().sum::<f64>() / (k + 2).min(bars.len()).saturating_sub(k) as f64;
        let before = bars[k.saturating_sub(2)..k].iter().sum::<f64>() / (k - k.saturating_sub(2)) as f64;
        let rise = after / before.max(1e-12);
        if rise >= best {
            best = rise;
            drop = Some((k * n) as f64);
        }
    }
    // The reported tempo is the least-squares slope of the beat times.
    let count = beats.len() as f64;
    let mean_i = (count - 1.0) / 2.0;
    let mean_t = beats.iter().sum::<f64>() / count;
    let (mut covariance, mut variance) = (0.0, 0.0);
    for (i, t) in beats.iter().enumerate() {
        covariance += (i as f64 - mean_i) * (t - mean_t);
        variance += (i as f64 - mean_i).powi(2);
    }
    Ok(BeatGrid {
        bpm: 60.0 * variance / covariance,
        beats_per_bar: options.beats_per_bar,
        beats,
        origin,
        drop,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48000;

    /// A drum loop: a kick on beats 1 and 3 (accented on the downbeat), a
    /// snare on 2 and 4 and hats on eighths, quiet until bar `drop`. Returns
    /// the mono samples and the true beat times.
    fn drums(bpm: f64, first: f64, bars: usize, drop: usize) -> (Vec<f32>, Vec<f64>) {
        let period = 60.0 / bpm;
        let total = first + bars as f64 * 4.0 * period + 1.0;
        let mut samples = vec![0.0f32; (total * RATE as f64) as usize];
        let mut seed = 1u32;
        let mut noise = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0
        };
        let mut truth = Vec::new();
        for i in 0..bars * 4 {
            let t = first + i as f64 * period;
            truth.push(t);
            let loud = if i >= drop * 4 { 1.0 } else { 0.25 };
            let at = (t * RATE as f64) as usize;
            let mut add = |offset: usize, len: f64, f: &mut dyn FnMut(f32) -> f32| {
                for k in 0..(len * RATE as f64) as usize {
                    if let Some(s) = samples.get_mut(at + offset + k) {
                        *s += loud * f(k as f32 / RATE as f32);
                    }
                }
            };
            if i % 2 == 0 {
                let accent = if i % 4 == 0 { 1.0 } else { 0.6 };
                add(0, 0.15, &mut |s| accent * (2.0 * std::f32::consts::PI * 55.0 * s).sin() * (-s * 25.0).exp());
            } else {
                add(0, 0.1, &mut |s| 0.5 * noise() * (-s * 40.0).exp());
            }
            for half in [0, 1] {
                let offset = (half as f64 * period / 2.0 * RATE as f64) as usize;
                add(offset, 0.03, &mut |s| 0.15 * noise() * (-s * 150.0).exp());
            }
        }
        (samples, truth)
    }

    fn check(bpm: f64, first: f64, options: BeatOptions) {
        let (samples, truth) = drums(bpm, first, 16, 8);
        let grid = detect(&samples, 1, RATE, &options).unwrap();
        assert!((grid.bpm - bpm).abs() < 0.2, "{bpm} BPM read as {}", grid.bpm);
        assert_eq!(grid.beats.len(), truth.len());
        for (found, real) in grid.beats.iter().zip(&truth) {
            assert!((found - real).abs() < 0.012, "beat at {real} found at {found}");
        }
        assert!((grid.time(0.0) - first).abs() < 0.012, "first downbeat at {}", grid.time(0.0));
        assert_eq!(grid.drop, Some(32.0));
        assert!((grid.time(32.0) - truth[32]).abs() < 0.012);
    }

    #[test]
    fn detects_tempo_beats_bars_and_the_drop() {
        check(120.0, 0.0, BeatOptions::default());
        check(128.0, 0.6, BeatOptions::default());
        check(90.0, 0.1, BeatOptions::default());
    }

    #[test]
    fn a_known_tempo_settles_the_octave() {
        // Alone, this loop reads as 87 BPM, its half tempo.
        let (samples, _) = drums(174.0, 0.25, 16, 8);
        let free = detect(&samples, 1, RATE, &BeatOptions::default()).unwrap();
        assert!((free.bpm - 87.0).abs() < 0.2, "{}", free.bpm);
        check(174.0, 0.25, BeatOptions { bpm: Some(174.0), ..Default::default() });
    }

    #[test]
    fn grids_map_positions_and_times_both_ways() {
        let grid = BeatGrid::fixed(120.0, 0.5, 4).unwrap();
        assert_eq!(grid.time(8.0), 4.5);
        assert_eq!(grid.time(-1.0), 0.0);
        assert_eq!(grid.position(4.5), 8.0);
        let detected = BeatGrid { bpm: 100.0, beats_per_bar: 2, beats: vec![0.1, 0.6, 1.2, 1.7], origin: 1, drop: None };
        assert!((detected.time(0.5) - 0.9).abs() < 1e-12);
        assert!((detected.time(3.0) - 2.2).abs() < 1e-12);
        assert!((detected.position(0.9) - 0.5).abs() < 1e-12);
        assert_eq!(detected.downbeats(), vec![0.6, 1.7]);
        assert!(BeatGrid::fixed(0.0, 0.0, 4).is_err());
    }

    #[test]
    fn silence_has_no_beats() {
        let error = detect(&vec![0.0; RATE as usize * 4], 1, RATE, &BeatOptions::default()).unwrap_err();
        assert!(error.contains("no onsets"), "{error}");
    }
}
