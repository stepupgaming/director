use director_core::systems::renderer::render_frame;
use director_html::{compile, CompileOptions};

fn frame(source: &str, width: i32, height: i32) -> Vec<u8> {
    let mut c = compile(source, options(width, height)).unwrap();
    let mut surface = skia_safe::surfaces::raster_n32_premul((width, height)).unwrap();
    render_frame(&mut c.director, 0.0, surface.canvas()).unwrap();
    let info = skia_safe::ImageInfo::new(
        (width, height),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let mut pixels = vec![0; (width as usize) * (height as usize) * 4];
    assert!(surface.read_pixels(&info, &mut pixels, (width * 4) as usize, (0, 0)));
    pixels
}

fn pixel(p: &[u8], width: i32, x: usize, y: usize) -> &[u8] {
    let i = (y * width as usize + x) * 4;
    &p[i..i + 4]
}

fn options(width: i32, height: i32) -> CompileOptions {
    CompileOptions {
        width,
        height,
        duration: 3.0,
        ..Default::default()
    }
}

fn stage(width: i32, height: i32, css: &str, body: &str) -> String {
    format!(
        "<style>body{{margin:0}}.stage{{position:relative;width:{width}px;height:{height}px}}{css}</style><div class='stage'>{body}</div>"
    )
}

fn assert_black(sample: &[u8]) {
    assert!(
        sample[0] < 20 && sample[1] < 20 && sample[2] < 20,
        "{sample:?}"
    );
}

#[test]
fn nested_orientation_on_a_wide_canvas() {
    let html = stage(
        64,
        32,
        ".land{position:absolute;left:0;top:0;width:32px;height:32px;@media (orientation: landscape){background:red}}.port{position:absolute;left:32px;top:0;width:32px;height:32px;@media (orientation: portrait){background:red}}",
        "<div class='land'></div><div class='port'></div>",
    );
    let p = frame(&html, 64, 32);
    assert_eq!(pixel(&p, 64, 8, 8), &[255, 0, 0, 255], "landscape");
    assert_black(pixel(&p, 64, 48, 8));
}

#[test]
fn nested_orientation_portrait_on_a_tall_canvas() {
    let html = stage(
        32,
        64,
        ".port{position:absolute;left:0;top:0;width:32px;height:32px;@media (orientation: portrait){background:red}}",
        "<div class='port'></div>",
    );
    let p = frame(&html, 32, 64);
    assert_eq!(pixel(&p, 32, 8, 8), &[255, 0, 0, 255], "portrait");
}

#[test]
fn top_level_media_stays_unsupported() {
    let err = compile(
        "<style>@media (orientation: landscape) { .card { background: red } }</style><div></div>",
        options(64, 32),
    )
    .err()
    .unwrap()
    .to_string();
    assert!(err.contains("unsupported CSS at-rule"), "{err}");
}

#[test]
fn nested_supports_object_fit_cover_paints_red() {
    let html = stage(
        64,
        32,
        ".card{position:absolute;left:0;top:0;width:64px;height:32px;@supports (object-fit: cover){background:red}}",
        "<div class='card'></div>",
    );
    let p = frame(&html, 64, 32);
    assert_eq!(pixel(&p, 64, 8, 8), &[255, 0, 0, 255], "object-fit: cover");
}

#[test]
fn nested_supports_position_fixed_and_sticky_paint_red() {
    let html = stage(
        64,
        32,
        ".fixed{position:absolute;left:0;top:0;width:32px;height:32px;@supports (position: fixed){background:red}}.sticky{position:absolute;left:32px;top:0;width:32px;height:32px;@supports (position: sticky){background:red}}",
        "<div class='fixed'></div><div class='sticky'></div>",
    );
    let p = frame(&html, 64, 32);
    assert_eq!(pixel(&p, 64, 8, 8), &[255, 0, 0, 255], "position: fixed");
    assert_eq!(pixel(&p, 64, 40, 8), &[255, 0, 0, 255], "position: sticky");
}

#[test]
fn nested_min_aspect_ratio_matches_two_to_one() {
    let wide = stage(
        64,
        32,
        ".card{position:absolute;left:0;top:0;width:64px;height:32px;@media (min-aspect-ratio: 2/1){background:red}}",
        "<div class='card'></div>",
    );
    let p = frame(&wide, 64, 32);
    assert_eq!(pixel(&p, 64, 8, 8), &[255, 0, 0, 255], "64x32");

    let tall = stage(
        32,
        64,
        ".card{position:absolute;left:0;top:0;width:32px;height:64px;@media (min-aspect-ratio: 2/1){background:red}}",
        "<div class='card'></div>",
    );
    let p = frame(&tall, 32, 64);
    assert_black(pixel(&p, 32, 8, 8));
}
