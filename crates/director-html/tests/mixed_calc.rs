use director_core::systems::renderer::render_frame;
use director_html::{compile, CompileOptions};

fn frame(source: &str) -> Vec<u8> {
    let mut c = compile(source, canvas()).unwrap();
    let mut surface = skia_safe::surfaces::raster_n32_premul((64, 32)).unwrap();
    render_frame(&mut c.director, 0.0, surface.canvas()).unwrap();
    let info = skia_safe::ImageInfo::new(
        (64, 32),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let mut pixels = vec![0; 64 * 32 * 4];
    assert!(surface.read_pixels(&info, &mut pixels, 64 * 4, (0, 0)));
    pixels
}

fn canvas() -> CompileOptions {
    CompileOptions {
        width: 64,
        height: 32,
        duration: 3.0,
        ..Default::default()
    }
}

fn pixel(p: &[u8], x: usize, y: usize) -> &[u8] {
    &p[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4]
}

fn red(sample: &[u8]) -> bool {
    sample[0] > 200 && sample[1] < 20 && sample[2] < 20
}

fn near_black(sample: &[u8]) -> bool {
    sample[0] < 20 && sample[1] < 20 && sample[2] < 20
}

#[test]
fn width_mixed_calc_is_forty_pixels() {
    let pixels = frame(
        "<style>body{margin:0}div{width:calc(50% + 8px);height:32px;background-color:red}</style><div></div>",
    );
    let inside = pixel(&pixels, 36, 8);
    assert!(red(inside), "x=36 red, got {inside:?}");
    let outside = pixel(&pixels, 44, 8);
    assert!(near_black(outside), "x=44 near-black, got {outside:?}");
}

#[test]
fn height_mixed_calc_is_twenty_pixels() {
    let pixels = frame(
        "<style>body{margin:0}div{width:64px;height:calc(50% + 4px);background-color:red}</style><div></div>",
    );
    let inside = pixel(&pixels, 8, 10);
    assert!(red(inside), "y=10 red, got {inside:?}");
    let below = pixel(&pixels, 8, 24);
    assert!(near_black(below), "y=24 near-black, got {below:?}");
}

#[test]
fn pure_percent_width_is_half_the_canvas() {
    // `width: 50%` stays a percent. Taffy resolves that fraction against the
    // canvas, so the used width is 32px. It is not compiled to pixels.
    let pixels = frame(
        "<style>body{margin:0}div{width:50%;height:32px;background-color:red}</style><div></div>",
    );
    let inside = pixel(&pixels, 16, 8);
    assert!(red(inside), "x=16 red, got {inside:?}");
    let outside = pixel(&pixels, 40, 8);
    assert!(near_black(outside), "x=40 near-black, got {outside:?}");
}

#[test]
fn unswitched_length_still_requires_a_basis() {
    let error = compile(
        "<style>body{margin:0}div{transform:translateX(calc(50% + 8px))}</style><div></div>",
        canvas(),
    )
    .err()
    .unwrap()
    .to_string();
    assert!(
        error.contains("mixed calc() requires a length basis"),
        "{error}"
    );
}
