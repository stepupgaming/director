use director_core::systems::renderer::render_frame;
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
fn css_text_line_boxes_preserve_fractional_height_and_normal_font_metrics() {
    let c = compile("<style>#explicit{font-size:104px;line-height:1.1}#normal{font-size:36px}</style><p id='explicit'>Director</p><p id='normal'>Director</p>", Default::default()).unwrap();
    for (id, expected) in [("explicit", 114.4), ("normal", 48.0)] {
        let node = c.director.scene.get_node(c.element_ids[id]).unwrap();
        let size = node.element.measure(
            taffy::geometry::Size {
                width: None,
                height: None,
            },
            taffy::geometry::Size {
                width: taffy::style::AvailableSpace::MaxContent,
                height: taffy::style::AvailableSpace::MaxContent,
            },
        );
        assert!(
            (size.height - expected).abs() < 0.001,
            "{id}: {}",
            size.height
        );
    }
}

#[test]
fn selector_cascade_inheritance_and_important() {
    let html="<style>body{margin:0}.box{width:64px;height:64px;background:red} div.box{background:green} #box{background:blue!important}</style><div id='box' class='box' style='background:white'></div>";
    assert_eq!(pixel(&frame(html, 0.0), 30, 30), [0, 0, 255, 255]);
}
#[test]
fn flex_and_grid_use_native_layout() {
    for display in [
        "display:flex",
        "display:grid;grid-template-columns:repeat(2,1fr)",
    ] {
        let html=format!("<style>body{{margin:0}}.stage{{{display};width:64px;height:64px}}.cell{{width:32px;height:64px}}.red{{background:red}}.blue{{background:blue}}</style><div class='stage'><div class='cell red'></div><div class='cell blue'></div></div>");
        let p = frame(&html, 0.0);
        assert_eq!(pixel(&p, 16, 32), [255, 0, 0, 255]);
        assert_eq!(pixel(&p, 48, 32), [0, 0, 255, 255]);
    }
}
#[test]
fn animation_seek_order_is_deterministic() {
    let html="<style>body{margin:0}div{width:64px;height:64px;background:red;animation:fade 2s linear both}@keyframes fade{from{opacity:0}to{opacity:1}}</style><div></div>";
    let mut c = compile(
        html,
        CompileOptions {
            width: 64,
            height: 64,
            ..Default::default()
        },
    )
    .unwrap();
    let mut s = skia_safe::surfaces::raster_n32_premul((64, 64)).unwrap();
    let mut encodings = Vec::new();
    for time in [1.0, 0.0, 2.0, 1.0] {
        render_frame(&mut c.director, time, s.canvas()).unwrap();
        encodings.push(
            s.image_snapshot()
                .encode(None, skia_safe::EncodedImageFormat::PNG, 100)
                .unwrap()
                .as_bytes()
                .to_vec(),
        );
    }
    assert_eq!(encodings[0], encodings[3]);
    assert_ne!(encodings[0], encodings[1]);
    let p = frame(html, 1.0);
    assert!((pixel(&p, 32, 32)[0] as i16 - 128).abs() <= 1);
}
#[test]
fn opacity_is_a_group_layer() {
    let html="<style>body{margin:0}.group{position:relative;width:64px;height:64px;opacity:.5}.child{position:absolute;inset:0;background:red}</style><div class='group'><div class='child'></div><div class='child'></div></div>";
    let p = frame(html, 0.0);
    assert!((pixel(&p, 32, 32)[0] as i16 - 128).abs() <= 1);
}

#[test]
fn inset_shadow_covers_the_inside_edge_above_background() {
    let html = "<style>body{margin:0}div{width:64px;height:64px;background:blue;box-shadow:inset 8px 0 0 red}</style><div></div>";
    let p = frame(html, 0.0);
    assert_eq!(pixel(&p, 2, 32), [255, 0, 0, 255]);
    assert_eq!(pixel(&p, 32, 32), [0, 0, 255, 255]);
    assert_eq!(pixel(&p, 62, 32), [0, 0, 255, 255]);
}

#[test]
fn svg_attribute_dimensions_render_and_external_paint_css_is_diagnosed() {
    let p = frame("<style>body{margin:0}</style><svg width='32' height='32' viewBox='0 0 32 32' xmlns='http://www.w3.org/2000/svg'><rect width='32' height='32' fill='red'/></svg>", 0.0);
    assert_eq!(pixel(&p, 16, 16), [255, 0, 0, 255]);
    assert_eq!(pixel(&p, 48, 48), [0, 0, 0, 255]);
    let error = compile(
        "<style>.shape{fill:red}</style><svg><rect class='shape' width='32' height='32'/></svg>",
        Default::default(),
    )
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains("stylesheet 'fill' inside SVG"), "{error}");
}

#[test]
fn svg_css_group_opacity_and_inherited_current_color_apply_once() {
    let p = frame("<style>body{margin:0;color:blue}</style><svg width='64' height='64' style='opacity:.5' xmlns='http://www.w3.org/2000/svg'><rect width='64' height='64' fill='currentColor'/></svg>",0.0);
    assert_eq!(pixel(&p, 32, 32), [0, 0, 128, 255]);
    let p = frame("<style>body{margin:0}</style><svg width='64' height='64' xmlns='http://www.w3.org/2000/svg'><style>.shape{fill:red}</style><rect class='shape' width='64' height='64'/></svg>",0.0);
    assert_eq!(pixel(&p, 32, 32), [255, 0, 0, 255]);
}
#[test]
fn unsupported_features_fail_with_context() {
    for html in [
        "<div id='bad' style='filter:url(#unknown)'></div>",
        "<script>gsap.to('div',{x:10})</script><div></div>",
        "<canvas></canvas>",
        "<div data-composition-src='scene.html'></div>",
    ] {
        assert!(
            compile(html, CompileOptions::default()).is_err(),
            "unexpected success for {html}"
        );
    }
}
#[test]
fn clip_end_is_exclusive_and_track_is_not_paint_order() {
    let html="<style>body{margin:0}div{position:absolute;inset:0;background:red}#top{background:blue}</style><div data-start='0' data-duration='1' data-track-index='10'></div><div id='top' data-start='1' data-duration='1' data-track-index='0'></div>";
    assert_eq!(pixel(&frame(html, 1.0), 32, 32), [0, 0, 255, 255]);
}
#[test]
fn css_variables_and_shorthand_cascade() {
    let html="<style>body{margin:0;--fill:red}div{width:64px;height:64px;background:var(--fill);padding:8px;padding-left:16px}div:where(.box){background:blue}</style><div class='box'></div>";
    assert_eq!(pixel(&frame(html, 0.0), 32, 32), [0, 0, 255, 255]);
}

#[test]
fn variable_shorthand_keeps_selector_specificity() {
    let html="<style>body{margin:0;--paint:red}#box{background:var(--paint)}.box{background-color:blue}</style><div id='box' class='box' style='width:64px;height:64px'></div>";
    assert_eq!(pixel(&frame(html, 0.0), 32, 32), [255, 0, 0, 255]);
}
#[test]
fn malformed_and_cyclic_styles_are_errors() {
    for html in [
        "<div style='opacity:invalid'></div>",
        "<div style='--a:var(--b);--b:var(--a);width:var(--a)'></div>",
        "<div style='transform:translateX(calc(10% + 2px))'></div>",
        "<svg><text>Needs a font</text></svg>",
    ] {
        assert!(
            compile(html, CompileOptions::default()).is_err(),
            "unexpected success: {html}"
        );
    }
}
#[test]
fn repeating_gradients_tile_before_the_box_ends() {
    let conic = "<style>body{margin:0}div{width:64px;height:64px;background:repeating-conic-gradient(red 0deg,red 10deg,blue 10deg,blue 20deg)}</style><div></div>";
    let p = frame(conic, 0.0);
    assert!(pixel(&p, 34, 8)[0] > 200, "5deg should stay in the red wedge");
    assert!(pixel(&p, 38, 9)[2] > 200, "15deg should stay in the blue wedge");
    let linear = "<style>body{margin:0}div{width:64px;height:64px;background:repeating-linear-gradient(90deg,red 0px,red 8px,blue 8px,blue 16px)}</style><div></div>";
    let p = frame(linear, 0.0);
    assert!(pixel(&p, 4, 32)[0] > 200);
    assert!(pixel(&p, 12, 32)[2] > 200);
    assert!(pixel(&p, 20, 32)[0] > 200);
    let radial = "<style>body{margin:0}div{width:64px;height:64px;background:repeating-radial-gradient(circle,red 0px,red 8px,blue 8px,blue 16px)}</style><div></div>";
    let p = frame(radial, 0.0);
    assert!(pixel(&p, 32, 32)[0] > 200, "radial center {:?}", pixel(&p, 32, 32));
    assert!(pixel(&p, 44, 32)[2] > 200, "radial 12px {:?}", pixel(&p, 44, 32));
    assert!(pixel(&p, 52, 32)[0] > 200, "radial 20px {:?}", pixel(&p, 52, 32));
}

#[test]
fn nonuniform_dashed_borders_keep_each_side() {
    let html = "<style>body{margin:0}div{width:64px;height:64px;box-sizing:border-box;background:white;border-top:4px dashed red;border-right:8px solid blue;border-bottom:4px dotted green;border-left:6px solid black}</style><div></div>";
    let p = frame(html, 0.0);
    assert_eq!(pixel(&p, 8, 2), [255, 0, 0, 255]);
    assert_eq!(pixel(&p, 18, 2), [255, 255, 255, 255]);
    assert_eq!(pixel(&p, 60, 32), [0, 0, 255, 255]);
    assert_eq!(pixel(&p, 3, 32), [0, 0, 0, 255]);
    // CSS named green is #008000, so a solid dot is [0, 128, 0, 255].
    let green = (0..64).any(|x| pixel(&p, x, 62) == [0, 128, 0, 255]);
    assert!(
        green,
        "bottom dots missing: {:?}",
        (0..64).step_by(4).map(|x| pixel(&p, x, 62)).collect::<Vec<_>>()
    );
}

#[test]
fn perspective_changes_a_rotate_y_card_and_backface_can_hide_it() {
    let card = "<style>body{margin:0}.stage{width:64px;height:64px;perspective:PERSP}.card{position:absolute;left:8px;top:16px;width:48px;height:32px;background:red;transform:rotateY(60deg)}</style><div class='stage'><div class='card'></div></div>";
    let with = frame(&card.replace("PERSP", "80px"), 0.0);
    let without = frame(&card.replace("PERSP", "none"), 0.0);
    assert_ne!(with, without);
    assert_eq!(pixel(&with, 32, 32), [255, 0, 0, 255]);
    let hidden = "<style>body{margin:0}div{width:64px;height:64px;background:red;transform:rotateY(180deg);backface-visibility:hidden}</style><div></div>";
    assert_eq!(pixel(&frame(hidden, 0.0), 32, 32), [0, 0, 0, 255]);
    let spin = "<style>body{margin:0}.stage{width:64px;height:64px;perspective:120px}.card{width:64px;height:64px;background:red;animation:spin 1s linear both}@keyframes spin{from{transform:rotateY(0deg)}to{transform:rotateY(70deg)}}</style><div class='stage'><div class='card'></div></div>";
    assert_eq!(pixel(&frame(spin, 0.0), 4, 32), [255, 0, 0, 255]);
    assert_eq!(pixel(&frame(spin, 1.0), 4, 32), [0, 0, 0, 255]);
}

#[test]
fn svg_stylesheet_filters_and_blend_modes_reach_usvg() {
    let blur = "<style>body{margin:0}svg .box{filter:blur(4px)}</style><svg width='64' height='64' xmlns='http://www.w3.org/2000/svg'><rect class='box' x='22' y='22' width='20' height='20' fill='red'/></svg>";
    let p = frame(blur, 0.0);
    assert!(pixel(&p, 32, 32)[0] > 200);
    assert!(
        pixel(&p, 16, 32)[0] > 20,
        "blur should spill red, got {:?}",
        pixel(&p, 16, 32)
    );
    assert_eq!(pixel(&p, 0, 0), &[0, 0, 0, 255], "far corner {:?}", pixel(&p, 0, 0));
    let multiply = "<style>body{margin:0}svg .top{mix-blend-mode:multiply}</style><svg width='64' height='64' xmlns='http://www.w3.org/2000/svg'><rect width='64' height='64' fill='blue'/><rect class='top' width='64' height='64' fill='red'/></svg>";
    let p = frame(multiply, 0.0);
    assert!(
        pixel(&p, 32, 32)[0] < 20 && pixel(&p, 32, 32)[2] < 20,
        "multiply red over blue should go dark, got {:?}",
        pixel(&p, 32, 32)
    );
    // The default filter region is only 10% past the object box, which stops at x=20 here.
    let primitive = "<style>body{margin:0}</style><svg width='64' height='64' xmlns='http://www.w3.org/2000/svg'><filter id='b' x='-50%' y='-50%' width='200%' height='200%'><feGaussianBlur stdDeviation='3'/></filter><rect x='22' y='22' width='20' height='20' fill='red' filter='url(#b)'/></svg>";
    let p = frame(primitive, 0.0);
    assert!(pixel(&p, 32, 32)[0] > 200, "center {:?}", pixel(&p, 32, 32));
    // stdDeviation 3 falls off faster than CSS blur(4px). x=16 is 6px outside the rect.
    assert!(
        pixel(&p, 16, 32)[0] > 5,
        "filter primitive should spill red, got {:?}",
        pixel(&p, 16, 32)
    );
}

#[test]
fn radial_conic_and_gradient_mask_paint_natively() {
    let radial="<style>body{margin:0}div{width:64px;height:64px;background:radial-gradient(circle,white 0%,black 100%)}</style><div></div>";
    let p = frame(radial, 0.0);
    assert!(pixel(&p, 32, 32)[0] > 240);
    assert!(pixel(&p, 0, 0)[0] < 10);
    let conic="<style>body{margin:0}div{width:64px;height:64px;background:conic-gradient(red 0deg,blue 180deg,red 360deg)}</style><div></div>";
    let p = frame(conic, 0.0);
    assert_ne!(pixel(&p, 32, 3), pixel(&p, 32, 60));
    let mask="<style>body{margin:0}div{width:64px;height:64px;background:red;mask-image:linear-gradient(90deg,transparent,black)}</style><div></div>";
    let p = frame(mask, 0.0);
    assert!(pixel(&p, 2, 32)[0] < 20);
    assert!(pixel(&p, 62, 32)[0] > 240);
}
#[test]
fn borders_remain_inside_the_box() {
    let html="<style>body{margin:0}div{width:32px;height:32px;border:4px solid red;border-radius:8px;box-sizing:border-box;background:blue}</style><div></div>";
    let p = frame(html, 0.0);
    assert_eq!(pixel(&p, 16, 2), [255, 0, 0, 255]);
    assert_eq!(pixel(&p, 16, 16), [0, 0, 255, 255]);
    assert_eq!(pixel(&p, 33, 16), [0, 0, 0, 255]);
}
#[test]
fn css_steps_iterations_and_negative_delay_are_seekable() {
    let html="<style>body{margin:0}div{width:64px;height:64px;background:red;animation:fade 1s steps(2,end) -.5s 2 alternate both}@keyframes fade{from{opacity:0}to{opacity:1}}</style><div></div>";
    let p = frame(html, 0.0);
    assert!((pixel(&p, 32, 32)[0] as i16 - 128).abs() <= 1);
    assert_eq!(pixel(&frame(html, 1.5), 32, 32), [0, 0, 0, 255]);
}
#[test]
fn captured_data_round_trips_and_rejects_gaps() {
    use director_html::{compile_capture, Capture, CapturedFrame, CapturedNode, CAPTURE_FORMAT};
    let root = CapturedNode {
        tag: "div".into(),
        id: "captured".into(),
        rect: [0.0, 0.0, 64.0, 64.0],
        styles: std::collections::BTreeMap::from([("background-color".into(), "red".into())]),
        text: None,
        svg: None,
        src: None,
        children: Vec::new(),
    };
    let capture = Capture {
        format: CAPTURE_FORMAT.into(),
        width: 64,
        height: 64,
        fps: 30.0,
        duration: 1.0,
        sample_rate: 2.0,
        fonts: Vec::new(),
        assets: Default::default(),
        frames: vec![
            CapturedFrame {
                time: 0.0,
                root: root.clone(),
            },
            CapturedFrame { time: 0.5, root },
        ],
    };
    let json = serde_json::to_string(&capture).unwrap();
    let mut copy: Capture = serde_json::from_str(&json).unwrap();
    let mut compiled = compile_capture(copy.clone(), std::path::Path::new(".")).unwrap();
    let mut s = skia_safe::surfaces::raster_n32_premul((64, 64)).unwrap();
    let mut encoded = Vec::new();
    for t in [0.6, 0.0, 0.6] {
        render_frame(&mut compiled.director, t, s.canvas()).unwrap();
        encoded.push(
            s.image_snapshot()
                .encode(None, skia_safe::EncodedImageFormat::PNG, 100)
                .unwrap()
                .as_bytes()
                .to_vec(),
        );
    }
    assert_eq!(encoded[0], encoded[2]);
    copy.frames[1].time = 0.75;
    assert!(compile_capture(copy, std::path::Path::new(".")).is_err());
}
#[test]
fn embedded_images_are_decoded_and_cached() {
    use base64::Engine;
    let mut image = skia_safe::surfaces::raster_n32_premul((2, 2)).unwrap();
    image.canvas().clear(skia_safe::Color::GREEN);
    let data = image
        .image_snapshot()
        .encode(None, skia_safe::EncodedImageFormat::PNG, 100)
        .unwrap();
    let encoded = base64::engine::general_purpose::STANDARD.encode(data.as_bytes());
    let html=format!("<style>body{{margin:0}}img{{width:64px;height:64px}}</style><img src='data:image/png;base64,{encoded}'>");
    let p = frame(&html, 0.0);
    assert_eq!(pixel(&p, 32, 32), [0, 255, 0, 255]);
}

#[test]
fn radius_animation_changes_native_clipping() {
    let html="<style>body{margin:0}div{width:64px;height:64px;background:red;animation:round 1s linear both}@keyframes round{from{border-radius:0px}to{border-radius:32px}}</style><div></div>";
    assert_eq!(pixel(&frame(html, 0.0), 1, 1), [255, 0, 0, 255]);
    assert_eq!(pixel(&frame(html, 1.0), 1, 1), [0, 0, 0, 255]);
}
