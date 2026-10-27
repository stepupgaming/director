//! Runtime harness to determine the coordinate space of Skia runtime-shader
//! `main(float2 pos)` when applied as a node effect.
//!
//! The engine feeds `u_resolution = rect.width/height` into the shader. If `pos`
//! is layer-local (correct), a node positioned at (100,50,200,100) yields a UV
//! gradient that is ~(0,0) at the node's top-left and ~(1,1) at its bottom-right.
//! If `pos` is absolute canvas coordinates (the suspected bug), the gradient is
//! shifted and the node's top-left reads ~mid-gray instead of near-black.
//!
//! Run with: `cargo test -p director-core --release --test integration effect_uv_probe:: -- --nocapture`

use director_core::scripting::{register_rhai_api, MovieHandle};
use director_core::DefaultAssetLoader;
use rhai::Engine;
use skia_safe::{AlphaType, ColorSpace, ColorType, ImageInfo};
use std::sync::Arc;

// Shader that paints uv = pos / u_resolution as RGB. Ignores the input image.
const PROBE_SKSL: &str = r#"
uniform shader image;
uniform float2 u_resolution;
uniform float u_time;
half4 main(float2 pos) {
    float2 uv = pos / u_resolution;
    return half4(uv.x, uv.y, 0.0, 1.0);
}
"#;

fn build_probe_script() -> String {
    let mut s = String::new();
    s.push_str("let movie = new_director(400, 200, 30);\n");
    s.push_str("let scene = movie.add_scene(1.0);\n");
    s.push_str(
        "let b = scene.add_box(#{ position: \"absolute\", left: 100.0, top: 50.0, \
         width: 200.0, height: 100.0, bg_color: \"#000000\" });\n",
    );
    s.push_str("b.apply_effect(\"shader\", #{ code: `");
    s.push_str(PROBE_SKSL);
    s.push_str("`, uniforms: #{ u_time: 0.0 } });\n");
    s.push_str("return movie;\n");
    s
}

fn render_and_sample() -> (Vec<u8>, usize, usize) {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = build_probe_script();
    let movie = engine
        .eval::<MovieHandle>(&script)
        .expect("probe script evaluation failed");

    let mut director = movie.director.lock().unwrap();

    let width = director.width;
    let height = director.height;
    let info = ImageInfo::new(
        (width, height),
        ColorType::RGBA8888,
        AlphaType::Premul,
        Some(ColorSpace::new_srgb()),
    );
    let mut surface = skia_safe::surfaces::raster(&info, None, None).expect("create surface");
    director_render::render_frame(&mut director, 0.0, &mut surface)
        .expect("render_frame failed");

    let row_bytes = width as usize * 4;
    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    assert!(
        surface.read_pixels(&info, &mut pixels, row_bytes, (0, 0)),
        "read_pixels failed"
    );
    (pixels, width as usize, height as usize)
}

#[inline]
fn px(pixels: &[u8], w: usize, x: usize, y: usize) -> (u8, u8, u8, u8) {
    let i = (y * w + x) * 4;
    (pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3])
}

#[test]
fn effect_uv_is_layer_local() {
    let (pixels, w, _h) = render_and_sample();

    // Sample inside the node rect (x in [100,300), y in [50,150)).
    // Top-left interior and bottom-right interior.
    let (r_tl, g_tl, _b_tl, a_tl) = px(&pixels, w, 102, 52);
    let (r_br, g_br, _b_br, _a_br) = px(&pixels, w, 298, 148);

    eprintln!(
        "UV probe: TL=({},{}), BR=({},{}) alpha_tl={}",
        r_tl, g_tl, r_br, g_br, a_tl
    );

    // The effect layer must be opaque where the node is drawn.
    assert!(
        a_tl > 200,
        "effect should be opaque (alpha~255) at node interior, got {}",
        a_tl
    );

    // Layer-local (correct): node top-left interior is near black (uv ~ 0).
    // Absolute (bug): node top-left interior is ~mid-gray (uv ~ 0.5).
    if r_tl >= 80 {
        panic!(
            "SKSL UV BUG CONFIRMED: pos is absolute canvas coordinates (r_tl={}, expected near 0). \
             Shader UV math is offset by the node origin.",
            r_tl
        );
    }

    assert!(
        r_tl < 40,
        "expected layer-local UV: red at node top-left should be near 0, got {}",
        r_tl
    );
    assert!(
        r_br > 200,
        "red at node bottom-right should approach 255, got {}",
        r_br
    );
    // Green should also increase top->bottom under correct (layer-local) mapping.
    assert!(
        g_br > g_tl,
        "green should increase top->bottom; g_tl={}, g_br={}",
        g_tl,
        g_br
    );
}
