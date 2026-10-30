//! The `__d*` natives `dom.js` calls, free of any JavaScript engine. Tree
//! natives run on the installed document; layout, style, path, media and
//! beat natives ask the compiler.

use crate::ask;
use director_dom::protocol::{Ask, Placed, Reply};
use director_dom::{Doc, Placement};
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;

/// A probed box and its arena index in the probe the compiler holds.
#[derive(Debug, Clone)]
pub(crate) struct Spot {
    pub node: u32,
    pub placed: Placed,
}

impl Placement for Spot {
    fn positioned(&self) -> bool {
        self.placed.positioned
    }
}

pub(crate) type Page = Doc<Spot>;

/// A `getComputedStyle` question: probe count, node, tag and property.
type Question = (u64, Option<u32>, String, String);

thread_local! {
    static DOC: RefCell<Option<Page>> = const { RefCell::new(None) };
    static COMPUTED: RefCell<HashMap<Question, String>> = RefCell::new(HashMap::new());
}

/// Install `doc` for the natives, run `f`, and hand the document back.
pub(crate) fn with_installed<R>(doc: Page, f: impl FnOnce() -> R) -> (R, Page) {
    DOC.with(|d| *d.borrow_mut() = Some(doc));
    let out = f();
    let doc = DOC.with(|d| d.borrow_mut().take()).expect("installed document");
    (out, doc)
}

pub(crate) fn with_doc<R>(f: impl FnOnce(&mut Page) -> Result<R, String>) -> Result<R, Fault> {
    DOC.with(|d| {
        let mut d = d.borrow_mut();
        f(d.as_mut().expect("installed document"))
    })
    .map_err(Fault::Error)
}

/// Why a native throws.
pub(crate) enum Fault {
    Error(String),
    Range(String),
    /// An exception the engine raised while converting an argument.
    Thrown(Box<dyn Any>),
}

/// A native's result.
pub(crate) enum Out {
    Undefined,
    Null,
    Bool(bool),
    Int(i32),
    Number(f64),
    Text(String),
    Indexes(Vec<usize>),
    Strings(Vec<String>),
    Numbers(Vec<f32>),
}

/// Native arguments, converted as the engine converts JavaScript values.
pub(crate) trait Args {
    /// `ToInt32` of argument `n`, which must not be negative.
    fn int(&mut self, n: usize) -> Result<i64, Fault>;
    /// `ToString` of argument `n`.
    fn text(&mut self, n: usize) -> Result<String, Fault>;
    /// `ToString` of argument `n`, or `None` for null and undefined.
    fn opt(&mut self, n: usize) -> Result<Option<String>, Fault>;
    /// `ToBoolean` of argument `n`.
    fn flag(&mut self, n: usize) -> bool;
    /// `ToNumber` of argument `n`.
    fn number(&mut self, n: usize) -> Result<f64, Fault>;
    /// Whether argument `n` is present and not undefined.
    fn given(&self, n: usize) -> bool;

    fn index(&mut self, n: usize) -> Result<usize, Fault> {
        usize::try_from(self.int(n)?).map_err(|_| Fault::Range("negative node index".into()))
    }
}

pub(crate) type Native = fn(&mut dyn Args) -> Result<Out, Fault>;

pub(crate) const NATIVES: [(&str, usize, Native); 30] = [
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

fn kind(a: &mut dyn Args) -> Result<Out, Fault> {
    let i = a.index(0)?;
    with_doc(|d| d.kind(i)).map(Out::Int)
}

fn tag(a: &mut dyn Args) -> Result<Out, Fault> {
    let i = a.index(0)?;
    with_doc(|d| d.tag(i)).map(Out::Text)
}

fn namespace(a: &mut dyn Args) -> Result<Out, Fault> {
    let i = a.index(0)?;
    with_doc(|d| d.namespace(i)).map(Out::Text)
}

fn parent(a: &mut dyn Args) -> Result<Out, Fault> {
    let i = a.index(0)?;
    with_doc(|d| d.parent(i)).map(Out::Int)
}

fn children(a: &mut dyn Args) -> Result<Out, Fault> {
    let i = a.index(0)?;
    with_doc(|d| d.children(i)).map(Out::Indexes)
}

fn connected(a: &mut dyn Args) -> Result<Out, Fault> {
    let i = a.index(0)?;
    with_doc(|d| d.is_connected(i)).map(Out::Bool)
}

fn attribute(a: &mut dyn Args) -> Result<Out, Fault> {
    let i = a.index(0)?;
    let name = a.text(1)?;
    with_doc(|d| d.attr(i, &name)).map(|v| v.map_or(Out::Null, Out::Text))
}

fn attributes(a: &mut dyn Args) -> Result<Out, Fault> {
    let i = a.index(0)?;
    with_doc(|d| d.attributes(i)).map(Out::Strings)
}

fn set_attribute(a: &mut dyn Args) -> Result<Out, Fault> {
    let i = a.index(0)?;
    let name = a.text(1)?;
    let value = a.opt(2)?;
    with_doc(|d| d.write_attr(i, &name, value)).map(|_| Out::Undefined)
}

fn text_content(a: &mut dyn Args) -> Result<Out, Fault> {
    let i = a.index(0)?;
    with_doc(|d| d.text(i)).map(Out::Text)
}

fn set_text(a: &mut dyn Args) -> Result<Out, Fault> {
    let i = a.index(0)?;
    let value = a.text(1)?;
    with_doc(|d| d.write_text(i, &value)).map(|_| Out::Undefined)
}

fn create(a: &mut dyn Args) -> Result<Out, Fault> {
    let tag = a.text(0)?;
    let ns = a.text(1)?;
    with_doc(|d| Ok(d.create(&tag, &ns))).map(|i| Out::Int(i as i32))
}

fn create_text(a: &mut dyn Args) -> Result<Out, Fault> {
    let value = a.text(0)?;
    let comment = a.flag(1);
    with_doc(|d| Ok(d.create_text(&value, comment))).map(|i| Out::Int(i as i32))
}

fn insert(a: &mut dyn Args) -> Result<Out, Fault> {
    let parent = a.index(0)?;
    let child = a.index(1)?;
    let before = a.int(2)?;
    with_doc(|d| d.insert(parent, child, before)).map(|_| Out::Undefined)
}

fn remove(a: &mut dyn Args) -> Result<Out, Fault> {
    let child = a.index(0)?;
    with_doc(|d| d.remove(child)).map(|_| Out::Undefined)
}

fn clone(a: &mut dyn Args) -> Result<Out, Fault> {
    let i = a.index(0)?;
    let deep = a.flag(1);
    with_doc(|d| d.clone_node(i, deep)).map(|i| Out::Int(i as i32))
}

fn set_inner(a: &mut dyn Args) -> Result<Out, Fault> {
    let i = a.index(0)?;
    let markup = a.text(1)?;
    with_doc(|d| d.set_inner(i, &markup)).map(|_| Out::Undefined)
}

fn markup(a: &mut dyn Args) -> Result<Out, Fault> {
    let i = a.index(0)?;
    let outer = a.flag(1);
    with_doc(|d| d.markup(i, outer)).map(Out::Text)
}

fn select(a: &mut dyn Args) -> Result<Out, Fault> {
    let scope = a.index(0)?;
    let selector = a.text(1)?;
    let first = a.flag(2);
    with_doc(|d| d.select(scope, &selector, first)).map(Out::Indexes)
}

fn matches(a: &mut dyn Args) -> Result<Out, Fault> {
    let i = a.index(0)?;
    let selector = a.text(1)?;
    with_doc(|d| d.matches(i, &selector)).map(Out::Bool)
}

fn layout(a: &mut dyn Args) -> Result<Out, Fault> {
    let i = a.index(0)?;
    let found = with_doc(|d| d.probed(i, false))?;
    Ok(match found {
        Some(spot) => {
            let p = spot.placed;
            let mut v = p.rect.to_vec();
            v.extend(p.abs);
            v.push(p.font_size);
            Out::Numbers(v)
        }
        None => Out::Null,
    })
}

fn viewport(_: &mut dyn Args) -> Result<Out, Fault> {
    let (w, h) = with_doc(|d| Ok(d.viewport))?;
    Ok(Out::Numbers(vec![w, h]))
}

fn computed(a: &mut dyn Args) -> Result<Out, Fault> {
    let i = a.index(0)?;
    let prop = a.text(1)?;
    let (probe, node, tag) = with_doc(|d| {
        let tag = d.tag_or_empty(i);
        let spot = d.probed(i, true)?;
        Ok((d.probes, spot.map(|s| s.node), tag))
    })?;
    let key = (probe, node, tag, prop);
    if let Some(text) = COMPUTED.with(|c| c.borrow().get(&key).cloned()) {
        return Ok(Out::Text(text));
    }
    let (_, node, tag, prop) = key.clone();
    let text = text_answer(ask(Ask::Computed { node, tag, prop }))?;
    COMPUTED.with(|c| {
        let mut c = c.borrow_mut();
        c.retain(|k, _| k.0 == probe);
        c.insert(key, text.clone());
    });
    Ok(Out::Text(text))
}

fn matrix(a: &mut dyn Args) -> Result<Out, Fault> {
    let i = a.index(0)?;
    let value = a.text(1)?;
    let size = with_doc(|d| Ok(d.probed(i, false)?.map_or((0.0, 0.0), |s| (s.placed.rect[2], s.placed.rect[3]))))?;
    text_answer(ask(Ask::Matrix { value, size })).map(Out::Text)
}

fn generation(_: &mut dyn Args) -> Result<Out, Fault> {
    with_doc(|d| Ok(d.generation as f64)).map(Out::Number)
}

fn media(a: &mut dyn Args) -> Result<Out, Fault> {
    let query = a.text(0)?;
    match ask(Ask::Media(query)).map_err(Fault::Error)? {
        Reply::Flag(b) => Ok(Out::Bool(b)),
        _ => Err(unexpected()),
    }
}

/// `__dPath(d)` is the length of path data `d` over every contour.
/// `__dPath(d, at)` is the `[x, y]` point at distance `at` along it.
fn path(a: &mut dyn Args) -> Result<Out, Fault> {
    let d = a.text(0)?;
    let at = if a.given(1) { Some(a.number(1)? as f32) } else { None };
    match ask(Ask::Path { d, at }).map_err(Fault::Error)? {
        Reply::Number(n) => Ok(Out::Number(n)),
        Reply::Numbers(v) => Ok(Out::Numbers(v)),
        _ => Err(unexpected()),
    }
}

/// Seconds at a beat position on the composition's beat grid.
fn beat(a: &mut dyn Args) -> Result<Out, Fault> {
    let position = a.number(0)?;
    number_answer(ask(Ask::Beat(position)))
}

/// The beat position at a time in seconds.
fn beat_at(a: &mut dyn Args) -> Result<Out, Fault> {
    let time = a.number(0)?;
    number_answer(ask(Ask::BeatAt(time)))
}

/// The grid's tempo, bar length and drop as JSON.
fn beat_grid(_: &mut dyn Args) -> Result<Out, Fault> {
    text_answer(ask(Ask::BeatGrid)).map(Out::Text)
}

fn text_answer(answer: Result<Reply, String>) -> Result<String, Fault> {
    match answer.map_err(Fault::Error)? {
        Reply::Text(t) => Ok(t),
        _ => Err(unexpected()),
    }
}

fn number_answer(answer: Result<Reply, String>) -> Result<Out, Fault> {
    match answer.map_err(Fault::Error)? {
        Reply::Number(n) => Ok(Out::Number(n)),
        _ => Err(unexpected()),
    }
}

fn unexpected() -> Fault {
    Fault::Error("the compiler sent an unexpected answer".into())
}
