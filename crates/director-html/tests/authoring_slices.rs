use director_core::systems::renderer::render_frame;
use director_html::{compile, CompileOptions};
use std::path::PathBuf;

fn example(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join("html")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

fn options() -> CompileOptions {
    CompileOptions {
        width: 64,
        height: 64,
        duration: 3.0,
        ..Default::default()
    }
}

fn pixel(pixels: &[u8], x: usize, y: usize) -> &[u8] {
    &pixels[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4]
}

#[test]
fn authoring_slices_compile() {
    for name in [
        "image-background.html",
        "text-decoration.html",
        "nested-rule.html",
    ] {
        compile(&example(name), options()).unwrap_or_else(|err| panic!("{name}: {err}"));
    }
}

#[test]
fn image_background_paints_png_and_uncovered_color() {
    let mut compiled = compile(&example("image-background.html"), options())
        .unwrap_or_else(|err| panic!("image-background.html: {err}"));
    let mut surface = skia_safe::surfaces::raster_n32_premul((64, 64)).unwrap();
    render_frame(&mut compiled.director, 0.0, surface.canvas()).unwrap();
    let info = skia_safe::ImageInfo::new(
        (64, 64),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let mut pixels = vec![0; 64 * 64 * 4];
    assert!(surface.read_pixels(&info, &mut pixels, 64 * 4, (0, 0)));
    let ink = pixel(&pixels, 3, 3);
    assert!(ink[0] > 200 && ink[1] < 30 && ink[2] < 30, "{ink:?}");
    let uncovered = pixel(&pixels, 63, 63);
    assert!(
        uncovered[2] > 200 && uncovered[0] < 30 && uncovered[1] < 30,
        "{uncovered:?}"
    );
}
