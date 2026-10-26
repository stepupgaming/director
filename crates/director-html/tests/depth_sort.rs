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
fn preserve_3d_paints_the_closer_sibling_last() {
    let source = |transform_style| {
        format!(
            "<style>body{{margin:0}}.stage{{position:relative;width:64px;height:64px;perspective:200px;transform-style:{transform_style}}}.card{{position:absolute;top:0;right:0;bottom:0;left:0}}.near{{background:red;transform:translateZ(40px)}}.far{{background:blue;transform:translateZ(-40px)}}</style><div class='stage'><div class='card near'></div><div class='card far'></div></div>"
        )
    };
    assert_eq!(
        pixel(&frame(&source("preserve-3d"), 0.0), 32, 32),
        [255, 0, 0, 255]
    );
    assert_eq!(pixel(&frame(&source("flat"), 0.0), 32, 32), [0, 0, 255, 255]);
}

#[test]
fn preserve_3d_z_index_breaks_a_depth_tie() {
    let html = "<style>body{margin:0}.stage{position:relative;width:64px;height:64px;perspective:200px;transform-style:preserve-3d}.card{position:absolute;top:0;right:0;bottom:0;left:0;transform:translateZ(0px)}.near{background:red}.far{background:blue;z-index:-1}</style><div class='stage'><div class='card near'></div><div class='card far'></div></div>";
    assert_eq!(pixel(&frame(html, 0.0), 32, 32), [255, 0, 0, 255]);
}

#[test]
fn hidden_backface_stays_unpainted() {
    let hidden = "<style>body{margin:0}div{width:64px;height:64px;background:red;transform:rotateY(180deg);backface-visibility:hidden}</style><div></div>";
    assert_eq!(pixel(&frame(hidden, 0.0), 32, 32), [0, 0, 0, 255]);
}

#[test]
fn backface_follows_the_preserve_3d_ancestor() {
    let source = |turn: &str| {
        format!("<style>body{{margin:0}}.stage{{width:64px;height:64px;transform-style:preserve-3d;transform:rotateY({turn})}}.card{{width:64px;height:64px;background:red;backface-visibility:hidden}}</style><div class='stage'><div class='card'></div></div>")
    };
    assert_eq!(pixel(&frame(&source("180deg"), 0.0), 32, 32), [0, 0, 0, 255]);
    assert_eq!(pixel(&frame(&source("0deg"), 0.0), 32, 32), [255, 0, 0, 255]);
    let unturned = "<style>body{margin:0}.stage{width:64px;height:64px;transform-style:preserve-3d;transform:rotateY(180deg)}.card{width:64px;height:64px;background:red;backface-visibility:hidden;transform:rotateY(180deg)}</style><div class='stage'><div class='card'></div></div>";
    assert_eq!(pixel(&frame(unturned, 0.0), 32, 32), [255, 0, 0, 255]);
}

#[test]
fn mirrored_card_keeps_its_front_face() {
    let html = "<style>body{margin:0}div{width:64px;height:64px;background:red;transform:scaleX(-1);backface-visibility:hidden}</style><div></div>";
    assert_eq!(pixel(&frame(html, 0.0), 32, 32), [255, 0, 0, 255]);
}

#[test]
fn flat_origin_z_stays_on_the_card() {
    let html = "<style>body{margin:0}div{width:64px;height:64px;background:red;transform-origin:50% 50% 40px}</style><div></div>";
    let pixels = frame(html, 0.0);
    assert_eq!(pixel(&pixels, 2, 2), [255, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 62, 62), [255, 0, 0, 255]);
}

#[test]
fn transform_origin_z_changes_which_sibling_is_in_front() {
    let source = |z: &str| {
        format!(
            "<style>body{{margin:0}}.stage{{position:relative;width:64px;height:64px;perspective:200px;transform-style:preserve-3d}}.card{{position:absolute;top:0;right:0;bottom:0;left:0}}.back{{background:blue;transform:translateZ(40px)}}.spin{{background:red;transform:rotateY(180deg);transform-origin:50% 50% {z}}}</style><div class='stage'><div class='card back'></div><div class='card spin'></div></div>"
        )
    };
    assert_eq!(pixel(&frame(&source("0px"), 0.0), 32, 32), [0, 0, 255, 255]);
    assert_eq!(pixel(&frame(&source("30px"), 0.0), 32, 32), [255, 0, 0, 255]);
}

#[test]
fn percent_transform_origin_z_still_fails() {
    let error = compile(
        "<div style='transform-origin:50% 50% 10%'></div>",
        CompileOptions::default(),
    )
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains("transform-origin"), "{error}");
}

#[test]
fn crossing_rotate_x_splits_the_overlap() {
    let html = "<style>body{margin:0}.stage{position:relative;width:64px;height:64px;transform-style:preserve-3d}.card{position:absolute;top:0;right:0;bottom:0;left:0}.red{background:red;transform:rotateX(60deg)}.blue{background:blue;transform:rotateX(-60deg)}</style><div class='stage'><div class='card red'></div><div class='card blue'></div></div>";
    let pixels = frame(html, 0.0);
    assert_eq!(pixel(&pixels, 32, 20), [0, 0, 255, 255]);
    assert_eq!(pixel(&pixels, 32, 44), [255, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 32, 8), [0, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 32, 56), [0, 0, 0, 255]);
}

#[test]
fn perspective_origin_z_still_fails() {
    let error = compile(
        "<div style='perspective-origin:50% 50% 10px'></div>",
        CompileOptions::default(),
    )
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains("perspective-origin"), "{error}");
}
