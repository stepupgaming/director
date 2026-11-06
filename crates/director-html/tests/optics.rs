//! Lens effects and ambient occlusion on `<model>`: a vignette darkening
//! the corners, chromatic aberration fringing edges, lens distortion bending
//! the frame, SSAO darkening the floor where the type stands, transparent
//! skies staying transparent through them, the scene document keeping them
//! and the compile errors. GPU tests skip on a machine without a Vulkan
//! adapter.

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

/// `mirror.glb`'s floor holding a white pillar of type, the model taking
/// `attrs`.
fn page(attrs: &str) -> String {
    format!(
        "<style>body{{margin:0}}#main{{background:rgb(0,100,0)}}model{{display:block;width:{W}px;height:{H}px}}</style>\
         <div id='main' data-composition-id='main' data-width='{W}' data-height='{H}' data-duration='2'>\
         <model id='m' src='{}' {attrs}><text3d position='0 0 0' size='1' depth='0.3' color='white' roughness='1'>I</text3d></model></div>",
        fixture("mirror.glb")
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

fn luma(c: [f64; 3]) -> f64 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn luma_at(p: &[u8], x: i32, y: i32) -> f64 {
    luma(mean(p, (x, x + 1), (y, y + 1)))
}

#[test]
fn a_vignette_darkens_the_corners_and_leaves_the_centre() {
    if !gpu() {
        return;
    }
    let (plain, dark) = (shot(&page("background")), shot(&page("background vignette='0.6'")));
    let corner = |p: &[u8]| luma(mean(p, (0, 16), (0, 16)));
    let centre = |p: &[u8]| luma(mean(p, (W / 2 - 30, W / 2 + 30), (H / 2 + 10, H / 2 + 30)));
    eprintln!("corner {:.1} -> {:.1}, centre {:.1} -> {:.1}", corner(&plain), corner(&dark), centre(&plain), centre(&dark));
    assert!(corner(&dark) < 0.85 * corner(&plain), "the vignette darkens the corners");
    assert!(centre(&dark) > 0.95 * centre(&plain), "the vignette leaves the centre");
}

#[test]
fn chromatic_aberration_fringes_edges_with_colour() {
    if !gpu() {
        return;
    }
    // The studio probe, the grey floor and the white type are all neutral,
    // so colour appears only where the lens splits it.
    let coloured = |p: &[u8]| {
        p.chunks(4).filter(|c| c[..3].iter().max().unwrap() - c[..3].iter().min().unwrap() > 25).count()
    };
    let (plain, split) = (coloured(&shot(&page("background"))), coloured(&shot(&page("background chromatic-aberration='0.2'"))));
    eprintln!("coloured pixels {plain} -> {split}");
    assert!(plain < 10 && split > 200, "aberration fringes the horizon and the type");
}

#[test]
fn lens_distortion_bends_straight_lines() {
    if !gpu() {
        return;
    }
    // The floor's far edge runs straight across the frame above its centre.
    // A barrel pulls its ends down toward the centre and a pincushion pushes
    // them out, so fewer or more columns keep the edge at the centre row.
    let level = |p: &[u8]| {
        let edge = |x: i32| (1..H / 2 + 20).max_by(|&a, &b| {
            let rise = |y: i32| luma_at(p, x, y) - luma_at(p, x, y - 1);
            rise(a).total_cmp(&rise(b))
        })
        .expect("rows");
        let mid = edge(W / 2);
        (0..W).filter(|&x| edge(x).abs_diff(mid) <= 1).count()
    };
    let plain = level(&shot(&page("background")));
    let barrel = level(&shot(&page("background lens-distortion='0.5'")));
    let pincushion = level(&shot(&page("background lens-distortion='-0.5'")));
    eprintln!("columns on the edge's centre row: plain {plain}, barrel {barrel}, pincushion {pincushion}");
    assert!(barrel + 10 < plain && pincushion > plain + 10);
}

#[test]
fn ambient_occlusion_darkens_the_floor_beside_the_type() {
    if !gpu() {
        return;
    }
    let (plain, ao) = (shot(&page("background")), shot(&page("background ambient-occlusion")));
    // The pillar of type stands in columns 118 to 121 and rows 78 to 93.
    let darkening = |x0: i32, x1: i32| luma(mean(&plain, (x0, x1), (80, 92))) - luma(mean(&ao, (x0, x1), (80, 92)));
    let beside = (darkening(116, 118) + darkening(122, 124)) / 2.0;
    let far = darkening(30, 60);
    eprintln!("darkening beside the type {beside:.1}, on the open floor {far:.1}");
    assert!(beside > 8.0 && beside > 1.4 * far, "the floor darkens where the type meets it");
}

#[test]
fn a_transparent_sky_stays_clear_through_the_lens() {
    if !gpu() {
        return;
    }
    // The page is green under the model's transparent sky.
    let p = shot(&page("vignette='0.6' chromatic-aberration='0.2' lens-distortion='0.5'"));
    let sky = mean(&p, (0, W), (0, 40));
    eprintln!("sky over the page {sky:?}");
    assert!(sky[0] < 3.0 && (sky[1] - 100.0).abs() < 3.0 && sky[2] < 3.0, "the page shows through the sky");
    let floor = luma(mean(&p, (W / 2 - 30, W / 2 + 30), (H / 2 + 10, H / 2 + 30)));
    assert!(floor > 150.0, "the floor still covers the page: {floor}");
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
fn the_scene_document_keeps_the_lens_and_the_occlusion() {
    let source = page("background vignette='40%' chromatic-aberration='0.03' lens-distortion='-0.2' ambient-occlusion='0.5m'");
    let document = export_scene(&build(&source).director).unwrap();
    let mut parsed = SceneDocument::from_json(&document.to_json().unwrap()).unwrap();
    assert_eq!(parsed, document);
    let body = model_of(&mut parsed).clone();
    assert_eq!(body.optics, Some(director_scene::ModelOptics { vignette: 0.4, aberration: 0.03, distortion: -0.2 }));
    assert_eq!(body.occlusion, 0.5);
    let plain = model_of(&mut export_scene(&build(&page("background")).director).unwrap()).clone();
    assert_eq!((plain.optics, plain.occlusion), (None, 0.0), "a plain model adds no effects");
    if gpu() {
        let imported = import(parsed).render_frame(0.0).unwrap();
        let live = shot(&source);
        let changed = imported.rgba.chunks(4).zip(live.chunks(4)).filter(|(p, q)| (0..3).any(|k| p[k].abs_diff(q[k]) > 2)).count();
        assert_eq!(changed, 0, "the imported effects render as compiled");
    }
}

#[test]
fn malformed_lens_effects_fail_to_compile() {
    let error = |source: &str| compile(source, options()).err().expect("a compile error").to_string();
    let e = error(&page("vignette='1.5'"));
    assert!(e.contains("vignette '1.5' must be a number from 0 to 1"), "{e}");
    let e = error(&page("chromatic-aberration='lots'"));
    assert!(e.contains("chromatic-aberration 'lots' must be a number from 0 to 1"), "{e}");
    let e = error(&page("lens-distortion='-2'"));
    assert!(e.contains("lens-distortion '-2' must be a number from -1 to 1"), "{e}");
    let e = error(&page("ambient-occlusion='-1'"));
    assert!(e.contains("ambient-occlusion '-1' must be a radius in metres above 0"), "{e}");
    let e = error(&page("ambient-occlusion paths='4'"));
    assert!(e.contains("paths trace occlusion themselves, so they cannot take ambient-occlusion"), "{e}");
}
