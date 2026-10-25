use director_render::render_frame;
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

fn source(extra: &str, text: &str) -> String {
    format!(
        "<style>body{{margin:0}}div{{width:64px;height:64px;color:red;font-size:16px;line-height:1;{extra}}}</style><div>{text}</div>"
    )
}

fn ink_rows(pixels: &[u8]) -> Vec<usize> {
    (0..64)
        .filter(|&y| {
            (0..64).any(|x| {
                let sample = pixel(pixels, x, y);
                sample[0] > 40 || sample[1] > 40 || sample[2] > 40
            })
        })
        .collect()
}

fn row_dark(pixels: &[u8], y: usize) -> bool {
    (0..64).all(|x| {
        let sample = pixel(pixels, x, y);
        sample[0] < 20 && sample[1] < 20 && sample[2] < 20
    })
}

const SENTENCE: &str = "MMMM MMMM MMMM MMMM";

#[test]
fn line_clamp_hides_the_second_line() {
    let open = frame(&source("", SENTENCE));
    let clamped = frame(&source("line-clamp:1;", SENTENCE));
    let open_rows = ink_rows(&open);
    let clamp_rows = ink_rows(&clamped);
    assert!(!open_rows.is_empty(), "unclamped text did not paint");
    assert!(!clamp_rows.is_empty(), "line-clamp ate the first line");
    let open_last = *open_rows.last().unwrap();
    let clamp_last = *clamp_rows.last().unwrap();
    assert!(
        open_last >= clamp_last + 8,
        "second line did not extend past the clamp: open {open_last} clamp {clamp_last}"
    );
    assert!(
        row_dark(&clamped, open_last),
        "line-clamp still painted the lower line at {open_last}"
    );
    assert_eq!(
        clamped,
        frame(&source("-webkit-line-clamp:1;", SENTENCE)),
        "webkit line-clamp diverged"
    );
}

#[test]
fn line_clamp_none_keeps_the_second_line() {
    let open = frame(&source("", SENTENCE));
    let none = frame(&source("line-clamp:none;", SENTENCE));
    let clamped = frame(&source("line-clamp:1;", SENTENCE));
    assert_eq!(none, open);
    let none_last = *ink_rows(&none).last().expect("none painted nothing");
    let clamp_last = *ink_rows(&clamped).last().expect("clamp painted nothing");
    assert!(
        none_last >= clamp_last + 8,
        "none dropped the second line: none {none_last} clamp {clamp_last}"
    );
}

#[test]
fn line_clamp_keeps_a_short_word() {
    let open = frame(&source("", "M"));
    let clamped = frame(&source("line-clamp:1;", "M"));
    assert_eq!(clamped, open);
    assert!(
        !ink_rows(&clamped).is_empty(),
        "line-clamp ate the short word"
    );
}

#[test]
fn line_clamp_rejects_waffle_zero_and_negative() {
    let options = CompileOptions {
        width: 64,
        height: 64,
        duration: 3.0,
        ..Default::default()
    };
    for value in ["waffle", "0", "-1"] {
        let err = compile(&source(&format!("line-clamp:{value};"), "M"), options.clone())
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("unsupported line-clamp"), "{value}: {err}");
    }
}
