//! GSAP at compile time. The page scripts, GSAP included, run once in Boa
//! against a Rust-owned document (`dom.rs`, `dom.js`). The registered
//! timelines then seek to every frame, and what the scripts write while they
//! play becomes native `director-timeline/1` tracks.

mod dom;
pub(crate) mod ease;

use crate::Attr;
use crate::animation::{
    border_side, box_side, filter_default as default_amount, filter_scalar, filter_shadow_color, indexed, shadow_scalar, Parts,
    COMPONENTS, NO_TEXT_SHADOW,
    FILTERS as FILTER_CHANNELS, NO_BOX_SHADOW,
    variation_axis,
};
use crate::style::{AxisPos, BackgroundFit, Shadow};
use director_core::element::TextShadow;
use crate::assets::AssetStore;
use crate::style::{color, PaintStyle};
use crate::timeline::{TimelineKeyframe, TimelineTrack, TimelineValue};
use crate::CompileOptions;
use boa_engine::{Context, Source};
use director_core::types::Color;
use ego_tree::NodeId;
use scraper::{Html, Selector};
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

pub(crate) use dom::{matrix_text, Probe, Probed};

const DOM: &str = include_str!("dom.js");

/// One value per frame for each property a script animates, `None` where the
/// element falls back to its cascaded value.
pub(crate) type Series = BTreeMap<String, Vec<Option<String>>>;

/// Where a set jumps, by recorded property or transform channel: the frame
/// that first shows the new value and the jump time in seconds.
pub(crate) type Steps = BTreeMap<String, Vec<(usize, f64)>>;

/// What the page scripts animate, by element of the compiled document.
#[derive(Debug, Default)]
pub(crate) struct Motion {
    pub(crate) series: HashMap<NodeId, Series>,
    pub(crate) steps: HashMap<NodeId, Steps>,
    /// The composition time of each sample in the series.
    pub(crate) times: Vec<f64>,
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
    "filter",
    "box-shadow",
    "clip-path",
    "text-shadow",
    "border-color",
    "border-top-color",
    "border-right-color",
    "border-bottom-color",
    "border-left-color",
    "border-top-width",
    "border-right-width",
    "border-bottom-width",
    "border-left-width",
    "background-position",
    "background-size",
    "font-size",
    "letter-spacing",
    "word-spacing",
    "font-stretch",
    "font-variation-settings",
    "-webkit-text-stroke-color",
    "-webkit-text-stroke-width",
    "line-height",
    "padding",
    "margin",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
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
        let kind = script.value().attr_text("type").map(str::trim).unwrap_or("");
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
        let Some(src) = script.value().attr_text("src") else {
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

/// Run the page scripts and record every frame, and with motion blur every
/// shutter sample, given as `(samples per frame, shutter angle)`. Constant
/// results bake into `dom`, which is replaced by its compacted form; varying
/// ones come back as series.
pub(crate) fn run(
    dom: &mut Html,
    options: &CompileOptions,
    blur: (u32, f32),
    assets: &Arc<AssetStore>,
    beats: &Rc<crate::beats::Beats>,
) -> Result<Motion, String> {
    let scripts = scripts(dom, options, assets)?;
    if scripts.is_empty() {
        return Ok(Motion::default());
    }
    let html = std::mem::replace(dom, Html::new_document());
    let frames = (options.duration * options.fps).ceil() as usize + 1;
    let times = sample_times(frames, options.fps, blur);
    let (recorded, mut doc) = record(html, &scripts, &times, options, assets, beats);
    let series = recorded.and_then(|records| collect(&mut doc, &records, &times, options.fps));
    let (html, map) = dom::compact(&doc.html, &Default::default());
    *dom = html;
    let (series, steps) = series?;
    let series = series
        .into_iter()
        .filter_map(|(i, series)| map.get(&doc.ids[i]).map(|id| (*id, series)))
        .collect();
    let steps = steps
        .into_iter()
        .filter_map(|(i, steps)| map.get(&doc.ids[i]).map(|id| (*id, steps)))
        .collect();
    Ok(Motion { series, steps, times })
}

/// Probe results shared by script runs, keyed by the probed markup's hash
/// and length. Runs replay the same setup, so the first run to reach a
/// probe computes it and the others wait for it.
type Probes = Mutex<HashMap<(u64, usize), Arc<OnceLock<Result<Probe, String>>>>>;

/// The DOM the scripts run against, with layout reads probing `html`.
fn document(
    html: Html,
    options: &CompileOptions,
    assets: &Arc<AssetStore>,
    beats: Rc<crate::beats::Beats>,
    shared: Option<Arc<Probes>>,
) -> dom::Doc {
    let prober: dom::Prober = {
        let options = options.clone();
        let assets = assets.clone();
        let beats = beats.clone();
        Box::new(move |html: &Html| {
            let probe = || crate::compiler::probe(html, &options, &assets, &beats).map_err(|e| e.to_string());
            let Some(shared) = &shared else { return probe() };
            let markup = html.html();
            let mut hash = std::hash::DefaultHasher::new();
            markup.hash(&mut hash);
            let cell = shared.lock().unwrap().entry((hash.finish(), markup.len())).or_default().clone();
            cell.get_or_init(probe).clone()
        })
    };
    dom::Doc::new(html, prober, (options.width as f32, options.height as f32), beats)
}

/// The fewest samples one script run records. Each run replays setup, which
/// costs about as much as fifty samples, so a run records a few times that.
const RUN_SAMPLES: usize = 128;
/// The most runs a page splits into, one per core on a 24-thread machine.
/// Each run past the first replays setup and starts mid-timeline, so fewer
/// runs save setup and start fewer runs inside overlapping tweens.
const RUNS: usize = 24;

/// Contiguous sample ranges, one per script run. The split depends only on
/// the sample count, so every machine records the same samples.
fn runs(samples: usize) -> Vec<Range<usize>> {
    let size = samples.div_ceil(RUNS).max(RUN_SAMPLES);
    (0..samples).step_by(size).map(|start| start..(start + size).min(samples)).collect()
}

/// Every sample's change record, and the document after setup. Ranges past
/// the first run on worker threads, each against its own copy of the
/// setup document. Seeking is deterministic, so they record what one run
/// would, unless a script draws random numbers while the timeline plays;
/// then one run records them all.
fn record(
    html: Html,
    scripts: &[Script],
    times: &[f64],
    options: &CompileOptions,
    assets: &Arc<AssetStore>,
    beats: &Rc<crate::beats::Beats>,
) -> (Result<Vec<String>, String>, dom::Doc) {
    let ranges = runs(times.len());
    if ranges.len() == 1 {
        let doc = document(html, options, assets, beats.clone(), None);
        let (run, doc) = dom::with_installed(doc, || execute(scripts, times, 0..times.len()));
        return (run.map(|run| run.records), doc);
    }
    let pristine = html.clone();
    let probes = Arc::new(Probes::default());
    let next = AtomicUsize::new(1);
    let done: Mutex<Vec<Option<Result<Run, String>>>> = Mutex::new((0..ranges.len()).map(|_| None).collect());
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get()).min(ranges.len()).saturating_sub(1);
    let (first, doc) = std::thread::scope(|scope| {
        for _ in 0..workers {
            let (html, beats) = (pristine.clone(), beats.fork());
            let (ranges, next, done, probes) = (&ranges, &next, &done, &probes);
            std::thread::Builder::new()
                .stack_size(64 << 20)
                .spawn_scoped(scope, move || loop {
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    let Some(range) = ranges.get(k) else { break };
                    let doc = document(html.clone(), options, assets, Rc::new(beats.fork()), Some(probes.clone()));
                    let (run, _) = dom::with_installed(doc, || execute(scripts, times, range.clone()));
                    done.lock().unwrap()[k] = Some(run);
                })
                .expect("script worker thread");
        }
        let doc = document(html, options, assets, beats.clone(), Some(probes.clone()));
        let first = dom::with_installed(doc, || execute(scripts, times, ranges[0].clone()));
        loop {
            let k = next.fetch_add(1, Ordering::Relaxed);
            let Some(range) = ranges.get(k) else { break };
            let doc = document(pristine.clone(), options, assets, Rc::new(beats.fork()), Some(probes.clone()));
            let (run, _) = dom::with_installed(doc, || execute(scripts, times, range.clone()));
            done.lock().unwrap()[k] = Some(run);
        }
        first
    });
    let mut runs = vec![first];
    runs.extend(done.into_inner().unwrap().into_iter().skip(1).map(|run| run.expect("every range ran")));
    let runs: Result<Vec<Run>, String> = runs.into_iter().collect();
    match runs {
        Ok(runs) if runs.iter().all(|run| run.draws == 0) => (Ok(runs.into_iter().flat_map(|run| run.records).collect()), doc),
        Ok(_) => {
            let doc = document(pristine, options, assets, beats.clone(), None);
            let (run, doc) = dom::with_installed(doc, || execute(scripts, times, 0..times.len()));
            (run.map(|run| run.records), doc)
        }
        Err(e) => (Err(e), doc),
    }
}

/// The seek times: every frame, and with more than one sample per frame the
/// renderer's sub-frame times spread over the shutter that opens at each
/// frame, so motion blur reads
/// sampled values instead of a line between frames.
fn sample_times(frames: usize, fps: f64, (samples, angle): (u32, f32)) -> Vec<f64> {
    let mut times: Vec<f64> = (0..frames).map(|f| f as f64 / fps).collect();
    if samples > 1 {
        let shutter = (1.0 / fps) * (angle.clamp(0.0, 360.0) as f64 / 360.0);
        for f in 0..frames {
            let start = f as f64 / fps;
            times.extend((0..samples).map(|k| start + (k as f64 / (samples - 1) as f64) * shutter));
        }
        times.sort_by(f64::total_cmp);
        times.dedup_by(|b, a| (*b - *a).abs() < 1e-9);
    }
    times
}

/// One script run: the change record `dom.js` returns for each sample, and
/// how many random numbers the scripts drew while the timeline played.
struct Run {
    records: Vec<String>,
    draws: u64,
}

/// Evaluate the scripts, fire the load events and seek each sample time in
/// `range`. A range that starts mid-timeline first seeks the sample before
/// it, and before that the last earlier sample inside each finished
/// animation that draws from `onUpdate`, so the run starts from the state
/// earlier samples leave.
fn execute(scripts: &[Script], times: &[f64], range: Range<usize>) -> Result<Run, String> {
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
    let frame = |cx: &mut Context, t: f64| eval(cx, &format!("__director.frame({t})")).map_err(|e| format!("at {t:.3}s: {e}"));
    if range.start > 0 {
        let earlier = &times[..range.start];
        let before = earlier[earlier.len() - 1];
        let spans: Vec<f64> = serde_json::from_str(&eval(&mut cx, "__director.drawers()")?).map_err(|e| e.to_string())?;
        // The reverse half of each seek renders an animation while its end
        // is at least a microsecond past the sample.
        let mut warm: Vec<f64> = spans
            .as_chunks::<2>()
            .0
            .iter()
            .filter_map(|span| {
                let last = earlier.partition_point(|&t| t + 1e-6 <= span[1] + 1e-9);
                let t = *earlier.get(last.checked_sub(1)?)?;
                (t >= span[0] - 1e-6 && t < before).then_some(t)
            })
            .collect();
        warm.sort_by(f64::total_cmp);
        warm.dedup();
        warm.push(before);
        for t in warm {
            frame(&mut cx, t)?;
        }
    }
    let mut records = Vec::with_capacity(range.len());
    for &t in &times[range] {
        records.push(frame(&mut cx, t)?);
    }
    let draws = eval(&mut cx, "__director.draws()")?.parse().map_err(|_| "GSAP draw count is malformed".to_string())?;
    Ok(Run { records, draws })
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
type Recorded = (HashMap<usize, Series>, HashMap<usize, Steps>);

fn collect(doc: &mut dom::Doc, frames: &[String], times: &[f64], fps: f64) -> Result<Recorded, String> {
    // Inline styles hold the setup state flushed when play began.
    let mut setup: HashMap<usize, Vec<(String, String)>> = HashMap::new();
    let mut series: HashMap<usize, Series> = HashMap::new();
    let mut current: BTreeMap<(usize, String), Option<String>> = BTreeMap::new();
    let mut steps: HashMap<usize, Steps> = HashMap::new();
    let malformed = || "GSAP frame record is malformed".to_string();
    for (f, frame) in frames.iter().enumerate() {
        if !frame.is_empty() {
            let parts: Vec<&str> = frame.split('\u{1}').collect();
            if !parts.len().is_multiple_of(3) {
                return Err(malformed());
            }
            for entry in parts.chunks(3) {
                let i: usize = entry[0].parse().map_err(|_| malformed())?;
                if let Some(name) = entry[1].strip_prefix('!') {
                    let time: f64 = entry[2].parse().map_err(|_| malformed())?;
                    steps.entry(i).or_default().entry(name.to_string()).or_default().push((f, time));
                    continue;
                }
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
                if let Some(probed) = doc.probed(i, true)? {
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
        // Set times on one recorded key. A transform set names its channels.
        let jumps = |key: &str| -> Vec<(usize, f64)> {
            let transform = matches!(key, "@transform" | "transform");
            steps.get(&i).map_or_else(Vec::new, |steps| {
                steps
                    .iter()
                    .filter(|(name, _)| *name == key || transform && COMPONENTS.contains(&name.as_str()))
                    .flat_map(|(_, list)| list.iter().copied())
                    .collect()
            })
        };
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
                    let (base, animate) = svg_animation(name, &values, &jumps(&key), times, fps)?;
                    doc.write_attr(i, name, Some(base))?;
                    smil.extend(animate);
                } else {
                    return Err(format!("GSAP animates the '{name}' attribute of {label}, which only SVG content animates"));
                }
                continue;
            }
            if in_svg && !constant && key != "#text" {
                let (base, animate) = svg_animation(&key, &values, &jumps(&key), times, fps)?;
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
    steps.retain(|i, _| out.contains_key(i));
    Ok((out, steps))
}

/// Element attributes in order.
type Attrs = Vec<(String, String)>;

/// Properties whose SVG values are colors.
const SVG_COLORS: &[&str] = &["fill", "stroke", "stop-color", "flood-color", "lighting-color", "color"];

/// The base value and the `<animate>` attributes that replay one value GSAP
/// varies on an SVG element. A value that only fills in from unset needs none.
/// Times are on the composition clock, which `director-clock` tells the SMIL
/// compiler. Frames whose values share one numeric skeleton interpolate
/// linearly between swing-door keys; other values change on their frames. A
/// set in `jumps` (frame, time) holds the value before it until its time,
/// through two keys that share one key time.
fn svg_animation(
    name: &str,
    values: &[Option<String>],
    jumps: &[(usize, f64)],
    times: &[f64],
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
    // Jumps where the value changes, earliest per frame.
    let mut jumps: Vec<(usize, f64)> =
        jumps.iter().copied().filter(|&(f, _)| f > 0 && f < texts.len() && texts[f] != texts[f - 1]).collect();
    jumps.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    jumps.dedup_by_key(|j| j.0);
    let jump = |f: usize| jumps.iter().find(|j| j.0 == f).map(|j| j.1);
    let mut attrs = vec![("attributeName".to_string(), name.to_string())];
    // Keys as (seconds, frame whose value shows).
    let mut keys: Vec<(f64, usize)> = Vec::new();
    let end = if linear {
        let samples: Vec<&[f64]> = lists.iter().map(|(_, n)| n.as_slice()).collect();
        let mut start = 0;
        for piece in jumps.iter().map(Some).chain([None]) {
            let stop = piece.map_or(samples.len(), |&(f, _)| f);
            keys.extend(keep(&samples[start..stop], &times[start..stop], 5e-4).into_iter().map(|k| (times[start + k], start + k)));
            if let Some(&(f, time)) = piece {
                keys.push((time, f - 1));
                keys.push((time, f));
                start = f;
            }
        }
        keys.dedup_by(|b, a| (b.0 - a.0).abs() < 1e-9 && texts[b.1] == texts[a.1]);
        keys[keys.len() - 1].0
    } else {
        keys = (1..texts.len())
            .filter(|&f| texts[f] != texts[f - 1])
            .map(|f| (jump(f).unwrap_or(times[f]), f))
            .collect();
        attrs.push(("calcMode".into(), "discrete".into()));
        times[texts.len() - 1].max(keys[0].0 + 1.0 / fps)
    };
    let start = keys[0].0;
    let span = end - start;
    let values: Vec<&str> = keys.iter().map(|&(_, f)| texts[f].as_str()).collect();
    let times: Vec<String> = keys.iter().map(|&(t, _)| short((t - start) / span)).collect();
    attrs.push(("values".into(), values.join(";")));
    attrs.push(("keyTimes".into(), times.join(";")));
    attrs.push(("begin".into(), format!("{}s", short(start))));
    attrs.push(("dur".into(), format!("{}s", short(span))));
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
        "font-size" => Ok(Val::Num(paint.font_size as f64)),
        "letter-spacing" => Ok(Val::Num(paint.letter_spacing as f64)),
        "word-spacing" => Ok(Val::Num(paint.word_spacing as f64)),
        "font-stretch" => Ok(Val::Num(paint.font_stretch as f64)),
        "-webkit-text-stroke-color" => Ok(Val::color(paint.text_stroke_color.unwrap_or(paint.color))),
        "-webkit-text-stroke-width" => Ok(Val::Num(paint.text_stroke_width as f64)),
        c if variation_axis(c).is_some() => {
            let axis = variation_axis(c).unwrap_or_default();
            let set = paint.font_variations.iter().find(|v| v.axis == axis).map(|v| v.value);
            // Axes CSS drives from other properties start at those values.
            let derived = match axis {
                "wdth" => Some(paint.font_stretch),
                "wght" => Some(paint.font_weight as f32),
                "opsz" => Some(paint.font_size),
                "ital" | "slnt" => Some(0.0),
                _ => None,
            };
            set.or(derived)
                .map(|v| Val::Num(v as f64))
                .ok_or_else(|| format!("GSAP '{c}' needs the axis in the element's font-variation-settings"))
        }
        "line-height" => paint
            .line_height
            .map(|h| Val::Num(h as f64))
            .ok_or_else(|| "GSAP 'line-height' needs a line-height on the element".to_string()),
        c if box_side(c).is_some() => {
            let side = box_side(c).map_or(0, |(_, side)| side);
            let raw = if c.starts_with("padding") {
                [style.padding.top, style.padding.right, style.padding.bottom, style.padding.left][side].into_raw()
            } else {
                [style.margin.top, style.margin.right, style.margin.bottom, style.margin.left][side].into_raw()
            };
            length(raw, c)
        }
        "filter-drop-shadow-color" => Ok(filter_shadow_color(&paint.filters).map_or_else(|| filter_default(channel), Val::color)),
        c if FILTER_CHANNELS.contains(&c) => Ok(filter_scalar(&paint.filters, c).map_or_else(|| filter_default(c), |v| Val::Num(v as f64))),
        c if border_side(c).is_some() => {
            let side = border_side(c).map_or(0, |(side, _)| side);
            if c.ends_with("color") {
                Ok(Val::color(paint.border_colors[side]))
            } else {
                let border = [style.border.top, style.border.right, style.border.bottom, style.border.left];
                length(border[side].into_raw(), c)
            }
        }
        c if indexed(c).is_some() => Ok(match indexed(c) {
            Some(("box-shadow", i, field)) => shadow_val(paint.shadows.get(i).unwrap_or(&NO_BOX_SHADOW), field),
            Some(("text-shadow", i, field)) => text_shadow_val(paint.text_shadows.get(i).unwrap_or(&NO_TEXT_SHADOW), field),
            Some(("background-position", i, field)) => {
                let list = if field.starts_with('x') { &paint.background_position_x } else { &paint.background_position_y };
                let none = AxisPos { percent: 0.0, length: 0.0 };
                position_val(if list.is_empty() { none } else { list[i % list.len()] }, field)
            }
            Some(("background-size", i, field)) => {
                let list = &paint.background_size;
                let fit = if list.is_empty() { BackgroundFit::Auto } else { list[i % list.len()] };
                match size_axis(fit, field.starts_with("width")) {
                    Some((percent, v)) if percent == field.ends_with("percent") => Val::Num(v),
                    _ => return Err(format!("GSAP '{c}' needs a base size in its unit")),
                }
            }
            Some((_, i, field)) => {
                let point = paint.box_node.clip_path.as_ref().and_then(|p| p.get(i)).copied().unwrap_or_default();
                corner_val(point, field)
            }
            None => unreachable!(),
        }),
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
        "line-height" => 1e-3,
        "filter-blur" | "filter-drop-shadow-x" | "filter-drop-shadow-y" | "filter-drop-shadow-blur" => 0.02,
        "filter-hue-rotate" => 0.01,
        c if c.starts_with("filter-") => 5e-4,
        c if c.ends_with("-color") => 5e-4,
        c if c.starts_with("background-") && c.ends_with("-percent") => 0.01,
        c if c.starts_with("clip-path-") => 0.01,
        "scale-x" | "scale-y" => 1e-4,
        "translate-x-percent" | "translate-y-percent" => 0.01,
        c if c.starts_with("rotate") || c.starts_with("skew") => 0.01,
        _ => 0.02,
    }
}

/// The samples a piecewise-linear line through them keeps, each within
/// `eps` of the line on every dimension (swing door from each anchor).
fn keep<T: AsRef<[f64]> + PartialEq>(samples: &[T], times: &[f64], eps: f64) -> Vec<usize> {
    let n = samples.len();
    let dims = samples[0].as_ref().len();
    let mut kept = vec![0];
    let mut anchor = 0;
    let mut lo = vec![f64::NEG_INFINITY; dims];
    let mut hi = vec![f64::INFINITY; dims];
    let mut j = 1;
    while j < n {
        let dt = times[j] - times[anchor];
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

/// A filter channel's value where its function is absent.
fn filter_default(channel: &str) -> Val {
    if channel == "filter-drop-shadow-color" {
        Val::Color([0.0; 4])
    } else {
        Val::Num(default_amount(channel) as f64)
    }
}

const SHADOW_FIELDS: [&str; 6] = ["x", "y", "blur", "spread", "color", "inset"];

fn shadow_val(shadow: &Shadow, field: &str) -> Val {
    if field == "color" {
        Val::color(shadow.color)
    } else {
        Val::Num(shadow_scalar(shadow, field) as f64)
    }
}

const TEXT_SHADOW_FIELDS: [&str; 4] = ["x", "y", "blur", "color"];

fn text_shadow_val(shadow: &TextShadow, field: &str) -> Val {
    match field {
        "color" => Val::color(shadow.color),
        "x" => Val::Num(shadow.offset.0 as f64),
        "y" => Val::Num(shadow.offset.1 as f64),
        _ => Val::Num(shadow.blur as f64),
    }
}

/// A background position axis: its length in px, or its percent.
fn position_val(axis: AxisPos, field: &str) -> Val {
    Val::Num(if field.ends_with("percent") { axis.percent as f64 * 100.0 } else { axis.length as f64 })
}

/// One axis of a background size as `(percent, value)` in its track unit.
/// `auto`, keywords and mixed lengths have none.
fn size_axis(fit: BackgroundFit, width: bool) -> Option<(bool, f64)> {
    use taffy::style::CompactLength;
    let BackgroundFit::Lengths(w, h) = fit else { return None };
    let raw = if width { w } else { h }?.into_raw();
    match raw.tag() {
        CompactLength::LENGTH_TAG => Some((false, raw.value() as f64)),
        CompactLength::PERCENT_TAG => Some((true, raw.value() as f64 * 100.0)),
        _ => None,
    }
}

/// A background size axis with no number: `auto` (0), `cover`, `contain`, or
/// an `auto` beside a length.
fn size_keyword(fit: BackgroundFit) -> u8 {
    match fit {
        BackgroundFit::Auto | BackgroundFit::Lengths(None, None) => 0,
        BackgroundFit::Cover => 1,
        BackgroundFit::Contain => 2,
        BackgroundFit::Lengths(..) => 3,
    }
}

/// A clip corner coordinate in box percent.
fn corner_val(point: (f32, f32), field: &str) -> Val {
    Val::Num(if field == "x" { point.0 } else { point.1 } as f64 * 100.0)
}

/// Whether two samples differ by more than `eps` on some dimension.
fn moved(a: &Val, b: &Val, eps: f64) -> bool {
    a.dims().iter().zip(b.dims()).any(|(a, b)| (a - b).abs() > eps)
}

/// The channels of one sampled `filter` value. Each function appears once.
fn filter_channels(text: &str) -> Result<Vec<(&'static str, Val)>, String> {
    use crate::style::FilterFn;
    let list = crate::style::parse_filters(text).map_err(|e| format!("GSAP {e}"))?;
    let mut out: Vec<(&'static str, Val)> = Vec::new();
    for f in &list {
        let channels: &[&'static str] = match f {
            FilterFn::DropShadow { .. } => &FILTER_CHANNELS[9..],
            _ => &FILTER_CHANNELS[..9],
        };
        for &c in channels {
            let value = if c == "filter-drop-shadow-color" {
                filter_shadow_color(std::slice::from_ref(f)).map(Val::color)
            } else {
                filter_scalar(std::slice::from_ref(f), c).map(|v| Val::Num(v as f64))
            };
            let Some(value) = value else { continue };
            if out.iter().any(|(seen, _)| *seen == c) {
                return Err(format!("GSAP filter '{text}' repeats a function; native tracks take each function once"));
            }
            out.push((c, value));
        }
    }
    Ok(out)
}

fn sample(channel: &str, text: &str) -> Result<Val, String> {
    match channel {
        "opacity" => text.trim().parse().map(Val::Num).map_err(|_| format!("GSAP opacity '{text}'")),
        "letter-spacing" | "word-spacing" if text.trim() == "normal" => Ok(Val::Num(0.0)),
        "font-stretch" => crate::style::stretch_percent(text).map(|p| Val::Num(p as f64)).map_err(|e| format!("GSAP {e}")),
        c if c == "color" || c.ends_with("-color") => {
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
    if let Some(inner) = matrix.trim().strip_prefix("matrix3d(").and_then(|m| m.strip_suffix(')')) {
        return components_3d(inner);
    }
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

/// GSAP's decomposition of a `matrix3d` into transformPerspective, rotationX, rotationY,
/// rotation, skewX and scales.
fn components_3d(inner: &str) -> Option<String> {
    let m: Vec<f64> = inner.split(',').map(|n| n.trim().parse().ok()).collect::<Option<_>>()?;
    let [mut a, mut b, mut c, d, mut a12, mut a22, mut a32, a42, mut a13, mut a23, mut a33, mut a43, x, y, z, _] =
        m[..]
    else {
        return None;
    };
    let mut rx = a32.atan2(a33);
    if rx != 0.0 {
        let (sin, cos) = (-rx).sin_cos();
        let (t1, t2, t3) = (a12 * cos + a13 * sin, a22 * cos + a23 * sin, a32 * cos + a33 * sin);
        a13 = a12 * -sin + a13 * cos;
        a23 = a22 * -sin + a23 * cos;
        a33 = a32 * -sin + a33 * cos;
        a43 = a42 * -sin + a43 * cos;
        (a12, a22, a32) = (t1, t2, t3);
    }
    let mut ry = (-c).atan2(a33);
    if ry != 0.0 {
        let (sin, cos) = (-ry).sin_cos();
        let (t1, t2, t3) = (a * cos - a13 * sin, b * cos - a23 * sin, c * cos - a33 * sin);
        a43 = d * sin + a43 * cos;
        (a, b, c) = (t1, t2, t3);
    }
    let mut rz = b.atan2(a);
    if rz != 0.0 {
        let (sin, cos) = rz.sin_cos();
        let (t1, t2) = (a * cos + b * sin, a12 * cos + a22 * sin);
        b = b * cos - a * sin;
        a22 = a22 * cos - a12 * sin;
        (a, a12) = (t1, t2);
    }
    let perspective = if a43.abs() > 1e-9 { format!("perspective({}px) ", 1.0 / a43.abs()) } else { String::new() };
    (rx, ry, rz) = (rx.to_degrees(), ry.to_degrees(), rz.to_degrees());
    if rx != 0.0 && rx.abs() + rz.abs() > 359.9 {
        (rx, rz, ry) = (0.0, 0.0, 180.0 - ry);
    }
    let sx = (a * a + b * b + c * c).sqrt();
    let sy = (a22 * a22 + a32 * a32).sqrt();
    let skew = a12.atan2(a22).to_degrees();
    let skew = if skew.abs() > 0.0002_f64.to_degrees() { skew } else { 0.0 };
    Some(format!(
        "{perspective}translate3d({x}px, {y}px, {z}px) rotate({rz}deg) rotateY({ry}deg) rotateX({rx}deg) skewX({skew}deg) scale({sx}, {sy})"
    ))
}

fn decompose(text: &str) -> Result<Parts, String> {
    use lightningcss::{properties::transform::TransformList, traits::Parse};
    TransformList::parse_string(text)
        .ok()
        .and_then(|list| Parts::decompose(&list))
        .ok_or_else(|| format!("GSAP transform '{text}' has no component form"))
}

/// Parsers for one element's sampled `box-shadow`, `text-shadow`,
/// `clip-path` and `background-position` text. The clip parser reads the
/// element's own clip for an unset frame.
pub(crate) struct Shapes<'a> {
    pub shadows: &'a dyn Fn(&str) -> Result<Vec<Shadow>, String>,
    pub text_shadows: &'a dyn Fn(&str) -> Result<Vec<TextShadow>, String>,
    pub clip: &'a dyn Fn(Option<&str>) -> Clip,
    pub positions: &'a dyn Fn(&str) -> Positions,
    /// Background sizes; an unset frame reads the element's own.
    pub sizes: &'a dyn Fn(Option<&str>) -> Sizes,
}
/// A clip polygon's corners as box fractions. `None` is no clip.
type Clip = Result<Option<Vec<(f32, f32)>>, String>;
type Positions = Result<Vec<(AxisPos, AxisPos)>, String>;
type Sizes = Result<Vec<BackgroundFit>, String>;

/// Native timeline tracks for one element's sampled properties. Keys sit on
/// sample `times` and interpolate linearly; a set holds the value before it
/// until its jump.
pub(crate) fn tracks(
    series: &Series,
    steps: &Steps,
    target: &str,
    base: &dyn Fn(&str) -> Result<Val, String>,
    shapes: &Shapes,
    times: &[f64],
) -> Result<Vec<TimelineTrack>, String> {
    let mut channels: Vec<(String, &str, Vec<Val>)> = Vec::new();
    let mut out = Vec::new();
    for (property, values) in series {
        if property == "#text" {
            out.push(text_track(values, steps.get("#text").map_or(&[], Vec::as_slice), target, times));
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
                channels.push((channel.to_string(), property, samples));
            }
            continue;
        }
        if property == "filter" {
            let lists = values
                .iter()
                .map(|v| v.as_deref().map(filter_channels).transpose())
                .collect::<Result<Vec<_>, _>>()?;
            for channel in FILTER_CHANNELS {
                if lists.iter().flatten().all(|list| !list.iter().any(|(c, _)| *c == channel)) {
                    continue;
                }
                let base = base(channel)?;
                let samples = lists
                    .iter()
                    .map(|list| {
                        list.as_ref().map_or(base, |list| {
                            list.iter().find(|(c, _)| *c == channel).map_or_else(|| filter_default(channel), |(_, v)| *v)
                        })
                    })
                    .collect();
                channels.push((channel.to_string(), property, samples));
            }
            continue;
        }
        if property == "font-variation-settings" {
            let lists = values
                .iter()
                .map(|v| {
                    v.as_deref()
                        .map(|text| crate::style::parse_variation_settings(text).map_err(|e| format!("GSAP {e}")))
                        .transpose()
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut axes: Vec<&str> = Vec::new();
            for v in lists.iter().flatten().flatten() {
                if !axes.contains(&v.axis.as_str()) {
                    axes.push(&v.axis);
                }
            }
            for axis in axes {
                let channel = format!("font-variation-{axis}");
                let base = base(&channel)?;
                let samples = lists
                    .iter()
                    .map(|list| {
                        list.as_ref()
                            .and_then(|list| list.iter().find(|v| v.axis == axis))
                            .map_or(base, |v| Val::Num(v.value as f64))
                    })
                    .collect();
                channels.push((channel, property, samples));
            }
            continue;
        }
        if property == "box-shadow" {
            let lists = values
                .iter()
                .map(|v| v.as_deref().map(|text| (shapes.shadows)(text).map_err(|e| format!("GSAP {e}"))).transpose())
                .collect::<Result<Vec<_>, _>>()?;
            let count = lists.iter().flatten().map(Vec::len).max().unwrap_or(0);
            for i in 0..count {
                for field in SHADOW_FIELDS {
                    let channel = format!("box-shadow-{i}-{field}");
                    let base = base(&channel)?;
                    let samples = lists
                        .iter()
                        .map(|list| {
                            list.as_ref().map_or(base, |list| shadow_val(list.get(i).unwrap_or(&NO_BOX_SHADOW), field))
                        })
                        .collect();
                    channels.push((channel, property, samples));
                }
            }
            continue;
        }
        if property == "text-shadow" {
            let lists = values
                .iter()
                .map(|v| v.as_deref().map(|text| (shapes.text_shadows)(text).map_err(|e| format!("GSAP {e}"))).transpose())
                .collect::<Result<Vec<_>, _>>()?;
            let count = lists.iter().flatten().map(Vec::len).max().unwrap_or(0);
            for i in 0..count {
                for field in TEXT_SHADOW_FIELDS {
                    let channel = format!("text-shadow-{i}-{field}");
                    let base = base(&channel)?;
                    let samples = lists
                        .iter()
                        .map(|list| {
                            list.as_ref().map_or(base, |list| text_shadow_val(list.get(i).unwrap_or(&NO_TEXT_SHADOW), field))
                        })
                        .collect();
                    channels.push((channel, property, samples));
                }
            }
            continue;
        }
        if property == "background-position" {
            let lists = values
                .iter()
                .map(|v| v.as_deref().map(|text| (shapes.positions)(text).map_err(|e| format!("GSAP {e}"))).transpose())
                .collect::<Result<Vec<_>, _>>()?;
            let count = lists.iter().flatten().map(Vec::len).max().unwrap_or(0);
            for i in 0..count {
                for field in ["x", "y", "x-percent", "y-percent"] {
                    let channel = format!("background-position-{i}-{field}");
                    let base = base(&channel)?;
                    let samples = lists
                        .iter()
                        .map(|list| {
                            list.as_ref().map_or(base, |list| {
                                let (x, y) = list[i % list.len()];
                                position_val(if field.starts_with('x') { x } else { y }, field)
                            })
                        })
                        .collect();
                    channels.push((channel, property, samples));
                }
            }
            continue;
        }
        // Each layer axis keeps one unit while it moves; `auto` and keywords hold.
        if property == "background-size" {
            let lists = values
                .iter()
                .map(|v| (shapes.sizes)(v.as_deref()).map_err(|e| format!("GSAP {e}")))
                .collect::<Result<Vec<_>, _>>()?;
            let count = lists.iter().map(Vec::len).max().unwrap_or(0);
            for i in 0..count {
                let fits: Vec<BackgroundFit> = lists
                    .iter()
                    .map(|list| if list.is_empty() { BackgroundFit::Auto } else { list[i % list.len()] })
                    .collect();
                for (width, axis) in [(true, "width"), (false, "height")] {
                    let axes: Vec<_> = fits.iter().map(|fit| size_axis(*fit, width)).collect();
                    if axes.iter().all(Option::is_none) && fits.iter().all(|f| size_keyword(*f) == size_keyword(fits[0])) {
                        continue;
                    }
                    let percent = axes[0].is_some_and(|(percent, _)| percent);
                    if !axes.iter().all(|a| a.is_some_and(|(p, _)| p == percent)) {
                        return Err(format!(
                            "GSAP background-size moves layer {i} {axis} between units or keywords ({})",
                            values.iter().flatten().next().map_or("unset", String::as_str)
                        ));
                    }
                    let channel = format!("background-size-{i}-{axis}{}", if percent { "-percent" } else { "" });
                    let samples = axes.iter().map(|a| Val::Num(a.map_or(0.0, |(_, v)| v))).collect();
                    channels.push((channel, property, samples));
                }
            }
            continue;
        }
        // Line heights key as multiples of the frame's font size.
        if property == "line-height" {
            let font = |f: usize| -> Result<f64, String> {
                let sampled = series.get("font-size").and_then(|s| s[f].as_deref());
                match sampled.and_then(|t| t.trim().strip_suffix("px")?.trim().parse().ok()) {
                    Some(size) => Ok(size),
                    None => match base("font-size")? {
                        Val::Num(size) => Ok(size),
                        Val::Color(_) => unreachable!(),
                    },
                }
            };
            let samples = values
                .iter()
                .enumerate()
                .map(|(f, v)| {
                    let Some(text) = v.as_deref() else { return base("line-height") };
                    let t = text.trim();
                    match t.strip_suffix("px") {
                        Some(n) => n.trim().parse::<f64>().ok().map(|n| n / font(f).unwrap_or(1.0).max(f64::EPSILON)),
                        None => t.parse::<f64>().ok(),
                    }
                    .map(Val::Num)
                    .ok_or_else(|| format!("GSAP animates line-height as '{text}'; native tracks take px or a number"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            channels.push(("line-height".into(), property, samples));
            continue;
        }
        // `padding` and `margin` list one to four lengths, top, right, bottom, left.
        if property == "padding" || property == "margin" {
            let sides = values
                .iter()
                .map(|v| {
                    v.as_deref()
                        .map(|text| {
                            let parts = crate::css::split(text.trim(), false);
                            let pick: &[usize; 4] = match parts.len() {
                                1 => &[0, 0, 0, 0],
                                2 => &[0, 1, 0, 1],
                                3 => &[0, 1, 2, 1],
                                4 => &[0, 1, 2, 3],
                                _ => return Err(format!("GSAP {property} '{text}' lists {} lengths", parts.len())),
                            };
                            pick.iter().map(|&k| sample(property, &parts[k])).collect::<Result<Vec<_>, _>>()
                        })
                        .transpose()
                })
                .collect::<Result<Vec<_>, _>>()?;
            for (side, name) in ["top", "right", "bottom", "left"].iter().enumerate() {
                let channel = format!("{property}-{name}");
                let samples = sides
                    .iter()
                    .map(|s| s.as_ref().map_or_else(|| base(&channel), |s| Ok(s[side])))
                    .collect::<Result<Vec<_>, _>>()?;
                channels.push((channel, property, samples));
            }
            continue;
        }
        // `border-color` lists one to four colors, top, right, bottom, left.
        if property == "border-color" {
            let sides = values
                .iter()
                .map(|v| {
                    v.as_deref()
                        .map(|text| {
                            let parts = crate::css::split(text.trim(), false);
                            let pick: &[usize; 4] = match parts.len() {
                                1 => &[0, 0, 0, 0],
                                2 => &[0, 1, 0, 1],
                                3 => &[0, 1, 2, 1],
                                4 => &[0, 1, 2, 3],
                                _ => return Err(format!("GSAP border-color '{text}' lists {} colors", parts.len())),
                            };
                            pick.iter().map(|&k| sample("color", &parts[k])).collect::<Result<Vec<_>, _>>()
                        })
                        .transpose()
                })
                .collect::<Result<Vec<_>, _>>()?;
            for (side, name) in ["top", "right", "bottom", "left"].iter().enumerate() {
                let channel = format!("border-{name}-color");
                let base = base(&channel)?;
                let samples = sides.iter().map(|s| s.as_ref().map_or(base, |s| s[side])).collect();
                channels.push((channel, property, samples));
            }
            continue;
        }
        if property == "clip-path" {
            let clips = values
                .iter()
                .map(|v| (shapes.clip)(v.as_deref()).map_err(|e| format!("GSAP clip-path: {e}")))
                .collect::<Result<Vec<_>, _>>()?;
            let Some(count) = clips.iter().flatten().map(Vec::len).next() else { continue };
            if clips.iter().any(|s| s.as_ref().is_none_or(|s| s.len() != count)) {
                return Err(format!(
                    "GSAP clip-path on {target} changes between shapes of different corner counts or 'none'; \
                     native clip tracks need one shape kind on every frame"
                ));
            }
            for i in 0..count {
                for field in ["x", "y"] {
                    let samples = clips.iter().flatten().map(|s| corner_val(s[i], field)).collect();
                    channels.push((format!("clip-path-{i}-{field}"), property, samples));
                }
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
        channels.push((channel.to_string(), property, samples));
    }
    for (channel, property, samples) in channels {
        let channel = channel.as_str();
        let eps = tolerance(channel);
        let base = base(channel)?;
        let differs = samples
            .iter()
            .any(|v| v.dims().iter().zip(base.dims()).any(|(a, b)| (a - b).abs() > eps));
        if !differs {
            continue;
        }
        // Jumps on this channel, earliest per frame, where the value moves.
        let mut jumps: Vec<(usize, f64)> = [channel, property]
            .iter()
            .filter_map(|name| steps.get(*name))
            .flatten()
            .copied()
            .filter(|&(f, _)| f > 0 && f < samples.len() && moved(&samples[f - 1], &samples[f], eps))
            .collect();
        jumps.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        jumps.dedup_by_key(|j| j.0);
        let mut keyframes = Vec::new();
        let mut start = 0;
        for piece in jumps.iter().map(Some).chain([None]) {
            let end = piece.map_or(samples.len(), |&(f, _)| f);
            keyframes.extend(keep(&samples[start..end], &times[start..end], eps).into_iter().map(|f| TimelineKeyframe {
                time: times[start + f],
                value: samples[start + f].timeline(),
                ease: None,
            }));
            if let Some(&(f, time)) = piece {
                keyframes.push(TimelineKeyframe {
                    time,
                    value: samples[f].timeline(),
                    ease: Some("steps(1, end)".into()),
                });
                start = f;
            }
        }
        // A piece's first frame key lands on its jump when the set sits on
        // the frame.
        keyframes.dedup_by(|b, a| (b.time - a.time).abs() < 1e-9);
        out.push(TimelineTrack {
            target: target.to_string(),
            property: channel.to_string(),
            keyframes,
        });
    }
    Ok(out)
}

/// A `text` track keyed on each frame whose text differs from the frame
/// before, at the set's time when a set wrote it. The element shows its own
/// text, the first frame's, until then.
fn text_track(values: &[Option<String>], steps: &[(usize, f64)], target: &str, times: &[f64]) -> TimelineTrack {
    let text = |f: usize| values[f].clone().unwrap_or_default();
    let keyframes = (1..values.len())
        .filter(|&f| text(f) != text(f - 1))
        .map(|f| TimelineKeyframe {
            time: steps.iter().filter(|s| s.0 == f).map(|s| s.1).reduce(f64::min).unwrap_or(times[f]),
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
        let frames = |n: usize| (0..n).map(|f| f as f64).collect::<Vec<_>>();
        assert_eq!(keep(&ramp, &frames(ramp.len()), 0.02), vec![10, 20]);
        let curve: Vec<Val> = (0..=60).map(|f| Val::Num((f as f64 / 60.0).powi(2) * 500.0)).collect();
        let kept = keep(&curve, &frames(curve.len()), 0.02);
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
