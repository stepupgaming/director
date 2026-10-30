//! The document page scripts see. Scraper's tree is mutated in place by the
//! script natives, which `dom.js` wraps in browser-shaped objects. Layout
//! reads come from a probe of the current tree.

use crate::wire::{self, Tree};
use crate::Attr;
use ego_tree::NodeId;
use html5ever::{ns, Attribute, LocalName, Namespace, QualName};
use scraper::node::{Comment, Element, Node, Text};
use scraper::{ElementRef, Html, Selector, StrTendril};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// What a probe knows about one element box.
pub trait Placement: Clone {
    /// Whether the box is `position: absolute` or `fixed`.
    fn positioned(&self) -> bool;
}

/// Probes a compacted copy of the document, keyed by that copy's node ids.
pub type Prober<P> = Box<dyn Fn(&Html) -> Result<HashMap<NodeId, P>, String>>;

/// The latest probe, the layout generation it saw and the composition clips
/// it compiled empty.
struct Probing<P> {
    layout: u64,
    emptied: HashSet<NodeId>,
    map: HashMap<NodeId, P>,
}

pub struct Doc<P> {
    pub html: Html,
    pub ids: Vec<NodeId>,
    index: HashMap<NodeId, usize>,
    selectors: HashMap<String, Rc<Selector>>,
    /// Bumped on every structural or attribute change.
    pub generation: u64,
    /// Bumped on every change that can move a box.
    layout: u64,
    /// Elements whose inline style changed only in paint since the probe,
    /// each with whether an inherited property changed.
    painted: HashMap<NodeId, bool>,
    /// The generation and node of the latest insert of a detached node, so
    /// removing it again restores the generation.
    undo: Option<(u64, NodeId)>,
    probe: Option<Probing<P>>,
    /// Probes taken so far, so answers derived from one probe can be cached.
    pub probes: u64,
    prober: Prober<P>,
    /// While the timeline plays, layout answers stay at the setup state.
    pub playing: bool,
    pub viewport: (f32, f32),
}

/// A document for the pipe, with each probed box as `Q`.
#[derive(Debug, Serialize, Deserialize)]
pub struct Saved<Q> {
    page: Tree,
    ids: Vec<u32>,
    generation: u64,
    layout: u64,
    painted: Vec<(u32, bool)>,
    undo: Option<(u64, u32)>,
    probe: Option<SavedProbe<Q>>,
    probes: u64,
    playing: bool,
    viewport: (f32, f32),
}

/// The last probe: its layout generation, the clips it compiled empty and
/// each probed box.
type SavedProbe<Q> = (u64, Vec<u32>, Vec<(u32, Q)>);

impl<P: Placement> Doc<P> {
    pub fn new(html: Html, prober: Prober<P>, viewport: (f32, f32)) -> Self {
        let mut doc = Self {
            html,
            ids: Vec::new(),
            index: HashMap::new(),
            selectors: HashMap::new(),
            generation: 0,
            layout: 0,
            painted: HashMap::new(),
            undo: None,
            probe: None,
            probes: 0,
            prober,
            playing: false,
            viewport,
        };
        let root = doc.html.tree.root().id();
        doc.register(root);
        let all: Vec<NodeId> = doc.html.tree.root().descendants().map(|n| n.id()).collect();
        for id in all {
            doc.register(id);
        }
        doc
    }

    /// This document for the pipe, each probed box mapped through `place`.
    pub fn save<Q>(&self, place: impl Fn(&P) -> Q) -> Saved<Q> {
        let at: HashMap<NodeId, u32> = self.html.tree.nodes().enumerate().map(|(k, n)| (n.id(), k as u32)).collect();
        Saved {
            page: wire::save(&self.html),
            ids: self.ids.iter().map(|id| at[id]).collect(),
            generation: self.generation,
            layout: self.layout,
            painted: self.painted.iter().map(|(id, inherited)| (at[id], *inherited)).collect(),
            undo: self.undo.map(|(generation, id)| (generation, at[&id])),
            probe: self.probe.as_ref().map(|p| {
                (
                    p.layout,
                    p.emptied.iter().map(|id| at[id]).collect(),
                    p.map.iter().map(|(id, placed)| (at[id], place(placed))).collect(),
                )
            }),
            probes: self.probes,
            playing: self.playing,
            viewport: self.viewport,
        }
    }

    /// The document `saved` holds, each probed box mapped through `place`.
    pub fn load<Q>(saved: Saved<Q>, place: impl Fn(Q) -> Option<P>, prober: Prober<P>) -> Result<Self, String> {
        let (html, arena) = wire::load(saved.page)?;
        let id = |k: u32| arena.get(k as usize).copied().ok_or_else(|| format!("saved node {k} is out of range"));
        let ids = saved.ids.into_iter().map(id).collect::<Result<Vec<_>, _>>()?;
        let probe = match saved.probe {
            Some((layout, emptied, map)) => Some(Probing {
                layout,
                emptied: emptied.into_iter().map(id).collect::<Result<_, _>>()?,
                map: map
                    .into_iter()
                    .filter_map(|(k, q)| match id(k) {
                        Ok(id) => place(q).map(|p| Ok((id, p))),
                        Err(e) => Some(Err(e)),
                    })
                    .collect::<Result<_, _>>()?,
            }),
            None => None,
        };
        Ok(Self {
            html,
            index: ids.iter().enumerate().map(|(i, id)| (*id, i)).collect(),
            ids,
            selectors: HashMap::new(),
            generation: saved.generation,
            layout: saved.layout,
            painted: saved.painted.into_iter().map(|(k, inherited)| Ok((id(k)?, inherited))).collect::<Result<_, String>>()?,
            undo: saved.undo.map(|(generation, k)| id(k).map(|id| (generation, id))).transpose()?,
            probe,
            probes: saved.probes,
            prober,
            playing: saved.playing,
            viewport: saved.viewport,
        })
    }

    /// Composition clips outside the ancestry and subtree of `id` that the
    /// last probe placed out of flow. Their content moves no box of `id`, so
    /// its probe compiles them empty.
    fn out_of_scope(&self, id: NodeId) -> HashSet<NodeId> {
        let Some(last) = &self.probe else { return HashSet::new() };
        let Some(node) = self.html.tree.get(id) else { return HashSet::new() };
        let ancestry: HashSet<NodeId> = node.ancestors().map(|a| a.id()).collect();
        self.html
            .tree
            .root()
            .descendants()
            .filter(|n| n.value().as_element().is_some_and(|e| e.attr_text("data-composition-id").is_some()))
            .filter(|n| !ancestry.contains(&n.id()) && n.id() != id && !n.ancestors().any(|a| a.id() == id))
            .filter(|n| last.map.get(&n.id()).is_some_and(Placement::positioned))
            .map(|n| n.id())
            .collect()
    }

    /// Whether paint writes since the probe changed the cascade of `id`.
    fn repainted(&self, id: NodeId) -> bool {
        if self.painted.contains_key(&id) {
            return true;
        }
        let Some(node) = self.html.tree.get(id) else { return false };
        node.ancestors().any(|a| self.painted.get(&a.id()) == Some(&true))
    }

    fn register(&mut self, id: NodeId) -> usize {
        if let Some(&i) = self.index.get(&id) {
            return i;
        }
        self.index.insert(id, self.ids.len());
        self.ids.push(id);
        self.ids.len() - 1
    }

    pub fn index_of(&self, id: NodeId) -> Option<usize> {
        self.index.get(&id).copied()
    }

    fn id(&self, i: usize) -> Result<NodeId, String> {
        self.ids.get(i).copied().ok_or_else(|| format!("unknown node {i}"))
    }

    fn node(&self, i: usize) -> Result<ego_tree::NodeRef<'_, Node>, String> {
        let id = self.id(i)?;
        self.html.tree.get(id).ok_or_else(|| format!("unknown node {i}"))
    }

    fn element(&self, i: usize) -> Result<ElementRef<'_>, String> {
        ElementRef::wrap(self.node(i)?).ok_or_else(|| "node is not an element".to_string())
    }

    fn selector(&mut self, text: &str) -> Result<Rc<Selector>, String> {
        if let Some(s) = self.selectors.get(text) {
            return Ok(s.clone());
        }
        let parsed = Selector::parse(text)
            .map_err(|e| format!("SyntaxError: '{text}' is not a valid selector: {e}"))?;
        let parsed = Rc::new(parsed);
        self.selectors.insert(text.to_string(), parsed.clone());
        Ok(parsed)
    }

    fn touch(&mut self) {
        self.generation += 1;
        self.layout += 1;
        self.undo = None;
    }

    /// Whether `node` hangs below the document root.
    fn connected(&self, id: NodeId) -> bool {
        let root = self.html.tree.root().id();
        self.html
            .tree
            .get(id)
            .is_some_and(|n| n.ancestors().any(|a| a.id() == root) || id == root)
    }

    fn orphan(&mut self, value: Node) -> usize {
        let id = self.html.tree.orphan(value).id();
        self.register(id)
    }

    fn set_element(&mut self, i: usize, edit: impl FnOnce(&mut Vec<Attribute>)) -> Result<(), String> {
        let id = self.id(i)?;
        let mut node = self.html.tree.get_mut(id).ok_or("unknown node")?;
        let Node::Element(el) = node.value() else {
            return Err("node is not an element".into());
        };
        let mut attrs: Vec<Attribute> = el
            .attrs
            .iter()
            .map(|(name, value)| Attribute {
                name: name.clone(),
                value: value.to_string().into(),
            })
            .collect();
        edit(&mut attrs);
        *el = Element::new(el.name.clone(), attrs);
        self.touch();
        Ok(())
    }

    fn deep_copy(&mut self, from: NodeId) -> NodeId {
        let value = self.html.tree.get(from).unwrap().value().clone();
        let children: Vec<NodeId> = self.html.tree.get(from).unwrap().children().map(|c| c.id()).collect();
        let copy = self.html.tree.orphan(value).id();
        self.register(copy);
        for child in children {
            let c = self.deep_copy(child);
            self.html.tree.get_mut(copy).unwrap().append_id(c);
        }
        copy
    }

    /// The value of attribute `name` on element `i`.
    pub fn attr(&self, i: usize, name: &str) -> Result<Option<String>, String> {
        Ok(self
            .element(i)?
            .value()
            .attrs
            .iter()
            .find(|(n, _)| attr_name(n) == name)
            .map(|(_, v)| v.to_string()))
    }

    /// Set or remove attribute `name` on element `i`.
    pub fn write_attr(&mut self, i: usize, name: &str, value: Option<String>) -> Result<(), String> {
        let element = self.element(i)?.value();
        let html = element.name.ns == ns!(html);
        let name = if html { name.to_ascii_lowercase() } else { name.to_string() };
        let paint = match name.as_str() {
            "style" => paint_change(element.attr_text("style").unwrap_or_default(), value.as_deref().unwrap_or_default()),
            _ => None,
        };
        let (id, layout) = (self.id(i)?, self.layout);
        self.set_element(i, |attrs| {
            let at = attrs.iter().position(|a| attr_name(&a.name) == name);
            match (value, at) {
                (Some(v), Some(at)) => attrs[at].value = v.into(),
                (Some(v), None) => {
                    let qual = match name.split_once(':') {
                        Some(("xlink", local)) => QualName::new(Some("xlink".into()), ns!(xlink), LocalName::from(local)),
                        _ => QualName::new(None, ns!(), LocalName::from(name.as_str())),
                    };
                    attrs.push(Attribute { name: qual, value: v.into() })
                }
                (None, Some(at)) => {
                    attrs.remove(at);
                }
                (None, None) => {}
            }
        })?;
        if let Some(inherited) = paint {
            self.layout = layout;
            *self.painted.entry(id).or_default() |= inherited;
        } else if self.element(i)?.value().attr_text("data-composition-id").is_some() {
            // Scoped probes trust each clip's last placement.
            self.probe = None;
        }
        Ok(())
    }

    /// `#id`, else `<tag>`, naming element `i` in diagnostics.
    pub fn label(&self, i: usize) -> String {
        match self.element(i) {
            Ok(el) => match el.value().attr_text("id") {
                Some(id) => format!("#{id}"),
                None => format!("<{}>", el.value().name()),
            },
            Err(_) => format!("node {i}"),
        }
    }

    /// The text content of node `i`.
    pub fn text(&self, i: usize) -> Result<String, String> {
        let node = self.node(i)?;
        Ok(match node.value() {
            Node::Text(t) => t.text.to_string(),
            Node::Comment(c) => c.comment.to_string(),
            Node::Element(_) | Node::Fragment | Node::Document => node
                .descendants()
                .filter_map(|n| n.value().as_text().map(|t| t.text.to_string()))
                .collect(),
            _ => String::new(),
        })
    }

    /// The element whose whole text a text node `i` is, or `None` when `i`
    /// is no text node.
    pub fn text_owner(&self, i: usize) -> Result<Option<usize>, String> {
        let node = self.node(i)?;
        if node.value().as_text().is_none() {
            return Ok(None);
        }
        let parent = node.parent().ok_or("GSAP writes a detached text node")?;
        if parent.children().count() != 1 {
            let owner = self.index_of(parent.id()).map_or_else(|| "an element".into(), |p| self.label(p));
            return Err(format!(
                "GSAP writes one text node of {owner}, which has other children; animate the element's textContent"
            ));
        }
        self.index_of(parent.id()).map(Some).ok_or_else(|| "GSAP writes an unregistered text node".into())
    }

    /// The nearest `<svg>` above element `i`, when `i` draws inside it.
    pub fn svg_owner(&self, i: usize) -> Result<Option<usize>, String> {
        let node = self.node(i)?;
        if node.value().as_element().is_none_or(|el| el.name.ns != ns!(svg)) {
            return Ok(None);
        }
        for above in node.ancestors() {
            let Some(el) = above.value().as_element() else { break };
            if el.name.ns != ns!(svg) || &*el.name.local == "foreignObject" {
                break;
            }
            if &*el.name.local == "svg" {
                return Ok(self.index_of(above.id()));
            }
        }
        Ok(None)
    }

    /// Whether element `i` is an `<svg>`.
    pub fn is_svg(&self, i: usize) -> Result<bool, String> {
        Ok(self.node(i)?.value().as_element().is_some_and(|el| el.name.ns == ns!(svg) && &*el.name.local == "svg"))
    }

    /// Append an SVG element `tag` with `attrs` as the last child of `i`.
    pub fn append_svg(&mut self, i: usize, tag: &str, attrs: Vec<(String, String)>) -> Result<(), String> {
        let id = self.id(i)?;
        let attrs = attrs
            .into_iter()
            .map(|(name, value)| Attribute { name: QualName::new(None, ns!(), LocalName::from(name)), value: value.into() })
            .collect();
        let element = Element::new(QualName::new(None, ns!(svg), LocalName::from(tag)), attrs);
        let child = self.html.tree.get_mut(id).ok_or("unknown node")?.append(Node::Element(element)).id();
        self.register(child);
        self.touch();
        Ok(())
    }

    /// Replace the text content of node `i`.
    pub fn write_text(&mut self, i: usize, value: &str) -> Result<(), String> {
        let id = self.id(i)?;
        let mut node = self.html.tree.get_mut(id).ok_or("unknown node")?;
        let parent = match node.value() {
            Node::Text(t) => {
                t.text = StrTendril::from(value);
                false
            }
            Node::Comment(c) => {
                c.comment = StrTendril::from(value);
                false
            }
            Node::Element(_) | Node::Fragment => true,
            _ => false,
        };
        if parent {
            let kids: Vec<NodeId> = self.html.tree.get(id).unwrap().children().map(|c| c.id()).collect();
            for kid in kids {
                self.html.tree.get_mut(kid).unwrap().detach();
            }
            if !value.is_empty() {
                let t = self
                    .html
                    .tree
                    .get_mut(id)
                    .unwrap()
                    .append(Node::Text(Text {
                        text: StrTendril::from(value),
                    }))
                    .id();
                self.register(t);
            }
        }
        self.touch();
        Ok(())
    }

    /// Layout facts for element `i` from the probe. Paint-only style writes
    /// keep every box, so the probe stays for layout reads, and for
    /// `cascade` reads unless they touched `i` or an inherited property above it.
    pub fn probed(&mut self, i: usize, cascade: bool) -> Result<Option<P>, String> {
        let id = self.id(i)?;
        let stale = match &self.probe {
            None => true,
            Some(p) => {
                let covered = self.html.tree.get(id).is_some_and(|n| !n.ancestors().any(|a| p.emptied.contains(&a.id())));
                !covered || (!self.playing && (p.layout != self.layout || (cascade && self.repainted(id))))
            }
        };
        if stale {
            let emptied = if self.playing { HashSet::new() } else { self.out_of_scope(id) };
            let (compact, map) = compact(&self.html, &emptied);
            let probe = (self.prober)(&compact)?;
            let back: HashMap<NodeId, NodeId> = map.into_iter().map(|(old, new)| (new, old)).collect();
            let probe = probe
                .into_iter()
                .filter_map(|(new, p)| back.get(&new).map(|old| (*old, p)))
                .collect();
            self.probe = Some(Probing { layout: self.layout, emptied, map: probe });
            self.probes += 1;
            self.painted.clear();
        }
        Ok(self.probe.as_ref().unwrap().map.get(&id).cloned())
    }

    /// The DOM `nodeType` of node `i`.
    pub fn kind(&self, i: usize) -> Result<i32, String> {
        Ok(match self.node(i)?.value() {
            Node::Element(_) => 1,
            Node::Text(_) => 3,
            Node::ProcessingInstruction(_) => 7,
            Node::Comment(_) => 8,
            Node::Document => 9,
            Node::Doctype(_) => 10,
            Node::Fragment => 11,
        })
    }

    pub fn tag(&self, i: usize) -> Result<String, String> {
        Ok(self.element(i)?.value().name().to_string())
    }

    pub fn namespace(&self, i: usize) -> Result<String, String> {
        Ok(self.element(i)?.value().name.ns.to_string())
    }

    /// The parent of node `i`, or -1.
    pub fn parent(&self, i: usize) -> Result<i32, String> {
        let parent = self.node(i)?.parent().map(|p| p.id());
        Ok(parent.and_then(|p| self.index_of(p)).map_or(-1, |p| p as i32))
    }

    pub fn children(&mut self, i: usize) -> Result<Vec<usize>, String> {
        let ids: Vec<NodeId> = self.node(i)?.children().map(|c| c.id()).collect();
        Ok(ids.into_iter().map(|id| self.register(id)).collect())
    }

    pub fn is_connected(&self, i: usize) -> Result<bool, String> {
        Ok(self.connected(self.id(i)?))
    }

    /// Element `i`'s attributes as name, value, name, value.
    pub fn attributes(&self, i: usize) -> Result<Vec<String>, String> {
        Ok(self.element(i)?.value().attrs.iter().flat_map(|(n, v)| [attr_name(n), v.to_string()]).collect())
    }

    /// A detached element, or a fragment for `#fragment`.
    pub fn create(&mut self, tag: &str, namespace: &str) -> usize {
        let node = if tag == "#fragment" {
            Node::Fragment
        } else {
            Node::Element(Element::new(qual(tag, namespace), Vec::new()))
        };
        self.orphan(node)
    }

    /// A detached text or comment node.
    pub fn create_text(&mut self, value: &str, comment: bool) -> usize {
        let value = StrTendril::from(value);
        let node = if comment {
            Node::Comment(Comment { comment: value })
        } else {
            Node::Text(Text { text: value })
        };
        self.orphan(node)
    }

    /// Move `child` under `parent`, before node `before` unless that is negative.
    pub fn insert(&mut self, parent: usize, child: usize, before: i64) -> Result<(), String> {
        let (p, c) = (self.id(parent)?, self.id(child)?);
        if self.html.tree.get(p).unwrap().ancestors().any(|a| a.id() == c) || p == c {
            return Err("HierarchyRequestError: the new child contains the parent".into());
        }
        let detached = !self.connected(c);
        let generation = self.generation;
        self.html.tree.get_mut(c).unwrap().detach();
        if before < 0 {
            self.html.tree.get_mut(p).unwrap().append_id(c);
        } else {
            let b = self.id(before as usize)?;
            if self.html.tree.get(b).and_then(|n| n.parent()).map(|n| n.id()) != Some(p) {
                return Err("NotFoundError: the reference node is not a child of this node".into());
            }
            self.html.tree.get_mut(b).unwrap().insert_id_before(c);
        }
        self.touch();
        if detached && self.connected(c) {
            self.undo = Some((generation, c));
        }
        Ok(())
    }

    pub fn remove(&mut self, child: usize) -> Result<(), String> {
        let c = self.id(child)?;
        let undo = self.undo.take();
        self.html.tree.get_mut(c).unwrap().detach();
        self.touch();
        if let Some((generation, node)) = undo {
            if node == c {
                self.generation = generation;
            }
        }
        Ok(())
    }

    pub fn clone_node(&mut self, i: usize, deep: bool) -> Result<usize, String> {
        let id = self.id(i)?;
        let copy = if deep {
            self.deep_copy(id)
        } else {
            let value = self.html.tree.get(id).unwrap().value().clone();
            self.html.tree.orphan(value).id()
        };
        Ok(self.register(copy))
    }

    /// Replace the children of node `i` with parsed `markup`.
    pub fn set_inner(&mut self, i: usize, markup: &str) -> Result<(), String> {
        let id = self.id(i)?;
        let kids: Vec<NodeId> = self.html.tree.get(id).ok_or("unknown node")?.children().map(|c| c.id()).collect();
        for kid in kids {
            self.html.tree.get_mut(kid).unwrap().detach();
        }
        let fragment = Html::parse_fragment(markup);
        let holder = fragment.root_element();
        fn copy<P: Placement>(src: ego_tree::NodeRef<'_, Node>, dst: NodeId, d: &mut Doc<P>) {
            for child in src.children() {
                let new = d.html.tree.get_mut(dst).unwrap().append(child.value().clone()).id();
                d.register(new);
                copy(child, new, d);
            }
        }
        copy(*holder, id, self);
        self.touch();
        Ok(())
    }

    pub fn markup(&self, i: usize, outer: bool) -> Result<String, String> {
        let el = self.element(i)?;
        Ok(if outer { el.html() } else { el.inner_html() })
    }

    /// Descendants of node `scope` that match `selector` against the whole
    /// document, or only the first.
    pub fn select(&mut self, scope: usize, selector: &str, first: bool) -> Result<Vec<usize>, String> {
        let selector = self.selector(selector)?;
        let root = self.node(scope)?;
        let mut out = Vec::new();
        for node in root.descendants().skip(1) {
            let Some(el) = ElementRef::wrap(node) else { continue };
            if selector.matches(&el) {
                out.push(node.id());
                if first {
                    break;
                }
            }
        }
        Ok(out.into_iter().map(|id| self.register(id)).collect())
    }

    pub fn matches(&mut self, i: usize, selector: &str) -> Result<bool, String> {
        let selector = self.selector(selector)?;
        Ok(selector.matches(&self.element(i)?))
    }

    /// The tag of element `i`, empty for other nodes.
    pub fn tag_or_empty(&self, i: usize) -> String {
        self.element(i).map(|e| e.value().name().to_string()).unwrap_or_default()
    }
}

/// A copy of `html` holding only nodes reachable from the root, in document
/// order, with the map from old node ids to new ones. Nodes in `empty` keep
/// no children.
pub fn compact(html: &Html, empty: &HashSet<NodeId>) -> (Html, HashMap<NodeId, NodeId>) {
    let mut out = Html::new_document();
    out.quirks_mode = html.quirks_mode;
    let mut map = HashMap::new();
    let root = html.tree.root();
    map.insert(root.id(), out.tree.root().id());
    fn copy(src: ego_tree::NodeRef<'_, Node>, dst: NodeId, out: &mut Html, map: &mut HashMap<NodeId, NodeId>, empty: &HashSet<NodeId>) {
        if empty.contains(&src.id()) {
            return;
        }
        for child in src.children() {
            let id = out.tree.get_mut(dst).unwrap().append(child.value().clone()).id();
            map.insert(child.id(), id);
            copy(child, id, out, map, empty);
        }
    }
    copy(root, out.tree.root().id(), &mut out, &mut map, empty);
    (out, map)
}

fn attr_name(name: &QualName) -> String {
    match &name.prefix {
        Some(prefix) => format!("{prefix}:{}", name.local),
        None => name.local.to_string(),
    }
}

fn qual(tag: &str, namespace: &str) -> QualName {
    let ns = match namespace {
        "" | "http://www.w3.org/1999/xhtml" => ns!(html),
        "http://www.w3.org/2000/svg" => ns!(svg),
        "http://www.w3.org/1998/Math/MathML" => ns!(mathml),
        other => Namespace::from(other),
    };
    let tag = if ns == ns!(html) { tag.to_ascii_lowercase() } else { tag.to_string() };
    QualName::new(None, ns, LocalName::from(tag))
}

/// `name: value` pairs of an inline style.
pub fn declarations(text: &str) -> Vec<(String, String)> {
    let mut parts = Vec::new();
    let (mut depth, mut quote, mut start) = (0i32, None, 0);
    for (at, c) in text.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => depth -= 1,
            (None, ';') if depth == 0 => {
                parts.push(&text[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
        .into_iter()
        .filter_map(|part| {
            let (name, value) = part.split_once(':')?;
            let name = name.trim();
            let name = if name.starts_with("--") { name.to_string() } else { name.to_ascii_lowercase() };
            let mut value = value.trim();
            if value.len() >= 10 && value[value.len() - 10..].eq_ignore_ascii_case("!important") {
                value = value[..value.len() - 10].trim_end();
            }
            (!name.is_empty() && !value.is_empty()).then(|| (name, value.to_string()))
        })
        .collect()
}

/// For a paint-only change of inline style `before` to `after`, whether it
/// changed an inherited property. Any change that can move a box is `None`.
fn paint_change(before: &str, after: &str) -> Option<bool> {
    let (before, after) = (declarations(before), declarations(after));
    let value = |list: &[(String, String)], name: &str| list.iter().rev().find(|(n, _)| n == name).map(|(_, v)| v.clone());
    let mut inherited = false;
    for (name, _) in before.iter().chain(&after) {
        if value(&before, name) == value(&after, name) {
            continue;
        }
        if !paint_only(name) {
            return None;
        }
        inherited |= inherited_paint(name);
    }
    Some(inherited)
}

/// Properties that change pixels and never a box.
fn paint_only(name: &str) -> bool {
    matches!(
        name,
        "opacity" | "transform" | "translate" | "rotate" | "scale" | "transform-origin" | "transform-style"
            | "perspective" | "perspective-origin" | "backface-visibility" | "filter" | "backdrop-filter"
            | "visibility" | "color" | "box-shadow" | "text-shadow" | "clip-path" | "mix-blend-mode"
            | "isolation" | "z-index" | "will-change" | "fill" | "fill-opacity" | "stroke-opacity"
            | "stroke-dasharray" | "stroke-dashoffset" | "stop-color" | "stop-opacity" | "flood-color"
            | "flood-opacity" | "caret-color" | "cursor" | "pointer-events" | "text-decoration-color"
            | "-webkit-text-fill-color" | "-webkit-text-stroke-color" | "border-color"
    ) || ["background", "mask", "-webkit-mask", "outline"].iter().any(|p| name == *p || name.starts_with(&format!("{p}-")))
        || (name.starts_with("border-") && name.ends_with("-color"))
}

/// Paint-only properties that descendants inherit.
fn inherited_paint(name: &str) -> bool {
    matches!(
        name,
        "visibility" | "color" | "text-shadow" | "fill" | "fill-opacity" | "stroke-opacity" | "stroke-dasharray"
            | "stroke-dashoffset" | "caret-color" | "cursor" | "pointer-events" | "-webkit-text-fill-color"
            | "-webkit-text-stroke-color"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarations_split_on_top_level_semicolons() {
        let decls = declarations("color: red; background: url(\"a;b\") ; transform: translate(1px, 2px) !important;");
        assert_eq!(decls.len(), 3);
        assert_eq!(decls[1].1, "url(\"a;b\")");
        assert_eq!(decls[2].1, "translate(1px, 2px)");
    }
}
