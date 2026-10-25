use director_render::render_frame;
use director_html::{compile, CompileOptions};

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

fn pixel(p: &[u8], x: usize, y: usize) -> &[u8] {
    &p[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4]
}

fn ink_bottom(pixels: &[u8]) -> usize {
    let mut bottom = 0usize;
    for y in 0..64 {
        for x in 0..64 {
            let sample = pixel(pixels, x, y);
            if sample[0] > 40 || sample[1] > 40 || sample[2] > 40 {
                bottom = y;
            }
        }
    }
    bottom
}

fn ink_right(pixels: &[u8]) -> usize {
    let mut right = 0usize;
    for y in 0..64 {
        for x in 0..64 {
            let sample = pixel(pixels, x, y);
            if (sample[0] > 40 || sample[1] > 40 || sample[2] > 40) && x > right {
                right = x;
            }
        }
    }
    right
}

fn options() -> CompileOptions {
    CompileOptions {
        width: 64,
        height: 64,
        duration: 3.0,
        ..Default::default()
    }
}

fn text_box(css: &str, text: &str) -> String {
    format!(
        "<style>body{{margin:0}}div{{width:64px;height:64px;color:white;font-size:16px;line-height:1;{css}}}</style><div>{text}</div>"
    )
}

#[test]
fn pre_line_keeps_newlines_and_collapses_spaces() {
    compile(&text_box("white-space:pre-line", "ab"), options()).unwrap();
    let broken = frame(&text_box("white-space:pre-line", "aa\nbb"));
    let collapsed = frame(&text_box("white-space:normal", "aa\nbb"));
    assert!(
        ink_bottom(&broken) > 16,
        "pre-line bottom {}",
        ink_bottom(&broken)
    );
    assert!(
        ink_bottom(&collapsed) < 24,
        "normal bottom {}",
        ink_bottom(&collapsed)
    );
    assert!(
        ink_bottom(&broken) > ink_bottom(&collapsed) + 8,
        "pre-line {} vs normal {}",
        ink_bottom(&broken),
        ink_bottom(&collapsed)
    );
    let spaced = frame(&text_box("white-space:pre-line", "a  b"));
    let single = frame(&text_box("white-space:pre-line", "a b"));
    let tabs = frame(&text_box("white-space:pre-line", "a\t\tb"));
    let kept = frame(&text_box("white-space:pre-wrap", "a  b"));
    assert_eq!(spaced, single, "pre-line kept the double space");
    assert_eq!(tabs, single, "pre-line kept the tabs");
    assert!(
        ink_right(&kept) > ink_right(&spaced),
        "pre-wrap right {} vs pre-line {}",
        ink_right(&kept),
        ink_right(&spaced)
    );
}

#[test]
fn break_all_wraps_and_keep_all_errors() {
    let word = "MMMMMMMMMMMMMMMMMMMMMMMM";
    let broken = frame(&text_box("word-break:break-all", word));
    let normal = frame(&text_box("white-space:normal", word));
    assert!(
        ink_bottom(&normal) < 24,
        "normal bottom {}",
        ink_bottom(&normal)
    );
    assert!(
        ink_bottom(&broken) > ink_bottom(&normal) + 8,
        "break-all {} vs normal {}",
        ink_bottom(&broken),
        ink_bottom(&normal)
    );
    let err = compile(
        "<style>div{word-break:keep-all}</style><div>ab</div>",
        options(),
    )
    .err()
    .unwrap()
    .to_string();
    assert!(err.contains("unsupported word-break"), "{err}");
}
