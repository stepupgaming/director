//! `<model track>`: the compiler solves the camera of the footage under a
//! model and films the model through it frame by frame. The tests composite
//! `solve.glb`, which holds no camera, over the `solve.mp4` plate it was
//! solved from and compare the result with Eevee's renders of the same shot
//! with the box; `make_solve.py` writes all four, with `--lens` the same
//! plate and renders through a barrel-distorted lens, with `--moving`
//! the shot with movers in it and their matte, and with `--pan` the yard
//! from a camera that only turns. The masks take the box's
//! lit faces by colour and the shadow by its darkening, each only where the
//! image moves away from the decoded plate. Each GPU test skips on a
//! machine without a Vulkan adapter.

use director_core::scene_export::export_scene;
use director_html::{compile, CompileOptions, CompiledHtml};
use director_render::render_frame;
use director_scene::{NodeBody, SceneDocument};
use std::path::PathBuf;
use std::sync::Arc;

const W: i32 = 480;
const H: i32 = 270;

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

/// The lens fixture's distortion, which `make_solve.py --lens` bends by.
const K1: f64 = -0.15;
const K2: f64 = 0.03;

/// The `plate` (`solve`, `lens`, `moving` or `pan`) under `solve.glb` with the sky and units
/// of `tests/eevee.rs`, its ground a catcher and its camera solved from the
/// plate with `track` attributes. An empty `track` leaves the plate alone.
fn page(plate: &str, track: &str) -> String {
    let layer = if track.is_empty() {
        String::new()
    } else {
        format!(
            "<model src='{}' environment='{}' environment-intensity='683' exposure='{}' shadow-catcher='Ground' track='{}' {track}></model>",
            fixture("solve.glb"),
            fixture("sky.hdr"),
            (683.0f64 / 1.2).log2(),
            fixture(&format!("{plate}.mp4"))
        )
    };
    format!(
        "<style>body{{margin:0}}video,model{{position:absolute;left:0;top:0;width:{W}px;height:{H}px}}</style>\
         <div id='main' data-composition-id='main' data-width='{W}' data-height='{H}' data-duration='2'>\
         <video muted src='{}'></video>{layer}</div>",
        fixture(&format!("{plate}.mp4"))
    )
}

fn build(source: &str) -> CompiledHtml {
    let options = CompileOptions { width: W, height: H, duration: 2.0, ..Default::default() };
    compile(source, options).unwrap_or_else(|e| panic!("{source}: {e}"))
}

fn frame(c: &mut CompiledHtml, time: f64) -> Vec<u8> {
    let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).unwrap();
    render_frame(&mut c.director, time, &mut surface).unwrap();
    let info = skia_safe::ImageInfo::new((W, H), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Unpremul, None);
    let mut pixels = vec![0; (W * H * 4) as usize];
    assert!(surface.read_pixels(&info, &mut pixels, W as usize * 4, (0, 0)));
    pixels
}

fn eevee(plate: &str, frame: u32) -> Vec<u8> {
    let bytes = std::fs::read(fixture(&format!("{plate}-{frame}.png"))).unwrap();
    let image = skia_safe::Image::from_encoded(skia_safe::Data::new_copy(&bytes)).expect("a PNG");
    assert_eq!((image.width(), image.height()), (W, H));
    let info = skia_safe::ImageInfo::new((W, H), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Unpremul, None);
    let mut pixels = vec![0; (W * H * 4) as usize];
    assert!(image.read_pixels(&info, &mut pixels, W as usize * 4, (0, 0), skia_safe::image::CachingHint::Allow));
    pixels
}

fn rgb(p: &[u8], i: usize) -> [u8; 3] {
    [p[i * 4], p[i * 4 + 1], p[i * 4 + 2]]
}

fn luma(p: [u8; 3]) -> f64 {
    0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64
}

/// The box's lit faces: within 45 levels of Eevee's (83, 127, 195) summed
/// over the channels. The plate's ground holds blue cells too, so callers
/// also ask for a change from the plate.
fn boxed(p: [u8; 3]) -> bool {
    p.iter().zip([83u8, 127, 195]).map(|(a, b)| a.abs_diff(b) as u32).sum::<u32>() < 45
}

fn diff(a: &[u8], b: &[u8], i: usize) -> u32 {
    (0..3).map(|k| rgb(a, i)[k].abs_diff(rgb(b, i)[k]) as u32).sum()
}

fn count(mask: &[bool]) -> usize {
    mask.iter().filter(|&&m| m).count()
}

fn iou(a: &[bool], b: &[bool]) -> f64 {
    let both = a.iter().zip(b).filter(|(a, b)| **a && **b).count();
    let either = a.iter().zip(b).filter(|(a, b)| **a || **b).count();
    both as f64 / either.max(1) as f64
}

/// Compares the composite through the camera solved from `plate` with
/// Eevee's render at Blender frames 10 and 50, the box overlapping Eevee's
/// at an IoU above `fit`.
fn matches_eevee(name: &str, track: &str, fit: f64) {
    let mut plate = build(&page(name, ""));
    let mut composite = build(&page(name, track));
    for frame in [10, 50] {
        let time = frame as f64 / 30.0;
        let (reference, plate, composite) = (eevee(name, frame), self::frame(&mut plate, time), self::frame(&mut composite, time));
        let n = (W * H) as usize;
        let mask = |image: &[u8], f: &dyn Fn(usize, [u8; 3]) -> bool| (0..n).map(|i| f(i, rgb(image, i))).collect::<Vec<bool>>();
        let box_ref = mask(&reference, &|i, p| boxed(p) && diff(&reference, &plate, i) > 30);
        let box_dir = mask(&composite, &|i, p| boxed(p) && diff(&composite, &plate, i) > 30);
        // The plate's yuv420p chroma moves Eevee's colourful ground by up to
        // 40 levels, so a shadow must darken it by 15.
        let dark = |image: &[u8]| {
            let image = image.to_vec();
            mask(&image, &|i, p| !box_ref[i] && !box_dir[i] && luma(p) < luma(rgb(&plate, i)) - 15.0)
        };
        let (shadow_ref, shadow_dir) = (dark(&reference), dark(&composite));
        let (near, away): (Vec<usize>, Vec<usize>) = (0..n).partition(|&i| box_ref[i] || box_dir[i] || shadow_ref[i] || shadow_dir[i]);
        // The mean error against Eevee where either image shows the box or its shadow.
        let error = |image: &[u8]| near.iter().map(|&i| diff(image, &reference, i) as f64).sum::<f64>() / (near.len() * 3) as f64;
        let drift = away.iter().map(|&i| diff(&composite, &plate, i) as f64).sum::<f64>() / (away.len() * 3) as f64;
        eprintln!(
            "{name} {track:?} frame {frame}: box {} / {} iou {:.3}; shadow {} / {} iou {:.2}; error plate {:.2} composite {:.2}; drift {drift:.2}",
            count(&box_ref), count(&box_dir), iou(&box_ref, &box_dir),
            count(&shadow_ref), count(&shadow_dir), iou(&shadow_ref, &shadow_dir),
            error(&plate), error(&composite)
        );
        assert!(count(&box_ref) > 500, "Eevee shows the box");
        // The box spans about 45 px, so each pixel of offset costs about 0.04.
        assert!(iou(&box_ref, &box_dir) > fit, "the solved camera puts the box where Eevee does");
        assert!(iou(&shadow_ref, &shadow_dir) > 0.8, "the shadow falls where Eevee's does");
        assert!(error(&composite) < error(&plate) * 0.3, "the composite comes closer to Eevee's shot than the plate");
        assert!(drift < 1.2, "the plate shows unchanged away from the box and its shadow: {drift:.2}");
    }
}

#[test]
fn a_model_rides_the_camera_solved_from_its_plate() {
    if gpu() {
        matches_eevee("solve", "track-fov='42' track-height='1.6'", 0.9);
    }
}

#[test]
fn the_solve_finds_the_lens_without_one() {
    if gpu() {
        matches_eevee("solve", "track-height='1.6'", 0.9);
    }
}

#[test]
fn a_mask_keeps_the_model_on_a_plate_with_movers() {
    if gpu() {
        // The movers hide part of the yard, so the masked solve keeps 581
        // points to the clean plate's 653 and lands within 0.10 m.
        let mask = format!("track-fov='42' track-height='1.6' track-mask='{}'", fixture("moving-matte.mp4"));
        matches_eevee("moving", &mask, 0.88);
    }
}

#[test]
fn a_model_holds_still_under_a_camera_that_only_turns() {
    if gpu() {
        // Frame 0 of the pan looks 8.12 degrees down, as the walk's does.
        matches_eevee("pan", "track-height='1.6' track-pitch='8.12'", 0.9);
    }
}

#[test]
fn a_model_bends_with_the_lens_of_its_plate() {
    if gpu() {
        matches_eevee("lens", "track-fov='42' track-height='1.6'", 0.9);
    }
}

/// The track a compiled page's model keeps.
fn track(compiled: &CompiledHtml) -> director_scene::ModelTrack {
    let document = export_scene(&compiled.director).unwrap();
    document
        .nodes
        .iter()
        .find_map(|node| match &node.body {
            NodeBody::Html(html) => html.content.as_ref().and_then(|c| match &c.body {
                NodeBody::Model(model) => model.track.clone(),
                _ => None,
            }),
            _ => None,
        })
        .expect("a model track")
}

#[test]
fn the_document_keeps_the_solved_distortion() {
    let compiled = build(&page("lens", "track-fov='42'"));
    let [k1, k2] = track(&compiled).distortion.map(f64::from);
    // k1 and k2 trade against each other, so the test compares where each
    // lens sends the frame's corner, 0.78 focal lengths out.
    let corner = |k1: f64, k2: f64| {
        let r2 = (240.0f64.powi(2) + 135.0f64.powi(2)) / (135.0 / 21f64.to_radians().tan()).powi(2);
        r2.sqrt() * (1.0 + k1 * r2 + k2 * r2 * r2) * 135.0 / 21f64.to_radians().tan()
    };
    eprintln!("solved k1 {k1:.4} k2 {k2:.4}; corner {:.2} px against {:.2} px", corner(k1, k2), corner(K1, K2));
    assert!((corner(k1, k2) - corner(K1, K2)).abs() < 0.75, "the solved lens bends the corner like the true one");

    let fixed = build(&page("lens", "track-fov='42' track-distortion='-0.15 0.03'"));
    assert_eq!(track(&fixed).distortion, [-0.15, 0.03], "track-distortion fixes the lens");
    let pinhole = build(&page("solve", "track-fov='42' track-distortion='0 0'"));
    let json = export_scene(&pinhole.director).unwrap().to_json().unwrap();
    assert!(!json.contains("distortion"), "a pinhole track writes no distortion");
}

#[test]
fn the_scene_document_keeps_the_solved_cameras() {
    if !gpu() {
        return;
    }
    let mut compiled = build(&page("lens", "track-fov='42'"));
    let document = export_scene(&compiled.director).unwrap();
    let parsed = SceneDocument::from_json(&document.to_json().unwrap()).unwrap();
    assert_eq!(parsed, document);
    let track = track(&compiled);
    assert_eq!((track.fps, track.fov, track.frames.len()), (30.0, 42.0, 60));
    assert!(track.distortion[0] < -0.1, "the document keeps the barrel distortion: {:?}", track.distortion);

    let mut registry = director_core::scene_import::ImportRegistry::core();
    director_html::register_import(&mut registry);
    let renderer = director_render::Renderer::new(Arc::new(parsed), Arc::new(director_core::DefaultAssetLoader))
        .with_registry(registry);
    let time = 50.0 / 30.0;
    let imported = renderer.render_frame(time).unwrap();
    let live = frame(&mut compiled, time);
    let differ = imported.rgba.chunks(4).zip(live.chunks(4)).filter(|(p, q)| (0..3).any(|k| p[k].abs_diff(q[k]) > 2)).count();
    assert_eq!(differ, 0, "the document renders the live frame");
}

#[test]
fn a_missing_plate_names_itself() {
    let source = page("solve", "").replace(
        "</div>",
        &format!("<model src='{}' track='no-such-plate.mp4'></model></div>", fixture("solve.glb")),
    );
    let error = compile(&source, CompileOptions { width: W, height: H, duration: 2.0, ..Default::default() })
        .err()
        .expect("a compile error")
        .to_string();
    assert!(error.contains("no-such-plate.mp4"), "{error}");
}

#[test]
fn a_lens_out_of_range_fails_to_compile() {
    let fails = |track: &str| {
        compile(&page("solve", track), CompileOptions { width: W, height: H, duration: 2.0, ..Default::default() })
            .err()
            .expect("a compile error")
            .to_string()
    };
    let error = fails("track-fov='0'");
    assert!(error.contains("track-fov must be from 1 to 179 degrees"), "{error}");
    let error = fails("track-distortion='-0.1'");
    assert!(error.contains("track-distortion must be two numbers, k1 and k2"), "{error}");
    let error = fails("track-pitch='90'");
    assert!(error.contains("track-pitch must be from -89 to 89 degrees"), "{error}");
    let error = fails("track-mask='no-such-matte.mp4'");
    assert!(error.contains("no-such-matte.mp4"), "{error}");
}

