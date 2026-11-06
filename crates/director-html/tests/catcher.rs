//! `<model shadow-catcher>`: glTF nodes whose meshes show only the shadows
//! cast on them, so a transparent model over footage darkens the plate
//! where its shadows fall. The matchmove test composites `track.glb` and its
//! solved camera over the `track.mp4` plate and compares the result with
//! Eevee's render of the same shot with the box; `make_track.py` writes all
//! three. Each test skips on a machine without a Vulkan adapter.

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
        ..Default::default()
    }
}

/// The test GLB over a page of `background`, the model taking `attrs`.
fn page(background: &str, attrs: &str) -> String {
    format!(
        "<style>body{{margin:0}}#main{{background:{background}}}model{{display:block;width:{W}px;height:{H}px}}</style>\
         <div id='main' data-composition-id='main' data-width='{W}' data-height='{H}' data-duration='2'>\
         <model id='m' src='{}' {attrs}></model></div>",
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

fn rgb(p: &[u8], i: usize) -> [u8; 3] {
    [p[i * 4], p[i * 4 + 1], p[i * 4 + 2]]
}

/// The page grey that measures coverage without clipping.
const GREY: u8 = 128;

/// Each pixel's coverage and colour from the model over a grey page and a
/// black page. Bloom glows over the transparent sky as added light, so it
/// cancels in the difference: `grey - black = (1 - alpha) * GREY`.
struct Roles {
    alpha: Vec<f64>,
    /// The page shows through the catcher unchanged.
    clear: Vec<bool>,
    /// The catcher covers part of the page with black.
    shadow: Vec<bool>,
    /// The model's own meshes cover the page.
    object: Vec<bool>,
}

fn roles(on_grey: &[u8], on_black: &[u8]) -> Roles {
    let n = (W * H) as usize;
    let mut roles = Roles { alpha: vec![0.0; n], clear: vec![false; n], shadow: vec![false; n], object: vec![false; n] };
    for i in 0..n {
        let (g, b) = (rgb(on_grey, i), rgb(on_black, i));
        let shown: Vec<f64> = (0..3).map(|k| (g[k] as f64 - b[k] as f64) / GREY as f64).collect();
        let alpha = 1.0 - shown.iter().sum::<f64>() / 3.0;
        roles.alpha[i] = alpha;
        roles.clear[i] = alpha < 0.03;
        roles.shadow[i] = (0.08..0.9).contains(&alpha);
        roles.object[i] = alpha > 0.97;
    }
    roles
}

/// The densest catcher coverage at least 2 px from the objects' antialiased edges.
fn darkest(roles: &Roles) -> f64 {
    let near = |x: i32, y: i32| {
        (-2..=2).any(|dy| (-2..=2).any(|dx| {
            let (x, y) = (x + dx, y + dy);
            (0..W).contains(&x) && (0..H).contains(&y) && roles.alpha[(y * W + x) as usize] > 0.9
        }))
    };
    (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .filter(|&(x, y)| !near(x, y))
        .map(|(x, y)| roles.alpha[(y * W + x) as usize])
        .fold(0.0, f64::max)
}

fn count(mask: &[bool]) -> usize {
    mask.iter().filter(|&&m| m).count()
}

fn iou(a: &[bool], b: &[bool]) -> f64 {
    let both = a.iter().zip(b).filter(|(a, b)| **a && **b).count();
    let either = a.iter().zip(b).filter(|(a, b)| **a || **b).count();
    both as f64 / either.max(1) as f64
}

fn grey() -> String {
    format!("rgb({GREY},{GREY},{GREY})")
}

const CATCH: &str = "shadow-catcher='Plane'";

#[test]
fn a_catcher_darkens_the_page_only_where_shadows_fall() {
    if !gpu() {
        return;
    }
    let on_grey = shot(&page(&grey(), CATCH));
    let on_black = shot(&page("rgb(0,0,0)", CATCH));
    let roles = roles(&on_grey, &on_black);
    let total = (W * H) as usize;
    let (clear, shadow, object) = (count(&roles.clear), count(&roles.shadow), count(&roles.object));
    let darkest = darkest(&roles);
    eprintln!("clear {clear}, shadow {shadow}, object {object} of {total}, darkest shadow {darkest:.2}");
    assert!(object > 300, "the cube and sphere draw over either page: {object}");
    assert!(shadow > 60, "the shadows darken the page: {shadow}");
    assert!(clear > total / 4, "the lit ground shows the page: {clear}");
    assert!(darkest > 0.1 && darkest < 0.6, "ambient light keeps a full shadow from black: {darkest:.2}");

    // Without the catcher the plane draws, so the ground covers the page.
    let plain = self::roles(&shot(&page(&grey(), "")), &shot(&page("rgb(0,0,0)", "")));
    let plane: Vec<usize> = (0..total).filter(|&i| plain.object[i] && !roles.object[i]).collect();
    let caught = plane.iter().filter(|&&i| roles.clear[i] || roles.shadow[i]).count();
    eprintln!("plane {}, caught {caught}", plane.len());
    assert!(plane.len() > total / 4, "the plane covers the ground: {}", plane.len());
    assert!(caught > plane.len() * 19 / 20, "{caught} of {} plane pixels turn into the catcher", plane.len());
}

#[test]
fn a_path_traced_model_casts_the_same_shadows_on_its_catcher() {
    if !gpu() || !director_3d::traceable() {
        eprintln!("skipped: no ray tracing Vulkan adapter");
        return;
    }
    let raster = roles(&shot(&page(&grey(), CATCH)), &shot(&page("rgb(0,0,0)", CATCH)));
    let hero_attrs = format!("{CATCH} paths='64'");
    let hero = roles(&shot(&page(&grey(), &hero_attrs)), &shot(&page("rgb(0,0,0)", &hero_attrs)));
    eprintln!("darkest raster {:.2} hero {:.2}", darkest(&raster), darkest(&hero));
    let overlap = iou(&raster.shadow, &hero.shadow);
    eprintln!("hero shadow {} raster shadow {} iou {overlap:.2}", count(&hero.shadow), count(&raster.shadow));
    assert!(count(&hero.shadow) > 60, "the traced frame catches shadows");
    assert!(overlap > 0.6, "shadow masks overlap by {overlap:.2}");
    assert!(iou(&raster.clear, &hero.clear) > 0.8, "the same ground shows the page");
}

#[test]
fn the_scene_document_keeps_the_catchers() {
    if !gpu() {
        return;
    }
    let mut compiled = build(&page(&grey(), CATCH));
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
    assert_eq!(model.shadow_catchers, vec!["Plane".to_string()]);

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
fn an_unknown_catcher_fails_to_compile() {
    let error = compile(&page("rgb(0,0,0)", "shadow-catcher='Plane Nope'"), options())
        .err()
        .expect("a compile error")
        .to_string();
    assert!(error.contains("shadow-catcher 'Nope' names no node in the model"), "{error}");
}

/// A matchmove: the plate under `track.glb`, its ground a catcher, with the
/// sky and units of `tests/eevee.rs`, at Blender frame `frame`. Without
/// `model` the plate shows alone.
fn matchmove(model: bool, frame: u32) -> Vec<u8> {
    let layer = if model {
        format!(
            "<model src='{}' environment='{}' environment-intensity='683' exposure='{}' shadow-catcher='Ground'></model>",
            fixture("track.glb"),
            fixture("sky.hdr"),
            (683.0f64 / 1.2).log2()
        )
    } else {
        String::new()
    };
    let source = format!(
        "<style>body{{margin:0}}video,model{{position:absolute;left:0;top:0;width:{W}px;height:{H}px}}</style>         <div id='main' data-composition-id='main' data-width='{W}' data-height='{H}' data-duration='2'>         <video muted src='{}'></video>{layer}</div>",
        fixture("track.mp4")
    );
    self::frame(&mut build(&source), frame as f64 / 30.0)
}

fn eevee(frame: u32) -> Vec<u8> {
    let bytes = std::fs::read(fixture(&format!("track-{frame}.png"))).unwrap();
    let image = skia_safe::Image::from_encoded(skia_safe::Data::new_copy(&bytes)).expect("a PNG");
    assert_eq!((image.width(), image.height()), (W, H));
    let info = skia_safe::ImageInfo::new((W, H), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Unpremul, None);
    let mut pixels = vec![0; (W * H * 4) as usize];
    assert!(image.read_pixels(&info, &mut pixels, W as usize * 4, (0, 0), skia_safe::image::CachingHint::Allow));
    pixels
}

fn luma(p: [u8; 3]) -> f64 {
    0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64
}

/// The box: blue well above red.
fn boxed(p: [u8; 3]) -> bool {
    p[2] as i32 > p[0] as i32 + 50
}

#[test]
fn a_matchmoved_model_casts_its_shadow_on_the_plate() {
    if !gpu() {
        return;
    }
    for frame in [1, 46] {
        let (reference, plate, composite) = (eevee(frame), matchmove(false, frame), matchmove(true, frame));
        let n = (W * H) as usize;
        let mask = |image: &[u8], f: &dyn Fn(usize, [u8; 3]) -> bool| (0..n).map(|i| f(i, rgb(image, i))).collect::<Vec<bool>>();
        let box_ref = mask(&reference, &|_, p| boxed(p));
        let box_dir = mask(&composite, &|_, p| boxed(p));
        let dark = |image: &[u8]| {
            let image = image.to_vec();
            mask(&image, &|i, p| !box_ref[i] && !box_dir[i] && luma(p) < luma(rgb(&plate, i)) - 6.0)
        };
        let (shadow_ref, shadow_dir) = (dark(&reference), dark(&composite));
        let both: Vec<usize> = (0..n).filter(|&i| shadow_ref[i] && shadow_dir[i]).collect();
        let ratio = |image: &[u8]| both.iter().map(|&i| luma(rgb(image, i)) / luma(rgb(&plate, i)).max(1.0)).sum::<f64>() / both.len().max(1) as f64;
        let error = |image: &[u8]| (0..n).map(|i| (0..3).map(|k| rgb(image, i)[k].abs_diff(rgb(&reference, i)[k]) as f64).sum::<f64>()).sum::<f64>() / (n * 3) as f64;
        let away: Vec<usize> = (0..n).filter(|&i| !box_ref[i] && !box_dir[i] && !shadow_ref[i] && !shadow_dir[i]).collect();
        let drift = away.iter().map(|&i| (0..3).map(|k| rgb(&composite, i)[k].abs_diff(rgb(&plate, i)[k]) as f64).sum::<f64>()).sum::<f64>() / (away.len() * 3) as f64;
        eprintln!(
            "frame {frame}: box {} / {} iou {:.2}; shadow {} / {} iou {:.2}; darkening eevee {:.2} director {:.2}; error plate {:.1} composite {:.1}; drift {drift:.2}",
            count(&box_ref), count(&box_dir), iou(&box_ref, &box_dir),
            count(&shadow_ref), count(&shadow_dir), iou(&shadow_ref, &shadow_dir),
            ratio(&reference), ratio(&composite), error(&plate), error(&composite)
        );
        assert!(iou(&box_ref, &box_dir) > 0.9, "the solved camera puts the box where Eevee does");
        assert!(iou(&shadow_ref, &shadow_dir) > 0.75, "the shadow falls where Eevee's does");
        assert!((ratio(&reference) - ratio(&composite)).abs() < 0.04, "the shadow darkens the plate as far as Eevee's");
        assert!(error(&composite) < error(&plate) * 0.5, "the composite comes closer to Eevee's shot than the plate");
        assert!(drift < 1.2, "the plate shows unchanged away from the box and its shadow: {drift:.2}");
    }
}
