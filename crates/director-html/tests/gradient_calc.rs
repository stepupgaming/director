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
fn html(background: &str) -> String {
    format!(
        "<style>body{{margin:0}}div{{width:64px;height:64px;background:{background}}}</style><div></div>"
    )
}
fn options() -> CompileOptions {
    CompileOptions {
        width: 64,
        height: 64,
        duration: 3.0,
        ..Default::default()
    }
}

#[test]
fn mixed_calc_stop_uses_the_gradient_line() {
    let painted = frame(
        &html("linear-gradient(to right, red calc(50% + 8px), blue calc(50% + 8px))"),
        0.0,
    );
    let left = pixel(&painted, 36, 32);
    assert!(
        left[0] > 200 && left[1] < 20 && left[2] < 20,
        "x=36 red, got {left:?}"
    );
    let right = pixel(&painted, 44, 32);
    assert!(
        right[2] > 200 && right[0] < 20 && right[1] < 20,
        "x=44 blue, got {right:?}"
    );
}

#[test]
fn mixed_calc_with_em_still_errors() {
    let error = compile(
        &html("linear-gradient(to right, red calc(50% + 10px + 1em), blue 100%)"),
        options(),
    )
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains("mixed calc"), "{error}");
}

#[test]
fn plain_linear_gradient_still_compiles() {
    compile(
        &html("linear-gradient(to right, red, blue)"),
        options(),
    )
    .unwrap();
}
