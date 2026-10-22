//! CSS object-position for image destination rects.
//!
//! Default 50% 50% must match the centered rect from `calculate_object_fit_rect`.

use director_core::element::Element;
use director_core::node::{calculate_object_fit_rect, calculate_object_fit_rect_at, ImageNode};
use director_core::types::{ObjectFit, ObjectPosition};
use skia_safe::Rect;

/// 8×8 solid red PNG (standard base64).
const RED_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAYAAADED76LAAAAEklEQVR4nGP4z8DwHx9mGBkKAMLXf4EvceABAAAAAElFTkSuQmCC";

#[test]
fn default_object_position_matches_centered_fit() {
    let sizes = [(8.0, 8.0, 32.0, 16.0), (8.0, 8.0, 16.0, 32.0)];
    let fits = [ObjectFit::Fill, ObjectFit::Contain, ObjectFit::Cover];
    for (src_w, src_h, dst_w, dst_h) in sizes {
        let dst = Rect::from_xywh(0.0, 0.0, dst_w, dst_h);
        for fit in fits {
            let centered = calculate_object_fit_rect(src_w, src_h, dst, fit);
            let positioned =
                calculate_object_fit_rect_at(src_w, src_h, dst, fit, ObjectPosition::default());
            assert_eq!(
                positioned, centered,
                "{fit:?} of {src_w}x{src_h} into {dst_w}x{dst_h}"
            );
        }
    }
}

#[test]
fn contain_object_position_places_red_pixels() {
    let rect = Rect::from_xywh(0.0, 0.0, 32.0, 16.0);

    let at_origin = render_contain(
        ObjectPosition {
            x_percent: 0.0,
            x_offset: 0.0,
            y_percent: 0.0,
            y_offset: 0.0,
        },
        32,
        16,
        rect,
    );
    assert_red(pixel(&at_origin, 32, 4, 8));
    assert_transparent(pixel(&at_origin, 32, 28, 8));

    let at_end_x = render_contain(
        ObjectPosition {
            x_percent: 1.0,
            x_offset: 0.0,
            y_percent: 0.0,
            y_offset: 0.0,
        },
        32,
        16,
        rect,
    );
    assert_transparent(pixel(&at_end_x, 32, 4, 8));
    assert_red(pixel(&at_end_x, 32, 28, 8));

    let centered = render_contain(ObjectPosition::default(), 32, 16, rect);
    assert_transparent(pixel(&centered, 32, 4, 8));
    assert_red(pixel(&centered, 32, 20, 8));
    assert_transparent(pixel(&centered, 32, 28, 8));
}

#[test]
fn contain_offset_is_clipped_to_element_rect() {
    let pixels = render_contain(
        ObjectPosition {
            x_percent: 0.0,
            x_offset: 24.0,
            y_percent: 0.0,
            y_offset: 0.0,
        },
        48,
        16,
        Rect::from_xywh(0.0, 0.0, 32.0, 16.0),
    );
    assert_transparent(pixel(&pixels, 48, 40, 8));
    assert_red(pixel(&pixels, 48, 28, 8));
}

fn render_contain(position: ObjectPosition, width: i32, height: i32, rect: Rect) -> Vec<u8> {
    let mut node = ImageNode::new(decode_standard_base64(RED_PNG_BASE64));
    let image = node.image.as_ref().expect("red png decodes");
    assert_eq!((image.width(), image.height()), (8, 8));
    node.object_fit = ObjectFit::Contain;
    node.object_position = position;

    let mut surface =
        skia_safe::surfaces::raster_n32_premul((width, height)).expect("raster surface");
    surface.canvas().clear(skia_safe::Color::TRANSPARENT);
    let mut draw_children = |_canvas: &skia_safe::Canvas| {};
    node.render(surface.canvas(), rect, 1.0, &mut draw_children)
        .expect("image render");

    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let info = skia_safe::ImageInfo::new_n32_premul((width, height), None);
    assert!(surface.read_pixels(&info, &mut pixels, (width * 4) as usize, (0, 0)));
    pixels
}

fn pixel(pixels: &[u8], width: i32, x: i32, y: i32) -> [u8; 4] {
    let i = ((y * width + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

fn assert_red(px: [u8; 4]) {
    let rgba = px[0] > 200 && px[1] < 50 && px[2] < 50;
    let bgra = px[2] > 200 && px[1] < 50 && px[0] < 50;
    assert!(
        px[3] > 200 && (rgba || bgra),
        "expected red, got {px:?}"
    );
}

fn assert_transparent(px: [u8; 4]) {
    assert_eq!(px[3], 0, "expected transparent, got {px:?}");
}

fn decode_standard_base64(input: &str) -> Vec<u8> {
    fn val(c: u8) -> u8 {
        match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("invalid standard base64 byte {c}"),
        }
    }

    let bytes = input.as_bytes();
    assert!(bytes.len().is_multiple_of(4), "standard base64 length");
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.as_chunks::<4>().0 {
        let pad = chunk.iter().filter(|&&c| c == b'=').count();
        let n = [
            val(chunk[0]),
            val(if chunk[1] == b'=' { b'A' } else { chunk[1] }),
            val(if chunk[2] == b'=' { b'A' } else { chunk[2] }),
            val(if chunk[3] == b'=' { b'A' } else { chunk[3] }),
        ];
        let triple = ((n[0] as u32) << 18)
            | ((n[1] as u32) << 12)
            | ((n[2] as u32) << 6)
            | (n[3] as u32);
        out.push((triple >> 16) as u8);
        if pad < 2 {
            out.push((triple >> 8) as u8);
        }
        if pad < 1 {
            out.push(triple as u8);
        }
    }
    out
}
