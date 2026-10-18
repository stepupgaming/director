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
fn nested_width_mixed_calc_uses_the_parent_content_box() {
    // 32px parent. calc(50% + 8px) is 24. A composition basis of 64 would be 40.
    let pixels = frame(
        "<style>body{margin:0}.outer{width:32px;height:32px}.inner{width:calc(50% + 8px);height:32px;background-color:red}</style><div class='outer'><div class='inner'></div></div>",
    );
    let inside = pixel(&pixels, 20, 8);
    assert!(red(inside), "x=20 red, got {inside:?}");
    let outside = pixel(&pixels, 30, 8);
    assert!(near_black(outside), "x=30 near-black, got {outside:?}");
}

#[test]
fn nested_height_mixed_calc_uses_the_parent_content_box() {
    // 16px parent. calc(50% + 4px) is 12. A composition basis of 32 would be 20.
    let pixels = frame(
        "<style>body{margin:0}.outer{width:64px;height:16px}.inner{width:64px;height:calc(50% + 4px);background-color:red}</style><div class='outer'><div class='inner'></div></div>",
    );
    let inside = pixel(&pixels, 8, 8);
    assert!(red(inside), "y=8 red, got {inside:?}");
    let below = pixel(&pixels, 8, 14);
    assert!(near_black(below), "y=14 near-black, got {below:?}");
}

#[test]
fn stretched_auto_width_is_the_containing_block() {
    let pixels = frame(
        "<style>body{margin:0}.outer{height:32px}.inner{width:calc(50% + 8px);height:32px;background-color:red}</style><div class='outer'><div class='inner'></div></div>",
    );
    let inside = pixel(&pixels, 36, 8);
    assert!(red(inside), "x=36 red, got {inside:?}");
    let outside = pixel(&pixels, 44, 8);
    assert!(near_black(outside), "x=44 near-black, got {outside:?}");
}

#[test]
fn border_box_padding_shrinks_the_containing_block() {
    // Border box 48 with 8px padding. Content width 32. Inner calc is 24, placed at x=8.
    let pixels = frame(
        "<style>body{margin:0}.outer{box-sizing:border-box;width:48px;height:32px;padding:8px}.inner{width:calc(50% + 8px);height:16px;background-color:red}</style><div class='outer'><div class='inner'></div></div>",
    );
    let inside = pixel(&pixels, 12, 12);
    assert!(red(inside), "x=12 red, got {inside:?}");
    let outside = pixel(&pixels, 36, 12);
    assert!(near_black(outside), "x=36 near-black, got {outside:?}");
}

#[test]
fn column_gap_mixed_calc_uses_the_container_content_width() {
    // Own width 32. Gap is 20. Children are 4px, so the second starts at x=24.
    let pixels = frame(
        "<style>body{margin:0}.row{display:flex;width:32px;height:16px;column-gap:calc(50% + 4px)}.row div{width:4px;height:16px;background-color:red}</style><div class='row'><div></div><div></div></div>",
    );
    let first = pixel(&pixels, 2, 8);
    assert!(red(first), "x=2 red, got {first:?}");
    let gap = pixel(&pixels, 14, 8);
    assert!(near_black(gap), "x=14 near-black, got {gap:?}");
    let second = pixel(&pixels, 26, 8);
    assert!(red(second), "x=26 red, got {second:?}");
}

#[test]
fn flex_basis_mixed_calc_uses_the_container_content_width() {
    let pixels = frame(
        "<style>body{margin:0}.row{display:flex;width:32px;height:16px}.item{flex-basis:calc(50% + 8px);height:16px;background-color:red}</style><div class='row'><div class='item'></div></div>",
    );
    let inside = pixel(&pixels, 20, 8);
    assert!(red(inside), "x=20 red, got {inside:?}");
    let outside = pixel(&pixels, 28, 8);
    assert!(near_black(outside), "x=28 near-black, got {outside:?}");
}

#[test]
fn auto_height_mixed_calc_still_requires_a_basis() {
    let error = compile(
        "<style>body{margin:0}.outer{width:32px}.inner{width:32px;height:calc(50% + 4px)}</style><div class='outer'><div class='inner'></div></div>",
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

#[test]
fn inline_flex_shrink_to_fit_mixed_calc_still_requires_a_basis() {
    let error = compile(
        "<style>body{margin:0}</style><div style='display:inline-flex'><div style='width:calc(50% + 8px)'></div></div>",
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
