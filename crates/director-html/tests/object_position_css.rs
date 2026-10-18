use director_core::systems::renderer::render_frame;
use director_html::{compile, CompileOptions};

const RED_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAYAAADED76LAAAAEklEQVR4nGP4z8DwHx9mGBkKAMLXf4EvceABAAAAAElFTkSuQmCC";

fn frame(source: &str) -> Vec<u8> {
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
    render_frame(&mut c.director, 0.0, surface.canvas()).unwrap();
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

fn contain_img(object_position: Option<&str>) -> String {
    let position = match object_position {
        Some(value) => format!("object-position:{value};"),
        None => String::new(),
    };
    format!(
        "<style>body{{margin:0}}img{{width:32px;height:16px;object-fit:contain;{position}}}</style><img src=\"data:image/png;base64,{RED_PNG}\">"
    )
}

fn assert_red(sample: &[u8]) {
    assert!(
        sample[0] > 200 && sample[1] < 30 && sample[2] < 30 && sample[3] > 200,
        "{sample:?}"
    );
}

fn assert_empty(sample: &[u8]) {
    assert!(
        sample[0] < 20 && sample[1] < 20 && sample[2] < 20,
        "{sample:?}"
    );
}

fn assert_left_top(pixels: &[u8]) {
    assert_red(pixel(pixels, 4, 8));
    assert_empty(pixel(pixels, 28, 8));
}

#[test]
fn object_position_left_top() {
    assert_left_top(&frame(&contain_img(Some("left top"))));
}

#[test]
fn object_position_top_left() {
    assert_left_top(&frame(&contain_img(Some("top left"))));
}

#[test]
fn object_position_right_top() {
    let pixels = frame(&contain_img(Some("right top")));
    assert_empty(pixel(&pixels, 4, 8));
    assert_red(pixel(&pixels, 28, 8));
}

#[test]
fn object_position_omitted_is_centered() {
    let pixels = frame(&contain_img(None));
    assert_empty(pixel(&pixels, 4, 8));
    assert_red(pixel(&pixels, 20, 8));
    assert_empty(pixel(&pixels, 28, 8));
}

#[test]
fn object_position_zero_percent() {
    assert_left_top(&frame(&contain_img(Some("0% 0%"))));
}

#[test]
fn object_position_five_tokens_errors() {
    let err = compile(
        &contain_img(Some("10px 10px 10px 10px 10px")),
        CompileOptions {
            width: 64,
            height: 64,
            duration: 3.0,
            ..Default::default()
        },
    )
    .err()
    .unwrap()
    .to_string();
    assert!(err.contains("object-position"), "{err}");
}
