//! Patch ops on a scene document by stable node id or authored name.
//!
//! Ops apply in order to a copy of the document. Any failure rejects the whole
//! patch, so a session only ever holds a document that every op reached.

use crate::error::{ErrorCode, HostError};
use director_scene::{NodeRef, SceneDocument};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

/// A node by authored name (an HTML `id`) or by generational id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum NodeSelector {
    Name(String),
    Ref(NodeRef),
}

/// One edit of the document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PatchOp {
    /// Replace the value at JSON pointer `path` inside the node record, such
    /// as `/opacity` or `/body/text`. Identity and tree fields (`/id`,
    /// `/parent`, `/children`) are fixed.
    Set { node: NodeSelector, path: String, value: Value },
    /// Replace the value at JSON pointer `path` of the document, such as
    /// `/fps` or `/timeline/tracks/0/clips/0/duration`. `/version` is fixed
    /// and `/nodes` changes through the node ops.
    SetDocument { path: String, value: Value },
    /// Remove the node and its subtree. A clip's scene root or a mask that a
    /// remaining node uses cannot be removed.
    Remove { node: NodeSelector },
}

/// Ops against the revision they were written for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Patch {
    pub base_revision: u64,
    pub ops: Vec<PatchOp>,
}

/// The node `selector` names in `document`.
pub fn resolve(document: &SceneDocument, names: &BTreeMap<String, NodeRef>, selector: &NodeSelector) -> Result<NodeRef, HostError> {
    let node = match selector {
        NodeSelector::Name(name) => *names
            .get(name)
            .ok_or_else(|| HostError::new(ErrorCode::NotFound, format!("no node is named `{name}`")))?,
        NodeSelector::Ref(node) => *node,
    };
    if document.nodes.iter().any(|record| record.id == node) {
        Ok(node)
    } else {
        Err(HostError::new(
            ErrorCode::NotFound,
            format!("node {}:{} is not in the document", node.index, node.generation),
        ))
    }
}

/// `document` with every op applied in order.
pub fn apply(document: &SceneDocument, names: &BTreeMap<String, NodeRef>, ops: &[PatchOp]) -> Result<SceneDocument, HostError> {
    let mut document = document.clone();
    for (index, op) in ops.iter().enumerate() {
        apply_op(&mut document, names, op).map_err(|error| HostError::new(error.code, format!("op {index}: {}", error.message)))?;
    }
    Ok(document)
}

fn apply_op(document: &mut SceneDocument, names: &BTreeMap<String, NodeRef>, op: &PatchOp) -> Result<(), HostError> {
    match op {
        PatchOp::Set { node, path, value } => {
            check_path(path, &["id", "parent", "children"])?;
            let node = resolve(document, names, node)?;
            let slot = document.nodes.iter_mut().find(|record| record.id == node).expect("resolved node");
            *slot = set_pointer(&*slot, path, value)?;
        }
        PatchOp::SetDocument { path, value } => {
            check_path(path, &["version", "nodes"])?;
            *document = set_pointer(&*document, path, value)?;
        }
        PatchOp::Remove { node } => {
            let node = resolve(document, names, node)?;
            remove(document, node)?;
        }
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> HostError {
    HostError::new(ErrorCode::InvalidPatch, message)
}

/// Reject an empty path and paths whose first segment is `fixed`.
fn check_path(path: &str, fixed: &[&str]) -> Result<(), HostError> {
    let Some(rest) = path.strip_prefix('/') else {
        return Err(invalid(format!("`{path}` is not a JSON pointer below the root")));
    };
    let first = rest.split('/').next().unwrap_or_default();
    if fixed.contains(&first) {
        return Err(invalid(format!("`/{first}` cannot be set")));
    }
    Ok(())
}

/// `target` with the value at `path` replaced, read back as its own type.
fn set_pointer<T: Serialize + serde::de::DeserializeOwned>(target: &T, path: &str, value: &Value) -> Result<T, HostError> {
    let mut json = serde_json::to_value(target).map_err(|error| invalid(error.to_string()))?;
    let slot = json.pointer_mut(path).ok_or_else(|| invalid(format!("`{path}` names no field")))?;
    *slot = value.clone();
    serde_json::from_value(json).map_err(|error| invalid(format!("`{path}`: {error}")))
}

fn remove(document: &mut SceneDocument, node: NodeRef) -> Result<(), HostError> {
    let mut subtree = HashSet::new();
    let mut stack = vec![node];
    while let Some(id) = stack.pop() {
        if subtree.insert((id.index, id.generation)) {
            if let Some(record) = document.nodes.iter().find(|record| record.id == id) {
                stack.extend(&record.children);
            }
        }
    }
    let inside = |id: &NodeRef| subtree.contains(&(id.index, id.generation));
    let clips = document.timeline.tracks.iter().flat_map(|track| &track.clips);
    if let Some(clip) = clips.into_iter().find(|clip| inside(&clip.scene_root)) {
        let name = clip.name.as_deref().unwrap_or("unnamed");
        return Err(invalid(format!("the subtree holds the scene root of clip `{name}`")));
    }
    if let Some(record) = document.nodes.iter().find(|record| !inside(&record.id) && record.mask.as_ref().is_some_and(inside)) {
        return Err(invalid(format!(
            "node {}:{} masks with a node in the subtree",
            record.id.index, record.id.generation
        )));
    }
    let parent = document.nodes.iter().find(|record| record.id == node).and_then(|record| record.parent);
    document.nodes.retain(|record| !inside(&record.id));
    if let Some(parent) = document.nodes.iter_mut().find(|record| Some(record.id) == parent) {
        parent.children.retain(|child| *child != node);
    }
    Ok(())
}
