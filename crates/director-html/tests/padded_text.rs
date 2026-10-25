use director_render::render_frame;
use director_html::{compile, CompileOptions};

const WIDTH: i32 = 512;
const HEIGHT: i32 = 384;

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

/// A padded box whose width layout already knows measures its text at the
/// content width, so the box grows for every line the text paints.
#[test]
fn a_padded_box_holds_every_line_of_its_text() {
    let layouts = [
        (
            "flex max-width",
            ".row{position:absolute;top:0;left:6px;width:500px;display:flex;justify-content:center}\
             .card{max-width:500px}",
        ),
        (
            "flex width",
            ".row{position:absolute;top:0;left:6px;width:500px;display:flex}.card{width:500px}",
        ),
        ("grid", ".row{position:absolute;top:0;left:6px;width:500px;display:grid}"),
        ("block", ".row{position:absolute;top:0;left:6px;width:500px}"),
    ];
    let texts = [
        "I just feel overwhelmed by my life at that moment",
        "Did you find any numbers around like what percentage of people?",
        "Nobody talks about meditation in that way",
    ];
    let mut overflowed = Vec::new();
    for (name, layout) in layouts {
        for text in texts {
            for size in [24, 32, 40] {
                let source = format!(
                    "<style>body{{margin:0;background:black}}{layout}\
                     .card{{padding:8px 60px 20px;background:#0000ff;color:white;\
                     font-size:{size}px;line-height:1.2;text-align:center}}</style>\
                     <div class=\"row\"><div class=\"card\">{text}</div></div>"
                );
                let pixels = frame(&source);
                let ink = bottom(&pixels, |p| p[0] > 128 && p[1] > 128 && p[2] > 128);
                let card = bottom(&pixels, |p| p[0] < 64 && p[1] < 64 && p[2] > 192);
                if ink + 10 > card {
                    overflowed.push(format!("{name} {size}px {text:?}: ink to row {ink}, box to row {card}"));
                }
            }
        }
    }
    assert!(overflowed.is_empty(), "text ran past its padded box:\n{}", overflowed.join("\n"));
}
