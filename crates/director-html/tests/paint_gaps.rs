use director_core::systems::renderer::render_frame;
use director_html::{compile, CompileOptions};

const RED_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAYAAADED76LAAAAEklEQVR4nGP4z8DwHx9mGBkKAMLXf4EvceABAAAAAElFTkSuQmCC";

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
    render_frame(&mut c.director, 0.0, surface.canvas()).unwrap();
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
fn file_background_uses_compile_base_dir() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let home = tempfile::tempdir().unwrap();
    let empty = tempfile::tempdir().unwrap();
    let bytes = STANDARD.decode(RED_PNG).unwrap();
    std::fs::write(home.path().join("spot.png"), &bytes).unwrap();
    let source = "<style>body{margin:0}div{width:64px;height:64px;background-image:url(\"spot.png\");background-repeat:no-repeat;background-size:auto}</style><div></div>";
    let mut options = CompileOptions {
        width: 64,
        height: 64,
        duration: 3.0,
        base_dir: home.path().to_path_buf(),
        ..Default::default()
    };
    let mut compiled = compile(source, options.clone()).unwrap();
    let mut surface = skia_safe::surfaces::raster_n32_premul((64, 64)).unwrap();
    render_frame(&mut compiled.director, 0.0, surface.canvas()).unwrap();
    let info = skia_safe::ImageInfo::new(
        (64, 64),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let mut pixels = vec![0; 64 * 64 * 4];
    assert!(surface.read_pixels(&info, &mut pixels, 64 * 4, (0, 0)));
    let ink = pixel(&pixels, 2, 2);
    assert!(ink[0] > 200 && ink[1] < 30 && ink[2] < 30, "{ink:?}");
    options.base_dir = empty.path().to_path_buf();
    let err = compile(source, options).err().unwrap().to_string();
    assert!(err.contains("spot.png"), "{err}");
}

fn image_box(extra: &str) -> String {
    format!(
        "<style>body{{margin:0}}div{{box-sizing:border-box;width:64px;height:64px;background-image:url(\"data:image/png;base64,{RED_PNG}\");{extra}}}</style><div></div>"
    )
}

#[test]
fn image_background_is_visible_and_leaves_the_padding_offset() {
    let pixels = frame(&image_box(
        "padding:16px;background-color:blue;background-repeat:no-repeat;background-position:16px 16px;background-size:auto",
    ));
    let ink = pixel(&pixels, 20, 20);
    assert!(ink[0] > 200 && ink[1] < 30 && ink[2] < 30, "{ink:?}");
    let padding = pixel(&pixels, 4, 4);
    assert!(
        padding[2] > 200 && padding[0] < 30 && padding[1] < 30,
        "{padding:?}"
    );
}

#[test]
fn content_box_origin_insets_past_the_border_and_padding() {
    let pixels = frame(&image_box(
        "border:8px solid blue;padding:16px;background-color:blue;background-repeat:no-repeat;background-position:left top;background-size:auto;background-origin:content-box",
    ));
    let padding = pixel(&pixels, 10, 10);
    assert!(
        padding[2] > 200 && padding[0] < 30 && padding[1] < 30,
        "{padding:?}"
    );
    let ink = pixel(&pixels, 26, 26);
    assert!(ink[0] > 200 && ink[1] < 30 && ink[2] < 30, "{ink:?}");
}

#[test]
fn no_repeat_leaves_the_far_corner_at_the_page() {
    let pixels = frame(&image_box(
        "background-repeat:no-repeat;background-position:left top;background-size:auto",
    ));
    let ink = pixel(&pixels, 3, 3);
    assert!(ink[0] > 200 && ink[1] < 30 && ink[2] < 30, "{ink:?}");
    let corner = pixel(&pixels, 63, 63);
    assert!(
        corner[0] < 20 && corner[1] < 20 && corner[2] < 20,
        "{corner:?}"
    );
}

#[test]
fn dashed_underline_leaves_a_gap_that_solid_fills() {
    let source = |decoration: &str| {
        format!(
            "<style>body{{margin:0}}div{{width:64px;height:64px;color:white;font-size:32px;line-height:1;text-decoration:{decoration}}}</style><div>T</div>"
        )
    };
    let solid = frame(&source("underline"));
    let dashed = frame(&source("underline dashed"));
    let mut left = 64usize;
    let mut right = 0usize;
    let mut bottom = 0usize;
    let mut seen = false;
    for y in 0..64 {
        for x in 0..64 {
            let sample = pixel(&solid, x, y);
            if sample[0] > 40 || sample[1] > 40 || sample[2] > 40 {
                seen = true;
                left = left.min(x);
                right = right.max(x);
                bottom = bottom.max(y);
            }
        }
    }
    assert!(seen, "solid underline did not paint");
    let mut bright = false;
    let mut dark = false;
    for y in bottom.saturating_sub(4)..=bottom {
        for x in left..=right {
            let solid_px = pixel(&solid, x, y);
            if solid_px[0] < 200 {
                continue;
            }
            let dashed_px = pixel(&dashed, x, y);
            if dashed_px[0] > 200 {
                bright = true;
            } else if dashed_px[0] < 30 {
                dark = true;
            }
        }
    }
    assert!(bright && dark, "dashed underline missing ink or gap at bottom {bottom}");
}

#[test]
fn underline_changes_a_pixel_under_the_glyph() {
    let plain = frame(
        "<style>body{margin:0}div{width:64px;height:64px;color:white;font-size:32px;line-height:1;text-decoration:none}</style><div>T</div>",
    );
    let marked = frame(
        "<style>body{margin:0}div{width:64px;height:64px;color:white;font-size:32px;line-height:1;text-decoration:underline}</style><div>T</div>",
    );
    let mut left = 64usize;
    let mut right = 0usize;
    let mut bottom = 0usize;
    let mut seen = false;
    for y in 0..64 {
        for x in 0..64 {
            let sample = pixel(&plain, x, y);
            if sample[0] > 40 || sample[1] > 40 || sample[2] > 40 {
                seen = true;
                left = left.min(x);
                right = right.max(x);
                bottom = bottom.max(y);
            }
        }
    }
    assert!(seen, "glyph did not paint");
    let mut changed = false;
    for y in bottom..=(bottom + 16).min(63) {
        for x in left..=right {
            let before = pixel(&plain, x, y);
            let after = pixel(&marked, x, y);
            if after[0] as i16 >= before[0] as i16 + 80 {
                changed = true;
            }
        }
    }
    assert!(changed, "underline missing under glyph bottom {bottom}");
}

#[test]
fn decoration_color_paints_the_underline_red() {
    let marked = frame(
        "<style>body{margin:0}div{width:64px;height:64px;color:white;font-size:32px;line-height:1;text-decoration:underline;text-decoration-color:red}</style><div>T</div>",
    );
    let mut left = 64usize;
    let mut right = 0usize;
    let mut top = 64usize;
    let mut bottom = 0usize;
    let mut seen = false;
    for y in 0..64 {
        for x in 0..64 {
            let sample = pixel(&marked, x, y);
            if sample[0] > 40 || sample[1] > 40 || sample[2] > 40 {
                seen = true;
                left = left.min(x);
                right = right.max(x);
                top = top.min(y);
                bottom = bottom.max(y);
            }
        }
    }
    assert!(seen, "decorated glyph did not paint");
    let mut red = false;
    let mut white = false;
    for y in top..=bottom {
        for x in left..=right {
            let sample = pixel(&marked, x, y);
            if sample[0] > 200 && sample[1] < 30 && sample[2] < 30 {
                red = true;
            }
            if sample[0] > 200 && sample[1] > 200 && sample[2] > 200 {
                white = true;
            }
        }
    }
    assert!(red && white, "red underline or white glyph missing in {top}..={bottom}");
}

#[test]
fn luminance_mask_is_more_transparent_on_black_than_alpha() {
    let html = |mode: &str| {
        format!(
            "<style>body{{margin:0}}div{{width:64px;height:64px;background:red;mask-image:linear-gradient(90deg,white,black);mask-mode:{mode}}}</style><div></div>"
        )
    };
    let luminance = frame(&html("luminance"));
    let alpha = frame(&html("alpha"));
    let lum_black = pixel(&luminance, 60, 32);
    let alpha_black = pixel(&alpha, 60, 32);
    let lum_white = pixel(&luminance, 4, 32);
    assert!(
        lum_black[0] as i16 + 80 <= alpha_black[0] as i16,
        "black side {lum_black:?} vs alpha {alpha_black:?}"
    );
    assert!(
        lum_white[0] as i16 >= lum_black[0] as i16 + 80,
        "white side {lum_white:?} vs black side {lum_black:?}"
    );
}

fn ink_bottom(pixels: &[u8]) -> usize {
    let mut bottom = 0usize;
    for y in 0..64 {
        for x in 0..64 {
            let sample = pixel(pixels, x, y);
            if sample[0] > 40 || sample[1] > 40 || sample[2] > 40 {
                bottom = y;
            }
        }
    }
    bottom
}

#[test]
fn overflow_wrap_breaks_a_long_token() {
    let source = |wrap: &str, text: &str| {
        format!(
            "<style>body{{margin:0}}div{{width:64px;height:64px;color:white;font-size:16px;line-height:1;white-space:pre-wrap;{wrap}}}</style><div>{text}</div>"
        )
    };
    let wrapped = frame(&source(
        "overflow-wrap:anywhere",
        "MMMMMMMMMMMMMMMMMMMMMMMM",
    ));
    let normal = frame(&source("overflow-wrap:normal", "MMMMMMMMMMMMMMMMMMMMMMMM"));
    let break_word = frame(&source(
        "overflow-wrap:break-word",
        "MMMMMMMMMMMMMMMMMMMMMMMM",
    ));
    let word_break = frame(&source("word-break:break-word", "MMMMMMMMMMMMMMMMMMMMMMMM"));
    assert!(
        ink_bottom(&normal) < 24,
        "normal bottom {}",
        ink_bottom(&normal)
    );
    assert!(ink_bottom(&wrapped) > ink_bottom(&normal) + 8);
    assert!(ink_bottom(&break_word) > ink_bottom(&normal) + 8);
    assert!(ink_bottom(&word_break) > ink_bottom(&normal) + 8);
    let spaced = frame(&source("overflow-wrap:normal", "MMMM MMMM MMMM MMMM"));
    assert!(
        ink_bottom(&spaced) > 24,
        "spaced bottom {}",
        ink_bottom(&spaced)
    );
    let fitted = "<style>body{margin:0}div{width:64px;height:64px;color:white;font-size:16px;line-height:1;white-space:pre-wrap;";
    assert_eq!(
        frame(&format!(
            "{fitted}overflow-wrap:anywhere}}</style><div>ab cd</div>"
        )),
        frame(&format!(
            "{fitted}overflow-wrap:normal}}</style><div>ab cd</div>"
        ))
    );
    let options = CompileOptions {
        width: 64,
        height: 64,
        duration: 3.0,
        ..Default::default()
    };
    let err = compile(
        "<style>div{overflow-wrap:break-spaces}</style><div>ab</div>",
        options.clone(),
    )
    .err()
    .unwrap()
    .to_string();
    assert!(err.contains("unsupported overflow-wrap"), "{err}");
    let err = compile(
        "<style>div{word-break:keep-all}</style><div>ab</div>",
        options,
    )
    .err()
    .unwrap()
    .to_string();
    assert!(err.contains("unsupported word-break"), "{err}");
}

#[test]
fn text_overflow_ellipsis_changes_the_line_end() {
    let source = |extra: &str, text: &str| {
        format!(
            "<style>body{{margin:0}}div{{width:32px;height:64px;color:white;font-size:16px;line-height:1;white-space:nowrap;{extra}}}</style><div>{text}</div>"
        )
    };
    let word = "MMMMMMMMMMMMMMMMMMMMMMMM";
    let dots = frame(&source("overflow:hidden;text-overflow:ellipsis", word));
    let clip = frame(&source("overflow:hidden;text-overflow:clip", word));
    assert_ne!(dots, clip, "ellipsis matched clip");
    assert!(ink_bottom(&dots) < 20 && ink_bottom(&clip) < 20, "wrapped");
    let right_differs = (12..32).any(|x| (0..20).any(|y| pixel(&dots, x, y) != pixel(&clip, x, y)));
    assert!(right_differs, "right edge matched");
    let spilled = frame(&source("text-overflow:ellipsis", word));
    let spill = (40..64).any(|x| (0..24).any(|y| pixel(&spilled, x, y)[0] > 40));
    assert!(spill, "visible overflow was ellipsized");
    assert_eq!(
        frame(&source("overflow:hidden;text-overflow:ellipsis", "M")),
        frame(&source("overflow:hidden;text-overflow:clip", "M"))
    );
    let err = compile(
        "<style>div{text-overflow:fade}</style><div>ab</div>",
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
    assert!(err.contains("unsupported text-overflow"), "{err}");
}

#[test]
fn background_clip_text_keeps_the_gradient_inside_the_glyph() {
    let source = |clip: &str| {
        format!(
            "<style>body{{margin:0}}div{{width:64px;height:64px;color:transparent;font-size:48px;line-height:1;background-image:linear-gradient(red,red);background-clip:{clip}}}</style><div>M</div>"
        )
    };
    let text = frame(&source("text"));
    let boxed = frame(&source("border-box"));
    let corner = pixel(&text, 2, 2);
    assert!(
        corner[0] < 20 && corner[1] < 20 && corner[2] < 20,
        "corner {corner:?}"
    );
    let box_corner = pixel(&boxed, 2, 2);
    assert!(box_corner[0] > 200, "box corner {box_corner:?}");
    assert!(
        text.chunks(4)
            .any(|sample| sample[0] > 200 && sample[1] < 40 && sample[2] < 40),
        "no red glyph"
    );
    let err = compile(
        "<style>div{background-clip:margin-box}</style><div>ab</div>",
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
    assert!(err.contains("unsupported background-clip"), "{err}");
}

fn text_box(text: &str, transform: &str) -> String {
    format!(
        "<style>body{{margin:0}}div{{width:64px;height:64px;color:white;font-size:16px;line-height:1;text-transform:{transform}}}</style><div>{text}</div>"
    )
}

#[test]
fn text_transform_matches_the_authored_case() {
    let upper = frame(&text_box("ab", "uppercase"));
    assert_eq!(upper, frame(&text_box("AB", "none")));
    assert_ne!(upper, frame(&text_box("ab", "none")));
    assert_eq!(
        frame(&text_box("AB", "lowercase")),
        frame(&text_box("ab", "none"))
    );
    assert_eq!(
        frame(&text_box("ab cd", "capitalize")),
        frame(&text_box("Ab Cd", "none"))
    );
    assert_eq!(
        frame(&text_box("ab CD", "capitalize")),
        frame(&text_box("Ab CD", "none"))
    );
    let err = compile(
        "<style>div{text-transform:full-width}</style><div>ab</div>",
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
    assert!(err.contains("unsupported text-transform"), "{err}");
}

#[test]
fn url_and_gradient_paint_in_css_order() {
    let image_on_top = frame(&format!(
        "<style>body{{margin:0}}div{{width:64px;height:64px;background-image:url(\"data:image/png;base64,{RED_PNG}\"),linear-gradient(blue,blue);background-repeat:no-repeat;background-position:left top;background-size:auto}}</style><div></div>"
    ));
    let top = pixel(&image_on_top, 2, 2);
    let far = pixel(&image_on_top, 40, 40);
    assert!(top[0] > 200 && top[1] < 30 && top[2] < 30, "{top:?}");
    assert!(far[2] > 200 && far[0] < 30 && far[1] < 30, "{far:?}");
    let gradient_on_top = frame(&format!(
        "<style>body{{margin:0}}div{{width:64px;height:64px;background-image:linear-gradient(blue,blue),url(\"data:image/png;base64,{RED_PNG}\");background-repeat:no-repeat;background-position:left top;background-size:auto}}</style><div></div>"
    ));
    let covered = pixel(&gradient_on_top, 2, 2);
    assert!(
        covered[2] > 200 && covered[0] < 30 && covered[1] < 30,
        "{covered:?}"
    );
    let err = compile(
        &format!(
            "<style>div{{background-image:url(\"data:image/png;base64,{RED_PNG}\"),url(\"data:image/png;base64,{RED_PNG}\")}}</style><div></div>"
        ),
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
    assert!(err.contains("background-image accepts a single url"), "{err}");
}

#[test]
fn text_shadow_paints_red_beside_white_text() {
    let plain = frame(
        "<style>body{margin:0}div{width:64px;height:64px;color:white;font-size:32px;line-height:1}</style><div>T</div>",
    );
    let none = frame(
        "<style>body{margin:0}div{width:64px;height:64px;color:white;font-size:32px;line-height:1;text-shadow:none}</style><div>T</div>",
    );
    assert_eq!(plain, none);
    assert_eq!(red_pixels(&plain), 0);
    let marked = frame(
        "<style>body{margin:0}div{width:64px;height:64px;color:white;font-size:32px;line-height:1;text-shadow:20px 0 0 red}</style><div>T</div>",
    );
    assert!(red_pixels(&marked) > 0, "text-shadow painted no red");
    assert!(
        marked.chunks(4).any(|sample| sample[0] > 200 && sample[1] > 200 && sample[2] > 200),
        "white glyph missing"
    );
    let err = compile(
        "<style>div{text-shadow:2px 2px red, 4px 4px blue}</style><div>T</div>",
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
    assert!(err.contains("text-shadow accepts a single shadow"), "{err}");
}

fn red_pixels(pixels: &[u8]) -> usize {
    pixels
        .chunks(4)
        .filter(|sample| sample[0] > 200 && sample[1] < 30 && sample[2] < 30)
        .count()
}

#[test]
fn from_font_underline_is_thinner_than_auto() {
    let source = |thickness: &str| {
        format!(
            "<style>body{{margin:0}}div{{width:64px;height:64px;color:white;font-size:64px;line-height:1;text-decoration:underline;text-decoration-color:red;text-decoration-thickness:{thickness}}}</style><div>T</div>"
        )
    };
    let auto = red_pixels(&frame(&source("auto")));
    let from_font = red_pixels(&frame(&source("from-font")));
    assert!(auto > 0, "auto underline painted no red");
    assert!(from_font > 0, "from-font underline painted no red");
    assert!(
        from_font < auto,
        "from-font {from_font} red pixels, auto {auto}"
    );
}

#[test]
fn space_leaves_a_gap_that_round_fills() {
    let space = frame(&image_box(
        "background-color:blue;background-repeat:space;background-size:30px 30px",
    ));
    let round = frame(&image_box(
        "background-color:blue;background-repeat:round;background-size:30px 30px",
    ));
    let space_ink = pixel(&space, 2, 2);
    let space_gap = pixel(&space, 31, 2);
    assert!(
        space_ink[0] > 200 && space_ink[1] < 30 && space_ink[2] < 30,
        "{space_ink:?}"
    );
    assert!(
        space_gap[2] > 200 && space_gap[0] < 30 && space_gap[1] < 30,
        "{space_gap:?}"
    );
    let round_ink = pixel(&round, 2, 2);
    let round_join = pixel(&round, 31, 2);
    assert!(
        round_ink[0] > 200 && round_ink[1] < 30 && round_ink[2] < 30,
        "{round_ink:?}"
    );
    assert!(
        round_join[0] > 200 && round_join[1] < 30 && round_join[2] < 30,
        "{round_join:?}"
    );
}

#[test]
fn space_ignores_position_when_two_tiles_fit() {
    let pixels = frame(&image_box(
        "background-color:blue;background-repeat:space;background-position:100% 100%;background-size:30px 30px",
    ));
    let ink = pixel(&pixels, 2, 2);
    assert!(ink[0] > 200 && ink[1] < 30 && ink[2] < 30, "{ink:?}");
}

#[test]
fn content_box_clip_keeps_the_image_origin_and_clears_the_padding() {
    let pixels = frame(&image_box(
        "border:8px solid blue;padding:16px;background-color:blue;background-repeat:no-repeat;background-position:left top;background-size:auto;background-origin:content-box;background-clip:content-box",
    ));
    let border = pixel(&pixels, 4, 20);
    assert!(
        border[2] > 200 && border[0] < 30 && border[1] < 30,
        "{border:?}"
    );
    let padding = pixel(&pixels, 12, 12);
    assert!(
        padding[0] < 20 && padding[1] < 20 && padding[2] < 20,
        "{padding:?}"
    );
    let ink = pixel(&pixels, 26, 26);
    assert!(ink[0] > 200 && ink[1] < 30 && ink[2] < 30, "{ink:?}");
}

#[test]
fn padding_box_clip_clears_the_border_and_keeps_the_padding() {
    let pixels = frame(&image_box(
        "border:8px solid transparent;padding:16px;background-color:blue;background-repeat:no-repeat;background-position:left top;background-size:auto;background-origin:content-box;background-clip:padding-box",
    ));
    let border = pixel(&pixels, 4, 20);
    assert!(
        border[0] < 20 && border[1] < 20 && border[2] < 20,
        "{border:?}"
    );
    let padding = pixel(&pixels, 12, 12);
    assert!(
        padding[2] > 200 && padding[0] < 30 && padding[1] < 30,
        "{padding:?}"
    );
    let ink = pixel(&pixels, 26, 26);
    assert!(ink[0] > 200 && ink[1] < 30 && ink[2] < 30, "{ink:?}");
}

#[test]
fn border_box_clip_paints_under_a_transparent_border() {
    let pixels = frame(&image_box(
        "border:8px solid transparent;padding:16px;background-color:blue;background-repeat:no-repeat;background-position:left top;background-size:auto;background-origin:content-box;background-clip:border-box",
    ));
    let border = pixel(&pixels, 4, 20);
    assert!(
        border[2] > 200 && border[0] < 30 && border[1] < 30,
        "{border:?}"
    );
    let padding = pixel(&pixels, 12, 12);
    assert!(
        padding[2] > 200 && padding[0] < 30 && padding[1] < 30,
        "{padding:?}"
    );
}

#[test]
fn repeat_x_inside_a_pair_still_fails() {
    let err = compile(
        &image_box("background-repeat:repeat-x repeat"),
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
    assert!(err.contains("background-repeat"), "{err}");
}

#[test]
fn gradient_sizes_to_the_padding_box_and_clamps_across_the_border() {
    let source = |origin: &str| {
        format!(
            "<style>body{{margin:0}}div{{box-sizing:border-box;width:64px;height:64px;border:24px solid transparent;background-image:linear-gradient(90deg,red,blue);{origin}}}</style><div></div>"
        )
    };
    let padding = frame(&source(""));
    let left = pixel(&padding, 20, 32);
    let right = pixel(&padding, 44, 32);
    assert!(left[0] > 240 && left[2] < 20, "{left:?}");
    assert!(right[0] < 20 && right[2] > 240, "{right:?}");
    let border = frame(&source("background-origin:border-box"));
    let mixed = pixel(&border, 20, 32);
    assert!(
        mixed[0] > 100 && mixed[2] > 40 && mixed[0] < 220,
        "{mixed:?}"
    );
}
