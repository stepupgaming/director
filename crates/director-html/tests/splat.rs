//! `<splat>` children of `<model>`: Gaussian splat clouds painted by
//! `director-3d`, alone or beside a glTF scene, and their scene document
//! round trip. Each test skips on a machine without a Vulkan adapter.

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

/// A full-frame model over a blue page whose markup is `model`.
fn page(style: &str, model: &str) -> String {
    format!(
        "<style>body{{margin:0}}#main{{background:rgb(0,0,255)}}model{{display:block;width:{W}px;height:{H}px;{style}}}</style>\
         <div id='main' data-composition-id='main' data-width='{W}' data-height='{H}' data-duration='2'>{model}</div>"
    )
}

/// The two-ball fixture alone, its `<splat>` taking `attrs`.
fn balls(style: &str, attrs: &str) -> String {
    page(style, &format!("<model id='m'><splat src='{}' {attrs}></splat></model>", fixture("splats.ply")))
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

/// A fresh compile's first frame, so no frame cache answers it.
fn shot(source: &str) -> Vec<u8> {
    frame(&mut build(source), 0.0)
}

fn pixel(p: &[u8], x: i32, y: i32) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [p[i], p[i + 1], p[i + 2]]
}

/// Pixels whose channels differ by more than 2.
fn changed(a: &[u8], b: &[u8]) -> usize {
    a.chunks(4).zip(b.chunks(4)).filter(|(p, q)| (0..3).any(|k| p[k].abs_diff(q[k]) > 2)).count()
}

fn red(p: [u8; 3]) -> bool {
    p[0] > 170 && p[1] < 80 && p[2] < 80
}

fn green(p: [u8; 3]) -> bool {
    p[1] > 150 && p[0] < 80 && p[2] < 100
}

fn blue(p: [u8; 3]) -> bool {
    p[2] > 250 && p[0] < 6 && p[1] < 6
}

/// The framing camera fits both balls: the red one left of centre, the
/// green one right of it, 17 px out at this size.
const LEFT: (i32, i32) = (W / 2 - 17, H / 2);
const RIGHT: (i32, i32) = (W / 2 + 17, H / 2);

#[test]
fn splats_paint_their_captured_colours_over_a_transparent_sky() {
    if !gpu() {
        return;
    }
    let pixels = shot(&balls("", ""));
    assert!(red(pixel(&pixels, LEFT.0, LEFT.1)), "{:?}", pixel(&pixels, LEFT.0, LEFT.1));
    assert!(green(pixel(&pixels, RIGHT.0, RIGHT.1)), "{:?}", pixel(&pixels, RIGHT.0, RIGHT.1));
    // The captured colours pass through without a filmic curve.
    let [r, g, b] = pixel(&pixels, LEFT.0, LEFT.1);
    assert!(r.abs_diff(230) < 12 && g.abs_diff(26) < 12 && b.abs_diff(26) < 12, "{:?}", [r, g, b]);
    assert!(blue(pixel(&pixels, 2, 2)), "{:?}", pixel(&pixels, 2, 2));
    let page = pixels.chunks(4).filter(|p| blue([p[0], p[1], p[2]])).count();
    let total = (W * H) as usize;
    assert!(page > total / 2 && page < total * 19 / 20, "{page} of {total} pixels show the page");
}

#[test]
fn a_splat_frame_depends_only_on_its_camera() {
    if !gpu() {
        return;
    }
    let front = shot(&balls("", ""));
    let side = shot(&balls("--camera-yaw:60deg", ""));
    assert!(changed(&front, &side) > 300, "the orbit turns the cloud");
    assert_eq!(changed(&front, &shot(&balls("", ""))), 0, "the front view comes back after the side view");
    shot(&balls("--camera-yaw:-150deg", ""));
    assert_eq!(changed(&side, &shot(&balls("--camera-yaw:60deg", ""))), 0, "the sort follows each shot's camera");
}

#[test]
fn placement_and_opacity_move_and_fade_the_cloud() {
    if !gpu() {
        return;
    }
    let turned = shot(&balls("", "rotation='0 0 180'"));
    assert!(green(pixel(&turned, LEFT.0, LEFT.1)), "{:?}", pixel(&turned, LEFT.0, LEFT.1));
    assert!(red(pixel(&turned, RIGHT.0, RIGHT.1)), "{:?}", pixel(&turned, RIGHT.0, RIGHT.1));
    let faded = shot(&balls("", "opacity='0.3'"));
    let [r, _, b] = pixel(&faded, LEFT.0, LEFT.1);
    assert!(b > 60 && r > 60, "the page shows through a faded ball: {:?}", [r, b]);
    // The framing camera follows the cloud, so a moved cloud frames the same.
    let front = shot(&balls("", ""));
    let moved = changed(&front, &shot(&balls("", "position='3 1 -2'")));
    assert!(moved < (W * H) as usize / 100, "the camera frames the moved cloud: {moved} pixels change");
}

#[test]
fn splats_share_a_stage_with_the_glb_meshes() {
    if !gpu() {
        return;
    }
    let mesh = format!("<model id='m' src='{}'></model>", fixture("model.glb"));
    let both = format!(
        "<model id='m' src='{}'><splat src='{}' position='0 1.5 0' scale='0.5'></splat></model>",
        fixture("model.glb"),
        fixture("splats.ply")
    );
    let alone = shot(&page("", &mesh));
    let mixed = shot(&page("", &both));
    assert!(changed(&alone, &mixed) > 100, "the cloud adds to the scene");
    assert!(mixed.chunks(4).any(|p| red([p[0], p[1], p[2]])), "the red ball shows beside the meshes");
    assert_eq!(changed(&alone, &shot(&page("", &mesh))), 0, "the mesh stage shows no cloud after the splat stage");
}

/// The meshes keep the filmic curve and bloom beside a cloud: under the
/// file's camera a small cloud in front of them changes only the pixels it
/// covers, and its balls keep their captured colours.
#[test]
fn meshes_beside_splats_keep_the_filmic_curve() {
    if !gpu() {
        return;
    }
    let mesh = format!("<model id='m' src='{}'></model>", fixture("model.glb"));
    let both = format!(
        "<model id='m' src='{}'><splat src='{}' position='0 0.6 1.5' scale='0.3'></splat></model>",
        fixture("model.glb"),
        fixture("splats.ply")
    );
    let alone = shot(&page("", &mesh));
    let mixed = shot(&page("", &both));
    let balls = mixed.chunks(4).filter(|p| red([p[0], p[1], p[2]]) || green([p[0], p[1], p[2]])).count();
    let moved = changed(&alone, &mixed);
    assert!(balls > 20, "the cloud shows in front of the meshes: {balls} ball pixels");
    assert!(moved < balls * 2, "only the cloud's pixels change: {moved} changed for {balls} ball pixels");
}

#[test]
fn export_samples_keep_a_splat_frame_stable() {
    if !gpu() {
        return;
    }
    let sampled = balls("", "").replace("<model id='m'", "<model id='m' samples='8'");
    let live = shot(&balls("", ""));
    let export = shot(&sampled);
    let differ = changed(&live, &export);
    assert!(differ > 0 && differ < (W * H) as usize / 4, "{differ} pixels change under 8 samples");
    shot(&balls("--camera-yaw:60deg", ""));
    assert_eq!(changed(&export, &shot(&sampled)), 0, "a sampled splat frame depends only on its inputs");
}

#[test]
fn the_scene_document_keeps_the_splats() {
    if !gpu() {
        return;
    }
    let mut compiled = build(&balls("--camera-yaw:-20deg", "position='0 0.2 0' rotation='0 30 0' opacity='0.8'"));
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
    assert_eq!(model.splats.len(), 1);
    let splat = &model.splats[0];
    assert_eq!((splat.position, splat.rotation, splat.scale, splat.opacity), ([0.0, 0.2, 0.0], [0.0, 30.0, 0.0], 1.0, 0.8));
    assert_eq!(parsed.asset(&splat.asset).expect("splat asset").media_type, "application/x-ply");
    assert_eq!(parsed.asset(&model.asset).expect("scene asset").media_type, "model/gltf-binary");

    let mut registry = director_core::scene_import::ImportRegistry::core();
    director_html::register_import(&mut registry);
    let renderer = director_render::Renderer::new(Arc::new(parsed), Arc::new(director_core::DefaultAssetLoader))
        .with_registry(registry);
    let imported = renderer.render_frame(0.0).unwrap();
    assert_eq!(changed(&imported.rgba, &frame(&mut compiled, 0.0)), 0);
}

#[test]
fn malformed_splats_fail_to_compile() {
    let error = |source: &str| compile(source, options()).err().expect("a compile error").to_string();
    let e = error(&page("", "<model id='m'></model>"));
    assert!(e.contains("model without src or <splat> children"), "{e}");
    let e = error(&page("", &format!("<model id='m'><splat src='{}'></splat></model>", fixture("model.glb"))));
    assert!(e.contains("splat src must be a Gaussian splat .ply file"), "{e}");
    let e = error(&balls("", "opacity='2'"));
    assert!(e.contains("splat opacity '2' must be a number from 0 to 1"), "{e}");
    let e = error(&balls("", "position='1 2'"));
    assert!(e.contains("splat position '1 2' must be three numbers in metres"), "{e}");
}
