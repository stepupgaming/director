//! `<fog>` children of `<model>`: distance haze against near and far type,
//! a sunlit fog volume, the scene document round trip and the compile
//! errors. GPU tests skip on a machine without a Vulkan adapter.

use director_core::scene_export::export_scene;
use director_html::{compile, CompileOptions, CompiledHtml};
use director_render::render_frame;
use director_scene::{ModelFog, NodeBody, SceneDocument};
use std::sync::Arc;

const W: i32 = 320;
const H: i32 = 160;

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

/// A full-frame model over a blue page holding `inner`.
fn page(model: &str, inner: &str) -> String {
    format!(
        "<style>body{{margin:0}}#main{{background:rgb(0,0,255)}}model{{display:block;width:{W}px;height:{H}px}}</style>\
         <div id='main' data-composition-id='main' data-width='{W}' data-height='{H}' data-duration='2'>\
         <model id='m' {model}>{inner}</model></div>"
    )
}

/// White type near the camera on the left and far from it on the right,
/// with `fog`.
fn lines(fog: &str) -> String {
    page(
        "style='--camera-distance:0.4'",
        &format!(
            "<text3d position='-1 -0.5 0' color='white' roughness='1'>N</text3d>\
             <text3d position='3 -2 -20' size='4' color='white' roughness='1'>F</text3d>{fog}"
        ),
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

/// The pixels the type covers in full in a frame without fog: red well
/// above the blue page's, which antialiased edges blend toward.
fn solid(clear: &[u8]) -> Vec<bool> {
    clear.chunks(4).map(|px| px[0] > 120).collect()
}

/// The mean colour of the `solid` pixels in columns `x0..x1`.
fn ink(p: &[u8], solid: &[bool], x0: i32, x1: i32) -> [f32; 3] {
    let mut sum = [0.0; 3];
    let mut n = 0.0;
    for (i, px) in p.chunks(4).enumerate() {
        let x = i as i32 % W;
        if (x0..x1).contains(&x) && solid[i] {
            for k in 0..3 {
                sum[k] += px[k] as f32;
            }
            n += 1.0;
        }
    }
    assert!(n > 50.0, "type shows in columns {x0} to {x1}");
    sum.map(|s| s / n)
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.iter().zip(b).map(|(a, b)| (a - b).powi(2)).sum::<f32>().sqrt()
}

/// The mean red of row `y` across columns `x - 3 ..= x + 3`.
fn spot(p: &[u8], x: i32, y: i32) -> f32 {
    (x - 3..=x + 3).map(|x| p[((y * W + x) * 4) as usize] as f32).sum::<f32>() / 7.0
}

/// How far the darkest dip along row `y` falls below the line between the
/// points 12 px either side of it: shafts of shadow cut dips into lit fog,
/// and an evenly lit fog keeps the row smooth.
fn deepest_dip(p: &[u8], y: i32) -> f32 {
    (80..240).map(|x| (spot(p, x - 12, y) + spot(p, x + 12, y)) / 2.0 - spot(p, x, y)).fold(0.0, f32::max)
}

#[test]
fn haze_fades_far_type_toward_the_fog_colour() {
    if !gpu() {
        return;
    }
    let red = [255.0, 40.0, 40.0];
    let clear = shot(&lines(""));
    let hazed = shot(&lines("<fog color='rgb(255,40,40)' visibility='40'></fog>"));
    let thick = shot(&lines("<fog color='rgb(255,40,40)' visibility='0.5'></fog>"));
    let (half, mask) = (W / 2, solid(&clear));
    let ink = |p: &[u8], x0, x1| ink(p, &mask, x0, x1);
    let (near, far) = ([ink(&clear, 0, half), ink(&hazed, 0, half)], [ink(&clear, half, W), ink(&hazed, half, W)]);
    eprintln!("near type {:?} hazed {:?}, far type {:?} hazed {:?}", near[0], near[1], far[0], far[1]);
    // Haze thins exponentially with distance, so the far type keeps less
    // of its own colour than the near type and leans further to the fog.
    assert!(distance(far[1], red) + 40.0 < distance(near[1], red), "the far type lies deeper in the haze");
    assert!(distance(near[1], near[0]) + 40.0 < distance(far[1], far[0]), "the near type keeps more of its colour");
    let thick = [ink(&thick, 0, half), ink(&thick, half, W)];
    eprintln!("thick haze {thick:?}");
    // Haze much thinner than the scene's depth leaves only its colour,
    // which passes through the filmic curve as the type's own colour does.
    assert!(distance(thick[0], thick[1]) < 8.0, "near and far type both vanish into the haze");
    assert!(thick[0][0] > 190.0 && thick[0][0] - thick[0][1].max(thick[0][2]) > 130.0, "the haze shows its colour");
    // Haze fades surfaces, and the transparent sky shows the page unchanged.
    let page = |p: &[u8]| p.chunks(4).filter(|px| px[0] < 8 && px[1] < 8 && px[2] > 247).count();
    eprintln!("page pixels {} clear, {} hazed", page(&clear), page(&hazed));
    assert!(page(&hazed) > W as usize * H as usize * 9 / 10);
    assert!(page(&hazed).abs_diff(page(&clear)) < 20, "only antialiased type edges change");
}

/// Pillars of type lit by a low sun from behind, with `fog`.
fn pillars(fog: &str) -> String {
    page(
        "",
        &format!(
            "<text3d position='0 0 0' size='2' depth='0.5' color='rgb(120,120,120)'>I I I I</text3d>\
             <light type='sun' position='0 3 -8' target='0 0 4' intensity='20000'></light>{fog}"
        ),
    )
}

/// `source` over a page of `background`.
fn over(source: &str, background: &str) -> Vec<u8> {
    shot(&source.replace("rgb(0,0,255)", background))
}

/// How much of the page a frame of `source` covers at its top left corner,
/// from the frame over a black page and over a grey one.
fn coverage(source: &str) -> f32 {
    1.0 - (over(source, "rgb(60,60,60)")[0] as f32 - over(source, "rgb(0,0,0)")[0] as f32) / 60.0
}

#[test]
fn a_fog_volume_scatters_light_and_hides_the_page_with_its_density() {
    if !gpu() {
        return;
    }
    let fog = |density: &str| format!("<fog color='white' density='{density}'></fog>");
    let covered = [coverage(&pillars("")), coverage(&pillars(&fog("0.05"))), coverage(&pillars(&fog("0.3")))];
    eprintln!("page covered at the corner: none {:.3}, density 0.05 {:.3}, 0.3 {:.3}", covered[0], covered[1], covered[2]);
    assert!(covered[0].abs() < 0.02, "a model without fog leaves the sky clear");
    assert!(covered[1] > 0.04 && covered[2] > covered[1] + 0.2, "a denser volume hides more of the page");
    // The volume scatters the probe's light, so it shows without a sun.
    let unlit = over(&pillars(&fog("0.05")).replace("intensity='20000'", "intensity='0'"), "rgb(0,0,0)")[0];
    // The sun lights it more, and the pillars' shadows cut shafts into it.
    let lit = over(&pillars(&fog("0.05")), "rgb(0,0,0)");
    let even = over(&pillars(&fog("0.05")).replace("intensity='20000'", "intensity='20000' shadows='false'"), "rgb(0,0,0)");
    let (shafts, smooth) = (deepest_dip(&lit, 140), deepest_dip(&even, 140));
    eprintln!("corner {unlit} without the sun, {} with it; deepest dip below the pillars {shafts:.1} with shadows, {smooth:.1} without", lit[0]);
    assert!(unlit > 25, "the probe lights the fog");
    assert!(lit[0] > unlit + 30, "the sun brightens the fog");
    assert!(shafts > 8.0 && smooth < 3.0, "shadows cut shafts into the sunlit fog");
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
fn the_scene_document_keeps_the_fog() {
    if !gpu() {
        return;
    }
    let source = pillars("<fog color='#336699' visibility='25' density='0.1'></fog>");
    let document = export_scene(&build(&source).director).unwrap();
    let mut parsed = SceneDocument::from_json(&document.to_json().unwrap()).unwrap();
    assert_eq!(parsed, document);
    assert_eq!(model_of(&mut parsed).fog, Some(ModelFog { color: [0.2, 0.4, 0.6], visibility: 25.0, density: 0.1 }));
    let imported = import(parsed.clone()).render_frame(0.0).unwrap();
    let live = shot(&source);
    let changed = imported.rgba.chunks(4).zip(live.chunks(4)).filter(|(p, q)| (0..3).any(|k| p[k].abs_diff(q[k]) > 2)).count();
    assert_eq!(changed, 0, "the imported fog renders as compiled");
    let mut broken = parsed;
    model_of(&mut broken).fog.as_mut().unwrap().visibility = -1.0;
    let Err(e) = import(broken).render_frame(0.0) else { panic!("an import error") };
    let e = e.to_string();
    assert!(e.contains("model fog needs colour channels from 0 to 1"), "{e}");
}

#[test]
fn malformed_fog_fails_to_compile() {
    let error = |source: &str| compile(source, options()).err().expect("a compile error").to_string();
    let e = error(&lines("<fog visibility='10'></fog><fog density='1'></fog>"));
    assert!(e.contains("a model takes one <fog>"), "{e}");
    let e = error(&lines("<fog color='red'></fog>"));
    assert!(e.contains("fog needs a visibility or a density above 0"), "{e}");
    let e = error(&lines("<fog visibility='-1'></fog>"));
    assert!(e.contains("fog visibility '-1' must be a number from 0"), "{e}");
    let e = error(&lines("<fog density='thick'></fog>"));
    assert!(e.contains("fog density 'thick' must be a number from 0"), "{e}");
    let e = error(&lines("<fog color='nope' visibility='10'></fog>"));
    assert!(e.contains("fog "), "{e}");
    let e = error(&page("paths='4'", "<text3d>A</text3d><fog visibility='10'></fog>"));
    assert!(e.contains("fog renders at the rasterized levels, so it cannot take paths"), "{e}");
}
