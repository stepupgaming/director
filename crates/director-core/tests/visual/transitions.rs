// Scene transition frames captured at the midpoint of a one-second overlap.

use director_core::animation::{Animated, EasingType};
use director_core::node::BoxNode;
use director_core::systems::{Transition, TransitionType};
use director_core::types::Color;
use director_core::Director;
use taffy::style::Dimension;

fn push_color(director: &mut Director, color: Color, start: f64, duration: f64) {
    let mut root = BoxNode::new();
    root.style.size.width = Dimension::percent(1.0);
    root.style.size.height = Dimension::percent(1.0);
    root.bg_color = Some(Animated::new(color));
    let root_id = director.scene.add_node(Box::new(root));
    director.push_scene_root_at(root_id, start, duration, None);
}

fn transition_mid(kind: TransitionType, name: &str) {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(200, 120);
    {
        let mut director = director_arc.lock().unwrap();
        push_color(
            &mut director,
            Color::new(0.85, 0.15, 0.15, 1.0),
            0.0,
            2.0,
        );
        push_color(
            &mut director,
            Color::new(0.15, 0.35, 0.9, 1.0),
            1.0,
            2.0,
        );
        director.transitions.push(Transition {
            from_scene_idx: 0,
            to_scene_idx: 1,
            start_time: 1.0,
            duration: 1.0,
            kind,
            easing: EasingType::Linear,
        });
    }

    let mut director = director_arc.lock().unwrap();
    assert_visual_match(&mut director, 1.5, "transitions", name).unwrap();
}

#[test]
fn transition_fade_mid() {
    transition_mid(TransitionType::Fade, "transition_fade_mid");
}

#[test]
fn transition_wipe_mid() {
    transition_mid(TransitionType::WipeLeft, "transition_wipe_mid");
}
