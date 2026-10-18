use director_core::systems::renderer::render_frame;
use director_html::{compile, CompileOptions};

const WIDTH: i32 = 64;
const HEIGHT: i32 = 32;

fn frame(source: &str, time: f64) -> Vec<u8> {
    let mut c = compile(
        source,
        CompileOptions {
            width: WIDTH,
            height: HEIGHT,
            duration: 3.0,
            ..Default::default()
        },
    )
    .unwrap();
    let mut surface = skia_safe::surfaces::raster_n32_premul((WIDTH, HEIGHT)).unwrap();
    render_frame(&mut c.director, time, surface.canvas()).unwrap();
    let info = skia_safe::ImageInfo::new(
        (WIDTH, HEIGHT),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let mut pixels = vec![0; (WIDTH * HEIGHT * 4) as usize];
    assert!(surface.read_pixels(&info, &mut pixels, (WIDTH * 4) as usize, (0, 0)));
    pixels
}

fn pixel(p: &[u8], x: usize, y: usize) -> &[u8] {
    let width = WIDTH as usize;
    &p[(y * width + x) * 4..(y * width + x) * 4 + 4]
}

fn svg_doc(rules: &str, inner: &str) -> String {
    format!(
        "<style>body{{margin:0}}{rules}</style><svg xmlns='http://www.w3.org/2000/svg' width='64' height='32' viewBox='0 0 64 32'>{inner}</svg>"
    )
}

fn near_black(px: &[u8]) -> bool {
    px[0] < 20 && px[1] < 20 && px[2] < 20
}

fn is_red(px: &[u8]) -> bool {
    px[0] >= 240 && px[1] < 20 && px[2] < 20
}

fn is_blue(px: &[u8]) -> bool {
    px[2] >= 240 && px[0] < 20 && px[1] < 20
}

#[test]
fn external_mask_shows_red_only_on_the_white_half() {
    let p = frame(
        &svg_doc(
            "svg .box{mask:url(#cut)}",
            "<mask id='cut' maskUnits='userSpaceOnUse' maskContentUnits='userSpaceOnUse' x='0' y='0' width='64' height='32'><rect width='32' height='32' fill='white'/><rect x='32' width='32' height='32' fill='black'/></mask><rect class='box' width='64' height='32' fill='red'/>",
        ),
        0.0,
    );
    let shown = pixel(&p, 8, 16);
    let hidden = pixel(&p, 48, 16);
    assert!(is_red(shown), "white half {shown:?}");
    assert!(near_black(hidden), "black half {hidden:?}");
}

#[test]
fn external_marker_end_paints_a_blue_cap() {
    let p = frame(
        &svg_doc(
            "svg .edge{marker-end:url(#arrow)}",
            "<marker id='arrow' markerUnits='userSpaceOnUse' markerWidth='16' markerHeight='16' refX='0' refY='8'><rect width='16' height='16' fill='blue'/></marker><line class='edge' x1='0' y1='16' x2='32' y2='16' stroke='red' stroke-width='8'/>",
        ),
        0.0,
    );
    let stroke = pixel(&p, 8, 16);
    let cap = pixel(&p, 40, 16);
    let empty = pixel(&p, 4, 4);
    assert!(is_red(stroke), "line {stroke:?}");
    assert!(is_blue(cap), "marker {cap:?}");
    assert!(near_black(empty), "empty {empty:?}");
}

#[test]
fn foreign_object_stays_rejected() {
    let message = compile(
        "<svg><foreignObject></foreignObject></svg>",
        CompileOptions {
            width: WIDTH,
            height: HEIGHT,
            duration: 3.0,
            ..Default::default()
        },
    )
    .err()
    .unwrap()
    .to_string();
    assert!(
        message.contains(
            "SVG foreignObject, SMIL, scripts, and nested images require native translation"
        ),
        "{message}"
    );
}
