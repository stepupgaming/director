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
fn foreign_object_paints_red_over_blue() {
    let p = frame(
        &svg_doc(
            "",
            "<rect width='64' height='32' fill='#0000ff'/><foreignObject x='4' y='6' width='20' height='12'><div style='background:#ff0000;width:20px;height:12px'></div></foreignObject>",
        ),
        0.0,
    );
    let ink = pixel(&p, 6, 8);
    let left = pixel(&p, 2, 2);
    let right = pixel(&p, 30, 8);
    let below = pixel(&p, 6, 22);
    assert!(is_red(ink), "html {ink:?}");
    assert!(is_blue(left), "left {left:?}");
    assert!(is_blue(right), "right {right:?}");
    assert!(is_blue(below), "below {below:?}");
}

#[test]
fn empty_foreign_object_compiles() {
    compile(
        "<svg><foreignObject></foreignObject></svg>",
        CompileOptions {
            width: WIDTH,
            height: HEIGHT,
            duration: 3.0,
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn nested_svg_image_stays_rejected() {
    let message = compile(
        "<svg><image href='a.png'></image></svg>",
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
        message.contains("SVG scripts and nested images require native translation"),
        "{message}"
    );
}

#[test]
fn animate_moves_the_blue_rect() {
    let source = svg_doc(
        "",
        "<rect x='0' y='0' width='12' height='32' fill='#0000ff'><animate attributeName='x' from='0' to='40' dur='2s' fill='freeze'/></rect>",
    );
    let start = frame(&source, 0.0);
    let end = frame(&source, 2.0);
    assert!(is_blue(pixel(&start, 4, 16)), "start left {:?}", pixel(&start, 4, 16));
    assert!(near_black(pixel(&start, 46, 16)), "start right {:?}", pixel(&start, 46, 16));
    assert!(near_black(pixel(&end, 4, 16)), "end left {:?}", pixel(&end, 4, 16));
    assert!(is_blue(pixel(&end, 46, 16)), "end right {:?}", pixel(&end, 46, 16));
}

#[test]
fn set_holds_red_then_returns() {
    let source = svg_doc(
        "",
        "<rect width='64' height='32' fill='#0000ff'><set attributeName='fill' to='#ff0000' begin='1s' dur='1s'/></rect>",
    );
    assert!(is_blue(pixel(&frame(&source, 0.0), 8, 8)));
    assert!(is_red(pixel(&frame(&source, 1.2), 8, 8)));
    assert!(is_blue(pixel(&frame(&source, 2.2), 8, 8)));
}

#[test]
fn animate_transform_translates() {
    let source = svg_doc(
        "",
        "<rect x='0' y='0' width='12' height='32' fill='#0000ff'><animateTransform attributeName='transform' type='translate' from='0 0' to='40 0' dur='2s' fill='freeze'/></rect>",
    );
    let start = frame(&source, 0.0);
    let end = frame(&source, 2.0);
    assert!(is_blue(pixel(&start, 4, 16)), "start {:?}", pixel(&start, 4, 16));
    assert!(near_black(pixel(&start, 46, 16)), "start right {:?}", pixel(&start, 46, 16));
    assert!(near_black(pixel(&end, 4, 16)), "end left {:?}", pixel(&end, 4, 16));
    assert!(is_blue(pixel(&end, 46, 16)), "end {:?}", pixel(&end, 46, 16));
}

#[test]
fn animate_motion_follows_the_line() {
    let source = svg_doc(
        "",
        "<rect width='12' height='32' fill='#0000ff'><animateMotion path='M0,0 L40,0' dur='2s' fill='freeze'/></rect>",
    );
    let start = frame(&source, 0.0);
    let end = frame(&source, 2.0);
    assert!(is_blue(pixel(&start, 4, 16)), "start {:?}", pixel(&start, 4, 16));
    assert!(near_black(pixel(&start, 46, 16)), "start right {:?}", pixel(&start, 46, 16));
    assert!(near_black(pixel(&end, 4, 16)), "end left {:?}", pixel(&end, 4, 16));
    assert!(is_blue(pixel(&end, 46, 16)), "end {:?}", pixel(&end, 46, 16));
}

#[test]
fn animate_fill_ramps_through_purple() {
    let source = svg_doc(
        "",
        "<rect width='64' height='32' fill='#0000ff'><animate attributeName='fill' from='#0000ff' to='#ff0000' dur='2s' fill='freeze'/></rect>",
    );
    let start = frame(&source, 0.0);
    let mid_frame = frame(&source, 1.0);
    let end = frame(&source, 2.0);
    assert!(is_blue(pixel(&start, 8, 8)));
    let mid = pixel(&mid_frame, 8, 8);
    assert!(mid[0] > 100 && mid[2] > 100 && mid[1] < 30, "mid {mid:?}");
    assert!(is_red(pixel(&end, 8, 8)));
}

#[test]
fn smil_spline_requires_extraction() {
    let message = compile(
        "<svg><rect width='10' height='10'><animate attributeName='x' from='0' to='1' dur='1s' calcMode='spline' keySplines='0 0 1 1'/></rect></svg>",
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
        message.contains("SMIL calcMode spline requires extraction")
            || message.contains("SMIL keySplines requires extraction"),
        "{message}"
    );
}

#[test]
fn transformed_foreign_object_requires_extraction() {
    let message = compile(
        &svg_doc(
            "",
            "<foreignObject transform='translate(1 1)' x='0' y='0' width='10' height='10'><div></div></foreignObject>",
        ),
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
        message.contains("transformed foreignObject requires extraction"),
        "{message}"
    );
}
