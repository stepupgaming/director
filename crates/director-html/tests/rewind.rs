//! A `time` track runs an element's descendants on their own clock: rewinds,
//! replays, freezes and speed ramps.

use director_core::scene_export::export_scene;
use director_html::{compile, CompileOptions};
use director_render::render_frame;
use director_scene::{NodeBody, SceneDocument};

fn options() -> CompileOptions {
    CompileOptions {
        width: 64,
        height: 64,
        duration: 6.0,
        ..Default::default()
    }
}

/// A green box slides 0 to 40 px over 0 to 2 s and a blue box appears at 1 s,
/// both inside `#film`. `time` holds the film's time track keys.
fn page(time: &str, root: &str) -> String {
    format!(
        "<style>body{{margin:0;background:white}}#film{{position:absolute;inset:0}}\
         #box{{position:absolute;left:0;top:0;width:16px;height:16px;background:rgb(0,128,0)}}\
         #late{{position:absolute;left:0;top:40px;width:16px;height:16px;background:rgb(0,0,255)}}</style>\
         <div id='root' style='position:relative;width:64px;height:64px' {root}>\
         <div id='film'><div id='box'></div><div id='late' data-start='1'></div></div></div>\
         <script type='application/director-timeline+json'>{{\"format\":\"director-timeline/1\",\"tracks\":[\
         {{\"target\":\"box\",\"property\":\"transform\",\"keyframes\":[\
         {{\"time\":0,\"value\":\"translateX(0px)\"}},{{\"time\":2,\"value\":\"translateX(40px)\"}}]}},\
         {{\"target\":\"film\",\"property\":\"time\",\"keyframes\":[{time}]}}]}}</script>"
    )
}

const REWIND: &str = r#"{"time":2,"value":2},{"time":3,"value":0}"#;

fn frame(source: &str, time: f64) -> Vec<u8> {
    let mut c = compile(source, options()).unwrap();
    let mut surface = skia_safe::surfaces::raster_n32_premul((64, 64)).unwrap();
    render_frame(&mut c.director, time, &mut surface).unwrap();
    let info = skia_safe::ImageInfo::new((64, 64), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Unpremul, None);
    let mut pixels = vec![0; 64 * 64 * 4];
    assert!(surface.read_pixels(&info, &mut pixels, 64 * 4, (0, 0)));
    pixels
}

fn pixel(p: &[u8], x: usize, y: usize) -> [u8; 4] {
    p[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4].try_into().unwrap()
}

/// Left edge of the green box on row 8.
fn box_left(p: &[u8]) -> usize {
    (0..64).find(|&x| pixel(p, x, 8) == GREEN).expect("the box is on screen")
}

const GREEN: [u8; 4] = [0, 128, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

#[test]
fn a_time_track_rewinds_then_replays_the_subtree() {
    let html = page(REWIND, "");
    // Before the first key the film runs on composition time.
    assert_eq!(box_left(&frame(&html, 1.0)), 20);
    assert_eq!(box_left(&frame(&html, 2.0)), 40);
    // From 2 s to 3 s the film clock runs from 2 s back to 0 s.
    let rewound = frame(&html, 2.75);
    assert_eq!(box_left(&rewound), 10);
    // The blue box's own start is on the film clock: hidden again at film time 0.5 s.
    assert_eq!(pixel(&rewound, 8, 48), WHITE);
    assert_eq!(pixel(&frame(&html, 2.25), 8, 48), BLUE);
    assert_eq!(box_left(&frame(&html, 3.0)), 0);
    // After the last key the film plays forward again from 0 s.
    let replay = frame(&html, 4.0);
    assert_eq!(box_left(&replay), 20);
    assert_eq!(pixel(&replay, 8, 48), BLUE);
}

#[test]
fn time_keys_ease_hold_and_take_seconds_text() {
    // An eased rewind starts slow: GSAP power2.in is cubic, so half time covers an eighth of the way.
    let eased = page(r#"{"time":2,"value":2},{"time":3,"value":0,"ease":"power2.in"}"#, "");
    assert_eq!(box_left(&frame(&eased, 2.5)), 35);
    // Equal values hold the film; `ms` and `s` text are seconds.
    let freeze = page(r#"{"time":1,"value":"1000ms"},{"time":4,"value":"1s"}"#, "");
    assert_eq!(box_left(&frame(&freeze, 3.0)), 20);
    assert_eq!(box_left(&frame(&freeze, 4.5)), 30);
}

#[test]
fn motion_blur_follows_the_rewound_clock() {
    // On the composition clock the box holds after 2 s, but the rewind moves it.
    let root = "data-composition-id='m' data-width='64' data-height='64' data-fps='10' data-duration='6'";
    let blur = &format!("{root} data-motion-blur='8' data-shutter-angle='360'");
    let blurred = frame(&page(REWIND, blur), 2.5);
    let edge = (0..64).map(|x| pixel(&blurred, x, 8)).filter(|p| *p != GREEN && *p != WHITE).count();
    assert!(edge > 2, "the rewinding box smears, {edge} mixed pixels");
    // A frozen film paints one sharp sample.
    let held = frame(&page(r#"{"time":2,"value":2},{"time":4,"value":2}"#, blur), 3.0);
    assert_eq!(held, frame(&page(r#"{"time":2,"value":2},{"time":4,"value":2}"#, root), 3.0));
}

#[test]
fn the_time_map_round_trips_through_the_scene_document() {
    let compiled = compile(&page(REWIND, ""), options()).unwrap();
    let document = export_scene(&compiled.director).unwrap();
    let parsed = SceneDocument::from_json(&document.to_json().unwrap()).unwrap();
    assert_eq!(parsed, document);
    let maps: Vec<_> = parsed
        .nodes
        .iter()
        .filter_map(|node| match &node.body {
            NodeBody::Html(html) => html.time_map.as_ref(),
            _ => None,
        })
        .collect();
    assert_eq!(maps.len(), 1);
    assert_eq!((maps[0][1].time, maps[0][1].value), (3.0, 0.0));
}

#[test]
fn time_tracks_refuse_what_would_leave_their_clock() {
    let refused = |html: String, needle: &str| {
        let error = compile(&html, options()).err().expect(needle).to_string();
        assert!(error.contains(needle), "{error}");
    };
    // An element with its own text has no child clock to set.
    refused(
        r#"<p id='film'>title</p><script type='application/director-timeline+json'>{"format":"director-timeline/1",
           "tracks":[{"target":"film","property":"time","keyframes":[{"time":0,"value":0}]}]}</script>"#
            .to_string(),
        "needs children and no own content",
    );
    refused(
        page(REWIND, "").replace("<div id='late' data-start='1'></div>", "<div style='position:fixed'></div>"),
        "fixed box inside a time track",
    );
    refused(
        page(REWIND, "").replace("<div id='film'>", "<div id='film' style='color:red;animation:c 1s'>")
            .replace("</style>", "@keyframes c{to{color:blue}}</style>"),
        "text animations cannot pass through a time track",
    );
}
