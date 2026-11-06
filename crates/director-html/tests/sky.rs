//! `<sky>` children of `<model>`: a clear sky drawn behind `mirror.glb`'s
//! floor, its sun warming as it sets and casting shadows away from its
//! azimuth, the scene document keeping the probe and the sun, and the
//! compile errors. GPU tests skip on a machine without a Vulkan adapter.

use director_core::scene_export::export_scene;
use director_html::{compile, CompileOptions, CompiledHtml};
use director_render::render_frame;
use director_scene::{ModelLightKind, NodeBody, SceneDocument};
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
/// `attrs` and holding `inner`.
fn page(attrs: &str, inner: &str) -> String {
    format!(
        "<style>body{{margin:0}}#main{{background:rgb(0,0,0)}}model{{display:block;width:{W}px;height:{H}px}}</style>\
         <div id='main' data-composition-id='main' data-width='{W}' data-height='{H}' data-duration='2'>\
         <model id='m' src='{}' {attrs}><text3d position='0 0 0' size='1' depth='0.3' color='white' roughness='1'>I</text3d>{inner}</model></div>",
        fixture("mirror.glb")
    )
}

/// The page with a sky of `attrs` drawn behind the model.
fn sky(attrs: &str) -> String {
    page("background", &format!("<sky {attrs}></sky>"))
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

/// The floor in front of the type, below it in the frame.
fn floor(p: &[u8]) -> [f64; 3] {
    mean(p, (20, W - 20), (H - 24, H - 4))
}

#[test]
fn the_sky_is_blue_overhead_and_lights_the_scene_as_daylight() {
    if !gpu() {
        return;
    }
    let p = shot(&sky("elevation='45'"));
    let top = mean(&p, (0, W), (0, 6));
    let floor = floor(&p);
    eprintln!("top of the frame {top:?}, floor {floor:?}");
    assert!(top[2] > top[0] + 20.0, "the sky above the floor is blue");
    // EV100 14 under the sun and sky lands the grey floor mid-curve.
    assert!((70.0..230.0).contains(&luma(floor)), "daylight exposes the floor: {floor:?}");
    let transparent = shot(&page("", "<sky elevation='45'></sky>"));
    assert!(mean(&transparent, (0, W), (0, 6)).iter().all(|&c| c < 3.0), "without background the page shows through");
}

#[test]
fn a_low_sun_lights_the_floor_warm_and_a_high_one_near_white() {
    if !gpu() {
        return;
    }
    let low = floor(&shot(&sky("elevation='4' azimuth='0'")));
    let high = floor(&shot(&sky("elevation='70' azimuth='0'")));
    eprintln!("floor under a sun 4 degrees up {low:?}, 70 degrees up {high:?}");
    assert!(low[0] > 1.3 * low[2], "a setting sun lights the floor orange");
    assert!(high[0] < 1.15 * high[2], "a high sun and the blue sky light it near neutral");
    assert!(luma(high) > luma(low), "a high sun lights the floor brighter");
}

#[test]
fn the_sun_casts_shadows_away_from_its_azimuth() {
    if !gpu() {
        return;
    }
    // Azimuth 90 puts the sun at +X, so the pillar's shadow falls to -X.
    // A shadowed floor pixel is a quarter darker than the lit floor.
    let shadowed = |p: &[u8], x0: i32, x1: i32| {
        let lit = luma(floor(p));
        (H / 2..H - 20)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|&(x, y)| luma(mean(p, (x, x + 1), (y, y + 1))) < 0.75 * lit)
            .count()
    };
    let (mid, gap) = (W / 2, 6);
    for (azimuth, toward) in [(90, "west"), (-90, "east")] {
        let p = shot(&sky(&format!("elevation='15' azimuth='{azimuth}'")));
        let (west, east) = (shadowed(&p, 20, mid - gap), shadowed(&p, mid + gap, W - 20));
        eprintln!("sun at azimuth {azimuth}: {west} shadowed px to the west, {east} to the east");
        let (near, far) = if azimuth > 0 { (east, west) } else { (west, east) };
        assert!(far > 20 && near < far / 4, "a sun at azimuth {azimuth} casts its shadow {toward}");
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
fn the_scene_document_keeps_the_sky_and_its_sun() {
    let source = sky("elevation='30' azimuth='45' intensity='100000'");
    let document = export_scene(&build(&source).director).unwrap();
    let mut parsed = SceneDocument::from_json(&document.to_json().unwrap()).unwrap();
    assert_eq!(parsed, document);
    let body = model_of(&mut parsed).clone();
    assert!(body.environment.is_some(), "the document embeds the sky's probe");
    assert_eq!(body.environment_intensity, 100000.0);
    assert_eq!(body.exposure, 14.0);
    let suns: Vec<_> = body.lights.iter().filter(|l| l.kind == ModelLightKind::Sun).collect();
    assert_eq!(suns.len(), 1, "{:?}", body.lights);
    let (sun, e, a) = (suns[0].position.unwrap(), 30f32.to_radians(), 45f32.to_radians());
    let truth = [e.cos() * a.sin(), e.sin(), e.cos() * a.cos()];
    assert!(sun.iter().zip(truth).all(|(s, t)| (s - t).abs() < 1e-4), "{sun:?}");
    // The air takes some of the sun, more of its blue.
    assert!(suns[0].intensity < 100000.0 && suns[0].intensity > 70000.0, "{}", suns[0].intensity);
    assert!(suns[0].color[0] == 1.0 && suns[0].color[2] < 0.95, "{:?}", suns[0].color);
    let set = model_of(&mut export_scene(&build(&sky("elevation='-5'")).director).unwrap()).clone();
    assert!(set.lights.iter().all(|l| l.kind != ModelLightKind::Sun), "a set sun adds no light");
    if gpu() {
        let imported = import(parsed).render_frame(0.0).unwrap();
        let live = shot(&source);
        let changed = imported.rgba.chunks(4).zip(live.chunks(4)).filter(|(p, q)| (0..3).any(|k| p[k].abs_diff(q[k]) > 2)).count();
        assert_eq!(changed, 0, "the imported sky renders as compiled");
    }
}

#[test]
fn malformed_skies_fail_to_compile() {
    let error = |source: &str| compile(source, options()).err().expect("a compile error").to_string();
    let e = error(&page("", "<sky></sky><sky></sky>"));
    assert!(e.contains("a model takes one <sky>"), "{e}");
    let e = error(&page(&format!("environment='{}'", fixture("sky.hdr")), "<sky></sky>"));
    assert!(e.contains("a <sky> lights the model, so it cannot take an environment or track-light"), "{e}");
    let e = error(&page("", "<sky elevation='95'></sky>"));
    assert!(e.contains("sky elevation '95' must be a number from -10 to 90"), "{e}");
    let e = error(&page("", "<sky elevation='12deg' haze='-1'></sky>"));
    assert!(e.contains("sky haze '-1' must be a number from 0 to 50"), "{e}");
    let e = error(&page("", "<sky azimuth='east'></sky>"));
    assert!(e.contains("sky azimuth 'east' must be a number"), "{e}");
    let e = error(&page("", "<sky ground='nope'></sky>"));
    assert!(e.contains("sky ground "), "{e}");
}
