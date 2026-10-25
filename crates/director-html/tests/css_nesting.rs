use director_render::render_frame;
use director_html::{compile, CompileOptions};

fn frame(source: &str) -> Vec<u8> {
    let mut c = compile(source, options()).unwrap();
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
fn nested_media_screen_paints_red() {
    let html = stage(
        ".card{background:blue;@media screen{color:red;background:red}}",
        "<div class='card' style='position:absolute;left:0;top:0;width:64px;height:64px'></div>",
    );
    let p = frame(&html);
    assert_eq!(pixel(&p, 8, 8), [255, 0, 0, 255], "nested @media screen");
}

#[test]
fn nested_media_screen_contributes_a_nested_rule() {
    let html = stage(
        ".card{background:blue;@media screen{.title{background:red}}}.title{background:green}",
        "<div class='card' style='position:absolute;left:0;top:0;width:32px;height:64px'><div class='title' style='position:absolute;left:0;top:0;width:32px;height:32px'></div></div><div class='title' style='position:absolute;left:32px;top:0;width:32px;height:64px'></div>",
    );
    let p = frame(&html);
    assert_eq!(pixel(&p, 8, 8), [255, 0, 0, 255], ".card .title inside @media screen");
    assert_eq!(pixel(&p, 8, 48), [0, 0, 255, 255], ".card");
    assert_eq!(pixel(&p, 48, 8), [0, 128, 0, 255], "title outside .card");
}

#[test]
fn nested_media_print_does_not_apply() {
    let html = stage(
        ".card{background:blue;@media print{color:red;background:red;.title{background:white}}}",
        "<div class='card' style='position:absolute;left:0;top:0;width:64px;height:64px'><div class='title' style='position:absolute;left:0;top:0;width:32px;height:32px'></div></div>",
    );
    let p = frame(&html);
    assert_eq!(
        pixel(&p, 48, 48),
        [0, 0, 255, 255],
        "false media block does not contribute a declaration"
    );
    assert_eq!(pixel(&p, 8, 8), [0, 0, 255, 255], "false media nested rule");
}

#[test]
fn nested_min_width_matches_compile_width_and_max_width_zero_does_not() {
    let html = stage(
        ".min{background:blue;@media (min-width: 1px){background:red}}.fit{background:blue;@media (min-width: 64px){background:red}}.over{background:blue;@media (min-width: 65px){background:red}}.max{background:blue;@media (max-width: 0px){background:red}}",
        "<div class='min' style='position:absolute;left:0;top:0;width:32px;height:32px'></div><div class='fit' style='position:absolute;left:32px;top:0;width:32px;height:32px'></div><div class='over' style='position:absolute;left:0;top:32px;width:32px;height:32px'></div><div class='max' style='position:absolute;left:32px;top:32px;width:32px;height:32px'></div>",
    );
    let p = frame(&html);
    assert_eq!(pixel(&p, 8, 8), [255, 0, 0, 255], "min-width 1px");
    assert_eq!(pixel(&p, 48, 8), [255, 0, 0, 255], "min-width equals compile width");
    assert_eq!(pixel(&p, 8, 48), [0, 0, 255, 255], "min-width above compile width");
    assert_eq!(pixel(&p, 48, 48), [0, 0, 255, 255], "max-width 0px");
}

#[test]
fn nested_media_all_and_height_follow_the_compile_size() {
    let html = stage(
        ".all{background:blue;@media all{background:red}}.tall{background:blue;@media (min-height: 1px){background:red}}.short{background:blue;@media (max-height: 0px){background:red}}",
        "<div class='all' style='position:absolute;left:0;top:0;width:32px;height:64px'></div><div class='tall' style='position:absolute;left:32px;top:0;width:16px;height:32px'></div><div class='short' style='position:absolute;left:48px;top:0;width:16px;height:32px'></div>",
    );
    let p = frame(&html);
    assert_eq!(pixel(&p, 8, 8), [255, 0, 0, 255], "@media all");
    assert_eq!(pixel(&p, 40, 8), [255, 0, 0, 255], "min-height 1px");
    assert_eq!(pixel(&p, 56, 8), [0, 0, 255, 255], "max-height 0px");
}

#[test]
fn nested_supports_grid_and_flex_paint_red() {
    let html = stage(
        ".grid{background:blue;@supports (display: grid){background:red}}.flex{background:blue;@supports (display: flex){background:red}}.no{background:blue;@supports not (display: flex){background:red}}",
        "<div class='grid' style='position:absolute;left:0;top:0;width:32px;height:32px'></div><div class='flex' style='position:absolute;left:32px;top:0;width:32px;height:32px'></div><div class='no' style='position:absolute;left:0;top:32px;width:64px;height:32px'></div>",
    );
    let p = frame(&html);
    assert_eq!(pixel(&p, 8, 8), [255, 0, 0, 255], "display: grid");
    assert_eq!(pixel(&p, 48, 8), [255, 0, 0, 255], "display: flex");
    assert_eq!(pixel(&p, 8, 48), [0, 0, 255, 255], "not (display: flex)");
}

#[test]
fn unknown_nested_conditions_fail_and_top_level_media_stays_unsupported() {
    for source in [
        ".card { @media (hover: hover) { background: red } }",
        ".card { @media speech { background: red } }",
        ".card { @media screen and (prefers-color-scheme: dark) { background: red } }",
    ] {
        let err = compile(
            &format!("<style>{source}</style><div class='card'></div>"),
            options(),
        )
        .err()
        .unwrap()
        .to_string();
        assert!(err.contains("unsupported"), "{err}");
    }
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
