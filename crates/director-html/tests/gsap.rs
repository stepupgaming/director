//! Real GSAP runs at compile time and its frames become native
//! `director-timeline/1` tracks. The fixtures under `tests/fixtures/gsap` are
//! the phase 5 GSAP gate set; reference values come from GSAP itself.

use director_core::scene_export::export_scene;
use director_html::{
    compile, compile_file, CompileOptions, CompiledHtml, TimelineKeyframe, TimelineTrack,
    TimelineValue,
};
use director_render::render_frame;
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

/// The track value at `time`: keys interpolate linearly and hold at the ends.
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
        (page("gsap.to('#box', {filter: 'blur(2px)', duration: 1})"), "'filter' on #box"),
        (page("gsap.to('#word', {opacity: 0})"), "not an animatable box"),
        (page("gsap.to('#box', {attr: {'data-x': 10}})"), "'data-x' attribute of #box"),
        (page("setTimeout(() => {}, 100)"), "setTimeout of 100 ms"),
        (page("fetch('data.json')"), "fetch is not available"),
        (
            format!("<div data-composition-id='main'><svg><path id='p' d='M0 0L9 9'/></svg></div>{GSAP}<script>document.querySelector('#p').getTotalLength()</script>"),
            "SVG path length",
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
fn boa_reads_left_operands_before_the_right_assigns() {
    // GSAP's grid detection runs `max < (max = rect.left)`; the vendored Boa
    // copies the left local first, matching Node.
    let source = "<div data-composition-id='main'></div><script>        function a() { let k = -1; return k < (k = 5); } function c() { var k = 3; return k - (k = 5); }         function e() { var k = 3; return k + k++; } function g() { var k = 3; return k === (k = 5); }         function i() { var k = 3; k += (k = 5); return k; } function j() { var k = 3; return k * (k++, k); }         function l() { let k = -1, n = 0; while (k < (k = n++) && n < 4) {} return n; }         throw new Error(JSON.stringify([a(), c(), e(), g(), i(), j(), l()]));</script>";
    assert!(refused(source).contains("[true,-2,6,false,8,12,4]"));
}
