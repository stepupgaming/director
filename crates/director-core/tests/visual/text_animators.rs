// Per-Glyph Text Animation (Kinetic Typography) Visual Regression Tests
//
// Tests for TextNode.animators: karaoke-style opacity reveals and wave-style
// vertical offsets applied to individual characters.

use director_core::animation::{Animated, EasingType};
use director_core::director::TimelineItem;
use director_core::element::{Element, TextSpan};
use director_core::node::{BoxNode, TextNode};
use director_core::types::Color;
use taffy::style::Dimension;

/// Karaoke-style text reveal: each character fades in with a stagger.
/// Captured at t=1.0 — middle characters should be partially visible.
#[test]
fn karaoke_fade() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(400, 150);
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
            freeze_time: None,
        });

        let spans = vec![TextSpan {
            text: "KARAOKE".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(48.0),
            color: Some(Color::WHITE),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let mut text_node = TextNode::new(spans, fc);
        text_node.style.size.width = Dimension::auto();
        text_node.style.size.height = Dimension::auto();

        // Add per-glyph opacity animator: each character fades from 0 to 1
        // with 0.15s stagger between characters.
        let char_count = text_node.get_total_char_count();
        text_node.add_text_animator_full(
            0,
            char_count,
            "opacity".to_string(),
            0.0,
            1.0,
            2.0,
            "linear",
            0.15,
        );

        let text_id = d.scene.add_node(Box::new(text_node));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 1.0, "text_animators", "karaoke_fade").unwrap();
}

/// Wave-style text: each character oscillates vertically with a phase offset.
/// Captured at t=0.6 — letters should be at different Y positions forming a wave.
#[test]
fn text_wave() {
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
            freeze_time: None,
        });

        let spans = vec![TextSpan {
            text: "WAVE".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(56.0),
            color: Some(Color::new(0.2, 0.8, 0.6, 1.0)),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let mut text_node = TextNode::new(spans, fc);
        text_node.style.size.width = Dimension::auto();
        text_node.style.size.height = Dimension::auto();

        // Per-glyph Y offset: each character moves from -15 to +15 with stagger
        let char_count = text_node.get_total_char_count();
        text_node.add_text_animator_full(
            0,
            char_count,
            "offset_y".to_string(),
            -15.0,
            15.0,
            1.2,
            "ease_in_out",
            0.12,
        );

        let text_id = d.scene.add_node(Box::new(text_node));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.6, "text_animators", "text_wave").unwrap();
}
