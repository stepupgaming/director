//! `<model reflection>`: shadow catchers that mirror the model onto the page
//! or footage under it. `mirror.glb` (written by `make_mirror.py`) holds a
//! flat `Floor`, a folded `Ridge` and a camera. Extruded type is symmetric
//! through its depth, so a copy turned 180 degrees about X at the mirrored
//! height has the reflection's exact shape: the tests compare the mirror
//! with that copy. GPU tests skip on a machine without a Vulkan adapter.

use director_core::scene_export::export_scene;
use director_html::{compile, CompileOptions, CompiledHtml};
use director_render::render_frame;
use director_scene::{NodeBody, SceneDocument};
use std::path::PathBuf;
use std::sync::Arc;

const W: i32 = 240;
const H: i32 = 160;

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
        ..Default::default()
    }
}

const SUN: &str = "<light type='sun' intensity='4000' position='3 4 8' target='0 0 0'></light>";
const TYPE: &str = "<text3d size='1' color='#f2c040' position='0 0.6 0'>LP</text3d>";
const FLIPPED: &str = "<text3d size='1' color='#f2c040' position='0 -0.6 0' rotation='180 0 0'>LP</text3d>";

/// `mirror.glb` over a page of `background`, the model taking `attrs` and
/// holding `inner` beside the sun.
fn page(background: &str, attrs: &str, inner: &str) -> String {
    format!(
        "<style>body{{margin:0}}#main{{background:{background}}}model{{display:block;width:{W}px;height:{H}px}}</style>\
         <div id='main' data-composition-id='main' data-width='{W}' data-height='{H}' data-duration='2'>\
         <model id='m' src='{}' {attrs}>{SUN}{inner}</model></div>",
        fixture("mirror.glb")
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

fn luma(p: &[u8]) -> f64 {
    0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64
}

/// Pixels the type lights over a black page.
fn lit(p: &[u8]) -> Vec<bool> {
    p.chunks(4).map(|p| luma(p) > 24.0).collect()
}

fn minus(a: &[bool], b: &[bool]) -> Vec<bool> {
    a.iter().zip(b).map(|(a, b)| *a && !*b).collect()
}

fn count(mask: &[bool]) -> usize {
    mask.iter().filter(|&&m| m).count()
}

fn iou(a: &[bool], b: &[bool]) -> f64 {
    let both = a.iter().zip(b).filter(|(a, b)| **a && **b).count();
    let either = a.iter().zip(b).filter(|(a, b)| **a || **b).count();
    both as f64 / either.max(1) as f64
}

fn mean_luma(p: &[u8], mask: &[bool]) -> f64 {
    let (sum, n) = p.chunks(4).zip(mask).filter(|(_, m)| **m).fold((0.0, 0), |(s, n), (p, _)| (s + luma(p), n + 1));
    sum / n.max(1) as f64
}

/// The mask grown by `r` pixels.
fn grow(mask: &[bool], r: i32) -> Vec<bool> {
    (0..H * W)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            (-r..=r).any(|dy| {
                (-r..=r).any(|dx| {
                    let (x, y) = (x + dx, y + dy);
                    (0..W).contains(&x) && (0..H).contains(&y) && mask[(y * W + x) as usize]
                })
            })
        })
        .collect()
}

const BLACK: &str = "rgb(0,0,0)";
const GREY: &str = "rgb(128,128,128)";
const CATCH: &str = "shadow-catcher='Floor'";

/// The type alone, its reflection in a full mirror, and the flipped copy
/// under a catcher that reflects nothing, all over a black page.
fn mirrored() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let alone = shot(&page(BLACK, CATCH, TYPE));
    let mirror = shot(&page(BLACK, &format!("{CATCH} reflection='1'"), TYPE));
    let copy = shot(&page(BLACK, CATCH, &format!("{TYPE}{FLIPPED}")));
    (alone, mirror, copy)
}

#[test]
fn a_full_mirror_shows_the_type_where_its_flipped_copy_stands() {
    if !gpu() {
        return;
    }
    let (alone, mirror, copy) = mirrored();
    let base = lit(&alone);
    let reflected = minus(&lit(&mirror), &base);
    let flipped = minus(&lit(&copy), &base);
    let overlap = iou(&reflected, &flipped);
    let (seen, truth) = (mean_luma(&mirror, &reflected), mean_luma(&copy, &flipped));
    eprintln!(
        "type {} px, reflection {} px, flipped copy {} px, iou {overlap:.3}, luma {seen:.1} against {truth:.1}",
        count(&base),
        count(&reflected),
        count(&flipped)
    );
    assert!(count(&base) > 400, "the type draws: {}", count(&base));
    assert!(count(&flipped) > 400, "the copy stands under the floor: {}", count(&flipped));
    assert!(overlap > 0.9, "the reflection has the flipped copy's shape: {overlap:.3}");
    // The copy's faces turn toward the sun the way the type's own do, and
    // the mirror sees the type's own shading from below, so only the sides
    // and highlights differ.
    assert!(seen > truth * 0.75 && seen < truth * 1.33, "a full mirror is as bright as the copy: {seen:.1} against {truth:.1}");
    // Over a black page the mirror changes nothing outside the reflection.
    let outside = grow(&reflected, 2);
    let stray = (0..(W * H) as usize)
        .filter(|&i| !outside[i] && (0..3).any(|k| mirror[i * 4 + k].abs_diff(alone[i * 4 + k]) > 2))
        .count();
    assert_eq!(stray, 0, "the mirror adds light only where the type reflects");
}

#[test]
fn reflection_strength_scales_the_mirror_and_keeps_the_plate() {
    if !gpu() {
        return;
    }
    let (alone, mirror, _) = mirrored();
    let reflected = minus(&lit(&mirror), &lit(&alone));
    let at = |strength: &str| shot(&page(GREY, &format!("{CATCH} reflection='{strength}'"), TYPE));
    let (none, some, full) = (shot(&page(GREY, CATCH, TYPE)), at("0.3"), at("1"));
    let differ = |a: &[u8], mask: &[bool]| {
        let n = count(mask).max(1) as f64;
        a.chunks(4).zip(none.chunks(4)).zip(mask).filter(|(_, m)| **m).map(|((p, q), _)| (luma(p) - luma(q)).abs()).sum::<f64>() / n
    };
    let (weak, strong) = (differ(&some, &reflected), differ(&full, &reflected));
    eprintln!("mean change over the reflection: 0.3 {weak:.1}, 1 {strong:.1}");
    // Fresnel lifts a weak mirror toward full at the floor's grazing angle,
    // so 0.3 reflects about two thirds of what a full mirror does.
    assert!(weak > 4.0 && strong > weak * 1.2, "a stronger mirror changes the plate more: {weak:.1} {strong:.1}");
    // Away from the reflection the plate keeps its grey and its shadows.
    let outside = grow(&reflected, 3);
    let kept = (0..(W * H) as usize)
        .filter(|&i| !outside[i])
        .all(|i| (0..3).all(|k| full[i * 4 + k].abs_diff(none[i * 4 + k]) <= 2));
    assert!(kept, "the plate shows unchanged outside the reflection");
}

#[test]
fn a_path_traced_model_reflects_the_same_type() {
    if !gpu() || !director_3d::traceable() {
        eprintln!("skipped: no ray tracing Vulkan adapter");
        return;
    }
    let (alone, mirror, _) = mirrored();
    let raster = minus(&lit(&mirror), &lit(&alone));
    let traced_attrs = format!("{CATCH} paths='16'");
    let traced_alone = shot(&page(BLACK, &traced_attrs, TYPE));
    let traced = shot(&page(BLACK, &format!("{traced_attrs} reflection='1'"), TYPE));
    let hero = minus(&lit(&traced), &lit(&traced_alone));
    let overlap = iou(&raster, &hero);
    eprintln!("hero reflection {} px, raster {} px, iou {overlap:.3}", count(&hero), count(&raster));
    assert!(overlap > 0.85, "the Hero frame reflects the same type: {overlap:.3}");
}

#[test]
fn the_scene_document_keeps_the_reflection() {
    if !gpu() {
        return;
    }
    let mut compiled = build(&page(GREY, &format!("{CATCH} reflection='0.5'"), TYPE));
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
    assert_eq!(model.reflection, 0.5);
    let mut registry = director_core::scene_import::ImportRegistry::core();
    director_html::register_import(&mut registry);
    let renderer = director_render::Renderer::new(Arc::new(parsed), Arc::new(director_core::DefaultAssetLoader))
        .with_registry(registry);
    let imported = renderer.render_frame(0.0).unwrap();
    let live = frame(&mut compiled, 0.0);
    let differ = imported.rgba.chunks(4).zip(live.chunks(4)).filter(|(p, q)| (0..3).any(|k| p[k].abs_diff(q[k]) > 2)).count();
    assert_eq!(differ, 0);
}

#[test]
fn a_catcher_out_of_one_plane_cannot_reflect() {
    if !gpu() {
        return;
    }
    for catchers in ["Ridge", "Floor Ridge"] {
        let mut compiled = build(&page(BLACK, &format!("shadow-catcher='{catchers}' reflection='1'"), TYPE));
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).unwrap();
        let error = render_frame(&mut compiled.director, 0.0, &mut surface).expect_err("a render error").to_string();
        assert!(error.contains("lie in one plane"), "{catchers}: {error}");
    }
    // The folded ridge still catches shadows without a reflection.
    shot(&page(BLACK, "shadow-catcher='Ridge'", TYPE));
}

#[test]
fn reflection_needs_a_catcher_and_a_strength_from_0_to_1() {
    for (attrs, message) in [
        ("reflection='0.5'", "reflection needs a shadow-catcher"),
        ("shadow-catcher='Floor' reflection='1.5'", "reflection must be from 0 to 1"),
        ("shadow-catcher='Floor' reflection='-0.1'", "reflection must be from 0 to 1"),
    ] {
        let error = compile(&page(BLACK, attrs, TYPE), options()).err().expect("a compile error").to_string();
        assert!(error.contains(message), "{attrs}: {error}");
    }
}
