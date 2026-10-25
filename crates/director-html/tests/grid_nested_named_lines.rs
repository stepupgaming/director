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
fn sibling_after_nested_grid_uses_the_outer_line() {
    let html = "<style>body{margin:0}.outer{display:grid;width:64px;height:64px;grid-template-columns:[alpha] 32px [beta] 32px;grid-template-rows:32px 32px}.nested{display:grid;grid-column:span 2;grid-row:1;grid-template-columns:[beta] 32px [gamma] 32px}.mark{grid-column:beta;background:blue}.sib{grid-column:beta;grid-row:2;background:red}</style><div class='outer'><div class='nested'><div class='mark'></div></div><div class='sib'></div></div>";
    let pixels = frame(html, 0.0);
    assert_eq!(pixel(&pixels, 8, 16), &[0, 0, 255, 255]);
    let top_right = pixel(&pixels, 48, 16);
    assert!(
        top_right[2] < 20,
        "nested line leaked into the outer right column: {top_right:?}"
    );
    assert_eq!(pixel(&pixels, 48, 48), &[255, 0, 0, 255]);
    let bottom_left = pixel(&pixels, 8, 48);
    assert!(
        bottom_left[0] < 20 && bottom_left[1] < 20 && bottom_left[2] < 20,
        "sibling resolved beta on the nested grid: {bottom_left:?}"
    );
}
