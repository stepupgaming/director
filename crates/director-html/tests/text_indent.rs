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

fn bright(pixels: &[u8], x_ok: impl Fn(usize) -> bool) -> bool {
    pixels.chunks(4).enumerate().any(|(index, sample)| {
        x_ok(index % 64) && sample[0] > 200 && sample[1] > 200 && sample[2] > 200
    })
}

fn source(indent: &str) -> String {
    format!(
        "<style>body{{margin:0}}div{{width:64px;height:64px;color:white;font-size:16px;{indent}}}</style><div>M</div>"
    )
}

#[test]
fn no_indent_paints_before_sixteen_pixels() {
    let pixels = frame(&source(""));
    assert!(bright(&pixels, |x| x < 16), "M did not paint in x<16");
}

#[test]
fn length_indent_shifts_the_glyph_right() {
    let pixels = frame(&source("text-indent:24px"));
    assert!(!bright(&pixels, |x| x < 16), "ink remained in x<16");
    assert!(bright(&pixels, |x| x >= 24), "no ink at x>=24");
}

#[test]
fn hanging_indent_is_rejected() {
    let err = compile(
        &source("text-indent:hanging 10px"),
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
    assert!(err.contains("unsupported text-indent"), "{err}");
}
