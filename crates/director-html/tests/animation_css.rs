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

fn compile_error(source: &str) -> String {
    compile(source, options()).err().unwrap().to_string()
}

fn fade(extra: &str) -> String {
    format!(
        "<style>body{{margin:0}}div{{width:64px;height:64px;background:red;animation-name:fade;animation-duration:1s;animation-timing-function:linear;{extra}}}@keyframes fade{{from{{opacity:0}}to{{opacity:1}}}}</style><div></div>"
    )
}

fn frame(source: &str, time: f64) -> Vec<u8> {
    let mut c = compile(source, options()).unwrap();
    let mut surface = skia_safe::surfaces::raster_n32_premul((64, 64)).unwrap();
    render_frame(&mut c.director, time, surface.canvas()).unwrap();
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
fn direction_waffle_is_rejected() {
    let err = compile_error(&fade("animation-direction:waffle"));
    assert!(err.contains("unsupported animation direction"), "{err}");
}

#[test]
fn fill_waffle_is_rejected() {
    let err = compile_error(&fade("animation-fill-mode:waffle"));
    assert!(err.contains("unsupported fill mode"), "{err}");
}

#[test]
fn reverse_compiles() {
    compile(&fade("animation-direction:reverse"), options()).unwrap();
}

#[test]
fn forwards_compiles() {
    compile(&fade("animation-fill-mode:forwards"), options()).unwrap();
}

#[test]
fn accepted_directions_and_fills_compile() {
    for direction in ["normal", "reverse", "alternate", "alternate-reverse"] {
        compile(
            &fade(&format!("animation-direction:{direction}")),
            options(),
        )
        .unwrap();
    }
    for fill in ["none", "forwards", "backwards", "both"] {
        compile(&fade(&format!("animation-fill-mode:{fill}")), options()).unwrap();
    }
}

#[test]
fn reverse_width_is_near_the_end_just_after_zero() {
    let source = |direction: &str| {
        format!(
            "<style>body{{margin:0}}div{{height:64px;background:red;animation-name:grow;animation-duration:1s;animation-timing-function:linear;animation-direction:{direction}}}@keyframes grow{{from{{width:0px}}to{{width:100px}}}}</style><div></div>"
        )
    };
    let normal = frame(&source("normal"), 0.05);
    let reverse = frame(&source("reverse"), 0.05);
    let normal_mid = pixel(&normal, 32, 32);
    let reverse_mid = pixel(&reverse, 32, 32);
    assert!(
        normal_mid[0] < 20,
        "normal width should still be near 0px, got {normal_mid:?}"
    );
    assert!(
        reverse_mid[0] > 200 && reverse_mid[1] < 30 && reverse_mid[2] < 30,
        "reverse width should be near 100px, got {reverse_mid:?}"
    );
}

#[test]
fn jump_start_and_jump_end_still_compile() {
    for position in ["start", "jump-start", "end", "jump-end"] {
        compile(
            &fade(&format!("animation-timing-function:steps(2,{position})")),
            options(),
        )
        .unwrap();
    }
}

#[test]
fn jump_none_and_jump_both_stay_rejected() {
    for position in ["jump-none", "jump-both"] {
        let err = compile_error(&fade(&format!(
            "animation-timing-function:steps(2,{position})"
        )));
        assert!(
            err.contains("only jump-start/jump-end steps supported"),
            "{position}: {err}"
        );
    }
}
