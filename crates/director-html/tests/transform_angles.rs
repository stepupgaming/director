use director_render::render_frame;
use director_html::{compile, CompileOptions};

fn frame(source: &str) -> Vec<u8> {
    let mut c = compile(
        source,
        CompileOptions {
            width: 64,
            height: 64,
            duration: 1.0,
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

const RED: [u8; 4] = [255, 0, 0, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];

// A bar standing up from the centre, turned about its foot.
fn bar(transform: &str) -> Vec<u8> {
    frame(&format!(
        "<style>body{{margin:0}}div{{position:absolute;left:24px;top:0;width:16px;height:32px;background:red;transform-origin:8px 32px;transform:{transform}}}</style><div></div>"
    ))
}

#[test]
fn negative_rotation_turns_the_box() {
    for transform in ["rotate(-90deg)", "rotate(270deg)", "rotate(630deg)", "rotateZ(-90deg)"] {
        let pixels = bar(transform);
        assert_eq!(pixel(&pixels, 8, 32), RED, "{transform}");
        assert_eq!(pixel(&pixels, 32, 8), BLACK, "{transform}");
    }
}

#[test]
fn negative_rotation_keeps_the_rest_of_the_list() {
    let pixels = bar("translateY(16px) rotate(-90deg)");
    assert_eq!(pixel(&pixels, 8, 48), RED);
    assert_eq!(pixel(&pixels, 8, 32), BLACK);
}

#[test]
fn negative_skew_leans_the_box() {
    for transform in ["skewX(-45deg)", "skew(-45deg)", "skew(-45deg, 0deg)"] {
        let pixels = frame(&format!(
            "<style>body{{margin:0}}div{{width:64px;height:64px;background:red;transform:{transform}}}</style><div></div>"
        ));
        assert_eq!(pixel(&pixels, 2, 2), BLACK, "{transform}");
        assert_eq!(pixel(&pixels, 61, 2), RED, "{transform}");
    }
}
