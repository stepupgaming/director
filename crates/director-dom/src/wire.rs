//! An exact copy of a scraper tree for the pipe. Nodes travel in arena order
//! with their children, so the copy has the same node ids, detached nodes
//! included.

use ego_tree::NodeId;
use html5ever::tree_builder::QuirksMode;
use html5ever::{Attribute, LocalName, Namespace, Prefix, QualName};
use scraper::node::{Comment, Doctype, Element, Node, ProcessingInstruction, Text};
use scraper::{Html, StrTendril};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tree {
    quirks: u8,
    /// Each node with the arena indexes of its children in order.
    nodes: Vec<(Wire, Vec<u32>)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Wire {
    Document,
    Fragment,
    Doctype(String, String, String),
    Comment(String),
    Text(String),
    Element(Name, Vec<(Name, String)>),
    Instruction(String, String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Name(Option<String>, String, String);

impl Name {
    fn of(q: &QualName) -> Self {
        Self(q.prefix.as_ref().map(|p| p.to_string()), q.ns.to_string(), q.local.to_string())
    }
    fn qual(&self) -> QualName {
        QualName::new(self.0.as_deref().map(Prefix::from), Namespace::from(&*self.1), LocalName::from(&*self.2))
    }
}

/// `html` for the pipe.
pub fn save(html: &Html) -> Tree {
    let at: HashMap<NodeId, u32> = html.tree.nodes().enumerate().map(|(k, n)| (n.id(), k as u32)).collect();
    let nodes = html
        .tree
        .nodes()
        .map(|n| {
            let wire = match n.value() {
                Node::Document => Wire::Document,
                Node::Fragment => Wire::Fragment,
                Node::Doctype(d) => Wire::Doctype(d.name.to_string(), d.public_id.to_string(), d.system_id.to_string()),
                Node::Comment(c) => Wire::Comment(c.comment.to_string()),
                Node::Text(t) => Wire::Text(t.text.to_string()),
                Node::Element(e) => Wire::Element(
                    Name::of(&e.name),
                    e.attrs.iter().map(|(q, v)| (Name::of(q), v.to_string())).collect(),
                ),
                Node::ProcessingInstruction(p) => Wire::Instruction(p.target.to_string(), p.data.to_string()),
            };
            (wire, n.children().map(|c| at[&c.id()]).collect())
        })
        .collect();
    let quirks = match html.quirks_mode {
        QuirksMode::Quirks => 0,
        QuirksMode::LimitedQuirks => 1,
        QuirksMode::NoQuirks => 2,
    };
    Tree { quirks, nodes }
}

/// The document a [`Tree`] copies, with its node ids in arena order.
pub fn load(tree: Tree) -> Result<(Html, Vec<NodeId>), String> {
    let mut html = Html::new_document();
    html.quirks_mode = match tree.quirks {
        0 => QuirksMode::Quirks,
        1 => QuirksMode::LimitedQuirks,
        _ => QuirksMode::NoQuirks,
    };
    let mut nodes = tree.nodes.into_iter();
    let (first, first_children) = nodes.next().ok_or("an empty document tree")?;
    let mut links = vec![first_children];
    html.tree = ego_tree::Tree::with_capacity(node(first), nodes.len() + 1);
    let mut ids = vec![html.tree.root().id()];
    for (wire, children) in nodes {
        ids.push(html.tree.orphan(node(wire)).id());
        links.push(children);
    }
    for (k, children) in links.into_iter().enumerate() {
        for c in children {
            let child = *ids.get(c as usize).ok_or("a tree child out of range")?;
            html.tree.get_mut(ids[k]).unwrap().append_id(child);
        }
    }
    Ok((html, ids))
}

fn node(wire: Wire) -> Node {
    match wire {
        Wire::Document => Node::Document,
        Wire::Fragment => Node::Fragment,
        Wire::Doctype(name, public_id, system_id) => Node::Doctype(Doctype {
            name: StrTendril::from(name),
            public_id: StrTendril::from(public_id),
            system_id: StrTendril::from(system_id),
        }),
        Wire::Comment(c) => Node::Comment(Comment { comment: StrTendril::from(c) }),
        Wire::Text(t) => Node::Text(Text { text: StrTendril::from(t) }),
        Wire::Element(name, attrs) => Node::Element(Element::new(
            name.qual(),
            attrs.into_iter().map(|(n, v)| Attribute { name: n.qual(), value: v.into() }).collect(),
        )),
        Wire::Instruction(target, data) => Node::ProcessingInstruction(ProcessingInstruction {
            target: StrTendril::from(target),
            data: StrTendril::from(data),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_tree_loads_with_the_same_arena() {
        let mut html = Html::parse_document(
            "<!DOCTYPE html><html><body><svg xmlns:xlink=\"http://www.w3.org/1999/xlink\"><use xlink:href=\"#a\"/></svg><p b=\"2\" a=\"1\">hi<!--c--></p></body></html>",
        );
        let body = html.tree.root().descendants().find(|n| n.value().as_element().is_some_and(|e| e.name() == "body")).unwrap().id();
        let loose = html.tree.orphan(Node::Text(Text { text: "loose".into() })).id();
        html.tree.get_mut(loose).unwrap().append(Node::Fragment);
        let (copy, ids) = load(save(&html)).unwrap();
        assert_eq!(copy.html(), html.html());
        assert_eq!(ids.len(), html.tree.nodes().count());
        let original: Vec<NodeId> = html.tree.nodes().map(|n| n.id()).collect();
        assert_eq!(ids, original);
        assert_eq!(copy.tree.get(body).unwrap().children().count(), html.tree.get(body).unwrap().children().count());
        assert!(copy.tree.get(loose).unwrap().parent().is_none());
        assert_eq!(copy.quirks_mode, html.quirks_mode);
    }
}
