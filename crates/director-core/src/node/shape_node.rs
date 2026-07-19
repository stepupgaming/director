use crate::animation::Animated;
use crate::element::Element;
use crate::errors::RenderError;
use crate::node::parse_easing;
use crate::types::{Color, GradientConfig};
use skia_safe::{Canvas, Paint, PaintStyle, Path, Point, Rect, TileMode};
use std::any::Any;
use taffy::geometry::Size;
use taffy::style::{AvailableSpace, Style};

/// A single path drawing command.
#[derive(Debug, Clone)]
pub enum PathCommand {
    MoveTo { x: f32, y: f32 },
    LineTo { x: f32, y: f32 },
    QuadTo { cx: f32, cy: f32, x: f32, y: f32 },
    CubicTo {
        c1x: f32,
        c1y: f32,
        c2x: f32,
        c2y: f32,
        x: f32,
        y: f32,
    },
    ArcTo {
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        start_angle: f32,
        sweep_angle: f32,
    },
    Close,
}

/// The type of shape to draw.
#[derive(Debug, Clone)]
pub enum ShapeType {
    /// Circle centered at (cx, cy) with radius r.
    Circle { cx: f32, cy: f32, r: f32 },
    /// Ellipse centered at (cx, cy) with radii rx, ry.
    Ellipse { cx: f32, cy: f32, rx: f32, ry: f32 },
    /// Custom path built from commands.
    Path { commands: Vec<PathCommand> },
}

/// A node that draws geometric shapes directly via Skia canvas calls.
///
/// Supports fill (solid or gradient), stroke, and opacity.
#[derive(Debug, Clone)]
pub struct ShapeNode {
    pub style: Style,
    pub shape: ShapeType,
    pub fill_color: Option<Animated<Color>>,
    pub fill_gradient: Option<GradientConfig>,
    pub stroke_color: Option<Animated<Color>>,
    pub stroke_width: Animated<f32>,
    pub opacity: Animated<f32>,
}

impl ShapeNode {
    pub fn new(shape: ShapeType) -> Self {
        Self {
            style: Style::default(),
            shape,
            fill_color: None,
            fill_gradient: None,
            stroke_color: None,
            stroke_width: Animated::new(0.0),
            opacity: Animated::new(1.0),
        }
    }

    /// Build a Skia Path from the shape definition, offset into the layout rect.
    fn build_skia_path(&self, rect: &Rect) -> Path {
        let mut path = Path::new();
        match &self.shape {
            ShapeType::Circle { cx, cy, r } => {
                let center = Point::new(rect.left + cx, rect.top + cy);
                path.add_circle(center, *r, skia_safe::PathDirection::CW);
            }
            ShapeType::Ellipse { cx, cy, rx, ry } => {
                let oval = Rect::from_xywh(rect.left + cx - rx, rect.top + cy - ry, rx * 2.0, ry * 2.0);
                path.add_oval(&oval, Some((skia_safe::PathDirection::CW, 1)));
            }
            ShapeType::Path { commands } => {
                for cmd in commands {
                    match cmd {
                        PathCommand::MoveTo { x, y } => {
                            path.move_to((rect.left + x, rect.top + y));
                        }
                        PathCommand::LineTo { x, y } => {
                            path.line_to((rect.left + x, rect.top + y));
                        }
                        PathCommand::QuadTo { cx, cy, x, y } => {
                            path.quad_to(
                                (rect.left + cx, rect.top + cy),
                                (rect.left + x, rect.top + y),
                            );
                        }
                        PathCommand::CubicTo {
                            c1x,
                            c1y,
                            c2x,
                            c2y,
                            x,
                            y,
                        } => {
                            path.cubic_to(
                                (rect.left + c1x, rect.top + c1y),
                                (rect.left + c2x, rect.top + c2y),
                                (rect.left + x, rect.top + y),
                            );
                        }
                        PathCommand::ArcTo {
                            cx,
                            cy,
                            rx,
                            ry,
                            start_angle,
                            sweep_angle,
                        } => {
                            let oval = Rect::from_xywh(
                                rect.left + cx - rx,
                                rect.top + cy - ry,
                                rx * 2.0,
                                ry * 2.0,
                            );
                            path.arc_to(
                                &oval,
                                *start_angle,
                                *sweep_angle,
                                false,
                            );
                        }
                        PathCommand::Close => {
                            path.close();
                        }
                    }
                }
            }
        }
        path
    }

    /// Configure a paint for filling the shape.
    fn configure_fill_paint(&self, paint: &mut Paint, local_opacity: f32, rect: &Rect) {
        paint.set_anti_alias(true);
        paint.set_style(PaintStyle::Fill);

        if let Some(gradient) = &self.fill_gradient {
            let skia_colors: Vec<skia_safe::Color> =
                gradient.colors.iter().map(|c| c.to_skia()).collect();
            let start = Point::new(
                rect.left + gradient.start.0 * rect.width(),
                rect.top + gradient.start.1 * rect.height(),
            );
            let end = Point::new(
                rect.left + gradient.end.0 * rect.width(),
                rect.top + gradient.end.1 * rect.height(),
            );
            if let Some(shader) = skia_safe::gradient_shader::linear(
                (start, end),
                skia_colors.as_slice(),
                gradient.positions.as_deref(),
                TileMode::Clamp,
                None,
                None,
            ) {
                paint.set_shader(shader);
            }
        } else if let Some(fill) = &self.fill_color {
            let mut c = fill.current_value;
            c.a *= local_opacity;
            paint.set_color4f(c.to_color4f(), None);
        }
    }

    /// Configure a paint for stroking the shape.
    fn configure_stroke_paint(&self, paint: &mut Paint, local_opacity: f32) {
        paint.set_anti_alias(true);
        paint.set_style(PaintStyle::Stroke);
        paint.set_stroke_width(self.stroke_width.current_value);

        if let Some(stroke) = &self.stroke_color {
            let mut c = stroke.current_value;
            c.a *= local_opacity;
            paint.set_color4f(c.to_color4f(), None);
        } else {
            paint.set_color(skia_safe::Color::BLACK);
        }
    }
}

impl Element for ShapeNode {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout_style(&self) -> Style {
        self.style.clone()
    }

    fn needs_measure(&self) -> bool {
        true
    }

    fn measure(
        &self,
        _known_dimensions: Size<Option<f32>>,
        _available_space: Size<AvailableSpace>,
    ) -> Size<f32> {
        match &self.shape {
            ShapeType::Circle { cx, cy, r } => Size {
                width: (cx + r).ceil(),
                height: (cy + r).ceil(),
            },
            ShapeType::Ellipse { cx, cy, rx, ry } => Size {
                width: (cx + rx).ceil(),
                height: (cy + ry).ceil(),
            },
            ShapeType::Path { commands } => {
                let mut min_x = f32::INFINITY;
                let mut min_y = f32::INFINITY;
                let mut max_x = f32::NEG_INFINITY;
                let mut max_y = f32::NEG_INFINITY;
                for cmd in commands {
                    match cmd {
                        PathCommand::MoveTo { x, y }
                        | PathCommand::LineTo { x, y } => {
                            min_x = min_x.min(*x);
                            min_y = min_y.min(*y);
                            max_x = max_x.max(*x);
                            max_y = max_y.max(*y);
                        }
                        PathCommand::QuadTo { cx, cy, x, y } => {
                            min_x = min_x.min(*cx).min(*x);
                            min_y = min_y.min(*cy).min(*y);
                            max_x = max_x.max(*cx).max(*x);
                            max_y = max_y.max(*cy).max(*y);
                        }
                        PathCommand::CubicTo { c1x, c1y, c2x, c2y, x, y } => {
                            min_x = min_x.min(*c1x).min(*c2x).min(*x);
                            min_y = min_y.min(*c1y).min(*c2y).min(*y);
                            max_x = max_x.max(*c1x).max(*c2x).max(*x);
                            max_y = max_y.max(*c1y).max(*c2y).max(*y);
                        }
                        PathCommand::ArcTo { cx, cy, rx, ry, .. } => {
                            min_x = min_x.min(cx - rx);
                            min_y = min_y.min(cy - ry);
                            max_x = max_x.max(cx + rx);
                            max_y = max_y.max(cy + ry);
                        }
                        PathCommand::Close => {}
                    }
                }
                if min_x.is_finite() {
                    Size {
                        width: max_x.ceil(),
                        height: max_y.ceil(),
                    }
                } else {
                    Size::ZERO
                }
            }
        }
    }

    fn update(&mut self, time: f64) -> bool {
        let mut changed = false;
        if let Some(fill) = &mut self.fill_color {
            fill.update(time);
            changed = true;
        }
        if let Some(stroke) = &mut self.stroke_color {
            stroke.update(time);
            changed = true;
        }
        self.opacity.update(time);
        self.stroke_width.update(time);
        changed
    }

    fn render(
        &self,
        canvas: &Canvas,
        rect: Rect,
        opacity: f32,
        _draw_children: &mut dyn FnMut(&Canvas),
    ) -> Result<(), RenderError> {
        let local_opacity = self.opacity.current_value * opacity;
        let path = self.build_skia_path(&rect);

        // Fill
        let has_fill = self.fill_color.is_some() || self.fill_gradient.is_some();
        if has_fill {
            let mut fill_paint = Paint::default();
            self.configure_fill_paint(&mut fill_paint, local_opacity, &rect);
            canvas.draw_path(&path, &fill_paint);
        }

        // Stroke
        if self.stroke_width.current_value > 0.0 {
            let mut stroke_paint = Paint::default();
            self.configure_stroke_paint(&mut stroke_paint, local_opacity);
            canvas.draw_path(&path, &stroke_paint);
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
            "stroke_width" => self.stroke_width.add_segment(start, target, duration, ease_fn),
            other => {
                return Err(format!(
                    "unknown property '{other}' on shape; valid: opacity, stroke_width"
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
        let apply = |anim: &mut Animated<f32>| {
            if let Some(s) = start {
                anim.add_spring_with_start(s, target, config);
            } else {
                anim.add_spring(target, config);
            }
        };
        match property {
            "opacity" => apply(&mut self.opacity),
            "stroke_width" => apply(&mut self.stroke_width),
            other => {
                return Err(format!(
                    "spring: unknown property '{other}' on shape; valid: opacity, stroke_width"
                ));
            }
        }
        Ok(())
    }

    fn animate_color(
        &mut self,
        start: Color,
        end: Color,
        duration: f64,
        easing: &str,
    ) {
        let ease_fn = parse_easing(easing);
        if let Some(fill) = &mut self.fill_color {
            fill.add_segment(start, end, duration, ease_fn);
        }
    }

    fn set_color(&mut self, color: Color) {
        if let Some(fill) = &mut self.fill_color {
            *fill = Animated::new(color);
        }
    }
}
