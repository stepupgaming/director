//! GSAP timelines translate to native `director-timeline/1` tracks. The
//! fixtures under `tests/fixtures/gsap` are the phase 5 GSAP gate set.

use director_core::scene_export::export_scene;
use director_html::{
    compile, compile_file, CompileOptions, CompiledHtml, TimelineKeyframe, TimelineTrack,
    TimelineValue,
};
use director_render::render_frame;
use director_scene::{html::CssTiming, NodeBody};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gsap").join(name)
}

fn load(name: &str) -> CompiledHtml {
    compile_file(&fixture(name), CompileOptions::default()).unwrap()
}

fn track<'a>(c: &'a CompiledHtml, target: &str, property: &str) -> &'a TimelineTrack {
    c.timeline
        .as_ref()
        .unwrap()
        .tracks
        .iter()
        .find(|t| t.target == target && t.property == property)
        .unwrap_or_else(|| panic!("no {property} track on {target}"))
}

fn num(k: &TimelineKeyframe) -> f64 {
    match k.value {
        TimelineValue::Number(n) => n,
        TimelineValue::Text(_) => panic!("text keyframe"),
    }
}

/// `(time, value, ease)` for a numeric track.
fn keys(t: &TimelineTrack) -> Vec<(f64, f64, Option<&str>)> {
    t.keyframes
        .iter()
        .map(|k| ((k.time * 1e4).round() / 1e4, (num(k) * 1e4).round() / 1e4, k.ease.as_deref()))
        .collect()
}

fn frame(c: &mut CompiledHtml, time: f64) -> Vec<u8> {
    let (w, h) = (c.director.width, c.director.height);
    let mut surface = skia_safe::surfaces::raster_n32_premul((w, h)).unwrap();
    render_frame(&mut c.director, time, &mut surface).unwrap();
    let info = skia_safe::ImageInfo::new(
        (w, h),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let mut pixels = vec![0; (w * h * 4) as usize];
    assert!(surface.read_pixels(&info, &mut pixels, w as usize * 4, (0, 0)));
    pixels
}

fn pixel(p: &[u8], width: i32, x: i32, y: i32) -> [u8; 4] {
    let i = ((y * width + x) * 4) as usize;
    p[i..i + 4].try_into().unwrap()
}

fn close(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 3)
}

#[test]
fn every_fixture_renders_as_native_tracks() {
    for name in ["entrance.html", "loop.html", "stagger.html"] {
        let mut c = load(name);
        let timeline = c.timeline.clone().expect(name);
        assert!(!timeline.tracks.is_empty(), "{name}");
        timeline.validate().unwrap();
        let duration = c.director.timeline_duration();
        for i in 0..=8 {
            frame(&mut c, duration * i as f64 / 8.0);
        }
    }
}

#[test]
fn entrance_resolves_positions_labels_defaults_and_stagger() {
    let c = load("entrance.html");
    // from() at 0.2 with the timeline defaults.
    assert_eq!(
        keys(track(&c, "headline", "translate-y")),
        [(0.2, 40.0, None), (0.8, 0.0, Some("power3.out"))]
    );
    assert_eq!(
        keys(track(&c, "headline", "opacity")),
        [(0.2, 0.0, None), (0.8, 1.0, Some("power3.out"))]
    );
    // ">0.4" follows the rule's fromTo, and xPercent starts from the CSS translate(-50%).
    assert_eq!(
        keys(track(&c, "headline", "translate-x-percent")),
        [(2.5, -50.0, None), (3.3, -40.0, Some("sine.inOut"))]
    );
    // "chips" sits 0.1 after the headline; each chip staggers by 0.15.
    assert_eq!(
        keys(track(&c, "/2", "scale-x")),
        [(1.05, 0.5, None), (1.65, 1.0, Some("back.out(1.7)"))]
    );
    // "<0.2" starts 0.2 after the staggered chips begin.
    assert_eq!(
        keys(track(&c, "rule", "width")),
        [(1.1, 0.0, None), (2.1, 400.0, Some("expo.inOut"))]
    );
}

#[test]
fn entrance_pixels_follow_gsap_eases() {
    let mut c = load("entrance.html");
    let w = c.director.width;
    let rule = |p: &[u8], x| pixel(p, w, x, 302);
    // expo.inOut is exactly half way at the midpoint: 200px of rule.
    let mid = frame(&mut c, 1.6);
    assert!(close(rule(&mid, 120 + 190), [255, 255, 255, 255]));
    assert!(!close(rule(&mid, 120 + 210), [255, 255, 255, 255]));
    // Before its from() starts, the headline holds the from state: invisible.
    let early = frame(&mut c, 0.1);
    assert!(close(pixel(&early, w, 320, 110), [0x10, 0x18, 0x20, 255]));
    let settled = frame(&mut c, 1.0);
    assert!(close(pixel(&settled, w, 320, 110), [0xf2, 0xaa, 0x4c, 255]));
}

#[test]
fn loop_covers_yoyo_repeats_sets_steps_and_colors() {
    let c = load("loop.html");
    assert_eq!(
        keys(track(&c, "dot", "translate-x")),
        [
            (0.0, 0.0, None),
            (1.0, 480.0, Some("power1.inOut")),
            (2.0, 0.0, Some("power1.inOut")),
            (3.0, 480.0, Some("power1.inOut")),
            (4.0, 0.0, Some("power1.inOut")),
        ]
    );
    // gsap.set hides the square until the timeline set at 1s.
    assert_eq!(
        keys(track(&c, "blink", "opacity")),
        [(0.9999, 0.0, None), (1.0, 1.0, None)]
    );
    // steps(3) jumps at quarters; repeat -1 restarts every 1.5s until the end.
    let rotation = keys(track(&c, "blink", "rotate-z"));
    assert_eq!(
        &rotation[..5],
        [
            (1.5, 0.0, None),
            (1.7499, 0.0, None),
            (1.75, 30.0, None),
            (1.9999, 30.0, None),
            (2.0, 60.0, None)
        ]
    );
    assert!(rotation.iter().any(|k| k.0 == 3.0 && k.1 == 0.0));
    assert!(rotation.last().unwrap().0 >= 5.0);
    let bar = track(&c, "bar", "background-color");
    assert_eq!(bar.keyframes.len(), 2);
    assert_eq!(bar.keyframes[1].value, TimelineValue::Text("rgba(233, 196, 106, 1)".into()));
    assert_eq!(bar.keyframes[1].ease.as_deref(), Some("power2.out"));
}

#[test]
fn loop_pixels_follow_yoyo_and_steps() {
    let mut c = load("loop.html");
    let w = c.director.width;
    let red = [0xe8, 0x4a, 0x5f, 255];
    // power1.inOut halfway: x = 240. Odd iterations play backward.
    let half = frame(&mut c, 0.5);
    assert!(close(pixel(&half, w, 40 + 240 + 30, 180), red));
    let back = frame(&mut c, 1.5);
    assert!(close(pixel(&back, w, 40 + 240 + 30, 180), red));
    let end = frame(&mut c, 4.5);
    assert!(close(pixel(&end, w, 70, 180), red));
}

#[test]
fn stagger_runs_listeners_nested_timelines_and_function_values() {
    let c = load("stagger.html");
    // Center-out stagger inside a nested timeline added at 0.25: GSAP spreads
    // each * (n - 1) over the normalized distance, so neighbors start 0.2 later.
    assert_eq!(keys(track(&c, "/2", "height"))[0], (0.25, 0.0, None));
    assert_eq!(keys(track(&c, "/1", "height"))[0], (0.45, 0.0, None));
    assert_eq!(keys(track(&c, "/0", "height"))[0], (0.65, 0.0, None));
    assert_eq!(keys(track(&c, "/0", "height"))[1], (1.15, 200.0, Some("circ.out")));
    // Function values per index, from "swing+=0.2".
    assert_eq!(
        keys(track(&c, "/4", "skew-x")),
        [(1.35, 0.0, None), (1.95, 10.0, Some("elastic.out(1, 0.4)"))]
    );
    // "-=0.2" overlaps the swing on other channels; "none" stays linear.
    assert_eq!(
        keys(track(&c, "/1", "border-top-left-radius")),
        [(1.75, 0.0, None), (2.25, 30.0, None)]
    );
    // amount 0.4 from the end: the last bar leads.
    assert_eq!(keys(track(&c, "/4", "opacity"))[0].0, 2.75);
    assert_eq!(keys(track(&c, "/0", "opacity"))[0].0, 3.15);
}

#[test]
fn gsap_eases_survive_scene_export() {
    let c = load("entrance.html");
    let doc = export_scene(&c.director).unwrap();
    let timings: Vec<_> = doc
        .nodes
        .iter()
        .filter_map(|n| match &n.body {
            NodeBody::Html(html) => Some(html),
            _ => None,
        })
        .flat_map(|h| &h.animations)
        .flat_map(|a| &a.channels)
        .flat_map(|ch| &ch.keyframes)
        .map(|k| k.timing.clone())
        .collect();
    assert!(timings.contains(&CssTiming::Gsap("power3.out".into())));
    assert!(timings.contains(&CssTiming::Gsap("back.out(1.7)".into())));
}

fn page(script: &str) -> String {
    format!(
        "<style>body{{margin:0}}#box{{position:absolute;width:10px;height:10px;background:red}}</style>\
         <div id='root' data-composition-id='main'><div id='box'></div><p>text <span id='word'>here</span></p></div>\
         <script>{script}</script>"
    )
}

fn refused(script: &str) -> String {
    let error = compile(&page(script), CompileOptions::default()).err().expect(script);
    error.to_string()
}

#[test]
fn unsupported_gsap_and_scripts_fail_with_context() {
    let tl = "const tl = gsap.timeline({paused:true}); window.__timelines.main = tl;";
    for (script, message) in [
        ("gsap.to('#box', {x: 10})", "gsap.to outside a timeline"),
        (&format!("{tl} tl.to('#box', {{x: 10, onUpdate() {{}}}})"), "callback 'onUpdate'"),
        (&format!("{tl} tl.to('#box', {{filter: 'blur(2px)'}})"), "property 'filter' has no native track"),
        (&format!("{tl} tl.to('#box', {{x: Math.random()}})"), "Math.random"),
        (&format!("{tl} tl.call(() => {{}})"), "call()"),
        (&format!("{tl} tl.to('#box', {{x: 10, ease: 'rough'}})"), "unsupported GSAP ease"),
        (
            &format!("{tl} tl.to('#box', {{x: 10, duration: 2}}, 0).to('#box', {{x: 0, duration: 0.5}}, 0.5)"),
            "overlap",
        ),
        (&format!("{tl} tl.to('#word', {{opacity: 0}})"), "not an animatable box"),
        ("gsap.timeline().to('#box', {x: 10});", "neither registered"),
        ("document.querySelector('#box').style.opacity = 0", "element style"),
    ] {
        let error = refused(script);
        assert!(error.contains(message), "{script}: {error}");
    }
    let error = compile(
        "<div></div><script src='app.js'></script>",
        CompileOptions::default(),
    )
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains("external script 'app.js'"), "{error}");
}

#[test]
fn script_without_gsap_compiles_static() {
    let c = compile(&page("const unused = 1 + 1;"), CompileOptions::default()).unwrap();
    assert!(c.timeline.is_none());
}

#[test]
fn transform_components_compose_over_the_css_transform() {
    let source = "<style>body{margin:0;background:white}#box{position:absolute;left:20px;top:20px;width:20px;height:20px;background:rgb(0,0,255);transform:translate(-50%,-50%)}</style>\
        <div id='root' data-composition-id='main' data-width='64' data-height='64' data-duration='2'><div id='box'></div></div>\
        <script>const tl = gsap.timeline({paused:true}); tl.to('#box', {x: 30, duration: 1, ease: 'none'}); window.__timelines.main = tl;</script>";
    let mut c = compile(source, CompileOptions::default()).unwrap();
    let start = frame(&mut c, 0.0);
    assert!(close(pixel(&start, 64, 15, 15), [0, 0, 255, 255]));
    let end = frame(&mut c, 1.5);
    assert!(close(pixel(&end, 64, 45, 15), [0, 0, 255, 255]));
    assert!(!close(pixel(&end, 64, 15, 15), [0, 0, 255, 255]));
    let rotated = "<style>#box{width:10px;height:10px;transform:skewX(5deg) rotate(10deg)}</style>\
        <div id='root' data-composition-id='main'><div id='box'></div></div>\
        <script>const tl = gsap.timeline({paused:true}); tl.to('#box', {x: 30}); window.__timelines.main = tl;</script>";
    let error = compile(rotated, CompileOptions::default()).err().unwrap().to_string();
    assert!(error.contains("base transform"), "{error}");
}

#[test]
fn gsap_motion_example_compiles_to_tracks() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/html/gsap-motion.html");
    let c = compile_file(&path, CompileOptions::default()).unwrap();
    assert_eq!(
        keys(track(&c, "pulse", "scale-x"))[..3],
        [(1.0, 0.6, None), (1.5, 1.0, Some("sine.inOut")), (2.0, 0.6, Some("sine.inOut"))]
    );
    assert_eq!(track(&c, "/5", "background-color").keyframes.len(), 2);
}

#[test]
fn stagger_distribution_matches_gsap() {
    let starts = |stagger: &str| {
        let source = format!(
            "<style>div div{{position:absolute;width:10px;height:10px;background:red}}</style>\
             <div id='root' data-composition-id='main'><div></div><div></div><div></div><div></div><div></div></div>\
             <script>const tl = gsap.timeline({{paused:true}}); tl.from('#root div', {{opacity: 0, duration: 1, stagger: {stagger}}}); window.__timelines.main = tl;</script>"
        );
        let c = compile(&source, CompileOptions::default()).unwrap();
        (0..5)
            .map(|i| keys(track(&c, &format!("/{i}"), "opacity"))[0].0)
            .collect::<Vec<_>>()
    };
    assert_eq!(starts("0.1"), [0.0, 0.1, 0.2, 0.3, 0.4]);
    assert_eq!(starts("{each: 0.1, from: 'center'}"), [0.4, 0.2, 0.0, 0.2, 0.4]);
    assert_eq!(starts("{each: 0.1, from: 'edges'}"), [0.0, 0.2, 0.4, 0.2, 0.0]);
    assert_eq!(starts("{each: 0.1, from: 1}"), [0.1333, 0.0, 0.1333, 0.2667, 0.4]);
    assert_eq!(starts("{amount: 0.4, from: 'end'}"), [0.4, 0.3, 0.2, 0.1, 0.0]);
    assert_eq!(starts("{each: 0.1, from: 0.25}"), [0.0667, 0.0, 0.1333, 0.2667, 0.4]);
}
