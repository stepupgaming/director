use director_core::systems::renderer::render_frame;
use director_html::{compile, CompileOptions};

const RED_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAYAAADED76LAAAAEklEQVR4nGP4z8DwHx9mGBkKAMLXf4EvceABAAAAAElFTkSuQmCC";

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
    render_frame(&mut c.director, 0.0, surface.canvas()).unwrap();
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
    sample[0] > 200 && sample[1] < 30 && sample[2] < 30
}

fn dark(sample: &[u8]) -> bool {
    sample[0] < 20 && sample[1] < 20 && sample[2] < 20
}

#[test]
fn background_position_offsets_the_tile() {
    let source = format!(
        "<style>body{{margin:0}}div{{width:64px;height:64px;background-image:url(\"data:image/png;base64,{RED_PNG}\");background-repeat:no-repeat;background-size:8px 8px;background-position:16px 8px}}</style><div></div>"
    );
    let pixels = frame(&source);
    let origin = pixel(&pixels, 16, 8);
    assert!(red(origin), "tile origin {origin:?}");
    let inside = pixel(&pixels, 20, 12);
    assert!(red(inside), "tile interior {inside:?}");
    let corner = pixel(&pixels, 0, 0);
    assert!(dark(corner), "page origin {corner:?}");
}
