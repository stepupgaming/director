use crate::animation::Animated;
use crate::element::Element;
use crate::errors::RenderError;
use crate::node::{calculate_object_fit_rect_at, parse_easing};
use crate::types::{ObjectFit, ObjectPosition};
use skia_safe::{Canvas, ClipOp, Color4f, Data, Image, Paint, Rect, RRect};
use std::any::Any;
use taffy::style::Style;

// --- Image Node ---
/// A node that renders a static raster image (PNG, JPG, etc.).
#[derive(Debug, Clone)]
pub struct ImageNode {
    pub image: Option<Image>,
    pub opacity: Animated<f32>,
    pub style: Style,
    pub object_fit: ObjectFit,
    pub object_position: ObjectPosition,
    pub border_radius: Animated<f32>,
    /// Original asset path, if loaded from a file or blob path.
    pub source_path: Option<String>,
}

impl ImageNode {
    pub fn new(data: Vec<u8>) -> Self {
        let image = Image::from_encoded(Data::new_copy(&data));
        Self {
            image,
            opacity: Animated::new(1.0),
            style: Style::DEFAULT,
            object_fit: ObjectFit::Cover,
            object_position: ObjectPosition::default(),
            border_radius: Animated::new(0.0),
            source_path: None,
        }
    }

    pub fn from_image(image: Option<Image>) -> Self {
        Self {
            image,
            opacity: Animated::new(1.0),
            style: Style::DEFAULT,
            object_fit: ObjectFit::Cover,
            object_position: ObjectPosition::default(),
            border_radius: Animated::new(0.0),
            source_path: None,
        }
    }

    pub fn from_image_with_path(image: Option<Image>, path: impl Into<String>) -> Self {
        Self {
            image,
            opacity: Animated::new(1.0),
            style: Style::DEFAULT,
            object_fit: ObjectFit::Cover,
            object_position: ObjectPosition::default(),
            border_radius: Animated::new(0.0),
            source_path: Some(path.into()),
        }
    }
}

impl Element for ImageNode {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn kind(&self) -> &'static str {
        "image"
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
        self.border_radius.update(time);
        true
    }

    fn render(
        &self,
        canvas: &Canvas,
        rect: Rect,
        parent_opacity: f32,
        draw_children: &mut dyn FnMut(&Canvas),
    ) -> Result<(), RenderError> {
        let op = self.opacity.current_value * parent_opacity;
        let mut paint = Paint::new(Color4f::new(1.0, 1.0, 1.0, op), None);
        paint.set_anti_alias(true);

        if let Some(img) = &self.image {
            let sampling = skia_safe::SamplingOptions::new(
                skia_safe::FilterMode::Linear,
                skia_safe::MipmapMode::Linear,
            );

            let draw_rect = calculate_object_fit_rect_at(
                img.width() as f32,
                img.height() as f32,
                rect,
                self.object_fit,
                self.object_position,
            );

            canvas.save();
            let radius = self.border_radius.current_value;
            if radius > 0.0 {
                let rrect = RRect::new_rect_xy(&rect, radius, radius);
                canvas.clip_rrect(rrect, ClipOp::Intersect, true);
            }
            canvas.clip_rect(rect, ClipOp::Intersect, true);
            canvas.draw_image_rect_with_sampling_options(img, None, draw_rect, sampling, &paint);
            canvas.restore();
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
        let ease_fn = crate::scripting::utils::parse_easing_result(easing)?;
        match property {
            "opacity" => self.opacity.add_segment(start, target, duration, ease_fn),
            "border_radius" => self.border_radius.add_segment(start, target, duration, ease_fn),
            other => {
                return Err(format!(
                    "unknown property '{other}' on image; valid: opacity, border_radius"
                ));
            }
        }
        Ok(())
    }

    fn animate_property_spring(
        &mut self,
        property: &str,
        start: Option<f32>,
        target: f32,
        config: crate::animation::SpringConfig,
    ) -> Result<(), String> {
        match property {
            "opacity" => {
                if let Some(s) = start {
                    self.opacity.add_spring_with_start(s, target, config);
                } else {
                    self.opacity.add_spring(target, config);
                }
            }
            "border_radius" => {
                if let Some(s) = start {
                    self.border_radius.add_spring_with_start(s, target, config);
                } else {
                    self.border_radius.add_spring(target, config);
                }
            }
            other => {
                return Err(format!(
                    "spring: unknown property '{other}' on image; valid: opacity, border_radius"
                ));
            }
        }
        Ok(())
    }
}
