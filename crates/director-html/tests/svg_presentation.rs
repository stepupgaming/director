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

fn svg_doc(rules: &str, inner: &str) -> String {
    format!(
        "<style>body{{margin:0}}{rules}</style><svg xmlns='http://www.w3.org/2000/svg' width='64' height='64' viewBox='0 0 64 64'>{inner}</svg>"
    )
}

fn assert_compile_contains(source: &str, needle: &str) {
    match compile(
        source,
        CompileOptions {
            width: 64,
            height: 64,
            duration: 3.0,
            ..Default::default()
        },
    ) {
        Ok(_) => panic!("expected compile failure containing {needle}"),
        Err(e) => {
            let message = format!("{e}");
            assert!(message.contains(needle), "{message}");
        }
    }
}

#[test]
fn external_fill_paints_a_rect_without_a_fill_attribute() {
    let p = frame(
        &svg_doc("svg .box{fill:red}", "<rect class='box' width='64' height='64'/>"),
        0.0,
    );
    assert_eq!(pixel(&p, 32, 32), [255, 0, 0, 255]);
}

#[test]
fn external_fill_opacity_composites_on_black() {
    let p = frame(
        &svg_doc(
            "svg .box{fill:red;opacity:0.5}",
            "<rect class='box' width='64' height='64'/>",
        ),
        0.0,
    );
    let center = pixel(&p, 32, 32);
    assert!(
        (center[0] as i16 - 128).abs() <= 2 && center[1] == 0 && center[2] == 0,
        "{center:?}"
    );
}

#[test]
fn external_color_sets_current_color() {
    let p = frame(
        &svg_doc(
            "svg .box{color:red}",
            "<rect class='box' width='64' height='64' fill='currentColor'/>",
        ),
        0.0,
    );
    assert_eq!(pixel(&p, 32, 32), [255, 0, 0, 255]);
}

#[test]
fn external_stroke_paints_the_border_and_not_the_center() {
    let p = frame(
        &svg_doc(
            "svg .box{stroke:blue;stroke-width:8px}",
            "<rect class='box' fill='none' x='8' y='8' width='48' height='48'/>",
        ),
        0.0,
    );
    let edge = pixel(&p, 32, 8);
    assert!(
        edge[2] >= 240 && edge[0] <= 15 && edge[1] <= 15,
        "top stroke {edge:?}"
    );
    assert_eq!(pixel(&p, 32, 32), [0, 0, 0, 255]);
}

#[test]
fn external_transform_moves_the_rect() {
    let p = frame(
        &svg_doc(
            "svg .box{transform:translate(32px, 0);fill:red}",
            "<rect class='box' x='0' y='0' width='16' height='64'/>",
        ),
        0.0,
    );
    assert_eq!(pixel(&p, 4, 32), [0, 0, 0, 255]);
    assert_eq!(pixel(&p, 40, 32), [255, 0, 0, 255]);
}

#[test]
fn external_clip_path_uses_an_svg_url() {
    let p = frame(
        &svg_doc(
            "svg .box{clip-path:url(#cut)}",
            "<clipPath id='cut'><rect width='32' height='64'/></clipPath><rect class='box' width='64' height='64' fill='red'/>",
        ),
        0.0,
    );
    assert_eq!(pixel(&p, 16, 32), [255, 0, 0, 255]);
    assert_eq!(pixel(&p, 48, 32), [0, 0, 0, 255]);
}

#[test]
fn external_fill_opacity_longhand_halves_red() {
    let p = frame(
        &svg_doc(
            "svg .box{fill:red;fill-opacity:0.5}",
            "<rect class='box' width='64' height='64'/>",
        ),
        0.0,
    );
    let center = pixel(&p, 32, 32);
    assert!(
        (center[0] as i16 - 128).abs() <= 2 && center[1] == 0 && center[2] == 0,
        "{center:?}"
    );
}

#[test]
fn external_stroke_opacity_halves_the_stroke() {
    let p = frame(
        &svg_doc(
            "svg .box{fill:none;stroke:blue;stroke-width:8px;stroke-opacity:0.5}",
            "<rect class='box' x='8' y='8' width='48' height='48'/>",
        ),
        0.0,
    );
    let edge = pixel(&p, 32, 8);
    assert!(
        (edge[2] as i16 - 128).abs() <= 8 && edge[0] <= 15 && edge[1] <= 15,
        "{edge:?}"
    );
    assert_eq!(pixel(&p, 32, 32), [0, 0, 0, 255]);
}

#[test]
fn external_fill_rule_evenodd_punches_the_hole() {
    let even = frame(
        &svg_doc(
            "svg .box{fill:red;fill-rule:evenodd}",
            "<path class='box' d='M0 0 H64 V64 H0 Z M16 16 H48 V48 H16 Z'/>",
        ),
        0.0,
    );
    let solid = frame(
        &svg_doc(
            "svg .box{fill:red}",
            "<path class='box' d='M0 0 H64 V64 H0 Z M16 16 H48 V48 H16 Z'/>",
        ),
        0.0,
    );
    assert_eq!(pixel(&even, 4, 4), [255, 0, 0, 255]);
    assert_eq!(pixel(&even, 32, 32), [0, 0, 0, 255]);
    assert_eq!(pixel(&solid, 32, 32), [255, 0, 0, 255]);
}

#[test]
fn external_animation_is_refused() {
    assert_compile_contains(
        &svg_doc(
            "@keyframes spin{from{opacity:1}to{opacity:1}}svg rect{animation:spin 1s linear infinite}",
            "<rect width='64' height='64'/>",
        ),
        "external stylesheet 'animation'",
    );
}

#[test]
fn svg_text_is_still_rejected() {
    assert_compile_contains(
        "<svg><text>No</text></svg>",
        "SVG text, foreignObject, SMIL, scripts, and nested images require native translation",
    );
}
