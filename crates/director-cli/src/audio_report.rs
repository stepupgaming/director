//! Loudness report for audio already loaded on the mixer.

use crate::input::Input;
use clap::Parser;
use director_core::loudness::{analyze_stereo, LoudnessAnalysis};
use director_core::video_wrapper::RenderMode;

/// 10 minutes of stereo at 48 kHz. Longer tracks are reported and not analyzed.
pub const MAX_AUDIO_SAMPLES: usize = 48_000 * 2 * 600;

#[derive(Parser, Debug)]
pub struct AudioArgs {
    #[command(flatten)]
    pub input: Input,
}

pub struct TrackInput<'a> {
    pub index: usize,
    pub start: f64,
    pub duration: Option<f64>,
    pub samples: &'a [f32],
}

pub fn report_tracks(tracks: &[TrackInput<'_>], sample_rate: u32) -> serde_json::Value {
    let tracks = tracks
        .iter()
        .map(|track| track_value(track, sample_rate))
        .collect::<Vec<_>>();
    serde_json::json!({ "tracks": tracks })
}

pub fn audio_rejection(sample_count: usize, non_finite: bool) -> Option<&'static str> {
    if sample_count > MAX_AUDIO_SAMPLES {
        return Some("audio exceeds 57600000 samples");
    }
    if non_finite {
        return Some("non-finite audio sample");
    }
    None
}

fn track_value(track: &TrackInput<'_>, sample_rate: u32) -> serde_json::Value {
    if let Some(reason) = audio_rejection(
        track.samples.len(),
        track.samples.iter().any(|sample| !sample.is_finite()),
    ) {
        return track_error(track, reason);
    }
    let clipped_samples = track
        .samples
        .iter()
        .filter(|sample| sample.abs() >= 1.0)
        .count();
    let analysis = if track.samples.is_empty() || sample_rate == 0 {
        LoudnessAnalysis::silent()
    } else {
        analyze_stereo(track.samples, sample_rate)
    };
    let silent = !analysis.is_valid() || !analysis.integrated_lufs.is_finite();
    serde_json::json!({
        "index": track.index,
        "start": finite_or_null(track.start),
        "duration": match track.duration {
            Some(duration) => finite_or_null(duration),
            None => serde_json::Value::Null,
        },
        "integrated_lufs": finite_or_null(analysis.integrated_lufs),
        "true_peak_db": finite_or_null(analysis.true_peak_db),
        "loudness_range_lu": finite_or_null(analysis.loudness_range_lu),
        "channels": analysis.channels,
        "sample_rate": analysis.sample_rate,
        "clipped_samples": clipped_samples,
        "silent": silent,
    })
}

fn track_error(track: &TrackInput<'_>, reason: &str) -> serde_json::Value {
    serde_json::json!({
        "index": track.index,
        "start": finite_or_null(track.start),
        "duration": match track.duration {
            Some(duration) => finite_or_null(duration),
            None => serde_json::Value::Null,
        },
        "integrated_lufs": serde_json::Value::Null,
        "true_peak_db": serde_json::Value::Null,
        "loudness_range_lu": serde_json::Value::Null,
        "channels": 0,
        "sample_rate": 0,
        "clipped_samples": 0,
        "silent": true,
        "error": reason,
    })
}

fn finite_or_null(value: f64) -> serde_json::Value {
    if value.is_finite() {
        serde_json::json!(value)
    } else {
        serde_json::Value::Null
    }
}

pub fn run(args: AudioArgs) -> i32 {
    let loaded = match args.input.load(RenderMode::Export) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    let director = match loaded.movie.director.lock() {
        Ok(director) => director,
        Err(_) => {
            eprintln!("Director lock poisoned");
            return 1;
        }
    };
    let tracks = director
        .audio_mixer
        .tracks
        .iter()
        .enumerate()
        .filter_map(|(index, track)| {
            let track = track.as_ref()?;
            Some(TrackInput {
                index,
                start: track.start_time,
                duration: track.duration,
                samples: &track.samples,
            })
        })
        .collect::<Vec<_>>();
    println!(
        "{}",
        report_tracks(&tracks, director.audio_mixer.sample_rate)
    );
    0
}

#[cfg(test)]
mod tests {
    use super::{report_tracks, TrackInput};

    #[test]
    fn agent_audio_empty_track_is_silent() {
        let report = report_tracks(
            &[TrackInput {
                index: 0,
                start: 0.0,
                duration: None,
                samples: &[],
            }],
            48000,
        );
        let track = &report["tracks"][0];
        assert_eq!(track["silent"], true);
        assert_eq!(track["clipped_samples"], 0);
    }

    #[test]
    fn agent_audio_counts_full_scale_samples() {
        let report = report_tracks(
            &[TrackInput {
                index: 0,
                start: 0.0,
                duration: None,
                samples: &[0.0, 0.0, 1.0, -1.0],
            }],
            48000,
        );
        assert_eq!(report["tracks"][0]["clipped_samples"], 2);
    }

    #[test]
    fn agent_audio_rejects_non_finite_and_over_cap_samples() {
        use super::{audio_rejection, report_tracks, TrackInput, MAX_AUDIO_SAMPLES};
        let report = report_tracks(
            &[TrackInput {
                index: 0,
                start: f64::NAN,
                duration: Some(f64::INFINITY),
                samples: &[f32::NAN],
            }],
            48000,
        );
        let track = &report["tracks"][0];
        assert_eq!(track["error"], "non-finite audio sample");
        assert!(track["start"].is_null());
        assert!(track["duration"].is_null());
        assert!(track["integrated_lufs"].is_null());
        assert_eq!(
            audio_rejection(MAX_AUDIO_SAMPLES + 1, false),
            Some("audio exceeds 57600000 samples")
        );
    }
}
