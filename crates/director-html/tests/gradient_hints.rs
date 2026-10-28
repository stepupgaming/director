use director_render::render_frame;
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
fn pixel(p: &[u8], x: usize, y: usize) -> &[u8] {
    &p[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4]
}
fn html(background: &str) -> String {
    format!(
        "<style>body{{margin:0}}div{{width:64px;height:64px;background:{background}}}</style><div></div>"
    )
}

#[test]
fn linear_hint_pulls_the_midpoint_toward_blue() {
    let hinted = frame(&html("linear-gradient(90deg, red, 25%, blue)"), 0.0);
    let plain = frame(&html("linear-gradient(90deg, red, blue)"), 0.0);
    let mid = pixel(&hinted, 32, 32);
    assert!(mid[0] < 110, "{mid:?}");
    assert!(mid[2] > 140, "{mid:?}");
    let quarter = pixel(&hinted, 16, 32);
    assert!(quarter[0] > 80 && quarter[2] > 80, "{quarter:?}");
    let plain_mid = pixel(&plain, 32, 32);
    assert!(
        (plain_mid[0] as i16 - plain_mid[2] as i16).abs() <= 20,
        "{plain_mid:?}"
    );
    assert!(
        mid[0] as i16 + 30 <= plain_mid[0] as i16,
        "{mid:?} vs {plain_mid:?}"
    );
}

#[test]
fn fifty_percent_linear_hint_matches_the_unhinted_ramp() {
    let hinted = frame(&html("linear-gradient(90deg, red, 50%, blue)"), 0.0);
    let plain = frame(&html("linear-gradient(90deg, red, blue)"), 0.0);
    let hinted_px = pixel(&hinted, 32, 32);
    let plain_px = pixel(&plain, 32, 32);
    for channel in 0..4 {
        assert!(
            (hinted_px[channel] as i16 - plain_px[channel] as i16).abs() <= 8,
            "{channel}: {hinted_px:?} vs {plain_px:?}"
        );
    }
}

#[test]
fn radial_hint_mixes_before_the_unhinted_circle() {
    let hinted = frame(
        &html("radial-gradient(circle at 50% 50%, red, 20%, blue)"),
        0.0,
    );
    let plain = frame(&html("radial-gradient(circle at 50% 50%, red, blue)"), 0.0);
    // `circle` is farthest-corner, so x=38 is about 20% of the way to the edge.
    let hinted_mix = pixel(&hinted, 38, 32);
    let plain_mix = pixel(&plain, 38, 32);
    assert!(
        hinted_mix[0] as i16 + 20 <= plain_mix[0] as i16
            && hinted_mix[2] as i16 >= plain_mix[2] as i16 + 20,
        "more mixed {hinted_mix:?} vs {plain_mix:?}"
    );
    let hinted_blue = pixel(&hinted, 48, 32);
    let plain_blue = pixel(&plain, 48, 32);
    assert!(
        hinted_blue[2] as i16 >= plain_blue[2] as i16 + 20,
        "more blue {hinted_blue:?} vs {plain_blue:?}"
    );
}

#[test]
fn conic_hint_changes_the_sweep() {
    let hinted = frame(&html("conic-gradient(from 0deg, red, 25%, blue)"), 0.0);
    let plain = frame(&html("conic-gradient(from 0deg, red, blue)"), 0.0);
    let mut best = 0i16;
    for (x, y) in [(62, 32), (32, 62), (2, 32), (32, 2)] {
        let hinted_px = pixel(&hinted, x, y);
        let plain_px = pixel(&plain, x, y);
        for channel in 0..3 {
            best = best.max((hinted_px[channel] as i16 - plain_px[channel] as i16).abs());
        }
    }
    assert!(best >= 30, "conic hint delta {best}");
}

#[test]
fn repeating_linear_hint_still_paints_later_periods() {
    let hinted = frame(
        &html("repeating-linear-gradient(90deg, red, 25%, blue 16px)"),
        0.0,
    );
    let plain = frame(
        &html("repeating-linear-gradient(90deg, red, blue 16px)"),
        0.0,
    );
    assert_ne!(
        pixel(&hinted, 8, 32),
        pixel(&plain, 8, 32),
        "first period {:?} vs {:?}",
        pixel(&hinted, 8, 32),
        pixel(&plain, 8, 32)
    );
    assert_ne!(pixel(&hinted, 24, 32), &[0, 0, 0, 255]);
}

#[test]
fn vendor_gradients_fail() {
    let error = compile(
        &html("-webkit-linear-gradient(red, blue)"),
        CompileOptions {
            width: 64,
            height: 64,
            duration: 3.0,
            ..Default::default()
        },
    )
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains("vendor"), "{error}");
}

#[test]
fn orphan_color_hint_fails() {
    let error = compile(
        &html("linear-gradient(red, 10%)"),
        CompileOptions {
            width: 64,
            height: 64,
            duration: 3.0,
            ..Default::default()
        },
    )
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains("color hint"), "{error}");
}

#[test]
fn corner_linear_gradient_hits_the_opposite_corners() {
    let wide = |gradient: &str| {
        format!(
            "<style>body{{margin:0}}div{{width:64px;height:32px;background:{gradient}}}</style><div></div>"
        )
    };
    let corner = frame(&wide("linear-gradient(to bottom right, red, blue)"), 0.0);
    let swapped = frame(&wide("linear-gradient(to right bottom, red, blue)"), 0.0);
    let angled = frame(&wide("linear-gradient(45deg, red, blue)"), 0.0);
    let top_left = pixel(&corner, 1, 1);
    assert!(top_left[0] > 200 && top_left[2] < 40, "{top_left:?}");
    let bottom_right = pixel(&corner, 62, 30);
    assert!(
        bottom_right[2] > 200 && bottom_right[0] < 40,
        "{bottom_right:?}"
    );
    assert_eq!(corner, swapped);
    assert_ne!(corner, angled, "corner matched 45deg");
    let square = |gradient: &str| {
        format!(
            "<style>body{{margin:0}}div{{width:64px;height:64px;background:{gradient}}}</style><div></div>"
        )
    };
    let diagonal = frame(&square("linear-gradient(to bottom right, red, blue)"), 0.0);
    let deg = frame(&square("linear-gradient(135deg, red, blue)"), 0.0);
    for (x, y) in [(1usize, 1usize), (32, 32), (62, 62)] {
        let a = pixel(&diagonal, x, y);
        let b = pixel(&deg, x, y);
        for channel in 0..3 {
            assert!(
                (a[channel] as i16 - b[channel] as i16).abs() <= 8,
                "{x},{y} {a:?} vs {b:?}"
            );
        }
    }
}

#[test]
fn stops_past_the_gradient_line_keep_their_falloff() {
    // The circle reaches 64px at 200%, so 28px out sits 0.44 of the way to blue.
    let p = frame(&html("radial-gradient(circle closest-side at 50% 50%, red 0%, blue 200%)"), 0.0);
    let edge = pixel(&p, 60, 32);
    assert!((125..165).contains(&edge[0]) && (90..130).contains(&edge[2]), "{edge:?}");
    // A stop before the start leaves its interpolated colour at the start.
    let p = frame(&html("linear-gradient(to right, red -100%, blue 100%)"), 0.0);
    let start = pixel(&p, 0, 32);
    assert!((110..145).contains(&start[0]) && (110..145).contains(&start[2]), "{start:?}");
}
