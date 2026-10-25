use director_render::render_frame;
use director_html::{compile, CompileOptions};

fn frame(source: &str) -> Vec<u8> {
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
    render_frame(&mut c.director, 0.0, &mut surface).unwrap();
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

fn marked(decoration: &str, offset: &str) -> String {
    let offset = if offset.is_empty() {
        String::new()
    } else {
        format!("text-underline-offset:{offset};")
    };
    format!(
        "<style>body{{margin:0}}div{{width:64px;height:64px;color:white;font-size:32px;line-height:1;text-decoration:{decoration};text-decoration-color:red;{offset}}}</style><div>T</div>"
    )
}

fn red(sample: &[u8]) -> bool {
    sample[0] > 200 && sample[1] < 40 && sample[2] < 40
}

fn dark(sample: &[u8]) -> bool {
    sample[0] < 20 && sample[1] < 20 && sample[2] < 20
}

fn red_span(pixels: &[u8]) -> Option<(usize, usize)> {
    let mut top = 64usize;
    let mut bottom = 0usize;
    let mut seen = false;
    for y in 0..64 {
        for x in 0..64 {
            if red(pixel(pixels, x, y)) {
                seen = true;
                top = top.min(y);
                bottom = bottom.max(y);
            }
        }
    }
    seen.then_some((top, bottom))
}

#[test]
fn underline_offset_moves_below_the_default() {
    let auto = frame(&marked("underline", "auto"));
    let shifted = frame(&marked("underline", "8px"));
    let (_top, bottom) = red_span(&auto).expect("auto underline painted no red");
    let moved = (bottom + 1..64).any(|y| {
        (0..64).any(|x| red(pixel(&shifted, x, y)) && dark(pixel(&auto, x, y)))
    });
    assert!(
        moved,
        "8px offset did not paint red on an empty pixel below the default underline"
    );
}

#[test]
fn underline_offset_auto_and_zero_match_the_default() {
    let absent = frame(&marked("underline", ""));
    let auto = frame(&marked("underline", "auto"));
    let zero = frame(&marked("underline", "0"));
    assert_eq!(auto, absent);
    assert_eq!(zero, absent);
}

#[test]
fn underline_offset_percent_matches_one_em() {
    let length = frame(&marked("underline", "8px"));
    let percent = frame(&marked("underline", "25%"));
    assert_eq!(percent, length);
}

#[test]
fn negative_underline_offset_moves_up() {
    let auto = frame(&marked("underline", "auto"));
    let raised = frame(&marked("underline", "-8px"));
    let (top, _) = red_span(&auto).expect("auto underline painted no red");
    assert!(top > 0, "auto underline already touches the top");
    let moved = (0..top).any(|y| (0..64).any(|x| red(pixel(&raised, x, y))));
    assert!(moved, "negative offset did not paint red above the default underline");
}

#[test]
fn offset_leaves_overline_and_line_through() {
    for line in ["overline", "line-through"] {
        assert_eq!(
            frame(&marked(line, "8px")),
            frame(&marked(line, "")),
            "{line} moved"
        );
    }
}

#[test]
fn underline_offset_rejects_waffle() {
    let err = compile(
        &marked("underline", "waffle"),
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
    assert!(err.contains("unsupported text-underline-offset"), "{err}");
}
