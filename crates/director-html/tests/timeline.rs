//! `director-timeline/1` tracks animate authored HTML elements natively.

use director_core::scene_export::export_scene;
use director_html::{compile, CompileOptions, Timeline, TIMELINE_FORMAT};
use director_render::render_frame;
use director_scene::{html::CssValue, NodeBody};

fn options() -> CompileOptions {
    CompileOptions {
        width: 64,
        height: 64,
        duration: 3.0,
        ..Default::default()
    }
}

fn page(timeline: &str) -> String {
    format!(
        "<style>body{{margin:0;background:white}}#box{{position:absolute;left:0;top:0;width:16px;height:16px;background:rgb(0,128,0)}}</style>\
         <div id='box'></div>\
         <script type='application/director-timeline+json'>{timeline}</script>"
    )
}

fn frame(source: &str, time: f64) -> Vec<u8> {
    let mut c = compile(source, options()).unwrap();
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

fn pixel(p: &[u8], x: usize, y: usize) -> [u8; 4] {
    p[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4].try_into().unwrap()
}

const GREEN: [u8; 4] = [0, 128, 0, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

const SLIDE: &str = r#"{"format":"director-timeline/1","tracks":[
    {"target":"box","property":"transform","keyframes":[
        {"time":1,"value":"translateX(0px)"},
        {"time":2,"value":"translateX(40px)"}]}]}"#;

#[test]
fn transform_track_holds_then_moves_then_holds() {
    let html = page(SLIDE);
    let before = frame(&html, 0.5);
    assert_eq!(pixel(&before, 8, 8), GREEN);
    assert_eq!(pixel(&before, 28, 8), WHITE);
    let middle = frame(&html, 1.5);
    assert_eq!(pixel(&middle, 4, 8), WHITE);
    assert_eq!(pixel(&middle, 28, 8), GREEN);
    let after = frame(&html, 2.8);
    assert_eq!(pixel(&after, 30, 8), WHITE);
    assert_eq!(pixel(&after, 48, 8), GREEN);
}

#[test]
fn ease_shapes_the_segment_arriving_at_its_keyframe() {
    let html = page(
        r#"{"format":"director-timeline/1","tracks":[
        {"target":"box","property":"left","keyframes":[
            {"time":0,"value":0},
            {"time":1,"value":40,"ease":"steps(1, end)"}]}]}"#,
    );
    assert_eq!(pixel(&frame(&html, 0.9), 8, 8), GREEN);
    assert_eq!(pixel(&frame(&html, 1.0), 48, 8), GREEN);
}

#[test]
fn numeric_opacity_and_a_single_keyframe_hold_for_the_whole_timeline() {
    let html = page(
        r#"{"format":"director-timeline/1","tracks":[
        {"target":"box","property":"opacity","keyframes":[{"time":2,"value":0}]}]}"#,
    );
    assert_eq!(pixel(&frame(&html, 0.0), 8, 8), WHITE);
    assert_eq!(pixel(&frame(&html, 2.5), 8, 8), WHITE);
}

#[test]
fn color_tracks_interpolate_css_colors() {
    let html = page(
        r#"{"format":"director-timeline/1","tracks":[
        {"target":"box","property":"background-color","keyframes":[
            {"time":0,"value":"rgb(0,0,0)"},
            {"time":2,"value":"rgb(200,100,0)"}]}]}"#,
    );
    assert_eq!(pixel(&frame(&html, 1.0), 8, 8), [100, 50, 0, 255]);
}

#[test]
fn tracks_export_as_native_css_animations() {
    let compiled = compile(&page(SLIDE), options()).unwrap();
    let document = export_scene(&compiled.director).unwrap();
    let animation = document
        .nodes
        .iter()
        .find_map(|node| match &node.body {
            NodeBody::Html(html) => html.animations.first().cloned(),
            _ => None,
        })
        .expect("timeline animation");
    assert_eq!(animation.delay, 1.0);
    assert_eq!(animation.duration, 1.0);
    assert_eq!(animation.fill_mode, "both");
    assert_eq!(animation.channels[0].property, "transform");
    assert!(matches!(animation.channels[0].keyframes[1].value, CssValue::Transform(_)));
}

#[test]
fn timeline_documents_round_trip_through_serde() {
    let timeline: Timeline = serde_json::from_str(SLIDE).unwrap();
    assert_eq!(timeline.format, TIMELINE_FORMAT);
    timeline.validate().unwrap();
    let json = serde_json::to_string(&timeline).unwrap();
    assert_eq!(serde_json::from_str::<Timeline>(&json).unwrap(), timeline);
}

fn refusal(source: &str) -> String {
    match compile(source, options()) {
        Ok(_) => panic!("compiled"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn invalid_timelines_are_refused() {
    let track = |target: &str, property: &str, keys: &str| {
        format!(r#"{{"target":"{target}","property":"{property}","keyframes":[{keys}]}}"#)
    };
    let doc = |tracks: &[String]| {
        format!(r#"{{"format":"director-timeline/1","tracks":[{}]}}"#, tracks.join(","))
    };
    let fade = r#"{"time":0,"value":0},{"time":1,"value":1}"#;
    for (timeline, message) in [
        (doc(&[track("missing", "opacity", fade)]), "matches no animatable element"),
        (
            doc(&[track("box", "opacity", fade), track("box", "opacity", fade)]),
            "twice",
        ),
        (
            doc(&[track("box", "opacity", r#"{"time":1,"value":0},{"time":1,"value":1}"#)]),
            "increase in time",
        ),
        (
            doc(&[track("box", "color", r#"{"time":0,"value":1}"#)]),
            "takes CSS text",
        ),
        (doc(&[track("box", "rotate", fade)]), "unsupported animated property"),
        (
            r#"{"format":"director-timeline/2","tracks":[]}"#.to_string(),
            "unsupported timeline format",
        ),
    ] {
        let error = refusal(&page(&timeline));
        assert!(error.contains(message), "{message}: {error}");
    }
    let keyframed = page(&doc(&[track("box", "opacity", fade)])).replace(
        "</style>",
        "@keyframes f{to{opacity:0}}#box{animation:f 1s}</style>",
    );
    // A running @keyframes animation wins over the track, as it does over inline styles.
    compile(&keyframed, Default::default()).unwrap();
}
