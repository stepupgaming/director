//! Structural diff of two sessions' scene documents, with the patch that
//! turns the first into the second when one exists.

use crate::patch::{NodeSelector, PatchOp};
use crate::session::Session;
use director_scene::{NodeRecord, NodeRef};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

type Key = (u32, u32);

/// What changed from `before` to `after`.
#[derive(Clone, Debug, Serialize)]
pub struct Diff {
    /// The documents are equal.
    pub equal: bool,
    /// Changes outside the node records, as document JSON pointers.
    pub document: Vec<Change>,
    /// Nodes of `after` with no match in `before`.
    pub added_nodes: Vec<DiffNode>,
    /// Nodes of `before` with no match in `after`.
    pub removed_nodes: Vec<DiffNode>,
    /// Matched nodes under another parent or in another sibling order.
    pub moved_nodes: Vec<DiffNode>,
    /// Matched nodes whose record changed, as node JSON pointers.
    pub changed_nodes: Vec<NodeChanges>,
    /// Whether `patch` applied to `before` yields `after`. Added or moved
    /// nodes and matches that changed node id have no patch form.
    pub patchable: bool,
    /// Document sets, node sets, then removals, when `patchable`.
    pub patch: Vec<PatchOp>,
}

/// One value that differs. `path` is a JSON pointer.
#[derive(Clone, Debug, Serialize)]
pub struct Change {
    pub path: String,
    pub before: Value,
    pub after: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct DiffNode {
    pub id: NodeRef,
    pub name: Option<String>,
    pub kind: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct NodeChanges {
    /// The node in `before`.
    pub id: NodeRef,
    pub name: Option<String>,
    pub kind: String,
    pub changes: Vec<Change>,
}

/// Diff the current revisions of two sessions. Nodes match by authored name,
/// then by node id, and only when their kinds agree.
pub fn diff(before: &Session, after: &Session) -> Diff {
    let (old, new) = (&**before.document(), &**after.document());
    let old_names = names_by_node(before.names());
    let new_names = names_by_node(after.names());
    let new_by_name: HashMap<&str, &NodeRecord> = after
        .names()
        .iter()
        .filter_map(|(name, id)| Some((name.as_str(), record(new, *id)?)))
        .collect();
    let new_by_key: HashMap<Key, &NodeRecord> = new.nodes.iter().map(|record| (key(record.id), record)).collect();

    // before key -> after record
    let mut matched: BTreeMap<Key, &NodeRecord> = BTreeMap::new();
    let mut claimed = HashSet::new();
    for record in &old.nodes {
        let by_name = old_names.get(&key(record.id)).and_then(|name| new_by_name.get(name.as_str()));
        if let Some(other) = by_name.filter(|other| other.kind == record.kind && !claimed.contains(&key(other.id))) {
            claimed.insert(key(other.id));
            matched.insert(key(record.id), other);
        }
    }
    for record in &old.nodes {
        if matched.contains_key(&key(record.id)) {
            continue;
        }
        let other = new_by_key.get(&key(record.id)).filter(|other| {
            other.kind == record.kind && !claimed.contains(&key(other.id)) && !new_names.contains_key(&key(other.id))
        });
        if let Some(other) = other {
            claimed.insert(key(other.id));
            matched.insert(key(record.id), other);
        }
    }
    let describe = |record: &NodeRecord, names: &HashMap<Key, String>| DiffNode {
        id: record.id,
        name: names.get(&key(record.id)).cloned(),
        kind: record.kind.clone(),
    };
    let removed: Vec<&NodeRecord> = old.nodes.iter().filter(|record| !matched.contains_key(&key(record.id))).collect();
    let added_nodes: Vec<DiffNode> = new
        .nodes
        .iter()
        .filter(|record| !claimed.contains(&key(record.id)))
        .map(|record| describe(record, &new_names))
        .collect();
    let removed_keys: HashSet<Key> = removed.iter().map(|record| key(record.id)).collect();

    // Tree position: the mapped parent and the order of matched children.
    let mapped = |id: &NodeRef| matched.get(&key(*id)).map(|other| key(other.id));
    let mut moved_nodes = Vec::new();
    let mut changed_nodes = Vec::new();
    let mut node_ops = Vec::new();
    let mut same_ids = true;
    for record in &old.nodes {
        let Some(other) = matched.get(&key(record.id)) else {
            continue;
        };
        same_ids &= record.id == other.id;
        let parent_moved = record.parent.as_ref().map(mapped) != other.parent.map(|parent| Some(key(parent)));
        let old_children: Vec<Key> = record.children.iter().filter_map(mapped).collect();
        let new_children: Vec<Key> = other.children.iter().map(|child| key(*child)).filter(|child| claimed.contains(child)).collect();
        if parent_moved || old_children != new_children {
            moved_nodes.push(describe(record, &old_names));
        }
        let mut changes = Vec::new();
        let tree = ["id", "parent", "children"];
        compare(&to_value(record, &tree), &to_value(other, &tree), &mut String::new(), &mut changes);
        if changes.is_empty() {
            continue;
        }
        let name = old_names.get(&key(record.id)).cloned();
        let selector = match &name {
            Some(name) => NodeSelector::Name(name.clone()),
            None => NodeSelector::Ref(record.id),
        };
        node_ops.extend(changes.iter().map(|change| PatchOp::Set {
            node: selector.clone(),
            path: change.path.clone(),
            value: change.after.clone(),
        }));
        changed_nodes.push(NodeChanges {
            id: record.id,
            name,
            kind: record.kind.clone(),
            changes,
        });
    }

    let mut document = Vec::new();
    compare(&to_value(old, &["nodes"]), &to_value(new, &["nodes"]), &mut String::new(), &mut document);

    let patchable = added_nodes.is_empty() && moved_nodes.is_empty() && same_ids && document.iter().all(|change| change.path != "/version");
    let mut patch = Vec::new();
    if patchable {
        patch.extend(document.iter().map(|change| PatchOp::SetDocument {
            path: change.path.clone(),
            value: change.after.clone(),
        }));
        patch.append(&mut node_ops);
        // Removing a subtree root removes its descendants.
        patch.extend(
            removed
                .iter()
                .filter(|record| !record.parent.is_some_and(|parent| removed_keys.contains(&key(parent))))
                .map(|record| PatchOp::Remove {
                    node: match old_names.get(&key(record.id)) {
                        Some(name) => NodeSelector::Name(name.clone()),
                        None => NodeSelector::Ref(record.id),
                    },
                }),
        );
    }
    Diff {
        equal: old == new,
        document,
        added_nodes,
        removed_nodes: removed.iter().map(|record| describe(record, &old_names)).collect(),
        moved_nodes,
        changed_nodes,
        patchable,
        patch,
    }
}

fn key(id: NodeRef) -> Key {
    (id.index, id.generation)
}

fn record(document: &director_scene::SceneDocument, id: NodeRef) -> Option<&NodeRecord> {
    document.nodes.iter().find(|record| record.id == id)
}

fn names_by_node(names: &BTreeMap<String, NodeRef>) -> HashMap<Key, String> {
    let mut by_node = HashMap::new();
    for (name, id) in names {
        by_node.entry(key(*id)).or_insert_with(|| name.clone());
    }
    by_node
}

/// `value` as JSON without the top-level `skip` fields.
fn to_value(value: &impl Serialize, skip: &[&str]) -> Value {
    let mut json = serde_json::to_value(value).expect("scene documents serialize");
    if let Some(object) = json.as_object_mut() {
        object.retain(|field, _| !skip.contains(&field.as_str()));
    }
    json
}

/// Record the deepest paths where `left` and `right` differ. Objects with the
/// same keys and arrays of the same length recurse; anything else is one change.
/// An object or array of numbers (a color, a point) that differs in more than
/// one field is one change of the whole value, which replays the same.
fn compare(left: &Value, right: &Value, path: &mut String, changes: &mut Vec<Change>) {
    if left == right {
        return;
    }
    let len = path.len();
    let start = changes.len();
    let scalars = match (left, right) {
        (Value::Object(a), Value::Object(b)) if a.len() == b.len() && a.keys().all(|field| b.contains_key(field)) => {
            for (field, value) in a {
                path.push('/');
                path.push_str(&field.replace('~', "~0").replace('/', "~1"));
                compare(value, &b[field], path, changes);
                path.truncate(len);
            }
            a.values().all(Value::is_number)
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (index, (x, y)) in a.iter().zip(b).enumerate() {
                path.push('/');
                path.push_str(&index.to_string());
                compare(x, y, path, changes);
                path.truncate(len);
            }
            a.iter().all(Value::is_number)
        }
        _ => {
            changes.push(Change {
                path: path.clone(),
                before: left.clone(),
                after: right.clone(),
            });
            return;
        }
    };
    if scalars && changes.len() - start > 1 {
        changes.truncate(start);
        changes.push(Change {
            path: path.clone(),
            before: left.clone(),
            after: right.clone(),
        });
    }
}
