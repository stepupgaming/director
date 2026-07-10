//! # Template System
//!
//! Higher-level layout primitives for common short-form video patterns.
//!
//! ## Philosophy
//! Templates are pre-wired scene graph structures that solve common layout
//! problems without requiring users to manually configure flexbox/grid styles.
//! They are sugar over `add_box`, `add_text`, `add_image`, etc.
//!
//! ## Templates
//! - **Title Card**: Full-screen centered title + subtitle
//! - **Lower Third**: Bottom banner with text
//! - **Split Screen**: Side-by-side content panels
//! - **Reaction Overlay**: Main video + facecam corner
//! - **Bento Grid**: Grid of content cards
//! - **Watermark**: Corner logo/text overlay
//! - **Subscribe Button**: Channel info + CTA button pill
//! - **Progress Bar**: Percentage fill with label
//! - **Countdown Timer**: Days/hours/minutes/seconds segments
//! - **Quote Card**: Styled quote with attribution and accent bar
//! - **Social Handle Overlay**: Platform icon circles with handles
//!
//! ## Usage (Rhai)
//! ```rhai
//! let title = scene.add_title_card(#{
//!     title: "My Video",
//!     subtitle: "Part 1",
//!     bg_color: "#1a1a2e",
//!     title_color: "#ffffff"
//! });
//! ```

use crate::animation::Animated;
use crate::element::{TextFit, TextSpan};
use crate::node::{BoxNode, ImageNode, SpectrumVisualizerNode, TextNode};
use crate::scripting::types::{NodeHandle, SceneHandle};
use crate::scripting::utils::{parse_hex_color, parse_layout_style, parse_object_fit};
use crate::types::Color;
use rhai::Engine;
use skia_safe::textlayout::TextAlign as SkTextAlign;
use taffy::prelude::*;

/// Register all template functions into the Rhai engine.
pub fn register(engine: &mut Engine) {
    engine.register_fn(
        "add_title_card",
        |scene: &mut SceneHandle, props: rhai::Map| -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            build_title_card(scene, &props)
        },
    );

    engine.register_fn(
        "add_lower_third",
        |scene: &mut SceneHandle, props: rhai::Map| -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            build_lower_third(scene, &props)
        },
    );

    engine.register_fn(
        "add_split_screen",
        |scene: &mut SceneHandle, props: rhai::Map| -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            build_split_screen(scene, &props)
        },
    );

    engine.register_fn(
        "add_reaction_overlay",
        |scene: &mut SceneHandle, props: rhai::Map| -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            build_reaction_overlay(scene, &props)
        },
    );

    engine.register_fn(
        "add_bento_grid",
        |scene: &mut SceneHandle, props: rhai::Map| -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            build_bento_grid(scene, &props)
        },
    );

    engine.register_fn(
        "add_watermark",
        |scene: &mut SceneHandle, props: rhai::Map| -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            build_watermark(scene, &props)
        },
    );

    engine.register_fn(
        "add_subscribe_button",
        |scene: &mut SceneHandle, props: rhai::Map| -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            build_subscribe_button(scene, &props)
        },
    );

    engine.register_fn(
        "add_progress_bar",
        |scene: &mut SceneHandle, props: rhai::Map| -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            build_progress_bar(scene, &props)
        },
    );

    engine.register_fn(
        "add_countdown_timer",
        |scene: &mut SceneHandle, props: rhai::Map| -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            build_countdown_timer(scene, &props)
        },
    );

    engine.register_fn(
        "add_quote_card",
        |scene: &mut SceneHandle, props: rhai::Map| -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            build_quote_card(scene, &props)
        },
    );

    engine.register_fn(
        "add_social_handle_overlay",
        |scene: &mut SceneHandle, props: rhai::Map| -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            build_social_handle_overlay(scene, &props)
        },
    );

    engine.register_fn(
        "add_counter",
        |scene: &mut SceneHandle, props: rhai::Map| -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            build_counter(scene, &props)
        },
    );

    engine.register_fn(
        "add_waveform_visualizer",
        |scene: &mut SceneHandle, props: rhai::Map| -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
            build_waveform_visualizer(scene, &props)
        },
    );
}

// ============================================================================
// Helpers
// ============================================================================

fn make_text_node(d: &mut crate::director::Director, text: &str, color: Option<Color>, size: f32, weight: Option<u16>, font_family: Option<String>) -> TextNode {
    let span = TextSpan {
        text: text.to_string(),
        color,
        font_family: font_family.clone(),
        font_weight: weight,
        font_style: None,
        font_size: Some(size),
        background_color: None,
        background_padding: None,
        stroke_width: None,
        stroke_color: None,
        fill_gradient: None,
        letter_spacing: None,
        line_height: None,
    };
    let mut node = TextNode::new(vec![span], d.assets.font_collection.clone());
    node.default_font_size = Animated::new(size);
    if let Some(c) = color {
        node.default_color = Animated::new(c);
    }
    node.init_paragraph();
    node
}

fn make_box_node(bg_color: Option<Color>) -> BoxNode {
    let mut node = BoxNode::new();
    if let Some(c) = bg_color {
        node.bg_color = Some(Animated::new(c));
    }
    node
}

fn apply_box_shadow(node: &mut BoxNode, props: &rhai::Map) {
    if let Some(c) = props.get("shadow_color").and_then(|v| v.clone().into_string().ok()) {
        if let Some(color) = parse_hex_color(&c) {
            node.shadow_color = Some(Animated::new(color));
        }
    }
    if let Some(v) = props.get("shadow_blur").and_then(|v| v.as_float().ok()) {
        node.shadow_blur = Animated::new(v as f32);
    }
    if let Some(v) = props.get("shadow_x").and_then(|v| v.as_float().ok()) {
        node.shadow_offset_x = Animated::new(v as f32);
    }
    if let Some(v) = props.get("shadow_y").and_then(|v| v.as_float().ok()) {
        node.shadow_offset_y = Animated::new(v as f32);
    }
}

fn set_root_z_index(scene: &mut crate::scene::SceneGraph, root_id: usize, props: &rhai::Map) {
    if let Some(z) = props.get("z_index").and_then(|v| v.as_int().ok()) {
        if let Some(n) = scene.get_node_mut(root_id) {
            n.z_index = z as i32;
        }
    }
}

fn attach_child(
    scene: &mut crate::scene::SceneGraph,
    parent: usize,
    child: usize,
) -> Result<(), String> {
    if !scene.try_add_child(parent, child) {
        scene.destroy_node(child);
        return Err(format!("Failed to attach node {} to parent {}", child, parent));
    }
    Ok(())
}

fn to_eval_err(e: String) -> Box<rhai::EvalAltResult> {
    e.into()
}

fn attach_image_or_placeholder(
    scene: &mut crate::scene::SceneGraph,
    parent_id: usize,
    image: Option<skia_safe::Image>,
    size: Size<Dimension>,
    object_fit: crate::types::ObjectFit,
    fallback_color: Color,
    opacity: Option<f32>,
) -> Result<(), String> {
    if let Some(img_data) = image {
        let mut img = ImageNode::from_image(Some(img_data));
        img.style.size = size;
        img.object_fit = object_fit;
        if let Some(op) = opacity {
            img.opacity = Animated::new(op);
        }
        let img_id = scene.add_node(Box::new(img));
        attach_child(scene, parent_id, img_id)?;
    } else {
        let mut placeholder = make_box_node(Some(fallback_color));
        placeholder.style.size = size;
        let ph_id = scene.add_node(Box::new(placeholder));
        attach_child(scene, parent_id, ph_id)?;
    }
    Ok(())
}

// ============================================================================
// Title Card
// ============================================================================

fn build_title_card(
    scene: &mut SceneHandle,
    props: &rhai::Map,
) -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
    let mut d = scene.lock_director()?;

    let bg_color = props
        .get("bg_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s));
    let mut root = make_box_node(bg_color);
    parse_layout_style(props, &mut root.style);
    apply_box_shadow(&mut root, props);
    root.style.position = taffy::style::Position::Absolute;
    root.style.inset = Rect {
        left: LengthPercentageAuto::length(0.0),
        right: LengthPercentageAuto::length(0.0),
        top: LengthPercentageAuto::length(0.0),
        bottom: LengthPercentageAuto::length(0.0),
    };
    root.style.size = Size {
        width: Dimension::percent(1.0),
        height: Dimension::percent(1.0),
    };
    root.style.flex_direction = FlexDirection::Column;
    root.style.align_items = Some(AlignItems::Center);
    root.style.justify_content = Some(JustifyContent::Center);
    root.style.gap = Size::zero();

    let root_id = d.scene.add_node(Box::new(root));
    if !d.scene.try_add_child(scene.root_id, root_id) {
        d.scene.destroy_node(root_id);
        return Err("Failed to attach title card to scene".into());
    }
    set_root_z_index(&mut d.scene, root_id, props);

    let font_family = props.get("font_family").and_then(|v| v.clone().into_string().ok());
    let title_text = props
        .get("title")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Title".to_string());
    let title_color = props
        .get("title_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s));
    let title_size = props
        .get("title_size")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(72.0) as f32;
    let title_weight = props
        .get("title_bold")
        .and_then(|v| v.as_bool().ok())
        .filter(|&b| b)
        .map(|_| 700);

    let mut title_node = make_text_node(&mut d, &title_text, title_color, title_size, title_weight, font_family.clone());
    title_node.fit_mode = TextFit::Shrink;
    title_node.set_text_align(SkTextAlign::Center);
    title_node.style.size = Size {
        width: Dimension::percent(0.9),
        height: Dimension::auto(),
    };
    title_node.style.margin = Rect {
        left: LengthPercentageAuto::length(0.0),
        right: LengthPercentageAuto::length(0.0),
        top: LengthPercentageAuto::length(0.0),
        bottom: LengthPercentageAuto::length(8.0),
    };
    let title_id = d.scene.add_node(Box::new(title_node));
    attach_child(&mut d.scene, root_id, title_id).map_err(to_eval_err)?;

    if let Some(subtitle_text) = props.get("subtitle").map(|v| v.to_string()) {
        let subtitle_color = props
            .get("subtitle_color")
            .and_then(|v| v.clone().into_string().ok())
            .and_then(|s| parse_hex_color(&s));
    let subtitle_size = props
        .get("subtitle_size")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(48.0) as f32;
        let subtitle_weight = props
            .get("subtitle_bold")
            .and_then(|v| v.as_bool().ok())
            .filter(|&b| b)
            .map(|_| 700);

        let mut sub_node = make_text_node(&mut d, &subtitle_text, subtitle_color, subtitle_size, subtitle_weight, font_family.clone());
        sub_node.set_text_align(SkTextAlign::Center);
        // Use auto-width so the subtitle fits to its text content rather than
        // stretching to a fixed percentage. max_size caps it at 90% of frame.
        sub_node.style.size = Size {
            width: Dimension::auto(),
            height: Dimension::auto(),
        };
        sub_node.style.max_size = Size {
            width: Dimension::percent(0.9),
            height: Dimension::auto(),
        };
        let sub_id = d.scene.add_node(Box::new(sub_node));
        attach_child(&mut d.scene, root_id, sub_id).map_err(to_eval_err)?;
    }

    Ok(NodeHandle {
        director: scene.director.clone(),
        id: root_id,
            invalidated: false,
    })
}

// ============================================================================
// Counter / Number Ticker
// ============================================================================

fn build_counter(
    scene: &mut SceneHandle,
    props: &rhai::Map,
) -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
    let mut d = scene.lock_director()?;

    let start = props.get("start").and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
    let end = props.get("end").and_then(|v| v.as_float().ok()).unwrap_or(100.0) as f32;
    let duration = props.get("duration").and_then(|v| v.as_float().ok()).unwrap_or(2.0) as f64;
    let decimals = props.get("decimals").and_then(|v| v.as_int().ok()).unwrap_or(0) as usize;
    let prefix = props.get("prefix").map(|v| v.to_string()).unwrap_or_default();
    let suffix = props.get("suffix").map(|v| v.to_string()).unwrap_or_default();
    let thousands_sep = props
        .get("thousands_sep")
        .and_then(|v| v.as_bool().ok())
        .unwrap_or(false);
    let text_color = props
        .get("text_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s))
        .unwrap_or(Color::new(1.0, 1.0, 1.0, 1.0));
    let font_size = props
        .get("font_size")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(72.0) as f32;
    let font_family = props.get("font_family").and_then(|v| v.clone().into_string().ok());
    let easing = props
        .get("easing")
        .and_then(|v| v.clone().into_string().ok())
        .unwrap_or_else(|| "ease_out".to_string());

    // Create root box
    let mut root = make_box_node(None);
    root.style.position = taffy::style::Position::Absolute;
    root.style.inset = Rect {
        left: LengthPercentageAuto::length(0.0),
        right: LengthPercentageAuto::length(0.0),
        top: LengthPercentageAuto::length(0.0),
        bottom: LengthPercentageAuto::length(0.0),
    };
    root.style.size = Size {
        width: Dimension::percent(1.0),
        height: Dimension::percent(1.0),
    };
    root.style.flex_direction = FlexDirection::Column;
    root.style.align_items = Some(AlignItems::Center);
    root.style.justify_content = Some(JustifyContent::Center);

    let root_id = d.scene.add_node(Box::new(root));
    if !d.scene.try_add_child(scene.root_id, root_id) {
        d.scene.destroy_node(root_id);
        return Err("Failed to attach counter to scene".into());
    }
    set_root_z_index(&mut d.scene, root_id, props);

    // Build counter config first so we can format the initial text correctly
    let counter_cfg = crate::node::text::CounterConfig {
        decimals,
        prefix,
        suffix,
        thousands_sep,
    };

    // Create text node with the fully formatted initial value
    let initial_text = counter_cfg.format_value(start);
    let mut text_node = make_text_node(&mut d, &initial_text, Some(text_color), font_size, Some(800), font_family);
    text_node.style.size = Size {
        width: Dimension::auto(),
        height: Dimension::auto(),
    };
    let text_id = d.scene.add_node(Box::new(text_node));
    attach_child(&mut d.scene, root_id, text_id).map_err(to_eval_err)?;

    // Configure counter animation on the text node: per-frame interpolation
    // of the numeric value with the given formatting, easing, and duration.
    if let Some(node) = d.scene.get_node_mut(text_id) {
        if let Some(text_node) = node.element.as_any_mut().downcast_mut::<TextNode>() {
            text_node.counter = Some(counter_cfg);
            text_node.counter_value = crate::animation::Animated::new(start);
            let ease_fn = crate::node::parse_easing(&easing);
            text_node.counter_value.add_segment(start, end, duration, ease_fn);
        }
    }

    Ok(NodeHandle {
        director: scene.director.clone(),
        id: root_id,
            invalidated: false,
    })
}

// ============================================================================
// Audio Waveform Visualizer
// ============================================================================

fn build_waveform_visualizer(
    scene: &mut SceneHandle,
    props: &rhai::Map,
) -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
    let mut d = scene.lock_director()?;

    let bar_count = props.get("bars").and_then(|v| v.as_int().ok()).unwrap_or(32) as usize;
    let bar_color = props
        .get("bar_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s))
        .unwrap_or(Color::new(0.3, 0.7, 1.0, 1.0));
    let bar_gap = props
        .get("gap")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(4.0) as f32;
    let max_height_pct = props
        .get("max_height")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(80.0) as f32 / 100.0;
    let min_height_pct = props
        .get("min_height")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(2.0) as f32 / 100.0;
    let position = props
        .get("position")
        .and_then(|v| v.clone().into_string().ok())
        .unwrap_or_else(|| "bottom".to_string());
    let track_id = props
        .get("track_id")
        .and_then(|v| v.as_int().ok())
        .unwrap_or(0) as usize;
    let smoothing = props
        .get("smoothing")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(0.6) as f32;
    let border_radius = props
        .get("border_radius")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(2.0) as f32;

    let mut visualizer = SpectrumVisualizerNode::new(
        track_id,
        bar_count.max(1),
        bar_color,
        bar_gap,
        smoothing,
        position,
        min_height_pct,
        max_height_pct,
        border_radius,
    );

    // Position the visualizer within the scene
    visualizer.style.position = taffy::style::Position::Absolute;
    visualizer.style.inset = Rect {
        left: LengthPercentageAuto::length(0.0),
        right: LengthPercentageAuto::length(0.0),
        top: LengthPercentageAuto::length(0.0),
        bottom: LengthPercentageAuto::length(0.0),
    };
    visualizer.style.size = Size {
        width: Dimension::percent(1.0),
        height: Dimension::percent(1.0),
    };
    visualizer.style.padding = Rect {
        left: LengthPercentage::length(40.0),
        right: LengthPercentage::length(40.0),
        top: LengthPercentage::length(40.0),
        bottom: LengthPercentage::length(40.0),
    };

    let root_id = d.scene.add_node(Box::new(visualizer));
    if !d.scene.try_add_child(scene.root_id, root_id) {
        d.scene.destroy_node(root_id);
        return Err("Failed to attach waveform visualizer to scene".into());
    }
    set_root_z_index(&mut d.scene, root_id, props);

    Ok(NodeHandle {
        director: scene.director.clone(),
        id: root_id,
            invalidated: false,
    })
}

// ============================================================================
// Lower Third
// ============================================================================

fn build_lower_third(
    scene: &mut SceneHandle,
    props: &rhai::Map,
) -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
    let mut d = scene.lock_director()?;

    let text_content = props
        .get("text")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Lower Third".to_string());
    let bg_color = props
        .get("bg_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s));
    let text_color = props
        .get("text_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s));
    let text_size = props
        .get("text_size")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(32.0) as f32;
    let height_pct = props
        .get("height")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(20.0) as f32 / 100.0;
    let margin = props
        .get("margin")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(0.0) as f32;

    let mut root = make_box_node(bg_color);
    parse_layout_style(props, &mut root.style);
    apply_box_shadow(&mut root, props);
    root.style.position = taffy::style::Position::Absolute;
    root.style.inset = Rect {
        left: LengthPercentageAuto::length(0.0),
        right: LengthPercentageAuto::length(0.0),
        top: LengthPercentageAuto::auto(),
        bottom: LengthPercentageAuto::length(margin),
    };
    root.style.size = Size {
        width: Dimension::percent(1.0),
        height: Dimension::percent(height_pct),
    };
    root.style.flex_direction = FlexDirection::Row;
    root.style.align_items = Some(AlignItems::Center);
    root.style.padding = Rect {
        left: LengthPercentage::length(32.0),
        right: LengthPercentage::length(32.0),
        top: LengthPercentage::length(16.0),
        bottom: LengthPercentage::length(16.0),
    };

    let accent_color = props
        .get("accent_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s));

    let root_id = d.scene.add_node(Box::new(root));
    if !d.scene.try_add_child(scene.root_id, root_id) {
        d.scene.destroy_node(root_id);
        return Err("Failed to attach lower third to scene".into());
    }
    set_root_z_index(&mut d.scene, root_id, props);

    if let Some(accent) = accent_color {
        let mut bar = make_box_node(Some(accent));
        bar.style.size = Size {
            width: Dimension::length(6.0),
            height: Dimension::percent(0.7),
        };
        bar.style.margin = Rect {
            left: LengthPercentageAuto::length(0.0),
            right: LengthPercentageAuto::length(12.0),
            top: LengthPercentageAuto::length(0.0),
            bottom: LengthPercentageAuto::length(0.0),
        };
        let bar_id = d.scene.add_node(Box::new(bar));
        attach_child(&mut d.scene, root_id, bar_id).map_err(to_eval_err)?;
    }

    let font_family = props.get("font_family").and_then(|v| v.clone().into_string().ok());
    let mut text_node = make_text_node(&mut d, &text_content, text_color, text_size, Some(600), font_family);
    text_node.fit_mode = TextFit::Shrink;
    text_node.style.flex_grow = 1.0;
    let text_id = d.scene.add_node(Box::new(text_node));
    attach_child(&mut d.scene, root_id, text_id).map_err(to_eval_err)?;

    Ok(NodeHandle {
        director: scene.director.clone(),
        id: root_id,
            invalidated: false,
    })
}

// ============================================================================
// Split Screen
// ============================================================================

fn build_split_screen(
    scene: &mut SceneHandle,
    props: &rhai::Map,
) -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
    let mut d = scene.lock_director()?;

    let ratio = props
        .get("ratio")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(0.5) as f32;
    let gap = props
        .get("gap")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(0.0) as f32;
    let vertical = props
        .get("vertical")
        .and_then(|v| v.as_bool().ok())
        .unwrap_or(false);

    let left_path = props
        .get("left")
        .or_else(|| props.get("top"))
        .and_then(|v| v.clone().into_string().ok());
    let right_path = props
        .get("right")
        .or_else(|| props.get("bottom"))
        .and_then(|v| v.clone().into_string().ok());
    let left_color = props
        .get("left_color")
        .or_else(|| props.get("top_color"))
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s));
    let right_color = props
        .get("right_color")
        .or_else(|| props.get("bottom_color"))
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s));

    let mut root = make_box_node(None);
    parse_layout_style(props, &mut root.style);
    root.style.position = taffy::style::Position::Absolute;
    root.style.inset = Rect {
        left: LengthPercentageAuto::length(0.0),
        right: LengthPercentageAuto::length(0.0),
        top: LengthPercentageAuto::length(0.0),
        bottom: LengthPercentageAuto::length(0.0),
    };
    root.style.size = Size {
        width: Dimension::percent(1.0),
        height: Dimension::percent(1.0),
    };
    root.style.flex_direction = if vertical {
        FlexDirection::Column
    } else {
        FlexDirection::Row
    };
    root.style.gap = Size {
        width: LengthPercentage::length(gap),
        height: LengthPercentage::length(gap),
    };

    let root_id = d.scene.add_node(Box::new(root));
    if !d.scene.try_add_child(scene.root_id, root_id) {
        d.scene.destroy_node(root_id);
        return Err("Failed to attach split screen to scene".into());
    }
    set_root_z_index(&mut d.scene, root_id, props);

    let mut left_panel = make_box_node(left_color);
    left_panel.style.flex_grow = ratio;
    left_panel.style.flex_shrink = 1.0;
    left_panel.style.size = Size {
        width: Dimension::auto(),
        height: Dimension::auto(),
    };
    let left_id = d.scene.add_node(Box::new(left_panel));
    attach_child(&mut d.scene, root_id, left_id).map_err(to_eval_err)?;

    if let Some(path) = left_path {
        let image = d.assets.load_image(&path);
        let fallback_color = left_color.unwrap_or(Color::new(0.2, 0.2, 0.3, 1.0));
        attach_image_or_placeholder(
            &mut d.scene,
            left_id,
            image,
            Size {
                width: Dimension::percent(1.0),
                height: Dimension::percent(1.0),
            },
            crate::types::ObjectFit::Cover,
            fallback_color,
            None,
        ).map_err(to_eval_err)?;
    }

    let mut right_panel = make_box_node(right_color);
    right_panel.style.flex_grow = 1.0 - ratio;
    right_panel.style.flex_shrink = 1.0;
    right_panel.style.size = Size {
        width: Dimension::auto(),
        height: Dimension::auto(),
    };
    let right_id = d.scene.add_node(Box::new(right_panel));
    attach_child(&mut d.scene, root_id, right_id).map_err(to_eval_err)?;

    if let Some(path) = right_path {
        let image = d.assets.load_image(&path);
        let fallback_color = right_color.unwrap_or(Color::new(0.2, 0.2, 0.3, 1.0));
        attach_image_or_placeholder(
            &mut d.scene,
            right_id,
            image,
            Size {
                width: Dimension::percent(1.0),
                height: Dimension::percent(1.0),
            },
            crate::types::ObjectFit::Cover,
            fallback_color,
            None,
        ).map_err(to_eval_err)?;
    }

    Ok(NodeHandle {
        director: scene.director.clone(),
        id: root_id,
            invalidated: false,
    })
}

// ============================================================================
// Reaction Overlay
// ============================================================================

fn build_reaction_overlay(
    scene: &mut SceneHandle,
    props: &rhai::Map,
) -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
    let mut d = scene.lock_director()?;

    let main_path = props
        .get("main")
        .and_then(|v| v.clone().into_string().ok());
    let facecam_path = props
        .get("facecam")
        .and_then(|v| v.clone().into_string().ok());
    let corner = props
        .get("corner")
        .and_then(|v| v.clone().into_string().ok())
        .unwrap_or_else(|| "bottom_right".to_string());
    let facecam_size = props
        .get("facecam_size")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(30.0) as f32 / 100.0;
    let facecam_margin = props
        .get("facecam_margin")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(16.0) as f32;
    let border_radius = props
        .get("border_radius")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(12.0) as f32;
    let border_width = props
        .get("border_width")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(3.0) as f32;
    let border_color = props
        .get("border_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s));

    let mut root = make_box_node(None);
    parse_layout_style(props, &mut root.style);
    root.style.position = taffy::style::Position::Absolute;
    root.style.inset = Rect {
        left: LengthPercentageAuto::length(0.0),
        right: LengthPercentageAuto::length(0.0),
        top: LengthPercentageAuto::length(0.0),
        bottom: LengthPercentageAuto::length(0.0),
    };
    root.style.size = Size {
        width: Dimension::percent(1.0),
        height: Dimension::percent(1.0),
    };
    let root_id = d.scene.add_node(Box::new(root));
    if !d.scene.try_add_child(scene.root_id, root_id) {
        d.scene.destroy_node(root_id);
        return Err("Failed to attach reaction overlay to scene".into());
    }
    set_root_z_index(&mut d.scene, root_id, props);

    if let Some(path) = main_path {
        let image = d.assets.load_image(&path);
        attach_image_or_placeholder(
            &mut d.scene,
            root_id,
            image,
            Size {
                width: Dimension::percent(1.0),
                height: Dimension::percent(1.0),
            },
            crate::types::ObjectFit::Cover,
            Color::new(0.15, 0.15, 0.25, 1.0),
            None,
        ).map_err(to_eval_err)?;
    } else {
        // Fallback: solid background when no main image provided
        let mut placeholder = make_box_node(Some(Color::new(0.15, 0.15, 0.25, 1.0)));
        placeholder.style.size = Size {
            width: Dimension::percent(1.0),
            height: Dimension::percent(1.0),
        };
        let ph_id = d.scene.add_node(Box::new(placeholder));
        attach_child(&mut d.scene, root_id, ph_id).map_err(to_eval_err)?;
    }

    let mut facecam_container = make_box_node(None);
    facecam_container.style.position = taffy::style::Position::Absolute;
        facecam_container.style.size = Size {
            width: Dimension::percent(facecam_size),
            height: Dimension::auto(),
        };
        facecam_container.style.aspect_ratio = Some(16.0 / 9.0);
        facecam_container.border_radius = Animated::new(border_radius);
        facecam_container.border_width = Animated::new(border_width);
        if let Some(c) = border_color {
            facecam_container.border_color = Some(Animated::new(c));
        }
        facecam_container.overflow = "hidden".to_string();

    let margin = LengthPercentageAuto::length(facecam_margin);
    let auto = LengthPercentageAuto::auto();
    match corner.as_str() {
        "top_left" | "top-left" => {
            facecam_container.style.inset = Rect { left: margin, top: margin, right: auto, bottom: auto };
        }
        "top_right" | "top-right" => {
            facecam_container.style.inset = Rect { left: auto, right: margin, top: margin, bottom: auto };
        }
        "bottom_left" | "bottom-left" => {
            facecam_container.style.inset = Rect { left: margin, top: auto, right: auto, bottom: margin };
        }
        _ => {
            facecam_container.style.inset = Rect { left: auto, right: margin, top: auto, bottom: margin };
        }
    }

    let fc_id = d.scene.add_node(Box::new(facecam_container));
    attach_child(&mut d.scene, root_id, fc_id).map_err(to_eval_err)?;

    if let Some(path) = facecam_path {
        let image = d.assets.load_image(&path);
        attach_image_or_placeholder(
            &mut d.scene,
            fc_id,
            image,
            Size {
                width: Dimension::percent(1.0),
                height: Dimension::percent(1.0),
            },
            crate::types::ObjectFit::Cover,
            Color::new(0.3, 0.3, 0.5, 1.0),
            None,
        ).map_err(to_eval_err)?;
    } else {
        // Fallback: colored placeholder when no facecam image
        let mut placeholder = make_box_node(Some(Color::new(0.3, 0.3, 0.5, 1.0)));
        placeholder.style.size = Size {
            width: Dimension::percent(1.0),
            height: Dimension::percent(1.0),
        };
        let ph_id = d.scene.add_node(Box::new(placeholder));
        attach_child(&mut d.scene, fc_id, ph_id).map_err(to_eval_err)?;
    }

    Ok(NodeHandle {
        director: scene.director.clone(),
        id: root_id,
            invalidated: false,
    })
}

// ============================================================================
// Bento Grid
// ============================================================================

fn build_bento_grid(
    scene: &mut SceneHandle,
    props: &rhai::Map,
) -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
    let mut d = scene.lock_director()?;

    let columns = props
        .get("columns")
        .and_then(|v| v.as_int().ok())
        .unwrap_or(2) as usize;
    let rows = props
        .get("rows")
        .and_then(|v| v.as_int().ok())
        .unwrap_or(2) as usize;
    let gap = props
        .get("gap")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(12.0) as f32;
    let padding = props
        .get("padding")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(24.0) as f32;

    let mut root = make_box_node(None);
    parse_layout_style(props, &mut root.style);
    root.style.position = taffy::style::Position::Absolute;
    root.style.inset = Rect {
        left: LengthPercentageAuto::length(0.0),
        right: LengthPercentageAuto::length(0.0),
        top: LengthPercentageAuto::length(0.0),
        bottom: LengthPercentageAuto::length(0.0),
    };
    root.style.size = Size {
        width: Dimension::percent(1.0),
        height: Dimension::percent(1.0),
    };
    root.style.display = Display::Grid;
    root.style.grid_template_columns = (0..columns)
        .map(|_| GridTemplateComponent::Single(fr(1.0)))
        .collect();
    root.style.grid_template_rows = (0..rows)
        .map(|_| GridTemplateComponent::Single(fr(1.0)))
        .collect();
    root.style.gap = Size {
        width: LengthPercentage::length(gap),
        height: LengthPercentage::length(gap),
    };
    root.style.padding = Rect {
        left: LengthPercentage::length(padding),
        right: LengthPercentage::length(padding),
        top: LengthPercentage::length(padding),
        bottom: LengthPercentage::length(padding),
    };

    let root_id = d.scene.add_node(Box::new(root));
    if !d.scene.try_add_child(scene.root_id, root_id) {
        d.scene.destroy_node(root_id);
        return Err("Failed to attach bento grid to scene".into());
    }
    set_root_z_index(&mut d.scene, root_id, props);

    if let Ok(items) = props.get("items").map(|v| v.clone().into_array()).unwrap_or(Ok(Vec::new())) {
        for item in items.iter() {
            let Some(item_map) = item.clone().try_cast::<rhai::Map>() else { continue };

            let item_bg = item_map
                .get("bg_color")
                .and_then(|v| v.clone().into_string().ok())
                .and_then(|s| parse_hex_color(&s));
            let item_text = item_map.get("text").map(|v| v.to_string());
            let item_image = item_map
                .get("image")
                .and_then(|v| v.clone().into_string().ok());

            let mut cell = make_box_node(item_bg);
            cell.border_radius = Animated::new(
                item_map
                    .get("border_radius")
                    .and_then(|v| v.as_float().ok())
                    .unwrap_or(12.0) as f32,
            );
            cell.style.flex_direction = FlexDirection::Column;
            cell.style.align_items = Some(AlignItems::Center);
            cell.style.justify_content = Some(JustifyContent::Center);
            cell.style.padding = Rect {
                left: LengthPercentage::length(12.0),
                right: LengthPercentage::length(12.0),
                top: LengthPercentage::length(12.0),
                bottom: LengthPercentage::length(12.0),
            };

            let cell_id = d.scene.add_node(Box::new(cell));
            attach_child(&mut d.scene, root_id, cell_id).map_err(to_eval_err)?;

            if let Some(path) = item_image {
                let image = d.assets.load_image(&path);
                let fallback_color = item_bg.unwrap_or(Color::new(0.2, 0.2, 0.3, 1.0));
                attach_image_or_placeholder(
                    &mut d.scene,
                    cell_id,
                    image,
                    Size {
                        width: Dimension::percent(1.0),
                        height: Dimension::percent(1.0),
                    },
                    crate::types::ObjectFit::Cover,
                    fallback_color,
                    None,
                ).map_err(to_eval_err)?;
            }

            if let Some(text) = item_text {
                let text_color = item_map
                    .get("text_color")
                    .and_then(|v| v.clone().into_string().ok())
                    .and_then(|s| parse_hex_color(&s));
                let text_size = item_map
                    .get("text_size")
                    .and_then(|v| v.as_float().ok())
                    .unwrap_or(24.0) as f32;
                let mut text_node = make_text_node(&mut d, &text, text_color, text_size, Some(600), None);
                text_node.fit_mode = TextFit::Shrink;
                text_node.style.size = Size {
                    width: Dimension::percent(1.0),
                    height: Dimension::auto(),
                };
                let text_id = d.scene.add_node(Box::new(text_node));
                attach_child(&mut d.scene, cell_id, text_id).map_err(to_eval_err)?;
            }
        }
    }

    Ok(NodeHandle {
        director: scene.director.clone(),
        id: root_id,
            invalidated: false,
    })
}

// ============================================================================
// Subscribe Button
// ============================================================================

fn build_subscribe_button(
    scene: &mut SceneHandle,
    props: &rhai::Map,
) -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
    let mut d = scene.lock_director()?;

    let channel_name = props
        .get("channel_name")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Channel".to_string());
    let sub_count = props
        .get("sub_count")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "0".to_string());
    let button_text = props
        .get("button_text")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "SUBSCRIBE".to_string());
    let bg_color = props
        .get("bg_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s));
    let button_color = props
        .get("button_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s))
        .unwrap_or(Color::new(1.0, 0.2, 0.2, 1.0));
    let text_color = props
        .get("text_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s))
        .unwrap_or(Color::new(1.0, 1.0, 1.0, 1.0));
    let position = props
        .get("position")
        .and_then(|v| v.clone().into_string().ok())
        .unwrap_or_else(|| "bottom_center".to_string());
    let border_radius = props
        .get("border_radius")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(32.0) as f32;
    let font_family = props.get("font_family").and_then(|v| v.clone().into_string().ok());

    let mut root = make_box_node(bg_color);
    parse_layout_style(props, &mut root.style);
    root.style.position = taffy::style::Position::Absolute;
    root.style.flex_direction = FlexDirection::Row;
    root.style.align_items = Some(AlignItems::Center);
    root.style.padding = Rect {
        left: LengthPercentage::length(16.0),
        right: LengthPercentage::length(16.0),
        top: LengthPercentage::length(8.0),
        bottom: LengthPercentage::length(8.0),
    };
    root.border_radius = Animated::new(border_radius);
    apply_box_shadow(&mut root, props);

    let margin = LengthPercentageAuto::length(100.0);
    let auto = LengthPercentageAuto::auto();
    match position.as_str() {
        "bottom_left" | "bottom-left" => {
            root.style.inset = Rect { left: margin, right: auto, top: auto, bottom: margin };
        }
        "top_left" | "top-left" => {
            root.style.inset = Rect { left: margin, right: auto, top: margin, bottom: auto };
        }
        "top_right" | "top-right" => {
            root.style.inset = Rect { left: auto, right: margin, top: margin, bottom: auto };
        }
        "top_center" | "top-center" => {
            root.style.inset = Rect { left: auto, right: auto, top: margin, bottom: auto };
        }
        "bottom_center" | "bottom-center" => {
            root.style.inset = Rect { left: auto, right: auto, top: auto, bottom: margin };
        }
        _ => {
            root.style.inset = Rect { left: auto, right: margin, top: auto, bottom: margin };
        }
    }

    let root_id = d.scene.add_node(Box::new(root));
    if !d.scene.try_add_child(scene.root_id, root_id) {
        d.scene.destroy_node(root_id);
        return Err("Failed to attach subscribe button to scene".into());
    }
    set_root_z_index(&mut d.scene, root_id, props);

    // Channel name + sub count
    let info_text = format!("{} · {} subscribers", channel_name, sub_count);
    let mut info_node = make_text_node(&mut d, &info_text, Some(text_color), 18.0, Some(500), font_family.clone());
    info_node.style.flex_grow = 1.0;
    info_node.style.size = Size {
        width: Dimension::auto(),
        height: Dimension::auto(),
    };
    let info_id = d.scene.add_node(Box::new(info_node));
    attach_child(&mut d.scene, root_id, info_id).map_err(to_eval_err)?;

    // Button pill
    let mut button = make_box_node(Some(button_color));
    button.style.size = Size {
        width: Dimension::auto(),
        height: Dimension::auto(),
    };
    button.style.flex_direction = FlexDirection::Row;
    button.style.align_items = Some(AlignItems::Center);
    button.style.justify_content = Some(JustifyContent::Center);
    button.style.flex_shrink = 0.0;
    button.style.padding = Rect {
        left: LengthPercentage::length(20.0),
        right: LengthPercentage::length(20.0),
        top: LengthPercentage::length(8.0),
        bottom: LengthPercentage::length(8.0),
    };
    button.border_radius = Animated::new(border_radius * 0.8);
    let btn_id = d.scene.add_node(Box::new(button));
    attach_child(&mut d.scene, root_id, btn_id).map_err(to_eval_err)?;

    let mut btn_text = make_text_node(&mut d, &button_text, Some(text_color), 16.0, Some(700), font_family);
    btn_text.style.size = Size {
        width: Dimension::auto(),
        height: Dimension::auto(),
    };
    let btn_text_id = d.scene.add_node(Box::new(btn_text));
    attach_child(&mut d.scene, btn_id, btn_text_id).map_err(to_eval_err)?;

    Ok(NodeHandle {
        director: scene.director.clone(),
        id: root_id,
            invalidated: false,
    })
}

// ============================================================================
// Progress Bar
// ============================================================================

fn build_progress_bar(
    scene: &mut SceneHandle,
    props: &rhai::Map,
) -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
    let mut d = scene.lock_director()?;

    let progress = props
        .get("progress")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(0.0) as f32
        / 100.0;
    let progress = progress.clamp(0.0, 1.0);
    let label = props.get("label").map(|v| v.to_string());
    let bar_color = props
        .get("bar_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s))
        .unwrap_or(Color::new(0.2, 0.8, 0.4, 1.0));
    let bg_color = props
        .get("bg_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s))
        .unwrap_or(Color::new(0.15, 0.15, 0.2, 1.0));
    let text_color = props
        .get("text_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s))
        .unwrap_or(Color::new(1.0, 1.0, 1.0, 1.0));
    let height = props
        .get("height")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(24.0) as f32;
    let border_radius = props
        .get("border_radius")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(height as f64 / 2.0) as f32;
    let width_pct = props
        .get("width")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(80.0) as f32;
    let font_family = props.get("font_family").and_then(|v| v.clone().into_string().ok());

    let mut root = make_box_node(None);
    parse_layout_style(props, &mut root.style);
    root.style.position = taffy::style::Position::Absolute;
    root.style.inset = Rect {
        left: LengthPercentageAuto::length(0.0),
        right: LengthPercentageAuto::length(0.0),
        top: LengthPercentageAuto::length(0.0),
        bottom: LengthPercentageAuto::length(0.0),
    };
    root.style.size = Size {
        width: Dimension::percent(1.0),
        height: Dimension::percent(1.0),
    };
    root.style.flex_direction = FlexDirection::Column;
    root.style.align_items = Some(AlignItems::Center);
    root.style.justify_content = Some(JustifyContent::Center);
    root.style.gap = Size {
        width: LengthPercentage::length(0.0),
        height: LengthPercentage::length(6.0),
    };

    let root_id = d.scene.add_node(Box::new(root));
    if !d.scene.try_add_child(scene.root_id, root_id) {
        d.scene.destroy_node(root_id);
        return Err("Failed to attach progress bar to scene".into());
    }
    set_root_z_index(&mut d.scene, root_id, props);

    // Label row
    if let Some(lbl) = label {
        let label_text = format!("{}  {:.0}%", lbl, progress * 100.0);
        let mut label_node = make_text_node(&mut d, &label_text, Some(text_color), 16.0, Some(600), font_family.clone());
        label_node.style.size = Size {
            width: Dimension::auto(),
            height: Dimension::auto(),
        };
        let label_id = d.scene.add_node(Box::new(label_node));
        attach_child(&mut d.scene, root_id, label_id).map_err(to_eval_err)?;
    }

    // Bar track
    let mut track = make_box_node(Some(bg_color));
    track.style.size = Size {
        width: Dimension::percent(width_pct / 100.0),
        height: Dimension::length(height),
    };
    track.border_radius = Animated::new(border_radius);
    track.overflow = "hidden".to_string();
    let track_id = d.scene.add_node(Box::new(track));
    attach_child(&mut d.scene, root_id, track_id).map_err(to_eval_err)?;

    // Fill
    let mut fill = make_box_node(Some(bar_color));
    fill.style.size = Size {
        width: Dimension::percent(progress),
        height: Dimension::percent(1.0),
    };
    let fill_id = d.scene.add_node(Box::new(fill));
    attach_child(&mut d.scene, track_id, fill_id).map_err(to_eval_err)?;

    Ok(NodeHandle {
        director: scene.director.clone(),
        id: root_id,
            invalidated: false,
    })
}

// ============================================================================
// Countdown Timer
// ============================================================================

fn build_countdown_timer(
    scene: &mut SceneHandle,
    props: &rhai::Map,
) -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
    let mut d = scene.lock_director()?;

    let days = props.get("days").and_then(|v| v.as_int().ok()).unwrap_or(0);
    let hours = props.get("hours").and_then(|v| v.as_int().ok()).unwrap_or(0);
    let minutes = props.get("minutes").and_then(|v| v.as_int().ok()).unwrap_or(0);
    let seconds = props.get("seconds").and_then(|v| v.as_int().ok()).unwrap_or(0);
    let header = props.get("header").map(|v| v.to_string());
    let segment_bg = props
        .get("segment_bg")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s))
        .unwrap_or(Color::new(0.12, 0.12, 0.18, 1.0));
    let text_color = props
        .get("text_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s))
        .unwrap_or(Color::new(1.0, 1.0, 1.0, 1.0));
    let label_color = props
        .get("label_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s))
        .unwrap_or(Color::new(0.6, 0.6, 0.7, 1.0));
    let gap = props
        .get("gap")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(12.0) as f32;
    let font_family = props.get("font_family").and_then(|v| v.clone().into_string().ok());

    let mut root = make_box_node(None);
    parse_layout_style(props, &mut root.style);
    root.style.position = taffy::style::Position::Absolute;
    root.style.inset = Rect {
        left: LengthPercentageAuto::length(0.0),
        right: LengthPercentageAuto::length(0.0),
        top: LengthPercentageAuto::length(0.0),
        bottom: LengthPercentageAuto::length(0.0),
    };
    root.style.size = Size {
        width: Dimension::percent(1.0),
        height: Dimension::percent(1.0),
    };
    root.style.flex_direction = FlexDirection::Column;
    root.style.align_items = Some(AlignItems::Center);
    root.style.justify_content = Some(JustifyContent::Center);
    root.style.gap = Size {
        width: LengthPercentage::length(0.0),
        height: LengthPercentage::length(16.0),
    };

    let root_id = d.scene.add_node(Box::new(root));
    if !d.scene.try_add_child(scene.root_id, root_id) {
        d.scene.destroy_node(root_id);
        return Err("Failed to attach countdown timer to scene".into());
    }
    set_root_z_index(&mut d.scene, root_id, props);

    if let Some(h) = header {
        let mut header_node = make_text_node(&mut d, &h, Some(text_color), 24.0, Some(600), font_family.clone());
        header_node.style.size = Size {
            width: Dimension::auto(),
            height: Dimension::auto(),
        };
        let header_id = d.scene.add_node(Box::new(header_node));
        attach_child(&mut d.scene, root_id, header_id).map_err(to_eval_err)?;
    }

    // Segments container
    let mut segments = make_box_node(None);
    segments.style.flex_direction = FlexDirection::Row;
    segments.style.align_items = Some(AlignItems::Center);
    segments.style.justify_content = Some(JustifyContent::Center);
    segments.style.gap = Size {
        width: LengthPercentage::length(gap),
        height: LengthPercentage::length(0.0),
    };
    let seg_container_id = d.scene.add_node(Box::new(segments));
    attach_child(&mut d.scene, root_id, seg_container_id).map_err(to_eval_err)?;

    let segments_data = [
        (days, "DAYS"),
        (hours, "HOURS"),
        (minutes, "MINUTES"),
        (seconds, "SECONDS"),
    ];

    for (value, label) in segments_data.iter() {
        let mut seg = make_box_node(Some(segment_bg));
        seg.style.flex_direction = FlexDirection::Column;
        seg.style.align_items = Some(AlignItems::Center);
        seg.style.justify_content = Some(JustifyContent::Center);
        seg.style.size = Size {
            width: Dimension::length(80.0),
            height: Dimension::length(80.0),
        };
        seg.border_radius = Animated::new(12.0);
        seg.style.padding = Rect {
            left: LengthPercentage::length(8.0),
            right: LengthPercentage::length(8.0),
            top: LengthPercentage::length(8.0),
            bottom: LengthPercentage::length(8.0),
        };
        let seg_id = d.scene.add_node(Box::new(seg));
        attach_child(&mut d.scene, seg_container_id, seg_id).map_err(to_eval_err)?;

        let value_text = format!("{:02}", value);
        let mut num_node = make_text_node(&mut d, &value_text, Some(text_color), 32.0, Some(700), font_family.clone());
        num_node.style.size = Size {
            width: Dimension::auto(),
            height: Dimension::auto(),
        };
        let num_id = d.scene.add_node(Box::new(num_node));
        attach_child(&mut d.scene, seg_id, num_id).map_err(to_eval_err)?;

        let mut lbl_node = make_text_node(&mut d, label, Some(label_color), 14.0, Some(500), font_family.clone());
        lbl_node.style.size = Size {
            width: Dimension::auto(),
            height: Dimension::auto(),
        };
        let lbl_id = d.scene.add_node(Box::new(lbl_node));
        attach_child(&mut d.scene, seg_id, lbl_id).map_err(to_eval_err)?;

        // Separator colon (except for last segment)
        if label != &"SECONDS" {
            let mut colon = make_text_node(&mut d, ":", Some(text_color), 28.0, Some(700), font_family.clone());
            colon.style.size = Size {
                width: Dimension::auto(),
                height: Dimension::auto(),
            };
            let colon_id = d.scene.add_node(Box::new(colon));
            attach_child(&mut d.scene, seg_container_id, colon_id).map_err(to_eval_err)?;
        }
    }

    Ok(NodeHandle {
        director: scene.director.clone(),
        id: root_id,
            invalidated: false,
    })
}

// ============================================================================
// Quote Card
// ============================================================================

fn build_quote_card(
    scene: &mut SceneHandle,
    props: &rhai::Map,
) -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
    let mut d = scene.lock_director()?;

    let quote = props
        .get("quote")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Quote text here.".to_string());
    let author = props
        .get("author")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Author".to_string());
    let source = props.get("source").map(|v| v.to_string());
    let bg_color = props
        .get("bg_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s))
        .unwrap_or(Color::new(0.1, 0.1, 0.15, 1.0));
    let text_color = props
        .get("text_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s))
        .unwrap_or(Color::new(1.0, 1.0, 1.0, 1.0));
    let label_color = props
        .get("label_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s))
        .unwrap_or(Color::new(0.6, 0.6, 0.7, 1.0));
    let accent_color = props
        .get("accent_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s))
        .unwrap_or(Color::new(0.55, 0.36, 0.96, 1.0));
    let border_radius = props
        .get("border_radius")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(20.0) as f32;
    let font_family = props.get("font_family").and_then(|v| v.clone().into_string().ok());

    let mut root = make_box_node(Some(bg_color));
    parse_layout_style(props, &mut root.style);
    root.style.position = taffy::style::Position::Absolute;
    root.style.inset = Rect {
        left: LengthPercentageAuto::length(0.0),
        right: LengthPercentageAuto::length(0.0),
        top: LengthPercentageAuto::length(0.0),
        bottom: LengthPercentageAuto::length(0.0),
    };
    root.style.size = Size {
        width: Dimension::percent(1.0),
        height: Dimension::percent(1.0),
    };
    root.style.flex_direction = FlexDirection::Column;
    root.style.align_items = Some(AlignItems::Center);
    root.style.justify_content = Some(JustifyContent::Center);
    root.style.padding = Rect {
        left: LengthPercentage::length(40.0),
        right: LengthPercentage::length(40.0),
        top: LengthPercentage::length(40.0),
        bottom: LengthPercentage::length(40.0),
    };
    root.border_radius = Animated::new(border_radius);
    apply_box_shadow(&mut root, props);

    let root_id = d.scene.add_node(Box::new(root));
    if !d.scene.try_add_child(scene.root_id, root_id) {
        d.scene.destroy_node(root_id);
        return Err("Failed to attach quote card to scene".into());
    }
    set_root_z_index(&mut d.scene, root_id, props);

    // Decorative accent bar
    let mut accent = make_box_node(Some(accent_color));
    accent.style.size = Size {
        width: Dimension::length(48.0),
        height: Dimension::length(4.0),
    };
    accent.style.margin = Rect {
        left: LengthPercentageAuto::length(0.0),
        right: LengthPercentageAuto::length(0.0),
        top: LengthPercentageAuto::length(0.0),
        bottom: LengthPercentageAuto::length(20.0),
    };
    let accent_id = d.scene.add_node(Box::new(accent));
    attach_child(&mut d.scene, root_id, accent_id).map_err(to_eval_err)?;

    // Opening quote mark
    let mut quote_mark = make_text_node(&mut d, "\"", Some(accent_color), 64.0, Some(700), font_family.clone());
    quote_mark.style.size = Size {
        width: Dimension::auto(),
        height: Dimension::auto(),
    };
    quote_mark.style.margin = Rect {
        left: LengthPercentageAuto::length(0.0),
        right: LengthPercentageAuto::length(0.0),
        top: LengthPercentageAuto::length(0.0),
        bottom: LengthPercentageAuto::length(8.0),
    };
    let mark_id = d.scene.add_node(Box::new(quote_mark));
    attach_child(&mut d.scene, root_id, mark_id).map_err(to_eval_err)?;

    // Quote text
    let mut quote_node = make_text_node(&mut d, &quote, Some(text_color), 28.0, Some(500), font_family.clone());
    quote_node.set_text_align(SkTextAlign::Center);
    quote_node.style.size = Size {
        width: Dimension::percent(1.0),
        height: Dimension::auto(),
    };
    quote_node.style.margin = Rect {
        left: LengthPercentageAuto::length(0.0),
        right: LengthPercentageAuto::length(0.0),
        top: LengthPercentageAuto::length(0.0),
        bottom: LengthPercentageAuto::length(20.0),
    };
    let quote_id = d.scene.add_node(Box::new(quote_node));
    attach_child(&mut d.scene, root_id, quote_id).map_err(to_eval_err)?;

    // Author line
    let author_text = if let Some(src) = source {
        format!("— {}, {}", author, src)
    } else {
        format!("— {}", author)
    };
    let mut author_node = make_text_node(&mut d, &author_text, Some(label_color), 16.0, Some(600), font_family);
    author_node.set_text_align(SkTextAlign::Center);
    author_node.style.size = Size {
        width: Dimension::auto(),
        height: Dimension::auto(),
    };
    let author_id = d.scene.add_node(Box::new(author_node));
    attach_child(&mut d.scene, root_id, author_id).map_err(to_eval_err)?;

    Ok(NodeHandle {
        director: scene.director.clone(),
        id: root_id,
            invalidated: false,
    })
}

// ============================================================================
// Social Handle Overlay
// ============================================================================

fn build_social_handle_overlay(
    scene: &mut SceneHandle,
    props: &rhai::Map,
) -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
    let mut d = scene.lock_director()?;

    let position = props
        .get("position")
        .and_then(|v| v.clone().into_string().ok())
        .unwrap_or_else(|| "bottom_left".to_string());
    let bg_color = props
        .get("bg_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s));
    let text_color = props
        .get("text_color")
        .and_then(|v| v.clone().into_string().ok())
        .and_then(|s| parse_hex_color(&s))
        .unwrap_or(Color::new(1.0, 1.0, 1.0, 1.0));
    let icon_size = props
        .get("icon_size")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(20.0) as f32;
    let gap = props
        .get("gap")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(12.0) as f32;
    let margin = props
        .get("margin")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(100.0) as f32;
    let font_family = props.get("font_family").and_then(|v| v.clone().into_string().ok());

    let mut root = make_box_node(bg_color);
    parse_layout_style(props, &mut root.style);
    root.style.position = taffy::style::Position::Absolute;
    root.style.flex_direction = FlexDirection::Column;
    root.style.align_items = Some(AlignItems::FlexStart);
    root.style.gap = Size {
        width: LengthPercentage::length(0.0),
        height: LengthPercentage::length(gap),
    };
    root.style.padding = Rect {
        left: LengthPercentage::length(20.0),
        right: LengthPercentage::length(20.0),
        top: LengthPercentage::length(16.0),
        bottom: LengthPercentage::length(16.0),
    };
    root.border_radius = Animated::new(12.0);
    apply_box_shadow(&mut root, props);

    let m = LengthPercentageAuto::length(margin);
    let auto = LengthPercentageAuto::auto();
    match position.as_str() {
        "top_left" | "top-left" => {
            root.style.inset = Rect { left: m, top: m, right: auto, bottom: auto };
        }
        "top_right" | "top-right" => {
            root.style.inset = Rect { left: auto, right: m, top: m, bottom: auto };
        }
        "bottom_left" | "bottom-left" => {
            root.style.inset = Rect { left: m, top: auto, right: auto, bottom: m };
        }
        "bottom_right" | "bottom-right" => {
            root.style.inset = Rect { left: auto, right: m, top: auto, bottom: m };
        }
        "bottom_center" | "bottom-center" => {
            root.style.inset = Rect { left: auto, right: auto, top: auto, bottom: m };
        }
        "top_center" | "top-center" => {
            root.style.inset = Rect { left: auto, right: auto, top: m, bottom: auto };
        }
        _ => {
            root.style.inset = Rect { left: auto, right: m, top: auto, bottom: m };
        }
    }

    let root_id = d.scene.add_node(Box::new(root));
    if !d.scene.try_add_child(scene.root_id, root_id) {
        d.scene.destroy_node(root_id);
        return Err("Failed to attach social handle overlay to scene".into());
    }
    set_root_z_index(&mut d.scene, root_id, props);

    // Parse platforms array
    if let Ok(platforms) = props.get("platforms").map(|v| v.clone().into_array()).unwrap_or(Ok(Vec::new())) {
        for plat in platforms.iter() {
            let Some(plat_map) = plat.clone().try_cast::<rhai::Map>() else { continue };

            let platform_name = plat_map
                .get("platform")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "Social".to_string());
            let handle = plat_map
                .get("handle")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "@handle".to_string());
            let plat_color = plat_map
                .get("color")
                .and_then(|v| v.clone().into_string().ok())
                .and_then(|s| parse_hex_color(&s))
                .unwrap_or(Color::new(0.8, 0.8, 0.8, 1.0));

            let mut row = make_box_node(None);
            row.style.flex_direction = FlexDirection::Row;
            row.style.align_items = Some(AlignItems::Center);
            row.style.gap = Size {
                width: LengthPercentage::length(10.0),
                height: LengthPercentage::length(0.0),
            };
            let row_id = d.scene.add_node(Box::new(row));
            attach_child(&mut d.scene, root_id, row_id).map_err(to_eval_err)?;

            // Icon placeholder (colored circle with first letter)
            let mut icon = make_box_node(Some(plat_color));
            icon.style.size = Size {
                width: Dimension::length(icon_size),
                height: Dimension::length(icon_size),
            };
            icon.style.justify_content = Some(JustifyContent::Center);
            icon.style.align_items = Some(AlignItems::Center);
            icon.border_radius = Animated::new(icon_size / 2.0);
            let icon_id = d.scene.add_node(Box::new(icon));
            attach_child(&mut d.scene, row_id, icon_id).map_err(to_eval_err)?;

            let first_letter = platform_name.chars().next().unwrap_or('S').to_string();
            let mut icon_text = make_text_node(&mut d, &first_letter, Some(Color::new(1.0, 1.0, 1.0, 1.0)), icon_size * 0.5, Some(700), font_family.clone());
            icon_text.style.size = Size {
                width: Dimension::auto(),
                height: Dimension::auto(),
            };
            let icon_text_id = d.scene.add_node(Box::new(icon_text));
            attach_child(&mut d.scene, icon_id, icon_text_id).map_err(to_eval_err)?;

            // Handle text
            let mut handle_node = make_text_node(&mut d, &handle, Some(text_color), 18.0, Some(600), font_family.clone());
            handle_node.style.size = Size {
                width: Dimension::auto(),
                height: Dimension::auto(),
            };
            let handle_id = d.scene.add_node(Box::new(handle_node));
            attach_child(&mut d.scene, row_id, handle_id).map_err(to_eval_err)?;
        }
    }

    Ok(NodeHandle {
        director: scene.director.clone(),
        id: root_id,
            invalidated: false,
    })
}

// ============================================================================
// Watermark
// ============================================================================

fn build_watermark(
    scene: &mut SceneHandle,
    props: &rhai::Map,
) -> Result<NodeHandle, Box<rhai::EvalAltResult>> {
    let mut d = scene.lock_director()?;

    let text = props.get("text").map(|v| v.to_string());
    let image_path = props
        .get("image")
        .and_then(|v| v.clone().into_string().ok());
    let corner = props
        .get("corner")
        .and_then(|v| v.clone().into_string().ok())
        .unwrap_or_else(|| "bottom_right".to_string());
    let margin = props
        .get("margin")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(100.0) as f32;
    let opacity = props
        .get("opacity")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(0.6) as f32;

    let mut root = make_box_node(None);
    parse_layout_style(props, &mut root.style);
    root.style.position = taffy::style::Position::Absolute;

    let m = LengthPercentageAuto::length(margin);
    let auto = LengthPercentageAuto::auto();
    match corner.as_str() {
        "top_left" | "top-left" => {
            root.style.inset = Rect { left: m, top: m, right: auto, bottom: auto };
        }
        "top_right" | "top-right" => {
            root.style.inset = Rect { left: auto, right: m, top: m, bottom: auto };
        }
        "bottom_left" | "bottom-left" => {
            root.style.inset = Rect { left: m, top: auto, right: auto, bottom: m };
        }
        "bottom_right" | "bottom-right" => {
            root.style.inset = Rect { left: auto, right: m, top: auto, bottom: m };
        }
        _ => {
            root.style.inset = Rect { left: auto, right: m, top: auto, bottom: m };
        }
    }

    let root_id = d.scene.add_node(Box::new(root));
    if !d.scene.try_add_child(scene.root_id, root_id) {
        d.scene.destroy_node(root_id);
        return Err("Failed to attach watermark to scene".into());
    }
    set_root_z_index(&mut d.scene, root_id, props);

    if let Some(path) = image_path {
        let image = d.assets.load_image(&path);
        let size = props
            .get("size")
            .and_then(|v| v.as_float().ok())
            .unwrap_or(48.0) as f32;
        attach_image_or_placeholder(
            &mut d.scene,
            root_id,
            image,
            Size {
                width: Dimension::length(size),
                height: Dimension::length(size),
            },
            crate::types::ObjectFit::Contain,
            Color::new(0.3, 0.3, 0.5, 1.0),
            Some(opacity),
        ).map_err(to_eval_err)?;
    } else if let Some(text_content) = text {
        let mut text_color = props
            .get("color")
            .and_then(|v| v.clone().into_string().ok())
            .and_then(|s| parse_hex_color(&s))
            .unwrap_or(Color::new(1.0, 1.0, 1.0, 1.0));
        text_color.a *= opacity;
    let text_size = props
        .get("text_size")
        .and_then(|v| v.as_float().ok())
        .unwrap_or(18.0) as f32;
        let mut text_node = make_text_node(&mut d, &text_content, Some(text_color), text_size, Some(700), None);
        text_node.style.size = Size {
            width: Dimension::auto(),
            height: Dimension::auto(),
        };
        let text_id = d.scene.add_node(Box::new(text_node));
        attach_child(&mut d.scene, root_id, text_id).map_err(to_eval_err)?;
    }

    Ok(NodeHandle {
        director: scene.director.clone(),
        id: root_id,
            invalidated: false,
    })
}

