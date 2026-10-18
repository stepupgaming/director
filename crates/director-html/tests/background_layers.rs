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
    sample[0] > 200 && sample[1] < 40 && sample[2] < 40
}

fn blue(sample: &[u8]) -> bool {
    sample[2] > 200 && sample[0] < 40 && sample[1] < 40
}

fn dark(sample: &[u8]) -> bool {
    sample[0] < 20 && sample[1] < 20 && sample[2] < 20
}

#[test]
fn layers_use_their_own_size_and_position() {
    let pixels = frame(
        "<style>body{margin:0}div{width:64px;height:64px;background-image:linear-gradient(red,red),linear-gradient(blue,blue);background-repeat:no-repeat;background-size:20px 20px,40px 12px;background-position:8px 8px,0px 16px}</style><div></div>",
    );
    let red_only = pixel(&pixels, 18, 12);
    assert!(red(red_only), "first layer tile {red_only:?}");
    let overlap = pixel(&pixels, 18, 22);
    assert!(red(overlap), "first layer stays on top {overlap:?}");
    let blue_only = pixel(&pixels, 34, 22);
    assert!(blue(blue_only), "second layer tile {blue_only:?}");
    let outside = pixel(&pixels, 50, 50);
    assert!(dark(outside), "outside both tiles {outside:?}");
}

#[test]
fn shorter_background_size_cycles_across_layers() {
    let pixels = frame(
        "<style>body{margin:0}div{width:64px;height:64px;background-image:linear-gradient(red,red),linear-gradient(blue,blue);background-repeat:no-repeat;background-size:8px 64px;background-position:0px 0px,32px 0px}</style><div></div>",
    );
    let first = pixel(&pixels, 4, 32);
    assert!(red(first), "first layer used the single size {first:?}");
    let second = pixel(&pixels, 36, 32);
    assert!(blue(second), "second layer used the cycled size {second:?}");
    let gap = pixel(&pixels, 16, 32);
    assert!(dark(gap), "gap between the 8px tiles {gap:?}");
    let past = pixel(&pixels, 50, 32);
    assert!(dark(past), "cycled size did not fill the box {past:?}");
}

#[test]
fn second_background_url_stays_an_error() {
    let err = compile(
        &format!(
            "<style>div{{background-image:url(\"data:image/png;base64,{RED_PNG}\"),url(\"data:image/png;base64,{RED_PNG}\")}}</style><div></div>"
        ),
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
    assert!(err.contains("background-image accepts a single url"), "{err}");
}

#[test]
fn none_background_layer_is_skipped() {
    let leading = frame(
        "<style>body{margin:0}div{width:64px;height:64px;background-image:none,linear-gradient(red,red)}</style><div></div>",
    );
    assert!(red(pixel(&leading, 32, 32)), "{:?}", pixel(&leading, 32, 32));
    let trailing = frame(
        "<style>body{margin:0}div{width:64px;height:64px;background-image:linear-gradient(red,red),none}</style><div></div>",
    );
    assert!(
        red(pixel(&trailing, 32, 32)),
        "{:?}",
        pixel(&trailing, 32, 32)
    );
    let only_none = frame(
        "<style>body{margin:0}div{width:64px;height:64px;background-image:none}</style><div></div>",
    );
    assert!(
        dark(pixel(&only_none, 32, 32)),
        "{:?}",
        pixel(&only_none, 32, 32)
    );
}
