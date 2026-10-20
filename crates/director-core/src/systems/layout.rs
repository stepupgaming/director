//! # Layout System
//!
//! CSS Flexbox layout via Taffy.
//!
//! ## Responsibilities
//! - **Layout Computation**: Runs Taffy on the scene graph each frame.
//! - **Style Mapping**: Converts node styles to Taffy `Style`.
//! - **Intrinsic Sizing**: Handles `needs_measure()` nodes (text, images).
//!
//! ## Key Types
//! - `LayoutEngine`: Manages the Taffy tree and layout cache.

use crate::scene::SceneGraph;
use crate::types::NodeId;
use taffy::prelude::*;
use taffy::TraversePartialTree;
use tracing::{instrument, warn};

/// Manages the layout computation using the Taffy engine.
///
/// `LayoutEngine` synchronizes the Director's Scene Graph with Taffy's internal tree,
/// computes the Flexbox/Grid layout, and writes the results back to `SceneNode::layout_rect`.
pub struct LayoutEngine {
    taffy: TaffyTree<NodeId>, // Store Director NodeId as context
    // Persistent map for mapping Director NodeId -> Taffy NodeId
    node_map: std::collections::HashMap<NodeId, taffy::NodeId>,
}

impl LayoutEngine {
    /// Creates a new LayoutEngine instance.
    pub fn new() -> Self {
        Self {
            taffy: TaffyTree::new(),
            node_map: std::collections::HashMap::new(),
        }
    }

    /// Computes the layout for the current frame.
    ///
    /// # Process
    /// 1. **Sync Phase A**: Creates new Taffy nodes for new SceneNodes and updates styles for dirty nodes.
    /// 2. **Sync Phase B**: Updates parent-child relationships in Taffy to match the Scene Graph.
    /// 3. **Compute**: Triggers `taffy.compute_layout` for all active scene roots.
    /// 4. **Write Back**: Copies the computed (x, y, w, h) from Taffy back to `SceneNode`.
    #[instrument(level = "debug", skip(self, scene), fields(time = time))]
    pub fn compute_layout(&mut self, scene: &mut SceneGraph, width: i32, height: i32, time: f64) {
        // 1. Sync Phase A: Ensure Nodes Exist & Update Styles
        // Drop taffy nodes whose scene ids no longer resolve (destroy / gen bump).
        let stale_keys: Vec<NodeId> = self
            .node_map
            .keys()
            .copied()
            .filter(|id| scene.get_node(*id).is_none())
            .collect();
        for id in stale_keys {
            if let Some(t_id) = self.node_map.remove(&id) {
                self.taffy.remove(t_id).ok();
            }
        }

        for (id, node) in scene.iter_alive_mut() {
            // Ensure existence in Taffy
            let t_id = if let Some(&existing_t_id) = self.node_map.get(&id) {
                existing_t_id
            } else {
                let style = node.element.layout_style();

                // All nodes now have context (Director NodeId) to support measure if needed
                match self.taffy.new_leaf_with_context(style, id) {
                    Ok(new_t_id) => {
                        self.node_map.insert(id, new_t_id);
                        new_t_id
                    }
                    Err(e) => {
                        warn!(node_id = %id, error = %e, "Failed to create taffy node");
                        continue;
                    }
                }
            };

            // Sync Style if dirty
            if node.dirty_style {
                let style = node.element.layout_style();
                if self
                    .taffy
                    .style(t_id)
                    .map(|current| current != &style)
                    .unwrap_or(true)
                {
                    if let Err(e) = self.taffy.set_style(t_id, style) {
                        warn!(node_id = %id, error = %e, "Failed to sync taffy style");
                    }
                } else if node.element.needs_measure() {
                    // Text/media content may change its intrinsic size without
                    // changing Style. Preserve measure invalidation in that case.
                    if let Err(e) = self.taffy.mark_dirty(t_id) {
                        warn!(node_id = %id, error = %e, "Failed to invalidate taffy measure");
                    }
                }

                node.dirty_style = false;
            }
        }

        // 2. Sync Phase B: Update Relationships (Children)
        // We iterate again. Since we updated all nodes in Phase A, all valid children should be in node_map.
        for (id, node) in scene.iter_alive() {
            if let Some(&t_id) = self.node_map.get(&id) {
                let children = node
                    .children
                    .iter()
                    .copied()
                    .chain(node.mask_node)
                    .filter_map(|id| self.node_map.get(&id).copied());
                // Taffy 0.9.2 always dirties layout in set_children, even for
                // identical lists. Compare without allocating on the warm path.
                if !self.taffy.child_ids(t_id).eq(children.clone()) {
                    let children_t_ids: Vec<_> = children.collect();
                    if let Err(e) = self.taffy.set_children(t_id, &children_t_ids) {
                        warn!(node_id = %id, error = %e, "Failed to sync taffy children");
                    }
                }
            }
        }

        // 3. Compute Layout for Active Roots
        // Iterate scene nodes to find active roots (parent is None + recently visited)
        let mut active_roots = Vec::new();
        for (id, node) in scene.iter_alive() {
            // Is a root?
            if node.parent.is_none() {
                // Is active? (Visited in this frame)
                if (node.last_visit_time - time).abs() < 0.001 {
                    active_roots.push(id);
                }
            }
        }

        for root_id in active_roots {
            // Need to handle missing node safely
            if scene.get_node(root_id).is_some() {
                if let Some(&root_t_id) = self.node_map.get(&root_id) {
                    // Taffy measure closure
                    let measure_func = |known_dimensions: Size<Option<f32>>,
                                        available_space: Size<AvailableSpace>,
                                        _node_id: taffy::NodeId,
                                        context: Option<&mut NodeId>,
                                        _style: &Style|
                     -> Size<f32> {
                        if let Some(director_node_id) = context {
                            if let Some(node) = scene.get_node(*director_node_id) {
                                if node.element.needs_measure() {
                                    return node.element.measure(known_dimensions, available_space);
                                }
                            }
                        }
                        Size::ZERO
                    };

                    if let Err(e) = self.taffy.compute_layout_with_measure(
                        root_t_id,
                        Size {
                            width: AvailableSpace::Definite(width as f32),
                            height: AvailableSpace::Definite(height as f32),
                        },
                        measure_func,
                    ) {
                        warn!(%root_id, error = %e, "Failed to compute layout");
                    } else {
                        // 4. Write back results to Scene Nodes
                        self.write_back_recursive(scene, root_id);
                    }
                }
            }
        }
    }

    fn write_back_recursive(&self, scene: &mut SceneGraph, node_id: NodeId) {
        if let Some(t_id) = self.node_map.get(&node_id) {
            let layout = match self.taffy.layout(*t_id) {
                Ok(layout) => layout,
                Err(e) => {
                    warn!(%node_id, error = %e, "Failed to read taffy layout");
                    return;
                }
            };

            // Scope for mutable borrow
            let (children, mask_node) = {
                let Some(node) = scene.get_node_mut(node_id) else {
                    warn!(%node_id, "Missing scene node during layout write-back");
                    return;
                };

                node.layout_rect = skia_safe::Rect::from_xywh(
                    layout.location.x,
                    layout.location.y,
                    layout.size.width,
                    layout.size.height,
                );
                (node.children.clone(), node.mask_node)
            };

            // Recurse
            for child_id in children {
                self.write_back_recursive(scene, child_id);
            }
            if let Some(mask_id) = mask_node {
                self.write_back_recursive(scene, mask_id);
            }
        }
    }
}

impl Default for LayoutEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::Element;
    use crate::node::BoxNode;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[derive(Clone, Debug)]
    struct MeasuredContent {
        width: f32,
        calls: Arc<AtomicUsize>,
    }

    impl Element for MeasuredContent {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
        fn layout_style(&self) -> Style {
            Style::default()
        }
        fn needs_measure(&self) -> bool {
            true
        }
        fn measure(&self, known: Size<Option<f32>>, _: Size<AvailableSpace>) -> Size<f32> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Size {
                width: known.width.unwrap_or(self.width),
                height: known.height.unwrap_or(20.0),
            }
        }
        fn update(&mut self, _: f64) -> bool {
            false
        }
        fn render(
            &self,
            _: &skia_safe::Canvas,
            _: skia_safe::Rect,
            _: f32,
            _: &mut dyn FnMut(&skia_safe::Canvas),
        ) -> Result<(), crate::RenderError> {
            Ok(())
        }
        fn animate_property(
            &mut self,
            _: &str,
            _: f32,
            _: f32,
            _: f64,
            _: &str,
        ) -> Result<(), String> {
            Err("no animated properties".into())
        }
    }

    #[test]
    fn unchanged_children_and_paint_only_updates_preserve_measure_cache() {
        let mut scene = SceneGraph::new();
        let mut root = BoxNode::new();
        root.style.size = Size {
            width: Dimension::length(200.0),
            height: Dimension::length(100.0),
        };
        let root = scene.add_node(Box::new(root));
        let calls = Arc::new(AtomicUsize::new(0));
        let measured = scene.add_node(Box::new(MeasuredContent {
            width: 30.0,
            calls: calls.clone(),
        }));
        scene.add_child(root, measured);
        scene.get_node_mut(root).unwrap().last_visit_time = 0.0;
        let mut layout = LayoutEngine::new();
        layout.compute_layout(&mut scene, 200, 100, 0.0);
        let first_calls = calls.load(Ordering::SeqCst);
        assert!(first_calls > 0);
        let original_rect = scene.get_node(measured).unwrap().layout_rect;
        // Box paint updates report dirty_style even when the Style is unchanged.
        for _ in 0..3 {
            scene.get_node_mut(root).unwrap().dirty_style = true;
            layout.compute_layout(&mut scene, 200, 100, 0.0);
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            first_calls,
            "unchanged hierarchy/style must retain Taffy's measure cache"
        );
        assert_eq!(scene.get_node(measured).unwrap().layout_rect, original_rect);

        let node = scene.get_node_mut(measured).unwrap();
        node.element
            .as_any_mut()
            .downcast_mut::<MeasuredContent>()
            .unwrap()
            .width = 80.0;
        node.dirty_style = true;
        layout.compute_layout(&mut scene, 200, 100, 0.0);
        assert!(
            calls.load(Ordering::SeqCst) > first_calls,
            "content-only edits must invalidate intrinsic measurements"
        );
        assert_eq!(scene.get_node(measured).unwrap().layout_rect.width(), 80.0);
    }
}
