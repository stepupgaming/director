// Text Node Visual Regression Tests
//
// Tests for text rendering: basic text, multiline wrapping, colored text,
// large font, text shadows, rich multi-span text, and TextFit shrink.

use director_core::animation::Animated;
use director_core::director::TimelineItem;
use director_core::element::{TextFit, TextShadow, TextSpan};
use director_core::node::{BoxNode, TextNode};
use director_core::types::Color;
use taffy::style::Dimension;

/// Simple single-span text on a dark background.
#[test]
fn text_basic() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let spans = vec![TextSpan {
            text: "Hello World".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(32.0),
            color: Some(Color::WHITE),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let text_id = d.scene.add_node(Box::new(TextNode::new(spans, fc)));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_basic").unwrap();
}

/// Multi-line text that wraps within a constrained width.
#[test]
fn text_multiline() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let spans = vec![TextSpan {
            text: "This is a longer piece of text that should wrap across \
                   multiple lines when rendered in a constrained layout box."
                .to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(20.0),
            color: Some(Color::WHITE),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let mut text_node = TextNode::new(spans, fc);
        text_node.style.size.width = Dimension::length(250.0);
        let text_id = d.scene.add_node(Box::new(text_node));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_multiline").unwrap();
}

/// Red text on a white background for color contrast verification.
#[test]
fn text_colored() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::WHITE));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let spans = vec![TextSpan {
            text: "Colored Text".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(36.0),
            color: Some(Color::new(0.9, 0.1, 0.1, 1.0)),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let text_id = d.scene.add_node(Box::new(TextNode::new(spans, fc)));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_colored").unwrap();
}

/// Large font size text (48px).
#[test]
fn text_large() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let spans = vec![TextSpan {
            text: "BIG".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(72.0),
            color: Some(Color::WHITE),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let text_id = d.scene.add_node(Box::new(TextNode::new(spans, fc)));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_large").unwrap();
}

/// Text with a drop shadow effect.
#[test]
fn text_shadow() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.8, 0.8, 0.8, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let spans = vec![TextSpan {
            text: "Shadow".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(40.0),
            color: Some(Color::WHITE),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let mut text_node = TextNode::new(spans, fc);
        text_node.shadow = Some(TextShadow {
            color: Color::new(0.0, 0.0, 0.0, 0.5),
            blur: 8.0,
            offset: (4.0, 4.0),
        });
        let text_id = d.scene.add_node(Box::new(text_node));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_shadow").unwrap();
}

/// Rich text with multiple spans of different colors and sizes.
#[test]
fn text_rich_spans() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let spans = vec![
            TextSpan {
                text: "Red ".to_string(),
                font_family: Some("Roboto".to_string()),
                font_size: Some(28.0),
                color: Some(Color::new(0.9, 0.2, 0.2, 1.0)),
                ..Default::default()
            },
            TextSpan {
                text: "Green ".to_string(),
                font_family: Some("Roboto".to_string()),
                font_size: Some(36.0),
                color: Some(Color::new(0.2, 0.8, 0.2, 1.0)),
                ..Default::default()
            },
            TextSpan {
                text: "Blue".to_string(),
                font_family: Some("Roboto".to_string()),
                font_size: Some(22.0),
                color: Some(Color::new(0.3, 0.4, 0.9, 1.0)),
                ..Default::default()
            },
        ];

        let fc = d.assets.font_collection.clone();
        let text_id = d.scene.add_node(Box::new(TextNode::new(spans, fc)));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_rich_spans").unwrap();
}

/// TextFit::Shrink — large text shrinks to fit a small constrained box.
#[test]
fn text_textfit_shrink() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let spans = vec![TextSpan {
            text: "This text is too large for the box and should shrink.".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(80.0),
            color: Some(Color::WHITE),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let mut text_node = TextNode::new(spans, fc);
        text_node.fit_mode = TextFit::Shrink;
        text_node.min_size = 8.0;
        text_node.max_size = 80.0;
        text_node.style.size.width = Dimension::length(200.0);
        text_node.style.size.height = Dimension::length(60.0);
        let text_id = d.scene.add_node(Box::new(text_node));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_textfit_shrink").unwrap();
}

/// Left-aligned multi-line text in a fixed-width container.
#[test]
fn text_align_left() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let spans = vec![TextSpan {
            text: "Left aligned text should start from the left edge of its container and wrap naturally when the line becomes too long.".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(20.0),
            color: Some(Color::WHITE),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let mut text_node = TextNode::new(spans, fc);
        text_node.set_text_align(skia_safe::textlayout::TextAlign::Left);
        text_node.style.size.width = Dimension::length(260.0);
        text_node.style.size.height = Dimension::auto();
        let text_id = d.scene.add_node(Box::new(text_node));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_align_left").unwrap();
}

/// Center-aligned multi-line text in a fixed-width container.
#[test]
fn text_align_center() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let spans = vec![TextSpan {
            text: "Center aligned text should be centered within its container, with each line wrapping and maintaining equal space on both sides.".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(20.0),
            color: Some(Color::WHITE),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let mut text_node = TextNode::new(spans, fc);
        text_node.set_text_align(skia_safe::textlayout::TextAlign::Center);
        text_node.style.size.width = Dimension::length(260.0);
        text_node.style.size.height = Dimension::auto();
        let text_id = d.scene.add_node(Box::new(text_node));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_align_center").unwrap();
}

/// Right-aligned multi-line text in a fixed-width container.
#[test]
fn text_align_right() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let spans = vec![TextSpan {
            text: "Right aligned text should end flush with the right edge of its container, with each line wrapping and aligning to the right side.".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(20.0),
            color: Some(Color::WHITE),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let mut text_node = TextNode::new(spans, fc);
        text_node.set_text_align(skia_safe::textlayout::TextAlign::Right);
        text_node.style.size.width = Dimension::length(260.0);
        text_node.style.size.height = Dimension::auto();
        let text_id = d.scene.add_node(Box::new(text_node));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_align_right").unwrap();
}

/// Justify-aligned multi-line text in a fixed-width container.
#[test]
fn text_align_justify() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let spans = vec![TextSpan {
            text: "Justified text should stretch to fill the full width of each line, distributing space between words so both left and right edges are flush.".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(20.0),
            color: Some(Color::WHITE),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let mut text_node = TextNode::new(spans, fc);
        text_node.set_text_align(skia_safe::textlayout::TextAlign::Justify);
        text_node.style.size.width = Dimension::length(260.0);
        text_node.style.size.height = Dimension::auto();
        let text_id = d.scene.add_node(Box::new(text_node));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_align_justify").unwrap();
}

// ============================================================================
// Fit prop visual regression tests
// ============================================================================

/// fit: "shrink" — large text with width=100% shrinks to fit container.
#[test]
fn text_fit_shrink() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let spans = vec![TextSpan {
            text: "SHRINK".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(80.0),
            color: Some(Color::WHITE),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let mut text_node = TextNode::new(spans, fc);
        text_node.fit_mode = TextFit::Shrink;
        text_node.min_size = 8.0;
        text_node.max_size = 80.0;
        // shrink defaults to width=100% when no explicit width is set
        text_node.style.size.width = Dimension::percent(1.0);
        text_node.style.size.height = Dimension::length(80.0);
        let text_id = d.scene.add_node(Box::new(text_node));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_fit_shrink").unwrap();
}

/// fit: "auto" — text with width=auto() sizes to content, not container.
#[test]
fn text_fit_auto() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(400, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        // Short text should be narrow, not stretched to fill the 400px frame
        let spans = vec![TextSpan {
            text: "Auto".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(32.0),
            color: Some(Color::WHITE),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let mut text_node = TextNode::new(spans, fc);
        text_node.fit_mode = TextFit::None;
        text_node.style.size.width = Dimension::auto();
        text_node.style.size.height = Dimension::auto();
        let text_id = d.scene.add_node(Box::new(text_node));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_fit_auto").unwrap();
}

/// fit: "wrap" — text with fixed width wraps naturally within the bounds.
#[test]
fn text_fit_wrap() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let spans = vec![TextSpan {
            text: "Wrap mode text should flow across multiple lines when constrained to a narrow width container.".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(20.0),
            color: Some(Color::WHITE),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let mut text_node = TextNode::new(spans, fc);
        text_node.fit_mode = TextFit::None;
        text_node.style.size.width = Dimension::length(180.0);
        text_node.style.size.height = Dimension::auto();
        let text_id = d.scene.add_node(Box::new(text_node));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_fit_wrap").unwrap();
}

/// Text with stroke (outline) around each glyph.
#[test]
fn text_stroke() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let spans = vec![TextSpan {
            text: "STROKE".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(48.0),
            color: Some(Color::WHITE),
            stroke_width: Some(3.0),
            stroke_color: Some(Color::new(0.9, 0.2, 0.2, 1.0)),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let text_id = d.scene.add_node(Box::new(TextNode::new(spans, fc)));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_stroke").unwrap();
}

/// Text span with a background color behind the glyphs.
#[test]
fn text_background() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let spans = vec![TextSpan {
            text: "Background".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(36.0),
            color: Some(Color::WHITE),
            background_color: Some(Color::new(0.2, 0.5, 0.8, 1.0)),
            background_padding: Some(6.0),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let text_id = d.scene.add_node(Box::new(TextNode::new(spans, fc)));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_background").unwrap();
}

// ============================================================================
// Font Weight / Style Visual Regression Tests
// ============================================================================

/// Side-by-side comparison of normal, bold, and extra-bold numeric weights.
#[test]
fn font_weight_matrix() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let spans = vec![
            TextSpan {
                text: "400 ".to_string(),
                font_family: Some("Roboto".to_string()),
                font_size: Some(24.0),
                font_weight: Some(400),
                color: Some(Color::WHITE),
                ..Default::default()
            },
            TextSpan {
                text: "700 ".to_string(),
                font_family: Some("Roboto".to_string()),
                font_size: Some(24.0),
                font_weight: Some(700),
                color: Some(Color::WHITE),
                ..Default::default()
            },
            TextSpan {
                text: "900".to_string(),
                font_family: Some("Roboto".to_string()),
                font_size: Some(24.0),
                font_weight: Some(900),
                color: Some(Color::WHITE),
                ..Default::default()
            },
        ];

        let fc = d.assets.font_collection.clone();
        let text_id = d.scene.add_node(Box::new(TextNode::new(spans, fc)));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "font_weight_matrix").unwrap();
}

/// Side-by-side comparison of upright and italic text.
#[test]
fn font_style_italic() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let spans = vec![
            TextSpan {
                text: "Upright ".to_string(),
                font_family: Some("Roboto".to_string()),
                font_size: Some(28.0),
                color: Some(Color::WHITE),
                ..Default::default()
            },
            TextSpan {
                text: "Italic".to_string(),
                font_family: Some("Roboto".to_string()),
                font_size: Some(28.0),
                font_style: Some("italic".to_string()),
                color: Some(Color::WHITE),
                ..Default::default()
            },
        ];

        let fc = d.assets.font_collection.clone();
        let text_id = d.scene.add_node(Box::new(TextNode::new(spans, fc)));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "font_style_italic").unwrap();
}

// ============================================================================
// Text Gradient Visual Regression Test
// ============================================================================

/// Text with a vertical gradient fill overlay.
#[test]
fn text_gradient() {
    use crate::visual::{assert_visual_match, setup_test_director};
    use director_core::types::GradientConfig;

    let director_arc = setup_test_director(300, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let spans = vec![TextSpan {
            text: "Gradient".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(48.0),
            fill_gradient: Some(GradientConfig {
                colors: vec![
                    Color::new(0.9, 0.2, 0.2, 1.0),
                    Color::new(0.2, 0.2, 0.9, 1.0),
                ],
                positions: None,
                start: (0.0, 0.0),
                end: (0.0, 1.0),
                ..Default::default()
            }),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let text_id = d.scene.add_node(Box::new(TextNode::new(spans, fc)));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_gradient").unwrap();
}
