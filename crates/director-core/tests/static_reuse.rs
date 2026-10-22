//! Full-pass static subtree reuse. Dirty static nodes still never hit.

use director_core::animation::{Animated, EasingType};
use director_core::director::Director;
use director_core::node::BoxNode;
use director_core::systems::layout::LayoutEngine;
use director_core::systems::renderer::render_at_time;
use director_core::types::{Color, NodeId};
use director_core::video_wrapper::RenderMode;
use director_core::DefaultAssetLoader;
use skia_safe::{AlphaType, ColorSpace, ColorType, ImageInfo};
use std::sync::Arc;
use taffy::style::Dimension;

fn empty_director(w: i32, h: i32) -> Director {
    Director::new(
        w,
        h,
        30.0,
        Arc::new(DefaultAssetLoader),
        RenderMode::Preview,
        None,
    )
}

fn read_pixels(director: &mut Director, time: f64) -> Vec<u8> {
    let info = ImageInfo::new(
        (director.width, director.height),
        ColorType::RGBA8888,
        AlphaType::Premul,
        Some(ColorSpace::new_srgb()),
    );
    let mut surface = skia_safe::surfaces::raster(&info, None, None).expect("surface");
    let mut layout = LayoutEngine::new();
    render_at_time(director, &mut layout, time, surface.canvas(), &mut None).expect("render");
    let mut pixels = vec![0u8; (director.width * director.height * 4) as usize];
    assert!(surface.read_pixels(
        &info,
        &mut pixels,
        (director.width * 4) as usize,
        (0, 0)
    ));
    pixels
}

fn max_channel_diff(a: &[u8], b: &[u8]) -> u8 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

fn has_opaque(pixels: &[u8], channel: usize) -> bool {
    pixels.as_chunks::<4>().0.iter().any(|px| {
        px[3] > 200
            && px[channel] > 200
            && (0..3).filter(|i| *i != channel).all(|i| px[i] < 40)
    })
}

fn scene() -> (Director, NodeId, NodeId) {
    let mut director = empty_director(100, 100);
    let mut root = BoxNode::new();
    root.style.size.width = Dimension::percent(1.0);
    root.style.size.height = Dimension::percent(1.0);
    root.bg_color = Some(Animated::new(Color::new(0.05, 0.05, 0.05, 1.0)));
    let root_id = director.scene.add_node(Box::new(root));
    director.push_scene_root(root_id, 5.0);

    let mut child = BoxNode::new();
    child.style.size.width = Dimension::length(40.0);
    child.style.size.height = Dimension::length(40.0);
    child.bg_color = Some(Animated::new(Color::new(1.0, 0.0, 0.0, 1.0)));
    let child_id = director.scene.add_node(Box::new(child));
    director.scene.add_child(root_id, child_id);
    (director, root_id, child_id)
}

fn image_id(director: &Director, id: NodeId) -> u32 {
    director
        .static_cache
        .get(&id)
        .expect("static cache entry")
        .image
        .unique_id()
}

#[test]
fn full_pass_reuses_unchanged_static_subtree() {
    let (mut director, root, _child) = scene();
    director.scene.get_node_mut(root).unwrap().is_static = true;

    let first = read_pixels(&mut director, 0.0);
    assert!(director.static_cache.contains_key(&root));
    let cached = image_id(&director, root);
    assert!(director.static_cache[&root].key.time.is_none());

    let second = read_pixels(&mut director, 1.5);
    assert!(director.static_cache.contains_key(&root));
    assert_eq!(image_id(&director, root), cached);
    assert_eq!(max_channel_diff(&first, &second), 0);

    let key = director.static_cache[&root].key;
    let rect = director.scene.get_node(root).unwrap().layout_rect;
    let pivot_x = director.scene.get_node(root).unwrap().transform.pivot_x;
    let pivot_y = director.scene.get_node(root).unwrap().transform.pivot_y;
    assert_eq!(key.width, rect.width());
    assert_eq!(key.height, rect.height());
    assert_eq!(key.translate_x, 0.0);
    assert_eq!(key.translate_y, 0.0);
    assert_eq!(key.scale_x, 1.0);
    assert_eq!(key.scale_y, 1.0);
    assert_eq!(key.rotation, 0.0);
    assert_eq!(key.skew_x, 0.0);
    assert_eq!(key.skew_y, 0.0);
    assert_eq!(key.pivot_x, rect.width() * pivot_x);
    assert_eq!(key.pivot_y, rect.height() * pivot_y);
    assert_eq!(key.parent_opacity, 1.0);
    assert!(key.time.is_none());
}

#[test]
fn full_pass_misses_when_inherited_opacity_changes() {
    let (mut director, root, _child) = scene();
    director.scene.get_node_mut(root).unwrap().is_static = true;

    let before = read_pixels(&mut director, 0.0);
    let cached = image_id(&director, root);
    director.tracks[0].opacity = 0.25;
    assert_eq!(image_id(&director, root), cached);

    let after = read_pixels(&mut director, 0.0);
    assert!(director.static_cache.contains_key(&root));
    assert_ne!(image_id(&director, root), cached);
    assert_eq!(director.static_cache[&root].key.parent_opacity, 0.25);
    assert!(max_channel_diff(&before, &after) > 0);
}

#[test]
fn full_pass_misses_when_layout_size_changes() {
    let (mut director, _root, child) = scene();
    director.scene.get_node_mut(child).unwrap().is_static = true;

    let before = read_pixels(&mut director, 0.0);
    let cached = image_id(&director, child);
    let before_width = director.static_cache[&child].key.width;
    {
        let node = director.scene.get_node_mut(child).unwrap();
        node.element
            .as_any_mut()
            .downcast_mut::<BoxNode>()
            .unwrap()
            .style
            .size
            .width = Dimension::length(90.0);
        node.dirty_style = true;
    }
    assert_eq!(image_id(&director, child), cached);

    let after = read_pixels(&mut director, 0.0);
    assert!(director.static_cache.contains_key(&child));
    assert_ne!(image_id(&director, child), cached);
    assert!(director.static_cache[&child].key.width > before_width);
    assert!(max_channel_diff(&before, &after) > 0);
}

#[test]
fn child_color_change_evicts_and_repaints() {
    let (mut director, root, child) = scene();
    director.scene.get_node_mut(root).unwrap().is_static = true;

    let before = read_pixels(&mut director, 0.0);
    assert!(has_opaque(&before, 0));
    assert!(director.static_cache.contains_key(&root));

    {
        let node = director.scene.get_node_mut(child).unwrap();
        node.element
            .as_any_mut()
            .downcast_mut::<BoxNode>()
            .unwrap()
            .bg_color = Some(Animated::new(Color::new(0.0, 1.0, 0.0, 1.0)));
    }
    director.note_node_mutation(child);
    assert!(
        !director.static_cache.contains_key(&root),
        "note_mutation must evict the static ancestor"
    );
    assert!(!director.static_cache.contains_key(&child));

    let after = read_pixels(&mut director, 0.0);
    assert!(director.static_cache.contains_key(&root));
    assert!(has_opaque(&after, 1), "second frame must show the new green");
    assert!(max_channel_diff(&before, &after) > 0);
}

#[test]
fn time_varying_child_is_not_served_from_the_first_frame() {
    let (mut director, root, child) = scene();
    director.scene.get_node_mut(root).unwrap().is_static = true;
    {
        let node = director.scene.get_node_mut(child).unwrap();
        let mut color = Animated::new(Color::new(1.0, 0.0, 0.0, 1.0));
        color.add_keyframe(Color::new(0.0, 0.0, 1.0, 1.0), 1.0, EasingType::Linear);
        node.element
            .as_any_mut()
            .downcast_mut::<BoxNode>()
            .unwrap()
            .bg_color = Some(color);
    }

    let first = read_pixels(&mut director, 0.0);
    assert!(director.static_cache.contains_key(&root));
    assert_eq!(director.static_cache[&root].key.time, Some(0.0));
    assert!(has_opaque(&first, 0));
    let cached = image_id(&director, root);

    let later = read_pixels(&mut director, 1.0);
    assert!(director.static_cache.contains_key(&root));
    assert_eq!(director.static_cache[&root].key.time, Some(1.0));
    assert_ne!(image_id(&director, root), cached);
    assert!(has_opaque(&later, 2), "later frame must show the animated blue");
    assert!(max_channel_diff(&first, &later) > 0);
}

#[test]
fn expression_in_subtree_misses_at_a_later_time() {
    let (mut director, root, child) = scene();
    director.scene.get_node_mut(root).unwrap().is_static = true;
    {
        let node = director.scene.get_node_mut(child).unwrap();
        node.element
            .as_any_mut()
            .downcast_mut::<BoxNode>()
            .unwrap()
            .opacity
            .raw_keyframes[0]
            .2 = EasingType::Expression {
            expr: "1".into(),
        };
    }

    let first = read_pixels(&mut director, 0.0);
    assert_eq!(director.static_cache[&root].key.time, Some(0.0));
    let cached = image_id(&director, root);

    let later = read_pixels(&mut director, 1.0);
    assert!(director.static_cache.contains_key(&root));
    assert_eq!(director.static_cache[&root].key.time, Some(1.0));
    assert_ne!(image_id(&director, root), cached);
    assert_eq!(max_channel_diff(&first, &later), 0);
}

#[test]
fn dirty_static_node_does_not_serve_a_matching_key() {
    let (mut director, _root, child) = scene();
    director.scene.get_node_mut(child).unwrap().is_static = true;
    director.compile();

    let before = read_pixels(&mut director, 0.0);
    assert!(has_opaque(&before, 0));
    assert!(director.static_cache.contains_key(&child));
    let cached = image_id(&director, child);

    {
        let node = director.scene.get_node_mut(child).unwrap();
        node.element
            .as_any_mut()
            .downcast_mut::<BoxNode>()
            .unwrap()
            .bg_color = Some(Animated::new(Color::new(0.0, 1.0, 0.0, 1.0)));
    }
    assert_eq!(image_id(&director, child), cached);
    director.dirty_nodes.insert(child);

    let after = read_pixels(&mut director, 0.0);
    assert!(director.static_cache.contains_key(&child));
    assert_ne!(image_id(&director, child), cached);
    assert!(has_opaque(&after, 1), "dirty static node must not replay the cached red");
    assert!(max_channel_diff(&before, &after) > 0);
}
