// NOTE: Custom SkSL shaders are supported via EffectNode with RuntimeShader.
use crate::animation::Animated;
use crate::element::Element;
use crate::errors::RenderError;
use crate::node::{build_effect_filter, EffectType};
use crate::types::{Color, GradientConfig, GradientType};
use skia_safe::{
    gradient, shaders, Canvas, ClipOp, Paint, PaintStyle, Point, RRect, Rect, TileMode,
};
use std::any::Any;
use taffy::style::{AlignItems, FlexDirection, JustifyContent, Style};

// --- Box Node ---
/// A fundamental layout and styling block (div-like).
///
/// Supports background color, borders, shadows, rounded corners, gradients,
/// backdrop blur (glassmorphism), and multiple stacked shadows.
#[derive(Debug, Clone)]
pub struct BoxNode {
    pub style: Style,
    pub bg_color: Option<Animated<Color>>,
    pub opacity: Animated<f32>,
    pub blur: Animated<f32>,
    pub shadow_color: Option<Animated<Color>>,
    pub shadow_blur: Animated<f32>,
    pub shadow_offset_x: Animated<f32>,
    pub shadow_offset_y: Animated<f32>,
    // New fields
    pub border_radius: Animated<f32>,
    pub border_width: Animated<f32>,
    pub border_color: Option<Animated<Color>>,
    pub bg_gradient: Option<GradientConfig>,
    pub overflow: String,
    /// Backdrop blur radius for glassmorphism effect (blurs content behind this box).
    pub backdrop_blur: Animated<f32>,
    /// Additional stacked shadows for layered depth effects.
    pub extra_shadows: Vec<BoxShadow>,
    /// Optional clip-path polygon points (normalized 0..1) for non-rectangular clipping.
    /// e.g. [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]] creates a triangle clip.
    pub clip_path: Option<Vec<(f32, f32)>>,
}

/// A single box shadow definition for stacked/multiple shadows.
#[derive(Debug, Clone)]
pub struct BoxShadow {
    pub color: Animated<Color>,
    pub blur: Animated<f32>,
    pub offset_x: Animated<f32>,
    pub offset_y: Animated<f32>,
}

impl Default for BoxNode {
    fn default() -> Self {
        Self::new()
    }
}

impl BoxNode {
    pub fn new() -> Self {
        // Defaults to "Vertical Stack, Centered"
        let style = Style {
            flex_direction: FlexDirection::Column,
            align_items: Some(AlignItems::Center),
            justify_content: Some(JustifyContent::Center),
            ..Default::default()
        };

        Self {
            style,
            bg_color: None,
            opacity: Animated::new(1.0),
            blur: Animated::new(0.0),
            shadow_color: None,
            shadow_blur: Animated::new(0.0),
            shadow_offset_x: Animated::new(0.0),
            shadow_offset_y: Animated::new(0.0),
            border_radius: Animated::new(0.0),
            border_width: Animated::new(0.0),
            border_color: None,
            bg_gradient: None,
            overflow: "visible".to_string(),
            backdrop_blur: Animated::new(0.0),
            extra_shadows: Vec::new(),
            clip_path: None,
        }
    }
}

impl Element for BoxNode {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn kind(&self) -> &'static str {
        "box"
    }

    fn layout_style(&self) -> Style {
        self.style.clone()
    }

    fn set_layout_style(&mut self, style: Style) {
        self.style = style;
    }

    fn update(&mut self, time: f64) -> bool {
        let mut changed = false;
        if let Some(bg) = &mut self.bg_color {
            bg.update(time);
            changed = true;
        }
        if let Some(sc) = &mut self.shadow_color {
            sc.update(time);
            changed = true;
        }
        if let Some(bc) = &mut self.border_color {
            bc.update(time);
            changed = true;
        }
        self.opacity.update(time);
        self.blur.update(time);
        self.shadow_blur.update(time);
        self.shadow_offset_x.update(time);
        self.shadow_offset_y.update(time);
        self.border_radius.update(time);
        self.border_width.update(time);
        self.backdrop_blur.update(time);
        for shadow in &mut self.extra_shadows {
            shadow.color.update(time);
            shadow.blur.update(time);
            shadow.offset_x.update(time);
            shadow.offset_y.update(time);
        }
        changed
    }

    fn render(
        &self,
        canvas: &Canvas,
        rect: Rect,
        opacity: f32,
        draw_children: &mut dyn FnMut(&Canvas),
    ) -> Result<(), RenderError> {
        let local_opacity = self.opacity.current_value * opacity;
        let radius = self.border_radius.current_value;
        let rrect = RRect::new_rect_xy(rect, radius, radius);

        canvas.save();

        if self.overflow == "hidden" {
            canvas.clip_rrect(rrect, ClipOp::Intersect, true);
        }

        // Clip-path polygon: clip to a custom shape defined by normalized points
        if let Some(points) = &self.clip_path {
            if points.len() >= 3 {
                let mut path = skia_safe::PathBuilder::new();
                let first = points[0];
                path.move_to(skia_safe::Point::new(
                    rect.left + first.0 * rect.width(),
                    rect.top + first.1 * rect.height(),
                ));
                for pt in points.iter().skip(1) {
                    path.line_to(skia_safe::Point::new(
                        rect.left + pt.0 * rect.width(),
                        rect.top + pt.1 * rect.height(),
                    ));
                }
                path.close();
                canvas.clip_path(&path.detach(), ClipOp::Intersect, true);
            }
        }

        // Backdrop blur (glassmorphism): blur the content behind this box.
        // Uses a save layer with a backdrop image filter.
        // Track whether we actually created the layer so restore() stays balanced.
        let mut backdrop_layer_saved = false;
        if self.backdrop_blur.current_value > 0.0 {
            if let Some(backdrop_filter) = skia_safe::image_filters::blur(
                (
                    self.backdrop_blur.current_value,
                    self.backdrop_blur.current_value,
                ),
                TileMode::Decal,
                None,
                None,
            ) {
                canvas.save_layer(
                    &skia_safe::canvas::SaveLayerRec::default()
                        .bounds(&rect)
                        .backdrop(&backdrop_filter),
                );
                backdrop_layer_saved = true;
            }
        }

        let mut paint = Paint::default();
        paint.set_anti_alias(true);

        let mut effects = Vec::new();
        if self.blur.current_value > 0.0 {
            effects.push(EffectType::Blur(self.blur.clone()));
        }
        if let Some(sc) = &self.shadow_color {
            effects.push(EffectType::DropShadow {
                blur: self.shadow_blur.clone(),
                offset_x: self.shadow_offset_x.clone(),
                offset_y: self.shadow_offset_y.clone(),
                color: sc.clone(),
            });
        }
        // Add extra stacked shadows
        for shadow in &self.extra_shadows {
            effects.push(EffectType::DropShadow {
                blur: shadow.blur.clone(),
                offset_x: shadow.offset_x.clone(),
                offset_y: shadow.offset_y.clone(),
                color: shadow.color.clone(),
            });
        }

        // BoxNode effects don't use RuntimeShader for now, so we pass dummy resolution/time
        // Or we could pass proper ones if we wanted to support shaders on BoxNode later.
        // For now, these effects (Blur, DropShadow) ignore resolution/time.
        let filter = build_effect_filter(&effects, None, (rect.width(), rect.height()), 0.0);
        if let Some(f) = filter {
            paint.set_image_filter(f);
        }

        if let Some(gradient) = &self.bg_gradient {
            let skia_colors: Vec<skia_safe::Color4f> =
                gradient.colors.iter().map(|c| c.to_skia().into()).collect();
            let ramp = gradient::Gradient::new(
                gradient::Colors::new(
                    &skia_colors,
                    gradient.positions.as_deref(),
                    TileMode::Clamp,
                    None,
                ),
                gradient::Interpolation::default(),
            );

            let shader_opt = match gradient.gradient_type {
                GradientType::Linear => {
                    let start = Point::new(
                        rect.left + gradient.start.0 * rect.width(),
                        rect.top + gradient.start.1 * rect.height(),
                    );
                    let end = Point::new(
                        rect.left + gradient.end.0 * rect.width(),
                        rect.top + gradient.end.1 * rect.height(),
                    );
                    shaders::linear_gradient((start, end), &ramp, None)
                }
                GradientType::Radial => {
                    let center = Point::new(
                        rect.left + gradient.center.0 * rect.width(),
                        rect.top + gradient.center.1 * rect.height(),
                    );
                    // Radius relative to the shorter dimension
                    let min_dim = rect.width().min(rect.height());
                    let radius = gradient.radius * min_dim;
                    shaders::radial_gradient((center, radius.max(1.0)), &ramp, None)
                }
                GradientType::Conic => {
                    let center = Point::new(
                        rect.left + gradient.center.0 * rect.width(),
                        rect.top + gradient.center.1 * rect.height(),
                    );
                    shaders::sweep_gradient(
                        center,
                        (gradient.angle, gradient.angle + 360.0),
                        &ramp,
                        None,
                    )
                }
            };

            if let Some(shader) = shader_opt {
                let mut gradient_paint = Paint::default();
                gradient_paint.set_anti_alias(true);
                gradient_paint.set_shader(shader);
                gradient_paint.set_alpha_f(local_opacity);
                canvas.draw_rrect(rrect, &gradient_paint);
            }
        } else if let Some(bg) = &self.bg_color {
            let mut c = bg.current_value;
            c.a *= local_opacity;
            paint.set_color4f(c.to_color4f(), None);
            canvas.draw_rrect(rrect, &paint);
        }

        // Draw children (clipped if overflow: hidden)
        draw_children(canvas);

        // Restore backdrop blur save layer only if we actually created one
        if backdrop_layer_saved {
            canvas.restore();
        }

        canvas.restore(); // Restore clip and transform (except we didn't transform here, but clip)

        // Draw Border
        let bw = self.border_width.current_value;
        if bw > 0.0 {
            let mut border_paint = Paint::default();
            border_paint.set_anti_alias(true);
            border_paint.set_style(PaintStyle::Stroke);
            border_paint.set_stroke_width(bw);

            let color = if let Some(bc) = &self.border_color {
                bc.current_value
            } else {
                Color::BLACK
            };

            let mut c = color;
            c.a *= local_opacity;
            border_paint.set_color4f(c.to_color4f(), None);

            canvas.draw_rrect(rrect, &border_paint);
        }
        Ok(())
    }

    fn set_opacity(&mut self, opacity: f32) {
        self.opacity.current_value = opacity;
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
            "blur" => self.blur.add_segment(start, target, duration, ease_fn),
            "shadow_blur" => self
                .shadow_blur
                .add_segment(start, target, duration, ease_fn),
            "shadow_x" => self
                .shadow_offset_x
                .add_segment(start, target, duration, ease_fn),
            "shadow_y" => self
                .shadow_offset_y
                .add_segment(start, target, duration, ease_fn),
            "border_radius" => self
                .border_radius
                .add_segment(start, target, duration, ease_fn),
            "border_width" => self
                .border_width
                .add_segment(start, target, duration, ease_fn),
            "backdrop_blur" => self
                .backdrop_blur
                .add_segment(start, target, duration, ease_fn),
            other => {
                return Err(format!(
                    "unknown property '{other}' on box; valid: opacity, blur, shadow_blur, \
                     shadow_x, shadow_y, border_radius, border_width, backdrop_blur"
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
        let apply = |anim: &mut crate::animation::Animated<f32>| {
            if let Some(s) = start {
                anim.add_spring_with_start(s, target, config);
            } else {
                anim.add_spring(target, config);
            }
        };

        match property {
            "opacity" => apply(&mut self.opacity),
            "blur" => apply(&mut self.blur),
            "shadow_blur" => apply(&mut self.shadow_blur),
            "shadow_x" => apply(&mut self.shadow_offset_x),
            "shadow_y" => apply(&mut self.shadow_offset_y),
            "border_radius" => apply(&mut self.border_radius),
            "border_width" => apply(&mut self.border_width),
            "backdrop_blur" => apply(&mut self.backdrop_blur),
            other => {
                return Err(format!(
                    "spring: unknown property '{other}' on box; valid: opacity, blur, shadow_*, \
                     border_radius, border_width, backdrop_blur"
                ));
            }
        }
        Ok(())
    }
}
