//! # Types Module
//!
//! Shared data types used across the engine.
//!
//! ## Responsibilities
//! - **Color**: RGBA color representation with Skia conversion.
//! - **Transform**: Animated 2D transforms (scale, rotation, translation).
//! - **ObjectFit**: Image/video scaling modes (Cover, Contain, Fill).
//!
//! ## Key Types
//! - `Color`: Float-based RGBA color.
//! - `Transform`: All animated transform properties.
//! - `NodeId`: Type alias for arena indices (`usize`).

use crate::animation::Animated;
use keyframe::CanTween;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Color4f, Path, PathMeasure, Point};

/// Specifies how the content of a replaceable element (img, video) should
/// be resized to fit its container.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum ObjectFit {
    /// The content is sized to maintain its aspect ratio while filling the element's entire content box.
    /// If the object's aspect ratio does not match the aspect ratio of its box, then the object will be clipped to fit.
    Cover,
    /// The content is scaled to maintain its aspect ratio while fitting within the element's content box.
    /// The entire object is made to fill the box, while preserving its aspect ratio, so the object will be "letterboxed"
    /// if its aspect ratio does not match the aspect ratio of the box.
    Contain,
    /// The content is sized to fill the element's content box. The entire object will completely fill the box.
    /// If the object's aspect ratio does not match the aspect ratio of its box, then the object will be stretched to fit.
    Fill,
}

impl Default for ObjectFit {
    fn default() -> Self {
        Self::Cover
    }
}

// --- From director.rs ---

/// A unique identifier for a node in the scene graph.
pub type NodeId = usize;

/// State for a node currently animating along an SVG path.
pub struct PathAnimationState {
    pub path: Path,
    pub progress: Animated<f32>,
    /// Whether the node should rotate to follow the path tangent.
    pub follow_tangent: bool,
    /// Playback speed multiplier (1.0 = normal).
    pub speed: f32,
    /// Loop mode: 0 = play once, -1 = infinite, >0 = N times.
    pub loop_count: i32,
    /// Total duration of one animation loop in seconds.
    pub duration: f64,
    /// Cached total length of the path (computed once on creation).
    cached_length: f32,
    /// Cached PathMeasure to avoid re-flattening the path each frame.
    measure: Option<PathMeasure>,
}

impl PathAnimationState {
    /// Create a new path animation state.
    pub fn new(path: Path, follow_tangent: bool, duration: f64) -> Self {
        let mut measure = PathMeasure::new(&path, false, None);
        let length = measure.length();
        Self {
            path,
            progress: Animated::new(0.0),
            follow_tangent,
            speed: 1.0,
            loop_count: 0,
            duration,
            cached_length: length,
            measure: Some(measure),
        }
    }

    /// Get the cached path length.
    pub fn length(&self) -> f32 {
        self.cached_length
    }

    /// Sample a position and tangent at a given distance along the path.
    /// Reuses the cached PathMeasure instead of creating a new one each frame.
    /// Returns (position, tangent_angle_in_radians).
    pub fn sample(&mut self, distance: f32) -> Option<(skia_safe::Point, f32)> {
        let measure = self.measure.get_or_insert_with(|| PathMeasure::new(&self.path, false, None));
        measure.pos_tan(distance).map(|(pos, tangent)| {
            let angle = tangent.y.atan2(tangent.x);
            (pos, angle)
        })
    }
}

impl Clone for PathAnimationState {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            progress: self.progress.clone(),
            follow_tangent: self.follow_tangent,
            speed: self.speed,
            loop_count: self.loop_count,
            duration: self.duration,
            cached_length: self.cached_length,
            measure: None,
        }
    }
}

/// State for a node animating between two SVG paths (path morphing).
pub struct PathMorphState {
    pub from_path: Path,
    pub to_path: Path,
    pub progress: Animated<f32>,
    pub follow_tangent: bool,
    /// Playback speed multiplier (1.0 = normal).
    pub speed: f32,
    /// Loop mode: 0 = play once, -1 = infinite, >0 = N times.
    pub loop_count: i32,
    /// Total duration of one animation loop in seconds.
    pub duration: f64,
    cached_from_length: f32,
    cached_to_length: f32,
    from_measure: Option<PathMeasure>,
    to_measure: Option<PathMeasure>,
}

impl PathMorphState {
    pub fn new(from_path: Path, to_path: Path, follow_tangent: bool, duration: f64) -> Self {
        let from_len = PathMeasure::new(&from_path, false, None).length();
        let to_len = PathMeasure::new(&to_path, false, None).length();
        let from_measure = Some(PathMeasure::new(&from_path, false, None));
        let to_measure = Some(PathMeasure::new(&to_path, false, None));
        Self {
            from_path,
            to_path,
            progress: Animated::new(0.0),
            follow_tangent,
            speed: 1.0,
            loop_count: 0,
            duration,
            cached_from_length: from_len,
            cached_to_length: to_len,
            from_measure,
            to_measure,
        }
    }

    /// Sample both paths at a proportional distance (0..1) and interpolate.
    /// Reuses cached PathMeasures instead of creating new ones each frame.
    /// Returns (position, tangent_angle_in_radians).
    pub fn sample_morph(&mut self, t: f32, progress_dist: f32) -> Option<(Point, f32)> {
        let from_dist = progress_dist * self.cached_from_length;
        let to_dist = progress_dist * self.cached_to_length;

        let from_measure = self.from_measure.get_or_insert_with(|| PathMeasure::new(&self.from_path, false, None));
        let from_pos = {
            let (pos, tangent) = from_measure.pos_tan(from_dist)?;
            let angle = tangent.y.atan2(tangent.x);
            (pos, angle)
        };

        let to_measure = self.to_measure.get_or_insert_with(|| PathMeasure::new(&self.to_path, false, None));
        let to_pos = {
            let (pos, tangent) = to_measure.pos_tan(to_dist)?;
            let angle = tangent.y.atan2(tangent.x);
            (pos, angle)
        };

        let pos = Point::new(
            from_pos.0.x + (to_pos.0.x - from_pos.0.x) * t,
            from_pos.0.y + (to_pos.0.y - from_pos.0.y) * t,
        );
        let angle = from_pos.1 + (to_pos.1 - from_pos.1) * t;

        Some((pos, angle))
    }

    pub fn from_length(&self) -> f32 { self.cached_from_length }
    pub fn to_length(&self) -> f32 { self.cached_to_length }
}

impl Clone for PathMorphState {
    fn clone(&self) -> Self {
        Self {
            from_path: self.from_path.clone(),
            to_path: self.to_path.clone(),
            progress: self.progress.clone(),
            follow_tangent: self.follow_tangent,
            speed: self.speed,
            loop_count: self.loop_count,
            duration: self.duration,
            cached_from_length: self.cached_from_length,
            cached_to_length: self.cached_to_length,
            from_measure: None,
            to_measure: None,
        }
    }
}

/// Represents the affine transformation state of a node.
#[derive(Clone, Debug)]
pub struct Transform {
    pub scale_x: Animated<f32>,
    pub scale_y: Animated<f32>,
    pub rotation: Animated<f32>,
    pub skew_x: Animated<f32>,
    pub skew_y: Animated<f32>,
    pub translate_x: Animated<f32>,
    pub translate_y: Animated<f32>,
    pub pivot_x: f32,
    pub pivot_y: f32,
}

impl Transform {
    pub fn new() -> Self {
        Self {
            scale_x: Animated::new(1.0),
            scale_y: Animated::new(1.0),
            rotation: Animated::new(0.0),
            skew_x: Animated::new(0.0),
            skew_y: Animated::new(0.0),
            translate_x: Animated::new(0.0),
            translate_y: Animated::new(0.0),
            pivot_x: 0.5,
            pivot_y: 0.5,
        }
    }
}

// --- From element.rs ---

/// Represents a RGBA color in float format (0.0 - 1.0).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const BLACK: Color = Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    pub const WHITE: Color = Color {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };

    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Converts to Skia's `Color` struct (u8 based).
    pub fn to_skia(&self) -> skia_safe::Color {
        skia_safe::Color::from_argb(
            (self.a * 255.0) as u8,
            (self.r * 255.0) as u8,
            (self.g * 255.0) as u8,
            (self.b * 255.0) as u8,
        )
    }

    /// Converts to Skia's `Color4f` struct (float based).
    pub fn to_color4f(&self) -> Color4f {
        Color4f::new(self.r, self.g, self.b, self.a)
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn srgb_to_oklab(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let rl = srgb_to_linear(r);
    let gl = srgb_to_linear(g);
    let bl = srgb_to_linear(b);
    let l = 0.4122214708 * rl + 0.5363325363 * gl + 0.0514459929 * bl;
    let m = 0.2119034982 * rl + 0.6806995451 * gl + 0.1073969566 * bl;
    let s = 0.0883024619 * rl + 0.2817188376 * gl + 0.6299787005 * bl;
    let l_ = l.cbrt();
    let m_ = m.cbrt();
    let s_ = s.cbrt();
    (
        0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
    )
}

#[allow(non_snake_case)]
fn oklab_to_srgb(L: f32, a: f32, b: f32) -> (f32, f32, f32) {
    let l_ = L + 0.3963377774 * a + 0.2158037573 * b;
    let m_ = L - 0.1055613458 * a - 0.0638541728 * b;
    let s_ = L - 0.0894841775 * a - 1.2914855480 * b;
    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;
    let r = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
    let g = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
    let b = -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s;
    (linear_to_srgb(r), linear_to_srgb(g), linear_to_srgb(b))
}

impl CanTween for Color {
    fn ease(from: Self, to: Self, time: impl keyframe::num_traits::Float) -> Self {
        let t = time.to_f64().unwrap() as f32;
        let (l1, a1, b1) = srgb_to_oklab(from.r, from.g, from.b);
        let (l2, a2, b2) = srgb_to_oklab(to.r, to.g, to.b);
        let l = l1 + (l2 - l1) * t;
        let a = a1 + (a2 - a1) * t;
        let b = b1 + (b2 - b1) * t;
        let (r, g, b) = oklab_to_srgb(l, a, b);
        Self {
            r,
            g,
            b,
            a: from.a + (to.a - from.a) * t,
        }
    }
}

/// Configuration for a linear gradient fill.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GradientConfig {
    /// List of colors in the gradient.
    pub colors: Vec<Color>,
    /// Optional positions for the color stops (0.0 to 1.0).
    pub positions: Option<Vec<f32>>,
    /// Start point of the gradient (Relative coordinates 0.0 to 1.0).
    pub start: (f32, f32),
    /// End point of the gradient (Relative coordinates 0.0 to 1.0).
    pub end: (f32, f32),
}

impl Default for GradientConfig {
    fn default() -> Self {
        Self {
            colors: vec![Color::BLACK, Color::WHITE],
            positions: None,
            start: (0.0, 0.0),
            end: (0.0, 1.0), // Default Top-to-Bottom
        }
    }
}

// --- From tokens.rs ---

/// Represents a "safe zone" or padding area to avoid UI elements on specific platforms.
#[derive(Clone, Debug)]
pub struct SafeZone {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
    pub aspect_ratio: String,
}
