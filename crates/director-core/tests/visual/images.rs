// Image Node Visual Regression Tests
//
// Tests for image rendering with different ObjectFit modes (Cover, Contain, Fill)
// and opacity. Uses a programmatically generated test image to avoid external
// asset dependencies.

use director_core::animation::Animated;
use director_core::node::{BoxNode, ImageNode};
use director_core::types::{Color, ObjectFit};
use taffy::style::Dimension;

/// Generates a 120×80 PNG with three distinct color bands (red, green, blue)
/// and vertical brightness variation, so ObjectFit scaling/cropping is clearly
/// visible in visual regression snapshots.
fn create_test_image() -> Vec<u8> {
    let w = 120u32;
    let h = 80u32;
    let mut img = image::RgbaImage::new(w, h);

    for x in 0..w {
        for y in 0..h {
            let band = x * 3 / w; // 0=red, 1=green, 2=blue
            let brightness = 0.4 + 0.6 * (1.0 - y as f32 / h as f32);
            let (r, g, b) = match band {
                0 => (brightness, 0.1 * brightness, 0.1 * brightness),
                1 => (0.1 * brightness, brightness, 0.1 * brightness),
                _ => (0.1 * brightness, 0.1 * brightness, brightness),
            };
            img.put_pixel(
                x,
                y,
                image::Rgba([
                    (r * 255.0) as u8,
                    (g * 255.0) as u8,
                    (b * 255.0) as u8,
                    255,
                ]),
            );
        }
    }

    let mut buf = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut buf),
            image::ImageFormat::Png,
        )
        .expect("Failed to encode test image as PNG");
    buf
}

/// Helper: set up a 200×200 director with a white root and an image child
/// using the given ObjectFit. Returns the Arc<Mutex<Director>>.
fn setup_image_director(fit: ObjectFit) -> std::sync::Arc<std::sync::Mutex<director_core::Director>> {
    use crate::visual::setup_test_director;

    let director_arc = setup_test_director(200, 200);
    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.9, 0.9, 0.9, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.push_scene_root(root_id, 5.0);

        let png_bytes = create_test_image();
        let mut image_node = ImageNode::new(png_bytes);
        image_node.object_fit = fit;
        image_node.style.size.width = Dimension::percent(1.0);
        image_node.style.size.height = Dimension::percent(1.0);
        let img_id = d.scene.add_node(Box::new(image_node));
        d.scene.add_child(root_id, img_id);
    }
    director_arc
}

/// ObjectFit::Cover — image scaled to fill, excess cropped.
#[test]
fn image_cover() {
    use crate::visual::assert_visual_match;

    let director_arc = setup_image_director(ObjectFit::Cover);
    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "images", "image_cover").unwrap();
}

/// ObjectFit::Contain — image scaled to fit, letterboxed.
#[test]
fn image_contain() {
    use crate::visual::assert_visual_match;

    let director_arc = setup_image_director(ObjectFit::Contain);
    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "images", "image_contain").unwrap();
}

/// ObjectFit::Fill — image stretched to fill (distorted).
#[test]
fn image_fill() {
    use crate::visual::assert_visual_match;

    let director_arc = setup_image_director(ObjectFit::Fill);
    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "images", "image_fill").unwrap();
}

/// Image at 40% opacity over a colored background.
#[test]
fn image_opacity() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(200, 200);
    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.2, 0.6, 0.8, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.push_scene_root(root_id, 5.0);

        let png_bytes = create_test_image();
        let mut image_node = ImageNode::new(png_bytes);
        image_node.object_fit = ObjectFit::Cover;
        image_node.opacity = Animated::new(0.4);
        image_node.style.size.width = Dimension::percent(1.0);
        image_node.style.size.height = Dimension::percent(1.0);
        let img_id = d.scene.add_node(Box::new(image_node));
        d.scene.add_child(root_id, img_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "images", "image_opacity").unwrap();
}
