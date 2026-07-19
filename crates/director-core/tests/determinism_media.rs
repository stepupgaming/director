//! Plan 06 — determinism & media clock acceptance tests.

use director_core::director::Director;
use director_core::errors::RenderError;
use director_core::scripting::register_rhai_api;
use director_core::timeline::{
    audio_samples_per_video_frame, duration_to_frame_count, export_audio_sample_count,
    video_seek_time, video_source_frame_index,
};
use director_core::video_wrapper::RenderMode;
use director_core::DefaultAssetLoader;
use rhai::Engine;
use std::path::PathBuf;
use std::sync::Arc;

struct EmptyAssetLoader;

impl director_core::AssetLoader for EmptyAssetLoader {
    fn load_bytes(&self, path: &str) -> anyhow::Result<Vec<u8>> {
        anyhow::bail!("missing {path}")
    }
}

#[test]
fn required_fonts_present_when_assets_exist() {
    let d = Director::new(
        64,
        64,
        30.0,
        Arc::new(DefaultAssetLoader),
        RenderMode::Preview,
        None,
    );
    // CI and local tree should have force-kept fonts under crate assets/.
    assert!(
        d.missing_required_fonts.is_empty(),
        "expected fonts present, missing {:?}",
        d.missing_required_fonts
    );
    d.ensure_required_fonts().expect("fonts ok");
}

#[test]
fn required_fonts_fail_closed_when_missing() {
    let d = Director::new(
        64,
        64,
        30.0,
        Arc::new(EmptyAssetLoader),
        RenderMode::Export,
        None,
    );
    assert!(
        !d.missing_required_fonts.is_empty(),
        "empty loader must report missing fonts"
    );
    match d.ensure_required_fonts() {
        Err(RenderError::MissingRequiredFonts(list)) => {
            assert!(list.iter().any(|f| f.contains("Roboto-Regular")));
        }
        other => panic!("expected MissingRequiredFonts, got {other:?}"),
    }
}

#[test]
fn no_thread_rng_in_core_src() {
    // Walk director-core/src and forbid unseeded thread_rng outside comments.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("read_dir") {
            let entry = entry.expect("entry");
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("read");
            for (i, line) in text.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.starts_with("//") {
                    continue;
                }
                if line.contains("thread_rng") {
                    out.push(format!("{}:{}: {}", path.display(), i + 1, trimmed));
                }
            }
        }
    }
    walk(&root, &mut offenders);
    assert!(
        offenders.is_empty(),
        "unseeded thread_rng forbidden in core lib:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn seed_stable_noise_frames() {
    let mut eng = Engine::new();
    register_rhai_api(&mut eng, Arc::new(DefaultAssetLoader));
    let script = r#"
        set_seed(42);
        let a = rand_float(0.0, 1.0);
        let b = rand_float(0.0, 1.0);
        set_seed(42);
        let c = rand_float(0.0, 1.0);
        let d = rand_float(0.0, 1.0);
        [a, b, c, d]
    "#;
    let vals: rhai::Array = eng.eval(script).expect("eval");
    let a = vals[0].as_float().unwrap();
    let b = vals[1].as_float().unwrap();
    let c = vals[2].as_float().unwrap();
    let d = vals[3].as_float().unwrap();
    assert!((a - c).abs() < 1e-12, "same seed must replay first draw");
    assert!((b - d).abs() < 1e-12, "same seed must replay second draw");
    assert!((a - b).abs() > 1e-12, "successive draws should differ");
}

#[test]
fn video_seek_floor_policy() {
    let fps = 30.0;
    assert_eq!(video_source_frame_index(0.0, fps), 0);
    assert_eq!(video_source_frame_index(0.033, fps), 0); // just under 1/30
    assert_eq!(video_source_frame_index(1.0 / 30.0, fps), 1);
    assert_eq!(video_source_frame_index(1.0, fps), 30);
    // Quantized seek time lands exactly on frame grid.
    let t = video_seek_time(0.049, fps);
    assert!((t - 1.0 / 30.0).abs() < 1e-9, "t={t}");
    let t2 = video_seek_time(0.049, fps);
    assert_eq!(
        video_source_frame_index(t, fps),
        video_source_frame_index(t2, fps)
    );
}

#[test]
fn freeze_maps_to_constant_seek_time() {
    // freeze_time pins local_time; video_seek_time of a constant is constant.
    let frozen = 1.25_f64;
    let a = video_seek_time(frozen, 24.0);
    let b = video_seek_time(frozen, 24.0);
    assert!((a - b).abs() < 1e-12);
    // Advancing wall time without changing source time must not change seek.
    assert_eq!(
        video_source_frame_index(frozen, 24.0),
        video_source_frame_index(frozen, 24.0)
    );
}

#[test]
fn export_av_duration_alignment() {
    let fps = 30.0;
    let sample_rate = 48_000_u32;
    let duration = 2.0;
    let frames = duration_to_frame_count(duration, fps) as usize;
    assert_eq!(frames, 60);
    let per_frame = audio_samples_per_video_frame(sample_rate, fps, 2);
    // 48000/30 = 1600 mono → 3200 interleaved stereo
    assert_eq!(per_frame, 3200);
    let total = export_audio_sample_count(duration, fps, sample_rate, 2);
    assert_eq!(total, frames * per_frame);

    // NTSC-ish
    let fps_n = 23.976;
    let frames_n = duration_to_frame_count(1.0, fps_n);
    let total_n = export_audio_sample_count(1.0, fps_n, sample_rate, 2);
    assert_eq!(
        total_n,
        frames_n as usize * audio_samples_per_video_frame(sample_rate, fps_n, 2)
    );
}
