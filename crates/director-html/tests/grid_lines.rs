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
fn side_line_paints_the_second_column() {
    let html = "<style>body{margin:0}.grid{display:grid;width:64px;height:64px;grid-template-columns:[main] 32px [side] 32px}.cell{grid-column:side;background:red}</style><div class='grid'><div class='cell'></div></div>";
    let pixels = frame(html, 0.0);
    assert_eq!(pixel(&pixels, 48, 32), &[255, 0, 0, 255]);
    let dark = pixel(&pixels, 8, 32);
    assert!(dark[0] < 20 && dark[1] < 20 && dark[2] < 20, "{dark:?}");
}

#[test]
fn subgrid_places_the_grandchild_on_the_parent_line() {
    let html = "<style>body{margin:0}.grid{display:grid;width:64px;height:64px;grid-template-columns:[main] 32px [side] 32px}.sub{display:grid;grid-column:span 2;width:64px;height:64px;grid-template-columns:subgrid;grid-template-rows:64px}.cell{grid-column:side;background:red}</style><div class='grid'><div class='sub'><div class='cell'></div></div></div>";
    let pixels = frame(html, 0.0);
    assert_eq!(pixel(&pixels, 48, 32), &[255, 0, 0, 255]);
    let dark = pixel(&pixels, 8, 32);
    assert!(dark[0] < 20 && dark[1] < 20 && dark[2] < 20, "{dark:?}");
}
