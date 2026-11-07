use director_asr::audio::{decode, to_mono_16k};
use director_asr::{AsrError, Audio};

use crate::{ffmpeg, rms};

/// Index of the largest absolute sample.
fn peak(samples: &[f32]) -> usize {
    (0..samples.len()).max_by(|a, b| samples[*a].abs().total_cmp(&samples[*b].abs())).unwrap()
}

#[test]
fn resampling_keeps_length_and_timing() {
    for rate in [48_000u32, 44_100, 22_050, 8_000] {
        let len = (rate as f64 * 2.5) as usize;
        let mut mono = vec![0f32; len];
        let at = rate as usize; // 1.0 s
        for s in &mut mono[at..at + rate as usize / 1000] {
            *s = 1.0;
        }
        let out = to_mono_16k(Audio { samples: &mono, sample_rate: rate, channels: 1 }).unwrap();
        assert!((out.len() as i64 - 40_000).abs() <= 2, "{rate} Hz gives {} samples", out.len());
        // The 1 ms click covers output samples 16 000 to 16 016.
        let p = peak(&out);
        assert!((15_996..=16_020).contains(&p), "{rate} Hz click lands at sample {p}");
    }
}

#[test]
fn resampling_keeps_a_tone_level() {
    let rate = 48_000;
    let mono: Vec<f32> = (0..rate * 2).map(|i| 0.5 * (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin()).collect();
    let out = to_mono_16k(Audio { samples: &mono, sample_rate: rate as u32, channels: 1 }).unwrap();
    let level = rms(&out[4_000..28_000]);
    assert!((level - 0.5 / 2f32.sqrt()).abs() < 0.005, "rms {level}");
}

#[test]
fn downmix_averages_channels_and_passes_16k_through() {
    let stereo: Vec<f32> = (0..32_000).flat_map(|i| [0.25 + (i % 2) as f32 * 0.5, -0.25]).collect();
    let out = to_mono_16k(Audio { samples: &stereo, sample_rate: 16_000, channels: 2 }).unwrap();
    assert_eq!(out.len(), 32_000);
    assert_eq!(out[0], 0.0);
    assert_eq!(out[1], 0.25);
}

#[test]
fn bad_audio_shapes_fail() {
    let samples = [0f32; 5];
    for (rate, channels) in [(16_000, 0), (0, 1), (16_000, 2)] {
        let err = to_mono_16k(Audio { samples: &samples, sample_rate: rate, channels }).unwrap_err();
        assert!(matches!(err, AsrError::Invalid(_)), "{err}");
    }
}

#[test]
fn media_decodes_to_16k_mono() {
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("tone.wav");
    ffmpeg(&["-f", "lavfi", "-i", "sine=frequency=1000:sample_rate=44100:duration=2", "-ac", "2"], &wav);
    let samples = decode(&wav).unwrap();
    assert!((samples.len() as i64 - 32_000).abs() <= 2, "{} samples", samples.len());
    let level = rms(&samples[1_000..31_000]);
    assert!((level - 0.125 / 2f32.sqrt()).abs() < 0.01, "rms {level}");

    let mp4 = dir.path().join("clip.mp4");
    ffmpeg(
        &["-f", "lavfi", "-i", "color=black:s=64x64:d=1.5", "-f", "lavfi", "-i", "sine=frequency=500:sample_rate=48000:duration=1.5", "-shortest", "-c:v", "libx264", "-c:a", "aac"],
        &mp4,
    );
    let samples = decode(&mp4).unwrap();
    assert!((samples.len() as f64 / 16_000.0 - 1.5).abs() < 0.05, "{} samples", samples.len());
}

#[test]
fn media_without_audio_fails_with_ffmpeg_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let mp4 = dir.path().join("silent.mp4");
    ffmpeg(&["-f", "lavfi", "-i", "color=black:s=64x64:d=0.5", "-c:v", "libx264"], &mp4);
    let err = decode(&mp4).unwrap_err();
    assert!(matches!(err, AsrError::Tool { tool: "ffmpeg", .. }), "{err}");
}
