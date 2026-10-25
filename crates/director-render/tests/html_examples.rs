//! Compiled HTML paints the same frames from its scene document as from the
//! compiled `Director`.

use director_core::scene_export::export_scene;
use director_core::scene_import::ImportRegistry;
use director_html::{compile, compile_file, register_import, CompileOptions, CompiledHtml};
use director_render::{Frame, FrameSession, Renderer};
use std::path::Path;
use std::sync::Arc;

const TIMES: [f64; 4] = [0.0, 0.5, 1.25, 2.5];
const CHANNEL_DELTA: u8 = 16;
const MAX_FRACTION: f64 = 0.001;

fn registry() -> ImportRegistry {
    let mut registry = ImportRegistry::core();
    register_import(&mut registry);
    registry
}

fn differing_fraction(a: &Frame, b: &Frame) -> f64 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let different = a
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.rgba.as_chunks::<4>().0)
        .filter(|(x, y)| x.iter().zip(y.iter()).any(|(p, q)| p.abs_diff(*q) > CHANNEL_DELTA))
        .count();
    different as f64 / (a.width as f64 * a.height as f64)
}

/// Frames at [`TIMES`] whose document render differs from the compiled render.
fn mismatches(name: &str, compiled: CompiledHtml) -> Vec<String> {
    let document = Arc::new(export_scene(&compiled.director).unwrap());
    let renderer = Renderer::new(document, compiled.director.assets.loader.clone())
        .with_registry(registry())
        .with_render_mode(compiled.director.render_mode);
    let size = renderer.output_size();
    let mut imported = renderer.session().unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut original = FrameSession::new(compiled.director, size).unwrap();
    let mut failures = Vec::new();
    for time in TIMES {
        let expected = original.render(time).unwrap();
        let actual = imported.render(time).unwrap_or_else(|e| panic!("{name} at {time}s: {e}"));
        let fraction = differing_fraction(&expected, &actual);
        if fraction > MAX_FRACTION {
            failures.push(format!("{name} at {time}s: {fraction:.6} of pixels differ"));
        }
    }
    failures
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
        let compiled = compile_file(&path, CompileOptions::default()).unwrap_or_else(|e| panic!("{name}: {e}"));
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
    let frames = renderer.render_frames(&times);
    for (time, frame) in times.iter().zip(frames) {
        assert_eq!(frame.unwrap(), renderer.render_frame(*time).unwrap(), "frame at {time}s");
    }
}
