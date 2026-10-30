use director_html::{compile, CompileOptions};
use director_render::render_frame;

// A white 8px box crossing a 64px black canvas at 64 px/s, sampled at 10 fps.
fn source(root_attrs: &str) -> String {
    format!(
        "<style>body{{margin:0}}#r{{position:relative;width:64px;height:64px;background:black}}.box{{position:absolute;left:0;top:0;width:8px;height:64px;background:white;animation:go 1s linear both}}@keyframes go{{from{{transform:translateX(0px)}}to{{transform:translateX(64px)}}}}</style><div id='r' data-composition-id='m' data-width='64' data-height='64' data-fps='10' data-duration='1' {root_attrs}><div class='box'></div></div>"
    )
}

fn row(source: &str, time: f64) -> Vec<u8> {
    let mut c = compile(source, CompileOptions::default()).unwrap();
    let mut surface = skia_safe::surfaces::raster_n32_premul((64, 64)).unwrap();
    render_frame(&mut c.director, time, &mut surface).unwrap();
    let info = skia_safe::ImageInfo::new((64, 1), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Unpremul, None);
    let mut pixels = vec![0; 64 * 4];
    assert!(surface.read_pixels(&info, &mut pixels, 64 * 4, (0, 32)));
    pixels.chunks(4).map(|p| p[0]).collect()
}

#[test]
fn root_attributes_set_sub_frame_sampling() {
    let c = compile(&source("data-motion-blur='6' data-shutter-angle='270'"), CompileOptions::default()).unwrap();
    assert_eq!(c.director.samples_per_frame, 6);
    assert_eq!(c.director.shutter_angle, 270.0);
    let plain = compile(&source(""), CompileOptions::default()).unwrap();
    assert_eq!(plain.director.samples_per_frame, 1);
}

#[test]
fn motion_blur_smears_a_moving_edge() {
    // At 0.5 s the box covers x 32..40. A 360 degree shutter at 10 fps opens
    // at the frame and spans 0.1 s, 6.4 px to the right.
    let sharp = row(&source(""), 0.5);
    let blurred = row(&source("data-motion-blur='8' data-shutter-angle='360'"), 0.5);
    assert_eq!(sharp[30], 0);
    assert_eq!(sharp[36], 255);
    assert_eq!(blurred[30], 0, "nothing before the frame's position");
    assert!(blurred[34] > 0 && blurred[34] < 255, "left smear {}", blurred[34]);
    assert!(blurred[43] > 0 && blurred[43] < 255, "right smear {}", blurred[43]);
    assert!(blurred[39] > 200, "core stays bright {}", blurred[39]);
}

#[test]
fn a_still_shutter_matches_the_sharp_frame() {
    // The box stops at x 32..40.
    let held = |attrs: &str, animation: &str| {
        let source = source(attrs).replace("data-duration='1'", "data-duration='2'");
        source.replace("1s linear both", animation).replace("translateX(64px)", "translateX(32px)")
    };
    let blur = "data-motion-blur='8' data-shutter-angle='360'";
    // After the box stops, the shutter paints one sample.
    let still = row(&held(blur, "1s linear both"), 1.5);
    assert_eq!(still, row(&held("", "1s linear both"), 1.5));
    assert_eq!((still[31], still[36], still[41]), (0, 255, 0));
    // An infinite loop keeps moving.
    let looped = row(&held(blur, "1s linear infinite"), 1.5);
    assert_ne!(looped, row(&held("", "1s linear infinite"), 1.5));
}

#[test]
fn invalid_motion_blur_is_refused() {
    for attrs in ["data-motion-blur='0'", "data-motion-blur='many'", "data-shutter-angle='400'"] {
        let error = compile(&source(attrs), CompileOptions::default()).err().expect(attrs).to_string();
        assert!(error.contains("data-motion-blur") || error.contains("data-shutter-angle"), "{error}");
    }
}

/// The furthest any node of a 64 px SVG moves its paint inside its box over
/// `[t0, t1]`, as motion blur measures it.
fn svg_travel(markup: &str, t0: f64, t1: f64) -> Option<f32> {
    travel(&format!("<svg width='64' height='64' viewBox='0 0 64 64'>{markup}</svg>"), t0, t1)
}

/// The furthest any node of a 64 px composition holding `body` moves its
/// paint inside its box over `[t0, t1]`. `None` asks for every sample.
fn travel(body: &str, t0: f64, t1: f64) -> Option<f32> {
    let source = format!("<div id='r' data-composition-id='m' data-width='64' data-height='64' data-fps='10' data-duration='2'>{body}</div>");
    let c = compile(&source, CompileOptions::default()).unwrap();
    let travel = c.director.scene.iter_alive().try_fold(0.0_f32, |most, (_, node)| Some(most.max(node.element.paint_travel(t0, t1, (64.0, 64.0))?)));
    travel
}

#[test]
fn svg_paint_travel_measures_what_moves_inside_the_box() {
    let x = "<rect width='12' height='32'><animate attributeName='x' from='0' to='40' dur='2s' fill='freeze'/></rect>";
    let travel = svg_travel(x, 1.0, 1.05).unwrap();
    assert!((travel - 1.0).abs() < 0.01, "40 px over 2 s moves 1 px in 0.05 s, got {travel}");
    let fade = "<rect width='12' height='32'><animate attributeName='opacity' from='0' to='1' dur='2s'/></rect>";
    assert_eq!(svg_travel(fade, 1.0, 1.05), Some(0.0), "a fade moves nothing");
    let slide = "<rect width='12' height='32'><animateTransform attributeName='transform' type='translate' from='0 0' to='40 0' dur='2s'/></rect>";
    let travel = svg_travel(slide, 1.0, 1.05).unwrap();
    assert!((travel - 1.0).abs() < 0.01, "a translate moves its corners 1 px, got {travel}");
    let draw = "<path d='M0 0 L64 0' stroke='white' stroke-dasharray='64'><animate attributeName='stroke-dashoffset' from='64' to='0' dur='2s'/></path>";
    let travel = svg_travel(draw, 1.0, 1.05).unwrap();
    assert!((travel - 1.6).abs() < 0.01, "the stroke end draws on 1.6 px, got {travel}");
}

#[test]
fn a_shade_that_steps_inside_the_shutter_takes_every_sample() {
    let style = |animation: &str| format!("<style>@keyframes f{{from{{opacity:0}}to{{opacity:1}}}}.b{{width:8px;height:8px;background:white;animation:{animation}}}</style><div class='b'></div>");
    assert_eq!(travel(&style("f 2s linear both"), 1.0, 1.05), Some(0.0), "a linear fade moves nothing");
    assert_eq!(travel(&style("f 2s steps(1) both"), 0.98, 1.03), Some(0.0), "a step outside the shutter holds");
    assert_eq!(travel(&style("f 2s steps(2) both"), 0.98, 1.03), None, "a step inside the shutter flashes");
}
