//! # Audio Module
//!
//! Audio mixing, playback, and spectrum analysis.
//!
//!
//! ## Responsibilities
//! - **Audio Mixing**: Combines multiple `AudioTrack`s into final output.
//! - **Scene Graph Audio**: Collects audio from scene nodes (`mix_scene_audio`).
//! - **Sync**: Aligns audio with video timeline.
//! - **Track Management**: Add/remove/seek audio tracks.
//! - **FFT Analysis**: Compute frequency spectrum for audio-reactive visuals.
//!
//! ## Key Types
//! - `AudioMixer`: The main audio processor.
//! - `AudioTrack`: A single audio source with volume, pan, effects, and timing.
//! - `AudioAnalyzer`: FFT-based spectrum analyzer for beat detection.

use crate::animation::Animated;
use crate::audio_effects::{AudioEffectChain, EffectParamAnimation, pan_gains};
use anyhow::{Context, Result};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use std::io::Cursor;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Represents a single audio source on the timeline.
#[derive(Clone, Debug)]
pub struct AudioTrack {
    /// Interleaved stereo samples (L, R, L, R...). Normalized -1.0 to 1.0.
    pub samples: Vec<f32>,
    /// Volume multiplier (animated).
    pub volume: Animated<f32>,
    /// Stereo pan 0..1 (0=full left, 0.5=center, 1=full right). Animated.
    pub pan: Animated<f32>,
    /// Chain of audio effects (reverb, delay, EQ, compressor).
    pub effects: AudioEffectChain,
    /// Effect parameter animations applied per-frame during mix.
    pub effect_param_animations: Vec<EffectParamAnimation>,
    /// Start time in global seconds.
    pub start_time: f64,
    /// Optional clipping duration (in seconds).
    pub duration: Option<f64>,
    /// Whether to loop the audio.
    pub loop_audio: bool,
}

/// Manages mixing of multiple audio tracks.
#[derive(Clone, Debug)]
pub struct AudioMixer {
    /// List of active tracks. Option allows for empty slots (freelist style).
    pub tracks: Vec<Option<AudioTrack>>,
    /// Output sample rate (usually 48000Hz).
    pub sample_rate: u32,
}

impl AudioMixer {
    /// Creates a new mixer with the specified sample rate.
    pub fn new(sample_rate: u32) -> Self {
        Self {
            tracks: Vec::new(),
            sample_rate,
        }
    }

    /// Adds a track to the mixer.
    pub fn add_track(&mut self, track: AudioTrack) -> usize {
        if let Some(idx) = self.tracks.iter().position(|t| t.is_none()) {
            self.tracks[idx] = Some(track);
            idx
        } else {
            let idx = self.tracks.len();
            self.tracks.push(Some(track));
            idx
        }
    }

    /// Returns a mutable reference to a track.
    pub fn get_track_mut(&mut self, id: usize) -> Option<&mut AudioTrack> {
        self.tracks.get_mut(id).and_then(|t| t.as_mut())
    }

    /// Mixes all active tracks for a specific time window.
    ///
    /// # Arguments
    /// * `samples_needed` - Number of samples to generate (per channel).
    /// * `start_time` - Global start time for the mix window.
    ///
    /// # Returns
    /// * `Vec<f32>` - Interleaved stereo samples (length = samples_needed * 2).
    pub fn mix(&mut self, samples_needed: usize, start_time: f64) -> Vec<f32> {
        let mut output = vec![0.0; samples_needed * 2];
        let dt_per_sample = 1.0 / self.sample_rate as f64;

        for track_opt in self.tracks.iter_mut() {
            if let Some(track) = track_opt {
                track.volume.update(start_time);
                track.pan.update(start_time);
                let vol = track.volume.current_value;
                let (left_gain, right_gain) = pan_gains(track.pan.current_value);

                // Apply effect parameter animations
                for anim in &mut track.effect_param_animations {
                    anim.animated.update(start_time);
                    if let Some(effect) = track.effects.get_mut(anim.effect_idx) {
                        effect.set_parameter(&anim.param_name, anim.animated.current_value);
                    }
                }

                // Build per-track buffer
                let mut track_buf = vec![0.0; samples_needed * 2];

                for i in 0..samples_needed {
                    let t = start_time + i as f64 * dt_per_sample;
                    let mut relative_time = t - track.start_time;

                    if relative_time < 0.0 {
                        continue;
                    }

                    if let Some(dur) = track.duration {
                        if relative_time >= dur {
                            if track.loop_audio {
                                relative_time %= dur;
                            } else {
                                continue;
                            }
                        }
                    }

                    let mut sample_idx = (relative_time * self.sample_rate as f64) as usize;
                    let frame_count = track.samples.len() / 2;

                    if track.loop_audio {
                        sample_idx %= frame_count;
                    } else if sample_idx >= frame_count {
                        continue;
                    }

                    let left = track.samples[sample_idx * 2];
                    let right = track.samples[sample_idx * 2 + 1];

                    track_buf[i * 2] = left * vol * left_gain;
                    track_buf[i * 2 + 1] = right * vol * right_gain;
                }

                // Process effects chain
                track
                    .effects
                    .process(&mut track_buf, self.sample_rate, 2);

                // Mix into output
                for (out, val) in output.iter_mut().zip(track_buf.iter()) {
                    *out += val;
                }
            }
        }

        for s in output.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }

        output
    }
}

/// Mixes audio from scene graph nodes into an existing output buffer.
///
/// This traverses active scenes and collects audio from any nodes that provide it
/// (e.g., VideoNode, AudioNode). The collected audio is added to the provided buffer.
///
/// # Arguments
/// * `output` - Mutable buffer to mix audio into (interleaved stereo)
/// * `scene_nodes` - Reference to the scene graph's node storage
/// * `active_roots` - List of (root_node_id, local_time) for active scenes
/// * `samples_needed` - Number of samples per channel
/// * `sample_rate` - Audio sample rate
pub fn mix_scene_audio(
    output: &mut [f32],
    scene_nodes: &[Option<crate::scene::SceneNode>],
    active_roots: &[(crate::types::NodeId, f64)],
    samples_needed: usize,
    sample_rate: u32,
) {
    let mut stack: Vec<(crate::types::NodeId, f64)> = active_roots.to_vec();

    while let Some((id, local_time)) = stack.pop() {
        if id < scene_nodes.len() {
            if let Some(node) = &scene_nodes[id] {
                if let Some(samples) =
                    node.element
                        .get_audio(local_time, samples_needed, sample_rate)
                {
                    for (i, val) in samples.iter().enumerate() {
                        if i < output.len() {
                            output[i] += val;
                        }
                    }
                }

                for child_id in &node.children {
                    stack.push((*child_id, local_time));
                }
            }
        }
    }

    for s in output.iter_mut() {
        *s = s.clamp(-1.0, 1.0);
    }
}

// ============================================================================
// Audio Analyzer (FFT-based spectrum analysis)
// ============================================================================

use realfft::{RealFftPlanner, RealToComplex};
use std::f32::consts::PI;
use std::sync::Arc;

/// FFT-based audio analyzer for spectrum and beat detection.
///
/// Provides frequency spectrum analysis for audio-reactive visuals.
/// Default FFT size is 2048 samples (~43ms at 48kHz).
#[derive(Clone)]
pub struct AudioAnalyzer {
    /// The FFT algorithm instance
    fft: Arc<dyn RealToComplex<f32>>,
    /// FFT size (number of samples per analysis window)
    pub fft_size: usize,
    /// Sample rate for frequency bin calculations
    pub sample_rate: u32,
    /// Pre-computed Hann window coefficients
    window: Vec<f32>,
}

impl std::fmt::Debug for AudioAnalyzer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioAnalyzer")
            .field("fft_size", &self.fft_size)
            .field("sample_rate", &self.sample_rate)
            .finish()
    }
}

impl AudioAnalyzer {
    /// Creates a new AudioAnalyzer with the specified FFT size and sample rate.
    ///
    /// # Arguments
    /// * `fft_size` - Number of samples per FFT window (power of 2, e.g., 1024, 2048, 4096)
    /// * `sample_rate` - Audio sample rate in Hz (e.g., 48000)
    pub fn new(fft_size: usize, sample_rate: u32) -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(fft_size);

        let window: Vec<f32> = (0..fft_size)
            .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / (fft_size - 1) as f32).cos()))
            .collect();

        Self {
            fft,
            fft_size,
            sample_rate,
            window,
        }
    }

    /// Computes the frequency spectrum at a given time offset.
    ///
    /// # Arguments
    /// * `samples` - Interleaved stereo audio samples
    /// * `time` - Time offset in seconds to analyze
    ///
    /// # Returns
    /// Normalized magnitude values for frequency bins (0 to fft_size/2).
    /// Each bin represents a frequency range of sample_rate/fft_size Hz.
    pub fn compute_spectrum(&self, samples: &[f32], time: f64) -> Vec<f32> {
        let frame_count = samples.len() / 2;
        if frame_count == 0 {
            return vec![0.0; self.fft_size / 2 + 1];
        }

        let sample_offset = (time * self.sample_rate as f64) as usize;
        if sample_offset >= frame_count {
            return vec![0.0; self.fft_size / 2 + 1];
        }

        let mut input: Vec<f32> = (0..self.fft_size)
            .map(|i| {
                let idx = sample_offset + i;
                if idx < frame_count {
                    let left = samples[idx * 2];
                    let right = samples[idx * 2 + 1];
                    (left + right) * 0.5 * self.window[i]
                } else {
                    0.0
                }
            })
            .collect();

        let mut spectrum = self.fft.make_output_vec();

        if self.fft.process(&mut input, &mut spectrum).is_err() {
            return vec![0.0; self.fft_size / 2 + 1];
        }

        let scale = 2.0 / self.fft_size as f32;
        spectrum
            .iter()
            .map(|c| (c.re * c.re + c.im * c.im).sqrt() * scale)
            .collect()
    }

    /// Gets the energy level in a specific frequency band.
    ///
    /// # Arguments
    /// * `samples` - Interleaved stereo audio samples
    /// * `time` - Time offset in seconds
    /// * `band` - Frequency band: "bass", "mids", or "highs"
    ///
    /// # Returns
    /// Normalized energy level (0.0 to 1.0)
    pub fn get_energy(&self, samples: &[f32], time: f64, band: &str) -> f32 {
        let spectrum = self.compute_spectrum(samples, time);
        if spectrum.is_empty() {
            return 0.0;
        }

        let bin_hz = self.sample_rate as f32 / self.fft_size as f32;

        let (low_freq, high_freq) = match band {
            "bass" => (20.0, 250.0),
            "mids" => (250.0, 4000.0),
            "highs" => (4000.0, 20000.0),
            _ => (20.0, 20000.0),
        };

        let low_bin = (low_freq / bin_hz).floor() as usize;
        let high_bin = ((high_freq / bin_hz).ceil() as usize).min(spectrum.len());

        if low_bin >= high_bin {
            return 0.0;
        }

        let sum: f32 = spectrum[low_bin..high_bin].iter().sum();
        let avg = sum / (high_bin - low_bin) as f32;

        (avg * 10.0).min(1.0)
    }

    /// Convenience method: get bass energy (20-250 Hz)
    pub fn bass(&self, samples: &[f32], time: f64) -> f32 {
        self.get_energy(samples, time, "bass")
    }

    /// Convenience method: get mids energy (250-4000 Hz)
    pub fn mids(&self, samples: &[f32], time: f64) -> f32 {
        self.get_energy(samples, time, "mids")
    }

    /// Convenience method: get highs energy (4000-20000 Hz)
    pub fn highs(&self, samples: &[f32], time: f64) -> f32 {
        self.get_energy(samples, time, "highs")
    }
}

/// Resamples audio data to the target sample rate.
///
/// Uses high-quality Sinc interpolation (via `rubato`).
pub fn resample_audio(samples: &[f32], source_rate: u32, target_rate: u32) -> Result<Vec<f32>> {
    if source_rate == target_rate {
        return Ok(samples.to_vec());
    }

    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,
        window: WindowFunction::BlackmanHarris2,
    };

    let ratio = target_rate as f64 / source_rate as f64;
    let mut resampler = SincFixedIn::<f32>::new(
        ratio,
        256.0,
        params,
        1024,
        2,
    )
    .context("Failed to create resampler")?;

    let frames = samples.len() / 2;
    let mut left = Vec::with_capacity(frames);
    let mut right = Vec::with_capacity(frames);

    for chunk in samples.chunks_exact(2) {
        left.push(chunk[0]);
        right.push(chunk[1]);
    }

    let input_chunk_size = resampler.input_frames_max();
    let mut output_left = Vec::with_capacity((frames as f64 * ratio) as usize + 1024);
    let mut output_right = Vec::with_capacity((frames as f64 * ratio) as usize + 1024);

    let mut input_idx = 0;
    while input_idx < frames {
        let end = (input_idx + input_chunk_size).min(frames);
        let len = end - input_idx;

        let chunk_left = &left[input_idx..end];
        let chunk_right = &right[input_idx..end];

        let mut input_batch = vec![chunk_left.to_vec(), chunk_right.to_vec()];

        if len < input_chunk_size {
            input_batch[0].resize(input_chunk_size, 0.0);
            input_batch[1].resize(input_chunk_size, 0.0);
        }

        let output_batch = resampler
            .process(&input_batch, None)
            .context("Resampling failed")?;

        output_left.extend_from_slice(&output_batch[0]);
        output_right.extend_from_slice(&output_batch[1]);

        input_idx += input_chunk_size;
    }

    let out_len = output_left.len();
    let mut result = Vec::with_capacity(out_len * 2);
    for i in 0..out_len {
        result.push(output_left[i]);
        result.push(output_right[i]);
    }

    Ok(result)
}

/// Decodes an audio file from raw bytes into interleaved stereo float samples.
///
/// Automatically handles format detection and resampling to the target rate.
pub fn load_audio_bytes(data: &[u8], target_sample_rate: u32) -> Result<Vec<f32>> {
    let mss = MediaSourceStream::new(Box::new(Cursor::new(data.to_vec())), Default::default());
    let hint = Hint::new();
    let format_opts = FormatOptions::default();
    let metadata_opts = MetadataOptions::default();
    let decoder_opts = DecoderOptions::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .context("Unsupported format")?;

    let mut format = probed.format;
    let track = format.default_track().context("No track found")?;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &decoder_opts)
        .context("Unsupported codec")?;

    let track_id = track.id;
    let source_rate = track.codec_params.sample_rate.unwrap_or(44100);

    let mut samples = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(symphonia::core::errors::Error::IoError(_)) => break,
            Err(symphonia::core::errors::Error::ResetRequired) => break,
            Err(_) => break,
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                let duration = decoded.capacity() as u64;
                let mut buf = symphonia::core::audio::SampleBuffer::<f32>::new(duration, spec);
                buf.copy_interleaved_ref(decoded);

                let buf_samples = buf.samples();
                let channels = spec.channels.count();

                if channels == 1 {
                    for s in buf_samples {
                        samples.push(*s);
                        samples.push(*s);
                    }
                } else if channels >= 2 {
                    for chunk in buf_samples.chunks(channels) {
                        samples.push(chunk[0]);
                        samples.push(chunk[1]);
                    }
                }
            }
            Err(_) => break,
        }
    }

    resample_audio(&samples, source_rate, target_sample_rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mixing_logic() {
        let mut mixer = AudioMixer::new(48000);
        let track = AudioTrack {
            samples: vec![0.5; 48000 * 2],
            volume: Animated::new(1.0),
            pan: Animated::new(0.5),
            effects: AudioEffectChain::new(),
            effect_param_animations: Vec::new(),
            start_time: 0.0,
            duration: None,
            loop_audio: false,
        };
        let tid = mixer.add_track(track);

        let mixed = mixer.mix(100, 0.0);
        assert_eq!(mixed.len(), 200);
        let center_db = 0.5_f32.sqrt();
        assert!((mixed[0] - 0.5 * center_db).abs() < 1e-5);
        assert!((mixed[1] - 0.5 * center_db).abs() < 1e-5);

        // Test pan: full left
        if let Some(t) = mixer.get_track_mut(tid) {
            t.pan = Animated::new(0.0);
        }
        let mixed_left = mixer.mix(100, 0.0);
        assert!((mixed_left[0] - 0.5).abs() < 1e-5);
        assert!(mixed_left[1].abs() < 1e-5);
    }
}
