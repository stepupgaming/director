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

fn red(sample: &[u8]) -> bool {
    sample[0] > 200 && sample[1] < 40 && sample[2] < 40
}

fn dark(sample: &[u8]) -> bool {
    sample[0] < 20 && sample[1] < 20 && sample[2] < 20
}

#[test]
fn sized_linear_gradient_repeats_across_the_box() {
    let source = |size: &str| {
        format!(
            "<style>body{{margin:0}}div{{width:64px;height:64px;background-image:linear-gradient(to right, red 2px, transparent 2px);{size}}}</style><div></div>"
        )
    };
    let tiled = frame(&source("background-size:16px 16px"));
    let full = frame(&source(""));
    assert!(red(pixel(&tiled, 1, 32)), "first stripe {:?}", pixel(&tiled, 1, 32));
    assert!(dark(pixel(&tiled, 8, 32)), "gap {:?}", pixel(&tiled, 8, 32));
    assert!(red(pixel(&tiled, 17, 32)), "second stripe {:?}", pixel(&tiled, 17, 32));
    assert!(dark(pixel(&full, 17, 32)), "unsized stripe repeated");
}

#[test]
fn sized_radial_gradient_tiles_as_dots() {
    let tiled = frame(
        "<style>body{margin:0}div{width:64px;height:64px;background-image:radial-gradient(circle, red 3px, transparent 3.5px);background-size:16px 16px}</style><div></div>",
    );
    for (x, y) in [(8, 8), (24, 8), (8, 24), (40, 56)] {
        assert!(red(pixel(&tiled, x, y)), "dot at {x},{y} {:?}", pixel(&tiled, x, y));
    }
    for (x, y) in [(16, 16), (0, 0), (16, 8)] {
        assert!(dark(pixel(&tiled, x, y)), "gap at {x},{y} {:?}", pixel(&tiled, x, y));
    }
}
