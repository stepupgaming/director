//! Real GSAP runs at compile time and its frames become native
//! `director-timeline/1` tracks. The fixtures under `tests/fixtures/gsap` are
//! the phase 5 GSAP gate set; reference values come from GSAP itself.

use director_core::scene_export::export_scene;
use director_html::{
    compile, compile_file, CompileOptions, CompiledHtml, TimelineKeyframe, TimelineTrack,
    TimelineValue,
};
use director_render::render_frame;
use director_scene::html::{SmilCalcMode, SmilEffect};
use director_scene::NodeBody;
use std::path::PathBuf;

const GSAP: &str = "<script src='https://cdn.jsdelivr.net/npm/gsap@3.14.2/dist/gsap.min.js'></script>";

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gsap").join(name)
}

fn opts() -> CompileOptions {
    CompileOptions {
        gsap_dir: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools/node_modules/gsap/dist")),
        ..Default::default()
    }
}

fn load(name: &str) -> CompiledHtml {
    compile_file(&fixture(name), opts()).unwrap()
}

fn tracks(c: &CompiledHtml) -> &[TimelineTrack] {
    &c.timeline.as_ref().unwrap().tracks
}

fn track<'a>(c: &'a CompiledHtml, target: &str, property: &str) -> &'a TimelineTrack {
    tracks(c)
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

/// The track value at `time`: keys interpolate linearly, a set's
/// `steps(1, end)` key holds the value before it, and the ends hold.
fn at(t: &TimelineTrack, time: f64) -> f64 {
    let keys = &t.keyframes;
    let i = keys.partition_point(|k| k.time <= time);
    if i == 0 {
        return num(&keys[0]);
    }
    if i == keys.len() {
        return num(&keys[i - 1]);
    }
    let (a, b) = (&keys[i - 1], &keys[i]);
    if b.ease.as_deref() == Some("steps(1, end)") {
        return num(a);
    }
    num(a) + (num(b) - num(a)) * (time - a.time) / (b.time - a.time)
}

fn near(value: f64, expected: f64, eps: f64) {
    assert!((value - expected).abs() <= eps, "{value} vs {expected}");
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

/// A 64x64 page with a blue 20px `#box` at (20, 20) and five `.dot` cells.
fn page(script: &str) -> String {
    format!(
        "<style>body{{margin:0;background:white}}#box{{position:absolute;left:20px;top:20px;width:20px;height:20px;background:rgb(0,0,255)}}\
         .dot{{position:absolute;width:4px;height:4px;background:red}}</style>\
         <div id='root' data-composition-id='main' data-width='64' data-height='64' data-duration='2'>\
         <div id='box'></div><div class='dot'></div><div class='dot'></div><div class='dot'></div><div class='dot'></div><div class='dot'></div>\
         <p>text <span id='word'>here</span></p></div>{GSAP}<script>{script}</script>"
    )
}

fn run(script: &str) -> CompiledHtml {
    compile(&page(script), opts()).unwrap_or_else(|e| panic!("{script}: {e}"))
}

fn refused(source: &str) -> String {
    compile(source, opts()).err().expect(source).to_string()
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
    // from() at 0.2 with the timeline defaults: power3.out is 0.9375 halfway.
    let y = track(&c, "headline", "translate-y");
    near(at(y, 0.2), 40.0, 1e-6);
    near(at(y, 0.5), 2.5, 0.02);
    near(at(y, 1.0), 0.0, 1e-6);
    near(at(track(&c, "headline", "opacity"), 0.5), 0.9375, 1e-3);
    // ">0.4" follows the rule's fromTo; xPercent starts from the CSS translate(-50%).
    let x = track(&c, "headline", "translate-x-percent");
    near(at(x, 2.5), -50.0, 1e-6);
    near(at(x, 2.9), -45.0, 0.02);
    near(at(x, 3.5), -40.0, 1e-6);
    near(at(track(&c, "headline", "rotate-z"), 3.5), -4.0, 1e-6);
    // "chips" sits 0.1 after the headline; each chip staggers by 0.15 and
    // back.out(1.7) overshoots to 1.0875 halfway.
    let chip = track(&c, "/1", "scale-x");
    near(at(chip, 0.9), 0.5, 1e-6);
    near(at(chip, 1.2), 0.5 + 0.5 * 1.0875, 1e-3);
    near(at(track(&c, "/2", "scale-x"), 1.0333), 0.5, 1e-6);
    near(at(chip, 2.0), 1.0, 1e-6);
    // "<0.2" starts 0.2 after the staggered chips begin. GSAP rounds px widths.
    let rule = track(&c, "rule", "width");
    near(at(rule, 1.1), 0.0, 1e-6);
    near(at(rule, 1.4), 11.0, 1e-6);
    near(at(rule, 1.6), 200.0, 1e-6);
    near(at(rule, 2.2), 400.0, 1e-6);
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
    let dot = track(&c, "dot", "translate-x");
    for (time, x) in [(0.0, 0.0), (0.5, 240.0), (1.0, 480.0), (1.5, 240.0), (2.0, 0.0), (3.0, 480.0), (4.0, 0.0), (4.5, 0.0)] {
        near(at(dot, time), x, 0.02);
    }
    // gsap.set hides the square until the timeline set at 1s.
    let blink = track(&c, "blink", "opacity");
    near(at(blink, 0.9), 0.0, 1e-6);
    near(at(blink, 1.0), 1.0, 1e-6);
    // steps(3) jumps at quarters; repeat -1 restarts every 1.5s until the end.
    let rotation = track(&c, "blink", "rotate-z");
    for (time, angle) in [(1.7, 0.0), (1.8, 30.0), (2.1, 60.0), (2.4, 90.0), (2.9, 90.0), (3.1, 0.0), (3.3, 30.0)] {
        near(at(rotation, time), angle, 1e-6);
    }
    let bar = track(&c, "bar", "background-color");
    assert_eq!(bar.keyframes.last().unwrap().value, TimelineValue::Text("rgba(233, 196, 106, 1)".into()));
    near(at(track(&c, "bar", "width"), 2.5), 360.0, 1e-6);
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
    for (bar, start) in [("/2", 0.25), ("/1", 0.45), ("/0", 0.65)] {
        let height = track(&c, bar, "height");
        near(at(height, start - 1.0 / 60.0), 0.0, 1e-6);
        near(at(height, start + 0.5), 200.0, 1e-6);
    }
    // Function values per index, from "swing+=0.2".
    near(at(track(&c, "/4", "skew-x"), 2.0), 10.0, 0.01);
    near(at(track(&c, "/0", "skew-x"), 2.0), -10.0, 0.01);
    // "-=0.2" overlaps the swing on other channels; "none" stays linear and
    // the four GSAP corner longhands form one radius.
    let radius = track(&c, "/1", "border-top-left-radius");
    near(at(radius, 2.0), 15.0, 1.0);
    near(at(radius, 2.3), 30.0, 1e-6);
    assert_eq!(
        track(&c, "/1", "background-color").keyframes.last().unwrap().value,
        TimelineValue::Text("rgba(255, 107, 107, 1)".into())
    );
    // autoAlpha with amount 0.4 from the end: the last bar leads.
    let (last, first) = (track(&c, "/4", "opacity"), track(&c, "/0", "opacity"));
    near(at(last, 2.75 - 1.0 / 60.0), 1.0, 1e-6);
    near(at(last, 3.2), 0.25, 1e-6);
    near(at(first, 3.15 - 1.0 / 60.0), 1.0, 1e-6);
    near(at(first, 3.6), 0.25, 1e-6);
}

#[test]
fn gsap_tracks_survive_scene_export() {
    let c = load("entrance.html");
    let doc = export_scene(&c.director).unwrap();
    let channels = doc
        .nodes
        .iter()
        .filter_map(|n| match &n.body {
            NodeBody::Html(html) => Some(html),
            _ => None,
        })
        .flat_map(|h| &h.animations)
        .flat_map(|a| &a.channels)
        .count();
    assert!(channels >= tracks(&c).len());
}

#[test]
fn page_scripts_fail_with_context() {
    for (source, message) in [
        (page("gsap.to('#box', {textShadow: '0 0 4px red', duration: 1})"), "'text-shadow' on #box"),
        (
            page("gsap.fromTo('#box', {clipPath: 'polygon(0% 0%, 100% 0%, 50% 100%)'}, {clipPath: 'polygon(0% 0%, 100% 0%, 100% 100%, 0% 100%)', duration: 1})"),
            "different corner counts",
        ),
        (page("gsap.to('body', {opacity: 0})"), "not an animatable box"),
        (page("gsap.to('#box', {attr: {'data-x': 10}})"), "'data-x' attribute of #box"),
        (page("setTimeout(() => {}, 100)"), "setTimeout of 100 ms"),
        (page("fetch('data.json')"), "fetch is not available"),
        (
            format!("<div data-composition-id='main'><svg><path id='p' d='Q'/></svg></div>{GSAP}<script>document.querySelector('#p').getTotalLength()</script>"),
            "SVG path data 'Q' does not parse",
        ),
        ("<div></div><script type='module'>gsap</script>".into(), "module scripts"),
        ("<div></div><script src='https://example.com/app.js'></script>".into(), "must be downloaded"),
        ("<div></div><script src='app.js'></script>".into(), "app.js"),
        ("<div></div><script>window.missing.call()</script>".into(), "inline script"),
    ] {
        let error = refused(&source);
        assert!(error.contains(message), "{message}: {error}");
    }
}

#[test]
fn scripts_without_motion_compile_static() {
    let c = compile(&page("const unused = 1 + 1;"), opts()).unwrap();
    assert!(c.timeline.is_none());
    let c = compile(&page("gsap.set('#box', {x: 10, opacity: 0.5})"), opts()).unwrap();
    assert!(c.timeline.is_none());
}

#[test]
fn global_tweens_callbacks_and_random_values_play() {
    // The global timeline plays without registration.
    let c = run("gsap.to('#box', {x: 30, duration: 1, ease: 'none'})");
    near(at(track(&c, "box", "translate-x"), 0.5), 15.0, 1e-3);
    // Callbacks run every frame and their writes become tracks.
    let c = run(
        "gsap.to('#box', {x: 10, duration: 1, ease: 'none', onUpdate() { \
         document.querySelector('#box').style.opacity = String(1 - this.progress()); }})",
    );
    near(at(track(&c, "box", "opacity"), 0.5), 0.5, 1e-3);
    // Math.random is seeded, so compiles repeat.
    let random = "gsap.to('.dot', {x: () => Math.random() * 40, duration: 1})";
    assert_eq!(run(random).timeline, run(random).timeline);
}

#[test]
fn transform_components_compose_over_the_css_transform() {
    let source = "<style>body{margin:0;background:white}#box{position:absolute;left:20px;top:20px;width:20px;height:20px;background:rgb(0,0,255);transform:translate(-50%,-50%)}</style>\
        <div id='root' data-composition-id='main' data-width='64' data-height='64' data-duration='2'><div id='box'></div></div>"
        .to_string()
        + GSAP
        + "<script>gsap.to('#box', {x: 30, duration: 1, ease: 'none'});</script>";
    let mut c = compile(&source, opts()).unwrap();
    let start = frame(&mut c, 0.0);
    assert!(close(pixel(&start, 64, 15, 15), [0, 0, 255, 255]));
    let end = frame(&mut c, 1.5);
    assert!(close(pixel(&end, 64, 45, 15), [0, 0, 255, 255]));
    assert!(!close(pixel(&end, 64, 15, 15), [0, 0, 255, 255]));
    // GSAP parses a skewed and rotated base into its own components, which
    // stay put while x moves.
    let skewed = source.replace("translate(-50%,-50%)", "skewX(5deg) rotate(10deg)");
    let c = compile(&skewed, opts()).unwrap();
    near(at(track(&c, "box", "translate-x"), 0.5), 15.0, 1e-3);
    assert!(tracks(&c).iter().all(|t| t.property == "translate-x"));
}

#[test]
fn transform_origin_pivots_rotation() {
    let mut c = run("gsap.to('#box', {rotation: 90, transformOrigin: '0% 0%', duration: 1})");
    // A quarter turn about the top-left corner swings the box left of it.
    let end = frame(&mut c, 1.5);
    assert!(close(pixel(&end, 64, 10, 30), [0, 0, 255, 255]));
    assert!(!close(pixel(&end, 64, 30, 30), [0, 0, 255, 255]));
}

#[test]
fn keyframes_arrays_objects_and_percentages() {
    let c = run("gsap.to('#box', {keyframes: {x: [0, 30, 10], easeEach: 'none'}, ease: 'none', duration: 2})");
    let x = track(&c, "box", "translate-x");
    near(at(x, 1.0), 30.0, 1e-3);
    near(at(x, 2.0), 10.0, 1e-3);
    let c = run(
        "gsap.to('#box', {keyframes: [{x: 30, duration: 1, ease: 'none'}, {y: 20, duration: 1, ease: 'none'}]})",
    );
    near(at(track(&c, "box", "translate-x"), 1.0), 30.0, 1e-3);
    near(at(track(&c, "box", "translate-y"), 1.0), 0.0, 1e-3);
    near(at(track(&c, "box", "translate-y"), 1.5), 10.0, 1e-3);
    let c = run("gsap.to('#box', {keyframes: {'50%': {x: 30}, '100%': {x: 0}}, ease: 'none', duration: 2})");
    near(at(track(&c, "box", "translate-x"), 1.0), 30.0, 0.05);
}

/// Each dot's linear opacity at 0.6s is 0.6 minus its stagger offset.
fn offsets(stagger: &str, dots: usize) -> Vec<f64> {
    let script = format!("gsap.from('.dot', {{opacity: 0, duration: 1, ease: 'none', stagger: {stagger}}})");
    let c = run(&script);
    (0..dots)
        .map(|i| ((0.6 - at(track(&c, &format!("/{}", i + 1), "opacity"), 0.6)) * 1e4).round() / 1e4)
        .collect()
}

#[test]
fn stagger_distribution_matches_gsap() {
    assert_eq!(offsets("0.1", 5), [0.0, 0.1, 0.2, 0.3, 0.4]);
    assert_eq!(offsets("{each: 0.1, from: 'center'}", 5), [0.4, 0.2, 0.0, 0.2, 0.4]);
    assert_eq!(offsets("{each: 0.1, from: 'edges'}", 5), [0.0, 0.2, 0.4, 0.2, 0.0]);
    assert_eq!(offsets("{amount: 0.4, from: 'end'}", 5), [0.4, 0.3, 0.2, 0.1, 0.0]);
    // A stagger ease spreads the 0.4s total along power2.in, a cubic.
    assert_eq!(offsets("{each: 0.1, ease: 'power2.in'}", 5), [0.0, 0.0062, 0.05, 0.1687, 0.4]);
}

#[test]
fn grid_staggers_read_layout() {
    // Six dots in three columns: explicit and measured grids agree with GSAP.
    let cells = (0..6)
        .map(|i| format!("<div class='cell' style='left:{}px;top:{}px'></div>", (i % 3) * 10, (i / 3) * 10))
        .collect::<String>();
    let source = |stagger: &str| {
        format!(
            "<style>body{{margin:0}}.cell{{position:absolute;width:8px;height:8px;background:red}}</style>\
             <div id='root' data-composition-id='main' data-width='64' data-height='64' data-duration='2'>{cells}</div>{GSAP}\
             <script>gsap.from('.cell', {{opacity: 0, duration: 1, ease: 'none', stagger: {stagger}}})</script>"
        )
    };
    let offsets = |stagger: &str| {
        let c = compile(&source(stagger), opts()).unwrap();
        (0..6)
            .map(|i| ((0.6 - at(track(&c, &format!("/{i}"), "opacity"), 0.6)) * 1e4).round() / 1e4)
            .collect::<Vec<_>>()
    };
    let expected = [0.5, 0.0, 0.5, 0.5, 0.0, 0.5];
    assert_eq!(offsets("{grid: [2, 3], from: 'center', amount: 0.5}"), expected);
    assert_eq!(offsets("{grid: 'auto', from: 'center', amount: 0.5}"), expected);
    assert_eq!(offsets("{grid: 'auto', from: 'end', axis: 'x', each: 0.1}"), [0.3, 0.15, 0.0, 0.3, 0.15, 0.0]);
}

#[test]
fn gsap_motion_example_compiles_to_tracks() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/html/gsap-motion.html");
    let c = compile_file(&path, opts()).unwrap();
    let pulse = track(&c, "pulse", "scale-x");
    near(at(pulse, 1.0), 0.6, 1e-3);
    near(at(pulse, 1.5), 1.0, 1e-3);
    near(at(pulse, 2.0), 0.6, 1e-3);
    assert!(track(&c, "/5", "background-color").keyframes.len() >= 2);
}

#[test]
fn motion_path_example_follows_its_route_and_morphs() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/html/motion-path.html");
    let mut c = compile_file(&path, opts()).unwrap();
    // The route ends at (760, 60) in a 2x viewBox inside a box turned -4deg
    // about its center, which puts the end at (1664.295, 330.262) on the page.
    // The rocket's center starts at (168, 148).
    near(at(track(&c, "rocket", "translate-x"), 4.0), 1496.295, 0.05);
    near(at(track(&c, "rocket", "translate-y"), 4.0), 182.262, 0.05);
    assert!(track(&c, "rocket", "rotate-z").keyframes.len() > 2);
    // The title sharpens from a 12px blur and turns pink at 2.25s.
    near(at(track(&c, "title", "filter-blur"), 0.0), 12.0, 1e-3);
    near(at(track(&c, "title", "filter-blur"), 0.6), 0.0, 0.02);
    assert_eq!(step(track(&c, "title", "color")), [2.25]);
    // The rocket's glow pulses, and a wipe reveals the morph from the left.
    near(at(track(&c, "rocket", "box-shadow-0-blur"), 0.0), 0.0, 0.02);
    near(at(track(&c, "rocket", "box-shadow-0-blur"), 1.0), 32.0, 0.05);
    near(at(track(&c, "morph-wrap", "clip-path-1-x"), 0.4), 0.0, 0.01);
    near(at(track(&c, "morph-wrap", "clip-path-1-x"), 1.2), 100.0, 0.01);
    let (w, h) = (c.director.width, c.director.height);
    let morph = pixel(&frame(&mut c, 0.4), w, 1720, 860);
    assert!(morph[1] < 100, "{morph:?}");
    let morph = pixel(&frame(&mut c, 1.0), w, 1720, 860);
    assert!(morph[0] < 100 && morph[1] > 150 && morph[2] > 150, "{morph:?}");
    let p = frame(&mut c, 4.0);
    let rocket = pixel(&p, w, 1664, 330);
    assert!(rocket[0] > 200 && rocket[2] > 150, "{rocket:?}");
    assert_eq!(h, 1080);
}

#[test]
fn boa_reads_left_operands_before_the_right_assigns() {
    // GSAP's grid detection runs `max < (max = rect.left)`; the vendored Boa
    // copies the left local first, matching Node.
    let source = "<div data-composition-id='main'></div><script>        function a() { let k = -1; return k < (k = 5); } function c() { var k = 3; return k - (k = 5); }         function e() { var k = 3; return k + k++; } function g() { var k = 3; return k === (k = 5); }         function i() { var k = 3; k += (k = 5); return k; } function j() { var k = 3; return k * (k++, k); }         function l() { let k = -1, n = 0; while (k < (k = n++) && n < 4) {} return n; }         throw new Error(JSON.stringify([a(), c(), e(), g(), i(), j(), l()]));</script>";
    assert!(refused(source).contains("[true,-2,6,false,8,12,4]"));
}

fn text_at(t: &TimelineTrack, time: f64) -> Option<String> {
    let i = t.keyframes.partition_point(|k| k.time <= time);
    (i > 0).then(|| match &t.keyframes[i - 1].value {
        TimelineValue::Text(s) => s.clone(),
        TimelineValue::Number(n) => n.to_string(),
    })
}

/// Columns of `p` with dark ink in rows 30..60.
fn ink_columns(p: &[u8], width: i32) -> usize {
    (0..width)
        .filter(|&x| (30..60).any(|y| pixel(p, width, x, y)[0] < 128))
        .count()
}

fn counter(script: &str) -> CompiledHtml {
    let source = format!(
        "<style>body{{margin:0;background:white}}#n{{position:absolute;left:0;top:30px;font-size:20px;color:black}}</style>\
         <div data-composition-id='main' style='background:white' data-width='96' data-height='64' data-duration='2'><div id='n'>0</div></div>\
         {GSAP}<script>{script}</script>"
    );
    compile(&source, opts()).unwrap_or_else(|e| panic!("{script}: {e}"))
}

#[test]
fn text_content_snaps_into_a_text_track() {
    let mut c = counter("gsap.to('#n', {textContent: 100, snap: {textContent: 1}, duration: 1, ease: 'none'})");
    let t = track(&c, "n", "text");
    assert_eq!(text_at(t, 0.0), None);
    assert_eq!(text_at(t, 0.5).as_deref(), Some("50"));
    assert_eq!(text_at(t, 1.5).as_deref(), Some("100"));
    assert!(t.keyframes.iter().all(|k| matches!(&k.value, TimelineValue::Text(s) if s.parse::<u32>().is_ok())));
    let w = c.director.width;
    let start = ink_columns(&frame(&mut c, 0.0), w);
    let end = ink_columns(&frame(&mut c, 1.5), w);
    assert!(start > 0 && end > start * 2, "{start} {end}");
    // The scene document keeps the text channel.
    let doc = export_scene(&c.director).unwrap();
    let text = doc
        .nodes
        .iter()
        .filter_map(|n| match &n.body {
            NodeBody::Html(html) => Some(html),
            _ => None,
        })
        .flat_map(|h| &h.animations)
        .flat_map(|a| &a.channels)
        .any(|c| c.property == "text");
    assert!(text);
}

#[test]
fn text_node_writes_and_empty_boxes_take_text() {
    let c = counter(
        "const n = document.querySelector('#n'); \
         gsap.to({v: 0}, {v: 3, duration: 1, ease: 'none', onUpdate() { n.firstChild.nodeValue = 'x' + Math.round(this.targets()[0].v); }})",
    );
    let t = track(&c, "n", "text");
    assert_eq!(text_at(t, 0.9).as_deref(), Some("x3"));
    let mut c = counter(
        "const n = document.querySelector('#n'); n.textContent = ''; \
         gsap.to(n, {textContent: 9, snap: {textContent: 1}, duration: 1, ease: 'none'})",
    );
    let w = c.director.width;
    assert_eq!(ink_columns(&frame(&mut c, 0.0), w), 0);
    assert!(ink_columns(&frame(&mut c, 1.5), w) > 0);
}

/// A white 64x64 page holding `svg` in a 64-unit viewBox at the origin.
fn svg_page(svg_attrs: &str, svg: &str, scripts: &str) -> CompiledHtml {
    let source = format!(
        "<style>body{{margin:0;background:white}}</style>\
         <div data-composition-id='main' style='background:white' data-width='64' data-height='64' data-duration='2'>\
         <svg {svg_attrs} width='64' height='64' viewBox='0 0 64 64' style='position:absolute;left:0;top:0'>{svg}</svg></div>\
         {GSAP}{scripts}"
    );
    compile(&source, opts()).unwrap_or_else(|e| panic!("{scripts}: {e}"))
}

fn blue(p: [u8; 4]) -> bool {
    p[2] > 200 && p[0] < 80 && p[1] < 80
}

fn dark(p: [u8; 4]) -> bool {
    p[0] < 80 && p[1] < 80 && p[2] < 80
}

const DOT: &str = "<circle id='c' cx='10' cy='32' r='6' fill='blue'/>";

#[test]
fn svg_attributes_play_as_smil() {
    let mut c = svg_page("", DOT, "<script>gsap.to('#c', {attr: {cx: 50}, duration: 1, ease: 'none'})</script>");
    let at = |c: &mut CompiledHtml, t: f64, x: i32| blue(pixel(&frame(c, t), 64, x, 32));
    assert!(at(&mut c, 0.0, 10) && !at(&mut c, 0.0, 30));
    assert!(at(&mut c, 0.5, 30) && !at(&mut c, 0.5, 10));
    assert!(at(&mut c, 1.5, 50) && !at(&mut c, 1.5, 30));
    // The svg starts half a second in, and the tween keeps composition time.
    let mut c = svg_page("data-start='0.5'", DOT, "<script>gsap.to('#c', {attr: {cx: 50}, duration: 1, ease: 'none'})</script>");
    assert!(at(&mut c, 0.75, 40) && !at(&mut c, 0.75, 20));
}

#[test]
fn svg_transforms_and_fills_play_as_smil() {
    let mut c = svg_page(
        "",
        "<rect id='r' x='0' y='24' width='16' height='16' fill='blue'/>",
        "<script>gsap.to('#r', {x: 40, rotation: 90, transformOrigin: '50% 50%', fill: 'rgb(255,0,0)', duration: 1, ease: 'none'})</script>",
    );
    let p = frame(&mut c, 0.0);
    assert!(blue(pixel(&p, 64, 8, 32)));
    let p = frame(&mut c, 1.5);
    assert!(close(pixel(&p, 64, 48, 32), [255, 0, 0, 255]), "{:?}", pixel(&p, 64, 48, 32));
    assert!(close(pixel(&p, 64, 8, 32), [255, 255, 255, 255]));
    // Halfway the color is a mix and the square has turned 45 degrees about its center.
    let p = frame(&mut c, 0.5);
    let mid = pixel(&p, 64, 28, 32);
    assert!(mid[0] > 80 && mid[2] > 80, "{mid:?}");
    assert!(pixel(&p, 64, 28, 23)[1] < 200, "the turned corner reaches up");
}

#[test]
fn draw_svg_and_path_geometry() {
    let draw = "<script src='https://cdn.jsdelivr.net/npm/gsap@3.14.2/dist/DrawSVGPlugin.min.js'></script>\
        <script>gsap.registerPlugin(DrawSVGPlugin); gsap.fromTo('#p', {drawSVG: '0%'}, {drawSVG: '100%', duration: 1, ease: 'none'})</script>";
    let mut c = svg_page("", "<path id='p' d='M0 32H64' stroke='black' stroke-width='8' fill='none'/>", draw);
    let at = |c: &mut CompiledHtml, t: f64, x: i32| dark(pixel(&frame(c, t), 64, x, 32));
    assert!(!at(&mut c, 0.0, 16));
    assert!(at(&mut c, 0.5, 16) && !at(&mut c, 0.5, 48));
    assert!(at(&mut c, 1.5, 48));
    // Path points follow the curve, and a circle's length is its circumference.
    let mut c = svg_page(
        "",
        &format!("<path id='p' d='M0 0C0 64 64 64 64 0'/>{DOT}"),
        "<script>const p = document.querySelector('#p'); const pt = p.getPointAtLength(p.getTotalLength() / 2); \
         gsap.set('#c', {attr: {cx: pt.x, cy: pt.y}}); \
         if (Math.abs(document.querySelector('#c').getTotalLength() - 12 * Math.PI) > 0.05) throw new Error('circle length');</script>",
    );
    let p = frame(&mut c, 0.0);
    assert!(blue(pixel(&p, 64, 32, 48)), "{:?}", pixel(&p, 64, 32, 48));
}

const MOTION_PATH: &str = "<script src='https://cdn.jsdelivr.net/npm/gsap@3.14.2/dist/MotionPathPlugin.min.js'></script>";

fn motion_page(markup: &str, motion_path: &str) -> CompiledHtml {
    let source = page(&format!(
        "gsap.registerPlugin(MotionPathPlugin); \
         window.__timelines = {{main: gsap.timeline({{paused: true}}).to('#box', {{duration: 1, ease: 'none', motionPath: {motion_path}}})}}"
    ))
        .replace("<div id='box'></div>", &format!("<div id='box'></div>{markup}"))
        .replace(GSAP, &format!("{GSAP}{MOTION_PATH}"));
    compile(&source, opts()).unwrap_or_else(|e| panic!("{motion_path}: {e}"))
}

#[test]
fn motion_path_follows_path_strings_and_point_lists() {
    // References: GSAP 3.14.2 MotionPathPlugin at progress 0.2 and 0.6.
    let c = motion_page("", "{path: 'M0,0 C40,80 80,-40 120,40', autoRotate: true}");
    let (x, y, r) = (track(&c, "box", "translate-x"), track(&c, "box", "translate-y"), track(&c, "box", "rotate-z"));
    for (t, ex, ey, er) in [(0.2, 18.103, 23.925, 35.715), (0.6, 73.663, 13.759, -20.323), (1.0, 120.0, 40.0, 63.435)] {
        near(at(x, t), ex, 0.02);
        near(at(y, t), ey, 0.02);
        near(at(r, t), er, 0.02);
    }
    let c = motion_page("", "{path: [{x: 0, y: 0}, {x: 60, y: 60}, {x: 120, y: 0}], curviness: 1.5}");
    let (x, y) = (track(&c, "box", "translate-x"), track(&c, "box", "translate-y"));
    for (t, ex, ey) in [(0.2, 18.225, 30.214), (0.6, 76.691, 54.865)] {
        near(at(x, t), ex, 0.02);
        near(at(y, t), ey, 0.02);
    }
}

#[test]
fn motion_path_aligns_through_view_boxes_and_ancestor_transforms() {
    // The svg draws 128 user units into 64 px and its wrapper shifts it 4 px
    // right, so the path runs from (8, 48) to (64, 48) on the page.
    let mut c = motion_page(
        "<div style='position:absolute;left:0;top:0;transform:translateX(4px)'>\
         <svg width='64' height='64' viewBox='0 0 128 128' style='display:block'><path id='p' d='M8,96 L120,96' fill='none'/></svg></div>",
        "{path: '#p', align: '#p', alignOrigin: [0.5, 0.5], autoRotate: true}",
    );
    // The box's center starts at (30, 30). The y offset and the flat path's
    // rotation hold, so they bake.
    let x = track(&c, "box", "translate-x");
    for (t, ex) in [(0.0, -22.0), (0.5, 6.0), (1.0, 34.0)] {
        near(at(x, t), ex, 0.02);
    }
    assert!(tracks(&c).iter().all(|t| t.property != "translate-y" && t.property != "rotate-z"));
    let p = frame(&mut c, 0.0);
    assert!(blue(pixel(&p, 64, 8, 48)) && !blue(pixel(&p, 64, 8, 30)) && !blue(pixel(&p, 64, 30, 48)));
    let p = frame(&mut c, 0.5);
    assert!(blue(pixel(&p, 64, 36, 48)) && !blue(pixel(&p, 64, 8, 48)));
}

#[test]
fn morph_svg_interpolates_paths() {
    let morph = "<script src='https://cdn.jsdelivr.net/npm/gsap@3.14.2/dist/MorphSVGPlugin.min.js'></script>\
        <script>gsap.registerPlugin(MorphSVGPlugin); MorphSVGPlugin.convertToPath('#c'); \
        window.__timelines = {main: gsap.timeline({paused: true}).to('#a', {morphSVG: '#b', duration: 1, ease: 'none'})}</script>";
    let mut c = svg_page(
        "",
        "<path id='a' d='M8,8 L56,8 L56,56 L8,56 Z' fill='blue'/>\
         <path id='b' d='M32,2 C48,2 62,16 62,32 C62,48 48,62 32,62 C16,62 2,48 2,32 C2,16 16,2 32,2 Z' fill='none'/>\
         <circle id='c' cx='32' cy='32' r='2' fill='red'/>",
        morph,
    );
    let p = frame(&mut c, 0.0);
    assert!(blue(pixel(&p, 64, 10, 10)) && !blue(pixel(&p, 64, 4, 32)));
    let p = frame(&mut c, 1.5);
    assert!(!blue(pixel(&p, 64, 10, 10)) && blue(pixel(&p, 64, 4, 32)));
    // convertToPath swapped the circle for a path in place.
    assert!(close(pixel(&p, 64, 32, 32), [255, 0, 0, 255]), "{:?}", pixel(&p, 64, 32, 32));
    // Every frame, the end path GSAP writes back included, interpolates.
    let doc = export_scene(&c.director).unwrap();
    let modes: Vec<_> = doc
        .nodes
        .iter()
        .filter_map(|n| match &n.body {
            NodeBody::Html(h) => match &h.content.as_ref()?.body {
                NodeBody::SvgSmil(s) => Some(s),
                _ => None,
            },
            _ => None,
        })
        .flat_map(|s| &s.animations)
        .filter_map(|a| match &a.effect {
            SmilEffect::Attribute { name, mode, .. } if name == "d" => Some(*mode),
            _ => None,
        })
        .collect();
    assert_eq!(modes, [SmilCalcMode::Linear]);
}

#[test]
fn helper_elements_must_leave_before_the_frame_ends() {
    // GSAP's measuring helpers add and remove elements mid-timeline.
    run("gsap.to('#box', {x: 10, duration: 1, onUpdate() { const i = document.createElement('i'); document.body.appendChild(i); i.getBoundingClientRect(); i.remove(); }})");
    let e = refused(&page("gsap.to('#box', {x: 10, duration: 1, onUpdate() { document.body.appendChild(document.createElement('i')); }})"));
    assert!(e.contains("scripts left a <i> in the document"), "{e}");
}

/// A 200x64 page of 20px Roboto on 24px lines holding `body`.
fn line_page(body: &str, script: &str) -> CompiledHtml {
    let source = format!(
        "<style>body{{margin:0;background:white}}p,h1{{margin:0;font:20px Roboto;line-height:24px;color:black}}</style>\
         <div data-composition-id='main' style='background:white' data-width='200' data-height='64' data-duration='2'>{body}</div>\
         {GSAP}<script src='https://cdn.jsdelivr.net/npm/gsap@3.14.2/dist/SplitText.min.js'></script><script>{script}</script>"
    );
    compile(&source, opts()).unwrap_or_else(|e| panic!("{script}: {e}"))
}

/// Dark pixels in columns `x` and rows `y`.
fn ink(p: &[u8], x: std::ops::Range<i32>, y: std::ops::Range<i32>) -> usize {
    y.flat_map(|y| x.clone().map(move |x| (x, y))).filter(|&(x, y)| dark(pixel(p, 200, x, y))).count()
}

#[test]
fn inline_blocks_sit_on_their_line_and_move() {
    let mut c = line_page(
        "<p>Ab <span id='w' style='display:inline-block'>Cd</span> Ef</p>",
        "const r = document.querySelector('#w').getBoundingClientRect();\
         if (!(r.left > 20 && r.left < 40 && Math.abs(r.top) < 1 && r.height === 24)) throw new Error('rect ' + [r.left, r.top, r.width, r.height]);\
         gsap.from('#w', {y: 30, duration: 1, ease: 'none'})",
    );
    assert!(tracks(&c).iter().any(|t| t.target == "w" && t.property == "translate-y"));
    let p = frame(&mut c, 0.0);
    assert!(ink(&p, 30..56, 32..56) > 0, "the word starts 30px down");
    assert!(ink(&p, 0..26, 2..22) > 0 && ink(&p, 60..90, 2..22) > 0, "its neighbours stay on the line");
    let p = frame(&mut c, 1.5);
    assert_eq!(ink(&p, 0..200, 26..64), 0);
    assert!(ink(&p, 30..56, 2..22) > 0);
}

#[test]
fn inline_spans_animate_opacity_and_color_without_transforms() {
    let mut c = line_page(
        "<p>Ab <span id='w'>Cd</span> Ef</p>",
        "gsap.to('#w', {opacity: 0, y: 20, color: 'rgb(255,0,0)', duration: 1, ease: 'none'})",
    );
    track(&c, "w", "opacity");
    track(&c, "w", "color");
    assert!(!tracks(&c).iter().any(|t| t.target == "w" && t.property.starts_with("translate")));
    let p = frame(&mut c, 0.0);
    assert!(ink(&p, 30..56, 2..22) > 0);
    let p = frame(&mut c, 1.5);
    assert_eq!(ink(&p, 30..56, 0..64), 0, "the span faded out in place");
    assert!(ink(&p, 0..26, 2..22) > 0 && ink(&p, 60..90, 2..22) > 0);
}

#[test]
fn split_text_lines_words_and_chars_animate_natively() {
    let mut c = line_page(
        "<h1 id='t' style='width:110px'>Hello split world</h1>",
        "gsap.registerPlugin(SplitText); const s = SplitText.create('#t', {type: 'lines,words,chars'});\
         if (s.lines.length !== 2 || s.words.length !== 3 || s.chars.length !== 15) throw new Error([s.lines.length, s.words.length, s.chars.length].join());\
         gsap.from(s.chars, {opacity: 0, y: 12, stagger: 0.05, duration: 0.3, ease: 'none'})",
    );
    let opacity = tracks(&c).iter().filter(|t| t.property == "opacity").count();
    assert_eq!(opacity, 15, "one opacity track per character");
    assert_eq!(ink(&frame(&mut c, 0.0), 0..200, 0..64), 0);
    let p = frame(&mut c, 1.9);
    assert!(ink(&p, 0..110, 2..22) > 0 && ink(&p, 0..110, 26..46) > 0, "two lines of text");
    assert_eq!(ink(&p, 0..200, 50..64), 0);
}

/// The key a set leaves on a track: its time and value, held until then.
fn step(t: &TimelineTrack) -> Vec<f64> {
    t.keyframes
        .iter()
        .filter(|k| k.ease.as_deref() == Some("steps(1, end)"))
        .map(|k| k.time)
        .collect()
}

#[test]
fn sets_hold_until_their_jump() {
    let mut c = run(
        "const tl = gsap.timeline({paused: true});\
         tl.to('#box', {y: 20, duration: 1, ease: 'none'}, 0)\
           .set('#box', {x: 30}, 0.51)\
           .set('#box', {backgroundColor: 'rgb(255,0,0)'}, 1)\
           .set('#box', {textContent: 'A'}, 0.25);\
         gsap.set('#box', {opacity: 0.5, delay: 0.75});\
         window.__timelines = {main: tl};",
    );
    // A set between frames jumps at its own time; one on a frame keys there.
    let x = track(&c, "box", "translate-x");
    assert_eq!(step(x), [0.51], "{:?}", x.keyframes);
    assert_eq!(num(x.keyframes.last().unwrap()), 30.0);
    assert_eq!(step(track(&c, "box", "background-color")), [1.0]);
    let o = track(&c, "box", "opacity");
    assert_eq!(step(o), [0.75], "{:?}", o.keyframes);
    // The tween beside the set keeps its linear keys.
    let y = track(&c, "box", "translate-y");
    assert!(step(y).is_empty(), "{:?}", y.keyframes);
    let text = track(&c, "box", "text");
    assert_eq!(text.keyframes.iter().map(|k| k.time).collect::<Vec<_>>(), [0.25]);
    // Between frames the box still sits at x 0 just before the set.
    let p = frame(&mut c, 0.505);
    assert!(close(pixel(&p, 64, 21, 36), [0, 0, 255, 255]), "{:?}", pixel(&p, 64, 21, 36));
    let p = frame(&mut c, 0.52);
    assert!(close(pixel(&p, 64, 21, 36), [0, 0, 0, 255]), "{:?}", pixel(&p, 64, 21, 36));
    assert!(close(pixel(&p, 64, 52, 36), [0, 0, 255, 255]), "{:?}", pixel(&p, 64, 52, 36));
}

#[test]
fn filter_tweens_become_native_channels() {
    let mut c = run(
        "window.__timelines = {main: gsap.timeline({paused: true}).fromTo('#box',\
         {filter: 'blur(0px) grayscale(0)'}, {filter: 'blur(6px) grayscale(1)', duration: 1, ease: 'none'}, 0)};",
    );
    near(at(track(&c, "box", "filter-blur"), 0.5), 3.0, 1e-3);
    near(at(track(&c, "box", "filter-grayscale"), 0.5), 0.5, 1e-3);
    let p = frame(&mut c, 0.0);
    assert!(close(pixel(&p, 64, 30, 30), [0, 0, 255, 255]), "{:?}", pixel(&p, 64, 30, 30));
    assert!(close(pixel(&p, 64, 17, 30), [0, 0, 0, 255]), "{:?}", pixel(&p, 64, 17, 30));
    let p = frame(&mut c, 1.0);
    let [r, g, b, _] = pixel(&p, 64, 30, 30);
    assert!(r == g && g == b && b < 128, "grey {:?}", [r, g, b]);
    assert!(pixel(&p, 64, 17, 30)[0] > 3, "blur spreads past the edge");
    // The channels export to the scene document.
    let doc = export_scene(&c.director).unwrap();
    let names: Vec<_> = doc
        .nodes
        .iter()
        .filter_map(|n| match &n.body {
            NodeBody::Html(html) => Some(html),
            _ => None,
        })
        .flat_map(|h| &h.animations)
        .flat_map(|a| &a.channels)
        .map(|c| c.property.as_str())
        .collect();
    assert!(names.contains(&"filter-blur") && names.contains(&"filter-grayscale"), "{names:?}");
}

#[test]
fn box_shadow_tweens_become_native_channels() {
    let mut c = run(
        "window.__timelines = {main: gsap.timeline({paused: true}).fromTo('#box',\
         {boxShadow: '0px 0px 0px 0px rgba(255,0,0,1)'}, {boxShadow: '8px 8px 0px 0px rgba(255,0,0,1)', duration: 1, ease: 'none'}, 0)};",
    );
    near(at(track(&c, "box", "box-shadow-0-x"), 0.5), 4.0, 1e-3);
    near(at(track(&c, "box", "box-shadow-0-y"), 0.5), 4.0, 1e-3);
    let p = frame(&mut c, 0.0);
    assert!(close(pixel(&p, 64, 44, 44), [0, 0, 0, 255]), "{:?}", pixel(&p, 64, 44, 44));
    let p = frame(&mut c, 1.0);
    assert!(close(pixel(&p, 64, 44, 44), [255, 0, 0, 255]), "{:?}", pixel(&p, 64, 44, 44));
    assert!(close(pixel(&p, 64, 30, 30), [0, 0, 255, 255]), "{:?}", pixel(&p, 64, 30, 30));

    // An inset shadow grows from none and keeps its inset flag.
    let mut c = run(
        "window.__timelines = {main: gsap.timeline({paused: true}).to('#box',\
         {boxShadow: 'inset 0px 0px 0px 6px rgb(255,255,0)', duration: 1, ease: 'none'}, 0)};",
    );
    assert!(tracks(&c).iter().any(|t| t.property == "box-shadow-0-inset"));
    near(at(track(&c, "box", "box-shadow-0-spread"), 0.5), 3.0, 1e-3);
    let p = frame(&mut c, 1.0);
    assert!(close(pixel(&p, 64, 22, 30), [255, 255, 0, 255]), "{:?}", pixel(&p, 64, 22, 30));
    assert!(close(pixel(&p, 64, 30, 30), [0, 0, 255, 255]), "{:?}", pixel(&p, 64, 30, 30));
    assert!(close(pixel(&p, 64, 44, 30), [0, 0, 0, 255]), "{:?}", pixel(&p, 64, 44, 30));

    // The computed shadow lists its color first, so GSAP pairs the cascade's
    // lengths with the target's.
    let source = page(
        "window.__timelines = {main: gsap.timeline({paused: true}).to('#box',\
         {boxShadow: '0px 12px 0px 0px rgb(255,0,0)', duration: 1, ease: 'none'}, 0)};",
    )
    .replace("background:rgb(0,0,255)}", "background:rgb(0,0,255);box-shadow:0 4px 0 red}");
    let c = compile(&source, opts()).unwrap();
    near(at(track(&c, "box", "box-shadow-0-y"), 0.0), 4.0, 1e-3);
    near(at(track(&c, "box", "box-shadow-0-y"), 0.5), 8.0, 1e-3);
}

#[test]
fn clip_path_tweens_move_shape_corners() {
    let mut c = run(
        "window.__timelines = {main: gsap.timeline({paused: true}).fromTo('#box',\
         {clipPath: 'inset(0% 100% 0% 0%)'}, {clipPath: 'inset(0% 0% 0% 0%)', duration: 1, ease: 'none'}, 0)};",
    );
    near(at(track(&c, "box", "clip-path-1-x"), 0.5), 50.0, 1e-2);
    let p = frame(&mut c, 0.5);
    assert!(close(pixel(&p, 64, 25, 38), [0, 0, 255, 255]), "{:?}", pixel(&p, 64, 25, 38));
    assert!(close(pixel(&p, 64, 35, 38), [0, 0, 0, 255]), "{:?}", pixel(&p, 64, 35, 38));
    let p = frame(&mut c, 1.0);
    assert!(close(pixel(&p, 64, 38, 38), [0, 0, 255, 255]), "{:?}", pixel(&p, 64, 38, 38));

    let mut c = run(
        "window.__timelines = {main: gsap.timeline({paused: true}).fromTo('#box',\
         {clipPath: 'circle(0% at 50% 50%)'}, {clipPath: 'circle(50% at 50% 50%)', duration: 1, ease: 'none'}, 0)};",
    );
    let p = frame(&mut c, 0.0);
    assert!(close(pixel(&p, 64, 30, 37), [0, 0, 0, 255]), "{:?}", pixel(&p, 64, 30, 37));
    let p = frame(&mut c, 1.0);
    assert!(close(pixel(&p, 64, 30, 37), [0, 0, 255, 255]), "{:?}", pixel(&p, 64, 30, 37));
    assert!(close(pixel(&p, 64, 21, 21), [0, 0, 0, 255]), "{:?}", pixel(&p, 64, 21, 21));
}

#[test]
fn drop_shadow_filters_tween_offset_blur_and_color() {
    let mut c = run(
        "window.__timelines = {main: gsap.timeline({paused: true}).fromTo('#box',\
         {filter: 'drop-shadow(0px 0px 0px rgba(255,0,0,0))'},\
         {filter: 'drop-shadow(8px 8px 0px rgba(255,0,0,1))', duration: 1, ease: 'none'}, 0)};",
    );
    near(at(track(&c, "box", "filter-drop-shadow-x"), 0.5), 4.0, 1e-3);
    assert!(tracks(&c).iter().any(|t| t.property == "filter-drop-shadow-color"));
    let p = frame(&mut c, 0.0);
    assert!(close(pixel(&p, 64, 44, 44), [0, 0, 0, 255]), "{:?}", pixel(&p, 64, 44, 44));
    let p = frame(&mut c, 1.0);
    assert!(close(pixel(&p, 64, 44, 44), [255, 0, 0, 255]), "{:?}", pixel(&p, 64, 44, 44));
    assert!(close(pixel(&p, 64, 30, 30), [0, 0, 255, 255]), "{:?}", pixel(&p, 64, 30, 30));
}

