//! # Loudness Analysis
//!
//! True IT-R BS.1770 loudness analysis for audio tracks.
//!
//! This module wraps the `ebur128` crate to provide integrated loudness
//! (LUFS), true peak (dBTP), and loudness range (LU) measurements.
//!
//! ## Key Types
//! - `LoudnessAnalysis`: Results from analyzing a single audio track.
//! - `analyze_track`: Analyze interleaved stereo samples at a given sample rate.
//! - `analyze_track_mono`: Analyze mono samples (convenience wrapper).

use ebur128::{EbuR128, Mode};

/// Loudness analysis results for a single audio track.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoudnessAnalysis {
    /// Integrated loudness in LUFS. Lower (more negative) is quieter.
    pub integrated_lufs: f64,
    /// True peak in dBTP. 0 dBTP is digital full scale.
    pub true_peak_db: f64,
    /// Loudness range in LU. Larger values indicate more dynamic content.
    pub loudness_range_lu: f64,
    /// Number of channels in the analyzed audio.
    pub channels: u32,
    /// Sample rate used for the analysis.
    pub sample_rate: u32,
}

impl LoudnessAnalysis {
    /// Create an empty analysis result for silent/missing audio.
    pub fn silent() -> Self {
        Self {
            integrated_lufs: f64::NEG_INFINITY,
            true_peak_db: f64::NEG_INFINITY,
            loudness_range_lu: 0.0,
            channels: 0,
            sample_rate: 0,
        }
    }

    /// Returns true if the analysis contains valid loudness data.
    pub fn is_valid(&self) -> bool {
        self.channels > 0 && self.sample_rate > 0
    }
}

/// Analyze interleaved multi-channel audio samples using BS.1770.
///
/// # Arguments
/// * `samples` - Interleaved audio samples (e.g., L, R, L, R for stereo).
/// * `channels` - Number of channels.
/// * `sample_rate` - Sample rate in Hz.
///
/// # Returns
/// `LoudnessAnalysis` with integrated LUFS, true peak, and loudness range.
///
/// # Panics
/// Panics if `channels` is 0 or if `sample_rate` is 0.
pub fn analyze_track(samples: &[f32], channels: u32, sample_rate: u32) -> LoudnessAnalysis {
    assert!(channels > 0, "channels must be greater than 0");
    assert!(sample_rate > 0, "sample_rate must be greater than 0");

    if samples.is_empty() || (samples.len() % channels as usize) != 0 {
        return LoudnessAnalysis::silent();
    }

    let mode = Mode::I | Mode::TRUE_PEAK | Mode::LRA;
    let mut ebu = match EbuR128::new(channels, sample_rate, mode) {
        Ok(ebu) => ebu,
        Err(_) => return LoudnessAnalysis::silent(),
    };

    // ebur128 expects interleaved samples; the caller already provides them.
    if ebu.add_frames_f32(samples).is_err() {
        return LoudnessAnalysis::silent();
    }

    let integrated_lufs = ebu.loudness_global().unwrap_or(f64::NEG_INFINITY);
    let loudness_range_lu = ebu.loudness_range().unwrap_or(0.0);

    // True peak: report the maximum across all channels, converted to dBTP.
    let mut true_peak_db = f64::NEG_INFINITY;
    for ch in 0..channels {
        if let Ok(tp) = ebu.true_peak(ch) {
            let tp_db = 20.0 * tp.max(1e-10).log10();
            true_peak_db = true_peak_db.max(tp_db);
        }
    }

    LoudnessAnalysis {
        integrated_lufs,
        true_peak_db,
        loudness_range_lu,
        channels,
        sample_rate,
    }
}

/// Analyze interleaved stereo audio samples.
///
/// Convenience wrapper for `analyze_track` with `channels = 2`.
pub fn analyze_stereo(samples: &[f32], sample_rate: u32) -> LoudnessAnalysis {
    analyze_track(samples, 2, sample_rate)
}

/// Analyze mono audio samples.
///
/// Convenience wrapper for `analyze_track` with `channels = 1`.
pub fn analyze_mono(samples: &[f32], sample_rate: u32) -> LoudnessAnalysis {
    analyze_track(samples, 1, sample_rate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    /// Generate a 1 kHz sine wave at -20 dBFS.
    fn sine_wave(sample_rate: u32, duration_secs: f64, frequency: f32, dbfs: f32) -> Vec<f32> {
        let amplitude = 10.0_f32.powf(dbfs / 20.0);
        let num_samples = (sample_rate as f64 * duration_secs) as usize;
        (0..num_samples)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                (2.0 * PI * frequency * t).sin() * amplitude
            })
            .collect()
    }

    #[test]
    fn silent_track_returns_negative_infinity() {
        let samples = vec![0.0f32; 48000 * 2];
        let analysis = analyze_stereo(&samples, 48000);
        assert!(!analysis.is_valid() || analysis.integrated_lufs.is_infinite());
    }

    #[test]
    fn sine_wave_loudness_near_target() {
        let sample_rate = 48000;
        // Generate 3 seconds of stereo sine wave at -20 dBFS.
        let left = sine_wave(sample_rate, 3.0, 1000.0, -20.0);
        let right = sine_wave(sample_rate, 3.0, 1000.0, -20.0);
        let mut interleaved = Vec::with_capacity(left.len() * 2);
        for (l, r) in left.iter().zip(right.iter()) {
            interleaved.push(*l);
            interleaved.push(*r);
        }

        let analysis = analyze_stereo(&interleaved, sample_rate);
        assert!(analysis.is_valid());
        // A -20 dBFS sine wave should measure close to -20 LUFS after K-weighting.
        // K-weighting adds a small high-frequency boost, so expect slightly above -20 LUFS.
        assert!(analysis.integrated_lufs > -23.0 && analysis.integrated_lufs < -18.0,
            "expected integrated loudness near -20 LUFS, got {}", analysis.integrated_lufs);
    }

    #[test]
    fn true_peak_matches_known_amplitude() {
        // A -6 dBFS sine wave should report a true peak very close to -6 dBTP.
        let sample_rate = 48000;
        let amplitude = 10.0_f32.powf(-6.0 / 20.0);
        let left: Vec<f32> = (0..sample_rate * 3)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                (2.0 * PI * 1000.0 * t).sin() * amplitude
            })
            .collect();
        let mut interleaved = Vec::with_capacity(left.len() * 2);
        for s in &left {
            interleaved.push(*s);
            interleaved.push(*s);
        }

        let analysis = analyze_stereo(&interleaved, sample_rate);
        assert!(analysis.is_valid());
        assert!(
            (analysis.true_peak_db - -6.0).abs() < 0.5,
            "expected true peak near -6 dBTP, got {}",
            analysis.true_peak_db
        );
    }

    #[test]
    fn loudness_range_for_constant_tone_is_low() {
        let sample_rate = 48000;
        let left = sine_wave(sample_rate, 3.0, 1000.0, -20.0);
        let right = sine_wave(sample_rate, 3.0, 1000.0, -20.0);
        let mut interleaved = Vec::with_capacity(left.len() * 2);
        for (l, r) in left.iter().zip(right.iter()) {
            interleaved.push(*l);
            interleaved.push(*r);
        }

        let analysis = analyze_stereo(&interleaved, sample_rate);
        assert!(analysis.is_valid());
        // A constant tone should have very small loudness range.
        assert!(analysis.loudness_range_lu < 2.0,
            "expected small loudness range for constant tone, got {}", analysis.loudness_range_lu);
    }
}
