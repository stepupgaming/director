//! Video elements of one file read one shared source: export workers and
//! elements playing the same position share a decoder, and an element that
//! paints nothing decodes nothing.

use director_core::scene_export::export_scene;
use director_core::scene_import::ImportRegistry;
use director_core::video_wrapper::EncoderMode;
use director_html::{compile, register_import, CompileOptions};
use director_render::{Frame, Renderer};
use std::path::Path;
use std::sync::Arc;

const FPS: f64 = 30.0;

/// A three-second FFV1 test pattern whose frames change every frame.
fn clip(dir: &Path) {
    let result = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i"])
        .args(["testsrc=s=64x64:r=30:d=3", "-c:v", "ffv1"])
        .arg(dir.join("clip.mkv"))
        .output()
        .expect("FFmpeg must be installed for native media tests");
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
}

/// Seven elements of `clip.mkv`. Four paint the source at the timeline time
/// and one paints it a second behind. The fully transparent and the hidden
/// element would play 1.5 seconds ahead, but they paint nothing.
fn build(dir: &Path) -> Renderer {
    let html = "<style>body{margin:0}video{position:absolute;width:32px;height:32px}</style>
        <video muted src='clip.mkv' data-start='0' data-duration='3' style='left:0;top:0'></video>
        <video muted src='clip.mkv' data-start='0' data-duration='3' style='left:32px;top:0;opacity:0.5'></video>
        <video muted src='clip.mkv' data-start='0.5' data-duration='2' data-media-start='0.5' style='left:64px;top:0'></video>
        <video muted src='clip.mkv' data-start='0' data-duration='1.5' data-media-start='1.5' style='left:96px;top:0;opacity:0'></video>
        <video muted src='clip.mkv' data-start='0' data-duration='1.5' data-media-start='1.5' style='left:0;top:32px;visibility:hidden'></video>
        <video muted src='clip.mkv' data-start='2' data-duration='1' data-media-start='2' style='left:32px;top:32px'></video>
        <video muted src='clip.mkv' data-start='1' data-duration='2' style='left:64px;top:32px'></video>";
    let options = CompileOptions {
        width: 128,
        height: 64,
        duration: 3.0,
        base_dir: dir.into(),
        ..Default::default()
    };
    let compiled = compile(html, options).unwrap();
    let mut registry = ImportRegistry::core();
    register_import(&mut registry);
    Renderer::new(Arc::new(export_scene(&compiled.director).unwrap()), compiled.director.assets.loader.clone())
        .with_registry(registry)
        .with_render_mode(compiled.director.render_mode)
}

fn times() -> Vec<f64> {
    (0..(3.0 * FPS) as usize).map(|frame| frame as f64 / FPS).collect()
}

#[test]
fn elements_of_one_file_open_one_decoder_per_position() {
    let temp = tempfile::tempdir().unwrap();
    clip(temp.path());
    let renderer = build(temp.path());
    let times = times();
    let mut frames: Vec<Frame> = Vec::new();
    renderer.pool_with(1).render_into(&times, &mut frames).unwrap();
    assert_eq!(renderer.frame_sources().sessions_started(), 2, "one decoder per playing position");

    // A fresh renderer seeks cold for each frame and paints the same pixels.
    for index in [0, 14, 31, 45, 60, 77, 89] {
        let cold = build(temp.path()).render_frame(times[index]).unwrap();
        assert!(cold == frames[index], "frame {index} differs from a cold seek");
    }
}

#[test]
fn export_workers_share_their_decoders() {
    let temp = tempfile::tempdir().unwrap();
    clip(temp.path());
    let framemd5 = |workers: usize| {
        let renderer = build(temp.path());
        let path = temp.path().join(format!("out{workers}.mp4"));
        renderer.export_with(&path, EncoderMode::Software, Some(workers)).unwrap();
        let started = renderer.frame_sources().sessions_started();
        assert!(started <= 4, "{workers} workers started {started} decoders for two positions");
        assert_eq!(renderer.frame_sources().open_sources(), 0, "export must close its sources");
        let output = std::process::Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-i"])
            .arg(&path)
            .args(["-map", "0:v", "-f", "framemd5", "-"])
            .output()
            .expect("FFmpeg runs");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap()
    };
    assert_eq!(framemd5(4), framemd5(1));
}
