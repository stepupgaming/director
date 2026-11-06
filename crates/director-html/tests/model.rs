//! `<model>`: a glTF scene rendered by `director-3d`, its clips on the element
//! clock, its camera driven by CSS and GSAP, and the scene document round
//! trip. Each test skips on a machine without a Vulkan adapter.

use director_core::scene_export::export_scene;
use director_html::{compile, CompileOptions, CompiledHtml};
use director_render::render_frame;
use director_scene::{NodeBody, SceneDocument};
use std::path::PathBuf;
use std::sync::Arc;

const W: i32 = 160;
const H: i32 = 90;

fn fixture() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../director-3d/tests/fixtures/model.glb")
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
        gsap_dir: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools/node_modules/gsap/dist")),
        ..Default::default()
    }
}

/// A full-frame model over a red composition, with `style` on the model and `extra` after it.
fn page(style: &str, extra: &str) -> String {
    format!(
        "<style>body{{margin:0}}#main{{background:rgb(255,0,0)}}model{{display:block;width:{W}px;height:{H}px;{style}}}</style>\
         <div id='main' data-composition-id='main' data-width='{W}' data-height='{H}' data-duration='2'>\
         <model id='m' src='{}'></model></div>{extra}",
        fixture()
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

fn pixel(p: &[u8], x: i32, y: i32) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [p[i], p[i + 1], p[i + 2]]
}

/// Pixels whose channels differ by more than 2.
fn changed(a: &[u8], b: &[u8]) -> usize {
    a.chunks(4).zip(b.chunks(4)).filter(|(p, q)| (0..3).any(|k| p[k].abs_diff(q[k]) > 2)).count()
}

#[test]
fn a_model_paints_its_scene_over_a_transparent_sky() {
    if !gpu() {
        return;
    }
    let mut c = build(&page("", ""));
    let pixels = frame(&mut c, 0.0);
    // The sky above the ground shows the red page through the model, plus a
    // faint additive bloom glow.
    let page = |p: &[u8]| p[0] >= 250 && p[1] < 16 && p[2] < 16;
    assert!(page(&pixel(&pixels, 2, 2)), "{:?}", pixel(&pixels, 2, 2));
    let red = pixels.chunks(4).filter(|p| page(p)).count();
    let total = (W * H) as usize;
    assert!(red > total / 20 && red < total * 9 / 10, "{red} of {total} pixels show the page");
}

#[test]
fn clips_follow_the_element_clock_in_any_render_order() {
    if !gpu() {
        return;
    }
    let mut c = build(&page("", ""));
    let late = frame(&mut c, 1.5);
    let early = frame(&mut c, 0.0);
    let again = frame(&mut c, 1.5);
    assert!(changed(&early, &late) > 200, "the cube turns and the sphere rises");
    assert_eq!(changed(&late, &again), 0, "a frame depends only on its time");
}

#[test]
fn css_and_gsap_drive_the_camera() {
    if !gpu() {
        return;
    }
    let front = frame(&mut build(&page("", "")), 0.0);
    let turned = frame(&mut build(&page("--camera-yaw:30deg;--camera-pitch:10deg", "")), 0.0);
    assert!(changed(&front, &turned) > 500, "yaw and pitch move the camera");
    let closer = frame(&mut build(&page("--camera-distance:0.7", "")), 0.0);
    assert!(changed(&front, &closer) > 500, "distance moves the camera");

    let gsap = page(
        "",
        "<script src='https://cdn.jsdelivr.net/npm/gsap@3.14.2/dist/gsap.min.js'></script>\
         <script>gsap.fromTo('#m', {'--camera-yaw': 0, '--camera-pitch': 0}, {'--camera-yaw': 30, '--camera-pitch': 10, duration: 1, ease: 'none'})</script>",
    );
    let mut c = build(&gsap);
    let timeline = c.timeline.as_ref().expect("GSAP timeline");
    assert!(timeline.tracks.iter().any(|t| t.property == "--camera-yaw"), "{:?}", timeline.tracks);
    assert!(changed(&frame(&mut c, 0.0), &front) < 20, "the tween starts at the front view");
    let mut still = build(&page("--camera-yaw:30deg;--camera-pitch:10deg", ""));
    assert!(changed(&frame(&mut c, 1.0), &frame(&mut still, 1.0)) < 20, "the tween ends at the CSS view");

    let keyframes = page(
        "animation:orbit 1s linear forwards",
        "<style>@keyframes orbit{from{--camera-yaw:0deg}to{--camera-yaw:30deg;--camera-pitch:10deg}}</style>",
    );
    let mut c = build(&keyframes);
    assert!(changed(&frame(&mut c, 1.5), &frame(&mut still, 1.5)) < 20, "CSS keyframes reach the same view");
}

#[test]
fn the_scene_document_renders_what_the_preview_shows() {
    if !gpu() {
        return;
    }
    let mut compiled = build(&page("--camera-yaw:-20deg", ""));
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
    assert_eq!(model.view.yaw, -20.0);
    let asset = parsed.asset(&model.asset).expect("model asset");
    assert_eq!(asset.media_type, "model/gltf-binary");

    let mut registry = director_core::scene_import::ImportRegistry::core();
    director_html::register_import(&mut registry);
    let renderer = director_render::Renderer::new(Arc::new(parsed), Arc::new(director_core::DefaultAssetLoader))
        .with_registry(registry);
    for time in [0.0, 1.25] {
        let imported = renderer.render_frame(time).unwrap();
        let preview = frame(&mut compiled, time);
        assert_eq!(changed(&imported.rgba, &preview), 0, "t={time}");
    }
}

#[test]
fn a_gpu_canvas_samples_the_frame_where_bevy_left_it() {
    if !gpu() {
        return;
    }
    let mut context = director_3d::gpu_context().expect("a Skia context on the 3D device");
    let info = skia_safe::ImageInfo::new(
        (W, H),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        Some(skia_safe::ColorSpace::new_srgb()),
    );
    let mut surface = skia_safe::gpu::surfaces::render_target(
        &mut context,
        skia_safe::gpu::Budgeted::No,
        &info,
        0,
        None,
        None,
        false,
        false,
    )
    .expect("a Vulkan surface");
    let mut on_gpu = build(&page("--camera-yaw:-20deg", ""));
    let mut on_cpu = build(&page("--camera-yaw:-20deg", ""));
    let before = director_3d::texture_draws();
    for time in [1.25, 0.0, 1.25] {
        render_frame(&mut on_gpu.director, time, &mut surface).unwrap();
        context.flush_and_submit();
        let read = skia_safe::ImageInfo::new((W, H), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Unpremul, None);
        let mut pixels = vec![0; (W * H * 4) as usize];
        assert!(surface.read_pixels(&read, &mut pixels, W as usize * 4, (0, 0)));
        assert_eq!(changed(&pixels, &frame(&mut on_cpu, time)), 0, "t={time}");
    }
    assert_eq!(director_3d::texture_draws() - before, 3, "every frame sampled the studio texture");
}

fn model_body(c: &CompiledHtml) -> director_scene::ModelBody {
    let document = export_scene(&c.director).unwrap();
    document
        .nodes
        .iter()
        .find_map(|node| match &node.body {
            NodeBody::Html(html) => html.content.as_ref().and_then(|c| match &c.body {
                NodeBody::Model(model) => Some(model.clone()),
                _ => None,
            }),
            _ => None,
        })
        .expect("a model body")
}

#[test]
fn export_samples_smooth_edges_and_keep_the_frame() {
    if !gpu() {
        return;
    }
    let sampled = page("", "").replace("<model id='m'", "<model id='m' samples='16'");
    let live = frame(&mut build(&page("", "")), 0.0);
    let mut c = build(&sampled);
    let export = frame(&mut c, 0.0);
    let differ = changed(&live, &export);
    let total = (W * H) as usize;
    eprintln!("16 samples change {differ} of {total} pixels");
    assert!(differ > 20 && differ < total / 4, "{differ} of {total} pixels change");
    frame(&mut c, 1.0);
    assert_eq!(changed(&export, &frame(&mut c, 0.0)), 0, "a sampled frame depends only on its time");
    assert_eq!(model_body(&c).samples, 16);
    assert_eq!(model_body(&build(&page("", ""))).samples, 1);
    let error = compile(&page("", "").replace("<model id='m'", "<model id='m' samples='0'"), options())
        .err()
        .expect("samples below 1")
        .to_string();
    assert!(error.contains("samples must be a whole number from 1 to 256"), "{error}");
}

/// Pixels that differ by more than 2 in columns `x0..x1`.
fn changed_in(a: &[u8], b: &[u8], columns: std::ops::Range<i32>) -> usize {
    (0..H)
        .flat_map(|y| columns.clone().map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let (p, q) = (pixel(a, x, y), pixel(b, x, y));
            (0..3).any(|k| p[k].abs_diff(q[k]) > 2)
        })
        .count()
}

/// The skinned and morphing test rig.
fn rig() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../director-3d/tests/fixtures/rig.glb")
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn skins_and_morph_targets_follow_the_element_clock() {
    if !gpu() {
        return;
    }
    let mut c = build(&page("", "").replace(&fixture(), &rig()));
    let start = frame(&mut c, 0.0);
    let end = frame(&mut c, 1.95);
    let bend = changed_in(&start, &end, 0..W / 2);
    let squash = changed_in(&start, &end, W * 11 / 20..W);
    eprintln!("the column changes {bend} pixels and the ball {squash}");
    assert!(bend > 150, "the skinned column bends: {bend}");
    assert!(squash > 150, "the morph target squashes the ball: {squash}");
    assert_eq!(changed(&start, &frame(&mut c, 0.0)), 0, "skins and morphs reset with the clock");
    // Another model's frame hides this stage; the morph comes back with it.
    frame(&mut build(&page("", "")), 1.0);
    assert_eq!(changed(&end, &frame(&mut c, 1.95)), 0, "the squash survives a stage switch");
}

#[test]
fn a_model_without_a_glb_is_an_error() {
    let source = "<model src='data:application/octet-stream;base64,AAAA'></model>";
    let error = compile(source, options()).err().expect("an invalid model").to_string();
    assert!(error.contains("glTF binary"), "{error}");
}

#[test]
fn a_lens_blurs_off_the_focus_plane_alike_in_preview_and_export() {
    if !gpu() {
        return;
    }
    let sad = |a: &[u8], b: &[u8]| a.iter().zip(b).map(|(x, y)| x.abs_diff(*y) as u64).sum::<u64>();
    let pinhole = frame(&mut build(&page("", "")), 1.0);
    let none = frame(&mut build(&page("--camera-aperture:none;--camera-focus:2m", "")), 1.0);
    assert_eq!(changed(&pinhole, &none), 0, "no aperture is a pinhole");

    let lens = |style: &str| frame(&mut build(&page(&format!("--camera-aperture:0.1;{style}"), "")), 1.0);
    let (auto, far, near) = (lens(""), lens("--camera-focus:20m"), lens("--camera-focus:2"));
    let (auto, far, near) = (sad(&pinhole, &auto), sad(&pinhole, &far), sad(&pinhole, &near));
    eprintln!("blur against the pinhole: auto focus {auto}, 20 m {far}, 2 m {near}");
    assert!(auto > 0, "the ground before and behind the focus plane blurs");
    assert!(auto * 3 < far && auto * 3 < near, "the scene centre stays sharp at auto focus");
    let blurred = lens("--camera-focus:2");
    let sky = pixel(&blurred, W / 2, 1);
    assert!(sky[0] == 255 && sky[1] < 24 && sky[2] < 24, "the sky stays transparent: {sky:?}");

    let style = "--camera-aperture:0.1;--camera-focus:20m";
    let export = page(style, "").replace("<model id='m'", "<model id='m' samples='16'");
    let mut c = build(&export);
    assert_eq!(changed(&lens("--camera-focus:20m"), &frame(&mut c, 1.0)), 0, "a lens renders 16 samples at either level");
    let body = model_body(&c);
    assert_eq!((body.view.focus, body.view.aperture), (20.0, 0.1));
    assert_eq!((model_body(&build(&page("", ""))).view.focus, model_body(&build(&page("", ""))).view.aperture), (0.0, 0.0));

    for (style, message) in [
        ("--camera-aperture:-1", "must be none or a number from 0"),
        ("--camera-focus:near", "must be auto or a number from 0"),
    ] {
        let error = compile(&page(style, ""), options()).err().expect(style).to_string();
        assert!(error.contains(message), "{style}: {error}");
    }
}

#[test]
fn each_shot_sets_the_probe_intensity_of_a_shared_stage() {
    if !gpu() {
        return;
    }
    let shot = |intensity: &str| {
        let source = page("", "").replace("></model>", &format!(" environment-intensity='{intensity}'></model>"));
        frame(&mut build(&source), 1.0)
    };
    let (bright, dim) = (shot("2000"), shot("50"));
    assert!(changed(&bright, &dim) > (W * H / 3) as usize, "a dimmer probe darkens the scene");
    assert_eq!(changed(&bright, &shot("2000")), 0, "the stage takes each shot's intensity");
}

/// A model under a dim probe with `lights` inside it.
fn lit(lights: &str, attrs: &str) -> String {
    page("", "").replace("></model>", &format!(" environment-intensity='50' {attrs}>{lights}</model>"))
}

/// Pixels in the shadow rows whose luminance sits between the darkest and
/// brightest fifth of `reference` there: shadow edges and the ground's shading.
fn penumbra(p: &[u8], reference: &[u8]) -> usize {
    let rows = || (H / 2..H * 3 / 4).flat_map(|y| (0..W).map(move |x| (x, y)));
    let luma = |p: &[u8], (x, y): (i32, i32)| pixel(p, x, y).iter().map(|&c| c as i32).sum::<i32>();
    let (lo, hi) = rows().fold((i32::MAX, 0), |(lo, hi), at| (lo.min(luma(reference, at)), hi.max(luma(reference, at))));
    let (lo, hi) = (lo + (hi - lo) / 5, hi - (hi - lo) / 5);
    rows().filter(|&at| (lo..hi).contains(&luma(p, at))).count()
}

#[test]
fn added_lights_cast_shadows_softened_by_their_emitter_size() {
    if !gpu() {
        return;
    }
    let sun = "<light type='sun' position='3 4 2' intensity='20000'></light>";
    let dark = frame(&mut build(&lit("", "")), 1.0);
    let mut hard = build(&lit(sun, "samples='16'"));
    let hard_frame = frame(&mut hard, 1.0);
    assert!(changed(&dark, &hard_frame) > (W * H / 3) as usize, "a sun lights the scene");
    let soft = frame(&mut build(&lit(&sun.replace("'20000'", "'20000' angle='30'"), "")), 1.0);
    let (hard_mid, soft_mid) = (penumbra(&hard_frame, &hard_frame), penumbra(&soft, &hard_frame));
    eprintln!("mid-tone ground pixels: hard sun {hard_mid}, 30 degree sun {soft_mid}");
    assert!(soft_mid > hard_mid + 60, "a wide sun spreads its shadow edges");

    let rect = "<light type='rect' position='2 3 2' width='2' height='2' intensity='100000'></light>";
    let mut c = build(&lit(rect, ""));
    let live = frame(&mut c, 1.0);
    assert!(changed(&dark, &live) > (W * H / 3) as usize, "a rect light lights the scene");
    assert_eq!(changed(&live, &frame(&mut build(&lit(rect, "samples='16'")), 1.0)), 0, "a sized light renders 16 samples at either level");
    assert_eq!(changed(&hard_frame, &frame(&mut hard, 1.0)), 0, "lights respawn the same after another light set");
    frame(&mut build(&page("", "").replace(&fixture(), &rig())), 1.0);
    assert_eq!(changed(&live, &frame(&mut c, 1.0)), 0, "lit shadows come back with their stage");
    let document = SceneDocument::from_json(&export_scene(&c.director).unwrap().to_json().unwrap()).unwrap();
    let mut registry = director_core::scene_import::ImportRegistry::core();
    director_html::register_import(&mut registry);
    let renderer = director_render::Renderer::new(Arc::new(document), Arc::new(director_core::DefaultAssetLoader))
        .with_registry(registry);
    assert_eq!(changed(&renderer.render_frame(1.0).unwrap().rgba, &live), 0, "the document keeps the lights");

    let point = "<light type='point' position='2 3 2' radius='0.5' color='#ffcc88' shadows='false'></light>";
    let body = model_body(&build(&lit(point, "")));
    let light = &body.lights[0];
    assert_eq!(light.kind, director_scene::ModelLightKind::Point);
    assert_eq!((light.position, light.target, light.radius, light.shadows), (Some([2.0, 3.0, 2.0]), None, 0.5, false));
    assert_eq!(light.intensity, 1_000_000.0);
    assert!((light.color[1] - 0.8).abs() < 1e-3, "{:?}", light.color);
    assert!(model_body(&build(&page("", ""))).lights.is_empty());

    for (light, message) in [
        ("<light type='area'></light>", "must be sun, point, spot or rect"),
        ("<light position='1 2'></light>", "must be three numbers in metres"),
        ("<light radius='-1'></light>", "must be a number from 0"),
    ] {
        let error = compile(&lit(light, ""), options()).err().expect(light).to_string();
        assert!(error.contains(message), "{light}: {error}");
    }
}
