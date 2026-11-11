//! A video that fills every frame under the other layers stays inside
//! FFmpeg: the export decodes nothing in the engine and shows the frames a
//! full paint shows. A scene outside that shape exports through the full paint.

use director_core::scene_export::export_scene;
use director_core::scene_import::ImportRegistry;
use director_core::video_wrapper::EncoderMode;
use director_html::{compile, register_import, CompileOptions};
use director_render::{ExportOptions, Renderer};
use std::path::Path;
use std::sync::Arc;

const SIZE: (usize, usize) = (128, 96);
const FRAMES: usize = 90;

/// Smooth waves that move every frame, so a frame out of step stands out.
const PATTERN: &str = "color=s=128x96:r=30:d=3,format=yuv420p,    geq=lum='128+90*sin((X+N*7)/9)':cb='128+50*sin((Y+N*5)/11)':cr='128+40*cos((X-N*3)/13)'";

/// Drops the frames [`dropped`] names and keeps the rest at their times.
const GAPS: [&str; 4] = ["-vf", "select='not(eq(mod(n,10),3))'", "-fps_mode", "vfr"];

/// A frame `vfr.mp4` lacks.
fn dropped(index: usize) -> bool {
    index % 10 == 3
}

/// Three seconds of [`PATTERN`] with a tone: `clip.mp4` as H.264 4:2:0,
/// `vfr.mp4` as the same with every tenth frame missing (27 frames a second
/// on average, 30 nominal) and `rgb.mkv` as FFV1 RGB.
fn clips(dir: &Path) {
    let h264 = ["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"];
    let clips = [("clip.mp4", h264, &[][..]), ("vfr.mp4", h264, &GAPS[..]), ("rgb.mkv", ["-c:v", "ffv1", "-pix_fmt", "bgr0", "-c:a", "pcm_s16le"], &[][..])];
    for (name, codec, filter) in clips {
        let result = std::process::Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", PATTERN])
            .args(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=3"])
            .args(filter)
            .args(codec)
            .arg(dir.join(name))
            .output()
            .expect("FFmpeg must be installed for native media tests");
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    }
}

fn build(dir: &Path, body: &str, (width, height): (usize, usize)) -> Renderer {
    let html = format!(
        "<style>body{{margin:0}}video,div{{position:absolute}}
        .caption{{left:24px;top:72px;width:80px;height:16px;background:rgba(255,220,0,0.8);border-radius:4px}}</style>{body}"
    );
    let options = CompileOptions {
        width: width as i32,
        height: height as i32,
        duration: 3.0,
        base_dir: dir.into(),
        ..Default::default()
    };
    let compiled = compile(&html, options).unwrap();
    let mut registry = ImportRegistry::core();
    register_import(&mut registry);
    Renderer::new(Arc::new(export_scene(&compiled.director).unwrap()), compiled.director.assets.loader.clone())
        .with_registry(registry)
        .with_render_mode(compiled.director.render_mode)
}

/// Export `renderer` and decode the result to RGB frames.
fn export(renderer: &Renderer, path: &Path, paint_all: bool) -> Vec<u8> {
    let options = ExportOptions {
        paint_all,
        workers: Some(2),
        ..ExportOptions::new(EncoderMode::Software)
    };
    renderer.export_options(path, &options, None).unwrap();
    let output = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-map", "0:v", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .expect("FFmpeg runs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    output.stdout
}

/// Mean channel difference of two frames.
fn apart(a: &[u8], b: &[u8]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a.abs_diff(*b) as f64).sum::<f64>() / a.len() as f64
}

/// One scene exported over its base video and through the full paint.
struct Exports {
    /// Decoders the first export started in the engine.
    started: u64,
    frame: usize,
    over: Vec<u8>,
    full: Vec<u8>,
}

impl Exports {
    fn of(body: &str, size: (usize, usize)) -> Self {
        let temp = tempfile::tempdir().unwrap();
        clips(temp.path());
        let renderer = build(temp.path(), body, size);
        let over = export(&renderer, &temp.path().join("over.mp4"), false);
        let started = renderer.frame_sources().sessions_started();
        assert_eq!(renderer.frame_sources().open_sources(), 0, "export must close its sources");
        let full = export(&build(temp.path(), body, size), &temp.path().join("full.mp4"), true);
        let frame = size.0 * size.1 * 3;
        assert_eq!(over.len(), frame * FRAMES, "three seconds at 30 fps");
        assert_eq!(full.len(), over.len(), "both exports hold the same frames");
        Self { started, frame, over, full }
    }

    /// Every frame shows the layers and the source frame the full paint
    /// shows. The base video keeps its own samples, where the full paint
    /// converts them to RGB and back, a level or two apart.
    fn assert_same_picture(&self) {
        let over: Vec<&[u8]> = self.over.chunks_exact(self.frame).collect();
        let full: Vec<&[u8]> = self.full.chunks_exact(self.frame).collect();
        for index in 0..FRAMES {
            let same = apart(over[index], full[index]);
            assert!(same < 5.0, "frame {index} differs from the full paint by {same} levels a channel");
            for other in [index.wrapping_sub(1), index + 1] {
                if let Some(other) = full.get(other) {
                    assert!(same < apart(over[index], other), "frame {index} shows another source frame");
                }
            }
        }
    }
}

const VIDEO: &str = "<video muted src='clip.mp4' data-start='0' data-duration='3' style='left:0;top:0;width:128px;height:96px'></video>";

#[test]
fn a_caption_over_a_full_frame_video_leaves_the_video_to_ffmpeg() {
    let exports = Exports::of(&format!("{VIDEO}<audio src='clip.mp4' data-start='0'></audio><div class='caption'></div>"), SIZE);
    assert_eq!(exports.started, 0, "the engine decodes no video frame");
    exports.assert_same_picture();
}

#[test]
fn a_variable_rate_video_shows_each_frame_at_its_own_time() {
    let steady = Exports::of(&format!("{VIDEO}<div class='caption'></div>"), SIZE);
    let gapped = Exports::of(&format!("{}<div class='caption'></div>", VIDEO.replace("clip.mp4", "vfr.mp4")), SIZE);
    assert_eq!(gapped.started, 0, "the engine decodes no video frame");
    let steady: Vec<&[u8]> = steady.full.chunks_exact(steady.frame).collect();
    for (name, frames) in [("over the base video", &gapped.over), ("through the full paint", &gapped.full)] {
        let frames: Vec<&[u8]> = frames.chunks_exact(gapped.frame).collect();
        for index in (0..FRAMES).filter(|index| !dropped(*index)) {
            let same = apart(frames[index], steady[index]);
            for other in [index.wrapping_sub(1), index + 1] {
                if let Some(other) = steady.get(other) {
                    assert!(same < apart(frames[index], other), "frame {index} {name} shows another time of the source");
                }
            }
        }
    }
    for index in 0..FRAMES {
        let same = apart(&gapped.over[index * gapped.frame..][..gapped.frame], &gapped.full[index * gapped.frame..][..gapped.frame]);
        assert!(same < 5.0, "frame {index} differs from the full paint by {same} levels a channel");
    }
}

#[test]
fn paint_outside_the_sampled_band_exports_over_the_whole_frame() {
    // The second element shows for one frame the plan does not sample.
    let body = format!(
        "{VIDEO}<div class='caption'></div>
        <div class='clip' data-start='0.06' data-duration='0.03' style='left:8px;top:4px;width:60px;height:30px;background:#f0f'></div>"
    );
    let exports = Exports::of(&body, SIZE);
    assert_eq!(exports.started, 0, "the engine decodes no video frame");
    exports.assert_same_picture();
}

#[test]
fn scenes_outside_the_passthrough_shape_paint_in_full() {
    let scaled = "<video muted src='clip.mp4' data-start='0' data-duration='3' style='left:0;top:0;width:256px;height:192px'></video>";
    let late = "<video muted src='clip.mp4' data-start='1' data-duration='2' style='left:0;top:0;width:128px;height:96px'></video>";
    let cases = [
        ("a scaled video", scaled.to_string(), (256, 192)),
        ("a video absent from the first second", late.to_string(), SIZE),
        ("an RGB video", VIDEO.replace("clip.mp4", "rgb.mkv"), SIZE),
        ("a translucent video", VIDEO.replace("style='", "style='opacity:0.5;"), SIZE),
        ("a rounded video", VIDEO.replace("style='", "style='border-radius:12px;"), SIZE),
        ("a blend over the video", format!("{VIDEO}<div class='caption' style='mix-blend-mode:multiply'></div>"), SIZE),
        ("a backdrop blur over the video", format!("{VIDEO}<div class='caption' style='backdrop-filter:blur(4px)'></div>"), SIZE),
    ];
    for (name, body, size) in cases {
        let exports = Exports::of(&body, size);
        assert!(exports.started > 0, "{name} decodes in the engine");
        assert!(exports.over == exports.full, "{name} exports the same frames either way");
    }
}
