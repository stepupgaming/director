use director_core::systems::renderer::render_frame;
use director_html::{compile, CompileOptions};

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

fn options() -> CompileOptions {
    CompileOptions {
        width: 64,
        height: 64,
        duration: 3.0,
        ..Default::default()
    }
}

fn stage(css: &str, body: &str) -> String {
    format!("<style>body{{margin:0}}.stage{{position:relative;width:64px;height:64px}}{css}</style><div class='stage'>{body}</div>")
}

#[test]
fn nested_descendant_paints_the_title_and_not_an_outside_title() {
    let html = stage(
        ".card{background:blue;.title{background:red}}.title{background:green}",
        "<div class='card' style='position:absolute;left:0;top:0;width:32px;height:64px'><div class='title' style='position:absolute;left:0;top:0;width:32px;height:32px'></div></div><div class='title' style='position:absolute;left:32px;top:0;width:32px;height:64px'></div>",
    );
    let p = frame(&html);
    assert_eq!(pixel(&p, 8, 8), [255, 0, 0, 255], "title inside .card");
    assert_eq!(pixel(&p, 8, 48), [0, 0, 255, 255], "parent declaration");
    assert_eq!(pixel(&p, 48, 8), [0, 128, 0, 255], "title outside .card");
}

#[test]
fn ampersand_descendant_matches_the_same_elements() {
    let html = stage(
        ".card{background:blue;& .title{background:red}}.title{background:green}",
        "<div class='card' style='position:absolute;left:0;top:0;width:32px;height:64px'><div class='title' style='position:absolute;left:0;top:0;width:32px;height:32px'></div></div><div class='title' style='position:absolute;left:32px;top:0;width:32px;height:64px'></div>",
    );
    let p = frame(&html);
    assert_eq!(pixel(&p, 8, 8), [255, 0, 0, 255], "&.title inside .card");
    assert_eq!(pixel(&p, 8, 48), [0, 0, 255, 255], "parent declaration");
    assert_eq!(pixel(&p, 48, 8), [0, 128, 0, 255], "title outside .card");
}

#[test]
fn ampersand_compound_matches_the_extra_class() {
    let html = stage(
        ".card{background:blue;&.on{background:red}}.on{background:green}",
        "<div class='card on' style='position:absolute;left:0;top:0;width:64px;height:32px'></div><div class='card' style='position:absolute;left:0;top:32px;width:32px;height:32px'></div><div class='on' style='position:absolute;left:32px;top:32px;width:32px;height:32px'></div>",
    );
    let p = frame(&html);
    assert_eq!(pixel(&p, 16, 8), [255, 0, 0, 255], ".card.on");
    assert_eq!(pixel(&p, 8, 48), [0, 0, 255, 255], ".card without .on");
    assert_eq!(pixel(&p, 48, 48), [0, 128, 0, 255], ".on outside .card");
}

#[test]
fn double_nested_rule_paints_only_the_inner_mark() {
    let html = stage(
        ".card{background:blue;.title{background:green;.mark{background:red}}}.mark{background:white}",
        "<div class='card' style='position:absolute;left:0;top:0;width:32px;height:64px'><div class='title' style='position:absolute;left:0;top:0;width:32px;height:48px'><div class='mark' style='position:absolute;left:0;top:0;width:32px;height:24px'></div></div></div><div class='title' style='position:absolute;left:32px;top:0;width:32px;height:64px'><div class='mark' style='position:absolute;left:0;top:0;width:32px;height:64px'></div></div>",
    );
    let p = frame(&html);
    assert_eq!(pixel(&p, 8, 8), [255, 0, 0, 255], ".card .title .mark");
    assert_eq!(pixel(&p, 8, 36), [0, 128, 0, 255], ".card .title");
    assert_eq!(pixel(&p, 8, 56), [0, 0, 255, 255], ".card");
    assert_eq!(pixel(&p, 48, 8), [255, 255, 255, 255], "mark outside .card");
}

#[test]
fn parent_declaration_after_a_nested_rule_still_applies() {
    let html = stage(
        ".card{background:blue;.title{background:red}background:green}",
        "<div class='card' style='position:absolute;left:0;top:0;width:64px;height:64px'><div class='title' style='position:absolute;left:0;top:0;width:64px;height:32px'></div></div>",
    );
    let p = frame(&html);
    assert_eq!(pixel(&p, 32, 8), [255, 0, 0, 255], "child");
    assert_eq!(pixel(&p, 32, 48), [0, 128, 0, 255], "later parent declaration");
}

#[test]
fn nested_media_and_supports_are_rejected() {
    let media = compile(
        "<style>.card { @media (min-width: 1px) { background: red } }</style><div class='card'></div>",
        options(),
    )
    .err()
    .unwrap()
    .to_string();
    assert_eq!(
        media,
        "CSS nesting of @media requires compile-time extraction"
    );
    let supports = compile(
        "<style>.card { @supports (display: flex) { background: red } }</style><div class='card'></div>",
        options(),
    )
    .err()
    .unwrap()
    .to_string();
    assert_eq!(
        supports,
        "CSS nesting of @supports requires compile-time extraction"
    );
    let top_level = compile(
        "<style>@media (min-width: 1px) { .card { background: red } }</style><div></div>",
        options(),
    )
    .err()
    .unwrap()
    .to_string();
    assert!(
        top_level.contains("unsupported CSS at-rule"),
        "{top_level}"
    );
}
