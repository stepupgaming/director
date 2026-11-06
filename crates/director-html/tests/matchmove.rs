//! `<model track>`: the compiler solves the camera of the footage under a
//! model and films the model through it frame by frame. The tests composite
//! `solve.glb`, which holds no camera, over the `solve.mp4` plate it was
//! solved from and compare the result with Eevee's renders of the same shot
//! with the box; `make_solve.py` writes all four, with `--lens` the same
//! plate and renders through a barrel-distorted lens, with `--moving`
//! the shot with movers in it and their matte, with `--pan` the yard
//! from a camera that only turns, with `--zoom` that camera zooming in, and
//! with `--dolly` the walk zooming in and with `--shake` the walk held in a
//! shaking hand with motion blur. The masks take the box's
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

/// The `plate` (`solve`, `lens`, `moving`, `pan`, `zoom` or `dolly`) under `solve.glb` with the sky and units
/// of `tests/eevee.rs`, its ground a catcher and its camera solved from the
/// plate with `track` attributes. An empty `track` leaves the plate alone,
/// and a `track` with `track-light` lights the model from the plate in
/// place of the sky. A `track` with `track-sun` takes `solve.glb` without
/// its sun, so the sun read from the plate lights the box alone.
fn page(plate: &str, track: &str) -> String {
    page_at(&fixture(&format!("{plate}.mp4")), track)
}

/// `page` over the footage at the path `plate`.
fn page_at(plate: &str, track: &str) -> String {
    let sky = match track.contains("track-light") {
        true => String::new(),
        false => format!("environment='{}' environment-intensity='683' ", fixture("sky.hdr")),
    };
    let layer = if track.is_empty() {
        String::new()
    } else {
        format!(
            "<model src='{}' {sky}exposure='{}' shadow-catcher='Ground' track='{plate}' {track}></model>",
            if track.contains("track-sun") { sunless() } else { fixture("solve.glb") },
            (683.0f64 / 1.2).log2(),
        )
    };
    format!(
        "<style>body{{margin:0}}video,model{{position:absolute;left:0;top:0;width:{W}px;height:{H}px}}</style>\
         <div id='main' data-composition-id='main' data-width='{W}' data-height='{H}' data-duration='2'>\
         <video muted src='{plate}'></video>{layer}</div>"
    )
}

/// `solve.glb` with its glTF lights stripped, written once to the temp
/// folder.
fn sunless() -> String {
    static PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PATH.get_or_init(|| {
        let glb = std::fs::read(fixture("solve.glb")).unwrap();
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let mut json: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        let strip = |v: &mut serde_json::Value| {
            if let Some(extensions) = v.get_mut("extensions").and_then(|e| e.as_object_mut()) {
                extensions.remove("KHR_lights_punctual");
            }
        };
        strip(&mut json);
        json["nodes"].as_array_mut().unwrap().iter_mut().for_each(strip);
        if let Some(used) = json.get_mut("extensionsUsed").and_then(|u| u.as_array_mut()) {
            used.retain(|e| e != "KHR_lights_punctual");
        }
        let mut text = serde_json::to_vec(&json).unwrap();
        text.resize(text.len().div_ceil(4) * 4, b' ');
        let bin = &glb[20 + json_len..];
        let mut out = Vec::with_capacity(20 + text.len() + bin.len());
        out.extend_from_slice(&glb[..8]);
        out.extend_from_slice(&((20 + text.len() + bin.len()) as u32).to_le_bytes());
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&text);
        out.extend_from_slice(bin);
        let path = std::env::temp_dir().join(format!("director-sunless-{}.glb", std::process::id()));
        std::fs::write(&path, out).unwrap();
        path.to_string_lossy().replace('\\', "/")
    })
    .clone()
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
/// at an IoU above `fit` and its shadow above 0.8.
fn matches_eevee(name: &str, track: &str, fit: f64) {
    matches_eevee_shadow(name, track, fit, 0.8);
}

/// `matches_eevee` with the shadow overlapping Eevee's at an IoU above
/// `shadow_fit`.
fn matches_eevee_shadow(name: &str, track: &str, fit: f64, shadow_fit: f64) {
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
        assert!(iou(&shadow_ref, &shadow_dir) > shadow_fit, "the shadow falls where Eevee's does");
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
fn a_model_holds_still_through_a_zoom() {
    if gpu() {
        matches_eevee("zoom", "track-height='1.6' track-pitch='8.12' track-zoom", 0.9);
    }
}

#[test]
fn a_model_holds_still_through_a_dolly_zoom() {
    if gpu() {
        // `dolly.mp4` walks the yard while the lens zooms from 42 to 28
        // degrees. The zoom trades against travel along the view, so the
        // path holds a little looser than the fixed lens's walk.
        matches_eevee("dolly", "track-fov='42' track-height='1.6' track-distortion='0 0' track-zoom", 0.88);
    }
}

#[test]
fn a_model_rides_a_handheld_camera() {
    if gpu() {
        // The hand turns the view up to 10 px a frame and the footage
        // blurs it, and the solve holds as tight as the steady walk's.
        matches_eevee("shake", "track-height='1.6' track-shutter='180'", 0.9);
    }
}

/// The mean error against Eevee's render of `plate` at `frame` within 4 px
/// of where Eevee shows the box or its shadow, the same pixels whatever the
/// composite.
fn error_near_box(plate: &str, frame: u32, bare: &[u8], composite: &[u8]) -> f64 {
    let reference = eevee(plate, frame);
    let shows = |i: usize| {
        (boxed(rgb(&reference, i)) && diff(&reference, bare, i) > 30) || luma(rgb(&reference, i)) < luma(rgb(bare, i)) - 15.0
    };
    let near: Vec<usize> = (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            (-4..=4).any(|dy| (-4..=4).any(|dx| {
                let (u, v) = (x + dx, y + dy);
                (0..W).contains(&u) && (0..H).contains(&v) && shows((v * W + u) as usize)
            }))
        })
        .map(|(x, y)| (y * W + x) as usize)
        .collect();
    near.iter().map(|&i| diff(composite, &reference, i) as f64).sum::<f64>() / (near.len() * 3) as f64
}

#[test]
fn the_footage_shutter_blurs_the_model_as_eevee_does() {
    if !gpu() {
        return;
    }
    // Through the true cameras the composite differs from Eevee's only in
    // its blur. Eevee opens a half-frame shutter, so 180 degrees comes
    // closest and a sharp model or a full-frame smear falls further off.
    let compiled = build(&page("shake", "track-height='1.6'"));
    let document = export_scene(&compiled.director).unwrap();
    let truth: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(fixture("shake.json")).unwrap()).unwrap();
    let floats = |v: &serde_json::Value| v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect::<Vec<f32>>();
    let frames: Vec<director_scene::ModelTrackFrame> = truth["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            let (p, r) = (floats(&f["position"]), floats(&f["rotation"]));
            director_scene::ModelTrackFrame { position: [p[0], p[1], p[2]], rotation: [r[0], r[1], r[2], r[3]], fov: None }
        })
        .collect();
    let mut bare = build(&page("shake", ""));
    let bare = [10, 50].map(|f| self::frame(&mut bare, f as f64 / 30.0));
    let errors = [0.0f32, 180.0, 360.0].map(|shutter| {
        let mut document = document.clone();
        for node in &mut document.nodes {
            if let NodeBody::Html(html) = &mut node.body {
                if let Some(NodeBody::Model(model)) = html.content.as_mut().map(|c| &mut c.body) {
                    let track = model.track.as_mut().expect("a model track");
                    track.fov = (truth["yfov"].as_f64().unwrap() as f32).to_degrees();
                    track.shutter = shutter;
                    track.frames = frames.clone();
                }
            }
        }
        let mut registry = director_core::scene_import::ImportRegistry::core();
        director_html::register_import(&mut registry);
        let renderer = director_render::Renderer::new(Arc::new(document), Arc::new(director_core::DefaultAssetLoader)).with_registry(registry);
        [0, 1].map(|k| {
            let frame = [10, 50][k];
            let composite = renderer.render_frame(frame as f64 / 30.0).unwrap();
            error_near_box("shake", frame, &bare[k], &composite.rgba)
        })
    });
    eprintln!("true cameras, error near the box at frames 10 and 50: sharp {:.2?}, 180 degrees {:.2?}, 360 degrees {:.2?}", errors[0], errors[1], errors[2]);
    let [sharp, half, full] = errors;
    for ((sharp, half), full) in sharp.into_iter().zip(half).zip(full) {
        assert!(half < sharp && half < full, "the footage's shutter matches Eevee's blur best");
    }
}

#[test]
fn the_plate_lights_the_model() {
    if gpu() {
        // The probe stitched from the pan holds its sky and yard, so the box
        // lit by it lands as close to Eevee's sky-lit render as the sky does.
        matches_eevee("pan", "track-height='1.6' track-pitch='8.12' track-light", 0.9);
    }
}

/// The mean colour of the box's lit faces in `composite`, with the box
/// found where the `neutral` composite shows it over the bare `plate`.
fn box_colour(composite: &[u8], neutral: &[u8], plate: &[u8]) -> [f64; 3] {
    let n = (W * H) as usize;
    let mask: Vec<usize> = (0..n).filter(|&i| boxed(rgb(neutral, i)) && diff(neutral, plate, i) > 30).collect();
    assert!(mask.len() > 500, "the box shows: {} px", mask.len());
    std::array::from_fn(|k| mask.iter().map(|&i| rgb(composite, i)[k] as f64).sum::<f64>() / mask.len() as f64)
}

#[test]
fn a_warm_plate_warms_the_model() {
    let warm = std::env::temp_dir().join(format!("director-warm-pan-{}.mp4", std::process::id()));
    let status = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-i", &fixture("pan.mp4"), "-vf", "colorchannelmixer=rr=1:gg=0.8:bb=0.55"])
        .args(["-c:v", "libx264", "-crf", "12", "-pix_fmt", "yuv420p"])
        .arg(&warm)
        .status()
        .unwrap();
    assert!(status.success());
    let warm_path = warm.to_string_lossy().replace('\\', "/");
    let lit = "track-fov='42' track-height='1.6' track-pitch='8.12' track-light";
    let mut warmed = build(&page_at(&warm_path, lit));
    let document = export_scene(&warmed.director).unwrap();
    assert_eq!(SceneDocument::from_json(&document.to_json().unwrap()).unwrap(), document);
    let body = model(&warmed);
    assert!(body.environment.is_some(), "the document keeps the plate's probe");
    assert!((body.environment_intensity - 683.0).abs() < 0.5, "the probe renders the plate's light back at its value");
    if gpu() {
        let mut neutral = build(&page("pan", lit));
        let mut bare = build(&page("pan", ""));
        let time = 1.0;
        let (neutral, bare, warmed) = (frame(&mut neutral, time), frame(&mut bare, time), frame(&mut warmed, time));
        let (a, b) = (box_colour(&neutral, &neutral, &bare), box_colour(&warmed, &neutral, &bare));
        eprintln!("box under the neutral plate {a:?}, under the warm plate {b:?}");
        let warmth = |c: [f64; 3]| c[0] - c[2];
        assert!(warmth(b) > warmth(a) + 8.0, "the warm plate's light reddens the box: {a:?} to {b:?}");
    }
    std::fs::remove_file(&warm).ok();
}

#[test]
fn the_document_keeps_each_frame_of_a_zoom() {
    let compiled = build(&page("zoom", "track-fov='42' track-height='1.6' track-pitch='8.12' track-zoom"));
    let document = export_scene(&compiled.director).unwrap();
    assert_eq!(SceneDocument::from_json(&document.to_json().unwrap()).unwrap(), document);
    let lens: Vec<f32> = track(&compiled).frames.iter().map(|f| f.fov.expect("a lens per frame")).collect();
    // `make_solve.py --zoom` eases the lens from 42 to 24 degrees.
    assert_eq!(lens[0], 42.0, "track-fov holds frame 0's lens");
    assert!((lens[59] - 24.0).abs() < 0.15, "the last frame zoomed to 24 degrees: {}", lens[59]);
    let pan = build(&page("pan", "track-fov='42' track-height='1.6' track-pitch='8.12'"));
    assert!(track(&pan).frames.iter().all(|f| f.fov.is_none()), "a fixed lens writes no lens per frame");
}

#[test]
fn a_model_bends_with_the_lens_of_its_plate() {
    if gpu() {
        matches_eevee("lens", "track-fov='42' track-height='1.6'", 0.9);
    }
}

#[test]
fn the_plate_casts_the_sun_it_shows() {
    if gpu() {
        // The sun read from the plate points about 2.5 degrees off, which
        // costs the box's shadow about 0.05 of its overlap.
        matches_eevee_shadow("solve", "track-fov='42' track-height='1.6' track-light track-sun", 0.9, 0.75);
    }
}

#[test]
fn the_document_keeps_the_sun_read_from_the_plate() {
    let compiled = build(&page("solve", "track-fov='42' track-height='1.6' track-sun"));
    let lights = model(&compiled).lights;
    let suns: Vec<_> = lights.iter().filter(|l| l.kind == director_scene::ModelLightKind::Sun).collect();
    assert_eq!(suns.len(), 1, "the stripped model brings no sun of its own: {lights:?}");
    let (from, to) = (suns[0].position.unwrap(), suns[0].target.unwrap());
    let d: Vec<f64> = (0..3).map(|k| (from[k] - to[k]) as f64).collect();
    let truth = [-0.52726882, 0.64278761, -0.55570827];
    let cos = d.iter().zip(truth).map(|(a, b)| a * b).sum::<f64>() / d.iter().map(|v| v * v).sum::<f64>().sqrt();
    let error = cos.clamp(-1.0, 1.0).acos().to_degrees();
    // Blender's 3 W/m2 sun at 683 lm/W.
    let lux = suns[0].intensity;
    assert!(error < 4.0, "the sun points {error:.2} degrees off");
    assert!((lux / 2049.0 - 1.0).abs() < 0.25, "the sun reads {lux:.0} lux against 2049");
    assert!(suns[0].shadows, "the sun casts shadows");
}

/// The model body of a compiled page.
fn model(compiled: &CompiledHtml) -> director_scene::ModelBody {
    let document = export_scene(&compiled.director).unwrap();
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
        .expect("a model")
}

/// The track a compiled page's model keeps.
fn track(compiled: &CompiledHtml) -> director_scene::ModelTrack {
    model(compiled).track.expect("a model track")
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
    let error = fails("track-light environment='sky.hdr'");
    assert!(error.contains("track-light and environment both light the model; give one"), "{error}");
    let error = compile(
        &page("solve", "").replace("</div>", &format!("<model src='{}' track-light></model></div>", fixture("solve.glb"))),
        CompileOptions { width: W, height: H, duration: 2.0, ..Default::default() },
    )
    .err()
    .expect("a compile error")
    .to_string();
    assert!(error.contains("track-light needs a track to light the model from"), "{error}");
    let error = compile(
        &page("solve", "").replace("</div>", &format!("<model src='{}' track-sun></model></div>", fixture("solve.glb"))),
        CompileOptions { width: W, height: H, duration: 2.0, ..Default::default() },
    )
    .err()
    .expect("a compile error")
    .to_string();
    assert!(error.contains("track-sun needs a track to read the sun from"), "{error}");
    let error = compile(&page("pan", "track-pitch='8.12' track-sun"), CompileOptions { width: W, height: H, duration: 2.0, ..Default::default() })
        .err()
        .expect("a compile error")
        .to_string();
    assert!(error.contains("a tripod shot has none"), "{error}");
    let error = fails("track-mask='no-such-matte.mp4'");
    assert!(error.contains("no-such-matte.mp4"), "{error}");
}

