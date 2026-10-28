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

/// The bounds `[left, top, right, bottom]` of the pixels that pass `test`.
fn bounds(pixels: &[u8], test: impl Fn(&[u8]) -> bool) -> [usize; 4] {
    let mut b = [usize::MAX, usize::MAX, 0, 0];
    for (i, _) in pixels.chunks(4).enumerate().filter(|(_, p)| test(p)) {
        let (x, y) = (i % 64, i / 64);
        b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
    }
    b
}

fn inline_block_page(div_css: &str, body: &str) -> String {
    format!(
        "<style>body{{margin:0}}div{{width:64px;height:64px;color:white;font-size:16px;line-height:20px;{div_css}}}\
         b{{display:inline-block;padding:2px;background:#ff0000}}</style><div>{body}</div>"
    )
}

#[test]
fn an_inline_block_paints_its_box_on_the_line() {
    let pixels = frame(&inline_block_page("", "a<b>b</b>c"));
    let [left, top, right, bottom] = bounds(&pixels, red);
    assert!(left > 4 && right < 40, "after the a: {left}..{right}");
    // A 20px line plus 2px padding above and below, on the text baseline.
    assert!(top < 2 && (20..26).contains(&bottom), "{top}..{bottom}");
    let [.., c_right, _] = bounds(&pixels, white);
    assert!(c_right > right, "the c follows the box");
}

#[test]
fn an_inline_block_wraps_to_the_next_line() {
    let pixels = frame(&inline_block_page("width:44px", "aaaa <b>bb</b>"));
    let [left, top, ..] = bounds(&pixels, red);
    assert!(left < 4 && top >= 18, "the box starts the second line at {left},{top}");
}

#[test]
fn a_hidden_inline_child_takes_no_space() {
    let shown = frame(&inline_block_page("", "a<i>zzzz</i>"));
    let hidden = frame(&inline_block_page("", "a<i style='display:none'>zzzz</i>"));
    assert!(count(&hidden, white) * 3 < count(&shown, white));
}

#[test]
fn pre_keeps_its_lines_and_code_sets_monospace() {
    let pre = ink_bottom(&frame(&text_box("", "<pre style='margin:0'>aa\nbb</pre>")));
    assert_eq!(pre, ink_bottom(&frame(&text_box("white-space:pre;font-family:monospace", "aa\nbb"))));
    assert!(pre > 16, "pre bottom {pre}");
    // Monospace gives a narrow i the advance of a wide glyph.
    let lit = |p: &[u8]| p[0] > 40;
    let mono = bounds(&frame(&text_box("", "<code>iiii</code>")), lit);
    let sans = bounds(&frame(&text_box("", "iiii")), lit);
    assert!(mono[2] - mono[0] > 2 * (sans[2] - sans[0]), "{mono:?} {sans:?}");
}

#[test]
fn font_weight_and_stretch_drive_variable_font_axes() {
    let ink = |css: &str| count(&frame(&text_box(&format!("font-family:Archivo;{css}"), "HH")), |p| p[0] > 40);
    let lit = |p: &[u8]| p[0] > 40;
    assert!(ink("font-weight:900") > ink("font-weight:400") * 5 / 4, "{} {}", ink("font-weight:900"), ink("font-weight:400"));
    let wide = bounds(&frame(&text_box("font-family:Archivo;font-stretch:125%", "HH")), lit);
    let narrow = bounds(&frame(&text_box("font-family:Archivo;font-stretch:62%", "HH")), lit);
    assert!(wide[2] - wide[0] > (narrow[2] - narrow[0]) * 3 / 2, "{wide:?} {narrow:?}");
}

#[test]
fn centred_nowrap_text_keeps_its_line_height() {
    for align in ["left", "center", "right"] {
        let page = format!(
            "<style>body{{margin:0}}p{{margin:0;width:64px;font-size:16px;line-height:1.25;white-space:nowrap;\
             text-align:{align};color:white;background:#ff0000}}</style><p>ab</p>"
        );
        let pixels = frame(&page);
        let [.., bottom] = bounds(&pixels, red);
        assert_eq!(bottom, 19, "{align}: a 20px line box");
        assert!(count(&pixels, white) > 10, "{align}: the text paints");
    }
}

#[test]
fn pre_keeps_a_trailing_space_inside_an_inline_block() {
    // The space after the first box pushes the red box one advance right.
    let page = |first: &str| {
        format!(
            "<style>body{{margin:0}}p{{margin:0;white-space:pre;font-family:'JetBrains Mono';font-size:16px;color:white}}\
             span{{display:inline-block}}#b{{background:#ff0000}}</style><p><span>{first}</span><span id='b'>b</span></p>"
        )
    };
    let spaced = bounds(&frame(&page("a ")), red);
    let tight = bounds(&frame(&page("a")), red);
    let shift = spaced[0] as i64 - tight[0] as i64;
    assert!((8..=11).contains(&shift), "the trailing space is {shift}px");
}
