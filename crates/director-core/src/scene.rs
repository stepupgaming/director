//! # Scene Graph Module
//!
//! Arena-based storage for the node hierarchy.
//!
//!
//! ## Responsibilities
//! - **Node Storage**: Generational arena (`nodes` + `generations`) keyed by [`NodeId`].
//! - **Hierarchy**: Parent-child relationships via `children` and `parent`.
//! - **Node Operations**: Add, remove, reparent nodes with cycle prevention.
//!
//! ## Key Types
//! - `SceneGraph`: The arena container.
//! - `SceneNode`: Wraps an `Element` with layout and hierarchy data.
//! - `NodeId`: Generational handle (`index` + `generation`) defined in `types.rs`.

use crate::element::Element;
use crate::entity::EntityId;
use crate::types::{FilterConfig, NodeId, PathAnimationState, PathMorphState, Transform};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Runtime binding of an audio analysis value to a node property.
///
/// Enables beat-reactive visuals by mapping frequency band energy to node properties.
#[derive(Clone, Debug)]
pub struct AudioBinding {
    /// Index of the audio track in the mixer
    pub track_id: usize,
    /// Frequency band: "bass", "mids", "highs"
    pub band: String,
    /// Property to animate: "scale", "opacity", "x", "y", "rotation"
    pub property: String,
    /// Minimum output value (when energy is 0)
    pub min_value: f32,
    /// Maximum output value (when energy is 1)
    pub max_value: f32,
    /// Smoothing factor (0.0 = instant, 0.9 = heavy smoothing)
    pub smoothing: f32,
    /// Previous smoothed value for temporal smoothing
    pub prev_value: f32,
}

/// A wrapper around an `Element` that adds scene graph relationships and state.
///
/// `SceneNode` encapsulates the specific logic for hierarchy, layout positioning,
/// masking, and temporal state (local time).
#[derive(Clone)]
pub struct SceneNode {
    /// The actual visual element (Box, Text, etc.)
    pub element: Box<dyn Element>,
    /// Indices of child nodes.
    pub children: Vec<NodeId>,
    /// Index of parent node.
    pub parent: Option<NodeId>,
    /// The computed absolute layout rectangle (set by `LayoutEngine`).
    pub layout_rect: skia_safe::Rect,
    /// The local time for the current frame (computed during update pass).
    pub local_time: f64,
    /// The global time when this node was last visited/prepared for update.
    pub last_visit_time: f64,

    // Path Animation
    pub path_animation: Option<PathAnimationState>,
    // Path Morphing (interpolate between two SVG paths)
    pub path_morph: Option<PathMorphState>,
    pub transform: Transform,

    // Masking & Compositing
    pub mask_node: Option<NodeId>,
    pub blend_mode: skia_safe::BlendMode,

    /// Explicit render order z-index (default: 0).
    /// Higher values render on top of lower values.
    /// Sorting is local to the parent's children list.
    pub z_index: i32,

    pub dirty_style: bool,

    /// Audio-reactive bindings for this node
    pub audio_bindings: Vec<AudioBinding>,

    /// CSS-like image filters applied to this node and its children.
    pub filter: FilterConfig,

    /// If true, the subtree rooted at this node is treated as static and may
    /// be rasterized once and reused via the Director's static cache.
    pub is_static: bool,
    /// Semantic entities this node references (e.g., a caption avoiding faces).
    pub entity_refs: Vec<EntityId>,
    /// Incremented each time `Element::update` is called on this node.
    /// Used by the incremental render tests to verify that unchanged nodes
    /// are not re-evaluated.
    pub update_generation: u64,
}

/// SAFETY (P0 threading audit): `SceneNode` is `Send` so Rayon's
/// `par_iter_mut` in `Director::update` can hand disjoint `&mut SceneNode`s
/// to worker threads. Soundness rests on three checked properties:
///
/// 1. All current `Element` impls are semantically `Send` (plain data +
///    `Animated` channels; Taffy `Style`'s `*const ()` is a niche
///    discriminant, never dereferenced; Skia handles live behind `Mutex`).
/// 2. The parallel pass touches each arena slot from exactly one thread;
///    `NodeId` cross-references are read-only in that pass.
/// 3. No `Element` impl may add thread-local interior mutability (`Rc`,
///    `RefCell`, raw pointers) without re-auditing this impl.
unsafe impl Send for SceneNode {}

impl SceneNode {
    /// Creates a new SceneNode wrapping the given Element.
    pub fn new(element: Box<dyn Element>) -> Self {
        Self {
            element,
            children: Vec::new(),
            parent: None,
            layout_rect: skia_safe::Rect::default(),
            local_time: 0.0,
            last_visit_time: -1.0,
            path_animation: None,
            path_morph: None,
            transform: Transform::new(),
            mask_node: None,
            blend_mode: skia_safe::BlendMode::SrcOver,
            z_index: 0,
            dirty_style: true,
            audio_bindings: Vec::new(),
            filter: FilterConfig::default(),
            is_static: false,
            entity_refs: Vec::new(),
            update_generation: 0,
        }
    }

    /// Compute a hash of the structural inputs that this node depends on.
    ///
    /// This includes the parent/mask hierarchy, children, referenced semantic
    /// entities, audio-reactive bindings, and other structural properties that
    /// affect rendering (z-index, static flag, blend mode, filter).
    /// Element-specific visual properties are intentionally excluded;
    /// `Element::update()` already signals visual changes via its return value.
    ///
    /// Note: because `children` is part of the hash, adding or removing a
    /// child changes the parent's hash. There is no `DependencyInput` for
    /// "children changed", so the parent must be invalidated directly (or
    /// `compile()` must be rerun) when the hierarchy changes.
    pub(crate) fn compute_input_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.parent.hash(&mut hasher);
        self.children.hash(&mut hasher);
        self.mask_node.hash(&mut hasher);
        self.entity_refs.hash(&mut hasher);
        for binding in &self.audio_bindings {
            binding.track_id.hash(&mut hasher);
            binding.band.hash(&mut hasher);
            binding.property.hash(&mut hasher);
            binding.min_value.to_bits().hash(&mut hasher);
            binding.max_value.to_bits().hash(&mut hasher);
            binding.smoothing.to_bits().hash(&mut hasher);
            // prev_value is runtime state, not an input.
        }
        self.z_index.hash(&mut hasher);
        self.is_static.hash(&mut hasher);
        self.blend_mode.hash(&mut hasher);
        self.filter.blur.to_bits().hash(&mut hasher);
        self.filter.brightness.to_bits().hash(&mut hasher);
        self.filter.contrast.to_bits().hash(&mut hasher);
        self.filter.saturation.to_bits().hash(&mut hasher);
        self.filter.hue_rotate.to_bits().hash(&mut hasher);
        self.filter.invert.to_bits().hash(&mut hasher);
        self.filter.sepia.to_bits().hash(&mut hasher);
        self.filter.opacity.to_bits().hash(&mut hasher);
        hasher.finish()
    }

}

/// The Scene Graph data structure.
///
/// Manages the arena of nodes and their relationships.
///
/// Slots are recycled via `free_indices`, but each free bumps `generations[index]`
/// so previously issued [`NodeId`] values never alias the next occupant.
#[derive(Clone)]
pub struct SceneGraph {
    /// Arena slots. `None` means free (or never occupied after destroy).
    pub nodes: Vec<Option<SceneNode>>,
    /// Parallel generation counters; length always equals `nodes.len()`.
    generations: Vec<u32>,
    /// Free slot indices available for reuse.
    free_indices: Vec<u32>,
}

impl Default for SceneGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl SceneGraph {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            generations: Vec::new(),
            free_indices: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.nodes.clear();
        self.generations.clear();
        self.free_indices.clear();
    }

    /// Live [`NodeId`] for an occupied slot index, if any.
    #[inline]
    pub fn id_at(&self, index: usize) -> Option<NodeId> {
        if self.nodes.get(index).and_then(|n| n.as_ref()).is_some() {
            Some(NodeId::new(index as u32, self.generations[index]))
        } else {
            None
        }
    }

    /// Generation currently stored at `index` (0 if out of range).
    #[inline]
    pub fn generation_at(&self, index: usize) -> u32 {
        self.generations.get(index).copied().unwrap_or(0)
    }

    /// Snapshot of slot generations for parallel iteration (index → generation).
    #[inline]
    pub fn generations_snapshot(&self) -> Vec<u32> {
        self.generations.clone()
    }

    /// Whether `id` resolves to a live node (index + generation match).
    #[inline]
    pub fn contains(&self, id: NodeId) -> bool {
        self.get_node(id).is_some()
    }

    /// Iterate live nodes as `(NodeId, &SceneNode)`.
    pub fn iter_alive(&self) -> impl Iterator<Item = (NodeId, &SceneNode)> {
        self.nodes.iter().enumerate().filter_map(|(index, slot)| {
            let node = slot.as_ref()?;
            Some((NodeId::new(index as u32, self.generations[index]), node))
        })
    }

    /// Iterate live nodes mutably as `(NodeId, &mut SceneNode)`.
    pub fn iter_alive_mut(&mut self) -> impl Iterator<Item = (NodeId, &mut SceneNode)> {
        self.nodes
            .iter_mut()
            .zip(self.generations.iter().copied())
            .enumerate()
            .filter_map(|(index, (slot, generation))| {
                let node = slot.as_mut()?;
                Some((NodeId::new(index as u32, generation), node))
            })
    }

    /// Adds a new element to the scene graph and returns its ID.
    pub fn add_node(&mut self, element: Box<dyn Element>) -> NodeId {
        if let Some(index) = self.free_indices.pop() {
            let idx = index as usize;
            // Generation was bumped on destroy; issue the new id with that generation.
            let generation = self.generations[idx];
            self.nodes[idx] = Some(SceneNode::new(element));
            NodeId::new(index, generation)
        } else {
            let index = self.nodes.len() as u32;
            self.nodes.push(Some(SceneNode::new(element)));
            self.generations.push(0);
            NodeId::new(index, 0)
        }
    }

    /// Recursively destroys a node and its children, freeing their indices for reuse.
    ///
    /// After destroy, `id` (and descendant ids) no longer resolve via [`get_node`].
    /// Slot reuse assigns a higher generation so the old handle cannot alias.
    pub fn destroy_node(&mut self, id: NodeId) {
        // 1. Require a live generational match.
        if self.get_node(id).is_none() {
            return;
        }
        let idx = id.index();

        // 2. Collect IDs to process (to avoid holding borrows on self.nodes)
        let (parent_id, children_ids, mask_id) = {
            let Some(node) = self.nodes[idx].as_ref() else {
                return;
            };
            (node.parent, node.children.clone(), node.mask_node)
        };

        // 3. Detach from Parent
        if let Some(pid) = parent_id {
            self.remove_child(pid, id);
        }

        // 4. Recursively destroy children and exclusive mask
        for child_id in children_ids {
            self.destroy_node(child_id);
        }
        if let Some(mask) = mask_id {
            self.destroy_node(mask);
        }

        // 5. Free the slot and bump generation so stale handles fail.
        self.nodes[idx] = None;
        self.generations[idx] = self.generations[idx].wrapping_add(1);
        self.free_indices.push(id.index);
    }

    /// Establishes a parent-child relationship between two nodes.
    ///
    /// Invalid relationships (missing nodes, self-parenting, cycles) are ignored.
    pub fn add_child(&mut self, parent: NodeId, child: NodeId) {
        let _ = self.try_add_child(parent, child);
    }

    /// Attempts to establish a parent-child relationship between two nodes.
    ///
    /// Returns `true` when the relationship is created and `false` when rejected
    /// (missing nodes, self-parenting, or cycle detection).
    pub fn try_add_child(&mut self, parent: NodeId, child: NodeId) -> bool {
        if parent == child {
            return false;
        }

        // Both nodes must exist (generational match).
        if self.get_node(parent).is_none() || self.get_node(child).is_none() {
            return false;
        }

        // Prevent hierarchy cycles by checking whether `child` is an ancestor of `parent`.
        let mut current = Some(parent);
        while let Some(node_id) = current {
            if node_id == child {
                return false;
            }
            current = self.get_node(node_id).and_then(|n| n.parent);
        }

        // Detach from previous parent if re-parenting.
        let old_parent = self.get_node(child).and_then(|n| n.parent);
        if let Some(old_parent_id) = old_parent {
            if old_parent_id == parent {
                // Already correctly parented.
                return true;
            }
            self.remove_child(old_parent_id, child);
        }

        if let Some(p_node) = self.get_node_mut(parent) {
            if !p_node.children.contains(&child) {
                p_node.children.push(child);
            }
        } else {
            return false;
        }

        if let Some(c_node) = self.get_node_mut(child) {
            c_node.parent = Some(parent);
            true
        } else {
            false
        }
    }

    /// Removes a child from a parent node's children list.
    /// Also clears the child's `parent` field when it points to this parent.
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) {
        if let Some(p_node) = self.get_node_mut(parent) {
            if let Some(pos) = p_node.children.iter().position(|&x| x == child) {
                p_node.children.remove(pos);
            }
        }
        if let Some(c_node) = self.get_node_mut(child) {
            if c_node.parent == Some(parent) {
                c_node.parent = None;
            }
        }
    }

    /// Returns a mutable reference to the SceneNode when `id` is live.
    pub fn get_node_mut(&mut self, id: NodeId) -> Option<&mut SceneNode> {
        let idx = id.index();
        if self.generations.get(idx).copied() != Some(id.generation) {
            return None;
        }
        self.nodes.get_mut(idx).and_then(|n| n.as_mut())
    }

    /// Returns a shared reference to the SceneNode when `id` is live.
    pub fn get_node(&self, id: NodeId) -> Option<&SceneNode> {
        let idx = id.index();
        if self.generations.get(idx).copied() != Some(id.generation) {
            return None;
        }
        self.nodes.get(idx).and_then(|n| n.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::SceneGraph;
    use crate::node::BoxNode;

    #[test]
    fn add_child_rejects_self_parent() {
        let mut scene = SceneGraph::new();
        let id = scene.add_node(Box::new(BoxNode::new()));

        assert!(!scene.try_add_child(id, id));
        assert!(scene.get_node(id).is_some());
    }

    #[test]
    fn add_child_rejects_cycle() {
        let mut scene = SceneGraph::new();
        let a = scene.add_node(Box::new(BoxNode::new()));
        let b = scene.add_node(Box::new(BoxNode::new()));
        let c = scene.add_node(Box::new(BoxNode::new()));

        assert!(scene.try_add_child(a, b));
        assert!(scene.try_add_child(b, c));
        assert!(
            !scene.try_add_child(c, a),
            "cycle creation must be rejected"
        );
    }

    #[test]
    fn reparent_child_detaches_from_old_parent() {
        let mut scene = SceneGraph::new();
        let p1 = scene.add_node(Box::new(BoxNode::new()));
        let p2 = scene.add_node(Box::new(BoxNode::new()));
        let child = scene.add_node(Box::new(BoxNode::new()));

        assert!(scene.try_add_child(p1, child));
        assert!(scene.try_add_child(p2, child));

        let p1_node = scene.get_node(p1).unwrap();
        let p2_node = scene.get_node(p2).unwrap();
        let child_node = scene.get_node(child).unwrap();

        assert!(!p1_node.children.contains(&child));
        assert!(p2_node.children.contains(&child));
        assert_eq!(child_node.parent, Some(p2));
    }

    #[test]
    fn node_id_no_alias_after_destroy() {
        let mut scene = SceneGraph::new();
        let old = scene.add_node(Box::new(BoxNode::new()));
        scene.destroy_node(old);
        let fresh = scene.add_node(Box::new(BoxNode::new()));

        assert_eq!(old.index, fresh.index, "slot should recycle");
        assert_ne!(old.generation, fresh.generation, "generation must bump");
        assert!(scene.get_node(old).is_none(), "stale id must not resolve");
        assert!(scene.get_node(fresh).is_some());
    }

    #[test]
    fn reparent_keeps_id() {
        let mut scene = SceneGraph::new();
        let p1 = scene.add_node(Box::new(BoxNode::new()));
        let p2 = scene.add_node(Box::new(BoxNode::new()));
        let child = scene.add_node(Box::new(BoxNode::new()));
        assert!(scene.try_add_child(p1, child));
        let before = child;
        assert!(scene.try_add_child(p2, child));
        assert_eq!(before, child);
        assert!(scene.get_node(before).is_some());
        assert_eq!(scene.get_node(child).unwrap().parent, Some(p2));
    }
}
