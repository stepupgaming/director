//! Compiled HTML paints the same frames from its scene document as from the
//! compiled `Director`.

use director_core::scene_export::export_scene;
use director_core::scene_import::ImportRegistry;
use director_core::video_wrapper::EncoderMode;
use director_html::{compile, compile_file, register_import, CompileOptions, CompiledHtml};
use director_render::{export_director, render_export, render_export_at_scale, Renderer};
use std::path::Path;
use std::sync::Arc;

use crate::common;

const TIMES: [f64; 4] = [0.0, 0.5, 1.25, 2.5];

fn registry() -> ImportRegistry {
    let mut registry = ImportRegistry::core();
    register_import(&mut registry);
    registry
}

/// Frames at [`TIMES`] where the document render differs from the compiled `Director`.
fn mismatches(name: &str, compiled: CompiledHtml) -> Vec<String> {
    let document = Arc::new(export_scene(&compiled.director).unwrap());
    let renderer = Renderer::new(document, compiled.director.assets.loader.clone())
        .with_registry(registry())
        .with_render_mode(compiled.director.render_mode);
    common::mismatches(name, compiled.director, &renderer, &TIMES)
}

#[test]
fn every_html_example_renders_from_its_document() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/html");
    let mut failures = Vec::new();
    let mut count = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("html") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let options = CompileOptions {
            gsap_dir: Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("../director-html/tools/node_modules/gsap/dist")),
            ..Default::default()
        };
        let compiled = compile_file(&path, options).unwrap_or_else(|e| panic!("{name}: {e}"));
        failures.extend(mismatches(&name, compiled));
        count += 1;
    }
    assert!(count >= 4, "found {count} HTML examples");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn gradients_filters_text_and_animations_render_from_the_document() {
    let html = "<style>
        body { margin: 0; background: #eee }
        @keyframes slide {
          from { transform: translateX(0px) rotate(0deg); background-color: red }
          50% { animation-timing-function: steps(4, jump-start) }
          to { transform: translateX(40px) rotate(90deg); background-color: blue }
        }
        .card {
          width: 120px; height: 60px; margin: 20px;
          background-image: linear-gradient(to bottom right, red, 30%, blue calc(50% + 4px)),
            radial-gradient(circle closest-side at 25% 75%, white, black),
            conic-gradient(from 45deg at 50% 50%, red, green);
          background-size: 50% auto, cover, contain;
          background-repeat: no-repeat, repeat-x, space;
          filter: blur(2px) brightness(1.2) grayscale(50%) hue-rotate(30deg) drop-shadow(2px 3px 4px #00000080);
          box-shadow: inset 0 0 4px 2px rgba(0,0,0,.5), 4px 4px 8px black;
          border: 2px dashed rgb(10, 20, 30);
          border-radius: 10px 20% 5px 0;
          transform: perspective(200px) rotateY(20deg);
          transform-origin: 25% 75% 10px;
          mix-blend-mode: multiply;
          animation: slide 1s ease-in-out 0.25s infinite alternate both;
        }
        .title {
          color: #333; font-size: 18px; letter-spacing: 1px; text-transform: uppercase;
          text-decoration: underline wavy red; text-underline-offset: 3px;
          -webkit-text-stroke: 1px white; text-shadow: 1px 1px 2px black;
          text-indent: 10%; white-space: nowrap; text-overflow: ellipsis; overflow: hidden;
          width: 200px;
        }
        .mask { width: 80px; height: 40px; background: orange;
          mask-image: linear-gradient(to right, black, transparent) }
      </style>
      <div class='card'></div>
      <div class='title'>Hello scene document, rendered twice</div>
      <div class='mask'></div>";
    let options = CompileOptions {
        width: 320,
        height: 180,
        duration: 3.0,
        ..Default::default()
    };
    let compiled = compile(html, options).unwrap();
    let failures = mismatches("paint fixture", compiled);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn render_frames_matches_render_frame_in_order() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/html/native-motion.html");
    let compiled = compile_file(&path, CompileOptions::default()).unwrap();
    let document = Arc::new(export_scene(&compiled.director).unwrap());
    let renderer = Renderer::new(document, compiled.director.assets.loader.clone())
        .with_registry(registry())
        .with_render_mode(compiled.director.render_mode);
    let times = [1.25, 0.0, 0.5, 2.5, 0.75, 1.0];
    let frames = renderer.render_frames(&times).unwrap();
    for (time, frame) in times.iter().zip(frames) {
        assert_eq!(frame, renderer.render_frame(*time).unwrap(), "frame at {time}s");
    }
}

#[test]
fn warm_pool_matches_fresh_sessions_across_calls() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/html/text-decoration.html");
    let compiled = compile_file(&path, CompileOptions::default()).unwrap();
    let document = Arc::new(export_scene(&compiled.director).unwrap());
    let renderer = Renderer::new(document, compiled.director.assets.loader.clone())
        .with_registry(registry())
        .with_render_mode(compiled.director.render_mode);
    let mut pool = renderer.pool();
    let mut frames = Vec::new();
    for chunk in [[0.0, 0.1, 0.2, 0.3], [1.0, 1.1, 1.2, 1.3], [0.5, 0.6, 0.7, 0.8]] {
        pool.render_into(&chunk, &mut frames).unwrap();
        for (time, frame) in chunk.iter().zip(&frames) {
            assert_eq!(*frame, renderer.render_frame(*time).unwrap(), "frame at {time}s");
        }
    }
}

/// Per-frame MD5 of every decoded stream in `path`.
fn framemd5(path: &Path) -> String {
    let output = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-f", "framemd5", "-"])
        .output()
        .expect("FFmpeg runs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn document_export_decodes_like_the_live_export() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/html/native-motion.html");
    let temp = tempfile::tempdir().unwrap();
    let (live_path, document_path) = (temp.path().join("live.mp4"), temp.path().join("document.mp4"));
    let mut compiled = compile_file(&path, CompileOptions::default()).unwrap();
    compiled.director.encoder_mode = EncoderMode::Software;
    let document = Arc::new(export_scene(&compiled.director).unwrap());
    let renderer = Renderer::new(document, compiled.director.assets.loader.clone())
        .with_registry(registry())
        .with_render_mode(compiled.director.render_mode);
    render_export(&mut compiled.director, live_path.clone(), None, None, false).unwrap();
    renderer.export(&document_path, EncoderMode::Software).unwrap();
    assert_eq!(framemd5(&document_path), framemd5(&live_path));
    // One session painting on the calling thread, the GPU export path on a CPU raster.
    let session_path = temp.path().join("session.mp4");
    renderer.export_on(&session_path, EncoderMode::Software, None).unwrap();
    assert_eq!(framemd5(&session_path), framemd5(&live_path));
}

#[test]
fn director_export_at_half_scale_with_alpha_decodes_like_the_live_export() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/html/native-motion.html");
    let temp = tempfile::tempdir().unwrap();
    let (live_path, document_path) = (temp.path().join("live.webm"), temp.path().join("document.webm"));
    let mut compiled = compile_file(&path, CompileOptions::default()).unwrap();
    compiled.director.encoder_mode = EncoderMode::Software;
    export_director(&mut compiled.director, registry(), &document_path, true, 0.5, false, None).unwrap();
    let director = &compiled.director;
    assert_eq!((director.output_scale_x, director.output_scale_y, director.transparent_background), (1.0, 1.0, false));
    render_export_at_scale(&mut compiled.director, live_path.clone(), None, None, true, 0.5, false).unwrap();
    assert_eq!(framemd5(&document_path), framemd5(&live_path));
}

fn options(width: i32, height: i32) -> CompileOptions {
    CompileOptions {
        width,
        height,
        duration: 3.0,
        ..Default::default()
    }
}

/// A one-second FFV1 test pattern whose frames change every frame.
fn ffmpeg_clip(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("clip.mkv");
    let result = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i"])
        .args(["testsrc=s=64x64:r=30:d=1", "-c:v", "ffv1"])
        .arg(&path)
        .output()
        .expect("FFmpeg must be installed for native media tests");
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    path
}

#[test]
fn font_faces_render_from_the_document() {
    let html = "<style>
        @font-face { font-family: 'Display Face'; src: url(Poppins-Bold.ttf); }
        body { margin: 0; background: #fff }
        p { margin: 8px; width: 150px; font: 22px 'Display Face'; color: #000 }
      </style>
      <p>Faces travel with the scene document</p>";
    let options = CompileOptions {
        base_dir: Path::new(env!("CARGO_MANIFEST_DIR")).join("../director-core/assets/fonts"),
        ..options(160, 120)
    };
    let compiled = compile(html, options).unwrap();
    let document = export_scene(&compiled.director).unwrap();
    assert_eq!(document.fonts.len(), 1);
    assert_eq!(document.fonts[0].family, "Display Face");
    let failures = mismatches("font faces", compiled);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn smil_svg_renders_from_the_document() {
    let html = "<style>body{margin:0;background:#fff}</style>
        <svg width='64' height='32' viewBox='0 0 64 32'>
          <rect x='0' y='0' width='12' height='32' fill='#0000ff'>
            <animate attributeName='x' from='0' to='40' dur='2s' fill='freeze'/>
            <animateTransform attributeName='transform' type='rotate' from='0 6 16' to='90 6 16' dur='1s' repeatCount='indefinite'/>
          </rect>
          <text x='32' y='24' font-size='12' fill='#f00'>Hi
            <set attributeName='fill' to='#0f0' begin='1s' dur='1s'/>
          </text>
        </svg>";
    let failures = mismatches("smil", compile(html, options(64, 32)).unwrap());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn looped_video_renders_from_the_document() {
    let temp = tempfile::tempdir().unwrap();
    ffmpeg_clip(temp.path());
    let html = "<style>body{margin:0}video{width:64px;height:48px;object-fit:cover}</style>
        <video muted loop src='clip.mkv'></video><video muted loop src='clip.mkv'></video>";
    let compiled = compile(
        html,
        CompileOptions {
            base_dir: temp.path().into(),
            ..options(64, 96)
        },
    )
    .unwrap();
    let failures = mismatches("looped video", compiled);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn captures_render_from_the_document() {
    use base64::Engine as _;
    use director_html::{compile_capture, Capture, CapturedFrame, CapturedMedia, CapturedNode, CAPTURE_FORMAT};
    use std::collections::BTreeMap;

    let temp = tempfile::tempdir().unwrap();
    let clip = std::fs::read(ffmpeg_clip(temp.path())).unwrap();
    let assets = BTreeMap::from([
        ("clip.mkv".to_string(), "clip.mkv".to_string()),
        (
            "inline.mkv".to_string(),
            format!(
                "data:application/octet-stream;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(&clip)
            ),
        ),
    ]);
    let node = |tag: &str, id: &str, rect: [f32; 4], styles: &[(&str, &str)]| CapturedNode {
        tag: tag.into(),
        id: id.into(),
        rect,
        styles: styles.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        text: None,
        svg: None,
        src: None,
        shader: None,
        media: None,
        children: Vec::new(),
    };
    let video = |id: &str, src: &str, rect: [f32; 4], media_start: f64| CapturedNode {
        src: Some(src.into()),
        media: Some(CapturedMedia {
            muted: true,
            looped: true,
            hidden: false,
            volume: 1.0,
            media_start,
            playback_rate: 1.0,
            start: 0.0,
            end: 3.0,
        }),
        ..node("video", id, rect, &[("object-fit", "contain")])
    };
    let sample = |x: f32| {
        let mut title = node(
            "p",
            "title",
            [x, 70.0, 100.0, 20.0],
            &[("color", "rgb(20, 40, 200)"), ("font-size", "16px")],
        );
        title.text = Some("Captured".into());
        let mut icon = node("svg", "icon", [x + 100.0, 70.0, 20.0, 20.0], &[]);
        icon.svg = Some(
            "<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20'><circle cx='10' cy='10' r='8' fill='orange'/></svg>"
                .into(),
        );
        let mut card = node(
            "div",
            "card",
            [x, 64.0, 128.0, 32.0],
            &[("background-color", "rgb(230, 230, 230)"), ("border-top-left-radius", "6px")],
        );
        card.children = vec![title, icon];
        let mut root = node("body", "root", [0.0, 0.0, 160.0, 120.0], &[("background-color", "white")]);
        root.children = vec![
            video("a", "clip.mkv", [0.0, 0.0, 64.0, 64.0], 0.0),
            video("b", "inline.mkv", [80.0, 0.0, 64.0, 64.0], 0.25),
            card,
        ];
        root
    };
    let frames = vec![
        CapturedFrame { time: 0.0, root: sample(0.0) },
        CapturedFrame { time: 0.5, root: sample(0.0) },
        CapturedFrame { time: 1.0, root: sample(12.0) },
        CapturedFrame { time: 1.5, root: sample(24.0) },
        CapturedFrame { time: 2.0, root: sample(24.0) },
        CapturedFrame { time: 2.5, root: sample(32.0) },
    ];
    let capture = Capture {
        format: CAPTURE_FORMAT.into(),
        width: 160,
        height: 120,
        fps: 30.0,
        duration: 3.0,
        sample_rate: 2.0,
        fonts: Vec::new(),
        assets,
        frames,
    };
    let compiled = compile_capture(capture, temp.path()).unwrap();
    let failures = mismatches("capture", compiled);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn paper_shaders_render_from_the_document() {
    use director_html::{compile_capture, Capture, CapturedFrame, CapturedNode, CapturedShader, CAPTURE_FORMAT};
    use std::collections::BTreeMap;

    let root = CapturedNode {
        tag: "canvas".into(),
        id: "shader".into(),
        rect: [0.0, 0.0, 64.0, 64.0],
        styles: BTreeMap::new(),
        text: None,
        svg: None,
        src: None,
        shader: Some(CapturedShader {
            fragment: "#version 300 es
                 precision mediump float;
                 uniform vec4 u_color;
                 in vec2 v_objectUV;
                 out vec4 fragColor;
                 void main() { fragColor = vec4(u_color.rgb * v_objectUV.x, 1.0); }"
                .into(),
            uniforms: BTreeMap::from([("u_color".into(), vec![1.0, 0.0, 0.5, 1.0])]),
            images: BTreeMap::new(),
        }),
        media: None,
        children: Vec::new(),
    };
    let capture = Capture {
        format: CAPTURE_FORMAT.into(),
        width: 64,
        height: 64,
        fps: 30.0,
        duration: 3.0,
        sample_rate: 1.0,
        fonts: Vec::new(),
        assets: BTreeMap::new(),
        frames: (0..3).map(|i| CapturedFrame { time: i as f64, root: root.clone() }).collect(),
    };
    let compiled = compile_capture(capture, Path::new(".")).unwrap();
    let failures = mismatches("paper", compiled);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
