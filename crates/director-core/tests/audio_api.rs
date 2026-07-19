//! Runtime test for the AudioTrack loudness Rhai API.
//! Exercises `track.loudness()` from a Rhai script.

use director_core::scripting::register_rhai_api;
use director_core::DefaultAssetLoader;
use rhai::{Engine, Map};
use std::f32::consts::PI;
use std::sync::Arc;

fn generate_sine_wave(sample_rate: u32, duration_secs: f64, frequency: f32, dbfs: f32) -> Vec<f32> {
    let amplitude = 10.0_f32.powf(dbfs / 20.0);
    let num_samples = (sample_rate as f64 * duration_secs) as usize;
    let mut samples = Vec::with_capacity(num_samples * 2);
    for i in 0..num_samples {
        let t = i as f32 / sample_rate as f32;
        let sample = (2.0 * PI * frequency * t).sin() * amplitude;
        samples.push(sample);
        samples.push(sample);
    }
    samples
}

#[test]
fn audio_track_loudness_returns_lufs_true_peak_and_range() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    // Generate a 1 kHz sine wave at -20 dBFS, 3 seconds, stereo.
    let samples = generate_sine_wave(48000, 3.0, 1000.0, -20.0);
    let samples_array: rhai::Array = samples
        .into_iter()
        .map(|s| rhai::Dynamic::from(s as f64))
        .collect();

    let script = r#"
        let movie = new_director(1080, 1920, 30);
        let track = movie.add_global_audio(samples);
        let loudness = track.loudness();

        #{
            integrated_lufs: loudness.integrated_lufs,
            true_peak_db: loudness.true_peak_db,
            loudness_range_lu: loudness.loudness_range_lu,
            channels: loudness.channels,
            sample_rate: loudness.sample_rate,
        }
    "#;

    let mut scope = rhai::Scope::new();
    scope.push("samples", samples_array);
    let result = engine
        .eval_ast_with_scope(
            &mut scope,
            &engine.compile(script).expect("script must compile"),
        )
        .expect("loudness script must evaluate");

    let result: Map = result;

    let integrated_lufs: f64 = result
        .get("integrated_lufs")
        .and_then(|v| v.as_float().ok())
        .expect("integrated_lufs present");
    let true_peak_db: f64 = result
        .get("true_peak_db")
        .and_then(|v| v.as_float().ok())
        .expect("true_peak_db present");
    let loudness_range_lu: f64 = result
        .get("loudness_range_lu")
        .and_then(|v| v.as_float().ok())
        .expect("loudness_range_lu present");
    let channels: i64 = result
        .get("channels")
        .and_then(|v| v.as_int().ok())
        .expect("channels present");
    let sample_rate: i64 = result
        .get("sample_rate")
        .and_then(|v| v.as_int().ok())
        .expect("sample_rate present");

    assert!(integrated_lufs.is_finite(), "integrated LUFS must be finite");
    assert!(true_peak_db.is_finite(), "true peak must be finite");
    assert!(loudness_range_lu.is_finite(), "loudness range must be finite");

    // A -20 dBFS sine wave should measure close to -20 LUFS after K-weighting.
    assert!(
        integrated_lufs > -23.0 && integrated_lufs < -18.0,
        "expected integrated loudness near -20 LUFS, got {}",
        integrated_lufs
    );

    // True peak should be close to -20 dBTP.
    assert!(
        (true_peak_db - -20.0).abs() < 1.0,
        "expected true peak near -20 dBTP, got {}",
        true_peak_db
    );

    // Constant tone should have a small loudness range.
    assert!(
        loudness_range_lu < 2.0,
        "expected small loudness range for constant tone, got {}",
        loudness_range_lu
    );

    assert_eq!(channels, 2, "expected stereo analysis");
    assert_eq!(sample_rate, 48000, "expected 48 kHz sample rate");
}
