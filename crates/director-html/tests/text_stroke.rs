use director_core::systems::renderer::render_frame;
use director_html::{compile, CompileOptions};

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

fn pixel(pixels: &[u8], x: usize, y: usize) -> &[u8] {
    &pixels[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4]
}

fn source(extra: &str) -> String {
    format!(
        "<style>body{{margin:0}}div{{width:64px;height:64px;font-size:48px;line-height:1;{extra}}}</style><div>M</div>"
    )
}

#[test]
fn transparent_fill_paints_red_stroke_and_leaves_the_corner() {
    let pixels = frame(&source(
        "color:transparent;-webkit-text-stroke:2px red",
    ));
    assert!(
        pixels.chunks(4).any(|sample| sample[0] > 180 && sample[1] < 80 && sample[2] < 80),
        "text stroke painted no red"
    );
    let corner = pixel(&pixels, 2, 2);
    assert!(
        corner[0] < 20 && corner[1] < 20 && corner[2] < 20,
        "{corner:?}"
    );
}

#[test]
fn white_fill_without_stroke_still_paints() {
    let pixels = frame(&source("color:white"));
    assert!(
        pixels
            .chunks(4)
            .any(|sample| sample[0] > 200 && sample[1] > 200 && sample[2] > 200),
        "white text painted no white pixel"
    );
}

#[test]
fn unparsable_stroke_is_rejected() {
    let err = compile(
        &source("-webkit-text-stroke:waffle"),
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
    assert!(err.contains("unsupported text-stroke"), "{err}");
}

fn count(pixels: &[u8], test: impl Fn(&[u8]) -> bool) -> usize {
    pixels.chunks(4).filter(|sample| test(sample)).count()
}

fn white(sample: &[u8]) -> bool {
    sample[0] > 230 && sample[1] > 230 && sample[2] > 230
}

fn black(sample: &[u8]) -> bool {
    sample[0] < 30 && sample[1] < 30 && sample[2] < 30
}

#[test]
fn black_stroke_keeps_its_colour_around_an_opaque_white_fill() {
    let pixels = frame(&source(
        "background:#0000ff;color:white;-webkit-text-stroke:4px black;paint-order:stroke fill",
    ));
    assert!(count(&pixels, white) > 20, "white fill painted no white");
    assert!(count(&pixels, black) > 20, "black stroke painted no black");
}

#[test]
fn paint_order_normal_paints_the_stroke_over_the_fill() {
    let under = frame(&source(
        "background:#0000ff;color:white;-webkit-text-stroke:4px black;paint-order:stroke",
    ));
    let over = frame(&source(
        "background:#0000ff;color:white;-webkit-text-stroke:4px black",
    ));
    let (under_white, over_white) = (count(&under, white), count(&over, white));
    assert!(
        over_white < under_white,
        "stroke over fill left {over_white} white pixels, under left {under_white}"
    );
    assert!(count(&over, black) > count(&under, black));
}

#[test]
fn repeated_paint_order_layer_is_rejected() {
    let err = compile(
        &source("paint-order:fill fill"),
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
    assert!(err.contains("unsupported paint-order"), "{err}");
}
