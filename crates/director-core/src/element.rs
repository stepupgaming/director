//! # Element Module
//!
//! The core trait that all visual nodes must implement.
//!
//! ## Responsibilities
//! - **Element Trait**: Defines `update`, `render`, `layout_style`, `measure`.
//! - **Text Types**: `TextSpan`, `TextShadow`, `TextFit` for rich text.
//!
//! ## Key Types
//! - `Element`: The trait every node type implements.
//! - `TextSpan`: A styled span for rich text rendering.

use crate::types::{Color, GradientConfig};
use skia_safe::{Canvas, Rect};
use std::any::Any;
use taffy::geometry::Size;
use taffy::style::AvailableSpace;
use taffy::style::Style;
use tracing::warn;

/// Represents a span of text with specific styling properties.
///
/// Used for Rich Text rendering where different parts of a string have different styles.
#[derive(Clone, Debug, Default)]
pub struct TextSpan {
    pub text: String,
    pub color: Option<Color>,
    pub font_family: Option<String>,
    pub font_weight: Option<u16>,
    pub font_style: Option<String>,
    pub font_size: Option<f32>,
    // Rich Text / RFC 003
    pub background_color: Option<Color>,
    pub background_padding: Option<f32>,
    pub stroke_width: Option<f32>,
    pub stroke_color: Option<Color>,
    pub fill_gradient: Option<GradientConfig>,
    /// Letter spacing in pixels (positive = wider, negative = tighter).
    pub letter_spacing: Option<f32>,
    /// Extra advance after each space in pixels.
    pub word_spacing: Option<f32>,
    /// Line height multiplier (1.0 = single, 1.5 = 1.5x, etc.).
    pub line_height: Option<f32>,
    /// Variable font axis positions. `None` uses the font's defaults.
    pub font_variations: Option<Vec<FontVariation>>,
    /// The span keeps its advance and paints nothing, as `visibility: hidden` does.
    pub hidden: bool,
}

pub use director_scene::FontVariation;

/// Strategies for fitting text within a constrained container.
#[derive(Clone, Debug, PartialEq)]
pub enum TextFit {
    /// Normal behavior: text overflows if too long.
    None,
    /// Reduces font size until text fits within the box or hits min size.
    Shrink,
}

/// Drop shadow configuration for text.
#[derive(Clone, Debug)]
pub struct TextShadow {
    pub color: Color,
    pub blur: f32,
    pub offset: (f32, f32),
}

/// Helper trait for cloning trait objects (`Box<dyn Element>`).
pub trait ElementClone {
    fn clone_box(&self) -> Box<dyn Element>;
}

impl<T> ElementClone for T
where
    T: 'static + Element + Clone,
{
    fn clone_box(&self) -> Box<dyn Element> {
        Box::new(self.clone())
    }
}

/// The core trait defining a visual node in the Scene Graph.
///
/// All renderable entities (Box, Text, Video, etc.) must implement this trait.
pub trait Element: std::fmt::Debug + ElementClone {
    /// Returns a stable snake_case tag identifying the concrete element type.
    ///
    /// Used by manifest and snapshot code to report node kinds without exposing
    /// the element's internal state. The default implementation returns
    /// `"unknown"`.
    fn kind(&self) -> &'static str {
        "unknown"
    }

    /// Copies this element into a scene-document body and opacity channel.
    ///
    /// The default reports the kind as unsupported, so an export fails loudly
    /// for an element without a document form.
    fn export_body(
        &self,
        _cx: &mut crate::scene_export::ExportContext,
    ) -> Result<crate::scene_export::ElementExport, crate::scene_export::ExportError> {
        Err(crate::scene_export::ExportError::Unsupported { kind: self.kind() })
    }

    /// The layout style a scene document records. An element that animates its
    /// layout returns the style before animation.
    fn export_layout_style(&self) -> taffy::Style {
        self.layout_style()
    }

    /// Returns self as `Any` for downcasting.
    fn as_any(&self) -> &dyn Any;
    /// Returns mutable self as `Any` for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Whether this element has intrinsic content size that layout needs to know about.
    fn needs_measure(&self) -> bool {
        false
    }

    /// Computes the size of the element given the available space.
    fn measure(
        &self,
        _known_dimensions: Size<Option<f32>>,
        _available_space: Size<AvailableSpace>,
    ) -> Size<f32> {
        Size::ZERO
    }

    /// Returns the Taffy `Style` for layout computation.
    fn layout_style(&self) -> Style;

    /// Updates the Taffy `Style`.
    fn set_layout_style(&mut self, _style: Style) {
        // Default no-op
    }

    /// Updates the element's state for the given time.
    ///
    /// # Returns
    /// * `true` if the element's visual appearance changed (requiring a repaint).
    fn update(&mut self, time: f64) -> bool;

    /// Runs after every node's `update` and before layout, one node at a time,
    /// so an element can read what other nodes published while they updated in
    /// parallel. Returns `true` when its layout must be measured again. HTML
    /// text picks up the live sizes of its atomic inline boxes here.
    fn pre_layout(&mut self) -> bool {
        false
    }

    /// Paint depends on local time, so a static-subtree cache key includes that time.
    fn varies_with_time(&self) -> bool {
        false
    }

    /// True when this element's paint or layout may change somewhere in local
    /// time `[t0, t1]`. Motion blur paints a shutter in which nothing moves
    /// once. The default assumes motion.
    fn moves_between(&self, _t0: f64, _t1: f64) -> bool {
        true
    }

    /// How far, in local pixels, the element's own paint may move inside a
    /// box of `size` over local time `[t0, t1]` beyond what the box and its
    /// transforms show, such as a sliding shadow or a scrolling background.
    /// Motion blur spaces its samples by the furthest on-screen travel, and
    /// `None` makes it take every sample. The default answers zero for an
    /// element that does not move and `None` otherwise.
    fn paint_travel(&self, t0: f64, t1: f64, _size: (f32, f32)) -> Option<f32> {
        (!self.moves_between(t0, t1)).then_some(0.0)
    }

    /// The local time this node's children run at while the node runs at
    /// `time`. A time remap gives its subtree another clock; the default
    /// passes the node's time through.
    fn child_time(&self, time: f64) -> f64 {
        time
    }

    /// False when nothing below this node paints or sounds at local `time`,
    /// such as a clip outside its window. [`crate::Director::update`] then
    /// leaves the subtree's state where it was, and layout reuses its last
    /// result. The node itself still updates.
    fn children_active(&self, _time: f64) -> bool {
        true
    }

    /// The earliest and latest child times while the node runs through local
    /// time `[t0, t1]`, for motion queries over the subtree.
    fn child_time_range(&self, t0: f64, t1: f64) -> (f64, f64) {
        (t0, t1)
    }

    /// Called after layout is computed but before rendering.
    ///
    /// Useful for elements that need to adjust their state based on their final size (e.g. `TextFit`).
    fn post_layout(&mut self, _rect: Rect) {}

    /// Where this box sits in its parent once every `post_layout` has run,
    /// replacing the layout origin. An atomic inline box takes the position its
    /// sibling text paragraph laid out for it.
    fn layout_origin(&self) -> Option<(f32, f32)> {
        None
    }

    /// Renders the element to the Skia canvas.
    ///
    /// # Arguments
    /// * `canvas` - The Skia canvas to draw on.
    /// * `layout_rect` - The computed layout box for this element.
    /// * `opacity` - The inherited opacity from parent nodes.
    /// * `draw_children` - A closure to trigger rendering of children nodes.
    fn render(
        &self,
        canvas: &Canvas,
        layout_rect: Rect,
        opacity: f32,
        draw_children: &mut dyn FnMut(&Canvas),
    ) -> Result<(), crate::RenderError>;

    /// A scene graph and root this element paints in place of [`Element::render`].
    /// The renderer traverses it at the element's origin with no static layer reuse.
    fn sampled_graph(&self) -> Option<(std::sync::MutexGuard<'_, crate::scene::SceneGraph>, crate::types::NodeId)> {
        None
    }

    /// Direct children share a 3D context and are depth-sorted.
    fn shares_3d(&self) -> bool {
        false
    }

    /// Z of the element center after its local transform. Larger values paint later.
    fn view_depth(&self, _width: f32, _height: f32) -> f32 {
        0.0
    }

    /// CSS transform and transform-origin, mapping a border-box point into this
    /// element's plane. `None` leaves Z unchanged. Parent perspective is not included.
    fn local_view_matrix(&self, _width: f32, _height: f32) -> Option<skia_safe::M44> {
        None
    }

    /// What the element concatenates around its children in border-box space,
    /// such as CSS perspective. `None` paints them unchanged.
    fn child_view_matrix(&self, _width: f32, _height: f32) -> Option<skia_safe::M44> {
        None
    }

    /// The element's own 2D transform about its origin, sampled by the motion
    /// policy. `None` means the element has no transform of its own.
    fn local_transform(&self, _width: f32, _height: f32) -> Option<skia_safe::Matrix> {
        None
    }

    /// Bounds of what [`Element::render`] paints for `rect` when its children
    /// paint within `children`, in the space of `rect` after the element's own
    /// transform. `None` means unbounded, and an unbounded child keeps the
    /// element unbounded unless the element clips it.
    fn paint_bounds(&self, _rect: Rect, _children: Option<Rect>) -> Option<Rect> {
        None
    }

    /// True when [`Element::render`] composites its paint through a layer at
    /// inherited `opacity`. The painter bounds such layers by
    /// [`Element::paint_bounds`].
    fn opens_layer(&self, _opacity: f32) -> bool {
        false
    }

    /// Animates a specific named property.
    ///
    /// Returns `Err` for unknown property names or invalid easing strings.
    /// Authoring surfaces (Rhai) must propagate this rather than soft-failing.
    fn animate_property(
        &mut self,
        property: &str,
        start: f32,
        target: f32,
        duration: f64,
        easing: &str,
    ) -> Result<(), String>;

    /// Animates a property using physics-based spring dynamics.
    ///
    /// Returns `Err` for unknown property names.
    fn animate_property_spring(
        &mut self,
        property: &str,
        _start: Option<f32>,
        _target: f32,
        _config: crate::animation::SpringConfig,
    ) -> Result<(), String> {
        Err(format!(
            "spring: property '{property}' is not spring-animatable on {}",
            self.kind()
        ))
    }

    /// Animates a color property from start to end over a duration.
    fn animate_color(
        &mut self,
        _start: Color,
        _end: Color,
        _duration: f64,
        _easing: &str,
    ) {
        // Default: No-op
    }

    /// Sets a color property immediately (no animation).
    fn set_color(&mut self, _color: Color) {
        // Default: No-op
    }

    /// Replaces the content with a list of rich text spans.
    fn set_rich_text(&mut self, _spans: Vec<TextSpan>) {}

    /// Modifies the existing text spans in place.
    fn modify_text_spans(&mut self, _f: &dyn Fn(&mut Vec<TextSpan>)) {}

    /// Adds an animator to a specific range of text graphemes.
    ///
    /// Only applicable for `TextNode`.
    #[allow(clippy::too_many_arguments)]
    fn add_text_animator(
        &mut self,
        _start_idx: usize,
        _end_idx: usize,
        _property: String,
        _start_val: f32,
        _target_val: f32,
        _duration: f64,
        _easing: &str,
    ) {
        // Default implementation: Warn user
        warn!("add_animator called on a non-text node.");
    }

    /// Sets the node's local opacity immediately.
    fn set_opacity(&mut self, _opacity: f32) {}

    /// Adds an animator to a specific range of text graphemes with optional stagger.
    #[allow(clippy::too_many_arguments)]
    fn add_text_animator_full(
        &mut self,
        _start_idx: usize,
        _end_idx: usize,
        _property: String,
        _start_val: f32,
        _target_val: f32,
        _duration: f64,
        _easing: &str,
        _stagger: f32,
    ) {
        // Fallback to simpler version
        self.add_text_animator(
            _start_idx,
            _end_idx,
            _property,
            _start_val,
            _target_val,
            _duration,
            _easing,
        );
    }

    /// Returns audio samples if this element contains audio (e.g., Video or Composition).
    ///
    /// # Arguments
    /// * `time` - Local time in the element's timeline.
    /// * `samples_needed` - Number of samples requested.
    /// * `sample_rate` - Target sample rate.
    fn get_audio(&self, _time: f64, _samples_needed: usize, _sample_rate: u32) -> Option<Vec<f32>> {
        None
    }
}

impl Clone for Box<dyn Element> {
    fn clone(&self) -> Box<dyn Element> {
        self.clone_box()
    }
}

/// Returns a stable string tag for the concrete element type.
///
/// Used by manifest and snapshot code to report node kinds without exposing
/// the element's internal state. Delegates to [`Element::kind`] so each node
/// type controls its own tag.
pub(crate) fn element_kind(element: &dyn Element) -> &'static str {
    element.kind()
}
