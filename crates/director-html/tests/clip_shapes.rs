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

fn red_box(clip: &str) -> String {
    format!(
        "<style>body{{margin:0}}div{{width:64px;height:64px;background-color:red;clip-path:{clip}}}</style><div></div>"
    )
}

#[test]
fn circle_clip_keeps_the_center_and_drops_the_corner() {
    let pixels = frame(&red_box("circle(50% at 50% 50%)"));
    let center = pixel(&pixels, 32, 32);
    assert!(center[0] > 200, "{center:?}");
    let corner = pixel(&pixels, 2, 2);
    assert!(
        corner[0] < 20 && corner[1] < 20 && corner[2] < 20,
        "{corner:?}"
    );
}

#[test]
fn inset_clip_keeps_the_center_and_drops_the_outside() {
    let pixels = frame(&red_box("inset(25%)"));
    let center = pixel(&pixels, 32, 32);
    assert!(center[0] > 200, "{center:?}");
    let outside = pixel(&pixels, 4, 4);
    assert!(
        outside[0] < 20 && outside[1] < 20 && outside[2] < 20,
        "{outside:?}"
    );
}

#[test]
fn clip_path_url_is_rejected() {
    let err = compile(
        &red_box("url(#x)"),
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
    assert!(
        err.contains("only polygon") || err.contains("clip-path"),
        "{err}"
    );
}
