use director_core::systems::renderer::render_frame;
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
    render_frame(&mut c.director, time, surface.canvas()).unwrap();
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

fn html(filter: &str) -> String {
    format!(
        "<style>body{{margin:0}}div{{width:64px;height:64px;background:red;filter:{filter}}}</style><div></div>"
    )
}

fn compile_error(filter: &str) -> String {
    compile(
        &html(filter),
        CompileOptions {
            width: 64,
            height: 64,
            duration: 3.0,
            ..Default::default()
        },
    )
    .err()
    .unwrap()
    .to_string()
}

#[test]
fn invert_one_paints_cyan() {
    let pixels = frame(&html("invert(1)"), 0.0);
    let mid = pixel(&pixels, 32, 32);
    assert!(mid[0] < 30 && mid[1] > 200 && mid[2] > 200, "{mid:?}");
}

#[test]
fn sepia_one_is_not_pure_red() {
    let pixels = frame(&html("sepia(1)"), 0.0);
    let mid = pixel(&pixels, 32, 32);
    assert!(mid[1] > 40, "{mid:?}");
}

#[test]
fn opacity_zero_is_near_black() {
    let pixels = frame(&html("opacity(0)"), 0.0);
    let mid = pixel(&pixels, 32, 32);
    assert!(mid[0] < 8 && mid[1] < 8 && mid[2] < 8, "{mid:?}");
}

#[test]
fn none_stays_red() {
    let pixels = frame(&html("none"), 0.0);
    let mid = pixel(&pixels, 32, 32);
    assert!(mid[0] > 200 && mid[1] < 30 && mid[2] < 30, "{mid:?}");
}

#[test]
fn unknown_filter_is_rejected() {
    // CSS clamps invert() above 1 to 1, so invert(2) is not the unknown-function path.
    let pixels = frame(&html("invert(2)"), 0.0);
    let mid = pixel(&pixels, 32, 32);
    assert!(mid[0] < 30 && mid[1] > 200 && mid[2] > 200, "{mid:?}");

    // A trailing token has no function call. That reject path is not "unsupported filter".
    let extra = compile_error("blur(1px) extra");
    assert!(
        extra.contains("invalid filter function") || extra.contains("unsupported filter"),
        "{extra}"
    );

    let waffle = compile_error("waffle(1)");
    assert!(waffle.contains("unsupported filter"), "{waffle}");
}
