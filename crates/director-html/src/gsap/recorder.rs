//! The compiler's side of a script run. Page scripts run in the
//! `director-recorder` helper process, which links a JavaScript engine that
//! cannot link beside Skia. Each run spawns one helper and answers its
//! questions here: probes, computed style, transform matrices, SVG path
//! geometry, media queries and beats.

use director_dom::protocol::{self, Answer, Ask, Finish, Load, Placed, Reply, Span};
use director_dom::{Doc, Placement, Prober};
use ego_tree::NodeId;
use scraper::Html;
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};

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

impl Placement for Probed {
    fn positioned(&self) -> bool {
        matches!(self.css.get("position").map(String::as_str), Some("absolute" | "fixed"))
    }
}

/// The document after a run, with the full probe of each box.
pub(crate) type Page = Doc<Probed>;

/// Probe results shared by script runs, keyed by the probed markup's hash
/// and length. Runs replay the same setup, so the first run to reach a
/// probe computes it and the others wait for it.
pub(crate) type Probes = Mutex<HashMap<(u64, usize), Arc<OnceLock<Result<Arc<Probe>, String>>>>>;

/// What a run needs from the compile.
pub(crate) struct Host<'a> {
    pub options: &'a crate::CompileOptions,
    pub assets: &'a Arc<crate::assets::AssetStore>,
    pub beats: Rc<crate::beats::Beats>,
    pub shared: Option<Arc<Probes>>,
}

impl Host<'_> {
    fn viewport(&self) -> (f32, f32) {
        (self.options.width as f32, self.options.height as f32)
    }

    /// Probe `html`, through the shared cache when runs share one.
    fn probe(&self, html: &Html) -> Result<Arc<Probe>, String> {
        let probe = || {
            crate::compiler::probe(html, self.options, self.assets, &self.beats).map(Arc::new).map_err(|e| e.to_string())
        };
        let Some(shared) = &self.shared else { return probe() };
        let markup = html.html();
        let mut hash = std::hash::DefaultHasher::new();
        markup.hash(&mut hash);
        let cell = shared.lock().unwrap().entry((hash.finish(), markup.len())).or_default().clone();
        cell.get_or_init(probe).clone()
    }

    /// A prober for documents the compiler holds.
    fn prober(&self) -> Prober<Probed> {
        let (options, assets, beats, shared) =
            (self.options.clone(), self.assets.clone(), self.beats.clone(), self.shared.clone());
        Box::new(move |html: &Html| {
            let host = Host { options: &options, assets: &assets, beats: beats.clone(), shared: shared.clone() };
            Ok((*host.probe(html)?).clone())
        })
    }
}

/// One finished run: each sample's change record, how many random numbers
/// the scripts drew while the timeline played, and the document when asked.
pub(crate) struct Run {
    pub records: Vec<String>,
    pub draws: u64,
    pub page: Option<Page>,
}

/// The `director-recorder` executable: `DIRECTOR_RECORDER`, else beside the
/// running executable or one directory up, where test binaries find it.
fn helper() -> Result<&'static PathBuf, String> {
    static PATH: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    PATH.get_or_init(|| {
        if let Some(path) = std::env::var_os("DIRECTOR_RECORDER") {
            return Ok(path.into());
        }
        let name = format!("director-recorder{}", std::env::consts::EXE_SUFFIX);
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        exe.ancestors()
            .skip(1)
            .take(2)
            .map(|dir| dir.join(&name))
            .find(|path| path.is_file())
            .ok_or_else(|| {
                format!(
                    "GSAP pages need {name} beside {}; build it with `cargo build --release -p director-cli`",
                    exe.display()
                )
            })
    })
    .as_ref()
    .map_err(Clone::clone)
}

/// Record `span` of `load`'s page in a fresh helper.
pub(crate) fn run(load: &[u8], span: &Span, host: &Host) -> Result<Run, String> {
    let mut child = Command::new(helper()?)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("the GSAP recorder does not start: {e}"))?;
    let mut input = BufWriter::new(child.stdin.take().unwrap());
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut last: Option<(Arc<Probe>, Vec<NodeId>)> = None;
    let served = (|| -> std::io::Result<Result<Finish, String>> {
        std::io::Write::write_all(&mut input, load)?;
        protocol::write(&mut input, span)?;
        loop {
            let answer = match protocol::read::<Ask>(&mut output)? {
                Ask::Done(done) => return Ok(done),
                Ask::Probe(tree) => probe(tree, host, &mut last),
                ask => answer(ask, host, last.as_ref()),
            };
            protocol::write(&mut input, &answer)?;
        }
    })();
    drop(input);
    if served.is_err() {
        // A helper blocked on a write the compiler stopped reading never exits.
        let _ = child.kill();
    }
    let status = child.wait();
    let finish = match served {
        Ok(done) => done?,
        Err(e) => {
            let status = status.map_or_else(|e| e.to_string(), |s| s.to_string());
            return Err(format!("the GSAP recorder stopped ({status}): {e}"));
        }
    };
    let page = match finish.page {
        Some(saved) => {
            let (probe, ids) = last.clone().unzip();
            let place = |node: u32| Some(probe.as_ref()?.get(ids.as_ref()?.get(node as usize)?)?.clone());
            Some(Doc::load(saved, place, host.prober())?)
        }
        None => None,
    };
    Ok(Run { records: finish.records, draws: finish.draws, page })
}

/// The page and scripts every run of one compile loads.
pub(crate) fn load(html: &Html, viewport: (f32, f32), scripts: Vec<protocol::Script>) -> Vec<u8> {
    let mut frame = Vec::new();
    protocol::write(&mut frame, &Load { page: director_dom::save(html), viewport, scripts }).expect("a frame in memory");
    frame
}

fn probe(tree: director_dom::Tree, host: &Host, last: &mut Option<(Arc<Probe>, Vec<NodeId>)>) -> Answer {
    let (html, ids) = director_dom::load(tree)?;
    let probe = host.probe(&html)?;
    let placed = ids
        .iter()
        .enumerate()
        .filter_map(|(k, id)| {
            let p = probe.get(id)?;
            let placed = Placed { rect: p.rect, abs: p.abs, font_size: p.font_size, positioned: p.positioned() };
            Some((k as u32, placed))
        })
        .collect();
    *last = Some((probe, ids));
    Ok(Reply::Placed(placed))
}

fn answer(ask: Ask, host: &Host, last: Option<&(Arc<Probe>, Vec<NodeId>)>) -> Answer {
    Ok(match ask {
        Ask::Computed { node, tag, prop } => {
            let probed = node.and_then(|k| {
                let (probe, ids) = last?;
                probe.get(ids.get(k as usize)?)
            });
            Reply::Text(computed_value(probed, &tag, &prop))
        }
        Ask::Matrix { value, size } => Reply::Text(matrix_text(&value, size)),
        Ask::Path { d, at } => path(&d, at)?,
        Ask::Media(query) => {
            let (w, h) = host.viewport();
            Reply::Flag(crate::css::media_text_matches(&query, w, h)?)
        }
        Ask::Beat(position) => Reply::Number(host.beats.grid()?.time(position)),
        Ask::BeatAt(time) => Reply::Number(host.beats.grid()?.position(time)),
        Ask::BeatGrid => {
            let grid = host.beats.grid()?;
            let json = serde_json::json!({ "bpm": grid.bpm, "beatsPerBar": grid.beats_per_bar, "drop": grid.drop });
            Reply::Text(json.to_string())
        }
        Ask::Probe(_) | Ask::Done(_) => return Err("the recorder asked out of turn".into()),
    })
}

/// The length of SVG path data `d` over every contour, or the point at
/// distance `at` along it.
fn path(d: &str, at: Option<f32>) -> Answer {
    let Some(path) = skia_safe::Path::from_svg(d) else {
        return Err(format!("SVG path data '{d}' does not parse"));
    };
    // A fine resolution keeps curve lengths within a thousandth of a unit.
    let contours: Vec<_> = skia_safe::ContourMeasureIter::new(&path, false, 64.0).collect();
    let total: f32 = contours.iter().map(|c| c.length()).sum();
    let Some(at) = at else {
        return Ok(Reply::Number(total as f64));
    };
    let mut left = at.clamp(0.0, total);
    for (n, contour) in contours.iter().enumerate() {
        if left <= contour.length() || n + 1 == contours.len() {
            let point = contour.pos_tan(left.min(contour.length())).map_or((0.0, 0.0), |(p, _)| (p.x, p.y));
            return Ok(Reply::Numbers(vec![point.0, point.1]));
        }
        left -= contour.length();
    }
    Ok(Reply::Numbers(vec![0.0, 0.0]))
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
