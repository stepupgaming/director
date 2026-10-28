//! Slot-reel counters: `data-reel` turns each digit of an element's text into
//! a clipped window whose strip of digits rolls on to the final digit.
//!
//! The expansion rewrites the DOM before page scripts run. Each digit becomes
//! an `inline-block` window with `overflow: hidden` that holds the final digit
//! as hidden text, which sizes the window to one line, beside an absolute
//! strip of digits. A generated `director-timeline/1` script moves each strip
//! on `translate-y-percent`, so the reels compile, export and edit as ordinary
//! timeline tracks.

use crate::beats::Beats;
use crate::timeline::{Timeline, TimelineKeyframe, TimelineTrack, TimelineValue, TIMELINE_FORMAT, TIMELINE_SCRIPT};
use crate::CompileError;
use ego_tree::NodeId;
use html5ever::{ns, Attribute, LocalName, QualName};
use scraper::node::{Element, Text};
use scraper::{ElementRef, Html, Node, Selector};
use std::collections::HashSet;

/// The attributes a reel reads; the expansion removes them.
const ATTRIBUTES: [&str; 8] = [
    "data-reel",
    "data-reel-from",
    "data-reel-start",
    "data-reel-duration",
    "data-reel-stagger",
    "data-reel-spins",
    "data-reel-ease",
    "data-reel-direction",
];

/// One reel's settings, read from its element.
struct Reel {
    node: NodeId,
    /// The element's text: digits roll and other characters stay.
    text: String,
    /// The digit each window starts on, right-aligned to `text`'s digits.
    from: Vec<u32>,
    start: f64,
    duration: f64,
    stagger: f64,
    spins: u32,
    ease: String,
    down: bool,
    /// Prefix of the strip ids.
    base: String,
}

/// Expands every `data-reel` element of `dom` inside `root` and appends the
/// timeline that rolls its strips.
pub(crate) fn expand(dom: &mut Html, root: NodeId, beats: &Beats) -> Result<(), CompileError> {
    let reels: Vec<NodeId> = dom
        .select(&Selector::parse("[data-reel]").unwrap())
        .map(|el| el.id())
        .collect();
    if reels.is_empty() {
        return Ok(());
    }
    let mut ids: HashSet<String> = dom
        .select(&Selector::parse("[id]").unwrap())
        .filter_map(|el| el.value().attr("id").map(str::to_string))
        .collect();
    let mut tracks = Vec::new();
    for (index, node) in reels.into_iter().enumerate() {
        let reel = read(dom, node, root, index, beats)?;
        build(dom, &reel, &mut ids, &mut tracks)?;
    }
    if tracks.is_empty() {
        return Ok(());
    }
    let json = serde_json::to_string(&Timeline { format: TIMELINE_FORMAT.into(), tracks })
        .map_err(|e| CompileError::Input(format!("reel timeline: {e}")))?;
    let body = dom
        .select(&Selector::parse("body").unwrap())
        .next()
        .map(|b| b.id())
        .unwrap_or(root);
    let script = dom.tree.orphan(element("script", &[("type", TIMELINE_SCRIPT)])).id();
    dom.tree.get_mut(script).unwrap().append(Node::Text(Text { text: json.into() }));
    dom.tree.get_mut(body).unwrap().append_id(script);
    Ok(())
}

fn read(dom: &Html, node: NodeId, root: NodeId, index: usize, beats: &Beats) -> Result<Reel, CompileError> {
    let el = ElementRef::wrap(dom.tree.get(node).unwrap()).unwrap();
    let label = match el.value().attr("id") {
        Some(id) => format!("#{id}"),
        None => format!("<{}>", el.value().name()),
    };
    let fail = |message: String| CompileError::Input(format!("{label}: {message}"));
    if node != root && !el.ancestors().any(|a| a.id() == root) {
        return Err(fail("data-reel sits outside the composition".into()));
    }
    if el.value().attr("data-reel").is_some_and(|v| !v.trim().is_empty()) {
        return Err(fail("data-reel takes no value; the element's text is the final value".into()));
    }
    if el.children().any(|c| !c.value().is_text() && !c.value().is_comment()) {
        return Err(fail("data-reel holds text only".into()));
    }
    let text: String = el.text().collect();
    let digits: Vec<u32> = text.chars().filter_map(|c| c.to_digit(10)).collect();
    if digits.is_empty() || !text.chars().all(|c| !c.is_numeric() || c.is_ascii_digit()) {
        return Err(fail(format!("data-reel text '{text}' needs ASCII digits")));
    }
    let attr = |name: &str| el.value().attr(name).map(str::trim);
    // The starting digits align to the final digits from the right; missing ones start at 0.
    let mut from = vec![0; digits.len()];
    if let Some(value) = attr("data-reel-from") {
        let start: Vec<u32> = value.chars().filter_map(|c| c.to_digit(10)).collect();
        if start.is_empty() || start.len() > digits.len() {
            return Err(fail(format!(
                "data-reel-from '{value}' needs 1 to {} digits, one per rolling digit",
                digits.len()
            )));
        }
        from[digits.len() - start.len()..].copy_from_slice(&start);
    }
    let clip = clip_start(el, root, beats).map_err(fail)?;
    let start = match attr("data-reel-start") {
        Some(value) => match beats.seconds(value).map_err(|e| fail(format!("data-reel-start {e}")))? {
            Some(seconds) => seconds,
            None => clip + number(value, "data-reel-start").map_err(fail)?,
        },
        None => clip,
    };
    let span = |name: &str, default: f64| -> Result<f64, CompileError> {
        let Some(value) = attr(name) else { return Ok(default) };
        let seconds = match beats.span_end(start, value).map_err(|e| fail(format!("{name} {e}")))? {
            Some(end) => end - start,
            None => number(value, name).map_err(fail)?,
        };
        Ok(seconds)
    };
    let duration = span("data-reel-duration", 1.2)?;
    let stagger = span("data-reel-stagger", 0.1)?;
    if start < 0.0 || duration <= 0.0 || stagger < 0.0 {
        return Err(fail(format!(
            "a reel needs a start at or after 0 s, a positive duration and a stagger of at least 0 (got {start}, {duration}, {stagger})"
        )));
    }
    let spins = match attr("data-reel-spins") {
        Some(value) => value
            .parse::<u32>()
            .ok()
            .filter(|spins| *spins <= 20)
            .ok_or_else(|| fail(format!("data-reel-spins must be a whole number from 0 to 20 (got '{value}')")))?,
        None => 1,
    };
    let down = match attr("data-reel-direction") {
        None | Some("up") => false,
        Some("down") => true,
        Some(other) => return Err(fail(format!("data-reel-direction must be up or down (got '{other}')"))),
    };
    let base = match el.value().attr("id") {
        Some(id) => id.to_string(),
        None => format!("reel-{index}"),
    };
    Ok(Reel {
        node,
        text,
        from,
        start,
        duration,
        stagger,
        spins,
        ease: attr("data-reel-ease").unwrap_or("power3.out").to_string(),
        down,
        base,
    })
}

/// Seconds as a plain number.
fn number(value: &str, name: &str) -> Result<f64, String> {
    value
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("{name} must be seconds or a beat time (got '{value}')"))
}

/// The composition second `el`'s clip starts at: a beat-valued `data-start`
/// names a grid point, seconds count from the enclosing composition's start,
/// and an element without one starts with its nearest timed ancestor.
fn clip_start(el: ElementRef<'_>, root: NodeId, beats: &Beats) -> Result<f64, String> {
    if el.id() == root {
        return Ok(0.0);
    }
    let Some(value) = el.value().attr("data-start") else {
        return match el.parent().and_then(ElementRef::wrap) {
            Some(parent) => clip_start(parent, root, beats),
            None => Ok(0.0),
        };
    };
    if let Some(seconds) = beats.seconds(value)? {
        return Ok(seconds);
    }
    let composition = el
        .ancestors()
        .filter_map(ElementRef::wrap)
        .find(|a| a.id() == root || a.value().attr("data-composition-id").is_some());
    let offset = match composition {
        Some(parent) => clip_start(parent, root, beats)?,
        None => 0.0,
    };
    Ok(offset + number(value, "data-start")?)
}

/// Replaces the reel's text with static text and one window per digit, and
/// adds a strip track for every digit that moves.
fn build(dom: &mut Html, reel: &Reel, ids: &mut HashSet<String>, tracks: &mut Vec<TimelineTrack>) -> Result<(), CompileError> {
    let children: Vec<NodeId> = dom.tree.get(reel.node).unwrap().children().map(|c| c.id()).collect();
    for child in children {
        dom.tree.get_mut(child).unwrap().detach();
    }
    if let Node::Element(el) = dom.tree.get_mut(reel.node).unwrap().value() {
        el.attrs.retain(|(name, _)| !ATTRIBUTES.contains(&&*name.local));
    }
    let mut run = String::new();
    let mut digit = 0;
    for c in reel.text.chars() {
        let Some(to) = c.to_digit(10) else {
            run.push(c);
            continue;
        };
        if !run.is_empty() {
            let text = dom.tree.orphan(Node::Text(Text { text: std::mem::take(&mut run).into() })).id();
            dom.tree.get_mut(reel.node).unwrap().append_id(text);
        }
        // The strip counts up from the starting digit through every spin to the final digit.
        let steps = (to + 10 - reel.from[digit]) % 10 + 10 * reel.spins;
        let mut values: Vec<u32> = (0..=steps).map(|k| (reel.from[digit] + k) % 10).collect();
        if reel.down {
            values.reverse();
        }
        let id = format!("{}-reel-{digit}", reel.base);
        if !ids.insert(id.clone()) {
            return Err(CompileError::Input(format!(
                "data-reel strip id '{id}' is already in use; give the reel element another id"
            )));
        }
        let window = dom.tree.orphan(element("span", &[("style", "display:inline-block;position:relative;overflow:hidden")])).id();
        let placeholder = dom.tree.orphan(element("span", &[("style", "visibility:hidden")])).id();
        dom.tree.get_mut(placeholder).unwrap().append(Node::Text(Text { text: c.to_string().into() }));
        let strip = dom
            .tree
            .orphan(element("span", &[("id", &id), ("style", "position:absolute;left:0;right:0;top:0;text-align:center")]))
            .id();
        for value in &values {
            let cell = dom.tree.orphan(element("span", &[("style", "display:block")])).id();
            dom.tree.get_mut(cell).unwrap().append(Node::Text(Text { text: value.to_string().into() }));
            dom.tree.get_mut(strip).unwrap().append_id(cell);
        }
        dom.tree.get_mut(window).unwrap().append_id(placeholder);
        dom.tree.get_mut(window).unwrap().append_id(strip);
        dom.tree.get_mut(reel.node).unwrap().append_id(window);
        if steps > 0 {
            // A strip of n cells ends one cell short of its full height.
            let end = -100.0 * steps as f64 / (steps + 1) as f64;
            let (first, last) = if reel.down { (end, 0.0) } else { (0.0, end) };
            tracks.push(TimelineTrack {
                target: id,
                property: "translate-y-percent".into(),
                keyframes: vec![
                    TimelineKeyframe { time: reel.start, value: TimelineValue::Number(first), ease: None },
                    TimelineKeyframe {
                        time: reel.start + reel.duration + reel.stagger * digit as f64,
                        value: TimelineValue::Number(last),
                        ease: Some(reel.ease.clone()),
                    },
                ],
            });
        }
        digit += 1;
    }
    if !run.is_empty() {
        let text = dom.tree.orphan(Node::Text(Text { text: run.into() })).id();
        dom.tree.get_mut(reel.node).unwrap().append_id(text);
    }
    Ok(())
}

fn element(tag: &str, attrs: &[(&str, &str)]) -> Node {
    let qual = |name: &str| QualName::new(None, ns!(), LocalName::from(name));
    let attrs = attrs
        .iter()
        .map(|(name, value)| Attribute { name: qual(name), value: (*value).into() })
        .collect();
    Node::Element(Element::new(QualName::new(None, ns!(html), LocalName::from(tag)), attrs))
}
