//! `<model shutter>`: the model's own shutter sweeping its clips and its
//! keyframed and GSAP camera across its samples against the composition's
//! sub-frame motion blur, a still model keeping one render, path traced skins
//! and morph targets, the scene document and the compile errors. GPU tests
//! skip on a machine without a Vulkan adapter.

use director_core::scene_export::export_scene;
use director_html::{compile, CompileOptions, CompiledHtml};
use director_render::render_frame;
use director_scene::{NodeBody, SceneDocument};
use std::path::PathBuf;
use std::sync::Arc;

const W: i32 = 160;
const H: i32 = 90;

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../director-3d/tests/fixtures")
        .join(name)
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

/// `src` over a red composition at 10 fps with `root` on the composition,
/// `style` and `attrs` on the model and `extra` after the composition.
fn page(src: &str, root: &str, style: &str, attrs: &str, extra: &str) -> String {
    format!(
        "<style>body{{margin:0}}#main{{background:rgb(255,0,0)}}model{{display:block;width:{W}px;height:{H}px;{style}}}</style>\
         <div id='main' data-composition-id='main' data-width='{W}' data-height='{H}' data-duration='2' data-fps='10' {root}>\
         <model id='m' src='{}' {attrs}></model></div>{extra}",
        fixture(src)
    )
}

fn build(source: &str) -> CompiledHtml {
    compile(source, options()).unwrap_or_else(|e| panic!("{source}: {e}"))
}

fn frame(source: &str, time: f64) -> Vec<u8> {
    let mut c = build(source);
    let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).unwrap();
    render_frame(&mut c.director, time, &mut surface).unwrap();
    let info = skia_safe::ImageInfo::new((W, H), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Unpremul, None);
    let mut pixels = vec![0; (W * H * 4) as usize];
    assert!(surface.read_pixels(&info, &mut pixels, W as usize * 4, (0, 0)));
    pixels
}

/// The pixels in columns `x0..x1` where a channel differs by more than 8.
fn far(a: &[u8], b: &[u8], (x0, x1): (i32, i32)) -> usize {
    (0..H)
        .flat_map(|y| (x0..x1).map(move |x| ((y * W + x) * 4) as usize))
        .filter(|&i| (0..3).any(|k| a[i + k].abs_diff(b[i + k]) > 8))
        .count()
}

const ORBIT: &str = "<style>@keyframes orbit{from{--camera-yaw:0deg}to{--camera-yaw:90deg}}</style>";
const GSAP: &str = "<script src='https://cdn.jsdelivr.net/npm/gsap@3.14.2/dist/gsap.min.js'></script>\
    <script>gsap.fromTo('#m', {'--camera-yaw': 0}, {'--camera-yaw': 90, duration: 2, ease: 'none'})</script>";

#[test]
fn the_model_shutter_matches_composition_motion_blur() {
    if !gpu() {
        return;
    }
    // The composition reference renders 32 sub-frames of 16 samples each;
    // the model's own shutter sweeps its 16 samples across the same span.
    let reference = "data-motion-blur='32' data-shutter-angle='360'";
    for (name, style, extra, time) in [
        ("turning clip", "", "", 0.75),
        ("keyframed camera", "animation:orbit 2s linear forwards", ORBIT, 1.5),
        ("GSAP camera", "", GSAP, 1.0),
    ] {
        let sharp = frame(&page("model.glb", "", style, "samples='16'", extra), time);
        let wide = frame(&page("model.glb", reference, style, "samples='16'", extra), time);
        let own = frame(&page("model.glb", "", style, "shutter='360'", extra), time);
        let (blur, miss) = (far(&sharp, &wide, (0, W)), far(&own, &wide, (0, W)));
        eprintln!("{name}: {blur} pixels of the sharp frame and {miss} of the model shutter's differ from the reference");
        assert!(blur > 300, "{name}: the reference blurs: {blur}");
        assert!(miss < 20, "{name}: the model shutter matches the reference: {miss}");
    }
}

#[test]
fn a_still_model_renders_once() {
    if !gpu() {
        return;
    }
    // The fixture's clips end at 2 s and hold, and the camera rests.
    let late = |attrs: &str| frame(&page("model.glb", "", "", attrs, "").replace("data-duration='2'", "data-duration='3'"), 2.5);
    assert!(late("") == late("shutter='360'"), "a still shutter keeps the sharp render");
    let moving = frame(&page("model.glb", "", "", "shutter='360'", ""), 0.75);
    assert!(far(&moving, &frame(&page("model.glb", "", "", "", ""), 0.75), (0, W)) > 100, "a moving shutter blurs");
}

#[test]
fn path_traced_skins_and_morph_targets_blur_with_the_shutter() {
    if !gpu() || !director_3d::traceable() {
        eprintln!("skipped: no ray tracing GPU");
        return;
    }
    // The column bends on the left and the ball squashes on the right while
    // the floor between them rests.
    let traced = |attrs: &str| frame(&page("rig.glb", "", "", &format!("paths='4' samples='16' denoise {attrs}"), ""), 1.0);
    let (sharp, blurred) = (traced(""), traced("shutter='360'"));
    let raster = |attrs: &str| frame(&page("rig.glb", "", "", &format!("samples='16' {attrs}"), ""), 1.0);
    let (raster_sharp, raster_blurred) = (raster(""), raster("shutter='360'"));
    for (name, columns) in [("column", (0, W / 2)), ("floor", (W / 2, W * 11 / 20)), ("ball", (W * 11 / 20, W))] {
        let (hero, raster) = (far(&sharp, &blurred, columns), far(&raster_sharp, &raster_blurred, columns));
        eprintln!("the {name} blurs {hero} pixels traced and {raster} rasterized");
        if name == "floor" {
            assert!(hero < 5 && raster < 5, "the resting floor stays sharp: {hero} traced, {raster} rasterized");
        } else {
            assert!(hero > 30, "the traced {name} blurs: {hero}");
            assert!(hero.abs_diff(raster) * 2 < raster, "the traced {name} blurs like the raster: {hero} against {raster}");
        }
    }
}

/// The model body of a scene document.
fn model_of(document: &mut SceneDocument) -> &mut director_scene::ModelBody {
    document
        .nodes
        .iter_mut()
        .find_map(|node| match &mut node.body {
            NodeBody::Html(html) => html.content.as_mut().and_then(|c| match &mut c.body {
                NodeBody::Model(model) => Some(model),
                _ => None,
            }),
            _ => None,
        })
        .expect("a model body")
}

fn import(document: SceneDocument) -> director_render::Renderer {
    let mut registry = director_core::scene_import::ImportRegistry::core();
    director_html::register_import(&mut registry);
    director_render::Renderer::new(Arc::new(document), Arc::new(director_core::DefaultAssetLoader)).with_registry(registry)
}

#[test]
fn the_scene_document_keeps_the_shutter() {
    let source = page("model.glb", "", "animation:orbit 2s linear forwards", "shutter='180'", ORBIT);
    let document = export_scene(&build(&source).director).unwrap();
    let mut parsed = SceneDocument::from_json(&document.to_json().unwrap()).unwrap();
    assert_eq!(parsed, document);
    assert_eq!(model_of(&mut parsed).shutter, 0.05, "180 degrees at 10 fps stays open 0.05 s");
    let plain = model_of(&mut export_scene(&build(&page("model.glb", "", "", "", "")).director).unwrap()).shutter;
    assert_eq!(plain, 0.0, "a plain model keeps a sharp shutter");
    if gpu() {
        let imported = import(parsed).render_frame(1.5).unwrap();
        assert_eq!(imported.rgba, frame(&source, 1.5), "the imported shutter renders as compiled");
    }
}

#[test]
fn malformed_shutters_fail_to_compile() {
    let error = |source: &str| compile(source, options()).err().expect("a compile error").to_string();
    for value in ["400", "-1"] {
        let e = error(&page("model.glb", "", "", &format!("shutter='{value}'"), ""));
        assert!(e.contains(&format!("shutter '{value}' must be degrees from 0 to 360")), "{e}");
    }
    let e = error(&page("model.glb", "", "", "shutter='wide'", ""));
    assert!(e.contains("shutter must be numeric"), "{e}");
    let e = error(&page("model.glb", "", "", "shutter='180' track='plate.mp4'", ""));
    assert!(e.contains("a tracked model blurs with track-shutter, so it cannot take shutter"), "{e}");
    let e = error(&page("model.glb", "data-motion-blur='4'", "", "shutter='180'", ""));
    assert!(e.contains("data-motion-blur on the composition already blurs the model, so it cannot take shutter"), "{e}");
}
