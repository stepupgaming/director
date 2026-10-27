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
    // At 0.5 s the box covers x 32..40. A 360 degree shutter at 10 fps spans 0.1 s, 6.4 px.
    let sharp = row(&source(""), 0.5);
    let blurred = row(&source("data-motion-blur='8' data-shutter-angle='360'"), 0.5);
    assert_eq!(sharp[30], 0);
    assert_eq!(sharp[36], 255);
    assert!(blurred[30] > 0 && blurred[30] < 255, "leading smear {}", blurred[30]);
    assert!(blurred[41] > 0 && blurred[41] < 255, "trailing smear {}", blurred[41]);
    assert!(blurred[36] > 200, "core stays bright {}", blurred[36]);
}

#[test]
fn invalid_motion_blur_is_refused() {
    for attrs in ["data-motion-blur='0'", "data-motion-blur='many'", "data-shutter-angle='400'"] {
        let error = compile(&source(attrs), CompileOptions::default()).err().expect(attrs).to_string();
        assert!(error.contains("data-motion-blur") || error.contains("data-shutter-angle"), "{error}");
    }
}
