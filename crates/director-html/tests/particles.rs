//! `<particles>`: closed-form emitters inside `<model>`. Frames render the
//! same in any order, the seed decides the particles, the Live and Export
//! levels agree, a model shutter stretches fast particles, emissive particles
//! bloom over a background, a transparent sky keeps the page, surface and
//! mesh emitters, the scene document and the compile errors. GPU tests skip
//! on a machine without a Vulkan adapter.

use director_core::scene_export::export_scene;
use director_html::{compile, CompileOptions, CompiledHtml};
use director_render::render_frame;
use director_scene::{NodeBody, ParticleEmitter, SceneDocument};
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

/// A model with `attrs` holding one `<particles>` with `particles`, over a
/// `page` coloured composition at 30 fps.
fn page(page: &str, attrs: &str, particles: &str) -> String {
    format!(
        "<style>body{{margin:0}}#main{{background:{page}}}model{{display:block;width:{W}px;height:{H}px}}</style>\
         <div id='main' data-composition-id='main' data-width='{W}' data-height='{H}' data-duration='2' data-fps='30'>\
         <model id='m' {attrs}><particles {particles}></particles></model></div>"
    )
}

/// Sparks: a fountain from the origin that gravity pulls back down.
const SPARKS: &str = "rate='60' lifetime='1' speed='2.5' spread='35' gravity='0 -4 0' size='0.06' color='#ffd080' seed='7'";

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

/// Pixels brighter than black on any channel by 20, the particles over a
/// black page.
fn lit(rgba: &[u8]) -> usize {
    rgba.chunks(4).filter(|p| p[..3].iter().any(|c| *c >= 20)).count()
}

/// Pixels where a channel differs by more than 8.
fn far(a: &[u8], b: &[u8]) -> usize {
    a.chunks(4).zip(b.chunks(4)).filter(|(p, q)| (0..3).any(|k| p[k].abs_diff(q[k]) > 8)).count()
}

/// Mean of the RGB channels over the frame.
fn mean(rgba: &[u8]) -> f64 {
    rgba.chunks(4).map(|p| p[..3].iter().map(|c| *c as f64).sum::<f64>() / 3.0).sum::<f64>() / (W * H) as f64
}

#[test]
fn frames_render_the_same_in_any_order() {
    if !gpu() {
        return;
    }
    let mut c = build(&page("black", "", SPARKS));
    let times: Vec<f64> = (0..60).map(|f| f as f64 / 30.0).collect();
    let forward: Vec<Vec<u8>> = times.iter().map(|&t| render(&mut c, t)).collect();
    let backward: Vec<Vec<u8>> = times.iter().rev().map(|&t| render(&mut c, t)).collect();
    for (f, (a, b)) in forward.iter().zip(backward.iter().rev()).enumerate() {
        assert!(a == b, "frame {f} renders the same after frame {} as after frame {}", f.max(1) - 1, (f + 1).min(59));
    }
    assert_eq!(lit(&forward[0]), 0, "nothing is born before the first frame's time");
    let counts: Vec<usize> = [10, 30, 59].iter().map(|&f| lit(&forward[f])).collect();
    eprintln!("lit pixels at frames 10, 30 and 59: {counts:?}");
    assert!(counts.iter().all(|&n| n > 40), "the fountain shows sparks: {counts:?}");
    assert!(far(&forward[30], &forward[31]) > 20, "the sparks move between frames");
}

#[test]
fn the_seed_decides_the_particles() {
    if !gpu() {
        return;
    }
    let seeded = |seed: u32| frame(&page("black", "", &SPARKS.replace("seed='7'", &format!("seed='{seed}'"))), 0.8);
    let (a, b, other) = (seeded(7), seeded(7), seeded(8));
    assert!(a == b, "the same seed renders the same frame");
    let moved = far(&a, &other);
    eprintln!("{moved} pixels differ between seeds 7 and 8");
    assert!(moved > 40, "another seed scatters other particles: {moved}");
}

#[test]
fn the_live_and_export_levels_agree() {
    if !gpu() {
        return;
    }
    // Over the model's own black sky the jittered samples only soften the
    // cards' edges. Over a transparent sky the Export level resolves a soft
    // edge from its unpremultiplied mean, as it does every mesh edge.
    let model = "background environment-intensity='0'";
    let glowing = format!("{SPARKS} emissive='2'");
    let live = frame(&page("black", model, &glowing), 0.8);
    let export = frame(&page("black", &format!("{model} samples='16'"), &glowing), 0.8);
    let (l, e) = (lit(&live), lit(&export));
    let (ml, me) = (mean(&live), mean(&export));
    eprintln!("Live lights {l} pixels at mean {ml:.3}, Export {e} at mean {me:.3}");
    assert!(l > 40 && l.abs_diff(e) * 5 < l, "the levels cover the same pixels: {l} against {e}");
    assert!((ml - me).abs() < 0.1 * ml, "the levels add the same light: {ml} against {me}");
}

#[test]
fn a_shutter_stretches_fast_particles() {
    if !gpu() {
        return;
    }
    // A 360 degree shutter at 30 fps stays open 1/30 s, over which these
    // sparks fly 0.1 m, twice their size.
    let fast = "burst='12' lifetime='3' speed='3' spread='60' fade-in='0' size='0.05' emissive='4' seed='3'";
    let sharp = frame(&page("black", "samples='16'", fast), 0.15);
    let blurred = frame(&page("black", "shutter='360'", fast), 0.15);
    let peak = |rgba: &[u8]| rgba.chunks(4).map(|p| p[0].max(p[1]).max(p[2])).max().unwrap();
    let faint = |rgba: &[u8]| rgba.chunks(4).filter(|p| p[..3].iter().any(|c| *c >= 8)).count();
    let (s, b) = (faint(&sharp), faint(&blurred));
    eprintln!("sharp sparks light {s} pixels up to {}, blurred {b} up to {}", peak(&sharp), peak(&blurred));
    assert!(s > 12, "the sparks show");
    assert!(b * 2 > s * 3, "the shutter stretches each spark into a streak: {s} against {b}");
    assert!(peak(&blurred) < peak(&sharp), "the streaks spread each spark's light");
}

#[test]
fn emissive_particles_bloom_over_a_background() {
    if !gpu() {
        return;
    }
    // A dim probe behind the model: embers glow past their cards.
    let model = "background environment-intensity='20'";
    let embers = |emissive: &str| {
        format!("burst='6' lifetime='5' speed='0' emitter='sphere' radius='0.6' fade-in='0' size='0.08' color='#ff7a20' seed='5' {emissive}")
    };
    let dull = frame(&page("black", model, &embers("")), 1.0);
    let glowing = frame(&page("black", model, &embers("emissive='40'")), 1.0);
    let sky = frame(&page("black", model, &embers("opacity='0'")), 1.0);
    let (d, g) = (far(&dull, &sky), far(&glowing, &sky));
    eprintln!("dull embers change {d} pixels of the sky, glowing ones {g}");
    assert!(d > 10, "dull embers show");
    assert!(g > d * 2, "glowing embers bloom past their cards: {d} against {g}");
}

#[test]
fn a_transparent_sky_keeps_the_page() {
    if !gpu() {
        return;
    }
    let rgba = frame(&page("rgb(255,0,0)", "", &SPARKS.replace("#ffd080", "#40ff40")), 0.8);
    // Bevy's dither moves the transparent sky's pixels by up to 1.
    let red = rgba.chunks(4).filter(|p| p[0] >= 253 && p[1] <= 2 && p[2] <= 2).count();
    let green = rgba.chunks(4).filter(|p| p[1] > 100).count();
    eprintln!("{red} pixels keep the page and {green} show sparks");
    assert!(green > 40, "the sparks show over the page");
    let changed = (W * H) as usize - red;
    assert!(changed < green * 4, "only the sparks and their soft edges change the page: {changed}");
}

#[test]
fn surface_and_mesh_emitters_render() {
    if !gpu() {
        return;
    }
    // Dust born on `mirror.glb`'s floor lifts off it; debris tumbles as
    // small cubes.
    let mirror = fixture("mirror.glb");
    let dust = format!(
        "<style>body{{margin:0}}#main{{background:black}}model{{display:block;width:{W}px;height:{H}px}}</style>\
         <div id='main' data-composition-id='main' data-width='{W}' data-height='{H}' data-duration='2' data-fps='30'>\
         <model src='{mirror}' environment-intensity='0'><particles emitter='surface' node='Floor' rate='400' lifetime='2' \
         speed='0.3' size='0.08' emissive='2' seed='2'></particles></model></div>"
    );
    let (floor, bare) = (frame(&dust, 1.0), frame(&dust.replace("seed='2'", "seed='2' opacity='0'"), 1.0));
    let half = (W * H * 2) as usize;
    let (top, bottom) = (far(&floor[..half], &bare[..half]), far(&floor[half..], &bare[half..]));
    eprintln!("floor dust changes {top} pixels in the top half and {bottom} in the bottom half");
    assert!(bottom > 50, "dust rises from the floor in view");
    let cubes = frame(&page("black", "", "burst='20' lifetime='5' speed='1' spread='90' mesh='cube' size='0.2' emissive='1' seed='4'"), 0.5);
    let spun = frame(&page("black", "", "burst='20' lifetime='5' speed='1' spread='90' mesh='cube' size='0.2' emissive='1' seed='4' spin='360'"), 0.5);
    assert!(lit(&cubes) > 40, "the cubes show");
    assert!(far(&cubes, &spun) > 10, "spin turns the cubes");
    if director_3d::traceable() {
        // On a black floor with no light, only the glowing particles show.
        let hero = |paths: &str| {
            format!(
                "<style>body{{margin:0}}#main{{background:black}}model{{display:block;width:{W}px;height:{H}px}}</style>                 <div id='main' data-composition-id='main' data-width='{W}' data-height='{H}' data-duration='2' data-fps='30'>                 <model src='{mirror}' environment-intensity='0' {paths}><particles burst='20' lifetime='5' speed='1'                  spread='90' size='0.2' emissive='2' seed='4' position='0 0.5 0'></particles></model></div>"
            )
        };
        // The particles change the same pixels over the traced floor as over
        // the rasterized one.
        let bare = |source: String| source.replace("seed='4'", "seed='4' opacity='0'");
        let traced = far(&frame(&hero("paths='4'"), 0.5), &frame(&bare(hero("paths='4'")), 0.5));
        let raster = far(&frame(&hero(""), 0.5), &frame(&bare(hero("")), 0.5));
        eprintln!("the particles change {traced} traced pixels and {raster} rasterized");
        assert!(raster > 40 && traced.abs_diff(raster) * 10 < raster, "particles composite over the traced image: {traced} against {raster}");
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
fn the_scene_document_keeps_the_particles() {
    let source = page("black", "", &format!("{SPARKS} emitter='box' extent='1 0.2 1' stop='1.5' color-end='#ff2000' emissive='3'"));
    let document = export_scene(&build(&source).director).unwrap();
    let mut parsed = SceneDocument::from_json(&document.to_json().unwrap()).unwrap();
    assert_eq!(parsed, document);
    let particles = &model_of(&mut parsed).particles;
    assert_eq!(particles.len(), 1);
    let p = &particles[0];
    assert_eq!((p.emitter, p.rate, p.seed, p.stop, p.extent), (ParticleEmitter::Box, 60.0, 7, Some(1.5), [1.0, 0.2, 1.0]));
    assert_eq!((p.gravity, p.emissive), ([0.0, -4.0, 0.0], 3.0));
    if gpu() {
        let imported = import(parsed).render_frame(0.8).unwrap();
        assert_eq!(imported.rgba, frame(&source, 0.8), "the imported particles render as compiled");
    }
}

#[test]
fn bad_particles_fail_to_compile() {
    for (attrs, src, error) in [
        ("rate='10' burst='5'", "", "a rate or a burst"),
        ("rate='0'", "", "need a rate or a burst"),
        ("emitter='cone'", "", "emitter 'cone'"),
        ("emitter='surface'", "", "surface needs a node"),
        ("emitter='surface' node='Nowhere'", "mirror.glb", "'Nowhere' names no node"),
        ("node='Floor'", "mirror.glb", "node needs emitter surface"),
        ("lifetime='0'", "", "lifetime must be above 0"),
        ("spread='200'", "", "spread must be"),
        ("opacity='2'", "", "opacity must be"),
        ("rate='1000000' lifetime='5'", "", "keep up to"),
        ("gravity='0 -9'", "", "gravity '0 -9' must be three numbers"),
        ("mesh='cube' src='x.png'", "", "a sprite src or a mesh"),
        ("burst='2.5'", "", "whole number"),
    ] {
        let model = if src.is_empty() { String::new() } else { format!("src='{}'", fixture(src)) };
        let source = page("black", &model, attrs);
        let e = compile(&source, options()).err().unwrap_or_else(|| panic!("{attrs} compiles")).to_string();
        assert!(e.contains(error), "{attrs}: {e}");
    }
}
