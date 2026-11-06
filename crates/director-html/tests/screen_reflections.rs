//! Screen-space reflections on `<model>`: a glossy floor reflecting the type
//! standing on it, the roughness cutoff, the forward frame coming back when
//! they turn off, a transparent sky staying transparent, the scene document
//! keeping them and the compile errors. GPU tests skip on a machine without
//! a Vulkan adapter.

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
    CompileOptions { width: W, height: H, duration: 2.0, ..Default::default() }
}

/// `gloss.glb`'s dark glossy floor holding yellow type, the model taking
/// `attrs`. The floor and the studio probe are neutral, so yellow on the
/// floor comes from the type's reflection alone.
fn page(attrs: &str) -> String {
    format!(
        "<style>body{{margin:0}}#main{{background:rgb(0,100,0)}}model{{display:block;width:{W}px;height:{H}px}}</style>\
         <div id='main' data-composition-id='main' data-width='{W}' data-height='{H}' data-duration='2'>\
         <model id='m' src='{}' {attrs}><text3d position='0 0.6 0' size='1' color='#f2c040'>LP</text3d></model></div>",
        fixture("gloss.glb")
    )
}

fn build(source: &str) -> CompiledHtml {
    compile(source, options()).unwrap_or_else(|e| panic!("{source}: {e}"))
}

fn shot(source: &str) -> Vec<u8> {
    let mut c = build(source);
    let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).unwrap();
    render_frame(&mut c.director, 0.0, &mut surface).unwrap();
    let info = skia_safe::ImageInfo::new((W, H), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Unpremul, None);
    let mut pixels = vec![0; (W * H * 4) as usize];
    assert!(surface.read_pixels(&info, &mut pixels, W as usize * 4, (0, 0)));
    pixels
}

/// The mean colour of the pixels in columns `x0..x1` and rows `y0..y1`.
fn mean(p: &[u8], (x0, x1): (i32, i32), (y0, y1): (i32, i32)) -> [f64; 3] {
    let mut sum = [0.0; 3];
    for y in y0..y1 {
        for x in x0..x1 {
            for k in 0..3 {
                sum[k] += p[((y * W + x) * 4) as usize + k] as f64;
            }
        }
    }
    sum.map(|s| s / ((x1 - x0) * (y1 - y0)) as f64)
}

/// How far red and green rise above blue: the floor and probe are neutral,
/// so only the type and its reflection read yellow.
fn yellow(c: [f64; 3]) -> f64 {
    (c[0] + c[1]) / 2.0 - c[2]
}

fn luma(c: [f64; 3]) -> f64 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// The largest channel difference between two frames.
fn max_diff(p: &[u8], q: &[u8]) -> u8 {
    p.iter().zip(q).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0)
}

/// The floor below the type, where its reflection lands.
const UNDER: ((i32, i32), (i32, i32)) = ((90, 150), (86, 135));
/// Open floor to the left that reflects only the probe.
const OPEN: ((i32, i32), (i32, i32)) = ((10, 60), (100, 150));

#[test]
fn a_glossy_floor_reflects_the_type_standing_on_it() {
    if !gpu() {
        return;
    }
    let (plain, ssr) = (shot(&page("background")), shot(&page("background screen-reflections")));
    let under = |p: &[u8]| yellow(mean(p, UNDER.0, UNDER.1));
    let open = |p: &[u8]| luma(mean(p, OPEN.0, OPEN.1));
    eprintln!("yellow under the type {:.2} -> {:.2}, open floor luma {:.1} -> {:.1}", under(&plain), under(&ssr), open(&plain), open(&ssr));
    assert!(under(&ssr) > under(&plain) + 2.5, "the floor below the type reflects it");
    assert!((open(&ssr) - open(&plain)).abs() < 1.5, "open floor keeps reflecting the probe");
    // Each pixel averages its rays, so no lone pixel of the open floor lights
    // up yellow from a stray ray.
    let stray = (100..H).flat_map(|y| (0..W).map(move |x| (x, y))).filter(|&(x, y)| {
        let inside = (UNDER.0 .0 - 10..UNDER.0 .1 + 10).contains(&x);
        let c = |p: &[u8]| yellow(mean(p, (x, x + 1), (y, y + 1)));
        !inside && c(&ssr) - c(&plain) > 8.0
    });
    assert_eq!(stray.count(), 0, "no stray reflections on the open floor");
}

#[test]
fn surfaces_rougher_than_the_cutoff_keep_the_probe() {
    if !gpu() {
        return;
    }
    // The floor's roughness is 0.05, so a 0.02 cutoff leaves it to the probe.
    let (plain, cut) = (shot(&page("background")), shot(&page("background screen-reflections='0.02'")));
    assert!(max_diff(&plain, &cut) <= 2, "a cutoff below every surface changes nothing: {}", max_diff(&plain, &cut));
}

#[test]
fn turning_reflections_off_restores_the_forward_frame() {
    if !gpu() {
        return;
    }
    let plain = shot(&page("background"));
    let ssr = shot(&page("background screen-reflections"));
    assert!(max_diff(&plain, &ssr) > 8, "the reflections change the frame");
    let again = shot(&page("background"));
    assert_eq!(max_diff(&plain, &again), 0, "the stage renders forward again");
    assert_eq!(max_diff(&ssr, &shot(&page("background screen-reflections"))), 0, "the reflections are deterministic");
}

#[test]
fn a_transparent_sky_stays_clear_with_reflections() {
    if !gpu() {
        return;
    }
    // The page is green under the model's transparent sky.
    let p = shot(&page("screen-reflections"));
    let sky = mean(&p, (0, W), (0, 40));
    eprintln!("sky over the page {sky:?}");
    assert!(sky[0] < 3.0 && (sky[1] - 100.0).abs() < 3.0 && sky[2] < 3.0, "the page shows through the sky");
    let floor = luma(mean(&p, OPEN.0, OPEN.1));
    assert!(floor > 60.0, "the floor still covers the page: {floor}");
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
fn the_scene_document_keeps_screen_reflections() {
    let source = page("background screen-reflections='0.3'");
    let document = export_scene(&build(&source).director).unwrap();
    let mut parsed = SceneDocument::from_json(&document.to_json().unwrap()).unwrap();
    assert_eq!(parsed, document);
    assert_eq!(model_of(&mut parsed).screen_reflections, 0.3);
    let bare = model_of(&mut export_scene(&build(&page("screen-reflections")).director).unwrap()).screen_reflections;
    assert_eq!(bare, 0.6, "a bare attribute reflects up to 0.6");
    let plain = model_of(&mut export_scene(&build(&page("background")).director).unwrap()).screen_reflections;
    assert_eq!(plain, 0.0, "a plain model adds no reflections");
    if gpu() {
        let imported = import(parsed).render_frame(0.0).unwrap();
        assert_eq!(max_diff(&imported.rgba, &shot(&source)), 0, "the imported reflections render as compiled");
    }
}

#[test]
fn malformed_screen_reflections_fail_to_compile() {
    let error = |source: &str| compile(source, options()).err().expect("a compile error").to_string();
    for value in ["1.5", "0", "glossy"] {
        let e = error(&page(&format!("screen-reflections='{value}'")));
        assert!(e.contains(&format!("screen-reflections '{value}' must be a roughness above 0 and up to 1")), "{e}");
    }
    let e = error(&page("screen-reflections paths='4'"));
    assert!(e.contains("paths trace reflections themselves, so they cannot take screen-reflections"), "{e}");
}
