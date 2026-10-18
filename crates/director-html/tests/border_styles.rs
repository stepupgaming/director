use director_core::systems::renderer::render_frame;
use director_html::{compile, CompileOptions};

fn options() -> CompileOptions {
    CompileOptions {
        width: 64,
        height: 64,
        duration: 3.0,
        ..Default::default()
    }
}

fn document(decl: &str) -> String {
    format!(
        "<style>body{{margin:0}}div{{width:64px;height:64px;box-sizing:border-box;{decl}}}</style><div></div>"
    )
}

fn frame(source: &str) -> Vec<u8> {
    let mut c = compile(source, options()).unwrap();
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

#[test]
fn double_border_paints_two_stripes_and_a_gap() {
    // 8px double is ceil(8/3)=3 outer, 2px gap, 3px inner. Rows 3 and 4 stay empty.
    let pixels = frame(&document("border:8px double red"));
    let ink = pixel(&pixels, 2, 2);
    assert!(ink[0] > 200, "outer stripe {ink:?}");
    let gap = pixel(&pixels, 32, 4);
    assert!(
        gap[0] < 30 && gap[1] < 30 && gap[2] < 30,
        "gap at y=4 {gap:?}"
    );
    let inner = pixel(&pixels, 32, 6);
    assert!(inner[0] > 200, "inner stripe {inner:?}");
}

#[test]
fn groove_compiles_and_wavy_errors() {
    for style in ["groove", "ridge", "inset", "outset", "double", "hidden"] {
        compile(&document(&format!("border-style:{style}")), options())
            .unwrap_or_else(|e| panic!("{style} should compile: {e}"));
    }
    let err = compile(&document("border-style:wavy"), options());
    assert!(err.is_err(), "wavy border style should fail");
}

#[test]
fn hidden_border_matches_none() {
    let hidden = frame(&document("border:8px hidden red"));
    let none = frame(&document("border:8px none red"));
    assert_eq!(pixel(&hidden, 2, 2), pixel(&none, 2, 2));
    assert!(pixel(&hidden, 2, 2)[0] < 30, "{:?}", pixel(&hidden, 2, 2));
}
