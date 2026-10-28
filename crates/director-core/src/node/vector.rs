use crate::element::Element;
use skia_safe::{AlphaType, Canvas, ColorSpace, ColorType, Data, Image, Paint, Rect};
use std::any::Any;
use std::sync::{Arc, Mutex};
use taffy::style::Style;
use tiny_skia::{IntRect, Pixmap, Transform};
use usvg::{Group, Node, Options, Tree};

/// Largest raster padding per side, as a multiple of the output extent.
const MAX_LAYER_PADDING: u32 = 2;

/// Pixels to add left of and above the SVG raster so nested layers keep their paint.
///
/// resvg clamps every isolated group layer (clip-path, mask, opacity, filter) to
/// `-2 * size .. 3 * size` of the target pixmap, measured in the parent layer's own
/// coordinates. A layer reaching past `-2 * size` on the left or top gets its origin
/// pulled in, and the clamp of each deeper layer then sits further left in target
/// coordinates. From the third nested layer on, the right or bottom clamp falls inside
/// the visible area and paint vanishes. Right and bottom reach never move a layer
/// origin, so only left and top reach need padding.
///
/// resvg floors a layer edge and grows it by 2 px, so a layer escapes the clamp when
/// `floor(reach) + pad - 2 >= -2 * (extent + pad)`. The padding keeps a 1 px margin
/// over that bound and stops at `MAX_LAYER_PADDING` times the extent.
fn layer_padding(tree: &Tree, sx: f32, sy: f32, width: u32, height: u32) -> (u32, u32) {
    /// Lowest left and top edge, in canvas units, of any isolated group layer.
    fn reach(group: &Group, min: &mut (f32, f32)) {
        if group.should_isolate() {
            let bbox = group.abs_layer_bounding_box();
            min.0 = min.0.min(bbox.left());
            min.1 = min.1.min(bbox.top());
        }
        for node in group.children() {
            if let Node::Group(child) = node {
                reach(child, min);
            }
        }
    }

    let mut min = (0.0f32, 0.0f32);
    reach(tree.root(), &mut min);
    let pad = |edge: f32, extent: u32| {
        let need = (3.0 - 2.0 * extent as f32 - edge.floor()) / 3.0;
        if need > 0.0 {
            (need.ceil() as u32).min(extent.saturating_mul(MAX_LAYER_PADDING))
        } else {
            0
        }
    };
    (pad(min.0 * sx, width), pad(min.1 * sy, height))
}

/// A node that renders scalable vector graphics (SVG).
///
/// Rasterizes the SVG to a bitmap at the exact size required by the layout, ensuring crisp rendering.
#[derive(Debug)]
pub struct VectorNode {
    tree: Arc<Tree>,
    cache: Mutex<Option<(u32, u32, Image)>>,
    pub style: Style,
    pub opacity: crate::animation::Animated<f32>,
}

impl Clone for VectorNode {
    fn clone(&self) -> Self {
        Self {
            tree: self.tree.clone(),
            cache: Mutex::new(None), // Don't share cache across clones (or maybe we should? No, size might differ)
            style: self.style.clone(),
            opacity: self.opacity.clone(),
        }
    }
}

impl VectorNode {
    pub fn new(data: &[u8]) -> Self {
        let opt = Options::default();
        // usvg 0.44: Tree::from_data
        let tree = Tree::from_data(data, &opt).expect("Failed to parse SVG");
        Self::from_tree(tree)
    }

    /// Render a tree that has already resolved text into paths.
    pub fn from_tree(tree: Tree) -> Self {
        Self {
            tree: Arc::new(tree),
            cache: Mutex::new(None),
            style: Style::DEFAULT,
            opacity: crate::animation::Animated::new(1.0),
        }
    }

    /// The resolved SVG tree.
    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    /// Replace the resolved tree and drop the size-keyed pixmap.
    pub fn set_tree(&mut self, tree: Tree) {
        self.tree = Arc::new(tree);
        *self.cache.lock().unwrap() = None;
    }
}

/// How many device pixels one layout pixel of `rect` covers, from 1 to 8 in
/// half-octave steps, and at most 4096 pixels on a side.
fn device_scale(canvas: &Canvas, rect: Rect) -> f32 {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return 1.0;
    }
    let shown = canvas.local_to_device_as_3x3().map_rect(rect).0;
    let raw = (shown.width() / rect.width()).max(shown.height() / rect.height());
    if !raw.is_finite() || raw <= 1.0 {
        return 1.0;
    }
    let stepped = 2f32.powf((raw.log2() * 2.0).ceil() / 2.0).min(8.0);
    stepped.min(4096.0 / rect.width().max(rect.height()))
}

impl Element for VectorNode {
    fn export_body(
        &self,
        cx: &mut crate::scene_export::ExportContext,
    ) -> Result<crate::scene_export::ElementExport, crate::scene_export::ExportError> {
        crate::scene_export::nodes::vector_body(self, cx)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn kind(&self) -> &'static str {
        "vector"
    }

    fn moves_between(&self, t0: f64, t1: f64) -> bool {
        self.opacity.moves_between(t0, t1)
    }

    fn layout_style(&self) -> Style {
        self.style.clone()
    }

    fn set_layout_style(&mut self, style: Style) {
        self.style = style;
    }

    fn set_opacity(&mut self, opacity: f32) {
        self.opacity.current_value = opacity;
    }

    fn update(&mut self, time: f64) -> bool {
        self.opacity.update(time);
        true
    }

    fn render(
        &self,
        canvas: &Canvas,
        rect: Rect,
        parent_opacity: f32,
        draw_children: &mut dyn FnMut(&Canvas),
    ) -> Result<(), crate::RenderError> {
        // Rasterize at the size the canvas shows, so a scaled or perspective
        // camera keeps the vector sharp. Steps of a half octave bound re-rasters.
        let scale = device_scale(canvas, rect);
        let width = (rect.width() * scale).ceil() as u32;
        let height = (rect.height() * scale).ceil() as u32;

        if width == 0 || height == 0 {
            draw_children(canvas);
            return Ok(());
        }

        let mut cache_guard = self.cache.lock().unwrap();

        // Check if cache is valid
        let needs_update = if let Some((w, h, _)) = *cache_guard {
            w != width || h != height
        } else {
            true
        };

        if needs_update {
            // Rasterize
            let tree_size = self.tree.size();
            let sx = width as f32 / tree_size.width();
            let sy = height as f32 / tree_size.height();

            // Render into a padded pixmap so resvg's layer clamp sits past any left or
            // top reach, then keep the window the layout asked for.
            let (pad_x, pad_y) = layer_padding(&self.tree, sx, sy, width, height);
            let transform = Transform::from_row(sx, 0.0, 0.0, sy, pad_x as f32, pad_y as f32);

            let pixmap = Pixmap::new(width + pad_x, height + pad_y).and_then(|mut pixmap| {
                resvg::render(&self.tree, transform, &mut pixmap.as_mut());
                if pad_x == 0 && pad_y == 0 {
                    Some(pixmap)
                } else {
                    pixmap.clone_rect(IntRect::from_xywh(
                        pad_x as i32,
                        pad_y as i32,
                        width,
                        height,
                    )?)
                }
            });

            if let Some(pixmap) = pixmap {
                // Convert to Skia Image
                let data = Data::new_copy(pixmap.data());
                let image_info = skia_safe::ImageInfo::new(
                    (width as i32, height as i32),
                    ColorType::RGBA8888,
                    AlphaType::Premul,
                    Some(ColorSpace::new_srgb()),
                );

                if let Some(img) =
                    skia_safe::images::raster_from_data(&image_info, data, (width * 4) as usize)
                {
                    *cache_guard = Some((width, height, img));
                }
            }
        }

        // Draw
        let final_opacity = self.opacity.current_value * parent_opacity;
        let mut paint = Paint::default();
        paint.set_alpha_f(final_opacity);

        if let Some((_, _, img)) = cache_guard.as_ref() {
            let sampling = skia_safe::SamplingOptions::new(skia_safe::FilterMode::Linear, skia_safe::MipmapMode::None);
            canvas.draw_image_rect_with_sampling_options(img, None, rect, sampling, &paint);
        }

        draw_children(canvas);
        Ok(())
    }

    fn animate_property(
        &mut self,
        property: &str,
        start: f32,
        target: f32,
        duration: f64,
        easing: &str,
    ) -> Result<(), String> {
        match property {
            "opacity" => {
                let ease = crate::scripting::utils::parse_easing_result(easing)?;
                self.opacity.add_segment(start, target, duration, ease);
                Ok(())
            }
            other => Err(format!(
                "unknown property '{other}' on vector; valid: opacity"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: i32 = 120;

    /// Three nested clip-path groups, each clipping to the 40x40 viewBox, around `content`.
    fn nested_clip_svg(content: &str) -> String {
        format!(
            "<svg xmlns='http://www.w3.org/2000/svg' width='40' height='40' viewBox='0 0 40 40'>\
             <defs><clipPath id='c'><rect width='40' height='40'/></clipPath></defs>\
             <g clip-path='url(#c)'><g clip-path='url(#c)'><g clip-path='url(#c)'>{content}</g></g></g>\
             </svg>"
        )
    }

    /// Renders `svg` at 3x into a SIZE x SIZE surface and returns unpremultiplied RGBA.
    fn render(svg: &str) -> Vec<u8> {
        let node = VectorNode::new(svg.as_bytes());
        let mut surface = skia_safe::surfaces::raster_n32_premul((SIZE, SIZE)).unwrap();
        node.render(
            surface.canvas(),
            Rect::from_wh(SIZE as f32, SIZE as f32),
            1.0,
            &mut |_| {},
        )
        .unwrap();
        let info = skia_safe::ImageInfo::new(
            (SIZE, SIZE),
            ColorType::RGBA8888,
            AlphaType::Unpremul,
            None,
        );
        let mut pixels = vec![0; (SIZE * SIZE * 4) as usize];
        assert!(surface.read_pixels(&info, &mut pixels, (SIZE * 4) as usize, (0, 0)));
        pixels
    }

    fn assert_red(pixels: &[u8], label: &str) {
        for (x, y) in [(2, 2), (60, 60), (117, 2), (2, 117), (117, 117)] {
            let i = ((y * SIZE + x) * 4) as usize;
            assert_eq!(
                &pixels[i..i + 4],
                &[255, 0, 0, 255],
                "{label}: pixel ({x}, {y})"
            );
        }
    }

    #[test]
    fn nested_clip_layers_keep_paint_that_reaches_far_left() {
        // The rect starts 360 px left of the 120 px raster, past resvg's -2 * width layer clamp.
        let svg = nested_clip_svg("<rect x='-120' width='160' height='40' fill='#f00'/>");
        assert_red(&render(&svg), "left reach");
    }

    #[test]
    fn nested_clip_layers_keep_paint_that_reaches_far_up() {
        let svg = nested_clip_svg("<rect y='-120' width='40' height='160' fill='#f00'/>");
        assert_red(&render(&svg), "top reach");
    }

    #[test]
    fn layer_padding_is_zero_inside_the_clamp() {
        let svg = nested_clip_svg("<rect x='-60' width='100' height='40' fill='#f00'/>");
        let node = VectorNode::new(svg.as_bytes());
        assert_eq!(layer_padding(&node.tree, 3.0, 3.0, 120, 120), (0, 0));
        assert_red(&render(&svg), "control");
    }
}
