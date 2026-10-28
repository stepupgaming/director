//! GSAP at compile time. The page scripts, GSAP included, run once in Boa
//! against a Rust-owned document (`dom.rs`, `dom.js`). The registered
//! timelines then seek to every frame, and what the scripts write while they
//! play becomes native `director-timeline/1` tracks.

mod dom;
pub(crate) mod ease;

use crate::animation::{Parts, COMPONENTS};
use crate::assets::AssetStore;
use crate::style::{color, PaintStyle};
use crate::timeline::{TimelineKeyframe, TimelineTrack, TimelineValue};
use crate::CompileOptions;
use boa_engine::{Context, Source};
use director_core::types::Color;
use ego_tree::NodeId;
use scraper::{Html, Selector};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

pub(crate) use dom::{matrix_text, Probe, Probed};

const DOM: &str = include_str!("dom.js");

/// One value per frame for each property a script animates, `None` where the
/// element falls back to its cascaded value.
pub(crate) type Series = BTreeMap<String, Vec<Option<String>>>;

/// What the page scripts animate, by element of the compiled document.
#[derive(Debug, Default)]
pub(crate) struct Motion {
    pub(crate) series: HashMap<NodeId, Series>,
}

/// Inline styles GSAP writes that change nothing on screen.
const INVISIBLE: &[&str] = &[
    "pointer-events",
    "cursor",
    "will-change",
    "user-select",
    "-webkit-user-select",
    "touch-action",
];

/// Properties that vary into native tracks.
const CORNERS: [&str; 4] = [
    "border-top-left-radius",
    "border-top-right-radius",
    "border-bottom-right-radius",
    "border-bottom-left-radius",
];
const NATIVE: &[&str] = &[
    "opacity",
    "color",
    "background-color",
    "width",
    "height",
    "left",
    "top",
    "border-radius",
    "transform",
];

struct Script {
    name: String,
    source: String,
    /// The GSAP major version a CDN URL names.
    major: Option<u32>,
}

/// Whether a script `src` loads the GSAP library or one of its plugins.
fn gsap_library(src: &str) -> bool {
    let file = src.rsplit('/').next().unwrap_or(src).to_ascii_lowercase();
    src.contains("/gsap@") || src.contains("/gsap/") || file.starts_with("gsap")
}

/// The major version in `gsap@3.14.2` or cdnjs `/gsap/3.14.2/`.
fn url_major(src: &str) -> Option<u32> {
    let rest = src
        .split_once("gsap@")
        .or_else(|| src.split_once("/gsap/"))
        .map(|(_, rest)| rest)?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Where CDN GSAP files resolve: the option, `DIRECTOR_GSAP_DIR`, then the
/// nearest `node_modules/gsap/dist` at or above the base directory.
fn gsap_dir(options: &CompileOptions) -> Option<PathBuf> {
    options
        .gsap_dir
        .clone()
        .or_else(|| std::env::var_os("DIRECTOR_GSAP_DIR").map(PathBuf::from))
        .or_else(|| {
            let base = std::path::absolute(&options.base_dir).ok()?;
            base.ancestors()
                .map(|dir| dir.join("node_modules").join("gsap").join("dist"))
                .find(|dir| dir.is_dir())
        })
}

fn js_error(e: boa_engine::JsError) -> String {
    let text = e.to_string();
    text.strip_prefix("Error: ").unwrap_or(&text).to_string()
}

/// The page's classic scripts in document order.
fn scripts(dom: &Html, options: &CompileOptions, assets: &AssetStore) -> Result<Vec<Script>, String> {
    let mut out = Vec::new();
    for script in dom.select(&Selector::parse("script").unwrap()) {
        let kind = script.value().attr("type").map(str::trim).unwrap_or("");
        if matches!(
            kind,
            "application/json" | "application/ld+json" | crate::timeline::TIMELINE_SCRIPT
        ) {
            continue;
        }
        if kind == "module" {
            return Err("module scripts do not run at compile time; load GSAP with a classic script".into());
        }
        if !matches!(kind, "" | "text/javascript" | "application/javascript") {
            return Err(format!("script type '{kind}' requires extraction; run director-engine html capture"));
        }
        let Some(src) = script.value().attr("src") else {
            out.push(Script {
                name: "inline script".into(),
                source: script.text().collect(),
                major: None,
            });
            continue;
        };
        let bytes = if src.contains("://") || src.starts_with("//") {
            if !gsap_library(src) {
                return Err(format!(
                    "external script '{src}' must be downloaded before native compilation"
                ));
            }
            let file = src.split(['?', '#']).next().unwrap_or(src).rsplit('/').next().unwrap_or(src);
            let path = gsap_dir(options).map(|dir| dir.join(file)).filter(|path| path.is_file());
            assets.read_as(src, path.as_deref()).map_err(|_| {
                format!(
                    "GSAP from '{src}' needs a local copy of {file}: set CompileOptions.gsap_dir or \
                     DIRECTOR_GSAP_DIR, or install gsap under node_modules"
                )
            })?
        } else {
            assets.read(src).map_err(|e| e.to_string())?
        };
        out.push(Script {
            name: src.to_string(),
            source: String::from_utf8(bytes).map_err(|_| format!("script '{src}' is not UTF-8"))?,
            major: url_major(src),
        });
    }
    Ok(out)
}

/// Run the page scripts and record every frame. Constant results bake into
/// `dom`, which is replaced by its compacted form; varying ones come back as
/// series.
pub(crate) fn run(dom: &mut Html, options: &CompileOptions, assets: &Arc<AssetStore>) -> Result<Motion, String> {
    let scripts = scripts(dom, options, assets)?;
    if scripts.is_empty() {
        return Ok(Motion::default());
    }
    let prober: dom::Prober = {
        let options = options.clone();
        let assets = assets.clone();
        Box::new(move |html: &Html| crate::compiler::probe(html, &options, &assets).map_err(|e| e.to_string()))
    };
    let html = std::mem::replace(dom, Html::new_document());
    let doc = dom::Doc::new(html, prober, (options.width as f32, options.height as f32));
    let frames = (options.duration * options.fps).ceil() as usize + 1;
    let (recorded, mut doc) = dom::with_installed(doc, || execute(&scripts, frames, options.fps));
    let series = recorded.and_then(|frames| collect(&mut doc, &frames, options.fps));
    let (html, map) = dom::compact(&doc.html);
    *dom = html;
    let series = series?
        .into_iter()
        .filter_map(|(i, series)| map.get(&doc.ids[i]).map(|id| (*id, series)))
        .collect();
    Ok(Motion { series })
}

/// Evaluate the scripts, fire the load events and seek every frame. Each
/// frame is the change record `dom.js` returns.
fn execute(scripts: &[Script], frames: usize, fps: f64) -> Result<Vec<String>, String> {
    let mut cx = Context::default();
    cx.runtime_limits_mut().set_loop_iteration_limit(50_000_000);
    cx.runtime_limits_mut().set_recursion_limit(4_096);
    dom::register(&mut cx).map_err(js_error)?;
    cx.eval(Source::from_bytes(DOM)).map_err(js_error)?;
    let eval = |cx: &mut Context, code: &str| -> Result<String, String> {
        let value = cx.eval(Source::from_bytes(code)).map_err(js_error)?;
        cx.run_jobs().map_err(js_error)?;
        Ok(value.to_string(cx).map_err(js_error)?.to_std_string_escaped())
    };
    for script in scripts {
        eval(&mut cx, &script.source).map_err(|e| format!("{}: {e}", script.name))?;
        if let Some(major) = script.major {
            let version = eval(&mut cx, "typeof gsap === 'object' && gsap.version || ''")?;
            let found = version.split('.').next().and_then(|v| v.parse::<u32>().ok());
            if found.is_some_and(|found| found != major) {
                return Err(format!("{} names GSAP {major}, and the local copy is GSAP {version}", script.name));
            }
        }
    }
    eval(&mut cx, "__director.loaded()")?;
    eval(&mut cx, "__director.settle()")?;
    dom::with_doc(|d| {
        d.playing = true;
        Ok(())
    })
    .map_err(js_error)?;
    eval(&mut cx, "__director.play()")?;
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let t = f as f64 / fps;
        out.push(eval(&mut cx, &format!("__director.frame({t})")).map_err(|e| format!("at {t:.3}s: {e}"))?);
    }
    Ok(out)
}

/// `name: value` pairs of an inline style.
fn declarations(text: &str) -> Vec<(String, String)> {
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

fn put(decls: &mut Vec<(String, String)>, name: &str, value: Option<String>) {
    let at = decls.iter().position(|(n, _)| n == name);
    match (value, at) {
        (Some(v), Some(at)) => decls[at].1 = v,
        (Some(v), None) => decls.push((name.to_string(), v)),
        (None, Some(at)) => {
            decls.remove(at);
        }
        (None, None) => {}
    }
}

fn serialize(decls: &[(String, String)]) -> Option<String> {
    (!decls.is_empty()).then(|| decls.iter().map(|(n, v)| format!("{n}: {v}")).collect::<Vec<_>>().join("; "))
}

/// A GSAP write with no effect on screen.
fn invisible(name: &str, values: &[Option<String>]) -> bool {
    INVISIBLE.contains(&name)
        || name == "@data-svg-origin"
        || (matches!(name, "translate" | "rotate" | "scale")
            && values.iter().all(|v| v.as_deref().is_none_or(|v| v == "none")))
}

/// Turn the frame records into series, bake constant results into the
/// document and refuse varying ones without a native track. Keys are style
/// properties, `@name` attributes and `#text`.
fn collect(doc: &mut dom::Doc, frames: &[String], fps: f64) -> Result<HashMap<usize, Series>, String> {
    // Inline styles hold the setup state flushed when play began.
    let mut setup: HashMap<usize, Vec<(String, String)>> = HashMap::new();
    let mut series: HashMap<usize, Series> = HashMap::new();
    let mut current: BTreeMap<(usize, String), Option<String>> = BTreeMap::new();
    let malformed = || "GSAP frame record is malformed".to_string();
    for (f, frame) in frames.iter().enumerate() {
        if !frame.is_empty() {
            let parts: Vec<&str> = frame.split('\u{1}').collect();
            if !parts.len().is_multiple_of(3) {
                return Err(malformed());
            }
            for entry in parts.chunks(3) {
                let i: usize = entry[0].parse().map_err(|_| malformed())?;
                let key = entry[1].to_string();
                let value = (entry[2] != "\u{2}").then(|| entry[2].to_string());
                let slot = (i, key.clone());
                if !current.contains_key(&slot) {
                    let before = if let Some(name) = key.strip_prefix('@') {
                        doc.attr(i, name)?
                    } else if key == "#text" {
                        Some(doc.text(i)?)
                    } else {
                        if let std::collections::hash_map::Entry::Vacant(e) = setup.entry(i) {
                            e.insert(declarations(&doc.attr(i, "style")?.unwrap_or_default()));
                        }
                        setup[&i]
                            .iter().find(|(n, _)| *n == key).map(|(_, v)| v.clone())
                    };
                    series.entry(i).or_default().insert(key.clone(), vec![before.clone(); f]);
                    current.insert(slot.clone(), before);
                }
                current.insert(slot, value);
            }
        }
        for ((i, key), value) in &current {
            series.get_mut(i).and_then(|s| s.get_mut(key)).ok_or_else(malformed)?.push(value.clone());
        }
    }

    // A write to an element's only text node is a write to its text.
    let mut nodes: Vec<usize> = series.keys().copied().collect();
    nodes.sort_unstable();
    for i in nodes {
        let Some(owner) = doc.text_owner(i)? else { continue };
        let mut node = series.remove(&i).ok_or_else(malformed)?;
        let values = node.remove("#text").ok_or_else(malformed)?;
        let entry = series.entry(owner).or_default();
        if entry.contains_key("#text") {
            return Err(format!("GSAP writes the text of {} twice in one frame", doc.label(owner)));
        }
        entry.insert("#text".into(), values);
    }

    // GSAP clears independent transforms and hints on every target it parses.
    for i in 0..doc.ids.len() {
        let Ok(Some(style)) = doc.attr(i, "style") else { continue };
        let mut decls = declarations(&style);
        let before = decls.len();
        decls.retain(|(name, value)| !invisible(name, &[Some(value.clone())]));
        if decls.len() != before {
            doc.write_attr(i, "style", serialize(&decls))?;
        }
    }

    let mut out = HashMap::new();
    let mut elements: Vec<_> = series.into_iter().collect();
    elements.sort_by_key(|(i, _)| *i);
    for (i, mut props) in elements {
        let label = doc.label(i);
        props.retain(|name, values| !invisible(name, values));
        let mut decls = declarations(&doc.attr(i, "style")?.unwrap_or_default());
        if let Some(visibility) = props.get_mut("visibility") {
            for v in visibility.iter_mut() {
                if v.as_deref() == Some("inherit") {
                    *v = Some("visible".into());
                }
            }
        }
        // autoAlpha: hidden only while transparent draws the same as visible.
        if let Some(visibility) = props.get("visibility") {
            let opacity = |f: usize| -> Option<f64> {
                match props.get("opacity") {
                    Some(series) => series[f].as_deref(),
                    None => decls.iter().find(|(n, _)| n == "opacity").map(|(_, v)| v.as_str()),
                }
                .and_then(|v| v.trim().parse().ok())
            };
            let varies = visibility.iter().any(|v| v != &visibility[0]);
            let transparent = visibility
                .iter()
                .enumerate()
                .all(|(f, v)| v.as_deref() != Some("hidden") || opacity(f) == Some(0.0));
            if varies && transparent {
                props.remove("visibility");
                put(&mut decls, "visibility", Some("visible".into()));
            }
        }
        // GSAP writes borderRadius as four longhands; equal corners form the
        // native uniform radius.
        let corners = CORNERS.map(|c| props.get(c));
        if let [Some(a), Some(b), Some(c), Some(d)] = corners {
            if a == b && a == c && a == d {
                let values = a.clone();
                for corner in CORNERS {
                    props.remove(corner);
                    put(&mut decls, corner, None);
                }
                props.insert("border-radius".into(), values);
            }
        }
        // Before GSAP writes a transform the cascade one shows. When that has
        // no component form, its matrix in GSAP's own decomposition does.
        if let Some(values) = props.get_mut("transform") {
            if values.iter().any(Option::is_none) {
                if let Some(probed) = doc.probed(i)? {
                    let css = probed.css.get("transform").map_or("none", String::as_str);
                    if css != "none" && decompose(css).is_err() {
                        let fill = components(&probed.transform)
                            .ok_or_else(|| format!("GSAP animates {label} over the 3D transform '{css}'"))?;
                        for v in values.iter_mut().filter(|v| v.is_none()) {
                            *v = Some(fill.clone());
                        }
                    }
                }
            }
        }
        let mut kept = Series::new();
        // Values that vary inside an SVG become SMIL on the element.
        let in_svg = doc.svg_owner(i)?.is_some();
        let svg_root = doc.is_svg(i)?;
        let mut smil = Vec::new();
        for (key, values) in props {
            let first = values[0].clone();
            let constant = values.iter().all(|v| *v == first);
            if let Some(name) = key.strip_prefix('@') {
                if constant {
                    doc.write_attr(i, name, first)?;
                } else if in_svg || (svg_root && name == "viewBox") {
                    let (base, animate) = svg_animation(name, &values, fps)?;
                    doc.write_attr(i, name, Some(base))?;
                    smil.extend(animate);
                } else {
                    return Err(format!("GSAP animates the '{name}' attribute of {label}, which only SVG content animates"));
                }
                continue;
            }
            if in_svg && !constant && key != "#text" {
                let (base, animate) = svg_animation(&key, &values, fps)?;
                put(&mut decls, &key, Some(base));
                smil.extend(animate);
                continue;
            }
            if key == "#text" {
                if in_svg && !constant {
                    return Err(format!("GSAP animates the text of SVG {label}; SVG text keeps its markup"));
                }
                // The first frame's text becomes the element's own text.
                let first = first.unwrap_or_default();
                if doc.text(i)? != first {
                    doc.write_text(i, &first)?;
                }
                if !constant {
                    kept.insert(key, values);
                }
                continue;
            }
            if constant {
                put(&mut decls, &key, first);
                continue;
            }
            // GSAP sets transformOrigin once when a tween starts.
            if key == "transform-origin" {
                let set: Vec<_> = values.iter().flatten().collect();
                if set.iter().all(|v| *v == set[0]) {
                    put(&mut decls, &key, Some(set[0].clone()));
                    continue;
                }
            }
            if !NATIVE.contains(&key.as_str()) {
                let shown = |v: &Option<String>| v.as_deref().map_or("unset".into(), |v| format!("'{v}'"));
                let change = values.iter().find(|v| **v != first).unwrap_or(&first);
                return Err(format!(
                    "GSAP animates '{key}' on {label} from {} to {}; native tracks cover {}",
                    shown(&first),
                    shown(change),
                    NATIVE.join(", ")
                ));
            }
            // The first frame becomes the base unless some frame falls back
            // to the cascade.
            let base = if values.iter().all(Option::is_some) { first } else { None };
            put(&mut decls, &key, base);
            kept.insert(key, values);
        }
        doc.write_attr(i, "style", serialize(&decls))?;
        for animate in smil {
            doc.append_svg(i, "animate", animate)?;
        }
        if !kept.is_empty() {
            out.insert(i, kept);
        }
    }
    Ok(out)
}

/// Element attributes in order.
type Attrs = Vec<(String, String)>;

/// Properties whose SVG values are colors.
const SVG_COLORS: &[&str] = &["fill", "stroke", "stop-color", "flood-color", "lighting-color", "color"];

/// The base value and the `<animate>` attributes that replay one value GSAP
/// varies on an SVG element. A value that only fills in from unset needs none. Times are on the composition clock, which
/// `director-clock` tells the SMIL compiler. Frames whose values share one
/// numeric skeleton interpolate linearly between swing-door keys; other values
/// change on their frames.
fn svg_animation(
    name: &str,
    values: &[Option<String>],
    fps: f64,
) -> Result<(String, Option<Attrs>), String> {
    let identity = (name == "transform").then(|| "matrix(1,0,0,1,0,0)".to_string());
    let mut last = values.iter().flatten().next().cloned();
    let mut texts = Vec::with_capacity(values.len());
    for v in values {
        if v.is_some() {
            last = v.clone();
        }
        texts.push(match v {
            // `0px, 9px` and `2px,7px` share one skeleton.
            Some(v) => v.split(',').map(|p| p.split_whitespace().collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join(","),
            None => identity.clone().or_else(|| last.clone()).unwrap_or_default(),
        });
    }
    if name == "transform" {
        for t in texts.iter_mut() {
            if let Some(m) = svg_matrix(t) {
                *t = format!("matrix({})", m.map(|v| short(round(v))).join(","));
            }
        }
    }
    if name == "d" {
        for t in texts.iter_mut() {
            if let Some(d) = path_data(t) {
                *t = d;
            }
        }
    }
    let black = Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    if SVG_COLORS.contains(&name) && texts.iter().all(|t| color(t, black).is_ok()) {
        for t in texts.iter_mut() {
            let c = color(t, black)?;
            let byte = |v: f32| short((v as f64 * 255.0).round());
            *t = format!("rgba({},{},{},{})", byte(c.r), byte(c.g), byte(c.b), short(round(c.a as f64)));
        }
    }
    if texts.iter().any(|t| t.contains(';')) {
        return Err(format!("GSAP sets SVG '{name}' to a value with ';'"));
    }
    let base = texts[0].clone();
    if texts.iter().all(|t| *t == base) {
        return Ok((base, None));
    }
    let lists: Vec<(String, Vec<f64>)> = texts.iter().map(|t| skeleton(t)).collect();
    let linear = !lists[0].1.is_empty() && lists.iter().all(|(s, _)| *s == lists[0].0);
    let mut attrs = vec![("attributeName".to_string(), name.to_string())];
    let (frames, span) = if linear {
        let samples: Vec<&[f64]> = lists.iter().map(|(_, n)| n.as_slice()).collect();
        let frames = keep(&samples, 5e-4);
        let span = frames[frames.len() - 1] - frames[0];
        (frames, span)
    } else {
        let frames: Vec<usize> = (1..texts.len()).filter(|&f| texts[f] != texts[f - 1]).collect();
        let span = (texts.len() - 1 - frames[0]).max(1);
        attrs.push(("calcMode".into(), "discrete".into()));
        (frames, span)
    };
    let start = frames[0];
    let values: Vec<&str> = frames.iter().map(|&k| texts[k].as_str()).collect();
    let times: Vec<String> = frames.iter().map(|&k| short(round((k - start) as f64 / span as f64))).collect();
    attrs.push(("values".into(), values.join(";")));
    attrs.push(("keyTimes".into(), times.join(";")));
    attrs.push(("begin".into(), format!("{}s", short(start as f64 / fps))));
    attrs.push(("dur".into(), format!("{}s", short(span as f64 / fps))));
    attrs.push(("fill".into(), "freeze".into()));
    attrs.push(("director-clock".into(), "composition".into()));
    Ok((base, Some(attrs)))
}

/// Path data with every segment's command written out, numbers split by
/// spaces and `Z` closing, so MorphSVG's frames and the end path it writes back
/// share one skeleton. `None` leaves data this does not read untouched.
fn path_data(d: &str) -> Option<String> {
    let (shape, nums) = skeleton(d);
    let mut nums = nums.into_iter();
    let (mut out, mut cmd, mut args) = (Vec::<String>::new(), None::<char>, Vec::<f64>::new());
    let flush = |out: &mut Vec<String>, cmd: Option<char>, args: &mut Vec<f64>| -> Option<()> {
        let Some(c) = cmd else {
            return args.is_empty().then_some(());
        };
        let n = match c.to_ascii_lowercase() {
            'z' => 0,
            'h' | 'v' => 1,
            'm' | 'l' | 't' => 2,
            's' | 'q' => 4,
            'c' => 6,
            'a' => 7,
            _ => return None,
        };
        if n == 0 {
            return args.is_empty().then(|| out.push("Z".into()));
        }
        if args.is_empty() || !args.len().is_multiple_of(n) {
            return None;
        }
        for (k, seg) in args.chunks(n).enumerate() {
            // Coordinates after a moveto are implicit linetos.
            let letter = match (k, c) {
                (0, _) => c,
                (_, 'M') => 'L',
                (_, 'm') => 'l',
                _ => c,
            };
            let list: Vec<String> = seg.iter().map(|v| v.to_string()).collect();
            out.push(format!("{letter}{}", list.join(" ")));
        }
        args.clear();
        Some(())
    };
    for ch in shape.chars() {
        match ch {
            SLOT => args.push(nums.next()?),
            c if c.is_ascii_alphabetic() => {
                flush(&mut out, cmd, &mut args)?;
                cmd = Some(c);
            }
            c if c.is_whitespace() || c == ',' => {}
            _ => return None,
        }
    }
    flush(&mut out, cmd, &mut args)?;
    Some(out.join(" "))
}

/// Marks a number's place in a [`skeleton`].
pub(crate) const SLOT: char = '\u{1}';

/// `text` with each number replaced by [`SLOT`], and the numbers in order. Digits
/// inside a word such as `grad1` stay text; a one-letter path command does not.
pub(crate) fn skeleton(text: &str) -> (String, Vec<f64>) {
    let b = text.as_bytes();
    let (mut out, mut nums, mut i) = (String::new(), Vec::new(), 0);
    let digit = |j: usize| b.get(j).is_some_and(u8::is_ascii_digit);
    let alpha = |j: Option<usize>| j.and_then(|j| b.get(j)).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_');
    while i < b.len() {
        let c = b[i];
        let sign = matches!(c, b'-' | b'+') && (digit(i + 1) || (b.get(i + 1) == Some(&b'.') && digit(i + 2)));
        let starts = c.is_ascii_digit() || (c == b'.' && digit(i + 1)) || sign;
        let in_word = alpha(i.checked_sub(1)) && alpha(i.checked_sub(2));
        if !starts || in_word {
            let end = i + text[i..].chars().next().map_or(1, char::len_utf8);
            out.push_str(&text[i..end]);
            i = end;
            continue;
        }
        let mut j = i + usize::from(sign);
        while digit(j) {
            j += 1;
        }
        if b.get(j) == Some(&b'.') && digit(j + 1) {
            j += 1;
            while digit(j) {
                j += 1;
            }
        }
        if matches!(b.get(j), Some(b'e' | b'E')) {
            let k = j + 1 + usize::from(matches!(b.get(j + 1), Some(b'-' | b'+')));
            if digit(k) {
                j = k;
                while digit(j) {
                    j += 1;
                }
            }
        }
        match text[i..j].parse() {
            Ok(n) => {
                nums.push(n);
                out.push(SLOT);
            }
            Err(_) => out.push_str(&text[i..j]),
        }
        i = j;
    }
    (out, nums)
}

/// A number as short decimal text.
fn short(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// An SVG `transform` list as its matrix `[a, b, c, d, e, f]`.
fn svg_matrix(text: &str) -> Option<[f64; 6]> {
    let mul = |a: [f64; 6], b: [f64; 6]| {
        [
            a[0] * b[0] + a[2] * b[1],
            a[1] * b[0] + a[3] * b[1],
            a[0] * b[2] + a[2] * b[3],
            a[1] * b[2] + a[3] * b[3],
            a[0] * b[4] + a[2] * b[5] + a[4],
            a[1] * b[4] + a[3] * b[5] + a[5],
        ]
    };
    let mut m = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut rest = text.trim();
    while !rest.is_empty() {
        let open = rest.find('(')?;
        let close = rest.find(')')?;
        let name = rest[..open].trim();
        let v: Vec<f64> = rest[open + 1..close]
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .map(|s| s.parse().ok())
            .collect::<Option<_>>()?;
        let y = v.get(1).copied();
        let t = match (name, v.len()) {
            ("matrix", 6) => [v[0], v[1], v[2], v[3], v[4], v[5]],
            ("translate", 1 | 2) => [1.0, 0.0, 0.0, 1.0, v[0], y.unwrap_or(0.0)],
            ("scale", 1 | 2) => [v[0], 0.0, 0.0, y.unwrap_or(v[0]), 0.0, 0.0],
            ("rotate", 1 | 3) => {
                let (s, c) = v[0].to_radians().sin_cos();
                let r = [c, s, -s, c, 0.0, 0.0];
                if v.len() == 3 {
                    mul(mul([1.0, 0.0, 0.0, 1.0, v[1], v[2]], r), [1.0, 0.0, 0.0, 1.0, -v[1], -v[2]])
                } else {
                    r
                }
            }
            ("skewX", 1) => [1.0, 0.0, v[0].to_radians().tan(), 1.0, 0.0, 0.0],
            ("skewY", 1) => [1.0, v[0].to_radians().tan(), 0.0, 1.0, 0.0, 0.0],
            _ => return None,
        };
        m = mul(m, t);
        rest = rest[close + 1..].trim_start_matches(|c: char| c == ',' || c.is_whitespace());
    }
    Some(m)
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
    fn dims(&self) -> &[f64] {
        match self {
            Self::Num(n) => std::slice::from_ref(n),
            Self::Color(c) => c,
        }
    }
    fn timeline(self) -> TimelineValue {
        match self {
            Self::Num(n) => TimelineValue::Number(round(n)),
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
impl AsRef<[f64]> for Val {
    fn as_ref(&self) -> &[f64] {
        self.dims()
    }
}
fn round(v: f64) -> f64 {
    let r = (v * 1e4).round() / 1e4;
    if r == 0.0 {
        0.0
    } else {
        r
    }
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
        _ => Parts::decompose(&paint.transform)
            .and_then(|p| p.get(channel))
            .map(|v| Val::Num(v as f64))
            .ok_or_else(|| {
                "GSAP transforms need a base transform of translations, rotations, one skew and scales"
                    .to_string()
            }),
    }
}

/// How far a key line may stray from a sample in the channel's unit.
fn tolerance(channel: &str) -> f64 {
    match channel {
        "opacity" | "color" | "background-color" => 5e-4,
        "scale-x" | "scale-y" => 1e-4,
        "translate-x-percent" | "translate-y-percent" => 0.01,
        c if c.starts_with("rotate") || c.starts_with("skew") => 0.01,
        _ => 0.02,
    }
}

/// The samples a piecewise-linear line through them keeps, each within
/// `eps` of the line on every dimension (swing door from each anchor).
fn keep<T: AsRef<[f64]> + PartialEq>(samples: &[T], eps: f64) -> Vec<usize> {
    let n = samples.len();
    let dims = samples[0].as_ref().len();
    let mut kept = vec![0];
    let mut anchor = 0;
    let mut lo = vec![f64::NEG_INFINITY; dims];
    let mut hi = vec![f64::INFINITY; dims];
    let mut j = 1;
    while j < n {
        let dt = (j - anchor) as f64;
        let (a, b) = (samples[anchor].as_ref(), samples[j].as_ref());
        let fits = (0..dims).all(|d| {
            let slope = (b[d] - a[d]) / dt;
            slope >= lo[d] - 1e-12 && slope <= hi[d] + 1e-12
        });
        if fits {
            for d in 0..dims {
                lo[d] = lo[d].max((b[d] - eps - a[d]) / dt);
                hi[d] = hi[d].min((b[d] + eps - a[d]) / dt);
            }
            j += 1;
        } else {
            anchor = j - 1;
            kept.push(anchor);
            lo.fill(f64::NEG_INFINITY);
            hi.fill(f64::INFINITY);
        }
    }
    if *kept.last().unwrap() != n - 1 {
        kept.push(n - 1);
    }
    // A track holds its first and last values, so equal end keys add nothing.
    while kept.len() > 1 && samples[kept[kept.len() - 1]] == samples[kept[kept.len() - 2]] {
        kept.pop();
    }
    while kept.len() > 1 && samples[kept[0]] == samples[kept[1]] {
        kept.remove(0);
    }
    kept
}

fn sample(channel: &str, text: &str) -> Result<Val, String> {
    match channel {
        "opacity" => text.trim().parse().map(Val::Num).map_err(|_| format!("GSAP opacity '{text}'")),
        "color" | "background-color" => {
            color(text.trim(), Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }).map(Val::color)
        }
        _ => text
            .trim()
            .strip_suffix("px")
            .and_then(|n| n.trim().parse().ok())
            .map(Val::Num)
            .ok_or_else(|| format!("GSAP animates {channel} as '{text}'; native tracks take px")),
    }
}

/// GSAP's component form of a 2D `matrix(...)`: rotate, skewX and scale
/// recompose it in that order.
fn components(matrix: &str) -> Option<String> {
    let inner = matrix.trim().strip_prefix("matrix(")?.strip_suffix(')')?;
    let v: Vec<f64> = inner.split(',').map(|n| n.trim().parse().ok()).collect::<Option<_>>()?;
    let [a, b, mut c, mut d, e, f] = v[..] else { return None };
    // A mirrored matrix decomposes as its unmirrored form with scaleY negated.
    let mirrored = a * d - b * c < 0.0;
    if mirrored {
        (c, d) = (-c, -d);
    }
    let sx = a.hypot(b);
    let mut sy = c.hypot(d);
    let rotation = if a != 0.0 || b != 0.0 { b.atan2(a).to_degrees() } else { 0.0 };
    let skew = if c != 0.0 || d != 0.0 { c.atan2(d).to_degrees() + rotation } else { 0.0 };
    sy *= skew.to_radians().cos().abs();
    if mirrored {
        sy = -sy;
    }
    Some(format!(
        "translate({e}px, {f}px) rotate({rotation}deg) skewX({skew}deg) scale({sx}, {sy})"
    ))
}

fn decompose(text: &str) -> Result<Parts, String> {
    use lightningcss::{properties::transform::TransformList, traits::Parse};
    TransformList::parse_string(text)
        .ok()
        .and_then(|list| Parts::decompose(&list))
        .ok_or_else(|| format!("GSAP transform '{text}' has no component form"))
}

/// Native timeline tracks for one element's sampled properties. Keys sit on
/// frames and interpolate linearly.
pub(crate) fn tracks(
    series: &Series,
    target: &str,
    base: &dyn Fn(&str) -> Result<Val, String>,
    fps: f64,
) -> Result<Vec<TimelineTrack>, String> {
    let mut channels: Vec<(&str, Vec<Val>)> = Vec::new();
    let mut out = Vec::new();
    for (property, values) in series {
        if property == "#text" {
            out.push(text_track(values, target, fps));
            continue;
        }
        if property == "transform" {
            let parts = values
                .iter()
                .map(|v| v.as_deref().map(decompose).transpose())
                .collect::<Result<Vec<_>, _>>()?;
            for channel in COMPONENTS {
                let base = base(channel)?;
                let samples = parts
                    .iter()
                    .map(|p| p.map_or(base, |p| Val::Num(p.get(channel).unwrap_or_default() as f64)))
                    .collect();
                channels.push((channel, samples));
            }
            continue;
        }
        let channel = match property.as_str() {
            "border-radius" => "border-top-left-radius",
            other => other,
        };
        let base = base(channel)?;
        let samples = values
            .iter()
            .map(|v| v.as_deref().map_or(Ok(base), |text| sample(channel, text)))
            .collect::<Result<Vec<_>, _>>()?;
        channels.push((channel, samples));
    }
    for (channel, samples) in channels {
        let eps = tolerance(channel);
        let base = base(channel)?;
        let differs = samples
            .iter()
            .any(|v| v.dims().iter().zip(base.dims()).any(|(a, b)| (a - b).abs() > eps));
        if !differs {
            continue;
        }
        let keyframes = keep(&samples, eps)
            .into_iter()
            .map(|f| TimelineKeyframe {
                time: f as f64 / fps,
                value: samples[f].timeline(),
                ease: None,
            })
            .collect();
        out.push(TimelineTrack {
            target: target.to_string(),
            property: channel.to_string(),
            keyframes,
        });
    }
    Ok(out)
}

/// A `text` track keyed on each frame whose text differs from the frame
/// before. The element shows its own text, the first frame's, until then.
fn text_track(values: &[Option<String>], target: &str, fps: f64) -> TimelineTrack {
    let text = |f: usize| values[f].clone().unwrap_or_default();
    let keyframes = (1..values.len())
        .filter(|&f| text(f) != text(f - 1))
        .map(|f| TimelineKeyframe {
            time: f as f64 / fps,
            value: TimelineValue::Text(text(f)),
            ease: None,
        })
        .collect();
    TimelineTrack {
        target: target.to_string(),
        property: "text".into(),
        keyframes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swing_door_keeps_corners_within_tolerance() {
        let ramp: Vec<Val> = (0..=30)
            .map(|f| Val::Num(if f <= 10 { 0.0 } else if f <= 20 { (f - 10) as f64 * 10.0 } else { 100.0 }))
            .collect();
        assert_eq!(keep(&ramp, 0.02), vec![10, 20]);
        let curve: Vec<Val> = (0..=60).map(|f| Val::Num((f as f64 / 60.0).powi(2) * 500.0)).collect();
        let kept = keep(&curve, 0.02);
        for pair in kept.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            for f in a..=b {
                let t = (f - a) as f64 / (b - a) as f64;
                let (Val::Num(va), Val::Num(vb), Val::Num(v)) = (curve[a], curve[b], curve[f]) else { unreachable!() };
                assert!((va + (vb - va) * t - v).abs() <= 0.02 + 1e-9);
            }
        }
    }

    #[test]
    fn declarations_split_on_top_level_semicolons() {
        let decls = declarations("color: red; background: url(\"a;b\") ; transform: translate(1px, 2px) !important;");
        assert_eq!(decls.len(), 3);
        assert_eq!(decls[1].1, "url(\"a;b\")");
        assert_eq!(decls[2].1, "translate(1px, 2px)");
    }
}
