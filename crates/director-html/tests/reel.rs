//! `data-reel` rolls each digit of an element's text through a clipped
//! window to its final digit, on positioned boxes beside text and hidden spans.

use director_html::{compile, CompileOptions, CompiledHtml};
use director_render::render_frame;

const W: i32 = 160;
const H: i32 = 48;
const GREEN: [u8; 4] = [0, 128, 0, 255];

fn options() -> CompileOptions {
    CompileOptions { width: W, height: H, duration: 4.0, ..Default::default() }
}

/// A white composition root with `root` attributes holding `body`.
fn page(root: &str, body: &str) -> String {
    format!(
        "<style>body{{margin:0}}#root{{position:relative;width:{W}px;height:{H}px;background:white;color:black;font-size:32px;line-height:1}}\
         p{{margin:0;text-align:center;font-variant-numeric:tabular-nums}}</style>\
         <div id='root' data-composition-id='main' data-width='{W}' data-height='{H}' {root}>{body}</div>"
    )
}

fn run(source: &str) -> CompiledHtml {
    compile(source, options()).unwrap_or_else(|e| panic!("{source}: {e}"))
}

fn refused(source: &str) -> String {
    compile(source, options()).err().expect(source).to_string()
}

fn frame(c: &mut CompiledHtml, time: f64) -> Vec<u8> {
    let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).unwrap();
    render_frame(&mut c.director, time, &mut surface).unwrap();
    let info = skia_safe::ImageInfo::new((W, H), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Unpremul, None);
    let mut pixels = vec![0; (W * H * 4) as usize];
    assert!(surface.read_pixels(&info, &mut pixels, W as usize * 4, (0, 0)));
    pixels
}

/// Pixels whose channels differ by more than a third of full scale.
fn differing(a: &[u8], b: &[u8]) -> usize {
    a.chunks(4).zip(b.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 85)).count()
}

/// The frame of a reel at `time` matches a reel parked on `text`, which has
/// the same windows.
fn shows(reel: &mut CompiledHtml, time: f64, text: &str) -> bool {
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();
    let parked = format!("<p data-reel data-reel-from='{digits}' data-reel-spins='0'>{text}</p>");
    let mut parked = run(&page("", &parked));
    differing(&frame(reel, time), &frame(&mut parked, 0.0)) == 0
}

/// Mean absolute grey difference between two frames.
fn distance(a: &[u8], b: &[u8]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x.abs_diff(*y) as f64).sum::<f64>() / a.len() as f64
}

#[test]
fn reels_roll_from_zero_to_the_final_digits() {
    let source = page("", "<p id='n' data-reel data-reel-start='0.5' data-reel-duration='1'>$2,048</p>");
    let mut c = run(&source);
    for i in 0..4 {
        assert!(c.element_ids.contains_key(&format!("n-reel-{i}")), "{i}");
    }
    assert!(shows(&mut c, 0.25, "$0,000"));
    assert!(!shows(&mut c, 0.9, "$0,000") && !shows(&mut c, 0.9, "$2,048"));
    assert!(shows(&mut c, 2.0, "$2,048"));
    // The windows set the digits where plain text sets them, up to sub-pixel glyph placement.
    let landed = frame(&mut c, 2.0);
    let near = distance(&landed, &frame(&mut run(&page("", "<p>$2,048</p>")), 0.0));
    let far = distance(&landed, &frame(&mut run(&page("", "<p>$0,000</p>")), 0.0));
    assert!(near < 6.0 && far > 2.0 * near, "{near} {far}");
}

#[test]
fn reels_take_a_start_digit_direction_and_beat_times() {
    // 120 BPM: two beats is 1 s and one beat 0.5 s.
    let reel = "<p data-reel data-reel-from='95' data-reel-direction='down' data-reel-start='2b' \
                data-reel-duration='1b' data-reel-stagger='0' data-reel-spins='0'>100%</p>";
    let mut c = run(&page("data-bpm='120'", reel));
    assert!(shows(&mut c, 0.9, "095%"));
    assert!(!shows(&mut c, 1.2, "095%") && !shows(&mut c, 1.2, "100%"));
    assert!(shows(&mut c, 1.6, "100%"));
}

#[test]
fn reel_seconds_count_from_the_clip_start() {
    let reel = "<div data-composition-id='inner' data-start='1'>\
                <p data-start='0.5' data-reel data-reel-start='0.5' data-reel-duration='0.5' data-reel-stagger='0.25'>42</p></div>";
    let mut c = run(&page("", reel));
    // The paragraph appears at 1.5 s and its reels start at 2 s; the second lands at 2.75 s.
    assert!(shows(&mut c, 1.9, "00"));
    assert!(!shows(&mut c, 2.6, "42"));
    assert!(shows(&mut c, 2.9, "42"));
    // A reel without its own data-start starts with its clip.
    let reel = "<div data-start='1'><p data-reel data-reel-duration='0.5' data-reel-stagger='0'>42</p></div>";
    let mut c = run(&page("", reel));
    assert!(shows(&mut c, 1.0, "00"));
    assert!(shows(&mut c, 1.6, "42"));
}

#[test]
fn positioned_boxes_sit_beside_text() {
    let mark = "<div style='position:absolute;left:0;top:0;width:16px;height:16px;background:rgb(0,128,0)'></div>";
    let block = format!("<p style='position:relative;text-align:left'>Hi{mark}</p>");
    let inline = format!("<p style='text-align:left'>A<span style='display:inline-block;position:relative'>B{mark}</span></p>");
    for body in [block, inline] {
        let mut c = run(&page("", &body));
        let pixels = frame(&mut c, 0.0);
        let at = |x: i32, y: i32| -> [u8; 4] { pixels[((y * W + x) * 4) as usize..][..4].try_into().unwrap() };
        assert!((0..W).any(|x| at(x, 4) == GREEN), "{body}");
    }
}

#[test]
fn hidden_spans_keep_their_advance() {
    let hidden = run(&page("", "<p>A<span style='visibility:hidden'>WW</span>B</p>"));
    let white = run(&page("", "<p>A<span style='color:white'>WW</span>B</p>"));
    let shown = run(&page("", "<p>A<span>WW</span>B</p>"));
    let (mut hidden, mut white, mut shown) = (hidden, white, shown);
    let hidden = frame(&mut hidden, 0.0);
    assert_eq!(differing(&hidden, &frame(&mut white, 0.0)), 0);
    assert!(differing(&hidden, &frame(&mut shown, 0.0)) > 50);
}

#[test]
fn staggered_digits_start_one_after_another() {
    // Each digit rolls for 1 s, the second starting 1 s after the first.
    let attrs = "data-reel data-reel-duration='1' data-reel-stagger='1' data-reel-spins='0' data-reel-ease='none'";
    let mut reel = run(&page("", &format!("<p {attrs}>11</p>")));
    assert!(shows(&mut reel, 1.0, "10"));
    assert!(shows(&mut reel, 2.5, "11"));
}

#[test]
fn reel_windows_fit_any_line_height_and_inherit_text_shadow() {
    // Ink bounds `[left, top, right, bottom]` of the dark pixels.
    let ink = |f: &[u8]| {
        let mut b = [usize::MAX, usize::MAX, 0, 0];
        for (i, _) in f.chunks(4).enumerate().filter(|(_, p)| p[0] < 128) {
            let (x, y) = (i % W as usize, i / W as usize);
            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
        }
        b
    };
    for style in ["line-height:1", "line-height:.9", "line-height:1.4", "white-space:nowrap", "line-height:.9;white-space:nowrap"] {
        let mut reel = run(&page("", &format!("<p style='{style}'><span data-reel>12</span></p>")));
        let mut text = run(&page("", &format!("<p style='{style}'><span>12</span></p>")));
        let (landed, plain) = (ink(&frame(&mut reel, 3.0)), ink(&frame(&mut text, 0.0)));
        // Taffy rounds the windows to whole pixels, so a fractional line moves by at most one.
        let close = landed.iter().zip(plain).all(|(a, b)| a.abs_diff(b) <= 1);
        assert!(close && landed[0] < landed[2], "{style}: landed {landed:?}, text {plain:?}");
    }
    let glow = |shadow: &str| {
        let mut c = run(&page("", &format!("<p style='text-shadow:{shadow}'><span data-reel>42</span></p>")));
        frame(&mut c, 3.0).chunks(4).filter(|p| p[0] > 200 && p[1] < 150 && p[2] < 150).count()
    };
    assert_eq!(glow("none"), 0);
    assert!(glow("2px 2px 0 #f00") > 20, "{}", glow("2px 2px 0 #f00"));
}

#[test]
fn reels_refuse_what_they_cannot_roll() {
    let cases = [
        ("<p data-reel='5'>12</p>", "data-reel takes no value"),
        ("<p data-reel>1<b>2</b></p>", "data-reel holds text only"),
        ("<p data-reel>abc</p>", "needs ASCII digits"),
        ("<p data-reel data-reel-from='123'>12</p>", "data-reel-from '123' needs 1 to 2 digits"),
        ("<p data-reel data-reel-spins='30'>12</p>", "data-reel-spins must be a whole number"),
        ("<p data-reel data-reel-direction='left'>12</p>", "data-reel-direction must be up or down"),
        ("<p data-reel data-reel-duration='0'>12</p>", "a positive duration"),
        ("<p data-reel data-reel-start='soon'>12</p>", "data-reel-start must be seconds or a beat time"),
        ("<p data-reel data-reel-start='1b'>12</p>", "beat times need data-bpm"),
        ("<p id='n' data-reel>12</p><i id='n-reel-1'></i>", "strip id 'n-reel-1' is already in use"),
    ];
    for (body, message) in cases {
        let error = refused(&page("", body));
        assert!(error.contains(message), "{message}: {error}");
    }
}
