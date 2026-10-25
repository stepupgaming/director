use director_render::render_frame;
use director_html::{compile, CompileOptions};

fn options() -> CompileOptions {
    CompileOptions {
        width: 64,
        height: 64,
        duration: 3.0,
        ..Default::default()
    }
}

fn frame(source: &str) -> Vec<u8> {
    let mut c = compile(source, options()).unwrap();
    let mut surface = skia_safe::surfaces::raster_n32_premul((64, 64)).unwrap();
    render_frame(&mut c.director, 0.0, &mut surface).unwrap();
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

fn ink_bottom(pixels: &[u8]) -> usize {
    pixels
        .chunks(4)
        .enumerate()
        .filter(|(_, p)| p[0] > 40 || p[1] > 40 || p[2] > 40)
        .map(|(i, _)| i / 64)
        .max()
        .unwrap_or(0)
}

fn count(pixels: &[u8], test: impl Fn(&[u8]) -> bool) -> usize {
    pixels.chunks(4).filter(|p| test(p)).count()
}

fn text_box(css: &str, text: &str) -> String {
    format!(
        "<style>body{{margin:0}}div{{width:64px;height:64px;color:white;font-size:16px;line-height:1;{css}}}</style><div>{text}</div>"
    )
}

#[test]
fn br_breaks_the_line_when_spaces_collapse() {
    for mode in ["normal", "nowrap"] {
        let css = format!("white-space:{mode}");
        let broken = ink_bottom(&frame(&text_box(&css, "aa<br>bb")));
        let single = ink_bottom(&frame(&text_box(&css, "aa bb")));
        assert!(broken > single + 8, "{mode}: br bottom {broken}, one line {single}");
    }
    let preserved = ink_bottom(&frame(&text_box("white-space:pre-wrap", "aa<br>bb")));
    assert!(preserved > 16, "pre-wrap br bottom {preserved}");
}

#[test]
fn spaces_around_br_leave_centred_lines_where_they_were() {
    let css = "text-align:center";
    assert_eq!(
        frame(&text_box(css, "aa <br> bb")),
        frame(&text_box(css, "aa<br>bb")),
        "a space beside the break moved a line"
    );
    assert_eq!(
        frame(&text_box(css, "aa<br>")),
        frame(&text_box(css, "aa")),
        "a trailing br added a line"
    );
}

fn reset_page(span_css: &str, div_css: &str) -> String {
    format!(
        "<style>*{{margin:0;padding:0;box-sizing:border-box}}div{{width:64px;height:64px;color:white;font-size:32px;line-height:1;{div_css}}}span{{{span_css}}}</style><div>a<span>b</span></div>"
    )
}

fn red(p: &[u8]) -> bool {
    p[0] > 200 && p[1] < 60 && p[2] < 60
}

fn white(p: &[u8]) -> bool {
    p[0] > 200 && p[1] > 200 && p[2] > 200
}

#[test]
fn a_coloured_span_compiles_under_a_universal_reset() {
    let pixels = frame(&reset_page("color:#ff0000", ""));
    assert!(count(&pixels, red) > 20, "the span's red");
    assert!(count(&pixels, white) > 20, "the div's white");
}

#[test]
fn a_coloured_span_keeps_its_colour_inside_an_outlined_block() {
    let pixels = frame(&reset_page(
        "color:#ff0000",
        "-webkit-text-stroke:4px #0000ff;paint-order:stroke fill",
    ));
    assert!(count(&pixels, red) > 20, "the span's red fill");
    assert!(count(&pixels, white) > 20, "the div's white fill");
    assert!(
        count(&pixels, |p| p[2] > 200 && p[0] < 60 && p[1] < 60) > 20,
        "the blue stroke"
    );
}

#[test]
fn a_span_with_its_own_box_still_requires_extraction() {
    let error = compile(&reset_page("color:#ff0000;padding:4px", ""), options())
        .err()
        .expect("padding on an inline box is refused");
    assert!(
        format!("{error:?}").contains("styled inline boxes require extraction"),
        "{error:?}"
    );
}
