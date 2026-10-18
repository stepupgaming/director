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

#[test]
fn mixed_url_and_gradient_background_errors() {
    let err = compile(
        &format!(
            "<style>body{{margin:0}}div{{width:64px;height:64px;background-image:url(\"data:image/png;base64,{RED_PNG}\"),linear-gradient(red,blue)}}</style><div></div>"
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
    assert!(err.contains("cannot mix"), "{err}");
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
