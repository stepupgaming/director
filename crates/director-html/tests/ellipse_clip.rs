use director_core::systems::renderer::render_frame;
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

fn red_box(clip: &str) -> String {
    format!(
        "<style>body{{margin:0}}div{{width:64px;height:64px;background-color:red;clip-path:{clip}}}</style><div></div>"
    )
}

fn options() -> CompileOptions {
    CompileOptions {
        width: 64,
        height: 64,
        duration: 3.0,
        ..Default::default()
    }
}

#[test]
fn ellipse_clip_keeps_the_center_and_drops_the_corner() {
    let pixels = frame(&red_box("ellipse(25% 50% at 50% 50%)"));
    let center = pixel(&pixels, 32, 32);
    assert!(center[0] > 200, "{center:?}");
    let corner = pixel(&pixels, 2, 2);
    assert!(
        corner[0] < 20 && corner[1] < 20 && corner[2] < 20,
        "{corner:?}"
    );
}

#[test]
fn ellipse_default_matches_closest_side_at_center() {
    let bare = frame(&red_box("ellipse()"));
    let named = frame(&red_box("ellipse(closest-side closest-side at center)"));
    assert_eq!(bare, named);
    let center = pixel(&bare, 32, 32);
    assert!(center[0] > 200, "{center:?}");
    let corner = pixel(&bare, 2, 2);
    assert!(
        corner[0] < 20 && corner[1] < 20 && corner[2] < 20,
        "{corner:?}"
    );
}

#[test]
fn clip_path_url_is_rejected() {
    let err = compile(&red_box("url(#x)"), options())
        .err()
        .unwrap()
        .to_string();
    assert!(
        err.contains("only polygon") || err.contains("clip-path"),
        "{err}"
    );
}
