//! Compiled HTML exports into a scene document that survives JSON.

use base64::Engine as _;
use director_core::scene_export::export_scene;
use director_html::{
    compile, compile_capture, compile_file, Capture, CapturedFrame, CapturedMedia, CapturedNode,
    CapturedShader, CompileOptions, CompiledHtml, CAPTURE_FORMAT,
};
use director_scene::html::{BackgroundLayer, CssFilter, CssValue, GradientShape, SmilEffect};
use director_scene::{AssetSource, NodeBody, SceneDocument};
use std::collections::BTreeMap;
use std::path::Path;

fn round_trip(compiled: &CompiledHtml) -> SceneDocument {
    let document = export_scene(&compiled.director).unwrap();
    let json = document.to_json().unwrap();
    let parsed = SceneDocument::from_json(&json).unwrap();
    assert_eq!(parsed, document);
    parsed
}

fn bodies(document: &SceneDocument) -> impl Iterator<Item = &NodeBody> {
    document.nodes.iter().map(|node| &node.body)
}

fn html_bodies(document: &SceneDocument) -> impl Iterator<Item = &director_scene::HtmlBody> {
    bodies(document).filter_map(|body| match body {
        NodeBody::Html(html) => Some(html.as_ref()),
        _ => None,
    })
}

fn options(width: i32, height: i32) -> CompileOptions {
    CompileOptions {
        width,
        height,
        duration: 2.0,
        ..Default::default()
    }
}

#[test]
fn every_html_example_round_trips() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/html");
    let mut count = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("html") {
            continue;
        }
        let compiled = compile_file(&path, CompileOptions::default())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let document = round_trip(&compiled);
        assert!(
            html_bodies(&document).next().is_some(),
            "{} exported no HTML boxes",
            path.display()
        );
        count += 1;
    }
    assert!(count >= 4, "found {count} HTML examples");
}

#[test]
fn paint_gradients_filters_and_animations_round_trip() {
    let png = {
        let mut surface = skia_safe::surfaces::raster_n32_premul((4, 4)).unwrap();
        surface.canvas().clear(skia_safe::Color::from_rgb(0, 128, 255));
        let image = surface.image_snapshot();
        let data = image
            .encode(None, skia_safe::EncodedImageFormat::PNG, 100)
            .unwrap();
        base64::engine::general_purpose::STANDARD.encode(data.as_bytes())
    };
    let html = format!(
        "<style>
           body {{ margin: 0 }}
           @keyframes slide {{
             from {{ transform: translateX(0px) rotate(0deg); background-color: red }}
             50% {{ animation-timing-function: steps(4, jump-start) }}
             to {{ transform: translateX(40px) rotate(90deg); background-color: blue }}
           }}
           .card {{
             width: 120px; height: 60px;
             background-image: linear-gradient(to bottom right, red, 30%, blue calc(50% + 4px)),
               radial-gradient(circle closest-side at 25% 75%, white, black),
               conic-gradient(from 45deg at 50% 50%, red, green),
               url(data:image/png;base64,{png});
             background-size: 50% auto, cover, contain, 10px 10px;
             background-repeat: no-repeat, repeat-x, space, round;
             filter: blur(2px) brightness(1.2) grayscale(50%) hue-rotate(30deg) drop-shadow(2px 3px 4px #00000080);
             box-shadow: inset 0 0 4px 2px rgba(0,0,0,.5), 4px 4px 8px black;
             border: 2px dashed rgb(10, 20, 30);
             border-radius: 10px 20% 5px 0;
             transform: perspective(200px) rotateY(20deg);
             transform-origin: 25% 75% 10px;
             mix-blend-mode: multiply;
             animation: slide 1s ease-in-out 0.25s infinite alternate both;
           }}
           .title {{
             color: #333; font-size: 18px; letter-spacing: 1px; text-transform: uppercase;
             text-decoration: underline wavy red; text-underline-offset: 3px;
             -webkit-text-stroke: 1px white; text-shadow: 1px 1px 2px black;
             text-indent: 10%; white-space: nowrap; text-overflow: ellipsis; overflow: hidden;
           }}
         </style>
         <div class='card'></div>
         <div class='title'>Hello scene document</div>"
    );
    let compiled = compile(&html, options(320, 180)).unwrap();
    let document = round_trip(&compiled);

    let card = html_bodies(&document)
        .find(|body| body.paint.layers.len() == 4)
        .expect("the card exports its four background layers");
    assert!(matches!(
        &card.paint.layers[0],
        BackgroundLayer::Gradient(g) if matches!(g.shape, GradientShape::Linear { .. }) && g.colors.contains(&None)
    ));
    assert!(matches!(
        &card.paint.layers[1],
        BackgroundLayer::Gradient(g) if matches!(g.shape, GradientShape::Radial { .. })
    ));
    assert!(matches!(
        &card.paint.layers[2],
        BackgroundLayer::Gradient(g) if matches!(g.shape, GradientShape::Conic { .. })
    ));
    let BackgroundLayer::Image { asset } = &card.paint.layers[3] else {
        panic!("fourth layer is {:?}", card.paint.layers[3]);
    };
    assert!(document.assets.iter().any(|record| &record.id == asset));
    assert_eq!(card.paint.filters.len(), 5);
    assert!(matches!(card.paint.filters[4], CssFilter::DropShadow { .. }));
    assert_eq!(card.paint.shadows.len(), 2);
    assert_eq!(card.paint.blend_mode, "multiply");
    assert!(card.paint.transform.contains("rotateY"));
    let animation = &card.animations[0];
    assert_eq!(animation.iteration_count, None);
    assert_eq!(animation.direction, "alternate");
    assert!(animation.channels.iter().any(|channel| channel
        .keyframes
        .iter()
        .any(|k| matches!(&k.value, CssValue::Transform(t) if t.contains("translate")))));

    let title = html_bodies(&document)
        .find(|body| body.content.is_some())
        .expect("the title exports its text content");
    assert!(title.paint.text_stroke_width > 0.0);
    assert!(title.paint.decoration.underline);
    assert!(matches!(
        &title.content.as_ref().unwrap().body,
        NodeBody::Text(text) if text.spans.iter().any(|span| span.text == "HELLO SCENE DOCUMENT")
    ));
}

#[test]
fn smil_svg_round_trips() {
    let html = "<style>body{margin:0}</style>
        <svg width='64' height='32' viewBox='0 0 64 32'>
          <rect x='0' y='0' width='12' height='32' fill='#0000ff'>
            <animate attributeName='x' from='0' to='40' dur='2s' fill='freeze'/>
            <animateTransform attributeName='transform' type='rotate' from='0 6 16' to='90 6 16' dur='1s' repeatCount='indefinite'/>
          </rect>
        </svg>";
    let compiled = compile(html, options(64, 32)).unwrap();
    let document = round_trip(&compiled);
    let smil = html_bodies(&document)
        .find_map(|body| match &body.content.as_ref()?.body {
            NodeBody::SvgSmil(smil) => Some(smil),
            _ => None,
        })
        .expect("the svg exports its SMIL body");
    assert_eq!(smil.animations.len(), 2);
    assert!(smil
        .animations
        .iter()
        .any(|a| matches!(a.effect, SmilEffect::Transform { .. })));
}

#[test]
fn looped_video_round_trips() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("clip.mkv"), ffmpeg_clip()).unwrap();
    let html = "<style>body{margin:0}video{width:64px;height:64px}</style><video muted loop src='clip.mkv'></video>";
    let compiled = compile(
        html,
        CompileOptions {
            base_dir: temp.path().into(),
            ..options(64, 64)
        },
    )
    .unwrap();
    let document = round_trip(&compiled);
    let video = html_bodies(&document)
        .find_map(|body| match &body.content.as_ref()?.body {
            NodeBody::HtmlVideo(video) => Some(video),
            _ => None,
        })
        .expect("the video exports an HTML video body");
    assert!(video.looped);
    assert!(video.duration > 0.0);
    let asset = document.assets.iter().find(|asset| asset.id == video.video.asset).unwrap();
    assert_eq!(
        asset.source,
        AssetSource::Path {
            path: "clip.mkv".into()
        }
    );
}

#[test]
fn audio_files_export_as_path_assets() {
    let temp = tempfile::tempdir().unwrap();
    let tone = ffmpeg_tone();
    std::fs::write(temp.path().join("tone.wav"), &tone).unwrap();
    let html = format!(
        "<audio src='tone.wav' data-start='0' data-duration='1' data-media-start='0.25'></audio>         <audio src='data:audio/wav;base64,{}' data-start='0' data-duration='1'></audio>",
        base64::engine::general_purpose::STANDARD.encode(&tone)
    );
    let compiled = compile(
        &html,
        CompileOptions {
            base_dir: temp.path().into(),
            ..options(64, 64)
        },
    )
    .unwrap();
    let document = round_trip(&compiled);
    let [file, inline] = document.audio.tracks.as_slice() else {
        panic!("expected two audio tracks");
    };
    let source = |id: &str| &document.assets.iter().find(|asset| asset.id == id).unwrap().source;
    assert_eq!(
        source(&file.asset),
        &AssetSource::Path {
            path: "tone.wav".into()
        }
    );
    assert_eq!(file.source_start, 0.25);
    assert!(matches!(source(&inline.asset), AssetSource::Embedded { .. }));
    assert_eq!(inline.source_start, 0.0);
}

#[test]
fn captures_round_trip_with_shared_samples() {
    let src = "clip.mkv";
    let assets = BTreeMap::from([(
        src.to_string(),
        format!(
            "data:application/octet-stream;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(ffmpeg_clip())
        ),
    )]);
    let video = |fit: &str| CapturedNode {
        tag: "video".into(),
        id: "picture".into(),
        rect: [0.0, 0.0, 64.0, 64.0],
        styles: BTreeMap::from([("object-fit".into(), fit.into())]),
        text: None,
        svg: None,
        src: Some(src.into()),
        shader: None,
        media: Some(CapturedMedia {
            muted: true,
            looped: false,
            hidden: false,
            volume: 1.0,
            media_start: 0.0,
            playback_rate: 1.0,
            start: 0.0,
            end: 1.0,
        }),
        children: Vec::new(),
    };
    let frames = vec![
        CapturedFrame { time: 0.0, root: video("fill") },
        CapturedFrame { time: 0.5, root: video("fill") },
        CapturedFrame { time: 1.0, root: video("cover") },
    ];
    let compiled = compile_capture(capture(assets, 2.0, frames), Path::new(".")).unwrap();
    let document = round_trip(&compiled);
    let capture = bodies(&document)
        .find_map(|body| match body {
            NodeBody::HtmlCapture(capture) => Some(capture),
            _ => None,
        })
        .expect("the capture exports a capture body");
    assert_eq!(capture.frames.len(), 3);
    assert_eq!(capture.frames, vec![0, 0, 1]);
    assert!(capture.graphs.iter().any(|graph| graph
        .nodes
        .iter()
        .any(|node| matches!(node.body, NodeBody::HtmlVideo(_)) || html_content_is_video(&node.body))));
}

#[test]
fn paper_shader_round_trips() {
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
                 void main() { fragColor = u_color; }"
                .into(),
            uniforms: BTreeMap::from([("u_color".into(), vec![1.0, 0.0, 0.0, 1.0])]),
            images: BTreeMap::new(),
        }),
        media: None,
        children: Vec::new(),
    };
    let compiled = compile_capture(
        capture(BTreeMap::new(), 1.0, vec![CapturedFrame { time: 0.0, root }]),
        Path::new("."),
    )
    .unwrap();
    let document = round_trip(&compiled);
    let paper = bodies(&document)
        .filter_map(|body| match body {
            NodeBody::HtmlCapture(capture) => Some(capture),
            _ => None,
        })
        .flat_map(|capture| capture.graphs.iter().flat_map(|graph| graph.nodes.iter()))
        .find_map(|node| match &node.body {
            NodeBody::Html(html) => html.paper.as_ref(),
            _ => None,
        })
        .expect("the canvas exports its Paper shader");
    assert_eq!(paper.uniforms["u_color"], vec![1.0, 0.0, 0.0, 1.0]);
}

fn html_content_is_video(body: &NodeBody) -> bool {
    matches!(body, NodeBody::Html(html) if matches!(
        html.content.as_ref().map(|content| &content.body),
        Some(NodeBody::HtmlVideo(_))
    ))
}

fn ffmpeg_clip() -> Vec<u8> {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("clip.mkv");
    let result = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i"])
        .args(["color=c=red:s=64x64:r=30:d=0.5", "-c:v", "ffv1"])
        .arg(&path)
        .output()
        .expect("FFmpeg must be installed for native media compiler tests");
    assert!(
        result.status.success(),
        "fixture generation failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    std::fs::read(path).unwrap()
}

fn ffmpeg_tone() -> Vec<u8> {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tone.wav");
    let result = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i"])
        .args(["sine=frequency=440:sample_rate=48000:duration=0.5", "-c:a", "pcm_s16le"])
        .arg(&path)
        .output()
        .expect("FFmpeg must be installed for native media compiler tests");
    assert!(
        result.status.success(),
        "fixture generation failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    std::fs::read(path).unwrap()
}

fn capture(assets: BTreeMap<String, String>, sample_rate: f64, frames: Vec<CapturedFrame>) -> Capture {
    Capture {
        format: CAPTURE_FORMAT.into(),
        width: 64,
        height: 64,
        fps: 30.0,
        duration: frames.len() as f64 / sample_rate,
        sample_rate,
        fonts: Vec::new(),
        assets,
        frames,
    }
}
