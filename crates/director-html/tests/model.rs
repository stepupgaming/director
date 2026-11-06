//! `<model>`: a glTF scene rendered by `director-3d`, its clips on the element
//! clock, its camera driven by CSS and GSAP, and the scene document round
//! trip. Each test skips on a machine without a Vulkan adapter.

use director_core::scene_export::export_scene;
use director_html::{compile, CompileOptions, CompiledHtml};
use director_render::render_frame;
use director_scene::{NodeBody, SceneDocument};
use std::path::PathBuf;
use std::sync::Arc;

const W: i32 = 160;
const H: i32 = 90;

fn fixture() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../director-3d/tests/fixtures/model.glb")
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

fn gpu() -> bool {
    let ok = director_3d::available();
    if !ok {
        eprintln!("skipped: no Vulkan adapter for director-3d");
    }
    ok
}

fn options() -> CompileOptions {
    CompileOptions {
        width: W,
        height: H,
        duration: 2.0,
        gsap_dir: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools/node_modules/gsap/dist")),
        ..Default::default()
    }
}

/// A full-frame model over a red composition, with `style` on the model and `extra` after it.
fn page(style: &str, extra: &str) -> String {
    format!(
        "<style>body{{margin:0}}#main{{background:rgb(255,0,0)}}model{{display:block;width:{W}px;height:{H}px;{style}}}</style>\
         <div id='main' data-composition-id='main' data-width='{W}' data-height='{H}' data-duration='2'>\
         <model id='m' src='{}'></model></div>{extra}",
        fixture()
    )
}

fn build(source: &str) -> CompiledHtml {
    compile(source, options()).unwrap_or_else(|e| panic!("{source}: {e}"))
}

fn frame(c: &mut CompiledHtml, time: f64) -> Vec<u8> {
    let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).unwrap();
    render_frame(&mut c.director, time, &mut surface).unwrap();
    let info = skia_safe::ImageInfo::new((W, H), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Unpremul, None);
    let mut pixels = vec![0; (W * H * 4) as usize];
    assert!(surface.read_pixels(&info, &mut pixels, W as usize * 4, (0, 0)));
    pixels
}

fn pixel(p: &[u8], x: i32, y: i32) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [p[i], p[i + 1], p[i + 2]]
}

/// Pixels whose channels differ by more than 2.
fn changed(a: &[u8], b: &[u8]) -> usize {
    a.chunks(4).zip(b.chunks(4)).filter(|(p, q)| (0..3).any(|k| p[k].abs_diff(q[k]) > 2)).count()
}

#[test]
fn a_model_paints_its_scene_over_a_transparent_sky() {
    if !gpu() {
        return;
    }
    let mut c = build(&page("", ""));
    let pixels = frame(&mut c, 0.0);
    // The sky above the ground shows the red page through the model, plus a
    // faint additive bloom glow.
    let page = |p: &[u8]| p[0] >= 250 && p[1] < 16 && p[2] < 16;
    assert!(page(&pixel(&pixels, 2, 2)), "{:?}", pixel(&pixels, 2, 2));
    let red = pixels.chunks(4).filter(|p| page(p)).count();
    let total = (W * H) as usize;
    assert!(red > total / 20 && red < total * 9 / 10, "{red} of {total} pixels show the page");
}

#[test]
fn clips_follow_the_element_clock_in_any_render_order() {
    if !gpu() {
        return;
    }
    let mut c = build(&page("", ""));
    let late = frame(&mut c, 1.5);
    let early = frame(&mut c, 0.0);
    let again = frame(&mut c, 1.5);
    assert!(changed(&early, &late) > 200, "the cube turns and the sphere rises");
    assert_eq!(changed(&late, &again), 0, "a frame depends only on its time");
}

#[test]
fn css_and_gsap_drive_the_camera() {
    if !gpu() {
        return;
    }
    let front = frame(&mut build(&page("", "")), 0.0);
    let turned = frame(&mut build(&page("--camera-yaw:30deg;--camera-pitch:10deg", "")), 0.0);
    assert!(changed(&front, &turned) > 500, "yaw and pitch move the camera");
    let closer = frame(&mut build(&page("--camera-distance:0.7", "")), 0.0);
    assert!(changed(&front, &closer) > 500, "distance moves the camera");

    let gsap = page(
        "",
        "<script src='https://cdn.jsdelivr.net/npm/gsap@3.14.2/dist/gsap.min.js'></script>\
         <script>gsap.fromTo('#m', {'--camera-yaw': 0, '--camera-pitch': 0}, {'--camera-yaw': 30, '--camera-pitch': 10, duration: 1, ease: 'none'})</script>",
    );
    let mut c = build(&gsap);
    let timeline = c.timeline.as_ref().expect("GSAP timeline");
    assert!(timeline.tracks.iter().any(|t| t.property == "--camera-yaw"), "{:?}", timeline.tracks);
    assert!(changed(&frame(&mut c, 0.0), &front) < 20, "the tween starts at the front view");
    let mut still = build(&page("--camera-yaw:30deg;--camera-pitch:10deg", ""));
    assert!(changed(&frame(&mut c, 1.0), &frame(&mut still, 1.0)) < 20, "the tween ends at the CSS view");

    let keyframes = page(
        "animation:orbit 1s linear forwards",
        "<style>@keyframes orbit{from{--camera-yaw:0deg}to{--camera-yaw:30deg;--camera-pitch:10deg}}</style>",
    );
    let mut c = build(&keyframes);
    assert!(changed(&frame(&mut c, 1.5), &frame(&mut still, 1.5)) < 20, "CSS keyframes reach the same view");
}

#[test]
fn the_scene_document_renders_what_the_preview_shows() {
    if !gpu() {
        return;
    }
    let mut compiled = build(&page("--camera-yaw:-20deg", ""));
    let document = export_scene(&compiled.director).unwrap();
    let parsed = SceneDocument::from_json(&document.to_json().unwrap()).unwrap();
    assert_eq!(parsed, document);
    let model = parsed
        .nodes
        .iter()
        .find_map(|node| match &node.body {
            NodeBody::Html(html) => html.content.as_ref().and_then(|c| match &c.body {
                NodeBody::Model(model) => Some(model.clone()),
                _ => None,
            }),
            _ => None,
        })
        .expect("a model body");
    assert_eq!(model.view.yaw, -20.0);
    let asset = parsed.asset(&model.asset).expect("model asset");
    assert_eq!(asset.media_type, "model/gltf-binary");

    let mut registry = director_core::scene_import::ImportRegistry::core();
    director_html::register_import(&mut registry);
    let renderer = director_render::Renderer::new(Arc::new(parsed), Arc::new(director_core::DefaultAssetLoader))
        .with_registry(registry);
    for time in [0.0, 1.25] {
        let imported = renderer.render_frame(time).unwrap();
        let preview = frame(&mut compiled, time);
        assert_eq!(changed(&imported.rgba, &preview), 0, "t={time}");
    }
}

#[test]
fn a_model_without_a_glb_is_an_error() {
    let source = "<model src='data:application/octet-stream;base64,AAAA'></model>";
    let error = compile(source, options()).err().expect("an invalid model").to_string();
    assert!(error.contains("glTF binary"), "{error}");
}
