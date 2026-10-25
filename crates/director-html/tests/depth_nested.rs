use director_render::render_frame;
use director_html::{compile, CompileOptions};

fn frame(source: &str, time: f64) -> Vec<u8> {
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
    render_frame(&mut c.director, time, &mut surface).unwrap();
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

#[test]
fn nested_preserve_3d_paints_the_grandchild_over_the_uncle() {
    let html = "<style>body{margin:0}.stage{position:relative;width:64px;height:64px;transform-style:preserve-3d}.uncle{position:absolute;top:0;right:0;bottom:0;left:0;background:red;transform:translateZ(10px)}.group{position:absolute;top:0;right:0;bottom:0;left:0;transform-style:preserve-3d}.near{position:absolute;top:0;right:0;bottom:0;left:0;background:blue;transform:translateZ(100px)}</style><div class='stage'><div class='uncle'></div><div class='group'><div class='near'></div></div></div>";
    assert_eq!(pixel(&frame(html, 0.0), 32, 32), [0, 0, 255, 255]);
}
