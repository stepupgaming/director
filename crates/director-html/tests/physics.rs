//! `<physics>`: rigid bodies baked once inside `<model>` and played back from
//! their stored poses. Frames render the same in any order, the Live and
//! Export levels agree, a model shutter blurs falling bodies and leaves
//! rested ones sharp, a shadow catcher catches the bodies' shadows, the scene
//! document keeps the inputs and the bake hash and renders the same frame,
//! the Hero level traces the bodies, and bad input fails to compile. The
//! model is `physics.glb` from `director-3d`'s fixtures, its `Ground` a
//! static body. GPU tests skip on a machine without a Vulkan adapter.

use director_core::scene_export::export_scene;
use director_html::{compile, CompileOptions, CompiledHtml};
use director_render::render_frame;
use director_scene::{NodeBody, SceneDocument, SpawnEmitter};
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
    CompileOptions { width: W, height: H, duration: 2.0, ..Default::default() }
}

/// `physics.glb` with `attrs` over a `page` coloured composition at 30 fps,
/// lit by a sun, its `<physics>` taking `physics` and holding the static
/// ground and `bodies`.
fn page(page: &str, attrs: &str, physics: &str, bodies: &str) -> String {
    format!(
        "<style>body{{margin:0}}#main{{background:{page}}}model{{display:block;width:{W}px;height:{H}px}}</style>\
         <div id='main' data-composition-id='main' data-width='{W}' data-height='{H}' data-duration='2' data-fps='30'>\
         <model id='m' src='{}' {attrs}><light type='sun' position='2 5 3' target='0 0 0' intensity='3000'></light>\
         <physics duration='2' {physics}><rigid node='Ground' type='static'></rigid>{bodies}</physics></model></div>",
        fixture("physics.glb")
    )
}

/// Red boxes 0.25 m across, all born at once 1.5 m up, so they fall, bounce
/// and rest within the two seconds.
const BOXES: &str = "<spawner shape='box' name='Box' size='0.25' color='#ff2020' count='6' duration='0' \
                     position='0 1.5 0' emitter='box' extent='1 0.6 0.6' spin-jitter='180' restitution='0.3'></spawner>";

fn build(source: &str) -> CompiledHtml {
    compile(source, options()).unwrap_or_else(|e| panic!("{source}: {e}"))
}

fn render(c: &mut CompiledHtml, time: f64) -> Vec<u8> {
    let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).unwrap();
    render_frame(&mut c.director, time, &mut surface).unwrap();
    let info = skia_safe::ImageInfo::new((W, H), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Unpremul, None);
    let mut pixels = vec![0; (W * H * 4) as usize];
    assert!(surface.read_pixels(&info, &mut pixels, W as usize * 4, (0, 0)));
    pixels
}

fn frame(source: &str, time: f64) -> Vec<u8> {
    render(&mut build(source), time)
}

/// Pixels where a channel differs by more than 8.
fn far(a: &[u8], b: &[u8]) -> usize {
    a.chunks(4).zip(b.chunks(4)).filter(|(p, q)| (0..3).any(|k| p[k].abs_diff(q[k]) > 8)).count()
}

/// Pixels of the red boxes.
fn red(rgba: &[u8]) -> Vec<bool> {
    rgba.chunks(4).map(|p| p[0] > 60 && p[0] as u16 > 2 * (p[1] as u16).max(p[2] as u16)).collect()
}

fn count(mask: &[bool]) -> usize {
    mask.iter().filter(|&&m| m).count()
}

fn iou(a: &[bool], b: &[bool]) -> f64 {
    let both = a.iter().zip(b).filter(|(a, b)| **a && **b).count();
    let either = a.iter().zip(b).filter(|(a, b)| **a || **b).count();
    both as f64 / either.max(1) as f64
}

#[test]
fn frames_render_the_same_in_any_order() {
    if !gpu() {
        return;
    }
    let mut c = build(&page("black", "", "", BOXES));
    let times: Vec<f64> = (0..60).map(|f| f as f64 / 30.0).collect();
    let forward: Vec<Vec<u8>> = times.iter().map(|&t| render(&mut c, t)).collect();
    let backward: Vec<Vec<u8>> = times.iter().rev().map(|&t| render(&mut c, t)).collect();
    for (f, (a, b)) in forward.iter().zip(backward.iter().rev()).enumerate() {
        assert!(a == b, "frame {f} renders the same in either order");
    }
    let boxes: Vec<usize> = [0, 10, 59].iter().map(|&f| count(&red(&forward[f]))).collect();
    eprintln!("red pixels at frames 0, 10 and 59: {boxes:?}");
    assert!(boxes.iter().all(|&n| n > 20), "the boxes show: {boxes:?}");
    assert!(far(&forward[5], &forward[10]) > 20, "the boxes fall");
    assert!(far(&forward[58], &forward[59]) == 0, "the boxes rest by the end");
}

#[test]
fn the_live_and_export_levels_agree() {
    if !gpu() {
        return;
    }
    let live = frame(&page("black", "", "", BOXES), 1.2);
    let export = frame(&page("black", "samples='16'", "", BOXES), 1.2);
    let (l, e) = (red(&live), red(&export));
    let overlap = iou(&l, &e);
    eprintln!("Live shows {} box pixels, Export {}, overlap {overlap:.2}", count(&l), count(&e));
    assert!(count(&l) > 20 && overlap > 0.85, "the levels place the bodies alike");
}

#[test]
fn a_shutter_blurs_falling_bodies_and_leaves_rested_ones_sharp() {
    if !gpu() {
        return;
    }
    let blurred = |t: f64| frame(&page("black", "shutter='360'", "", BOXES), t);
    // At 0.35 s the boxes fall at 3.4 m/s, 0.11 m in the open 1/30 s; the
    // sharp frame takes the shutter's 16 jittered samples at one time.
    let falling = frame(&page("black", "samples='16'", "", BOXES), 0.35);
    let moving = far(&falling, &blurred(0.35));
    eprintln!("the shutter changes {moving} pixels of falling boxes");
    assert!(moving > 30, "falling boxes streak");
    // With every body at rest across the open shutter the model renders
    // once, as the Live level does.
    assert!(blurred(1.95) == frame(&page("black", "", "", BOXES), 1.95), "rested boxes render as one sharp frame");
}

#[test]
fn a_catcher_catches_the_bodies_shadows() {
    if !gpu() {
        return;
    }
    // Over a grey page, the catcher ground shows the page except where the
    // red boxes' shadows fall, as neutral darker grey.
    let source = page("rgb(128,128,128)", "shadow-catcher='Ground'", "", BOXES);
    let mut c = build(&source);
    let shadows = |rgba: &[u8]| {
        rgba.chunks(4).filter(|p| p[0] < 110 && p[0].abs_diff(p[1]) < 12 && p[1].abs_diff(p[2]) < 12).count()
    };
    let rested = render(&mut c, 1.95);
    let (s, boxes) = (shadows(&rested), count(&red(&rested)));
    eprintln!("{s} shadow pixels under {boxes} box pixels");
    assert!(boxes > 20 && s > 20, "the ground catches the boxes' shadows");
    let open = rested.chunks(4).filter(|p| p[..3].iter().all(|c| c.abs_diff(128) <= 2)).count();
    assert!(open > (W * H) as usize / 2, "elsewhere the page shows through the ground: {open}");
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
            NodeBody::Model(model) => Some(model),
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
fn the_scene_document_keeps_the_inputs_and_the_bake() {
    let source = page("black", "", "seed='9' rate='90' gravity='0 -9.8 0'", BOXES);
    let document = export_scene(&build(&source).director).unwrap();
    let mut parsed = SceneDocument::from_json(&document.to_json().unwrap()).unwrap();
    assert_eq!(parsed, document);
    let physics = model_of(&mut parsed).physics.clone().expect("the physics inputs");
    assert_eq!((physics.seed, physics.rate, physics.gravity), (9, 90.0, [0.0, -9.8, 0.0]));
    assert_eq!(physics.bodies.len(), 1);
    assert_eq!(physics.bodies[0].node.as_deref(), Some("Ground"));
    let spawner = &physics.spawners[0];
    assert_eq!((spawner.count, spawner.emitter, spawner.body.name.as_deref()), (6, SpawnEmitter::Box, Some("Box")));
    assert_eq!(physics.bake.as_ref().map(String::len), Some(16), "the bake hash is written");
    if gpu() {
        let imported = import(parsed).render_frame(1.2).unwrap();
        assert_eq!(imported.rgba, frame(&source, 1.2), "the imported bodies render as compiled");
    }
}

#[test]
fn the_hero_level_traces_the_bodies() {
    if !gpu() || !director_3d::traceable() {
        return;
    }
    let raster = red(&frame(&page("black", "", "", BOXES), 1.2));
    let traced = red(&frame(&page("black", "paths='4'", "", BOXES), 1.2));
    let overlap = iou(&raster, &traced);
    eprintln!("raster {} box pixels, traced {}, overlap {overlap:.2}", count(&raster), count(&traced));
    assert!(count(&traced) > 20 && overlap > 0.7, "the traced bodies stand where the rasterized ones do");
}

#[test]
fn bad_physics_fails_to_compile() {
    for (physics, bodies, error) in [
        ("rate='5'", "", "rate 5 must be from 10"),
        ("seed='-1'", "", "seed must be a whole number"),
        ("gravity='0 -9'", "", "gravity '0 -9' must be three numbers"),
        ("", "<rigid node='Nowhere'></rigid>", "'Nowhere' names no node"),
        ("", "<rigid node='Crate.0' collider='trimesh'></rigid>", "trimesh needs a static body"),
        ("", "<rigid node='Pusher'></rigid>", "animated, so it cannot be dynamic"),
        ("", "<rigid shape='cone'></rigid>", "shape 'cone' must be"),
        ("", "<rigid shape='box' size='1 2 3 4'></rigid>", "size '1 2 3 4'"),
        ("", "<rigid node='Crate.0' type='floating'></rigid>", "type 'floating'"),
        ("", "<rigid node='Crate.0' shape='box'></rigid>", "a node or a shape"),
        ("", "<rigid node='Crate.0' restitution='2'></rigid>", "restitution from 0 to 1"),
        ("", "<spawner node='Coin' count='0'></spawner>", "count must be"),
        ("", "<spawner node='Coin' emitter='cone'></spawner>", "emitter 'cone'"),
        ("", "<physics></physics>", "cannot hold another <physics>"),
    ] {
        let source = page("black", "", physics, bodies);
        let e = compile(&source, options()).err().unwrap_or_else(|| panic!("{physics} {bodies} compiles")).to_string();
        assert!(e.contains(error), "{physics} {bodies}: {e}");
    }
    let twice = page("black", "", "", "").replace("</model>", "<physics><rigid shape='box'></rigid></physics></model>");
    let e = compile(&twice, options()).err().expect("two <physics> compile").to_string();
    assert!(e.contains("one <physics>"), "{e}");
    let catcher = page("black", "shadow-catcher='Ground Nope'", "", BOXES);
    let e = compile(&catcher, options()).err().expect("an unknown catcher compiles").to_string();
    assert!(e.contains("'Nope' names no node"), "{e}");
}
