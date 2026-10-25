//! Incremental frames re-evaluate dirty nodes and their ancestors and
//! descendants, reuse clean static layers, and full frames update every node.

use director_core::compiler::DependencyInput;
use director_core::element::Element;
use director_core::node::BoxNode;
use director_core::types::NodeId;
use director_core::video_wrapper::RenderMode;
use director_core::{DefaultAssetLoader, Director, RenderError};
use director_render::FrameState;
use skia_safe::{AlphaType, Canvas, ColorSpace, ColorType, ImageInfo, Rect, Surface};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// A compiled scene of one root box with two child boxes.
fn two_boxes() -> (Director, NodeId, NodeId, NodeId) {
    let mut director = Director::new(1080, 1920, 30.0, Arc::new(DefaultAssetLoader), RenderMode::Preview, None);
    let root = director.scene.add_node(Box::new(BoxNode::new()));
    let child_a = director.scene.add_node(Box::new(BoxNode::new()));
    let child_b = director.scene.add_node(Box::new(BoxNode::new()));
    director.scene.try_add_child(root, child_a);
    director.scene.try_add_child(root, child_b);
    director.push_scene_root_at(root, 0.0, 5.0, Some("scene".into()));
    director.compile();
    (director, root, child_a, child_b)
}

fn surface(director: &Director) -> Surface {
    let info = ImageInfo::new(
        (director.width, director.height),
        ColorType::RGBA8888,
        AlphaType::Premul,
        Some(ColorSpace::new_srgb()),
    );
    skia_safe::surfaces::raster(&info, None, None).unwrap()
}

fn generation(director: &Director, id: NodeId) -> u64 {
    director.scene.get_node(id).unwrap().update_generation
}

/// Change a node's structural input so hash-based invalidation marks it dirty.
fn touch(director: &mut Director, id: NodeId) {
    director.scene.get_node_mut(id).unwrap().z_index = 7;
    director.invalidate(vec![DependencyInput::Node(id)]);
}

/// Counts its paints and measures 100 x 100.
#[derive(Clone, Debug)]
struct Tracker(Arc<AtomicUsize>);

impl Element for Tracker {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn layout_style(&self) -> taffy::style::Style {
        taffy::style::Style::default()
    }
    fn needs_measure(&self) -> bool {
        true
    }
    fn measure(
        &self,
        _: taffy::geometry::Size<Option<f32>>,
        _: taffy::geometry::Size<taffy::style::AvailableSpace>,
    ) -> taffy::geometry::Size<f32> {
        taffy::geometry::Size { width: 100.0, height: 100.0 }
    }
    fn update(&mut self, _: f64) -> bool {
        false
    }
    fn render(&self, _: &Canvas, _: Rect, _: f32, _: &mut dyn FnMut(&Canvas)) -> Result<(), RenderError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn animate_property(&mut self, _: &str, _: f32, _: f32, _: f64, _: &str) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn an_incremental_frame_skips_unchanged_nodes() {
    let (mut director, root, child_a, child_b) = two_boxes();
    let (mut state, mut surface) = (FrameState::default(), surface(&director));
    state.render(&mut director, 0.0, &mut surface).unwrap();
    let before = [root, child_a, child_b].map(|id| generation(&director, id));
    touch(&mut director, child_a);
    state.render(&mut director, 0.5, &mut surface).unwrap();
    let after = [root, child_a, child_b].map(|id| generation(&director, id));
    assert!(after[0] > before[0], "the ancestor of a dirty node is re-evaluated");
    assert!(after[1] > before[1], "the dirty node is re-evaluated");
    assert_eq!(after[2], before[2], "an unchanged sibling is not re-evaluated");
}

#[test]
fn an_incremental_frame_reuses_a_clean_static_subtree() {
    let (mut director, _, child_a, child_b) = two_boxes();
    let paints = Arc::new(AtomicUsize::new(0));
    let node = director.scene.get_node_mut(child_b).unwrap();
    node.element = Box::new(Tracker(paints.clone()));
    node.is_static = true;
    node.layout_rect = Rect::from_wh(100.0, 100.0);
    let (mut state, mut surface) = (FrameState::default(), surface(&director));
    state.render(&mut director, 0.0, &mut surface).unwrap();
    assert_eq!(paints.load(Ordering::SeqCst), 1, "a static subtree rasterizes once");
    touch(&mut director, child_a);
    state.render(&mut director, 0.5, &mut surface).unwrap();
    assert_eq!(paints.load(Ordering::SeqCst), 1, "a clean static subtree is not repainted");
}

#[test]
fn a_full_frame_re_evaluates_every_node() {
    let (mut director, _, child_a, child_b) = two_boxes();
    let (mut state, mut surface) = (FrameState::default(), surface(&director));
    state.render(&mut director, 0.0, &mut surface).unwrap();
    let before = [child_a, child_b].map(|id| generation(&director, id));
    state.render(&mut director, 0.5, &mut surface).unwrap();
    let after = [child_a, child_b].map(|id| generation(&director, id));
    assert!(after[0] > before[0] && after[1] > before[1], "{before:?} -> {after:?}");
}

#[test]
fn a_dirty_parent_re_evaluates_its_descendants() {
    let (mut director, root, child_a, child_b) = two_boxes();
    let (mut state, mut surface) = (FrameState::default(), surface(&director));
    state.render(&mut director, 0.0, &mut surface).unwrap();
    let before = [root, child_a, child_b].map(|id| generation(&director, id));
    touch(&mut director, root);
    state.render(&mut director, 0.5, &mut surface).unwrap();
    let after = [root, child_a, child_b].map(|id| generation(&director, id));
    assert!(before.iter().zip(&after).all(|(b, a)| a > b), "{before:?} -> {after:?}");
}
