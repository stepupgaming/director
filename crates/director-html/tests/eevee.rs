//! The Blender round trip: `model.glb` painted by `director-3d` against
//! Eevee's render of the scene it came from. `make_model.py --eevee` lights
//! both with `sky.hdr` and renders frames 1 and 46 with the Standard view
//! transform; the GLB places frame f at f / 30 seconds. An exposure of
//! log2(683 / 1.2) EV100 and a probe of 683 cd/m² match Blender's units
//! through the exporter's 683 lm/W. Each test skips without a Vulkan adapter.

use director_html::{compile, CompileOptions};
use director_render::render_frame;
use std::path::PathBuf;

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

/// RGBA8 pixels, unpremultiplied, of an image of the test size.
fn read(image: &skia_safe::Image) -> Vec<u8> {
    let info = skia_safe::ImageInfo::new((W, H), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Unpremul, None);
    let mut pixels = vec![0; (W * H * 4) as usize];
    assert!(image.read_pixels(&info, &mut pixels, W as usize * 4, (0, 0), skia_safe::image::CachingHint::Allow));
    pixels
}

fn eevee(frame: u32) -> Vec<u8> {
    let bytes = std::fs::read(fixture(&format!("eevee-{frame}.png"))).unwrap();
    let image = skia_safe::Image::from_encoded(skia_safe::Data::new_copy(&bytes)).expect("a PNG");
    assert_eq!((image.width(), image.height()), (W, H));
    read(&image)
}

/// Director's frame of `model.glb` at Blender frame `frame` over a
/// transparent page, the model taking `attrs`.
fn director(attrs: &str, frame: u32) -> Vec<u8> {
    let source = format!(
        "<style>body{{margin:0}}model{{display:block;width:{W}px;height:{H}px}}</style>\
         <div id='main' data-composition-id='main' data-width='{W}' data-height='{H}' data-duration='2'>\
         <model id='m' src='{}' environment='{}' environment-intensity='683' exposure='{}' {attrs}></model></div>",
        fixture("model.glb"),
        fixture("sky.hdr"),
        (683.0f64 / 1.2).log2(),
    );
    let options = CompileOptions {
        width: W,
        height: H,
        duration: 2.0,
        ..Default::default()
    };
    let mut compiled = compile(&source, options).unwrap();
    compiled.director.transparent_background = true;
    let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).unwrap();
    render_frame(&mut compiled.director, frame as f64 / 30.0, &mut surface).unwrap();
    read(&surface.image_snapshot())
}

/// Whether the pixel at `i` and its four neighbours are fully covered.
fn solid(p: &[u8], i: usize) -> bool {
    let (x, y) = ((i as i32) % W, (i as i32) / W);
    [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)].iter().all(|(dx, dy)| {
        let (x, y) = ((x + dx).clamp(0, W - 1), (y + dy).clamp(0, H - 1));
        p[((y * W + x) * 4 + 3) as usize] == 255
    })
}

struct Match {
    /// Intersection over union of the covered pixels.
    coverage: f64,
    /// Pearson correlation of luma over the pixels both cover solidly.
    shading: f64,
    /// Director's mean luma over Eevee's on those pixels.
    brightness: f64,
    /// Mean absolute channel difference on those pixels.
    error: f64,
}

fn compare(director: &[u8], eevee: &[u8]) -> Match {
    let covered = |p: &[u8], i: usize| p[i * 4 + 3] >= 128;
    let n = (W * H) as usize;
    let both = (0..n).filter(|&i| covered(director, i) && covered(eevee, i)).count();
    let either = (0..n).filter(|&i| covered(director, i) || covered(eevee, i)).count();
    let luma = |p: &[u8], i: usize| 0.2126 * p[i * 4] as f64 + 0.7152 * p[i * 4 + 1] as f64 + 0.0722 * p[i * 4 + 2] as f64;
    let inner: Vec<usize> = (0..n).filter(|&i| solid(director, i) && solid(eevee, i)).collect();
    let a: Vec<f64> = inner.iter().map(|&i| luma(director, i)).collect();
    let b: Vec<f64> = inner.iter().map(|&i| luma(eevee, i)).collect();
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let (ma, mb) = (mean(&a), mean(&b));
    let cov: f64 = a.iter().zip(&b).map(|(x, y)| (x - ma) * (y - mb)).sum();
    let va: f64 = a.iter().map(|x| (x - ma).powi(2)).sum();
    let vb: f64 = b.iter().map(|y| (y - mb).powi(2)).sum();
    let error = inner
        .iter()
        .map(|&i| (0..3).map(|k| director[i * 4 + k].abs_diff(eevee[i * 4 + k]) as f64).sum::<f64>())
        .sum::<f64>()
        / (inner.len() * 3) as f64;
    Match {
        coverage: both as f64 / either as f64,
        shading: cov / (va * vb).sqrt(),
        brightness: ma / mb,
        error,
    }
}

/// Director at `attrs` against Eevee at both frames: the silhouettes agree,
/// shading correlates, brightness lands within 20 % through the different
/// tone curves (TonyMcMapface against Standard), and each frame matches its
/// own Eevee frame better than the other one.
fn check(attrs: &str) {
    let frames = [1, 46];
    let renders = frames.map(|f| director(attrs, f));
    let references = frames.map(eevee);
    for (k, frame) in frames.iter().enumerate() {
        let m = compare(&renders[k], &references[k]);
        let label = format!("[{attrs}] frame {frame}: coverage {:.3}, shading {:.3}, brightness {:.3}, error {:.1}", m.coverage, m.shading, m.brightness, m.error);
        assert!(m.coverage > 0.99, "{label}");
        assert!(m.shading > 0.9, "{label}");
        assert!(m.brightness > 0.8 && m.brightness < 1.2, "{label}");
        assert!(m.error < 28.0, "{label}");
        let other = compare(&renders[k], &references[1 - k]);
        assert!(other.coverage < m.coverage - 0.02, "[{attrs}] frame {frame} against the other frame: coverage {:.3}", other.coverage);
    }
}

#[test]
fn the_raster_levels_match_eevee() {
    if !director_3d::available() {
        eprintln!("skipped: no Vulkan adapter for director-3d");
        return;
    }
    check("");
    check("samples='16'");
}

#[test]
fn the_hero_level_matches_eevee() {
    if !(director_3d::available() && director_3d::traceable()) {
        eprintln!("skipped: no ray tracing Vulkan adapter for director-3d");
        return;
    }
    check("paths='256'");
}
