//! Typography Tests
//!
//! Tests for text rendering features including TextFit, rich spans, and shadows.

use director_core::{
    element::{TextFit, TextSpan},
    node::TextNode,
    text_direction::TextDirectionMode,
    video_wrapper::RenderMode,
    DefaultAssetLoader, Director,
};
use skia_safe::textlayout::TextDirection;
use std::sync::Arc;

/// Test TextFit::Shrink behavior.
///
/// Validates that text automatically shrinks when constrained to a small box.
#[test]
fn text_fit_shrink() {
    let loader = Arc::new(DefaultAssetLoader);
    let mut director = Director::new(1920, 1080, 30.0, loader, RenderMode::Preview, None);

    let spans = vec![TextSpan {
        text: "This is a very long text that should shrink to fit.".to_string(),
        font_size: Some(100.0),
        ..Default::default()
    }];

    let font_collection = director.assets.font_collection.clone();
    let mut text_node = TextNode::new(spans, font_collection);
    text_node.fit_mode = TextFit::Shrink;
    text_node.min_size = 10.0;
    text_node.max_size = 100.0;

    // Constrain to a small box
    text_node.style.size = taffy::geometry::Size {
        width: taffy::style::Dimension::length(200.0),
        height: taffy::style::Dimension::length(50.0),
    };

    let id = director.scene.add_node(Box::new(text_node));

    director
        .timeline
        .push(director_core::director::TimelineItem {
            scene_root: id,
            name: None,
            start_time: 0.0,
            duration: 1.0,
            z_index: 0,
            audio_tracks: vec![],
            freeze_time: None,
        });

    // Trigger layout
    let mut surface = skia_safe::surfaces::raster_n32_premul((1920, 1080)).unwrap();
    director_core::systems::renderer::render_frame(&mut director, 0.0, surface.canvas()).unwrap();

    // Verify shrinkage
    let node = director.scene.get_node(id).unwrap();
    let text_node = node.element.as_any().downcast_ref::<TextNode>().unwrap();

    assert!(
        text_node.default_font_size.current_value < 100.0,
        "Font should have shrunk from 100.0, got {}",
        text_node.default_font_size.current_value
    );
    assert!(
        text_node.default_font_size.current_value >= 10.0,
        "Font should not go below min_size 10.0, got {}",
        text_node.default_font_size.current_value
    );
}

/// Test rich text span styling (background, stroke).
///
/// Validates that TextSpan properties are correctly applied.
#[test]
fn text_span_rich_styling() {
    let loader = Arc::new(DefaultAssetLoader);
    let mut director = Director::new(800, 600, 30.0, loader, RenderMode::Preview, None);

    let spans = vec![TextSpan {
        text: "Styled Text".to_string(),
        color: Some(director_core::types::Color::WHITE),
        font_weight: Some(700),
        font_size: Some(48.0),
        background_color: Some(director_core::types::Color::new(0.0, 0.0, 1.0, 1.0)),
        background_padding: Some(10.0),
        stroke_width: Some(2.0),
        stroke_color: Some(director_core::types::Color::BLACK),
        ..Default::default()
    }];

    let font_collection = director.assets.font_collection.clone();
    let text_node = TextNode::new(spans, font_collection);

    let id = director.scene.add_node(Box::new(text_node));

    director
        .timeline
        .push(director_core::director::TimelineItem {
            scene_root: id,
            name: None,
            start_time: 0.0,
            duration: 1.0,
            z_index: 0,
            audio_tracks: vec![],
            freeze_time: None,
        });

    // Should not panic during render
    let mut surface = skia_safe::surfaces::raster_n32_premul((800, 600)).unwrap();
    director_core::systems::renderer::render_frame(&mut director, 0.0, surface.canvas()).unwrap();
}

/// Explicit LTR keeps English glyphs in left-to-right visual order.
#[test]
fn text_explicit_ltr_english() {
    let loader = Arc::new(DefaultAssetLoader);
    let director = Director::new(800, 600, 30.0, loader, RenderMode::Preview, None);

    let spans = vec![TextSpan {
        text: "ABC".to_string(),
        font_size: Some(48.0),
        ..Default::default()
    }];

    let font_collection = director.assets.font_collection.clone();
    let mut text_node = TextNode::new(spans, font_collection);
    text_node.set_text_direction(TextDirectionMode::Ltr);

    let glyphs = text_node.get_glyph_rects(400.0);
    assert_eq!(glyphs.len(), 3);
    assert!(
        glyphs[0].rect.left() < glyphs[2].rect.left(),
        "LTR English: first glyph ({}) should be left of last ({})",
        glyphs[0].rect.left(),
        glyphs[2].rect.left()
    );
}

/// Auto-detect sets RTL for Arabic content.
#[test]
fn text_auto_detects_arabic_rtl() {
    let loader = Arc::new(DefaultAssetLoader);
    let director = Director::new(800, 600, 30.0, loader, RenderMode::Preview, None);

    let spans = vec![TextSpan {
        text: "مرحبا".to_string(),
        font_size: Some(48.0),
        ..Default::default()
    }];

    let font_collection = director.assets.font_collection.clone();
    let text_node = TextNode::new(spans, font_collection);
    let glyphs = text_node.get_glyph_rects(400.0);
    assert!(!glyphs.is_empty());
    assert_eq!(glyphs[0].direction, TextDirection::RTL);
}

/// Test text shadow rendering.
///
/// Validates that TextShadow is applied without panicking.
#[test]
fn text_shadow() {
    let loader = Arc::new(DefaultAssetLoader);
    let mut director = Director::new(800, 600, 30.0, loader, RenderMode::Preview, None);

    let spans = vec![TextSpan {
        text: "Shadow Test".to_string(),
        font_size: Some(48.0),
        color: Some(director_core::types::Color::WHITE),
        ..Default::default()
    }];

    let font_collection = director.assets.font_collection.clone();
    let mut text_node = TextNode::new(spans, font_collection);

    text_node.shadow = Some(director_core::element::TextShadow {
        color: director_core::types::Color::new(0.0, 0.0, 0.0, 0.5),
        blur: 10.0,
        offset: (5.0, 5.0),
    });

    let id = director.scene.add_node(Box::new(text_node));

    director
        .timeline
        .push(director_core::director::TimelineItem {
            scene_root: id,
            name: None,
            start_time: 0.0,
            duration: 1.0,
            z_index: 0,
            audio_tracks: vec![],
            freeze_time: None,
        });

    // Should not panic during render
    let mut surface = skia_safe::surfaces::raster_n32_premul((800, 600)).unwrap();
    director_core::systems::renderer::render_frame(&mut director, 0.0, surface.canvas()).unwrap();
}

/// Audit #10: `TextFit::Shrink` must NOT clobber a keyframe-pumped font size.
/// Before this landed, `post_layout` would write the post-fit size back into
/// `default_font_size.current_value`, fighting any concurrent
/// `animate_property("size")` and producing visible jitter. The fix stores
/// post-fit shrinkage in `resolved_font_size` and only re-clamps to bounds.
#[test]
fn text_fit_shrink_respects_keyframe_upper_bound() {
    use director_core::animation::Animated;
    use director_core::element::{TextFit, TextSpan};
    use director_core::node::text::TextNode;

    let mut director = director_core::Director::new(
        500,
        400,
        30.0,
        Arc::new(DefaultAssetLoader),
        director_core::video_wrapper::RenderMode::Preview,
        None,
    );
    let fc = director.assets.font_collection.clone();
    // An explicit default_font_size high enough that Shrink should always
    // be willing to shrink further — proves the upper bound is unaffected
    // by the binary search.
    let mut t = TextNode::new(
        vec![TextSpan {
            text: "Hello world".to_string(),
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
            line_height: None,
        }],
        fc,
    );
    t.default_font_size = Animated::new(120.0); // huge upper bound
    t.min_size = 18.0;
    t.max_size = 120.0;
    t.fit_mode = TextFit::Shrink;
    let id = director.scene.add_node(Box::new(t));
    director.timeline.push(director_core::director::TimelineItem { scene_root: id, name: None, start_time: 0.0, duration: 1.0, z_index: 0, audio_tracks: vec![], freeze_time: None });

    // Drive a frame so post_layout runs.
    director.update(0.5);
    director_core::systems::renderer::render_frame(
        &mut director,
        0.5,
        skia_safe::surfaces::raster_n32_premul((500, 400))
            .unwrap()
            .canvas(),
    )
    .unwrap();
    let text = director
        .scene
        .get_node(id)
        .unwrap()
        .element
        .as_any()
        .downcast_ref::<TextNode>()
        .unwrap();
    let animated = text.default_font_size.current_value;
    let resolved = text.resolved_font_size;
    assert!(
        (animated - 120.0).abs() < 0.001,
        "keyframe value must not be clobbered by Shrink; animated={animated}"
    );
    assert!(
        resolved <= animated,
        "resolved_font_size ({resolved}) must never exceed animated upper bound ({animated})"
    );
    assert!(
        resolved >= 18.0 - 0.001,
        "resolved must respect min_size floor (>=18); resolved={resolved}"
    );
}

/// `min_size` floor is honored whether `min_size` is driven by the user or
/// by a small animated value (audit #10: shrink search clamps to bounds).
#[test]
fn text_fit_shrink_clamps_to_user_min_size() {
    use director_core::animation::Animated;
    use director_core::element::{TextFit, TextSpan};
    use director_core::node::text::TextNode;

    let mut director = director_core::Director::new(
        200,
        100,
        30.0,
        Arc::new(DefaultAssetLoader),
        director_core::video_wrapper::RenderMode::Preview,
        None,
    );
    let fc = director.assets.font_collection.clone();
    let mut t = TextNode::new(
        vec![TextSpan {
            text: "x".to_string(),
            ..Default::default()
        }],
        fc,
    );
    t.default_font_size = Animated::new(8.0); // tiny upper bound, below min
    t.min_size = 24.0;
    t.max_size = 64.0;
    t.fit_mode = TextFit::Shrink;
    t.resolved_font_size = 8.0;
    let id = director.scene.add_node(Box::new(t));
    director.timeline.push(director_core::director::TimelineItem {
        scene_root: id,
        name: None,
        start_time: 0.0,
        duration: 1.0,
        z_index: 0,
        audio_tracks: vec![],
        freeze_time: None,
    });

    director.update(0.0);
    director_core::systems::renderer::render_frame(
        &mut director,
        0.0,
        skia_safe::surfaces::raster_n32_premul((200, 100))
            .unwrap()
            .canvas(),
    )
    .unwrap();
    let text = director
        .scene
        .get_node(id)
        .unwrap()
        .element
        .as_any()
        .downcast_ref::<TextNode>()
        .unwrap();
    // The animated value is left untouched (audit #10: animation drives
    // the *upper bound*). The size Skia actually renders goes through
    // `resolved_font_size`, which IS clamped to min_size and used by Shrink.
    assert!(
        (text.default_font_size.current_value - 8.0).abs() < 0.001,
        "animated value must remain raw keyframe output"
    );
    let resolved = text.resolved_font_size;
    assert!(
        resolved >= 24.0 - 0.001 && resolved <= 24.0 + 0.001,
        "resolved size must clamp to min_size when animation undershoots; resolved={resolved}"
    );
}

