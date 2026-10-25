use director_render::render_frame;
use director_html::{compile, CompileOptions};

const WIDTH: i32 = 512;
const HEIGHT: i32 = 128;

fn frame(source: &str) -> Vec<u8> {
    let mut c = compile(
        source,
        CompileOptions {
            width: WIDTH,
            height: HEIGHT,
            duration: 1.0,
            ..Default::default()
        },
    )
    .unwrap();
    let mut surface = skia_safe::surfaces::raster_n32_premul((WIDTH, HEIGHT)).unwrap();
    render_frame(&mut c.director, 0.0, &mut surface).unwrap();
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

/// The lowest row with a pixel that `hit` matches.
fn bottom(pixels: &[u8], hit: impl Fn(&[u8]) -> bool) -> usize {
    pixels
        .chunks((WIDTH * 4) as usize)
        .enumerate()
        .filter(|(_, row)| row.chunks(4).any(&hit))
        .map(|(y, _)| y)
        .last()
        .unwrap_or(0)
}

/// A box sized to its text's max-content width keeps the text, and its own height,
/// on one line wherever layout rounding puts its edges, for each text alignment.
/// Kerned text at display sizes covers the gap between shaped and unkerned widths.
#[test]
fn text_in_a_shrink_wrapped_box_stays_on_one_line() {
    let cases = (12..=30)
        .map(|size| ("ab cd ef", size))
        .chain([36, 48, 64, 72].map(|size| ("To Wave AV", size)));
    let mut wrapped = Vec::new();
    for (text, size) in cases {
        for align in ["center", "left", "right"] {
            for left in ["0", "0.25", "0.5", "0.75"] {
                let source = format!(
                    "<style>body{{margin:0;background:black}}\
                     .row{{position:absolute;top:0;left:{left}px;width:500px;display:flex;justify-content:center}}\
                     .card{{color:white;background:#0000ff;font-size:{size}px;line-height:1;text-align:{align}}}</style>\
                     <div class=\"row\"><div class=\"card\">{text}</div></div>"
                );
                let pixels = frame(&source);
                let ink = bottom(&pixels, |p| p[0] > 128 && p[1] > 128 && p[2] > 128);
                let card = bottom(&pixels, |p| p[2] > 128);
                if ink > size as usize + 2 || card > size as usize + 1 {
                    wrapped.push(format!("{text:?} {align} {size}px at left {left}: ink to row {ink}, box to row {card}"));
                }
            }
        }
    }
    assert!(wrapped.is_empty(), "text wrapped in its own max-content box:\n{}", wrapped.join("\n"));
}
