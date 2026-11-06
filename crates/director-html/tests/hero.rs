//! `<model paths>`: the Hero level, path traced by `director-3d` over the
//! test GLB, and its scene document round trip. Each test skips on a machine
//! without a Vulkan adapter that traces rays.

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
    let ok = director_3d::available() && director_3d::traceable();
    if !ok {
        eprintln!("skipped: no ray tracing Vulkan adapter for director-3d");
    }
    ok
}

fn options() -> CompileOptions {
    CompileOptions {
        width: W,
        height: H,
        duration: 2.0,
        ..Default::default()
    }
}

/// The test GLB over a red page, the model taking `attrs` and `children`.
fn page(attrs: &str, children: &str) -> String {
    format!(
        "<style>body{{margin:0}}#main{{background:rgb(255,0,0)}}model{{display:block;width:{W}px;height:{H}px}}</style>\
         <div id='main' data-composition-id='main' data-width='{W}' data-height='{H}' data-duration='2'>\
         <model id='m' src='{}' {attrs}>{children}</model></div>",
        fixture("model.glb")
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

fn shot(source: &str) -> Vec<u8> {
    frame(&mut build(source), 0.0)
}

/// Pixels whose channels differ by more than 2.
fn changed(a: &[u8], b: &[u8]) -> usize {
    a.chunks(4).zip(b.chunks(4)).filter(|(p, q)| (0..3).any(|k| p[k].abs_diff(q[k]) > 2)).count()
}

/// Mean absolute channel difference.
fn error(a: &[u8], b: &[u8]) -> f64 {
    let sum: u64 = a.chunks(4).zip(b.chunks(4)).map(|(p, q)| (0..3).map(|k| p[k].abs_diff(q[k]) as u64).sum::<u64>()).sum();
    sum as f64 / (W * H * 3) as f64
}

fn page_red(p: &[u8]) -> bool {
    p[0] >= 250 && p[1] < 16 && p[2] < 16
}

/// Mean luminance of the pixels that are not the page.
fn scene_luma(p: &[u8]) -> f64 {
    let lit: Vec<f64> = p
        .chunks(4)
        .filter(|p| !page_red(p))
        .map(|p| 0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64)
        .collect();
    lit.iter().sum::<f64>() / lit.len().max(1) as f64
}

#[test]
fn a_path_traced_model_covers_what_the_raster_covers() {
    if !gpu() {
        return;
    }
    let raster = shot(&page("", ""));
    let hero = shot(&page("paths='64'", ""));
    assert!(page_red(&hero[..4]), "the sky stays transparent: {:?}", &hero[..4]);
    let sky = |p: &[u8]| p.chunks(4).filter(|p| page_red(p)).count() as i64;
    let (a, b) = (sky(&raster), sky(&hero));
    assert!((a - b).abs() < (W * H) as i64 / 25, "raster shows {a} page pixels, Hero {b}");
    assert!(changed(&raster, &hero) > 200, "path tracing changes the shading");
    let (a, b) = (scene_luma(&raster), scene_luma(&hero));
    assert!(b > a * 0.5 && b < a * 2.0, "raster scene luma {a:.1}, Hero {b:.1}");
}

#[test]
fn a_hero_frame_depends_only_on_its_inputs() {
    if !gpu() {
        return;
    }
    let source = page("paths='8' samples='4'", "");
    let first = shot(&source);
    shot(&page("paths='8' style='--camera-yaw:40deg'", ""));
    shot(&page("", ""));
    assert_eq!(changed(&first, &shot(&source)), 0);
}

#[test]
fn more_paths_converge_on_the_reference() {
    if !gpu() {
        return;
    }
    let reference = shot(&page("paths='1024'", ""));
    let few = error(&shot(&page("paths='4'", "")), &reference);
    let many = error(&shot(&page("paths='64'", "")), &reference);
    assert!(many < few * 0.6, "64 paths miss by {many:.2}, 4 paths by {few:.2}");
    let split = error(&shot(&page("paths='256' samples='4'", "")), &reference);
    assert!(split < few * 0.6, "4 samples of 256 paths miss by {split:.2}");
}

#[test]
fn the_denoiser_lands_closer_to_the_reference() {
    if !gpu() {
        return;
    }
    let reference = shot(&page("paths='1024'", ""));
    let raw = error(&shot(&page("paths='16'", "")), &reference);
    let many = error(&shot(&page("paths='64'", "")), &reference);
    let denoised = shot(&page("paths='16' denoise", ""));
    let filtered = error(&denoised, &reference);
    assert!(filtered < many * 0.75, "16 denoised paths miss by {filtered:.2}, 64 raw paths by {many:.2}");
    let few = error(&shot(&page("paths='4' denoise", "")), &reference);
    assert!(few < raw * 0.75, "4 denoised paths miss by {few:.2}, 16 raw paths by {raw:.2}");
    assert!(page_red(&denoised[..4]), "the sky stays transparent: {:?}", &denoised[..4]);
    let sky = |p: &[u8]| p.chunks(4).filter(|p| page_red(p)).count() as i64;
    let (a, b) = (sky(&reference), sky(&denoised));
    assert!((a - b).abs() < (W * H) as i64 / 100, "the reference shows {a} page pixels, the denoised frame {b}");
}

#[test]
fn the_background_fills_the_sky_with_the_environment() {
    if !gpu() {
        return;
    }
    let hero = shot(&page("paths='16' background", ""));
    assert_eq!(hero.chunks(4).filter(|p| page_red(p)).count(), 0);
    let raster = shot(&page("background", ""));
    let (a, b) = (scene_luma(&raster), scene_luma(&hero));
    assert!(b > a * 0.5 && b < a * 2.0, "raster frame luma {a:.1}, Hero {b:.1}");
}

#[test]
fn a_lens_and_a_sized_sun_blur_the_path_traced_frame() {
    if !gpu() {
        return;
    }
    let sharp = shot(&page("paths='16'", ""));
    let lens = shot(&page("paths='16' style='--camera-aperture:1.4;--camera-focus:0.5'", ""));
    assert!(changed(&sharp, &lens) > 300, "a wide aperture defocuses the scene");
    let sun = "<light type='sun' intensity='20000' angle='8' position='2 4 3'></light>";
    let soft = shot(&page("paths='16'", sun));
    assert!(changed(&sharp, &soft) > 200, "an added sun lights the scene");
}

/// The luma `light` adds to each pixel of the scene at a level, NaN where
/// the page shows.
fn added(attrs: &str, light: &str) -> Vec<f64> {
    let luma = |p: &[u8]| 0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64;
    let (lit, dark) = (shot(&page(attrs, light)), shot(&page(attrs, "")));
    lit.chunks(4)
        .zip(dark.chunks(4))
        .map(|(p, q)| if page_red(p) || page_red(q) { f64::NAN } else { luma(p) - luma(q) })
        .collect()
}

/// Each light lands where the raster puts it (the added light correlates
/// pixel by pixel) and as strongly on the patch it lights directly, where
/// bounce light adds little.
#[test]
fn point_spot_and_rect_lights_match_the_raster_lights() {
    if !gpu() {
        return;
    }
    let lights = [
        "<light type='point' intensity='2000000' radius='0.2' position='1.5 2 2'></light>",
        "<light type='point' intensity='2000000' position='-1.5 2 2'></light>",
        "<light type='spot' intensity='4000000' cone='60' position='0 3 2'></light>",
        "<light type='spot' intensity='4000000' cone='50' radius='0.2' position='1 3 1.5'></light>",
        "<light type='rect' intensity='2000000' width='1.5' height='1' position='0 2.5 2.5'></light>",
    ];
    for light in lights {
        let (raster, hero): (Vec<f64>, Vec<f64>) = added("samples='16'", light)
            .into_iter()
            .zip(added("paths='64'", light))
            .filter(|(r, h)| !r.is_nan() && !h.is_nan())
            .unzip();
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        let (mr, mh) = (mean(&raster), mean(&hero));
        let cov: f64 = raster.iter().zip(&hero).map(|(r, h)| (r - mr) * (h - mh)).sum();
        let vr: f64 = raster.iter().map(|r| (r - mr).powi(2)).sum();
        let vh: f64 = hero.iter().map(|h| (h - mh).powi(2)).sum();
        let shape = cov / (vr * vh).sqrt();
        let peak = raster.iter().cloned().fold(0.0, f64::max);
        let patch: Vec<usize> = (0..raster.len()).filter(|&i| raster[i] > peak / 2.0).collect();
        let strength = patch.iter().map(|&i| hero[i]).sum::<f64>() / patch.iter().map(|&i| raster[i]).sum::<f64>();
        eprintln!("{light}: shape {shape:.3}, strength {strength:.3} over {} pixels, peak {peak:.1}", patch.len());
        assert!(peak > 20.0, "{light} lights the raster frame: peak {peak:.1}");
        assert!(shape > 0.8, "{light} lands where the raster light does: correlation {shape:.3}");
        assert!(strength > 0.75 && strength < 1.33, "{light}: Hero over raster on the lit patch {strength:.3}");
    }
}

/// Pixels where one frame shows the page and the other the scene.
fn coverage_differs(a: &[u8], b: &[u8]) -> usize {
    a.chunks(4).zip(b.chunks(4)).filter(|(p, q)| page_red(p) != page_red(q)).count()
}

#[test]
fn skins_and_morph_targets_pose_the_path_traced_scene() {
    if !gpu() {
        return;
    }
    let rig = |attrs: &str| page(attrs, "").replace(&fixture("model.glb"), &fixture("rig.glb"));
    let mut raster = build(&rig(""));
    let (raster_start, raster_end) = (frame(&mut raster, 0.0), frame(&mut raster, 1.9));
    let mut hero = build(&rig("paths='16'"));
    let (start, end) = (frame(&mut hero, 0.0), frame(&mut hero, 1.9));
    // The levels antialias edges differently, so they disagree on some edge
    // pixels in any pose.
    let edges = coverage_differs(&start, &raster_start);
    let rest = coverage_differs(&start, &raster_end);
    let posed = coverage_differs(&end, &raster_end);
    assert!(rest > edges + 300, "the bend and the squash change the silhouette: {rest} pixels, {edges} at the edges");
    assert!(posed < edges + rest / 8, "the Hero frame takes the pose of its time: {posed} pixels miss, {edges} at the edges");
}

fn green(p: &[u8]) -> bool {
    p[1] > 150 && p[0] < 80 && p[2] < 100
}

/// Pixels green in either frame, and in both.
fn greens(a: &[u8], b: &[u8]) -> (usize, usize) {
    let pairs = || a.chunks(4).zip(b.chunks(4));
    (pairs().filter(|(p, q)| green(p) || green(q)).count(), pairs().filter(|(p, q)| green(p) && green(q)).count())
}

/// The transparent pass draws the clouds over the traced meshes against the
/// raster depth, so they land where the raster puts them, and the meshes
/// still trace.
#[test]
fn splats_draw_over_the_path_traced_meshes() {
    if !gpu() {
        return;
    }
    let cloud = format!("<splat src='{}' position='0 1.5 0' scale='0.5'></splat>", fixture("splats.ply"));
    let raster = shot(&page("", &cloud));
    let hero = shot(&page("paths='16'", &cloud));
    let meshes = shot(&page("paths='16'", ""));
    let (either, both) = greens(&raster, &hero);
    eprintln!("green: {either} in either, {both} in both");
    assert!(both > 20, "the green ball shows in the Hero frame: {both} pixels");
    assert!(both * 10 > either * 8, "it lands where the raster draws it: {both} of {either}");
    assert_eq!(meshes.chunks(4).filter(|p| green(p)).count(), 0, "the meshes alone have no green");
    assert!(changed(&hero, &raster) > 500, "the meshes are path traced");
    let edges = coverage_differs(&hero, &raster);
    assert!(edges < (W * H) as usize / 50, "both levels cover the same pixels: {edges} differ");
}

/// A model of splats alone has nothing to trace and rasterizes.
#[test]
fn a_model_of_splats_alone_rasterizes_under_paths() {
    if !gpu() {
        return;
    }
    let balls = |attrs: &str| {
        format!(
            "<style>body{{margin:0}}model{{display:block;width:{W}px;height:{H}px}}</style>             <div id='main' data-composition-id='main' data-width='{W}' data-height='{H}' data-duration='2'>             <model id='m' {attrs}><splat src='{}'></splat></model></div>",
            fixture("splats.ply")
        )
    };
    assert_eq!(changed(&shot(&balls("paths='16'")), &shot(&balls(""))), 0);
}

#[test]
fn the_scene_document_keeps_the_paths() {
    if !gpu() {
        return;
    }
    let mut compiled = build(&page("paths='16' samples='2' denoise", ""));
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
    assert_eq!((model.paths, model.samples, model.denoise), (16, 2, true));
    let mut registry = director_core::scene_import::ImportRegistry::core();
    director_html::register_import(&mut registry);
    let renderer = director_render::Renderer::new(Arc::new(parsed), Arc::new(director_core::DefaultAssetLoader))
        .with_registry(registry);
    let imported = renderer.render_frame(0.0).unwrap();
    assert_eq!(changed(&imported.rgba, &frame(&mut compiled, 0.0)), 0);
}

#[test]
fn unsupported_hero_inputs_fail_to_compile() {
    let error = |source: &str| compile(source, options()).err().expect("a compile error").to_string();
    let e = error(&page("paths='5000'", ""));
    assert!(e.contains("paths must be a whole number from 0 to 4096"), "{e}");
    let e = error(&page("paths='1.5'", ""));
    assert!(e.contains("paths must be a whole number"), "{e}");
    let e = error(&page("denoise", ""));
    assert!(e.contains("denoise needs paths"), "{e}");
}
