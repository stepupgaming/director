use director_render::render_frame;
use director_html::{compile, CompileOptions};

fn frame(source: &str, time: f64) -> Vec<u8> {
    let mut c = compile(
        source,
        CompileOptions {
            width: 64,
            height: 64,
            duration: 3.0,
            ..Default::default()
        },
    )
    .unwrap();
    let mut surface = skia_safe::surfaces::raster_n32_premul((64, 64)).unwrap();
    render_frame(&mut c.director, time, &mut surface).unwrap();
    let info = skia_safe::ImageInfo::new(
        (64, 64),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let mut pixels = vec![0; 64 * 64 * 4];
    assert!(surface.read_pixels(&info, &mut pixels, 64 * 4, (0, 0)));
    pixels
}

fn pixel(p: &[u8], x: usize, y: usize) -> &[u8] {
    &p[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4]
}

fn near_black(sample: &[u8]) {
    assert!(
        sample[0] < 20 && sample[1] < 20 && sample[2] < 20,
        "{sample:?}"
    );
}

#[test]
fn fixed_child_pins_to_the_composition_corner() {
    let html = "<style>body{margin:0}.shift{position:relative;left:32px;top:16px;width:32px;height:32px;background:blue}.pin{position:fixed;left:0;top:0;width:8px;height:8px;background:red}</style><div class='shift'><div class='pin'></div></div>";
    let pixels = frame(html, 0.0);
    assert_eq!(pixel(&pixels, 2, 2), &[255, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 34, 18), &[0, 0, 255, 255]);
}

#[test]
fn fixed_inset_uses_the_composition_origin() {
    let html = "<style>body{margin:0}.shift{position:relative;left:32px;top:16px;width:32px;height:32px;background:blue}.pin{position:fixed;left:4px;top:4px;width:8px;height:8px;background:red}</style><div class='shift'><div class='pin'></div></div>";
    let pixels = frame(html, 0.0);
    assert_eq!(pixel(&pixels, 6, 6), &[255, 0, 0, 255]);
    near_black(pixel(&pixels, 2, 2));
    assert_eq!(pixel(&pixels, 34, 18), &[0, 0, 255, 255]);
}

#[test]
fn fixed_percent_inset_uses_the_composition_width() {
    let html = "<style>body{margin:0}.shift{position:relative;left:8px;top:8px;width:32px;height:32px;background:blue}.pin{position:fixed;left:50%;top:0;width:8px;height:8px;background:red}</style><div class='shift'><div class='pin'></div></div>";
    let pixels = frame(html, 0.0);
    assert_eq!(pixel(&pixels, 34, 2), &[255, 0, 0, 255]);
    near_black(pixel(&pixels, 18, 2));
}

#[test]
fn sticky_ignores_inset_at_scroll_zero() {
    let html = "<style>body{margin:0}.space{height:16px}.pin{position:sticky;top:20px;width:64px;height:8px;background:red}</style><div class='space'></div><div class='pin'></div>";
    let pixels = frame(html, 0.0);
    assert_eq!(pixel(&pixels, 4, 18), &[255, 0, 0, 255]);
    near_black(pixel(&pixels, 4, 4));
    near_black(pixel(&pixels, 4, 40));
}

#[test]
fn sticky_auto_width_stretches() {
    let html = "<style>body{margin:0}.space{height:16px}.pin{position:sticky;top:20px;height:8px;background:red}</style><div class='space'></div><div class='pin'></div>";
    let pixels = frame(html, 0.0);
    assert_eq!(pixel(&pixels, 4, 18), &[255, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 60, 18), &[255, 0, 0, 255]);
    near_black(pixel(&pixels, 4, 40));
}
