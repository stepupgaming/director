//! GSAP to `director-timeline/1`. Inline page scripts run once at compile time
//! in an embedded JavaScript engine against a recording shim; the recorded
//! tweens become exact keyframe tracks per element property.

pub(crate) mod ease;

use crate::animation::{is_component, Parts};
use crate::style::{color, PaintStyle};
use crate::timeline::{TimelineKeyframe, TimelineTrack, TimelineValue};
use boa_engine::{js_string, Context, JsResult, JsString, JsValue, NativeFunction, Source};
use director_core::types::Color;
use ease::GsapEase;
use ego_tree::NodeId;
use scraper::{ElementRef, Html, Selector};
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

const SHIM: &str = include_str!("shim.js");
/// Seconds a value jump takes between two keyframes at the same instant.
const JUMP: f64 = 1e-4;

/// Recorded GSAP tweens for each animated element.
#[derive(Debug, Default)]
pub(crate) struct Recording {
    pub(crate) records: HashMap<NodeId, Vec<Record>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Record {
    target: usize,
    order: u64,
    start: f64,
    duration: f64,
    ease: String,
    repeat: i64,
    yoyo: bool,
    repeat_delay: f64,
    immediate: bool,
    /// A `gsap.set` outside the timeline.
    #[serde(default)]
    base: bool,
    method: String,
    props: BTreeMap<String, Prop>,
}

#[derive(Debug, Clone, Deserialize)]
struct Prop {
    from: Option<Raw>,
    to: Option<Raw>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum Raw {
    Number(f64),
    Text(String),
}

#[derive(Deserialize)]
struct Output {
    error: Option<String>,
    #[serde(default)]
    records: Vec<Record>,
}

/// The snapshot one element contributes to the shim's read-only DOM.
#[derive(serde::Serialize)]
struct SnapshotElement {
    tag: String,
    attrs: BTreeMap<String, String>,
    text: String,
    parent: i64,
    children: Vec<usize>,
}

struct Snapshot {
    html: Html,
    ids: Vec<NodeId>,
    index: HashMap<NodeId, usize>,
}

thread_local! {
    static DOM: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
}

fn select(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let selector = args
        .first()
        .cloned()
        .unwrap_or_default()
        .to_string(context)?
        .to_std_string_escaped();
    let scope = args.get(1).cloned().unwrap_or_default().to_i32(context)?;
    let parsed = Selector::parse(&selector).map_err(|e| {
        boa_engine::JsNativeError::syntax().with_message(format!("invalid selector '{selector}': {e}"))
    })?;
    let found = DOM.with(|dom| {
        let dom = dom.borrow();
        let dom = dom.as_ref().expect("GSAP DOM snapshot");
        let found: Vec<NodeId> = if scope < 0 {
            dom.html.select(&parsed).map(|e| e.id()).collect()
        } else {
            let scope = dom
                .html
                .tree
                .get(dom.ids[scope as usize])
                .and_then(ElementRef::wrap)
                .expect("scope element");
            scope.select(&parsed).map(|e| e.id()).collect()
        };
        found
            .iter()
            .filter_map(|id| dom.index.get(id))
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(",")
    });
    Ok(JsValue::from(JsString::from(found.as_str())))
}

/// Whether a script `src` loads the GSAP library or one of its plugins.
fn gsap_library(src: &str) -> bool {
    let file = src.rsplit('/').next().unwrap_or(src).to_ascii_lowercase();
    src.contains("/gsap@") || src.contains("/gsap/") || file.starts_with("gsap")
}

fn js_error(e: boa_engine::JsError) -> String {
    let text = e.to_string();
    text.strip_prefix("Error: ").unwrap_or(&text).to_string()
}

/// Run the document's inline scripts against the GSAP recorder. Returns
/// `None` when the page has no executable script.
pub(crate) fn record(
    dom: &Html,
    composition: &str,
    width: i32,
    height: i32,
) -> Result<Option<Recording>, String> {
    let mut sources = Vec::new();
    for script in dom.select(&Selector::parse("script").unwrap()) {
        let kind = script.value().attr("type").map(str::trim).unwrap_or("");
        if matches!(
            kind,
            "application/json" | "application/ld+json" | crate::timeline::TIMELINE_SCRIPT
        ) {
            continue;
        }
        if !matches!(kind, "" | "text/javascript" | "application/javascript") {
            return Err(format!("script type '{kind}' requires extraction; run director-engine html capture"));
        }
        if let Some(src) = script.value().attr("src") {
            if gsap_library(src) {
                continue;
            }
            return Err(format!(
                "external script '{src}' requires extraction; run director-engine html capture"
            ));
        }
        sources.push(script.text().collect::<String>());
    }
    if sources.is_empty() {
        return Ok(None);
    }

    let mut ids = Vec::new();
    let mut index = HashMap::new();
    let mut elements = Vec::new();
    for node in dom.tree.root().descendants() {
        let Some(el) = ElementRef::wrap(node) else { continue };
        index.insert(el.id(), ids.len());
        ids.push(el.id());
        let parent = el
            .parent()
            .and_then(ElementRef::wrap)
            .and_then(|p| index.get(&p.id()).copied())
            .map_or(-1, |p| p as i64);
        if parent >= 0 {
            let at = ids.len() - 1;
            let p: &mut SnapshotElement = &mut elements[parent as usize];
            p.children.push(at);
        }
        elements.push(SnapshotElement {
            tag: el.value().name().to_string(),
            attrs: el.value().attrs().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            text: el.text().collect(),
            parent,
            children: Vec::new(),
        });
    }
    let snapshot = serde_json::json!({
        "width": width,
        "height": height,
        "composition": composition,
        "elements": elements,
    });
    DOM.with(|d| {
        *d.borrow_mut() = Some(Snapshot {
            html: dom.clone(),
            ids: ids.clone(),
            index,
        })
    });
    let result = run(&snapshot.to_string(), &sources);
    DOM.with(|d| d.borrow_mut().take());
    let output = result?;
    let mut recording = Recording::default();
    for record in output {
        let id = *ids.get(record.target).ok_or("GSAP target outside the document")?;
        recording.records.entry(id).or_default().push(record);
    }
    Ok(Some(recording))
}

fn run(snapshot: &str, sources: &[String]) -> Result<Vec<Record>, String> {
    let mut context = Context::default();
    context.runtime_limits_mut().set_loop_iteration_limit(50_000_000);
    context.runtime_limits_mut().set_recursion_limit(4_096);
    context
        .register_global_callable(js_string!("__director_select"), 2, NativeFunction::from_fn_ptr(select))
        .map_err(js_error)?;
    context
        .eval(Source::from_bytes(&format!("globalThis.__director_dom = {snapshot};")))
        .map_err(js_error)?;
    context.eval(Source::from_bytes(SHIM)).map_err(js_error)?;
    for source in sources {
        context.eval(Source::from_bytes(source)).map_err(|e| format!("script: {}", js_error(e)))?;
    }
    let mut call = |name: &str| -> Result<Output, String> {
        context.run_jobs().map_err(js_error)?;
        let out = context
            .eval(Source::from_bytes(&format!("{name}()")))
            .map_err(js_error)?;
        let text = out
            .as_string()
            .ok_or("GSAP recorder returned no output")?
            .to_std_string_escaped();
        let out: Output = serde_json::from_str(&text).map_err(|e| format!("GSAP recorder: {e}"))?;
        match out.error {
            Some(error) => Err(error),
            None => Ok(out),
        }
    };
    call("__director_ready")?;
    Ok(call("__director_export")?.records)
}

/// A channel value: a number in the track unit, or straight RGBA in 0..1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Val {
    Num(f64),
    Color([f64; 4]),
}
impl Val {
    fn color(c: Color) -> Self {
        Self::Color([c.r as f64, c.g as f64, c.b as f64, c.a as f64])
    }
    fn mix(self, other: Self, t: f64) -> Self {
        match (self, other) {
            (Self::Num(a), Self::Num(b)) => Self::Num(a + (b - a) * t),
            (Self::Color(a), Self::Color(b)) => {
                Self::Color(std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t))
            }
            _ => other,
        }
    }
    fn timeline(self) -> TimelineValue {
        match self {
            Self::Num(n) => TimelineValue::Number(n),
            Self::Color([r, g, b, a]) => TimelineValue::Text(format!(
                "rgba({}, {}, {}, {})",
                round(r * 255.0),
                round(g * 255.0),
                round(b * 255.0),
                round(a)
            )),
        }
    }
}
fn round(v: f64) -> f64 {
    (v * 1e4).round() / 1e4
}

/// The base value of a channel on an element before any tween.
pub(crate) fn base_value(channel: &str, style: &taffy::Style, paint: &PaintStyle) -> Result<Val, String> {
    use taffy::style::CompactLength;
    let length = |raw: CompactLength, name: &str| {
        if raw.tag() == CompactLength::LENGTH_TAG {
            Ok(Val::Num(raw.value() as f64))
        } else {
            Err(format!("GSAP '{name}' needs a px base value on the element"))
        }
    };
    match channel {
        "opacity" => Ok(Val::Num(paint.opacity as f64)),
        "color" => Ok(Val::color(paint.color)),
        "background-color" => Ok(Val::color(
            paint
                .box_node
                .bg_color
                .as_ref()
                .map(|c| c.current_value)
                .unwrap_or(Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }),
        )),
        "width" => length(style.size.width.into_raw(), channel),
        "height" => length(style.size.height.into_raw(), channel),
        "left" => length(style.inset.left.into_raw(), channel),
        "top" => length(style.inset.top.into_raw(), channel),
        "border-top-left-radius" => length(paint.corners[0].0.into_raw(), channel),
        _ if is_component(channel) => Parts::decompose(&paint.transform)
            .and_then(|p| p.get(channel))
            .map(|v| Val::Num(v as f64))
            .ok_or_else(|| {
                "GSAP transforms need a base transform of translations, rotations, one skew and scales"
                    .to_string()
            }),
        _ => Err(format!("unsupported GSAP channel '{channel}'")),
    }
}

/// A recorded property value before it meets the channel's current value.
#[derive(Debug, Clone, Copy)]
enum Goal {
    Abs(Val),
    Rel(f64),
}
impl Goal {
    fn resolve(self, current: Val) -> Result<Val, String> {
        match (self, current) {
            (Self::Abs(v), _) => Ok(v),
            (Self::Rel(d), Val::Num(n)) => Ok(Val::Num(n + d)),
            (Self::Rel(_), Val::Color(_)) => Err("relative GSAP colors are unsupported".into()),
        }
    }
}

fn amount(raw: &Raw, prop: &str, units: &[(&str, f64)]) -> Result<Goal, String> {
    let text = match raw {
        Raw::Number(n) => return Ok(Goal::Abs(Val::Num(*n))),
        Raw::Text(s) => s.trim(),
    };
    let (relative, rest) = match text.get(..2) {
        Some("+=") => (Some(1.0), &text[2..]),
        Some("-=") => (Some(-1.0), &text[2..]),
        _ => (None, text),
    };
    let rest = rest.trim();
    let (number, scale) = units
        .iter()
        .find_map(|(unit, scale)| rest.strip_suffix(unit).map(|n| (n, *scale)))
        .unwrap_or((rest, 1.0));
    let n = number
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("unsupported GSAP value '{text}' for '{prop}'"))?
        * scale;
    Ok(match relative {
        Some(sign) => Goal::Rel(sign * n),
        None => Goal::Abs(Val::Num(n)),
    })
}

const PX: &[(&str, f64)] = &[("px", 1.0)];
const PERCENT: &[(&str, f64)] = &[("%", 1.0)];
const DEG: &[(&str, f64)] = &[
    ("deg", 1.0),
    ("turn", 360.0),
    ("rad", 180.0 / std::f64::consts::PI),
];

/// The native channels one GSAP property drives, with its parsed goal.
fn channels(prop: &str, raw: &Raw) -> Result<Vec<(&'static str, Goal)>, String> {
    let percent = matches!(raw, Raw::Text(s) if s.trim().ends_with('%'));
    let one = |channel: &'static str, units| Ok(vec![(channel, amount(raw, prop, units)?)]);
    match prop {
        "opacity" | "autoAlpha" => one("opacity", &[]),
        "x" if percent => one("translate-x-percent", PERCENT),
        "y" if percent => one("translate-y-percent", PERCENT),
        "x" => one("translate-x", PX),
        "y" => one("translate-y", PX),
        "z" => one("translate-z", PX),
        "xPercent" => one("translate-x-percent", PERCENT),
        "yPercent" => one("translate-y-percent", PERCENT),
        "rotation" | "rotate" | "rotationZ" | "rotateZ" => one("rotate-z", DEG),
        "rotationX" | "rotateX" => one("rotate-x", DEG),
        "rotationY" | "rotateY" => one("rotate-y", DEG),
        "skewX" => one("skew-x", DEG),
        "skewY" => one("skew-y", DEG),
        "scaleX" => one("scale-x", &[]),
        "scaleY" => one("scale-y", &[]),
        "scale" => {
            let goal = amount(raw, prop, &[])?;
            Ok(vec![("scale-x", goal), ("scale-y", goal)])
        }
        "width" => one("width", PX),
        "height" => one("height", PX),
        "left" => one("left", PX),
        "top" => one("top", PX),
        "borderRadius" => one("border-top-left-radius", PX),
        "color" | "backgroundColor" => {
            let Raw::Text(text) = raw else {
                return Err(format!("GSAP '{prop}' takes a CSS color"));
            };
            let c = color(text.trim(), Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 })?;
            let channel = if prop == "color" { "color" } else { "background-color" };
            Ok(vec![(channel, Goal::Abs(Val::color(c)))])
        }
        _ => Err(format!(
            "GSAP property '{prop}' has no native track; run director-engine html capture"
        )),
    }
}

/// One tween on one channel, with resolved endpoints once playback reaches it.
#[derive(Debug, Clone)]
struct Segment {
    start: f64,
    duration: f64,
    ease: Option<GsapEase>,
    repeat: u32,
    yoyo: bool,
    repeat_delay: f64,
    from: Val,
    to: Val,
}
impl Segment {
    fn cycle(&self) -> f64 {
        self.duration + self.repeat_delay
    }
    fn end(&self) -> f64 {
        self.start + self.cycle() * (self.repeat as f64 + 1.0) - self.repeat_delay
    }
    fn final_value(&self) -> Val {
        if self.yoyo && self.repeat % 2 == 1 {
            self.from
        } else {
            self.to
        }
    }
    fn ratio(&self, p: f64) -> f64 {
        self.ease.as_ref().map_or(p, |e| e.eval(p))
    }
    fn value_at(&self, t: f64) -> Val {
        let local = t - self.start;
        if self.duration <= 0.0 || t >= self.end() {
            return self.final_value();
        }
        let k = (local / self.cycle()).floor().max(0.0);
        let within = local - k * self.cycle();
        let forward = !(self.yoyo && (k as u64) % 2 == 1);
        if within >= self.duration {
            return if forward { self.to } else { self.from };
        }
        let p = within / self.duration;
        let r = if forward { self.ratio(p) } else { self.ratio(1.0 - p) };
        self.from.mix(self.to, r)
    }
}

/// The value the latest-started segment holds at `t`, else `initial`.
fn value_at(segments: &[Segment], initial: Val, t: f64) -> Val {
    segments
        .iter()
        .rev()
        .find(|s| s.start <= t)
        .map_or(initial, |s| s.value_at(t))
}

struct Keys(Vec<TimelineKeyframe>, Vec<Val>);
impl Keys {
    fn last(&self) -> Option<(f64, Val)> {
        self.0.last().map(|k| k.time).zip(self.1.last().copied())
    }
    fn push(&mut self, time: f64, value: Val, ease: Option<String>) {
        self.0.push(TimelineKeyframe {
            time,
            value: value.timeline(),
            ease,
        });
        self.1.push(value);
    }
    /// `value` from `time` on, jumping there when it differs.
    fn at(&mut self, time: f64, value: Val) {
        match self.last() {
            None => self.push(time, value, None),
            Some((t, v)) if t >= time - 1e-9 => {
                if v == value {
                    return;
                }
                let n = self.0.len();
                if n >= 2 && self.0[n - 2].time >= time - JUMP - 1e-9 {
                    self.0.pop();
                    self.1.pop();
                    self.at(time, value);
                    return;
                }
                self.0[n - 1].time = time - JUMP;
                self.push(time, value, None);
            }
            Some((t, v)) => {
                if v != value && time - JUMP > t + 1e-9 {
                    self.push(time - JUMP, v, None);
                }
                self.push(time, value, None);
            }
        }
    }
    fn ramp(&mut self, time: f64, value: Val, ease: Option<String>) {
        if self.last().is_some_and(|(t, _)| time <= t + 1e-9) {
            self.at(time, value);
        } else {
            self.push(time, value, ease);
        }
    }
}

/// Exact keyframes for the visible part `[start, cut)` of one segment.
fn emit(keys: &mut Keys, s: &Segment, cut: f64, fps: f64) {
    for k in 0..=s.repeat {
        let begin = s.start + k as f64 * s.cycle();
        if begin >= cut {
            break;
        }
        let forward = !(s.yoyo && k % 2 == 1);
        let (a, b) = if forward { (s.from, s.to) } else { (s.to, s.from) };
        if s.duration <= 0.0 {
            keys.at(begin, a);
            keys.at(begin, b);
            continue;
        }
        let end = begin + s.duration;
        let ease = match (&s.ease, forward) {
            (None, _) => Some(None),
            (Some(e), true) => e.exact().then(|| Some(e.text())),
            (Some(e), false) => e.reversed().filter(GsapEase::exact).map(|e| Some(e.text())),
        };
        keys.at(begin, a);
        match ease {
            Some(ease) if end <= cut + 1e-9 => {
                let ease = ease.filter(|e| e != "none");
                keys.ramp(end, b, ease);
            }
            _ => {
                // Steps jump at their boundaries; other curves sample each frame.
                let stop = end.min(cut);
                let local = |t: f64| {
                    let p = (t - begin) / s.duration;
                    a.mix(b, if forward { s.ratio(p) } else { 1.0 - s.ratio(1.0 - p) })
                };
                if let Some(n) = s.ease.as_ref().and_then(GsapEase::steps) {
                    for i in 1..=n {
                        let t = begin + s.duration * i as f64 / (n as f64 + 1.0);
                        if t >= stop {
                            break;
                        }
                        keys.at(t, local(t + 1e-9));
                    }
                } else {
                    let mut f = (begin * fps).floor() + 1.0;
                    while f / fps < stop - 1e-9 {
                        keys.ramp(f / fps, local(f / fps), None);
                        f += 1.0;
                    }
                }
                if end <= cut + 1e-9 {
                    keys.at(end, b);
                }
            }
        }
    }
}

/// Native timeline tracks for one element's recorded tweens.
pub(crate) fn tracks(
    records: &[Record],
    target: &str,
    base: &dyn Fn(&str) -> Result<Val, String>,
    fps: f64,
    duration: f64,
) -> Result<Vec<TimelineTrack>, String> {
    // Each record's goals per channel, in record order.
    struct Entry<'a> {
        record: &'a Record,
        from: Option<Goal>,
        to: Option<Goal>,
    }
    let mut by_channel: BTreeMap<&'static str, Vec<Entry>> = BTreeMap::new();
    let mut records: Vec<&Record> = records.iter().collect();
    records.sort_by_key(|r| r.order);
    for record in records {
        for (prop, value) in &record.props {
            let mut goals: BTreeMap<&'static str, (Option<Goal>, Option<Goal>)> = BTreeMap::new();
            for (raw, side) in [(&value.from, 0), (&value.to, 1)] {
                let Some(raw) = raw else { continue };
                for (channel, goal) in channels(prop, raw)? {
                    let slot = goals.entry(channel).or_default();
                    if side == 0 {
                        slot.0 = Some(goal);
                    } else {
                        slot.1 = Some(goal);
                    }
                }
            }
            for (channel, (from, to)) in goals {
                let list = by_channel.entry(channel).or_default();
                list.retain(|e| !std::ptr::eq(e.record, record));
                list.push(Entry { record, from, to });
            }
        }
    }
    let mut out = Vec::new();
    for (channel, entries) in by_channel {
        let base = base(channel)?;
        // Creation pass: immediate renders change the starting state in
        // script order, and an immediate from() ends at the state it found.
        let mut state = base;
        let mut planned = Vec::new();
        for entry in &entries {
            let r = entry.record;
            let mut from = None;
            let mut to = None;
            if r.base {
                // gsap.set outside the timeline only moves the starting state.
                state = entry.to.ok_or("gsap.set() without values")?.resolve(state)?;
                continue;
            }
            if r.immediate {
                match r.method.as_str() {
                    "from" => {
                        to = Some(state);
                        let f = entry.from.ok_or("GSAP from() without values")?.resolve(state)?;
                        from = Some(f);
                        state = f;
                    }
                    "fromTo" => {
                        let f = entry.from.map(|g| g.resolve(state)).transpose()?.unwrap_or(state);
                        to = entry.to.map(|g| g.resolve(f)).transpose()?;
                        from = Some(f);
                        state = f;
                    }
                    "set" => {
                        let v = entry.to.ok_or("GSAP set() without values")?.resolve(state)?;
                        state = v;
                        from = Some(v);
                        to = Some(v);
                    }
                    // An immediate to() renders its own starting state.
                    _ => {}
                }
            }
            planned.push((entry, from, to));
        }
        let initial = state;
        // Playback pass: tweens start in time order and pick up the value
        // their channel holds when they start.
        planned.sort_by(|a, b| {
            (a.0.record.start, a.0.record.order)
                .partial_cmp(&(b.0.record.start, b.0.record.order))
                .unwrap()
        });
        let mut segments: Vec<Segment> = Vec::new();
        for (entry, from, to) in planned {
            let r = entry.record;
            let current = value_at(&segments, initial, r.start);
            let from = match from {
                Some(v) => v,
                None => match entry.from {
                    Some(goal) => goal.resolve(current)?,
                    None => current,
                },
            };
            let to = match to {
                Some(v) => v,
                None => match (r.method.as_str(), entry.to) {
                    ("from", _) => current,
                    (_, Some(goal)) => goal.resolve(from)?,
                    (_, None) => from,
                },
            };
            let cycle = r.duration.max(0.0) + r.repeat_delay.max(0.0);
            let repeat = match r.repeat {
                n if n >= 0 => n as u32,
                _ if cycle <= 0.0 => 0,
                _ => (((duration - r.start) / cycle).ceil().max(0.0) as u32).min(100_000),
            };
            let ease = if r.method == "set" || r.ease == "none" {
                None
            } else {
                Some(GsapEase::parse(&r.ease)?)
            };
            let segment = Segment {
                start: r.start,
                duration: r.duration.max(0.0),
                ease,
                repeat,
                yoyo: r.yoyo,
                repeat_delay: r.repeat_delay.max(0.0),
                from,
                to,
            };
            // Overlapping tweens resolve by seek order in GSAP; refuse the
            // case where an earlier tween outlives a later one.
            if let Some(earlier) = segments
                .iter()
                .find(|e| e.start < segment.start && e.end() > segment.end() + 1e-9)
            {
                return Err(format!(
                    "GSAP tweens overlap on '{channel}' of '{target}': the tween at {}s outlives the one at {}s",
                    earlier.start, segment.start
                ));
            }
            segments.push(segment);
        }
        if segments.is_empty() {
            if initial != base {
                let mut keys = Keys(Vec::new(), Vec::new());
                keys.at(0.0, initial);
                out.push(track(target, channel, keys));
            }
            continue;
        }
        let mut keys = Keys(Vec::new(), Vec::new());
        if segments[0].start > 0.0 && initial != segments[0].from {
            keys.at(0.0, initial);
        }
        for (i, segment) in segments.iter().enumerate() {
            let cut = segments.get(i + 1).map_or(f64::INFINITY, |n| n.start);
            emit(&mut keys, segment, cut, fps);
        }
        out.push(track(target, channel, keys));
    }
    Ok(out)
}

fn track(target: &str, channel: &str, keys: Keys) -> TimelineTrack {
    TimelineTrack {
        target: target.to_string(),
        property: channel.to_string(),
        keyframes: keys.0,
    }
}
