//! # Scripting Utilities
//!
//! Parsing helpers and utility functions for Rhai script bindings.
//!
//! ## Responsibilities
//! - **Color Parsing**: `parse_hex_color` for hex string to Color conversion
//! - **Layout Parsing**: `parse_layout_style` for Taffy style properties
//! - **Text Parsing**: `parse_text_style`, `parse_spans_from_dynamic`
//! - **Animation Parsing**: `parse_spring_config`, `parse_easing`
//! - **Effect Helpers**: `apply_effect_to_node`, `extract_outer_style`

use crate::animation::{EasingType, SpringConfig};
use crate::director::Director;
use crate::element::{TextShadow, TextSpan};
use crate::node::{BoxNode, CompositionNode, EffectNode, EffectType};
use crate::types::{Color, GradientConfig, GradientType, NodeId, ObjectFit};
use rhai::{Dynamic, Map};
use serde_json::{Map as JsonMap, Value as JsonValue};
use taffy::prelude::*;
use taffy::style::{GridPlacement, GridTemplateComponent, Style};

/// Encode a [`NodeId`] for Rhai as `#{ index, generation }`.
pub fn node_id_to_dynamic(id: NodeId) -> Dynamic {
    let mut m = Map::new();
    m.insert("index".into(), (id.index as i64).into());
    m.insert("generation".into(), (id.generation as i64).into());
    Dynamic::from_map(m)
}

/// Decode a Rhai value into a [`NodeId`].
///
/// Accepts:
/// - `#{ index, generation }` maps (preferred)
/// - bare integers as `index` at generation 0 (compat / tests)
pub fn node_id_from_dynamic(value: &Dynamic) -> Result<NodeId, String> {
    if let Some(map) = value.clone().try_cast::<Map>() {
        let index = map
            .get("index")
            .and_then(|v| v.as_int().ok())
            .ok_or_else(|| "node id map missing integer 'index'".to_string())?;
        if index < 0 {
            return Err(format!("node index must be non-negative, got {index}"));
        }
        let generation = map
            .get("generation")
            .and_then(|v| v.as_int().ok())
            .unwrap_or(0);
        if generation < 0 {
            return Err(format!(
                "node generation must be non-negative, got {generation}"
            ));
        }
        return Ok(NodeId::new(index as u32, generation as u32));
    }
    if let Ok(index) = value.as_int() {
        if index < 0 {
            return Err(format!("node index must be non-negative, got {index}"));
        }
        return Ok(NodeId::at(index as u32));
    }
    Err("node id must be an integer or #{ index, generation } map".into())
}

/// Resolve a Rhai node id against the live scene.
///
/// Maps use exact generation match. Bare integers select the **current** live
/// occupant of that slot (`id_at`), so compiler introspection stays correct
/// after free-list reuse.
pub fn resolve_live_node_id(director: &Director, value: &Dynamic) -> Result<NodeId, String> {
    if value.clone().try_cast::<Map>().is_some() {
        let id = node_id_from_dynamic(value)?;
        if director.scene.get_node(id).is_none() {
            return Err(format!("NodeId {id} is stale (node was destroyed)"));
        }
        return Ok(id);
    }
    if let Ok(index) = value.as_int() {
        if index < 0 {
            return Err(format!("node index must be non-negative, got {index}"));
        }
        return director
            .scene
            .id_at(index as usize)
            .ok_or_else(|| format!("no live node at index {index}"));
    }
    Err("node id must be an integer or #{ index, generation } map".into())
}

/// Integer index form for Rhai APIs that historically returned bare `i64` ids.
#[inline]
pub fn node_id_index_i64(id: NodeId) -> i64 {
    id.index as i64
}

/// Extract outer style properties for effect wrapper nodes.
/// Copies all container/layout properties so the wrapper preserves flex/grid
/// behaviour; the inner node gets reset to fill the wrapper (100% w/h).
pub fn extract_outer_style(source: &Style) -> Style {
    source.clone()
}

/// Apply an effect to a node by wrapping it in an EffectNode.
///
/// # Mutation policy (plan 02 — rewrite, honest)
///
/// - Returns the **new** effect wrapper [`NodeId`] (canonical outer identity).
/// - The original `node_id` remains alive as a **child** of the wrapper; it is
///   **not** destroyed. Callers that used the target as a scene root or parent
///   edge are rewritten to the wrapper.
/// - Rhai `apply_effect` returns a new [`crate::scripting::types::NodeHandle`]
///   with the wrapper id; scripts must use the returned handle for further
///   outer mutations. The old handle still addresses the inner content node.
pub fn apply_effect_to_node(d: &mut Director, node_id: NodeId, effect: EffectType) -> NodeId {
    let parent_id_opt = d.scene.get_node(node_id).and_then(|n| n.parent);

    let wrapper_style;

    // Modify target node style (Steal & Fill)
    if let Some(node) = d.scene.get_node_mut(node_id) {
        let original_style = node.element.layout_style();
        wrapper_style = extract_outer_style(&original_style);

        if let Some(box_node) = node.element.as_any_mut().downcast_mut::<BoxNode>() {
            box_node.style.size = taffy::geometry::Size {
                width: Dimension::percent(1.0),
                height: Dimension::percent(1.0),
            };
            box_node.style.margin = taffy::geometry::Rect::zero();
            box_node.style.flex_grow = 0.0;
            box_node.style.flex_shrink = 1.0;
            box_node.style.position = taffy::style::Position::Relative;
            box_node.style.inset = taffy::geometry::Rect::auto();
        } else if let Some(comp_node) = node.element.as_any_mut().downcast_mut::<CompositionNode>()
        {
            comp_node.style.size = taffy::geometry::Size {
                width: Dimension::percent(1.0),
                height: Dimension::percent(1.0),
            };
            comp_node.style.margin = taffy::geometry::Rect::zero();
            comp_node.style.flex_grow = 0.0;
            comp_node.style.flex_shrink = 1.0;
            comp_node.style.position = taffy::style::Position::Relative;
            comp_node.style.inset = taffy::geometry::Rect::auto();
        }
    } else {
        return node_id; // Failure fallback — caller should have validated liveness
    }

    let effect_node = EffectNode {
        effects: vec![effect],
        style: wrapper_style,
        shader_cache: d.assets.shader_cache.clone(),
        current_time: 0.0,
    };

    let effect_id = d.scene.add_node(Box::new(effect_node));

    if let Some(parent_id) = parent_id_opt {
        d.scene.remove_child(parent_id, node_id);
        d.scene.add_child(parent_id, effect_id);
    }

    d.scene.add_child(effect_id, node_id);

    // Update Root if needed (tracks only — plan 01).
    for track in &mut d.tracks {
        for clip in &mut track.clips {
            if clip.scene_root == node_id {
                clip.scene_root = effect_id;
            }
        }
    }

    effect_id
}

/// Helper to parse hex strings like "#RRGGBB" or "#RGB"
pub fn parse_hex_color(hex: &str) -> Option<Color> {
    let hex = hex.trim_start_matches('#');
    let (r, g, b) = match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            (r, g, b)
        }
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            (r * 17, g * 17, b * 17)
        }
        _ => return None,
    };

    Some(Color::new(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        1.0,
    ))
}

/// Parse a gradient configuration from a Rhai dynamic value.
pub fn parse_gradient_config(g: &rhai::Dynamic) -> Option<GradientConfig> {
    if let Ok(arr) = g.clone().into_array() {
        // Simple array of colors
        let mut colors = Vec::new();
        for item in arr {
            if let Ok(s) = item.into_string() {
                if let Some(c) = parse_hex_color(&s) {
                    colors.push(c);
                }
            }
        }
        if !colors.is_empty() {
            return Some(GradientConfig {
                colors,
                ..Default::default()
            });
        }
    } else if let Some(gmap) = g.clone().try_cast::<Map>() {
        // Advanced map
        let mut config = GradientConfig::default();
        if let Some(arr) = gmap.get("colors").and_then(|v| v.clone().into_array().ok()) {
            let mut colors = Vec::new();
            for item in arr {
                if let Ok(s) = item.into_string() {
                    if let Some(c) = parse_hex_color(&s) {
                        colors.push(c);
                    }
                }
            }
            config.colors = colors;
        }
        if let Some(arr) = gmap.get("positions").and_then(|v| v.clone().into_array().ok()) {
            let mut positions = Vec::new();
            for item in arr {
                if let Ok(f) = item.as_float() {
                    positions.push(f as f32);
                }
            }
            if !positions.is_empty() {
                config.positions = Some(positions);
            }
        }
        if let Some(arr) = gmap.get("start").and_then(|v| v.clone().into_array().ok()) {
            if arr.len() >= 2 {
                let x = arr[0].as_float().unwrap_or(0.0) as f32;
                let y = arr[1].as_float().unwrap_or(0.0) as f32;
                config.start = (x, y);
            }
        }
        if let Some(arr) = gmap.get("end").and_then(|v| v.clone().into_array().ok()) {
            if arr.len() >= 2 {
                let x = arr[0].as_float().unwrap_or(0.0) as f32;
                let y = arr[1].as_float().unwrap_or(0.0) as f32;
                config.end = (x, y);
            }
        }
        if !config.colors.is_empty() {
            // Parse gradient type
            if let Some(gt) = gmap.get("type").and_then(|v| v.clone().into_string().ok()) {
                config.gradient_type = match gt.as_str() {
                    "radial" => GradientType::Radial,
                    "conic" | "sweep" => GradientType::Conic,
                    _ => GradientType::Linear,
                };
            }
            // Parse center for radial/conic
            if let Some(arr) = gmap.get("center").and_then(|v| v.clone().into_array().ok()) {
                if arr.len() >= 2 {
                    config.center = (
                        arr[0].as_float().unwrap_or(0.5) as f32,
                        arr[1].as_float().unwrap_or(0.5) as f32,
                    );
                }
            }
            // Parse radius for radial
            if let Some(r) = gmap.get("radius").and_then(|v| v.as_float().ok()) {
                config.radius = r as f32;
            }
            // Parse angle for conic (degrees)
            if let Some(a) = gmap.get("angle").and_then(|v| v.as_float().ok()) {
                config.angle = a as f32;
            }
            return Some(config);
        }
    }
    None
}

/// Parse text style properties from a Rhai map into a TextSpan.
pub fn parse_text_style(map: &rhai::Map, span: &mut TextSpan) {
    if let Some(c) = map.get("color").and_then(|v| v.clone().into_string().ok()) {
        span.color = parse_hex_color(&c);
    }
    if let Some(w) = map.get("weight").or_else(|| map.get("font_weight")) {
        span.font_weight = parse_font_weight(w);
    }
    if let Some(s) = map.get("size").and_then(|v| v.as_float().ok())
        .or_else(|| map.get("font_size").and_then(|v| v.as_float().ok()))
    {
        span.font_size = Some(s as f32);
    }
    if let Some(f) = map.get("font_family").and_then(|v| v.clone().into_string().ok()) {
        span.font_family = Some(f);
    }
    if let Some(s) = map.get("font_style").and_then(|v| v.clone().into_string().ok()) {
        if s == "italic" || s == "oblique" {
            span.font_style = Some(s);
        }
    }
    // Rich Text Fields
    if let Some(c) = map
        .get("background_color")
        .and_then(|v| v.clone().into_string().ok())
    {
        span.background_color = parse_hex_color(&c);
    }
    if let Some(p) = map
        .get("background_padding")
        .and_then(|v| v.as_float().ok())
    {
        span.background_padding = Some(p as f32);
    }
    if let Some(w) = map.get("stroke_width").and_then(|v| v.as_float().ok()) {
        span.stroke_width = Some(w as f32);
    }
    if let Some(c) = map
        .get("stroke_color")
        .and_then(|v| v.clone().into_string().ok())
    {
        span.stroke_color = parse_hex_color(&c);
    }
    if let Some(g) = map.get("fill_gradient") {
        span.fill_gradient = parse_gradient_config(g);
    }
    // Letter spacing
    if let Some(ls) = map.get("letter_spacing").and_then(|v| v.as_float().ok()) {
        span.letter_spacing = Some(ls as f32);
    }
    // Word spacing
    if let Some(ws) = map.get("word_spacing").and_then(|v| v.as_float().ok()) {
        span.word_spacing = Some(ws as f32);
    }
    // Line height
    if let Some(lh) = map.get("line_height").and_then(|v| v.as_float().ok()) {
        span.line_height = Some(lh as f32);
    }
}

/// Parse spring configuration from a Rhai map.
pub fn parse_spring_config(map: &rhai::Map) -> SpringConfig {
    let mut config = SpringConfig::default();
    if let Some(v) = map.get("stiffness").and_then(|v| v.as_float().ok()) {
        config.stiffness = v as f32;
    }
    if let Some(v) = map.get("damping").and_then(|v| v.as_float().ok()) {
        config.damping = v as f32;
    }
    if let Some(v) = map.get("mass").and_then(|v| v.as_float().ok()) {
        config.mass = v as f32;
    }
    if let Some(v) = map.get("velocity").and_then(|v| v.as_float().ok()) {
        config.velocity = v as f32;
    }
    config
}

/// Parse a font weight value from a Rhai dynamic (numeric or keyword).
pub fn parse_font_weight(w: &rhai::Dynamic) -> Option<u16> {
    if let Ok(n) = w.as_int() {
        return Some(n as u16);
    }
    if let Ok(s) = w.clone().into_string() {
        return match s.as_str() {
            "thin" => Some(100),
            "extra_light" | "extra-light" | "extralight" => Some(200),
            "light" => Some(300),
            "normal" | "regular" => Some(400),
            "medium" => Some(500),
            "semi_bold" | "semi-bold" | "semibold" => Some(600),
            "bold" => Some(700),
            "extra_bold" | "extra-bold" | "extrabold" => Some(800),
            "black" => Some(900),
            _ => s.parse::<u16>().ok(),
        };
    }
    None
}/// Parse easing type from string.
pub fn parse_easing(ease: &str) -> EasingType {
    parse_easing_result(ease).unwrap_or(EasingType::Linear)
}

/// Parse easing type from string, logging an error with the valid name list on failure.
pub fn parse_easing_loud(ease: &str) -> EasingType {
    match parse_easing_result(ease) {
        Ok(e) => e,
        Err(msg) => {
            tracing::error!("{msg}");
            EasingType::Linear
        }
    }
}


/// Parse easing type from string, returning an error for unknown names.
pub fn parse_easing_result(ease: &str) -> Result<EasingType, String> {
    if let Some(inner) = ease.strip_prefix("cubic_bezier(") {
        let inner = inner.strip_suffix(')').ok_or_else(|| "missing closing parenthesis in cubic_bezier(...)".to_string())?;
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() != 4 {
            return Err("cubic_bezier expects 4 numbers: x1,y1,x2,y2".to_string());
        }
        let vals: Vec<f64> = parts
            .iter()
            .map(|s| s.trim().parse::<f64>().map_err(|e| format!("{e}")))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(EasingType::CubicBezier {
            x1: vals[0],
            y1: vals[1],
            x2: vals[2],
            y2: vals[3],
        });
    }
    if let Some(inner) = ease.strip_prefix("expr(") {
        let expr = inner.strip_suffix(')').ok_or_else(|| "missing closing parenthesis in expr(...)".to_string())?;
        return Ok(EasingType::Expression { expr: expr.to_string() });
    }
    match ease {
        "linear" => Ok(EasingType::Linear),
        "ease_in" => Ok(EasingType::EaseIn),
        "ease_out" => Ok(EasingType::EaseOut),
        "ease_in_out" => Ok(EasingType::EaseInOut),
        "bounce_out" => Ok(EasingType::BounceOut),
        "bounce_in" => Ok(EasingType::BounceIn),
        "bounce_in_out" => Ok(EasingType::BounceInOut),
        "elastic_out" => Ok(EasingType::ElasticOut),
        "elastic_in" => Ok(EasingType::ElasticIn),
        "elastic_in_out" => Ok(EasingType::ElasticInOut),
        "back_out" => Ok(EasingType::BackOut),
        "back_in" => Ok(EasingType::BackIn),
        "back_in_out" => Ok(EasingType::BackInOut),
        "hold" | "step" => Ok(EasingType::Hold),
        _ => Err(format!("unknown easing '{}'; valid: linear, ease_in/out/in_out, bounce_*, elastic_*, back_*, hold/step, cubic_bezier(x1,y1,x2,y2), expr(...)", ease)),
    }
}

/// Keys `parse_layout_style` reads. Other map keys are not layout.
pub const LAYOUT_STYLE_KEYS: &[&str] = &[
    "width",
    "height",
    "max_width",
    "max_height",
    "flex_direction",
    "align_items",
    "justify_content",
    "flex_wrap",
    "flex_grow",
    "flex_shrink",
    "padding",
    "padding_top",
    "padding_right",
    "padding_bottom",
    "padding_left",
    "margin",
    "margin_top",
    "margin_right",
    "margin_bottom",
    "margin_left",
    "gap",
    "display",
    "grid_template_columns",
    "grid_template_rows",
    "grid_row",
    "grid_column",
    "aspect_ratio",
    "position",
    "left",
    "right",
    "top",
    "bottom",
];

/// Keys `parse_text_style` reads, including documented aliases (`font_size`, `font_weight`).
pub const TEXT_STYLE_KEYS: &[&str] = &[
    "color",
    "weight",
    "font_weight",
    "size",
    "font_size",
    "font_family",
    "font_style",
    "background_color",
    "background_padding",
    "stroke_width",
    "stroke_color",
    "fill_gradient",
    "letter_spacing",
    "word_spacing",
    "line_height",
];

/// Error when `map` contains a key outside `allowed`.
pub fn reject_unknown_keys(map: &Map, allowed: &[&str], kind: &str) -> Result<(), String> {
    let mut unknown: Vec<&str> = map
        .keys()
        .map(|key| key.as_str())
        .filter(|key| !allowed.iter().any(|allowed_key| allowed_key == key))
        .collect();
    if unknown.is_empty() {
        return Ok(());
    }
    unknown.sort_unstable();
    Err(format!(
        "unknown {kind} key '{}'; valid: {}",
        unknown.join("', '"),
        allowed.join(", ")
    ))
}

/// Parse layout style properties from a Rhai map into a Taffy Style.
pub fn parse_layout_style(props: &rhai::Map, style: &mut Style) {
    let to_dim = |v: &rhai::Dynamic| -> Option<Dimension> {
        if let Ok(f) = v.as_float() {
            Some(Dimension::length(f as f32))
        } else if let Ok(i) = v.as_int() {
            Some(Dimension::length(i as f32))
        } else if let Ok(s) = v.clone().into_string() {
            if s == "auto" {
                Some(Dimension::auto())
            } else if s.ends_with("%") {
                if let Ok(p) = s.trim_end_matches('%').parse::<f32>() {
                    Some(Dimension::percent(p / 100.0))
                } else {
                    None
                }
            } else if let Ok(val) = s.parse::<f32>() {
                Some(Dimension::length(val))
            } else {
                None
            }
        } else {
            None
        }
    };

    if let Some(w) = props.get("width").and_then(to_dim) {
        style.size.width = w;
    }
    if let Some(h) = props.get("height").and_then(to_dim) {
        style.size.height = h;
    }
    if let Some(w) = props.get("max_width").and_then(to_dim) {
        style.max_size.width = w;
    }
    if let Some(h) = props.get("max_height").and_then(to_dim) {
        style.max_size.height = h;
    }

    if let Some(s) = props
        .get("flex_direction")
        .and_then(|v| v.clone().into_string().ok())
    {
        style.flex_direction = match s.as_str() {
            "row" => FlexDirection::Row,
            "column" => FlexDirection::Column,
            "row_reverse" | "row-reverse" => FlexDirection::RowReverse,
            "column_reverse" | "column-reverse" => FlexDirection::ColumnReverse,
            _ => FlexDirection::Row,
        };
    }

    if let Some(s) = props
        .get("align_items")
        .and_then(|v| v.clone().into_string().ok())
    {
        style.align_items = match s.as_str() {
            "center" => Some(AlignItems::Center),
            "flex_start" | "flex-start" | "start" => Some(AlignItems::FlexStart),
            "flex_end" | "flex-end" | "end" => Some(AlignItems::FlexEnd),
            "stretch" => Some(AlignItems::Stretch),
            _ => Some(AlignItems::Stretch),
        };
    }

    if let Some(s) = props
        .get("justify_content")
        .and_then(|v| v.clone().into_string().ok())
    {
        style.justify_content = match s.as_str() {
            "center" => Some(JustifyContent::Center),
            "flex_start" | "flex-start" | "start" => Some(JustifyContent::FlexStart),
            "flex_end" | "flex-end" | "end" => Some(JustifyContent::FlexEnd),
            "space_between" | "space-between" => Some(JustifyContent::SpaceBetween),
            "space_around" | "space-around" => Some(JustifyContent::SpaceAround),
            "space_evenly" | "space-evenly" => Some(JustifyContent::SpaceEvenly),
            _ => Some(JustifyContent::FlexStart),
        };
    }

    if let Some(s) = props
        .get("flex_wrap")
        .and_then(|v| v.clone().into_string().ok())
    {
        style.flex_wrap = match s.as_str() {
            "wrap" => FlexWrap::Wrap,
            "wrap_reverse" | "wrap-reverse" => FlexWrap::WrapReverse,
            _ => FlexWrap::NoWrap,
        };
    }

    if let Some(f) = props.get("flex_grow").and_then(|v| v.as_float().ok()) {
        style.flex_grow = f as f32;
    }
    if let Some(f) = props.get("flex_shrink").and_then(|v| v.as_float().ok()) {
        style.flex_shrink = f as f32;
    }

    // Padding/Margin
    let to_lp = |v: &rhai::Dynamic| -> Option<LengthPercentage> {
        if let Ok(f) = v.as_float() {
            Some(LengthPercentage::length(f as f32))
        } else if let Ok(i) = v.as_int() {
            Some(LengthPercentage::length(i as f32))
        } else if let Ok(s) = v.clone().into_string() {
            if s.ends_with("%") {
                if let Ok(p) = s.trim_end_matches('%').parse::<f32>() {
                    Some(LengthPercentage::percent(p / 100.0))
                } else {
                    None
                }
            } else if let Ok(val) = s.parse::<f32>() {
                Some(LengthPercentage::length(val))
            } else {
                None
            }
        } else {
            None
        }
    };

    if let Some(p) = props.get("padding").and_then(to_lp) {
        style.padding = Rect {
            left: p,
            right: p,
            top: p,
            bottom: p,
        };
    }
    if let Some(p) = props.get("padding_top").and_then(to_lp) {
        style.padding.top = p;
    }
    if let Some(p) = props.get("padding_right").and_then(to_lp) {
        style.padding.right = p;
    }
    if let Some(p) = props.get("padding_bottom").and_then(to_lp) {
        style.padding.bottom = p;
    }
    if let Some(p) = props.get("padding_left").and_then(to_lp) {
        style.padding.left = p;
    }

    let to_lpa = |v: &rhai::Dynamic| -> Option<LengthPercentageAuto> {
        if let Ok(f) = v.as_float() {
            Some(LengthPercentageAuto::length(f as f32))
        } else if let Ok(i) = v.as_int() {
            Some(LengthPercentageAuto::length(i as f32))
        } else if let Ok(s) = v.clone().into_string() {
            if s == "auto" {
                Some(LengthPercentageAuto::auto())
            } else if s.ends_with("%") {
                if let Ok(p) = s.trim_end_matches('%').parse::<f32>() {
                    Some(LengthPercentageAuto::percent(p / 100.0))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    };

    if let Some(m) = props.get("margin").and_then(to_lpa) {
        style.margin = Rect {
            left: m,
            right: m,
            top: m,
            bottom: m,
        };
    }
    if let Some(m) = props.get("margin_top").and_then(to_lpa) {
        style.margin.top = m;
    }
    if let Some(m) = props.get("margin_right").and_then(to_lpa) {
        style.margin.right = m;
    }
    if let Some(m) = props.get("margin_bottom").and_then(to_lpa) {
        style.margin.bottom = m;
    }
    if let Some(m) = props.get("margin_left").and_then(to_lpa) {
        style.margin.left = m;
    }

    // Gap
    if let Some(g) = props.get("gap").and_then(|v| v.as_float().ok()) {
        let gap_val = LengthPercentage::length(g as f32);
        style.gap = Size {
            width: gap_val,
            height: gap_val,
        };
    }

    // Display (Flex/Grid)
    if let Some(s) = props
        .get("display")
        .and_then(|v| v.clone().into_string().ok())
    {
        style.display = match s.as_str() {
            "grid" => Display::Grid,
            "none" => Display::None,
            _ => Display::Flex,
        };
    }

    // Grid Templates
    let parse_track_sizing = |s: &str| -> GridTemplateComponent<String> {
        if s == "auto" {
            GridTemplateComponent::Single(auto())
        } else if s == "min-content" || s == "min_content" {
            GridTemplateComponent::Single(min_content())
        } else if s == "max-content" || s == "max_content" {
            GridTemplateComponent::Single(max_content())
        } else if s.ends_with("fr") {
            let val = s.trim_end_matches("fr").parse::<f32>().unwrap_or(1.0);
            GridTemplateComponent::Single(fr(val))
        } else if s.ends_with("%") {
            let val = s.trim_end_matches("%").parse::<f32>().unwrap_or(0.0);
            GridTemplateComponent::Single(percent(val / 100.0))
        } else {
            let val = s.parse::<f32>().unwrap_or(0.0);
            GridTemplateComponent::Single(length(val))
        }
    };

    if let Some(cols) = props
        .get("grid_template_columns")
        .and_then(|v| v.clone().into_array().ok())
    {
        style.grid_template_columns = cols
            .into_iter()
            .map(|v| parse_track_sizing(&v.to_string()))
            .collect();
    }
    if let Some(rows) = props
        .get("grid_template_rows")
        .and_then(|v| v.clone().into_array().ok())
    {
        style.grid_template_rows = rows
            .into_iter()
            .map(|v| parse_track_sizing(&v.to_string()))
            .collect();
    }

    // Grid Placement (Item)
    let parse_gp = |s: &str| -> GridPlacement<String> {
        let s = s.trim();
        if s.starts_with("span") {
            let val = s
                .trim_start_matches("span")
                .trim()
                .parse::<u16>()
                .unwrap_or(1);
            GridPlacement::Span(val)
        } else if let Ok(val) = s.parse::<i16>() {
            GridPlacement::Line(val.into())
        } else {
            GridPlacement::Auto
        }
    };

    let parse_line = |s: &str| -> Line<GridPlacement<String>> {
        if s.contains('/') {
            let parts: Vec<&str> = s.split('/').collect();
            Line {
                start: parse_gp(parts[0]),
                end: if parts.len() > 1 {
                    parse_gp(parts[1])
                } else {
                    GridPlacement::Auto
                },
            }
        } else {
            Line {
                start: parse_gp(s),
                end: GridPlacement::Auto,
            }
        }
    };

    if let Some(s) = props
        .get("grid_row")
        .and_then(|v| v.clone().into_string().ok())
    {
        style.grid_row = parse_line(&s);
    }
    if let Some(s) = props
        .get("grid_column")
        .and_then(|v| v.clone().into_string().ok())
    {
        style.grid_column = parse_line(&s);
    }

    // Aspect Ratio (e.g. "16:9", 1.5, or "1.5")
    if let Some(v) = props.get("aspect_ratio") {
        if let Ok(f) = v.as_float() {
            style.aspect_ratio = Some(f as f32);
        } else if let Ok(s) = v.clone().into_string() {
            if s.contains(':') {
                let parts: Vec<&str> = s.split(':').collect();
                if parts.len() == 2 {
                    if let (Ok(w), Ok(h)) = (parts[0].parse::<f32>(), parts[1].parse::<f32>()) {
                        if h > 0.0 {
                            style.aspect_ratio = Some(w / h);
                        }
                    }
                }
            } else if let Ok(val) = s.parse::<f32>() {
                style.aspect_ratio = Some(val);
            }
        }
    }

    // Position (Relative/Absolute)
    if let Some(s) = props
        .get("position")
        .and_then(|v| v.clone().into_string().ok())
    {
        style.position = match s.as_str() {
            "absolute" => taffy::style::Position::Absolute,
            _ => taffy::style::Position::Relative,
        };
    }

    // Insets (Top, Left, Right, Bottom) - reuse to_lpa closure
    if let Some(v) = props.get("left").and_then(to_lpa) {
        style.inset.left = v;
    }
    if let Some(v) = props.get("right").and_then(to_lpa) {
        style.inset.right = v;
    }
    if let Some(v) = props.get("top").and_then(to_lpa) {
        style.inset.top = v;
    }
    if let Some(v) = props.get("bottom").and_then(to_lpa) {
        style.inset.bottom = v;
    }
}

/// Convert a Taffy `Style` into a JSON map using the same key vocabulary
/// consumed by `parse_layout_style`, so a project save/load round-trip can
/// faithfully restore node layout.
///
/// Only the layout knobs `parse_layout_style` understands are emitted; fields
/// it does not handle (e.g. `border`, `flex_basis`, `flex_wrap`, `align_self`,
/// `min_size`, grid templates) are intentionally omitted and remain
/// un-round-tripped for this slice.
pub fn style_to_json(style: &Style) -> JsonMap<String, JsonValue> {
    let mut m = JsonMap::new();

    // In taffy 0.9 the length types are newtype structs over a `CompactLength`
    // tagged value. `is_auto()` / `value()` read the numeric payload and
    // `tag()` distinguishes Length vs Percent (vs Auto).
    let dim = |d: &taffy::style::Dimension| -> JsonValue {
        if d.is_auto() {
            JsonValue::from("auto")
        } else if d.into_raw().tag() == taffy::style::CompactLength::PERCENT_TAG {
            JsonValue::from(format!("{}%", d.into_raw().value() * 100.0))
        } else {
            JsonValue::from(d.into_raw().value() as f64)
        }
    };
    let lp = |v: &taffy::style::LengthPercentage| -> JsonValue {
        if v.into_raw().tag() == taffy::style::CompactLength::PERCENT_TAG {
            JsonValue::from(format!("{}%", v.into_raw().value() * 100.0))
        } else {
            JsonValue::from(v.into_raw().value() as f64)
        }
    };
    let lpa = |v: &taffy::style::LengthPercentageAuto| -> JsonValue {
        if v.is_auto() {
            JsonValue::from("auto")
        } else if v.into_raw().tag() == taffy::style::CompactLength::PERCENT_TAG {
            JsonValue::from(format!("{}%", v.into_raw().value() * 100.0))
        } else {
            JsonValue::from(v.into_raw().value() as f64)
        }
    };

    m.insert(
        "display".into(),
        JsonValue::from(match style.display {
            taffy::style::Display::Grid => "grid",
            taffy::style::Display::None => "none",
            _ => "flex",
        }),
    );
    m.insert(
        "position".into(),
        JsonValue::from(match style.position {
            taffy::style::Position::Absolute => "absolute",
            _ => "relative",
        }),
    );
    m.insert("width".into(), dim(&style.size.width));
    m.insert("height".into(), dim(&style.size.height));
    m.insert("max_width".into(), dim(&style.max_size.width));
    m.insert("max_height".into(), dim(&style.max_size.height));
    m.insert("left".into(), lpa(&style.inset.left));
    m.insert("right".into(), lpa(&style.inset.right));
    m.insert("top".into(), lpa(&style.inset.top));
    m.insert("bottom".into(), lpa(&style.inset.bottom));
    m.insert(
        "flex_direction".into(),
        JsonValue::from(match style.flex_direction {
            taffy::style::FlexDirection::Row => "row",
            taffy::style::FlexDirection::Column => "column",
            taffy::style::FlexDirection::RowReverse => "row_reverse",
            taffy::style::FlexDirection::ColumnReverse => "column_reverse",
        }),
    );
    m.insert(
        "flex_wrap".into(),
        JsonValue::from(match style.flex_wrap {
            taffy::style::FlexWrap::Wrap => "wrap",
            taffy::style::FlexWrap::WrapReverse => "wrap_reverse",
            _ => "no_wrap",
        }),
    );
    if let Some(a) = style.align_items {
        m.insert(
            "align_items".into(),
            JsonValue::from(match a {
                taffy::style::AlignItems::Center => "center",
                taffy::style::AlignItems::FlexStart => "flex_start",
                taffy::style::AlignItems::FlexEnd => "flex_end",
                _ => "stretch",
            }),
        );
    }
    if let Some(j) = style.justify_content {
        m.insert(
            "justify_content".into(),
            JsonValue::from(match j {
                taffy::style::JustifyContent::Center => "center",
                taffy::style::JustifyContent::FlexStart => "flex_start",
                taffy::style::JustifyContent::FlexEnd => "flex_end",
                taffy::style::JustifyContent::SpaceBetween => "space_between",
                taffy::style::JustifyContent::SpaceAround => "space_around",
                taffy::style::JustifyContent::SpaceEvenly => "space_evenly",
                _ => "flex_start",
            }),
        );
    }
    m.insert("flex_grow".into(), JsonValue::from(style.flex_grow));
    m.insert("flex_shrink".into(), JsonValue::from(style.flex_shrink));
    if style.padding.left == style.padding.right
        && style.padding.left == style.padding.top
        && style.padding.left == style.padding.bottom
    {
        m.insert("padding".into(), lp(&style.padding.left));
    } else {
        m.insert("padding_top".into(), lp(&style.padding.top));
        m.insert("padding_right".into(), lp(&style.padding.right));
        m.insert("padding_bottom".into(), lp(&style.padding.bottom));
        m.insert("padding_left".into(), lp(&style.padding.left));
    }
    if style.margin.left == style.margin.right
        && style.margin.left == style.margin.top
        && style.margin.left == style.margin.bottom
    {
        m.insert("margin".into(), lpa(&style.margin.left));
    } else {
        m.insert("margin_top".into(), lpa(&style.margin.top));
        m.insert("margin_right".into(), lpa(&style.margin.right));
        m.insert("margin_bottom".into(), lpa(&style.margin.bottom));
        m.insert("margin_left".into(), lpa(&style.margin.left));
    }
    if style.gap.width == style.gap.height {
        m.insert("gap".into(), lp(&style.gap.width));
    }
    if let Some(ar) = style.aspect_ratio {
        m.insert("aspect_ratio".into(), JsonValue::from(ar));
    }
    m
}

/// Restore a Taffy `Style` from a JSON map produced by `style_to_json`, by
/// routing it through `parse_layout_style` (the canonical Rhai layout parser).
pub fn apply_layout_style_json(style: &mut Style, json: &JsonMap<String, JsonValue>) {
    let mut map: rhai::Map = rhai::Map::new();
    for (k, v) in json {
        map.insert(k.clone().into(), json_to_rhai(v));
    }
    parse_layout_style(&map, style);
}

/// Convert a `serde_json::Value` into a `rhai::Dynamic` for the subset of
/// types `parse_layout_style` reads (numbers, strings, arrays of strings).
fn json_to_rhai(v: &JsonValue) -> Dynamic {
    match v {
        JsonValue::Null => Dynamic::UNIT,
        JsonValue::Bool(b) => Dynamic::from(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Dynamic::from(i)
            } else {
                Dynamic::from(n.as_f64().unwrap_or(0.0))
            }
        }
        JsonValue::String(s) => Dynamic::from(s.clone()),
        JsonValue::Array(a) => Dynamic::from(a.iter().map(json_to_rhai).collect::<Vec<Dynamic>>()),
        JsonValue::Object(o) => {
            let mut m: rhai::Map = rhai::Map::new();
            for (k, v) in o {
                m.insert(k.clone().into(), json_to_rhai(v));
            }
            Dynamic::from(m)
        }
    }
}

/// Build a `rhai::Map` from a `serde_json::Map` so the canonical Rhai parsers
/// (`parse_layout_style`, `apply_text_props`, ...) can consume it.
pub fn json_map_to_rhai(json: &JsonMap<String, JsonValue>) -> rhai::Map {
    let mut map: rhai::Map = rhai::Map::new();
    for (k, v) in json {
        map.insert(k.clone().into(), json_to_rhai(v));
    }
    map
}

/// Serialize a `TextNode` into the same Rhai-props vocabulary consumed by
/// `parse_spans_from_dynamic` + `apply_text_props` + `parse_text_shadow`, so a
/// project save/load round-trip can faithfully restore rich text. Layout is
/// intentionally excluded (see `style_to_json`); merge the two maps on load.
pub fn text_node_to_json(t: &crate::node::text::TextNode) -> JsonMap<String, JsonValue> {
    let mut m = JsonMap::new();

    let spans: Vec<JsonValue> = t
        .spans
        .iter()
        .map(|s| {
            let mut sm = JsonMap::new();
            sm.insert("text".into(), JsonValue::from(s.text.clone()));
            if let Some(c) = s.color {
                sm.insert("color".into(), JsonValue::from(color_hex(c)));
            }
            if let Some(w) = s.font_weight {
                sm.insert("weight".into(), JsonValue::from(w as i64));
            }
            if let Some(fs) = s.font_size {
                sm.insert("size".into(), JsonValue::from(fs as f64));
            }
            if let Some(f) = &s.font_family {
                sm.insert("font_family".into(), JsonValue::from(f.clone()));
            }
            if let Some(st) = &s.font_style {
                sm.insert("font_style".into(), JsonValue::from(st.clone()));
            }
            if let Some(c) = s.background_color {
                sm.insert("background_color".into(), JsonValue::from(color_hex(c)));
            }
            if let Some(p) = s.background_padding {
                sm.insert("background_padding".into(), JsonValue::from(p as f64));
            }
            if let Some(w) = s.stroke_width {
                sm.insert("stroke_width".into(), JsonValue::from(w as f64));
            }
            if let Some(c) = s.stroke_color {
                sm.insert("stroke_color".into(), JsonValue::from(color_hex(c)));
            }
            if let Some(ls) = s.letter_spacing {
                sm.insert("letter_spacing".into(), JsonValue::from(ls as f64));
            }
            if let Some(ws) = s.word_spacing {
                sm.insert("word_spacing".into(), JsonValue::from(ws as f64));
            }
            if let Some(lh) = s.line_height {
                sm.insert("line_height".into(), JsonValue::from(lh as f64));
            }
            JsonValue::Object(sm)
        })
        .collect();
    m.insert("content".into(), JsonValue::Array(spans));

    m.insert("size".into(), JsonValue::from(t.default_font_size.current_value as f64));
    m.insert("color".into(), JsonValue::from(color_hex(t.default_color.current_value)));

    match t.fit_mode {
        crate::element::TextFit::Shrink => {
            m.insert("fit".into(), JsonValue::from("shrink"));
        }
        crate::element::TextFit::None => {
            m.insert("fit".into(), JsonValue::from("auto"));
        }
    }
    m.insert("min_size".into(), JsonValue::from(t.min_size as f64));
    m.insert("max_size".into(), JsonValue::from(t.max_size as f64));
    m.insert(
        "align".into(),
        JsonValue::from(match t.text_align {
            skia_safe::textlayout::TextAlign::Center => "center",
            skia_safe::textlayout::TextAlign::Right => "right",
            skia_safe::textlayout::TextAlign::Justify => "justify",
            _ => "left",
        }),
    );
    m.insert(
        "text_direction".into(),
        JsonValue::from(match t.text_direction {
            crate::text_direction::TextDirectionMode::Ltr => "ltr",
            crate::text_direction::TextDirectionMode::Rtl => "rtl",
            _ => "auto",
        }),
    );
    // Rhai text props carry one shadow, so the first (top) shadow round-trips.
    if let Some(sh) = t.shadows.first() {
        m.insert("text_shadow_color".into(), JsonValue::from(color_hex(sh.color)));
        m.insert("text_shadow_blur".into(), JsonValue::from(sh.blur as f64));
        m.insert("text_shadow_x".into(), JsonValue::from(sh.offset.0 as f64));
        m.insert("text_shadow_y".into(), JsonValue::from(sh.offset.1 as f64));
    }
    m
}

/// `#rrggbb` (or `#rrggbbaa`) hex string for a `Color`, matching the format
/// `parse_hex_color` / `parse_text_style` consume.
fn color_hex(c: Color) -> String {
    if (c.a * 255.0).round() as u8 == 255 {
        format!(
            "#{:02x}{:02x}{:02x}",
            (c.r * 255.0) as u8,
            (c.g * 255.0) as u8,
            (c.b * 255.0) as u8
        )
    } else {
        format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            (c.r * 255.0) as u8,
            (c.g * 255.0) as u8,
            (c.b * 255.0) as u8,
            (c.a * 255.0) as u8
        )
    }
}

/// Parse object fit mode from string.
pub fn parse_object_fit(val: &str) -> Option<ObjectFit> {
    match val {
        "cover" => Some(ObjectFit::Cover),
        "contain" => Some(ObjectFit::Contain),
        "fill" => Some(ObjectFit::Fill),
        _ => None,
    }
}

/// Parse text spans from a Rhai dynamic value.
pub fn parse_spans_from_dynamic(content: rhai::Dynamic) -> Vec<TextSpan> {
    let mut spans = Vec::new();
    if let Ok(arr) = content.clone().into_array() {
        for item in arr {
            if let Some(map) = item.clone().try_cast::<Map>() {
                let text = map.get("text").map(|v| v.to_string()).unwrap_or_default();
                let mut span = TextSpan {
                    text,
                    color: None,
                    font_family: None,
                    font_weight: None,
                    font_style: None,
                    font_size: None,
                    background_color: None,
                    background_padding: None,
                    stroke_width: None,
                    stroke_color: None,
                    fill_gradient: None,
                    letter_spacing: None,
                    word_spacing: None,
                    line_height: None,
                    font_variations: None,
                };
                parse_text_style(&map, &mut span);
                spans.push(span);
            } else if let Ok(s) = item.into_string() {
                spans.push(TextSpan {
                    text: s,
                    color: None,
                    font_family: None,
                    font_weight: None,
                    font_style: None,
                    font_size: None,
                    background_color: None,
                    background_padding: None,
                    stroke_width: None,
                    stroke_color: None,
                    fill_gradient: None,
                    letter_spacing: None,
                    word_spacing: None,
                    line_height: None,
                    font_variations: None,
                });
            }
        }
    } else if let Ok(s) = content.into_string() {
        spans.push(TextSpan {
            text: s,
            color: None,
            font_family: None,
            font_weight: None,
            font_style: None,
            font_size: None,
            background_color: None,
            background_padding: None,
            stroke_width: None,
            stroke_color: None,
            fill_gradient: None,
            letter_spacing: None,
            word_spacing: None,
            line_height: None,
            font_variations: None,
        });
    }
    spans
}

/// Parse a text shadow from Rhai props map.
pub fn parse_text_shadow(props: &rhai::Map) -> Option<TextShadow> {
    let mut has_shadow = false;
    let mut shadow = TextShadow {
        color: Color::BLACK,
        blur: 0.0,
        offset: (0.0, 0.0),
    };
    if let Some(c) = props
        .get("text_shadow_color")
        .and_then(|v| v.clone().into_string().ok())
    {
        if let Some(col) = parse_hex_color(&c) {
            shadow.color = col;
            has_shadow = true;
        }
    }
    if let Some(v) = props
        .get("text_shadow_blur")
        .and_then(|v| v.as_float().ok())
    {
        shadow.blur = v as f32;
        has_shadow = true;
    }
    if let Some(v) = props.get("text_shadow_x").and_then(|v| v.as_float().ok()) {
        shadow.offset.0 = v as f32;
        has_shadow = true;
    }
    if let Some(v) = props.get("text_shadow_y").and_then(|v| v.as_float().ok()) {
        shadow.offset.1 = v as f32;
        has_shadow = true;
    }
    if has_shadow {
        Some(shadow)
    } else {
        None
    }
}
