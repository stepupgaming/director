//! `--grade` colour grades: presets, `.cube` files, intensity, crossfades
//! from CSS keyframes and GSAP, and the scene document round trip.

use director_core::grade::{preset, Lut3d, PRESETS};
use director_core::scene_export::export_scene;
use director_html::{compile, CompileOptions, CompiledHtml};
use director_render::render_frame;
use director_scene::html::GradeLut;
use director_scene::{NodeBody, SceneDocument};
use std::path::PathBuf;
use std::sync::Arc;

const SIZE: i32 = 96;
const SWATCH: [u8; 3] = [200, 120, 60];

fn options() -> CompileOptions {
    CompileOptions {
        width: SIZE,
        height: SIZE,
        duration: 2.0,
        gsap_dir: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools/node_modules/gsap/dist")),
        ..Default::default()
    }
}

fn build(source: &str, options: CompileOptions) -> CompiledHtml {
    compile(source, options).unwrap_or_else(|e| panic!("{source}: {e}"))
}

fn frame(c: &mut CompiledHtml, time: f64) -> Vec<u8> {
    let mut surface = skia_safe::surfaces::raster_n32_premul((SIZE, SIZE)).unwrap();
    render_frame(&mut c.director, time, &mut surface).unwrap();
    let info = skia_safe::ImageInfo::new((SIZE, SIZE), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Unpremul, None);
    let mut pixels = vec![0; (SIZE * SIZE * 4) as usize];
    assert!(surface.read_pixels(&info, &mut pixels, SIZE as usize * 4, (0, 0)));
    pixels
}

fn pixel(p: &[u8], x: i32, y: i32) -> [u8; 3] {
    let i = ((y * SIZE + x) * 4) as usize;
    [p[i], p[i + 1], p[i + 2]]
}

fn graded(lut: &Lut3d, rgb: [u8; 3]) -> [f32; 3] {
    lut.apply(rgb.map(|v| v as f32 / 255.0)).map(|v| v * 255.0)
}

fn near(got: [u8; 3], want: [f32; 3], slack: f32, what: &str) {
    assert!((0..3).all(|k| (got[k] as f32 - want[k]).abs() <= slack), "{what}: {got:?} vs {want:?}");
}

/// A full-frame swatch box carrying `style` and an optional animation block.
fn swatch(style: &str, extra: &str) -> String {
    let [r, g, b] = SWATCH;
    format!(
        "<style>body{{margin:0;background:black}}#box{{width:{SIZE}px;height:{SIZE}px;background:rgb({r},{g},{b});{style}}}{extra}</style>\
         <div id='box'></div>"
    )
}

#[test]
fn every_preset_grades_a_clip_like_its_lut() {
    for name in PRESETS {
        let mut c = build(&swatch(&format!("--grade:{name}"), ""), options());
        let lut = preset(name).unwrap();
        near(pixel(&frame(&mut c, 0.0), 48, 48), graded(&lut, SWATCH), 1.5, name);
    }
    let mut ungraded = build(&swatch("--grade:none", ""), options());
    near(pixel(&frame(&mut ungraded, 0.0), 48, 48), SWATCH.map(f32::from), 0.5, "none");
}

#[test]
fn intensity_blends_the_grade_with_the_original() {
    let lut = preset("teal-orange").unwrap();
    let full = graded(&lut, SWATCH);
    for (value, t) in [("0", 0.0), ("50%", 0.5), ("0.25", 0.25)] {
        let mut c = build(&swatch(&format!("--grade:teal-orange;--grade-intensity:{value}"), ""), options());
        let want: [f32; 3] = std::array::from_fn(|k| SWATCH[k] as f32 + (full[k] - SWATCH[k] as f32) * t);
        near(pixel(&frame(&mut c, 0.0), 48, 48), want, 1.5, value);
    }
}

#[test]
fn a_cube_file_grades_from_a_url() {
    let dir = tempfile::tempdir().unwrap();
    // Two points per axis that swap red and blue.
    let mut cube = String::from("TITLE \"swap\"\nLUT_3D_SIZE 2\n");
    for b in 0..2 {
        for g in 0..2 {
            for r in 0..2 {
                cube.push_str(&format!("{b} {g} {r}\n"));
            }
        }
    }
    std::fs::write(dir.path().join("swap.cube"), cube).unwrap();
    let source = swatch("--grade:url(\"swap.cube\")", "");
    let mut c = build(&source, CompileOptions { base_dir: dir.path().into(), ..options() });
    near(pixel(&frame(&mut c, 0.0), 48, 48), [60.0, 120.0, 200.0], 1.0, "swap");

    let missing = compile(&swatch("--grade:url(\"gone.cube\")", ""), CompileOptions { base_dir: dir.path().into(), ..options() });
    assert!(missing.err().unwrap().to_string().contains("--grade"));
}

#[test]
fn unknown_presets_and_extra_luts_are_errors() {
    let unknown = compile(&swatch("--grade:sunset", ""), options()).err().unwrap().to_string();
    assert!(unknown.contains("unknown preset 'sunset'") && unknown.contains("film-print"), "{unknown}");
    let three = compile(&swatch("--grade:mono night film-print", ""), options()).err().unwrap().to_string();
    assert!(three.contains("one or two LUTs"), "{three}");
}

#[test]
fn a_parent_grade_covers_its_children_once() {
    let [r, g, b] = SWATCH;
    let source = format!(
        "<style>body{{margin:0;background:black}}#frame{{width:{SIZE}px;height:{SIZE}px;--grade:mono}}\
         #child{{width:48px;height:48px;background:rgb({r},{g},{b})}}</style>\
         <div id='frame'><div id='child'></div></div>"
    );
    let mut c = build(&source, options());
    let pixels = frame(&mut c, 0.0);
    near(pixel(&pixels, 24, 24), graded(&preset("mono").unwrap(), SWATCH), 1.5, "child");
    near(pixel(&pixels, 72, 72), graded(&preset("mono").unwrap(), [0, 0, 0]), 1.5, "frame background");
}

fn crossfade(time: f64) -> [f32; 3] {
    let (a, b) = (graded(&preset("film-print").unwrap(), SWATCH), graded(&preset("night").unwrap(), SWATCH));
    let t = time.clamp(0.0, 1.0) as f32;
    std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t)
}

#[test]
fn css_keyframes_crossfade_two_presets_over_one_second() {
    let source = swatch(
        "--grade:film-print night;animation:fade 1s linear both",
        "@keyframes fade{from{--grade-mix:0}to{--grade-mix:1}}",
    );
    let mut c = build(&source, options());
    for time in [0.0, 0.25, 0.5, 1.0, 1.5] {
        near(pixel(&frame(&mut c, time), 48, 48), crossfade(time), 1.5, &format!("t={time}"));
    }
}

#[test]
fn gsap_tweens_the_grade_mix_into_a_native_track() {
    let [r, g, b] = SWATCH;
    let source = format!(
        "<style>body{{margin:0;background:black}}#box{{width:{SIZE}px;height:{SIZE}px;background:rgb({r},{g},{b});\
         --grade:film-print night;--grade-mix:0}}</style>\
         <div data-composition-id='main' data-width='{SIZE}' data-height='{SIZE}' data-duration='2'><div id='box'></div></div>\
         <script src='https://cdn.jsdelivr.net/npm/gsap@3.14.2/dist/gsap.min.js'></script>\
         <script>gsap.fromTo('#box', {{'--grade-mix': 0}}, {{'--grade-mix': 1, duration: 1, ease: 'none'}})</script>"
    );
    let mut c = build(&source, options());
    let timeline = c.timeline.as_ref().expect("GSAP timeline");
    assert!(timeline.tracks.iter().any(|t| t.property == "--grade-mix"), "{:?}", timeline.tracks);
    for time in [0.0, 0.5, 1.0] {
        near(pixel(&frame(&mut c, time), 48, 48), crossfade(time), 2.0, &format!("t={time}"));
    }
}

#[test]
fn grades_round_trip_through_the_scene_document() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("mono.cube"), preset("mono").unwrap().to_cube("mono")).unwrap();
    let source = swatch(
        "--grade:warm-fade url(mono.cube);--grade-mix:0.5;--grade-intensity:0.8",
        "",
    );
    let mut compiled = build(&source, CompileOptions { base_dir: dir.path().into(), ..options() });
    let document = export_scene(&compiled.director).unwrap();
    let parsed = SceneDocument::from_json(&document.to_json().unwrap()).unwrap();
    assert_eq!(parsed, document);
    let grade = parsed
        .nodes
        .iter()
        .find_map(|node| match &node.body {
            NodeBody::Html(html) => html.paint.grade.clone(),
            _ => None,
        })
        .expect("a graded box");
    assert_eq!(grade.from, GradeLut::Preset { name: "warm-fade".into() });
    assert!(matches!(grade.to, Some(GradeLut::Cube { .. })));
    assert_eq!((grade.mix, grade.intensity), (0.5, 0.8));

    let mut registry = director_core::scene_import::ImportRegistry::core();
    director_html::register_import(&mut registry);
    let renderer = director_render::Renderer::new(Arc::new(parsed), Arc::new(director_core::DefaultAssetLoader))
        .with_registry(registry);
    let imported = renderer.render_frame(0.0).unwrap();
    let original = frame(&mut compiled, 0.0);
    let i = ((48 * SIZE + 48) * 4) as usize;
    let got = [imported.rgba[i], imported.rgba[i + 1], imported.rgba[i + 2]];
    near(got, pixel(&original, 48, 48).map(f32::from), 1.0, "imported");
}
