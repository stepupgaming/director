//! The document page scripts see. Scraper's tree is mutated in place through
//! native functions; `dom.js` wraps node indexes in browser-shaped objects.
//! Layout and computed style come from a probe compile of the current tree.

use crate::Attr;
use boa_engine::{
    object::builtins::JsArray, Context, JsNativeError, JsResult, JsString, JsValue, NativeFunction,
};
use ego_tree::NodeId;
use html5ever::{ns, Attribute, LocalName, Namespace, QualName};
use scraper::node::{Comment, Element, Node, Text};
use scraper::{ElementRef, Html, Selector, StrTendril};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

/// What the probe compile knows about one element box.
#[derive(Debug, Clone, Default)]
pub(crate) struct Probed {
    /// Computed longhands as cascade text.
    pub css: BTreeMap<String, String>,
    /// x, y relative to the parent box, then width and height.
    pub rect: [f32; 4],
    /// Position relative to the canvas.
    pub abs: [f32; 2],
    /// `none`, `matrix(...)` or `matrix3d(...)` in px.
    pub transform: String,
    /// Transform origin in px.
    pub origin: String,
    pub font_size: f32,
}

pub(crate) type Probe = HashMap<NodeId, Probed>;

/// The latest probe, the layout generation it saw and the composition clips
/// it compiled empty.
struct Probing {
    layout: u64,
    emptied: HashSet<NodeId>,
    map: Probe,
}
pub(crate) type Prober = Box<dyn Fn(&Html) -> Result<Probe, String>>;

pub(crate) struct Doc {
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
    probe: Option<Probing>,
    prober: Prober,
    /// While the timeline plays, layout answers stay at the setup state.
    pub playing: bool,
    pub viewport: (f32, f32),
    /// The beat grid behind `director.beat`.
    beats: Rc<crate::beats::Beats>,
}

thread_local! {
    static DOC: RefCell<Option<Doc>> = const { RefCell::new(None) };
}

impl Doc {
    pub(crate) fn new(html: Html, prober: Prober, viewport: (f32, f32), beats: Rc<crate::beats::Beats>) -> Self {
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
            prober,
            playing: false,
            viewport,
            beats,
        };
        let root = doc.html.tree.root().id();
        doc.register(root);
        let all: Vec<NodeId> = doc.html.tree.root().descendants().map(|n| n.id()).collect();
        for id in all {
            doc.register(id);
        }
        doc
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
            .filter(|n| {
                let position = last.map.get(&n.id()).and_then(|p| p.css.get("position"));
                position.is_some_and(|p| p == "absolute" || p == "fixed")
            })
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

    pub(crate) fn index_of(&self, id: NodeId) -> Option<usize> {
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
    pub(crate) fn attr(&self, i: usize, name: &str) -> Result<Option<String>, String> {
        Ok(self
            .element(i)?
            .value()
            .attrs
            .iter()
            .find(|(n, _)| attr_name(n) == name)
            .map(|(_, v)| v.to_string()))
    }

    /// Set or remove attribute `name` on element `i`.
    pub(crate) fn write_attr(&mut self, i: usize, name: &str, value: Option<String>) -> Result<(), String> {
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
    pub(crate) fn label(&self, i: usize) -> String {
        match self.element(i) {
            Ok(el) => match el.value().attr_text("id") {
                Some(id) => format!("#{id}"),
                None => format!("<{}>", el.value().name()),
            },
            Err(_) => format!("node {i}"),
        }
    }

    /// The text content of node `i`.
    pub(crate) fn text(&self, i: usize) -> Result<String, String> {
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
    pub(crate) fn text_owner(&self, i: usize) -> Result<Option<usize>, String> {
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
    pub(crate) fn svg_owner(&self, i: usize) -> Result<Option<usize>, String> {
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
    pub(crate) fn is_svg(&self, i: usize) -> Result<bool, String> {
        Ok(self.node(i)?.value().as_element().is_some_and(|el| el.name.ns == ns!(svg) && &*el.name.local == "svg"))
    }

    /// Append an SVG element `tag` with `attrs` as the last child of `i`.
    pub(crate) fn append_svg(&mut self, i: usize, tag: &str, attrs: Vec<(String, String)>) -> Result<(), String> {
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
    pub(crate) fn write_text(&mut self, i: usize, value: &str) -> Result<(), String> {
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

    /// Layout facts for element `i` from the probe compile. Paint-only style
    /// writes keep every box, so the probe stays for layout reads, and for
    /// `cascade` reads unless they touched `i` or an inherited property above it.
    pub(crate) fn probed(&mut self, i: usize, cascade: bool) -> Result<Option<Probed>, String> {
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
            let probe: Probe = probe
                .into_iter()
                .filter_map(|(new, p)| back.get(&new).map(|old| (*old, p)))
                .collect();
            self.probe = Some(Probing { layout: self.layout, emptied, map: probe });
            self.painted.clear();
        }
        Ok(self.probe.as_ref().unwrap().map.get(&id).cloned())
    }
}

/// A copy of `html` holding only nodes reachable from the root, in document
/// order, with the map from old node ids to new ones. Nodes in `empty` keep
/// no children.
pub(crate) fn compact(html: &Html, empty: &HashSet<NodeId>) -> (Html, HashMap<NodeId, NodeId>) {
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

/// Install `doc` for the natives, run `f`, and hand the document back.
pub(crate) fn with_installed<R>(doc: Doc, f: impl FnOnce() -> R) -> (R, Doc) {
    DOC.with(|d| *d.borrow_mut() = Some(doc));
    let out = f();
    let doc = DOC.with(|d| d.borrow_mut().take()).expect("installed document");
    (out, doc)
}

pub(crate) fn with_doc<R>(f: impl FnOnce(&mut Doc) -> Result<R, String>) -> JsResult<R> {
    DOC.with(|d| {
        let mut d = d.borrow_mut();
        let doc = d.as_mut().expect("installed document");
        f(doc)
    })
    .map_err(|e| JsNativeError::error().with_message(e).into())
}

fn arg_index(args: &[JsValue], n: usize, cx: &mut Context) -> JsResult<usize> {
    let v = args.get(n).cloned().unwrap_or_default().to_i32(cx)?;
    usize::try_from(v).map_err(|_| JsNativeError::range().with_message("negative node index").into())
}
fn arg_int(args: &[JsValue], n: usize, cx: &mut Context) -> JsResult<i64> {
    Ok(args.get(n).cloned().unwrap_or_default().to_i32(cx)? as i64)
}
fn arg_text(args: &[JsValue], n: usize, cx: &mut Context) -> JsResult<String> {
    Ok(args
        .get(n)
        .cloned()
        .unwrap_or_default()
        .to_string(cx)?
        .to_std_string_escaped())
}
fn arg_opt(args: &[JsValue], n: usize, cx: &mut Context) -> JsResult<Option<String>> {
    match args.get(n) {
        None => Ok(None),
        Some(v) if v.is_null_or_undefined() => Ok(None),
        Some(v) => Ok(Some(v.to_string(cx)?.to_std_string_escaped())),
    }
}
fn text(s: &str) -> JsValue {
    JsValue::from(JsString::from(s))
}
fn indexes(list: Vec<usize>, cx: &mut Context) -> JsValue {
    JsArray::from_iter(list.into_iter().map(|i| JsValue::from(i as i32)), cx).into()
}
fn strings(list: Vec<String>, cx: &mut Context) -> JsValue {
    JsArray::from_iter(list.iter().map(|s| text(s)), cx).into()
}
fn numbers(list: &[f32], cx: &mut Context) -> JsValue {
    JsArray::from_iter(list.iter().map(|n| JsValue::from(*n as f64)), cx).into()
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

fn kind(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let i = arg_index(args, 0, cx)?;
    with_doc(|d| {
        Ok(JsValue::from(match d.node(i)?.value() {
            Node::Element(_) => 1,
            Node::Text(_) => 3,
            Node::ProcessingInstruction(_) => 7,
            Node::Comment(_) => 8,
            Node::Document => 9,
            Node::Doctype(_) => 10,
            Node::Fragment => 11,
        }))
    })
}

fn tag(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let i = arg_index(args, 0, cx)?;
    with_doc(|d| Ok(text(d.element(i)?.value().name())))
}

fn namespace(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let i = arg_index(args, 0, cx)?;
    with_doc(|d| Ok(text(&d.element(i)?.value().name.ns)))
}

fn parent(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let i = arg_index(args, 0, cx)?;
    with_doc(|d| {
        let parent = d.node(i)?.parent().map(|p| p.id());
        Ok(JsValue::from(parent.and_then(|p| d.index_of(p)).map_or(-1, |p| p as i32)))
    })
}

fn children(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let i = arg_index(args, 0, cx)?;
    let list = with_doc(|d| {
        let ids: Vec<NodeId> = d.node(i)?.children().map(|c| c.id()).collect();
        Ok(ids.into_iter().map(|id| d.register(id)).collect::<Vec<_>>())
    })?;
    Ok(indexes(list, cx))
}

fn connected(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let i = arg_index(args, 0, cx)?;
    with_doc(|d| Ok(JsValue::from(d.connected(d.id(i)?))))
}

fn attribute(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let i = arg_index(args, 0, cx)?;
    let name = arg_text(args, 1, cx)?;
    with_doc(|d| Ok(d.attr(i, &name)?.map_or(JsValue::null(), |v| text(&v))))
}

fn attr_name(name: &QualName) -> String {
    match &name.prefix {
        Some(prefix) => format!("{prefix}:{}", name.local),
        None => name.local.to_string(),
    }
}

fn attributes(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let i = arg_index(args, 0, cx)?;
    let flat = with_doc(|d| {
        let el = d.element(i)?;
        Ok(el
            .value()
            .attrs
            .iter()
            .flat_map(|(n, v)| [attr_name(n), v.to_string()])
            .collect::<Vec<_>>())
    })?;
    Ok(strings(flat, cx))
}

fn set_attribute(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let i = arg_index(args, 0, cx)?;
    let name = arg_text(args, 1, cx)?;
    let value = arg_opt(args, 2, cx)?;
    with_doc(|d| d.write_attr(i, &name, value).map(|_| JsValue::undefined()))
}

fn text_content(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let i = arg_index(args, 0, cx)?;
    with_doc(|d| Ok(text(&d.text(i)?)))
}

fn set_text(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let i = arg_index(args, 0, cx)?;
    let value = arg_text(args, 1, cx)?;
    with_doc(|d| d.write_text(i, &value).map(|_| JsValue::undefined()))
}

fn create(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let tag = arg_text(args, 0, cx)?;
    let ns = arg_text(args, 1, cx)?;
    with_doc(|d| {
        let node = if tag == "#fragment" {
            Node::Fragment
        } else {
            Node::Element(Element::new(qual(&tag, &ns), Vec::new()))
        };
        Ok(JsValue::from(d.orphan(node) as i32))
    })
}

fn create_text(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let value = arg_text(args, 0, cx)?;
    let comment = args.get(1).is_some_and(JsValue::to_boolean);
    with_doc(|d| {
        let value = StrTendril::from(value.as_str());
        let node = if comment {
            Node::Comment(Comment { comment: value })
        } else {
            Node::Text(Text { text: value })
        };
        Ok(JsValue::from(d.orphan(node) as i32))
    })
}

fn insert(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let parent = arg_index(args, 0, cx)?;
    let child = arg_index(args, 1, cx)?;
    let before = arg_int(args, 2, cx)?;
    with_doc(|d| {
        let (p, c) = (d.id(parent)?, d.id(child)?);
        if d.html.tree.get(p).unwrap().ancestors().any(|a| a.id() == c) || p == c {
            return Err("HierarchyRequestError: the new child contains the parent".into());
        }
        let detached = !d.connected(c);
        let generation = d.generation;
        d.html.tree.get_mut(c).unwrap().detach();
        if before < 0 {
            d.html.tree.get_mut(p).unwrap().append_id(c);
        } else {
            let b = d.id(before as usize)?;
            if d.html.tree.get(b).and_then(|n| n.parent()).map(|n| n.id()) != Some(p) {
                return Err("NotFoundError: the reference node is not a child of this node".into());
            }
            d.html.tree.get_mut(b).unwrap().insert_id_before(c);
        }
        d.touch();
        if detached && d.connected(c) {
            d.undo = Some((generation, c));
        }
        Ok(JsValue::undefined())
    })
}

fn remove(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let child = arg_index(args, 0, cx)?;
    with_doc(|d| {
        let c = d.id(child)?;
        let undo = d.undo.take();
        d.html.tree.get_mut(c).unwrap().detach();
        d.touch();
        if let Some((generation, node)) = undo {
            if node == c {
                d.generation = generation;
            }
        }
        Ok(JsValue::undefined())
    })
}

fn clone(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let i = arg_index(args, 0, cx)?;
    let deep = args.get(1).is_some_and(JsValue::to_boolean);
    with_doc(|d| {
        let id = d.id(i)?;
        let copy = if deep {
            d.deep_copy(id)
        } else {
            let value = d.html.tree.get(id).unwrap().value().clone();
            d.html.tree.orphan(value).id()
        };
        Ok(JsValue::from(d.register(copy) as i32))
    })
}

fn set_inner(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let i = arg_index(args, 0, cx)?;
    let markup = arg_text(args, 1, cx)?;
    with_doc(|d| {
        let id = d.id(i)?;
        let kids: Vec<NodeId> = d.html.tree.get(id).ok_or("unknown node")?.children().map(|c| c.id()).collect();
        for kid in kids {
            d.html.tree.get_mut(kid).unwrap().detach();
        }
        let fragment = Html::parse_fragment(&markup);
        let holder = fragment.root_element();
        fn copy(src: ego_tree::NodeRef<'_, Node>, dst: NodeId, d: &mut Doc) {
            for child in src.children() {
                let new = d.html.tree.get_mut(dst).unwrap().append(child.value().clone()).id();
                d.register(new);
                copy(child, new, d);
            }
        }
        copy(*holder, id, d);
        d.touch();
        Ok(JsValue::undefined())
    })
}

fn markup(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let i = arg_index(args, 0, cx)?;
    let outer = args.get(1).is_some_and(JsValue::to_boolean);
    with_doc(|d| {
        let el = d.element(i)?;
        Ok(text(&if outer { el.html() } else { el.inner_html() }))
    })
}

fn select(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let scope = arg_index(args, 0, cx)?;
    let selector = arg_text(args, 1, cx)?;
    let first = args.get(2).is_some_and(JsValue::to_boolean);
    let found = with_doc(|d| {
        let selector = d.selector(&selector)?;
        let root = d.node(scope)?;
        let mut out = Vec::new();
        // Descendants of the scope, matched against the whole document.
        for node in root.descendants().skip(1) {
            let Some(el) = ElementRef::wrap(node) else { continue };
            if selector.matches(&el) {
                out.push(node.id());
                if first {
                    break;
                }
            }
        }
        Ok(out.into_iter().map(|id| d.register(id)).collect::<Vec<_>>())
    })?;
    Ok(indexes(found, cx))
}

fn matches(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let i = arg_index(args, 0, cx)?;
    let selector = arg_text(args, 1, cx)?;
    with_doc(|d| {
        let selector = d.selector(&selector)?;
        Ok(JsValue::from(selector.matches(&d.element(i)?)))
    })
}

fn layout(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let i = arg_index(args, 0, cx)?;
    let found = with_doc(|d| {
        Ok(d.probed(i, false)?.map(|p| {
            let mut v = p.rect.to_vec();
            v.extend(p.abs);
            v.push(p.font_size);
            v
        }))
    })?;
    Ok(match found {
        Some(v) => numbers(&v, cx),
        None => JsValue::null(),
    })
}

fn viewport(_: &JsValue, _: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let (w, h) = with_doc(|d| Ok(d.viewport))?;
    Ok(numbers(&[w, h], cx))
}

fn computed(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let i = arg_index(args, 0, cx)?;
    let prop = arg_text(args, 1, cx)?;
    with_doc(|d| {
        let tag = d.element(i).map(|e| e.value().name().to_string()).unwrap_or_default();
        let probed = d.probed(i, true)?;
        Ok(text(&computed_value(probed.as_ref(), &tag, &prop)))
    })
}

fn px(v: f32) -> String {
    format!("{}px", round(v))
}
/// Six significant digits, as browsers serialize a `matrix3d`, so GSAP reads
/// a small perspective term back at full distance.
fn significant(v: f32) -> f64 {
    let v = v as f64;
    if v == 0.0 {
        return 0.0;
    }
    let scale = 10f64.powi(5 - v.abs().log10().floor() as i32);
    let r = (v * scale).round() / scale;
    if r == 0.0 { 0.0 } else { r }
}
fn round(v: f32) -> f32 {
    let r = (v * 1e4).round() / 1e4;
    if r == 0.0 {
        0.0
    } else {
        r
    }
}

/// Properties whose computed zero carries no unit.
const UNITLESS: &[&str] = &[
    "opacity", "z-index", "order", "flex-grow", "flex-shrink", "font-weight", "line-height", "zoom",
    "fill-opacity", "stroke-opacity", "stop-opacity", "flood-opacity", "orphans", "widows", "tab-size",
    "column-count", "stroke-miterlimit", "scale", "rotate", "aspect-ratio",
];

/// `getComputedStyle` text for `prop` (kebab case).
/// For a paint-only change of inline style `before` to `after`, whether it
/// changed an inherited property. Any change that can move a box is `None`.
fn paint_change(before: &str, after: &str) -> Option<bool> {
    let (before, after) = (super::declarations(before), super::declarations(after));
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

fn computed_value(probed: Option<&Probed>, tag: &str, prop: &str) -> String {
    if let Some(p) = probed {
        match prop {
            "width" => return px(p.rect[2]),
            "height" => return px(p.rect[3]),
            "transform" => return p.transform.clone(),
            "transform-origin" => return p.origin.clone(),
            "font-size" => return px(p.font_size),
            _ => {}
        }
        let current = || {
            p.css
                .get("color")
                .and_then(|c| crate::style::color(c, director_core::types::Color::BLACK).ok())
                .unwrap_or(director_core::types::Color::BLACK)
        };
        // Border colors default to currentColor, and `border-color` lists
        // its sides once each unless they agree.
        let side_color = |side: &str| {
            p.css
                .get(&format!("border-{side}-color"))
                .and_then(|c| crate::style::color(c, current()).ok())
                .map_or_else(|| rgb(current()), rgb)
        };
        if prop == "border-color" {
            let sides = ["top", "right", "bottom", "left"].map(side_color);
            return if sides.iter().all(|s| *s == sides[0]) { sides[0].clone() } else { sides.join(" ") };
        }
        if let Some(side) = prop.strip_prefix("border-").and_then(|s| s.strip_suffix("-color")) {
            if ["top", "right", "bottom", "left"].contains(&side) {
                return side_color(side);
            }
        }
        // Padding and margin sides read their longhand, else the shorthand, in
        // px; the shorthand lists one value when the sides agree.
        let m = crate::style::Metrics { width: 0.0, height: 0.0, font_size: p.font_size };
        let box_side = |property: &str, side: usize| {
            let names = ["top", "right", "bottom", "left"];
            let text = p.css.get(&format!("{property}-{}", names[side])).cloned().or_else(|| {
                let parts = crate::css::split(p.css.get(property)?.trim(), false);
                let pick = [[0, 0, 0, 0], [0, 1, 0, 1], [0, 1, 2, 1], [0, 1, 2, 3]].get(parts.len().checked_sub(1)?)?;
                parts.get(pick[side]).cloned()
            });
            match text {
                Some(t) if t.trim().ends_with('%') || t.trim() == "auto" => t,
                Some(t) => crate::style::px(&t, m).map_or(t, px),
                None => "0px".to_string(),
            }
        };
        if prop == "padding" || prop == "margin" {
            let sides = [0, 1, 2, 3].map(|side| box_side(prop, side));
            return if sides.iter().all(|s| *s == sides[0]) { sides[0].clone() } else { sides.join(" ") };
        }
        if let Some((property, side)) = crate::animation::box_side(prop) {
            return box_side(property, side);
        }
        // Browsers compute a line height to px unless it is `normal`.
        if prop == "line-height" {
            if let Some(v) = p.css.get(prop).filter(|v| v.as_str() != "normal") {
                return match v.trim().parse::<f32>() {
                    Ok(factor) => px(factor * p.font_size),
                    Err(_) => crate::style::px(v, m).map_or_else(|_| v.clone(), px),
                };
            }
        }
        // The stroke longhands read from the `-webkit-text-stroke` shorthand
        // too; the color defaults to the text color and the width to 0.
        if prop == "-webkit-text-stroke-color" || prop == "-webkit-text-stroke-width" {
            let shorthand = p.css.get("-webkit-text-stroke").map(|v| crate::css::split(v, false)).unwrap_or_default();
            return if prop.ends_with("color") {
                let text = p.css.get(prop).cloned().or_else(|| shorthand.get(1).cloned());
                rgb(text.and_then(|t| crate::style::color(&t, current()).ok()).unwrap_or_else(current))
            } else {
                let text = p.css.get(prop).cloned().or_else(|| shorthand.first().cloned());
                px(text.and_then(|t| crate::style::px(&t, m).ok()).unwrap_or(0.0))
            };
        }
        // Each layer's x and y longhands pair into one position.
        if prop == "background-position" && !p.css.contains_key(prop) {
            let axis = |name: &str, fallback: &str| {
                p.css.get(name).map_or_else(|| vec![fallback.to_string()], |v| crate::css::split(v, true))
            };
            let (x, y) = (axis("background-position-x", "0%"), axis("background-position-y", "0%"));
            let layers = x.len().max(y.len());
            return (0..layers).map(|i| format!("{} {}", x[i % x.len()], y[i % y.len()])).collect::<Vec<_>>().join(", ");
        }
        if let Some(v) = p.css.get(prop) {
            if prop.ends_with("color") || matches!(prop, "fill" | "stroke") {
                if let Ok(c) = crate::style::color(v, current()) {
                    return rgb(c);
                }
            }
            // Browsers list each shadow as its color, four lengths, then `inset`.
            if prop == "box-shadow" {
                let m = crate::style::Metrics { width: 0.0, height: 0.0, font_size: p.font_size };
                if let Ok(list) = crate::style::parse_box_shadows(v, m, current()) {
                    if !list.is_empty() {
                        let shadow = |s: &crate::style::Shadow| {
                            let inset = if s.inset { " inset" } else { "" };
                            format!("{} {} {} {} {}{inset}", rgb(s.color), px(s.x), px(s.y), px(s.blur * 2.0), px(s.spread))
                        };
                        return list.iter().map(shadow).collect::<Vec<_>>().join(", ");
                    }
                }
            }
            // Browsers list each text shadow as its color and three lengths.
            if prop == "text-shadow" {
                let m = crate::style::Metrics { width: 0.0, height: 0.0, font_size: p.font_size };
                if let Ok(list) = crate::style::parse_text_shadows(v, m, current()) {
                    if !list.is_empty() {
                        let shadow = |s: &director_core::element::TextShadow| {
                            format!("{} {} {} {}", rgb(s.color), px(s.offset.0), px(s.offset.1), px(s.blur))
                        };
                        return list.iter().map(shadow).collect::<Vec<_>>().join(", ");
                    }
                }
            }
            // Browsers compute font-stretch keywords to percentages.
            if prop == "font-stretch" {
                if let Ok(percent) = crate::style::stretch_percent(v) {
                    return format!("{percent}%");
                }
            }
            // Browsers compute letter and word spacing to px.
            if matches!(prop, "letter-spacing" | "word-spacing") && v != "normal" {
                let m = crate::style::Metrics { width: 0.0, height: 0.0, font_size: p.font_size };
                if let Ok(spacing) = crate::style::px(v, m) {
                    return px(spacing);
                }
            }
            // Browsers serialize a zero length with its unit.
            if v == "0" && !UNITLESS.contains(&prop) {
                return "0px".into();
            }
            return v.clone();
        }
    }
    initial(tag, prop).to_string()
}

pub(crate) fn rgb(c: director_core::types::Color) -> String {
    let b = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    if c.a >= 1.0 {
        format!("rgb({}, {}, {})", b(c.r), b(c.g), b(c.b))
    } else {
        format!("rgba({}, {}, {}, {})", b(c.r), b(c.g), b(c.b), round(c.a))
    }
}

/// Initial values for longhands the cascade leaves unset.
fn initial(tag: &str, prop: &str) -> &'static str {
    match prop {
        "opacity" => "1",
        "visibility" => "visible",
        "display" => match tag {
            "span" | "b" | "strong" | "em" | "i" | "small" | "code" | "a" | "img" | "svg" => "inline",
            "head" | "script" | "style" | "link" | "meta" | "title" | "template" => "none",
            _ => "block",
        },
        "position" => "static",
        "z-index" | "left" | "top" | "right" | "bottom" => "auto",
        "filter" | "box-shadow" | "text-shadow" | "clip-path" | "mask-image" | "background-image" => "none",
        "letter-spacing" | "word-spacing" | "line-height" | "font-weight" | "font-style" => "normal",
        "font-stretch" => "100%",
        "font-variation-settings" => "normal",
        "color" => "rgb(0, 0, 0)",
        "background-color" => "rgba(0, 0, 0, 0)",
        p if p.starts_with("border") && p.ends_with("color") => "rgb(0, 0, 0)",
        "background-position" => "0% 0%",
        "background-size" => "auto",
        "transform" | "perspective" => "none",
        "transform-origin" => "0px 0px",
        "transform-style" => "flat",
        "stroke-dasharray" | "stroke" => "none",
        "stroke-dashoffset" => "0px",
        "stroke-width" => "1px",
        "fill" => "rgb(0, 0, 0)",
        "fill-opacity" | "stroke-opacity" => "1",
        "backface-visibility" => "visible",
        p if p.starts_with("margin") || p.starts_with("padding") || p.ends_with("radius") || p.ends_with("width") => "0px",
        _ => "",
    }
}

fn matrix(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let i = arg_index(args, 0, cx)?;
    let value = arg_text(args, 1, cx)?;
    with_doc(|d| {
        let size = d.probed(i, false)?.map_or((0.0, 0.0), |p| (p.rect[2], p.rect[3]));
        Ok(text(&matrix_text(&value, size)))
    })
}

/// `__dPath(d)` is the length of path data `d` over every contour.
/// `__dPath(d, at)` is the `[x, y]` point at distance `at` along it.
fn path(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let d = arg_text(args, 0, cx)?;
    let at = match args.get(1) {
        Some(v) if !v.is_undefined() => Some(v.to_number(cx)? as f32),
        _ => None,
    };
    let Some(path) = skia_safe::Path::from_svg(&d) else {
        return Err(JsNativeError::error().with_message(format!("SVG path data '{d}' does not parse")).into());
    };
    // A fine resolution keeps curve lengths within a thousandth of a unit.
    let contours: Vec<_> = skia_safe::ContourMeasureIter::new(&path, false, 64.0).collect();
    let total: f32 = contours.iter().map(|c| c.length()).sum();
    let Some(at) = at else {
        return Ok(JsValue::from(total as f64));
    };
    let mut left = at.clamp(0.0, total);
    for (n, contour) in contours.iter().enumerate() {
        if left <= contour.length() || n + 1 == contours.len() {
            let point = contour.pos_tan(left.min(contour.length())).map_or((0.0, 0.0), |(p, _)| (p.x, p.y));
            return Ok(numbers(&[point.0, point.1], cx));
        }
        left -= contour.length();
    }
    Ok(numbers(&[0.0, 0.0], cx))
}

/// The computed matrix text of a CSS transform at a box size.
pub(crate) fn matrix_text(value: &str, size: (f32, f32)) -> String {
    use lightningcss::{properties::transform::TransformList, traits::Parse};
    let Ok(list) = TransformList::parse_string(value) else {
        return "none".into();
    };
    if list.0.is_empty() {
        return "none".into();
    }
    let metrics = crate::style::Metrics {
        width: size.0,
        height: size.1,
        font_size: 16.0,
    };
    match crate::style::canvas_transform(&list, metrics, size) {
        crate::style::CanvasTransform::Affine(m) => {
            let v = [m.scale_x(), m.skew_y(), m.skew_x(), m.scale_y(), m.translate_x(), m.translate_y()];
            format!(
                "matrix({})",
                v.iter().map(|n| round(*n).to_string()).collect::<Vec<_>>().join(", ")
            )
        }
        crate::style::CanvasTransform::Projective(m) => {
            let mut v = Vec::with_capacity(16);
            for c in 0..4 {
                for r in 0..4 {
                    v.push(significant(m.rc(r, c)).to_string());
                }
            }
            format!("matrix3d({})", v.join(", "))
        }
    }
}

/// Register the DOM natives as globals.
pub(crate) fn register(cx: &mut Context) -> JsResult<()> {
    type Native = fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>;
    let natives: [(&str, usize, Native); 30] = [
        ("__dKind", 1, kind),
        ("__dTag", 1, tag),
        ("__dNs", 1, namespace),
        ("__dParent", 1, parent),
        ("__dChildren", 1, children),
        ("__dConnected", 1, connected),
        ("__dAttr", 2, attribute),
        ("__dAttrs", 1, attributes),
        ("__dSetAttr", 3, set_attribute),
        ("__dText", 1, text_content),
        ("__dSetText", 2, set_text),
        ("__dCreate", 2, create),
        ("__dCreateText", 2, create_text),
        ("__dInsert", 3, insert),
        ("__dRemove", 1, remove),
        ("__dClone", 2, clone),
        ("__dSetInner", 2, set_inner),
        ("__dMarkup", 2, markup),
        ("__dSelect", 3, select),
        ("__dMatches", 2, matches),
        ("__dLayout", 1, layout),
        ("__dViewport", 0, viewport),
        ("__dComputed", 2, computed),
        ("__dMatrix", 2, matrix),
        ("__dGeneration", 0, generation),
        ("__dMedia", 1, media),
        ("__dPath", 2, path),
        ("__dBeat", 1, beat),
        ("__dBeatAt", 1, beat_at),
        ("__dBeatGrid", 0, beat_grid),
    ];
    for (name, length, f) in natives {
        cx.register_global_callable(JsString::from(name), length, NativeFunction::from_fn_ptr(f))?;
    }
    Ok(())
}

fn media(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let query = arg_text(args, 0, cx)?;
    with_doc(|d| Ok(JsValue::from(crate::css::media_text_matches(&query, d.viewport.0, d.viewport.1)?)))
}

/// Seconds at a beat position on the composition's beat grid.
fn beat(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let position = args.first().cloned().unwrap_or_default().to_number(cx)?;
    with_doc(|d| Ok(JsValue::from(d.beats.grid()?.time(position))))
}

/// The beat position at a time in seconds.
fn beat_at(_: &JsValue, args: &[JsValue], cx: &mut Context) -> JsResult<JsValue> {
    let time = args.first().cloned().unwrap_or_default().to_number(cx)?;
    with_doc(|d| Ok(JsValue::from(d.beats.grid()?.position(time))))
}

/// The grid's tempo, bar length and drop as JSON.
fn beat_grid(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_doc(|d| {
        let grid = d.beats.grid()?;
        let json = serde_json::json!({ "bpm": grid.bpm, "beatsPerBar": grid.beats_per_bar, "drop": grid.drop });
        Ok(JsValue::from(JsString::from(json.to_string())))
    })
}

fn generation(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_doc(|d| Ok(JsValue::from(d.generation as f64)))
}
