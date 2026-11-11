use crate::Attr;
use crate::{
    animation,
    assets::AssetStore,
    beats::Beats,
    computed::ComputedStyle,
    css::{diagnostic, Css, Values},
    element::{AtomicSizing, HtmlElement, InlineFlow, InlineLink},
    style::{self, Metrics, PaintStyle},
    CompileError, CompileOptions, CompiledHtml,
};
use director_core::{
    element::TextSpan,
    node::{ImageNode, TextNode, VectorNode, VideoNode, VideoSource},
    systems::assets::FontFace,
    types::{NodeId, ObjectFit},
    video_wrapper::RenderMode,
    Director, Element, RenderError,
};
use scraper::{node::Node, ElementRef, Html, Selector};
use skia_safe::textlayout::FontCollection;
use std::{
    any::Any,
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
    rc::Rc,
    sync::Arc,
};

pub(crate) fn escape_xml(value: &str, attr: bool) -> String {
    let mut escaped = value.replace('&', "&amp;").replace('<', "&lt;");
    if attr {
        escaped = escaped.replace('"', "&quot;");
    }
    escaped
}

const SVG_EXTERNAL_PRESENTATION: &[&str] = &[
    "fill",
    "color",
    "stroke",
    "stroke-width",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-opacity",
    "stroke-miterlimit",
    "fill-opacity",
    "fill-rule",
    "clip-rule",
    "opacity",
    "transform",
    "clip-path",
    "filter",
    "mix-blend-mode",
    "marker-start",
    "marker-mid",
    "marker-end",
    "mask",
    "font-family",
    "font-size",
    "font-weight",
    "font-style",
];

const SVG_ANIMATION_PROPERTIES: &[&str] = &[
    "animation",
    "animation-name",
    "animation-duration",
    "animation-timing-function",
    "animation-iteration-count",
    "animation-direction",
    "animation-play-state",
    "animation-delay",
    "animation-fill-mode",
    "animation-timeline",
    "animation-composition",
    "animation-range",
    "animation-range-start",
    "animation-range-end",
];

fn serialize_svg_children(
    el: ElementRef<'_>,
    css: &Css,
    parent: &ComputedStyle,
    plan: &crate::smil::SmilPlan,
) -> Result<String, CompileError> {
    let mut out = String::new();
    write_svg_children(el, css, parent, &mut out, "", plan)?;
    Ok(out)
}

fn write_svg_children(
    el: ElementRef<'_>,
    css: &Css,
    parent: &ComputedStyle,
    out: &mut String,
    parent_path: &str,
    plan: &crate::smil::SmilPlan,
) -> Result<(), CompileError> {
    let mut index = 0usize;
    for child in el.children() {
        match child.value() {
            Node::Text(text) if el.value().name() == "style" => out.push_str(&escape_xml(&flatten_nested_colors(text), false)),
            Node::Text(text) => out.push_str(&escape_xml(text, false)),
            Node::Element(_) => {
                let child = ElementRef::wrap(child).unwrap();
                let child_path = crate::smil::child_path(parent_path, index);
                index += 1;
                let values = css.cascade(child, parent)?;
                let declared = css.stylesheet_properties(child);
                let mut extra = String::new();
                for name in SVG_EXTERNAL_PRESENTATION {
                    // lightningcss stores the mask shorthand as mask-image.
                    // usvg reads the presentation name mask.
                    let lookup = if *name == "mask" && !declared.contains(name) {
                        "mask-image"
                    } else {
                        *name
                    };
                    if declared.contains(&lookup) {
                        if let Some(value) = values.get(lookup) {
                            extra.push_str(name);
                            extra.push(':');
                            if *name == "transform" {
                                extra.push_str(&value.replace("px", ""));
                            } else {
                                extra.push_str(value);
                            }
                            extra.push(';');
                        }
                    }
                }
                let tag = child.value().name();
                out.push('<');
                out.push_str(tag);
                let mut wrote_style = false;
                for (name, value) in child.value().attrs() {
                    if name == "style" {
                        let mut style = value.to_string();
                        if !extra.is_empty() && !style.is_empty() && !style.trim_end().ends_with(';') {
                            style.push(';');
                        }
                        style.push_str(&extra);
                        out.push_str(&format!(" {name}=\"{}\"", escape_xml(&flatten_nested_colors(&style), true)));
                        wrote_style = true;
                    } else {
                        out.push_str(&format!(" {name}=\"{}\"", escape_xml(value, true)));
                    }
                }
                if !wrote_style && !extra.is_empty() {
                    out.push_str(&format!(" style=\"{}\"", escape_xml(&flatten_nested_colors(&extra), true)));
                }
                if let Some(id) = plan.target_id(&child_path) {
                    out.push_str(&format!(" director-smil-id=\"{id}\""));
                }
                out.push('>');
                write_svg_children(child, css, &values, out, &child_path, plan)?;
                out.push_str("</");
                out.push_str(tag);
                out.push('>');
            }
            _ => {}
        }
    }
    Ok(())
}

/// usvg's CSS tokenizer ends a function at its first `)`, so a color function
/// inside another function, as in `drop-shadow(0 0 4px rgba(0,0,0,.5))`,
/// would cut the declaration short. Those colors become `#rrggbbaa`.
fn flatten_nested_colors(css: &str) -> String {
    const FUNCTIONS: [&str; 10] = ["rgba(", "rgb(", "hsla(", "hsl(", "hwb(", "oklab(", "oklch(", "lab(", "lch(", "color("];
    let bytes = css.as_bytes();
    let mut out = String::with_capacity(css.len());
    let (mut depth, mut i) = (0usize, 0usize);
    while i < css.len() {
        let word_start = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'-');
        let function = FUNCTIONS.iter().find(|f| css[i..].len() >= f.len() && css[i..i + f.len()].eq_ignore_ascii_case(f));
        if let (true, true, Some(function)) = (depth > 0, word_start, function) {
            let close = css[i + function.len()..].find(')').map(|at| i + function.len() + at + 1);
            if let Some(Ok(c)) = close.map(|end| crate::style::color(&css[i..end], director_core::types::Color::BLACK)) {
                let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                out.push_str(&format!("#{:02x}{:02x}{:02x}{:02x}", channel(c.r), channel(c.g), channel(c.b), channel(c.a)));
                i = close.unwrap();
                continue;
            }
        }
        let ch = css[i..].chars().next().unwrap();
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

pub(crate) fn validate_svg(source: &str) -> Result<(), CompileError> {
    reject_svg(
        source,
        &["script", "image"],
        "SVG scripts and nested images require native translation",
    )
}

pub(crate) fn validate_captured_svg(source: &str) -> Result<(), CompileError> {
    reject_svg(
        source,
        &[
            "foreignObject",
            "animate",
            "animateTransform",
            "animateMotion",
            "set",
            "script",
            "image",
        ],
        "SVG foreignObject, SMIL, scripts, and nested images require native translation",
    )
}

fn reject_svg(source: &str, names: &[&str], message: &str) -> Result<(), CompileError> {
    let dom = Html::parse_fragment(source);
    let selector = Selector::parse(&names.join(", ")).unwrap();
    if dom.select(&selector).next().is_some() {
        return Err(CompileError::Input(message.into()));
    }
    Ok(())
}

struct SvgSpace {
    viewport_w: f32,
    viewport_h: f32,
    min_x: f32,
    min_y: f32,
    scale_x: f32,
    scale_y: f32,
}

enum SvgLen {
    User(f32),
    Percent(f32),
}

fn parse_svg_len(raw: &str) -> Result<SvgLen, CompileError> {
    let raw = raw.trim();
    if let Some(number) = raw.strip_suffix('%') {
        let value: f32 = number.trim().parse().map_err(|_| {
            CompileError::Input(
                "foreignObject length requires a number, px, or a percent".into(),
            )
        })?;
        if !value.is_finite() {
            return Err(CompileError::Input(
                "foreignObject length requires a number, px, or a percent".into(),
            ));
        }
        return Ok(SvgLen::Percent(value / 100.0));
    }
    let number = raw.strip_suffix("px").unwrap_or(raw).trim();
    let value: f32 = number.parse().map_err(|_| {
        CompileError::Input("foreignObject length requires a number, px, or a percent".into())
    })?;
    if !value.is_finite() {
        return Err(CompileError::Input(
            "foreignObject length requires a number, px, or a percent".into(),
        ));
    }
    Ok(SvgLen::User(value))
}

fn svg_space(svg: ElementRef<'_>, viewport: (f32, f32)) -> Result<SvgSpace, CompileError> {
    let (min_x, min_y, width, height) = if let Some(raw) = svg.value().attr_text("viewBox") {
        let parts: Vec<f32> = raw
            .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
            .filter(|part| !part.is_empty())
            .map(|part| {
                part.parse::<f32>().map_err(|_| {
                    CompileError::Input("foreignObject viewBox requires four numbers".into())
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if parts.len() != 4
            || !parts.iter().all(|value| value.is_finite())
            || parts[2] <= 0.0
            || parts[3] <= 0.0
        {
            return Err(CompileError::Input(
                "foreignObject viewBox requires four numbers".into(),
            ));
        }
        (parts[0], parts[1], parts[2], parts[3])
    } else {
        (0.0, 0.0, viewport.0, viewport.1)
    };
    Ok(SvgSpace {
        viewport_w: viewport.0,
        viewport_h: viewport.1,
        min_x,
        min_y,
        scale_x: viewport.0 / width,
        scale_y: viewport.1 / height,
    })
}

fn resolve_svg_x(len: SvgLen, space: &SvgSpace) -> f32 {
    match len {
        SvgLen::Percent(fraction) => fraction * space.viewport_w,
        SvgLen::User(user) => (user - space.min_x) * space.scale_x,
    }
}

fn resolve_svg_y(len: SvgLen, space: &SvgSpace) -> f32 {
    match len {
        SvgLen::Percent(fraction) => fraction * space.viewport_h,
        SvgLen::User(user) => (user - space.min_y) * space.scale_y,
    }
}

fn resolve_svg_width(len: SvgLen, space: &SvgSpace) -> f32 {
    match len {
        SvgLen::Percent(fraction) => fraction * space.viewport_w,
        SvgLen::User(user) => user * space.scale_x,
    }
}

fn resolve_svg_height(len: SvgLen, space: &SvgSpace) -> f32 {
    match len {
        SvgLen::Percent(fraction) => fraction * space.viewport_h,
        SvgLen::User(user) => user * space.scale_y,
    }
}

fn definite_svg_axis(
    raw: Option<&String>,
    fallback: f32,
    metrics: Metrics,
) -> Result<f32, CompileError> {
    let Some(raw) = raw else {
        if fallback.is_finite() && fallback > 0.0 {
            return Ok(fallback);
        }
        return Err(CompileError::Input(
            "foreignObject requires a definite SVG viewport".into(),
        ));
    };
    style::px(raw, metrics).map_err(|_| {
        CompileError::Input("foreignObject requires a definite SVG viewport".into())
    })
}

fn specifies_transform(css: &Css, el: ElementRef<'_>) -> Result<bool, CompileError> {
    if el.value().attr_text("transform").is_some_and(|value| {
        let value = value.trim();
        !value.is_empty() && value != "none"
    }) {
        return Ok(true);
    }
    if !css.has_local_property(el, "transform") {
        return Ok(false);
    }
    let values = css.cascade(el, &ComputedStyle::default())?;
    Ok(values.get("transform").is_some_and(|value| value != "none"))
}

const SVG_FONT_FILES: &[&str] = &[
    "fonts/Roboto-Regular.ttf",
    "fonts/Roboto-Bold.ttf",
    "fonts/Roboto-Italic.ttf",
    "fonts/Inter-Regular.ttf",
    "fonts/Inter-Bold.ttf",
    "fonts/Poppins-Bold.ttf",
];

fn decoded_font(family: &str, bytes: Vec<u8>) -> Result<Vec<u8>, CompileError> {
    match bytes.get(..4) {
        Some(b"wOF2") => wuff::decompress_woff2(&bytes).map_err(|e| {
            CompileError::Input(format!("font '{family}': WOFF2 decode: {e:?}"))
        }),
        Some(b"wOFF") => wuff::decompress_woff1(&bytes)
            .map_err(|e| CompileError::Input(format!("font '{family}': WOFF decode: {e:?}"))),
        _ => Ok(bytes),
    }
}

pub(crate) fn svg_fontdb(
    assets: &AssetStore,
    faces: &[(String, String)],
) -> Result<Arc<usvg::fontdb::Database>, CompileError> {
    let mut db = bundled_svg_fonts(assets)?;
    for (family, src) in faces {
        let bytes = decoded_font(family, assets.read(&url(src)?)?)?;
        db.load_font_data(bytes);
    }
    Ok(Arc::new(db))
}

/// The bundled faces every inline SVG resolves text against.
pub(crate) fn bundled_svg_fonts(
    loader: &dyn director_core::AssetLoader,
) -> Result<usvg::fontdb::Database, CompileError> {
    let mut db = usvg::fontdb::Database::new();
    for path in SVG_FONT_FILES {
        let bytes = loader
            .load_bytes(path)
            .map_err(|e| CompileError::Input(format!("SVG font '{path}': {e}")))?;
        db.load_font_data(bytes);
    }
    db.set_serif_family("Roboto");
    db.set_sans_serif_family("Roboto");
    Ok(db)
}

pub(crate) fn cached_svg_fonts(
    slot: &mut Option<Arc<usvg::fontdb::Database>>,
    assets: &AssetStore,
    faces: &[(String, String)],
) -> Result<Arc<usvg::fontdb::Database>, CompileError> {
    if let Some(fonts) = slot {
        return Ok(Arc::clone(fonts));
    }
    let fonts = svg_fontdb(assets, faces)?;
    *slot = Some(Arc::clone(&fonts));
    Ok(fonts)
}

pub(crate) fn parse_svg(
    markup: &str,
    context: &str,
    fonts: &Arc<usvg::fontdb::Database>,
) -> Result<usvg::Tree, CompileError> {
    let options = usvg::Options {
        font_family: "Roboto".into(),
        font_size: 16.0,
        fontdb: Arc::clone(fonts),
        ..usvg::Options::default()
    };
    usvg::Tree::from_data(markup.as_bytes(), &options)
        .map_err(|e| CompileError::Input(format!("{context}: invalid SVG: {e}")))
}
pub fn url(source: &str) -> Result<String, CompileError> {
    let mut input = cssparser::ParserInput::new(source);
    let mut parser = cssparser::Parser::new(&mut input);
    parser
        .expect_url()
        .map(|u| u.to_string())
        .map_err(|e| CompileError::Input(format!("font/image URL: {e:?}")))
}

pub(crate) fn load_fonts(
    director: &mut Director,
    fonts: &[(String, String)],
    assets: &AssetStore,
) -> Result<(), CompileError> {
    let faces = fonts
        .iter()
        .map(|(family, src)| {
            Ok(FontFace {
                family: family.clone(),
                bytes: Arc::new(decoded_font(family, assets.read(&url(src)?)?)?),
            })
        })
        .collect::<Result<Vec<_>, CompileError>>()?;
    director
        .assets
        .register_font_faces(faces)
        .map_err(CompileError::Input)
}

pub fn validate_options(options: &CompileOptions) -> Result<(), CompileError> {
    if options.width <= 0
        || options.height <= 0
        || options.width > 16384
        || options.height > 16384
        || !options.fps.is_finite()
        || options.fps <= 0.0
        || options.fps > 1000.0
        || !options.duration.is_finite()
        || options.duration <= 0.0
    {
        return Err(CompileError::Input(
            "canvas must be 1..16384 pixels, FPS 0..1000, duration finite and positive".into(),
        ));
    }
    Ok(())
}

pub fn compile(source: &str, options: CompileOptions) -> Result<CompiledHtml, CompileError> {
    let assets = Arc::new(AssetStore::local(&options.base_dir, false));
    compile_with_assets(source, options, assets)
}

/// The composition root: the `[data-composition-id]` element `options`
/// names, else the first one, else `body`.
fn composition_root<'a>(dom: &'a Html, options: &CompileOptions) -> Result<ElementRef<'a>, CompileError> {
    let selector = Selector::parse("[data-composition-id]").unwrap();
    let roots: Vec<_> = dom.select(&selector).collect();
    let root = if let Some(id) = &options.composition {
        roots
            .iter()
            .copied()
            .find(|r| r.value().attr_text("data-composition-id") == Some(id.as_str()))
            .ok_or_else(|| CompileError::Input(format!("composition '{id}' not found")))?
    } else {
        roots.first().copied().unwrap_or_else(|| {
            dom.select(&Selector::parse("body").unwrap())
                .next()
                .unwrap()
        })
    };
    if root
        .ancestors()
        .filter_map(ElementRef::wrap)
        .any(|e| e.value().name() == "template")
    {
        return Err(CompileError::Input(
            "standalone template roots must be assembled or captured first".into(),
        ));
    }
    Ok(root)
}

/// Sub-frame motion blur: `data-motion-blur` samples spread over
/// `data-shutter-angle` degrees, 1 sample at 180 degrees by default.
fn motion_blur(root: ElementRef<'_>) -> Result<(u32, f32), CompileError> {
    let mut blur = (1, 180.0);
    if let Some(v) = root.value().attr_text("data-motion-blur") {
        blur.0 = v
            .parse()
            .ok()
            .filter(|n| (1..=64).contains(n))
            .ok_or_else(|| CompileError::Input(format!("invalid data-motion-blur: {v} (1..=64 samples)")))?;
    }
    if let Some(v) = root.value().attr_text("data-shutter-angle") {
        blur.1 = v
            .parse()
            .ok()
            .filter(|a: &f32| (0.0..=360.0).contains(a))
            .ok_or_else(|| CompileError::Input(format!("invalid data-shutter-angle: {v} (0..=360 degrees)")))?;
    }
    Ok(blur)
}

/// `options` with the root's size, duration and frame rate applied. A beat
/// time as the root's duration ends the composition on that beat.
fn resolve_options(dom: &Html, mut options: CompileOptions, beats: &Beats) -> Result<CompileOptions, CompileError> {
    let root = composition_root(dom, &options)?;
    let beat_end = root
        .value()
        .attr_text("data-duration")
        .map(|v| beats.seconds(v))
        .transpose()
        .map_err(CompileError::Input)?
        .flatten();
    for (attr, target) in [
        ("data-width", &mut options.width),
        ("data-height", &mut options.height),
    ] {
        if let Some(v) = root.value().attr_text(attr) {
            *target = v
                .parse()
                .map_err(|_| CompileError::Input(format!("invalid {attr}: {v}")))?;
        }
    }
    for (attr, target) in [
        ("data-duration", &mut options.duration),
        ("data-fps", &mut options.fps),
    ] {
        if let Some(v) = root.value().attr_text(attr).filter(|_| attr != "data-duration" || beat_end.is_none()) {
            *target = v
                .parse()
                .map_err(|_| CompileError::Input(format!("invalid {attr}: {v}")))?;
        }
    }
    if let Some(end) = beat_end {
        options.duration = end;
    }
    validate_options(&options)?;
    Ok(options)
}

pub(crate) fn compile_with_assets(
    source: &str,
    options: CompileOptions,
    assets: Arc<AssetStore>,
) -> Result<CompiledHtml, CompileError> {
    let mut dom = Html::parse_document(source);
    crate::compose::mount(&mut dom, &assets)?;
    let beats = Rc::new(Beats::new(composition_root(&dom, &options)?, &assets).map_err(CompileError::Input)?);
    let options = resolve_options(&dom, options, &beats)?;
    let root = composition_root(&dom, &options)?.id();
    crate::reel::expand(&mut dom, root, &beats)?;
    let blur = motion_blur(composition_root(&dom, &options)?)?;
    let motion = crate::gsap::run(&mut dom, &options, blur, &assets, &beats)
        .map_err(|e| CompileError::Diagnostics(vec![diagnostic("script", e)]))?;
    let (mut builder, root_id) = build(&dom, options, assets, motion, false, beats.clone())?;
    if !builder.errors.is_empty() {
        return Err(CompileError::Diagnostics(builder.errors));
    }
    if let Some(target) = builder.timeline.keys().next() {
        return Err(CompileError::Input(format!(
            "timeline target '{target}' matches no animatable element"
        )));
    }
    if let Some(el) = builder.motion.series.keys().find_map(|id| dom.tree.get(*id).and_then(ElementRef::wrap)) {
        let name = match el.value().attr_text("id") {
            Some(id) => format!("#{id}"),
            None => format!("<{}>", el.value().name()),
        };
        return Err(CompileError::Diagnostics(vec![diagnostic(
            "script",
            format!("GSAP animates {name}, which is not an animatable box inside the composition"),
        )]));
    }
    let timeline = (!builder.emitted.is_empty()).then(|| crate::Timeline {
        format: crate::TIMELINE_FORMAT.into(),
        tracks: std::mem::take(&mut builder.emitted),
    });
    builder
        .director
        .push_scene_root(root_id, builder.options.duration);
    builder.director.compile();
    Ok(CompiledHtml {
        director: builder.director,
        element_ids: builder.ids,
        source_kind: "static".into(),
        sample_rate: None,
        node_count: builder.nodes,
        source_inspection: None,
        assets: builder.assets,
        timeline,
        beat_grid: beats.resolved().cloned(),
    })
}

/// The beat grid of the HTML page at `path`: its music track analysed, or
/// its root's fixed tempo.
pub fn beat_grid_file(path: &Path, mut options: CompileOptions) -> Result<director_core::beats::BeatGrid, CompileError> {
    options.base_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let source = std::fs::read_to_string(path)
        .map_err(|e| CompileError::Input(format!("HTML {}: {e}", path.display())))?;
    let mut dom = Html::parse_document(&source);
    let assets = Arc::new(AssetStore::local(&options.base_dir, false));
    crate::compose::mount(&mut dom, &assets)?;
    let beats = Beats::new(composition_root(&dom, &options)?, &assets).map_err(CompileError::Input)?;
    beats.grid().cloned().map_err(CompileError::Input)
}

/// Lay out `dom` at time 0 for the computed style and geometry page scripts
/// read while they set up.
pub(crate) fn probe(
    dom: &Html,
    options: &CompileOptions,
    assets: &Arc<AssetStore>,
    beats: &Rc<Beats>,
) -> Result<crate::gsap::Probe, CompileError> {
    let (mut builder, root_id) = build(dom, options.clone(), assets.clone(), Default::default(), true, beats.clone())?;
    if !builder.errors.is_empty() {
        return Err(CompileError::Diagnostics(builder.errors));
    }
    let entries = builder.probe.take().unwrap_or_default();
    let director = &mut builder.director;
    director.push_scene_root(root_id, builder.options.duration);
    director.update_motion(0.0);
    director_core::systems::layout::LayoutEngine::default().compute_layout(
        &mut director.scene,
        director.width,
        director.height,
        0.0,
    );
    director.run_post_layout(0.0);
    let mut probe = crate::gsap::Probe::new();
    for entry in entries {
        let Some(node) = director.scene.get_node(entry.node) else { continue };
        let rect = node.layout_rect;
        let (width, height) = (rect.width(), rect.height());
        let mut abs = [rect.left, rect.top];
        let mut parent = node.parent;
        while let Some(id) = parent {
            let Some(p) = director.scene.get_node(id) else { break };
            abs[0] += p.layout_rect.left;
            abs[1] += p.layout_rect.top;
            parent = p.parent;
        }
        let (x, y) = (style::resolve(entry.origin.0, width), style::resolve(entry.origin.1, height));
        let origin = if entry.origin_z != 0.0 {
            format!("{x}px {y}px {}px", entry.origin_z)
        } else {
            format!("{x}px {y}px")
        };
        probe.insert(
            entry.element,
            crate::gsap::Probed {
                transform: crate::gsap::matrix_text(&entry.transform, (width, height)),
                css: entry.css,
                rect: [rect.left, rect.top, width, height],
                abs,
                origin,
                font_size: entry.font_size,
            },
        );
    }
    Ok(probe)
}

/// Compile the composition tree of `dom`. A probe build records every box
/// for layout reads and skips timeline JSON.
fn build(
    dom: &Html,
    options: CompileOptions,
    assets: Arc<AssetStore>,
    motion: crate::gsap::Motion,
    probing: bool,
    beats: Rc<Beats>,
) -> Result<(Builder, NodeId), CompileError> {
    let root = composition_root(dom, &options)?;
    let mut css = Css::default();
    // Explicit video-authoring UA defaults. No host/browser-dependent styles.
    css.append("html, body { display:block; font-size:16px; color:black; font-family:Roboto; } body { margin:8px; } div,section,article,main,header,footer,p,h1,h2,h3,ul,li { display:block; } h1 {font-size:2em;font-weight:bold;margin-top:.67em;margin-bottom:.67em} h2 {font-size:1.5em;font-weight:bold;margin-top:.83em;margin-bottom:.83em} p {margin-top:1em;margin-bottom:1em} b,strong {font-weight:bold} em,i {font-style:italic} img,svg,video {display:block}", options.width, options.height)?;
    for el in dom.select(&Selector::parse("style, link, script").unwrap()) {
        match el.value().name() {
            "style" if !el.ancestors().filter_map(ElementRef::wrap).any(|p|p.value().name()=="svg")=>css.append(&el.text().collect::<String>(), options.width, options.height)?,
            "link" if el.value().attr_text("rel")==Some("stylesheet")=>{
                let href=el.value().attr_text("href").ok_or_else(||CompileError::Input("stylesheet without href".into()))?;
                let bytes=assets.read(href)?;let source=String::from_utf8(bytes).map_err(|e|CompileError::Input(e.to_string()))?;css.append(&source, options.width, options.height)?;
            },
            _=>{},
        }
    }
    let mut director = Director::new(
        options.width,
        options.height,
        options.fps,
        assets.clone(),
        RenderMode::Export,
        None,
    );
    (director.samples_per_frame, director.shutter_angle) = motion_blur(root)?;
    load_fonts(&mut director, &css.fonts, &assets)?;
    let timeline = if probing { BTreeMap::new() } else { crate::timeline::collect(dom, &beats)? };
    let mut builder = Builder {
        director,
        css,
        options,
        ids: BTreeMap::new(),
        images: HashMap::new(),
        errors: Vec::new(),
        nodes: 0,
        assets,
        root_id: None,
        fixed: HashSet::new(),
        svg_fonts: None,
        timeline,
        motion,
        probe: probing.then(Vec::new),
        root_element: root.id(),
        emitted: Vec::new(),
        atomic: HashSet::new(),
        leaves: HashSet::new(),
        flows: 0,
        inherited: Vec::new(),
        clocked: 0,
        beats,
        sounds: HashMap::new(),
    };
    // Compute inherited values along the full ancestor chain, including body resets.
    let mut ancestors: Vec<_> = root.ancestors().filter_map(ElementRef::wrap).collect();
    ancestors.reverse();
    let mut parent = ComputedStyle::default();
    for ancestor in ancestors {
        if ancestor.id() != root.id() {
            parent = builder.css.cascade(ancestor, &parent)?;
        }
    }
    let root_id = builder.node(
        root,
        &parent,
        None,
        0.0,
        true,
        style::ContainingBlock::from_axes(
            builder.options.width as f32,
            builder.options.height as f32,
        ),
        0,
    )?;
    Ok((builder, root_id))
}

/// What a probe build keeps of one box until layout runs.
struct ProbeEntry {
    element: ego_tree::NodeId,
    node: NodeId,
    css: Values,
    transform: String,
    origin: (taffy::style::LengthPercentage, taffy::style::LengthPercentage),
    origin_z: f32,
    font_size: f32,
}

struct Builder {
    director: Director,
    css: Css,
    options: CompileOptions,
    ids: BTreeMap<String, NodeId>,
    images: HashMap<String, skia_safe::Image>,
    errors: Vec<crate::Diagnostic>,
    nodes: usize,
    assets: Arc<AssetStore>,
    root_id: Option<NodeId>,
    fixed: HashSet<NodeId>,
    /// What the page scripts animate, by element.
    motion: crate::gsap::Motion,
    /// Boxes a probe build records for layout reads.
    probe: Option<Vec<ProbeEntry>>,
    root_element: ego_tree::NodeId,
    /// Every timeline track attached so far, in document order.
    emitted: Vec<crate::TimelineTrack>,
    svg_fonts: Option<Arc<usvg::fontdb::Database>>,
    /// Timeline tracks not yet attached, by target id.
    timeline: BTreeMap<String, Vec<crate::timeline::TimelineTrack>>,
    /// Elements compiling as atomic boxes on a text block's lines.
    atomic: HashSet<ego_tree::NodeId>,
    /// Anonymous leaves holding the text of a box beside its other children.
    leaves: HashSet<NodeId>,
    /// Inline formatting contexts with atomic boxes so far.
    flows: u32,
    /// Per open ancestor: its font size and the text animations its
    /// descendants inherit.
    inherited: Vec<(f32, Vec<animation::Animation>)>,
    /// Open ancestors with a `time` track.
    clocked: usize,
    /// The beat grid that beat-valued times resolve against.
    beats: Rc<Beats>,
    /// Decoded `data-sfx` files by source.
    sounds: HashMap<String, Vec<f32>>,
}

/// The HTML element on scene node `id`.
fn html_mut(director: &mut Director, id: NodeId) -> &mut HtmlElement {
    director
        .scene
        .get_node_mut(id)
        .and_then(|node| node.element.as_any_mut().downcast_mut::<HtmlElement>())
        .expect("compiled nodes are HTML elements")
}

fn label(el: ElementRef<'_>) -> String {
    el.value()
        .attr_text("id")
        .map(|id| format!("#{id}"))
        .unwrap_or_else(|| el.value().name().into())
}
fn inline(tag: &str) -> bool {
    matches!(tag, "span" | "b" | "strong" | "em" | "i" | "br" | "small" | "code")
}
pub fn text_content(spans: Vec<TextSpan>, paint: &PaintStyle, director: &Director) -> TextNode {
    let mut node = TextNode::new(spans, director.assets.font_collection.clone());
    node.default_font_size = director_core::animation::Animated::new(paint.font_size);
    node.default_color = director_core::animation::Animated::new(paint.color);
    node.set_text_align(paint.align);
    node.shadows = paint.text_shadows.clone();
    prepare_paragraph(&mut node, paint);
    node
}
fn span_font_style(weight: u16, italic: bool) -> skia_safe::FontStyle {
    skia_safe::FontStyle::new(
        skia_safe::font_style::Weight::from(weight as i32),
        skia_safe::font_style::Width::NORMAL,
        if italic {
            skia_safe::font_style::Slant::Italic
        } else {
            skia_safe::font_style::Slant::Upright
        },
    )
}

fn build_paragraph(
    node: &TextNode,
    fonts: &FontCollection,
    paint: &PaintStyle,
    width: f32,
) -> skia_safe::textlayout::Paragraph {
    let wrap = wrap_width(node, fonts, paint, width);
    build_paragraph_colored(node, fonts, paint, width, wrap, false, TextPass::Combined)
}

/// The width `append_wrapped` breaks lines at. It adds up unkerned token
/// advances, which run wider than the shaped line, and the gap grows with font
/// size. Text whose shaped lines already fit `width` keeps its own breaks, so a
/// box sized to its max-content width holds the same lines it was measured with.
fn wrap_width(node: &TextNode, fonts: &FontCollection, paint: &PaintStyle, width: f32) -> f32 {
    if paint.no_wrap || !width.is_finite() || width <= 0.0 {
        return width;
    }
    let mut probe = build_paragraph_colored(node, fonts, paint, width, f32::INFINITY, false, TextPass::Fill);
    probe.layout(f32::INFINITY);
    if probe.longest_line() <= width + WRAP_SLACK {
        f32::INFINITY
    } else {
        width
    }
}

/// The text paint one paragraph carries.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TextPass {
    /// One paint: stroke-only over a transparent fill, stroke-and-fill in the fill colour otherwise.
    Combined,
    /// Fill only. The stroke paints from its own paragraph.
    Fill,
    /// Stroke only, in the stroke colour.
    Stroke,
}

/// A stroke colour that differs from an opaque fill paints from its own paragraph, ordered by `paint-order`.
fn separate_stroke(node: &TextNode, paint: &PaintStyle) -> bool {
    paint.text_stroke_width > 0.0
        && paint.text_stroke_color.is_some_and(|ink| {
            node.spans.iter().any(|span| {
                let fill = span.color.unwrap_or(paint.color);
                fill.a > 0.0 && fill != ink
            })
        })
}

/// Builds the fill paragraph, plus the stroke paragraph when the stroke paints on its own.
fn text_paragraphs(
    node: &TextNode,
    fonts: &FontCollection,
    paint: &PaintStyle,
    width: f32,
) -> (
    skia_safe::textlayout::Paragraph,
    Option<skia_safe::textlayout::Paragraph>,
) {
    if separate_stroke(node, paint) {
        let wrap = wrap_width(node, fonts, paint, width);
        (
            build_paragraph_colored(node, fonts, paint, width, wrap, false, TextPass::Fill),
            Some(build_paragraph_colored(node, fonts, paint, width, wrap, false, TextPass::Stroke)),
        )
    } else {
        (build_paragraph(node, fonts, paint, width), None)
    }
}

fn build_paragraph_colored(
    node: &TextNode,
    fonts: &FontCollection,
    paint: &PaintStyle,
    width: f32,
    wrap: f32,
    force_white: bool,
    pass: TextPass,
) -> skia_safe::textlayout::Paragraph {
    use skia_safe::textlayout::{ParagraphBuilder, ParagraphStyle, StrutStyle, TextStyle};
    let mut paragraph = ParagraphStyle::new();
    paragraph.set_text_align(paint.align);
    // ParagraphStyle has no text indent. A leading placeholder shifts only the first line.
    let text_indent = style::resolve(
        paint.text_indent,
        if width.is_finite() { width } else { 0.0 },
    );
    if let Some(lines) = paint.line_clamp {
        paragraph.set_ellipsis("\u{2026}");
        paragraph.set_max_lines(lines as usize);
    } else if paint.no_wrap && matches!(paint.text_overflow, style::TextOverflow::Ellipsis) {
        paragraph.set_ellipsis("\u{2026}");
        paragraph.set_max_lines(1);
    }
    paragraph.set_text_direction(if paint.rtl {
        skia_safe::textlayout::TextDirection::RTL
    } else {
        skia_safe::textlayout::TextDirection::LTR
    });
    let mut strut = StrutStyle::new();
    strut.set_font_families(&[paint.font_family.as_str()]);
    strut.set_font_size(paint.font_size);
    let font_style = span_font_style(paint.font_weight, paint.font_style == "italic");
    strut.set_font_style(font_style);
    let mut default_text = TextStyle::new();
    default_text.set_font_size(paint.font_size);
    default_text.set_font_families(&[paint.font_family.as_str()]);
    default_text.set_font_style(font_style);
    if paint.tabular_nums {
        default_text.add_font_feature("tnum", 1);
    }
    paragraph.set_text_style(&default_text);
    let mut collection = fonts.clone();
    let normal_height = collection
        .find_typefaces(&[paint.font_family.as_str()], font_style)
        .first()
        .map(|face| {
            let (_, metrics) =
                skia_safe::Font::from_typeface(face.clone(), paint.font_size).metrics();
            (-metrics.ascent).round() + metrics.descent.round() + metrics.leading.round()
        })
        .unwrap_or(paint.font_size * 1.2);
    strut.set_height(paint.line_height.unwrap_or(normal_height / paint.font_size));
    strut.set_strut_enabled(true);
    strut.set_force_strut_height(paint.line_height.is_some());
    strut.set_height_override(true);
    strut.set_half_leading(true);
    strut.set_leading(0.0);
    paragraph.set_strut_style(strut);
    let mut builder = ParagraphBuilder::new(&paragraph, collection.clone());
    if text_indent != 0.0 {
        builder.add_placeholder(&skia_safe::textlayout::PlaceholderStyle::new(
            text_indent,
            0.0,
            skia_safe::textlayout::PlaceholderAlignment::Baseline,
            skia_safe::textlayout::TextBaseline::Alphabetic,
            0.0,
        ));
    }
    let soft = style::soft_wrap(paint) && !paint.no_wrap;
    let mut line_width = 0.0;
    let mut line_empty = true;
    let mut pieces = Vec::new();
    let mut boxes = paint.placeholders.iter();
    for span in &node.spans {
        if span.text == OBJECT {
            let size = boxes.next().copied().unwrap_or_default();
            if !paint.no_wrap && !line_empty && line_width + size.width > wrap + WRAP_SLACK {
                // The space before the break hangs.
                if let Some(Piece::Text(_, text)) = pieces.last_mut() {
                    if text.ends_with(' ') {
                        text.pop();
                    }
                }
                pieces.push(Piece::Text(Box::new(default_text.clone()), "\n".into()));
                line_width = 0.0;
            }
            pieces.push(Piece::Object(size));
            line_width += size.width;
            line_empty = false;
            continue;
        }
        let size = span.font_size.unwrap_or(paint.font_size);
        let family = span.font_family.as_deref().unwrap_or(&paint.font_family);
        let weight = span.font_weight.unwrap_or(paint.font_weight);
        let italic = span.font_style.as_deref().unwrap_or(&paint.font_style) == "italic";
        let spacing = span.letter_spacing.unwrap_or(paint.letter_spacing);
        let word = span.word_spacing.unwrap_or(paint.word_spacing);
        let style = span_font_style(weight, italic);
        let variations = span.font_variations.as_deref().unwrap_or(&[]);
        let font = collection
            .find_typefaces(&[family], style)
            .into_iter()
            .next()
            .map(|face| skia_safe::Font::from_typeface(director_core::node::text::vary_typeface(face, variations), size));
        let mut piece = String::new();
        style::append_wrapped(
            &mut piece,
            &span.text,
            if paint.no_wrap {
                f32::INFINITY
            } else {
                wrap + WRAP_SLACK
            },
            soft,
            spacing,
            &mut line_width,
            &mut line_empty,
            paint.break_all,
            paint.pre_line,
            |sample| {
                if sample.is_empty() {
                    return 0.0;
                }
                let advance = font
                    .as_ref()
                    .map(|font| font.measure_str(sample, None).0)
                    .unwrap_or(sample.chars().count() as f32 * size * 0.5);
                let gaps = sample.chars().count().saturating_sub(1) as f32;
                let spaces = sample.chars().filter(|c| matches!(c, ' ' | '\t')).count() as f32;
                advance + spacing * gaps + word * spaces
            },
        );
        let mut text = TextStyle::new();
        text.set_font_size(size);
        let css_color = span.color.unwrap_or(paint.color);
        let color = if span.hidden {
            director_core::types::Color { a: 0.0, ..css_color }
        } else if force_white {
            director_core::types::Color::WHITE
        } else {
            css_color
        };
        text.set_color(color.to_skia());
        text.set_font_families(&[family]);
        text.set_font_style(style);
        text.set_letter_spacing(spacing);
        text.set_word_spacing(word);
        director_core::node::text::set_font_variations(&mut text, variations);
        if paint.tabular_nums {
            text.add_font_feature("tnum", 1);
        }
        if paint.text_stroke_width > 0.0 && pass != TextPass::Fill && !span.hidden {
            // A transparent fill or a stroke pass is stroke-only.
            // A combined opaque fill uses stroke-and-fill in that color so the fill still inks.
            let mut ink = skia_safe::Paint::default();
            ink.set_anti_alias(true);
            ink.set_stroke_width(paint.text_stroke_width);
            ink.set_stroke_join(skia_safe::PaintJoin::Round);
            if pass == TextPass::Stroke || css_color.a == 0.0 {
                ink.set_style(skia_safe::PaintStyle::Stroke);
                let stroke_color = if force_white {
                    color
                } else {
                    paint.text_stroke_color.unwrap_or(css_color)
                };
                ink.set_color(stroke_color.to_skia());
            } else {
                ink.set_style(skia_safe::PaintStyle::StrokeAndFill);
                ink.set_color(color.to_skia());
            }
            text.set_foreground_paint(&ink);
        }
        pieces.push(Piece::Text(Box::new(text), piece));
    }
    for piece in pieces {
        match piece {
            Piece::Text(style, text) => {
                builder.push_style(&style);
                builder.add_text(&text);
                builder.pop();
            }
            Piece::Object(size) => {
                builder.add_placeholder(&skia_safe::textlayout::PlaceholderStyle::new(
                    size.width,
                    size.height,
                    skia_safe::textlayout::PlaceholderAlignment::Baseline,
                    skia_safe::textlayout::TextBaseline::Alphabetic,
                    size.baseline,
                ));
            }
        }
    }
    director_core::counters::count(director_core::counters::Counter::paragraphs_shaped);
    builder.build()
}

/// One run of a paragraph under construction.
enum Piece {
    Text(Box<skia_safe::textlayout::TextStyle>, String),
    /// An atomic inline box's margin box.
    Object(director_scene::html::InlinePlaceholder),
}

/// The text a text block holds for each atomic inline box.
const OBJECT: &str = "\u{FFFC}";

pub fn glyph_mask(
    node: &TextNode,
    paint: &PaintStyle,
    width: f32,
) -> skia_safe::textlayout::Paragraph {
    let fonts = node.font_collection.lock().unwrap();
    let wrap = wrap_width(node, &fonts, paint, width);
    let mut paragraph = build_paragraph_colored(node, &fonts, paint, width, wrap, true, TextPass::Combined);
    fit_paragraph(&mut paragraph, paint, width);
    paragraph
}

/// Skia's font collection caches typefaces without a lock, and elements update
/// on several threads, so every paragraph build and layout holds the
/// collection's mutex. Take it before a paragraph's own lock.
pub fn prepare_paragraph(node: &mut TextNode, paint: &PaintStyle) {
    let shared = node.font_collection.clone();
    let fonts = shared.lock().unwrap();
    let (paragraph, stroke) = text_paragraphs(node, &fonts, paint, f32::INFINITY);
    node.set_paragraphs(paragraph, stroke);
    node.stroke_over_fill = paint.stroke_over_fill;
    node.dirty_layout = false;
}

/// Taffy rounds each box edge to a whole pixel, so a box sized to its text's
/// max-content width can come out up to a pixel narrower than the text. A line
/// may run this far past its box before the next word wraps.
const WRAP_SLACK: f32 = 1.0;

/// Skia breaks a cluster wider than the layout width. Wrap to `width` first.
/// Skia also breaks a line laid out at exactly its own width, so a line within a
/// pixel of `width` lays out a pixel wider than itself. Left alignment lays out at
/// least that wide for any longer line too, so a token that CSS would overflow
/// stays on one line; other alignments do so for a line within `WRAP_SLACK`. An
/// ellipsized nowrap line, and a line-clamp, keep `width`, so the ellipsis stays
/// inside the box.
///
/// A paragraph built for the same width since the text last changed stands.
pub fn layout_paragraph(node: &TextNode, paint: &PaintStyle, width: f32) -> f32 {
    if let Some(used) = node.paragraph.lock().unwrap().as_ref().and_then(|p| p.fit(width)) {
        return used;
    }
    let fonts = node.font_collection.lock().unwrap();
    let (mut paragraph, stroke) = text_paragraphs(node, &fonts, paint, width);
    let used = fit_paragraph(&mut paragraph, paint, width);
    node.set_paragraphs(paragraph, stroke);
    if let Some(paragraph) = node.paragraph.lock().unwrap().as_mut() {
        paragraph.set_fit(width, used);
    }
    used
}

/// Lays `paragraph` out for a box `width` wide, as `layout_paragraph` describes, and returns the width used.
fn fit_paragraph(paragraph: &mut skia_safe::textlayout::Paragraph, paint: &PaintStyle, width: f32) -> f32 {
    let finite = width.is_finite() && width > 0.0;
    let ellipsis = paint.no_wrap && matches!(paint.text_overflow, style::TextOverflow::Ellipsis);
    let clamped = paint.line_clamp.is_some();
    if !finite {
        paragraph.layout(f32::INFINITY);
        // Skia aligns a centred or right line against an infinite width, so lay it out at its own.
        if paint.align != skia_safe::textlayout::TextAlign::Left {
            paragraph.layout(paragraph.longest_line() + 1.0);
        }
        width
    } else if ellipsis || clamped {
        paragraph.layout(width);
        width
    } else {
        paragraph.layout(f32::INFINITY);
        let longest = paragraph.longest_line();
        let left = paint.align == skia_safe::textlayout::TextAlign::Left;
        let used = if longest > width - 1.0 && (left || longest <= width + WRAP_SLACK) {
            longest + 1.0
        } else {
            width
        };
        paragraph.layout(used);
        used
    }
}
pub fn span(text: String, paint: &PaintStyle) -> TextSpan {
    TextSpan {
        text: style::transform_text(&text, paint.text_transform),
        color: None,
        font_family: Some(paint.font_family.clone()),
        font_weight: Some(paint.font_weight),
        font_style: Some(paint.font_style.clone()),
        font_size: Some(paint.font_size),
        letter_spacing: Some(paint.letter_spacing),
        word_spacing: Some(paint.word_spacing),
        line_height: paint.line_height,
        font_variations: paint.span_variations(),
        ..Default::default()
    }
}

/// How a text block lays out one element child.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    /// Text that flows on the block's lines.
    Inline,
    /// An atomic box on the block's lines: an `inline-block`, or an inline box
    /// that GSAP or the timeline animates.
    Atomic,
    Block,
    /// An absolutely or fixed positioned box, out of the block's flow: it
    /// compiles beside the block's text.
    Positioned,
    /// `display: none`.
    Skip,
}

impl Builder {
    /// Whether GSAP or the timeline animates `el`.
    fn animated(&self, el: ElementRef<'_>) -> bool {
        let path = crate::timeline::element_path(el, self.root_element);
        self.motion.series.contains_key(&el.id())
            || el
                .value()
                .attr_text("id")
                .into_iter()
                .chain([path.as_str()])
                .any(|key| self.timeline.contains_key(key))
    }

    /// How a text block lays out `child`, whose cascaded values are `values`.
    fn level(&self, child: ElementRef<'_>, values: &ComputedStyle) -> Level {
        let tag = child.value().name();
        if tag == "br" {
            return Level::Inline;
        }
        let animated = || if self.animated(child) { Level::Atomic } else { Level::Inline };
        let positioned = matches!(values.get("position").map(|p| p.trim()), Some("absolute" | "fixed"));
        match values.get("display").map(|d| d.trim()) {
            Some("none") => Level::Skip,
            _ if positioned => Level::Positioned,
            Some("inline-block") => Level::Atomic,
            Some("inline") => animated(),
            None if inline(tag) => animated(),
            _ => Level::Block,
        }
    }

    /// Whether GSAP or the timeline sets the text of `el`.
    fn text_tracked(&self, el: ElementRef<'_>) -> bool {
        let path = crate::timeline::element_path(el, self.root_element);
        self.motion.series.get(&el.id()).is_some_and(|s| s.contains_key("#text"))
            || el.value().attr_text("id").into_iter().chain([path.as_str()]).any(|key| {
                self.timeline.get(key).is_some_and(|tracks| tracks.iter().any(|t| t.property == "text"))
            })
    }
}

/// The spans of `text` set as an element's whole text content.
pub(crate) fn text_spans(text: &str, paint: &PaintStyle) -> Vec<TextSpan> {
    let mut spans = vec![span(text.to_string(), paint)];
    normalize(&mut spans, paint.preserve_space);
    spans
}

impl Builder {
    fn ensure_svg_fonts(&mut self) -> Result<Arc<usvg::fontdb::Database>, CompileError> {
        cached_svg_fonts(&mut self.svg_fonts, &self.assets, &self.css.fonts)
    }

    /// Play `src` once from `start`, the moment the clip `el` cuts in.
    fn sound_effect(&mut self, el: ElementRef<'_>, context: &str, src: &str, start: f64) -> Result<(), CompileError> {
        if self.clocked > 0 {
            return Err(CompileError::Diagnostics(vec![diagnostic(
                context,
                "a sound effect inside a time track plays on the composition clock".to_string(),
            )]));
        }
        let samples = match self.sounds.get(src) {
            Some(samples) => samples.clone(),
            None => {
                let samples = director_core::audio::load_audio_bytes(&self.assets.read(src)?, 48000)
                    .map_err(|e| CompileError::Input(format!("sound effect '{src}': {e}")))?;
                self.sounds.insert(src.into(), samples.clone());
                samples
            }
        };
        let volume = match el.value().attr_text("data-sfx-volume") {
            None => 1.0,
            Some(v) => v.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v >= 0.0).ok_or_else(|| {
                CompileError::Diagnostics(vec![diagnostic(
                    context,
                    format!("data-sfx-volume must be finite and nonnegative (got '{v}')"),
                )])
            })?,
        };
        let hidden = el
            .ancestors()
            .filter_map(ElementRef::wrap)
            .chain([el])
            .any(|el| el.value().attr_text("data-hidden").is_some_and(|v| v != "false"));
        let seconds = samples.len() as f64 / 96000.0;
        let id = self.director.add_scene_audio(samples, start, seconds);
        if let Some(track) = &mut self.director.audio_mixer.tracks[id] {
            track.volume = director_core::animation::Animated::new(if hidden { 0.0 } else { volume });
            if !src.starts_with("data:") {
                track.source_path = Some(src.into());
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn node(
        &mut self,
        el: ElementRef<'_>,
        parent: &ComputedStyle,
        cascaded: Option<ComputedStyle>,
        composition_start: f64,
        root: bool,
        containing_block: style::ContainingBlock,
        depth: usize,
    ) -> Result<NodeId, CompileError> {
        if depth > 256 || self.nodes > 100000 {
            return Err(CompileError::Input(
                "HTML tree exceeds compiler limits".into(),
            ));
        }
        let tag = el.value().name();
        let context = label(el);
        let mut values = match cascaded {
            Some(values) => values,
            None => self.css.cascade(el, parent)?,
        };
        if tag == "svg" && !self.css.has_local_property(el, "color") {
            if let Some(color) = el.value().attr_text("color") {
                values.insert("color", color);
            }
        }
        if matches!(tag, "img" | "svg" | "video" | "model") {
            for dimension in ["width", "height"] {
                if !values.contains_key(dimension) {
                    if let Some(value) = el.value().attr_text(dimension) {
                        let value = value
                            .parse::<f32>()
                            .map(|v| format!("{v}px"))
                            .unwrap_or_else(|_| value.into());
                        values.insert(dimension, value);
                    }
                }
            }
        }
        let parent_size = parent
            .get("font-size")
            .and_then(|s| {
                style::px(
                    s,
                    Metrics {
                        width: self.options.width as f32,
                        height: self.options.height as f32,
                        font_size: 16.0,
                    },
                )
                .ok()
            })
            .unwrap_or(16.0);
        let metrics = Metrics {
            width: self.options.width as f32,
            height: self.options.height as f32,
            font_size: parent_size,
        };
        let style_block = if values.get("position").map(String::as_str) == Some("fixed") {
            style::ContainingBlock::from_axes(
                self.options.width as f32,
                self.options.height as f32,
            )
        } else {
            containing_block
        };
        // An animated inline box lays out as a block on its parent's lines.
        let authored_display = values.get("display").cloned();
        let promoted = self.atomic.contains(&el.id()) && authored_display.as_deref() != Some("inline-block");
        if promoted {
            values.insert("display", "block");
        }
        let (mut layout, mut paint, _grid_scope) =
            style::translate(&values, metrics, style_block, &self.assets).map_err(
                |errors| {
                    CompileError::Diagnostics(
                        errors
                            .into_iter()
                            .map(|e| diagnostic(&context, e))
                            .collect(),
                    )
                },
            )?;
        // Store computed font-size to avoid repeated relative-unit multiplication in descendants.
        let specified_font = values.get("font-size").cloned();
        values.insert("font-size", format!("{}px", paint.font_size));
        if promoted {
            match authored_display {
                Some(display) => values.insert("display", display),
                None => values.remove("display"),
            }
        }
        if root {
            layout.size = taffy::geometry::Size {
                width: taffy::prelude::length(self.options.width as f32),
                height: taffy::prelude::length(self.options.height as f32),
            };
            layout.margin = taffy::geometry::Rect::zero();
        }
        for (name, _) in el.value().attrs() {
            if name.starts_with("data-")
                && !matches!(
                    name,
                    "data-composition-id"
                        | "data-composition-src"
                        | "data-width"
                        | "data-height"
                        | "data-duration"
                        | "data-start"
                        | "data-end"
                        | "data-fps"
                        | "data-motion-blur"
                        | "data-shutter-angle"
                        | "data-track-index"
                        | "data-hidden"
                        | "data-no-timeline"
                        | "data-media-start"
                        | "data-playback-rate"
                        | "data-volume"
                        | "data-hf-id"
                        | "data-hf-render-id"
                        | "data-label"
                        | "data-timeline-role"
                        | "data-bpm"
                        | "data-beat-offset"
                        | "data-beats-per-bar"
                        | "data-sfx"
                        | "data-sfx-volume"
                )
            {
                self.errors.push(diagnostic(
                    &context,
                    format!("unsupported HyperFrames attribute '{name}'"),
                ));
            }
        }
        let numeric = |name: &str, default: f64| -> Result<f64, CompileError> {
            let v = el
                .value()
                .attr_text(name)
                .map(|v| {
                    v.parse::<f64>().map_err(|_| {
                        CompileError::Input(format!("{context}: {name} must be numeric"))
                    })
                })
                .transpose()?
                .unwrap_or(default);
            if !v.is_finite() {
                return Err(CompileError::Input(format!("{name} must be finite")));
            }
            Ok(v)
        };
        if !root {
            if let Some(name) = ["data-bpm", "data-beat-offset", "data-beats-per-bar"]
                .into_iter()
                .find(|name| el.value().attr_text(name).is_some())
            {
                return Err(CompileError::Diagnostics(vec![diagnostic(
                    &context,
                    format!("{name} belongs on the composition root"),
                )]));
            }
        }
        // Beat times name points on the composition's beat grid; seconds count from the composition start.
        let beat_error = |e: String| CompileError::Diagnostics(vec![diagnostic(&context, e)]);
        let beats = self.beats.clone();
        let at = |name: &str| -> Result<Option<f64>, CompileError> {
            Ok(el.value().attr_text(name).map(|v| beats.seconds(v)).transpose().map_err(beat_error)?.flatten())
        };
        let start = if root {
            0.0
        } else if let Some(start) = at("data-start")? {
            start
        } else {
            composition_start + numeric("data-start", 0.0)?
        };
        let end = if root {
            self.options.duration
        } else if let Some(span) = el.value().attr_text("data-duration") {
            match beats.span_end(start, span).map_err(beat_error)? {
                Some(end) => end,
                None => start + numeric("data-duration", 0.0)?,
            }
        } else if let Some(end) = at("data-end")? {
            end
        } else if el.value().attr_text("data-end").is_some() {
            composition_start + numeric("data-end", self.options.duration)?
        } else {
            self.options.duration
        };
        if start < 0.0 || end <= start {
            return Err(CompileError::Input(format!(
                "{context}: invalid clip window [{start}, {end})"
            )));
        }
        if let Some(src) = el.value().attr_text("data-sfx") {
            self.sound_effect(el, &context, src, start)?;
        }
        let mut svg_viewport: Option<(f32, f32)> = None;
        let mut atomics = Vec::new();
        let mut positioned = Vec::new();
        // Child styles cascaded over `values` while classifying the children,
        // kept for the children's own builds.
        let mut cascaded = HashMap::new();
        let content: Option<Box<dyn Element>> = match tag {
            "img" => {
                let src = el
                    .value()
                    .attr_text("src")
                    .ok_or_else(|| CompileError::Input(format!("{context}: image without src")))?;
                if el.value().attr_text("srcset").is_some() {
                    return Err(CompileError::Input(
                        "responsive srcset requires extraction".into(),
                    ));
                }
                let image = if let Some(image) = self.images.get(src) {
                    image.clone()
                } else {
                    let bytes = self.assets.read(src)?;
                    director_core::counters::count(director_core::counters::Counter::image_decodes);
                    let image =
                        skia_safe::Image::from_encoded(skia_safe::Data::new_copy(&bytes))
                            .ok_or_else(|| CompileError::Input(format!("invalid image '{src}'")))?;
                    self.images.insert(src.into(), image.clone());
                    image
                };
                let mut image = ImageNode::from_image(Some(image));
                image.object_fit = fit(&values);
                image.object_position = paint.object_position;
                Some(Box::new(image))
            }
            "svg" => {
                validate_svg(&el.html())?;
                let clock = (start, numeric("data-media-start", 0.0)?, numeric("data-playback-rate", 1.0)?);
                let plan = crate::smil::collect(el, clock)?;
                for child in el
                    .descendants()
                    .filter_map(ElementRef::wrap)
                    .filter(|child| child.id() != el.id())
                {
                    let declared = self.css.stylesheet_properties(child);
                    for property in SVG_ANIMATION_PROPERTIES {
                        if declared.contains(property) {
                            return Err(CompileError::Input(format!("{}: external stylesheet 'animation' inside SVG requires extraction; author SVG presentation attributes or an internal stylesheet", label(child))));
                        }
                    }
                }
                // Wrapper CSS opacity/transforms paint the SVG once. Keep SVG-local
                // attributes and internal styles, and freeze inherited currentColor.
                let mut markup = String::from("<svg");
                for (name, value) in el.value().attrs().filter(|(name, _)| *name != "style") {
                    markup.push_str(&format!(
                        " {name}=\"{}\"",
                        value.replace('&', "&amp;").replace('"', "&quot;")
                    ));
                }
                if el.value().attr_text("xmlns").is_none() {
                    markup.push_str(" xmlns=\"http://www.w3.org/2000/svg\"");
                }
                if let Some(id) = plan.target_id("") {
                    markup.push_str(&format!(" director-smil-id=\"{id}\""));
                }
                markup.push_str(&format!(
                    " style=\"color:rgba({},{},{},{})\">",
                    paint.color.r * 255.0,
                    paint.color.g * 255.0,
                    paint.color.b * 255.0,
                    paint.color.a
                ));
                markup.push_str(&serialize_svg_children(el, &self.css, &values, &plan)?);
                markup.push_str("</svg>");
                let fonts = self.ensure_svg_fonts()?;
                let (width, height, content_node): (f32, f32, Box<dyn Element>) = if plan.is_empty() {
                    let vector = parse_svg(&markup, &context, &fonts)?;
                    let size = vector.size();
                    (
                        size.width(),
                        size.height(),
                        Box::new(VectorNode::from_tree(vector)),
                    )
                } else {
                    let animated = crate::smil::AnimatedSvg::new(
                        markup,
                        Arc::clone(&fonts),
                        &plan,
                        &context,
                    )?;
                    let (width, height) = animated.size();
                    (width, height, Box::new(animated))
                };
                svg_viewport = Some((width, height));
                if !values.contains_key("width") {
                    layout.size.width = taffy::prelude::length(width);
                }
                if !values.contains_key("height") {
                    layout.size.height = taffy::prelude::length(height);
                }
                Some(content_node)
            }
            "video" => {
                if el.value().attr_text("muted").is_none() {
                    return Err(CompileError::Input(
                        "video audio needs an explicit audio element; author muted video".into(),
                    ));
                }
                let src = el
                    .value()
                    .attr_text("src")
                    .ok_or_else(|| CompileError::Input("video without src".into()))?;
                let path = self.assets.media_path(src)?;
                let looped = el.value().attr_text("loop").is_some();
                let duration = if looped { media_duration(&path)? } else { 0.0 };
                let mut video =
                    VideoNode::shared(VideoSource::Path(path), RenderMode::Export, &self.director.assets.video_sources);
                // The document references the file through the compile asset loader.
                if !src.starts_with("data:") {
                    video = video.with_source_path(src);
                }
                video.object_fit = fit(&values);
                video.object_position = paint.object_position;
                let video: Box<dyn Element> = if looped {
                    Box::new(LoopedVideo {
                        node: video,
                        duration,
                    })
                } else {
                    Box::new(video)
                };
                Some(video)
            }
            "model" => {
                let attr = |name: &str| el.value().attr_text(name);
                let input = |e: String| CompileError::Input(format!("{context}: {e}"));
                let splats = model_splats(el, &self.assets, &context)?;
                let texts = model_texts(el, &self.assets, &self.director.assets.font_collection).map_err(input)?;
                let asset = match attr("src") {
                    Some(src) => {
                        let path = (!src.starts_with("data:")).then(|| src.to_string());
                        director_3d::Asset::new(self.assets.read(src)?, "glb", path)
                    }
                    None if !splats.is_empty() || !texts.is_empty() => director_3d::Asset::empty_scene(),
                    None => {
                        return Err(CompileError::Input(format!(
                            "{context}: model without src, <splat> or <text3d> children"
                        )))
                    }
                };
                let mut model = director_3d::ModelNode::new(asset).map_err(input)?;
                model.splats = std::sync::Arc::new(splats);
                model.texts = std::sync::Arc::new(texts);
                let catchers: Vec<String> =
                    attr("shadow-catcher").map(|v| v.split_whitespace().map(str::to_string).collect()).unwrap_or_default();
                if !catchers.is_empty() {
                    let names = director_3d::node_names(&model.model.bytes).map_err(input)?;
                    if let Some(missing) = catchers.iter().find(|c| !names.contains(c)) {
                        return Err(input(format!("shadow-catcher '{missing}' names no node in the model")));
                    }
                }
                let reflection = numeric("reflection", 0.0)?;
                if !(0.0..=1.0).contains(&reflection) {
                    return Err(input("reflection must be from 0 to 1".into()));
                }
                if reflection > 0.0 && catchers.is_empty() {
                    return Err(input("reflection needs a shadow-catcher to mirror the model".into()));
                }
                model.reflection = reflection as f32;
                model.shadow_catchers = std::sync::Arc::new(catchers);
                let plate_light = attr("track-light").is_some_and(|v| v != "false");
                if plate_light && attr("track").is_none() {
                    return Err(input("track-light needs a track to light the model from".into()));
                }
                let plate_sun = attr("track-sun").is_some_and(|v| v != "false");
                if plate_sun && attr("track").is_none() {
                    return Err(input("track-sun needs a track to read the sun from".into()));
                }
                if plate_light && attr("environment").is_some() {
                    return Err(input("track-light and environment both light the model; give one".into()));
                }
                let sky = model_sky(el).map_err(input)?;
                if sky.is_some() && (attr("environment").is_some() || plate_light) {
                    return Err(input("a <sky> lights the model, so it cannot take an environment or track-light".into()));
                }
                if sky.is_some() && plate_sun {
                    return Err(input("a <sky> and track-sun both add a sun; give one".into()));
                }
                if let Some(environment) = attr("environment") {
                    let path = (!environment.starts_with("data:")).then(|| environment.to_string());
                    model.environment =
                        Some(director_3d::Asset::environment(self.assets.read(environment)?, path).map_err(input)?);
                }
                // Daylight meters near EV100 14, where a grey card in the
                // sun and sky lands near the middle of the tone curve.
                model.exposure = numeric("exposure", if sky.is_some() { 14.0 } else { 9.7 })? as f32;
                // A probe of the plate's linear light renders each of its
                // values back at that value before the tone curve.
                let unity = 1.2 * 2f64.powf(model.exposure as f64);
                let intensity = match &sky {
                    Some((_, lux)) => *lux,
                    None if plate_light => unity,
                    None => 1000.0,
                };
                model.environment_intensity = numeric("environment-intensity", intensity)? as f32;
                model.background = attr("background").is_some_and(|v| v != "false");
                model.looped = attr("loop").is_some_and(|v| v != "false");
                let samples = numeric("samples", 1.0)?;
                if samples.fract() != 0.0 || !(1.0..=256.0).contains(&samples) {
                    return Err(CompileError::Input(format!("{context}: samples must be a whole number from 1 to 256")));
                }
                let mut lights = model_lights(el).map_err(input)?;
                if let Some((options, lux)) = &sky {
                    let (hdr, sun) = sky_probe(options, *lux);
                    model.environment = Some(director_3d::Asset::environment(hdr, None).map_err(input)?);
                    lights.extend(sun);
                }
                model.lights = std::sync::Arc::new(lights);
                model.samples = samples as u32;
                let paths = numeric("paths", 0.0)?;
                if paths.fract() != 0.0 || !(0.0..=director_3d::MAX_PATHS as f64).contains(&paths) {
                    return Err(CompileError::Input(format!(
                        "{context}: paths must be a whole number from 0 to {}",
                        director_3d::MAX_PATHS
                    )));
                }
                model.paths = paths as u32;
                model.denoise = attr("denoise").is_some_and(|v| v != "false");
                if model.denoise && model.paths == 0 {
                    return Err(CompileError::Input(format!("{context}: denoise needs paths")));
                }
                model.fog = model_fog(el).map_err(input)?;
                if model.fog.is_some() && model.paths > 0 {
                    return Err(input("fog renders at the rasterized levels, so it cannot take paths".into()));
                }
                model.optics = model_optics(el).map_err(input)?;
                // A bare `ambient-occlusion` searches the 0.73 m around each
                // surface that Bevy tunes its SSAO for.
                model.occlusion = match attr("ambient-occlusion").map(str::trim) {
                    None | Some("false") => 0.0,
                    Some("" | "true") => 0.7285,
                    Some(v) => match v.trim_end_matches('m').parse::<f32>() {
                        Ok(r) if r > 0.0 && r.is_finite() => r,
                        _ => return Err(input(format!("ambient-occlusion '{v}' must be a radius in metres above 0"))),
                    },
                };
                if model.occlusion > 0.0 && model.paths > 0 {
                    return Err(input("paths trace occlusion themselves, so they cannot take ambient-occlusion".into()));
                }
                // A bare `screen-reflections` reflects every surface up to a
                // perceptual roughness of 0.6.
                model.screen_reflections = match attr("screen-reflections").map(str::trim) {
                    None | Some("false") => 0.0,
                    Some("" | "true") => 0.6,
                    Some(v) => match v.parse::<f32>() {
                        Ok(r) if r > 0.0 && r <= 1.0 => r,
                        _ => return Err(input(format!("screen-reflections '{v}' must be a roughness above 0 and up to 1"))),
                    },
                };
                if model.screen_reflections > 0.0 && model.paths > 0 {
                    return Err(input("paths trace reflections themselves, so they cannot take screen-reflections".into()));
                }
                // The model's own shutter opens at each frame's time, as the
                // composition's motion blur does.
                let shutter = numeric("shutter", 0.0)?;
                if !(0.0..=360.0).contains(&shutter) {
                    return Err(input(format!("shutter '{shutter}' must be degrees from 0 to 360")));
                }
                if shutter > 0.0 && attr("track").is_some() {
                    return Err(input("a tracked model blurs with track-shutter, so it cannot take shutter".into()));
                }
                if shutter > 0.0 && self.director.samples_per_frame > 1 {
                    return Err(input("data-motion-blur on the composition already blurs the model, so it cannot take shutter".into()));
                }
                model.shutter = shutter / 360.0 / self.director.fps;
                model.base = paint.camera;
                model.view = paint.camera;
                if let Some(plate) = attr("track") {
                    let lens = (attr("track-fov").is_some(), attr("track-distortion"), attr("track-zoom").is_some_and(|v| v != "false"));
                    let plate = (plate, attr("track-mask"), attr("track-pitch"));
                    let (track, probe, sun) = solve_track(&self.assets, plate, &numeric, lens, (plate_light, plate_sun), &context)?;
                    model.track = Some(std::sync::Arc::new(track));
                    if let Some(hdr) = probe {
                        model.environment = Some(director_3d::Asset::environment(hdr, None).map_err(input)?);
                    }
                    if let Some(sun) = sun {
                        let mut lights = (*model.lights).clone();
                        lights.push(sun_light(sun.direction, sun.irradiance, unity));
                        model.lights = std::sync::Arc::new(lights);
                    }
                }
                if !values.contains_key("width") {
                    layout.size.width = taffy::prelude::length(300.0_f32);
                }
                if !values.contains_key("height") {
                    layout.size.height = taffy::prelude::length(150.0_f32);
                }
                Some(Box::new(model))
            }
            "audio" => {
                let src = el
                    .value()
                    .attr_text("src")
                    .ok_or_else(|| CompileError::Input("audio without src".into()))?;
                let mut samples = match self.beats.take_decoded(src) {
                    Some(samples) => samples,
                    None => director_core::audio::load_audio_bytes(&self.assets.read(src)?, 48000)
                        .map_err(|e| CompileError::Input(format!("audio '{src}': {e}")))?,
                };
                let media_start = numeric("data-media-start", 0.0)?;
                if media_start < 0.0 {
                    return Err(CompileError::Input("negative audio media start".into()));
                }
                if self.clocked > 0 {
                    return Err(CompileError::Diagnostics(vec![diagnostic(
                        &context,
                        "audio inside a time track plays on the composition clock".to_string(),
                    )]));
                }
                samples.drain(..((media_start * 96000.0) as usize).min(samples.len()));
                let id = self.director.add_scene_audio(samples, start, end - start);
                if let Some(track) = &mut self.director.audio_mixer.tracks[id] {
                    let volume = numeric("data-volume", 1.0)?;
                    if volume < 0.0 || volume > f32::MAX as f64 {
                        return Err(CompileError::Input(
                            "audio volume must be finite and nonnegative".into(),
                        ));
                    }
                    let hidden = el
                        .ancestors()
                        .filter_map(ElementRef::wrap)
                        .any(|el| el.value().attr_text("data-hidden").is_some_and(|v| v != "false"));
                    track.volume = director_core::animation::Animated::new(if hidden {
                        0.0
                    } else {
                        volume as f32
                    });
                    track.loop_audio = el.value().attr_text("loop").is_some();
                    // The document references the file through the compile asset loader.
                    if !src.starts_with("data:") {
                        track.source_path = Some(src.into());
                        track.source_start = media_start;
                    }
                }
                None
            }
            "foreignobject" => {
                return Err(CompileError::Input(
                    "nested foreignObject requires extraction".into(),
                ));
            }
            "animate" | "animatetransform" | "animatemotion" | "set" => {
                return Err(CompileError::Input(
                    "SMIL inside foreignObject requires extraction".into(),
                ));
            }
            "canvas" | "iframe" | "object" | "embed" => {
                return Err(CompileError::Diagnostics(vec![diagnostic(
                    &context,
                    format!(
                        "{tag} requires a native translator; screenshots are not compiler assets"
                    ),
                )]))
            }
            "body" | "div" | "section" | "article" | "main" | "header" | "footer" | "h1" | "h2"
            | "h3" | "p" | "pre" | "span" | "b" | "strong" | "em" | "i" | "small" | "code" => {
                let children: Vec<_> = el
                    .children()
                    .filter_map(ElementRef::wrap)
                    .filter(|e| !matches!(e.value().name(), "script" | "style" | "link"))
                    .collect();
                let mut text_block = true;
                for child in &children {
                    let child_values = self.css.cascade(*child, &values)?;
                    let level = self.level(*child, &child_values);
                    cascaded.insert(child.id(), child_values);
                    match level {
                        Level::Block => {
                            text_block = false;
                            break;
                        }
                        Level::Positioned => positioned.push(*child),
                        _ => {}
                    }
                }
                if !text_block {
                    positioned.clear();
                }
                if text_block {
                    let mut spans = Vec::new();
                    self.rich_text(el, &values, &paint, style_block, &mut spans, &mut atomics, &mut cascaded, true, paint.visible)?;
                    normalize(&mut spans, paint.preserve_space);
                    if !atomics.is_empty() && self.text_tracked(el) {
                        return Err(CompileError::Input(format!(
                            "{context}: a text track replaces inline-block children; animate a text-only box"
                        )));
                    }
                    if spans.is_empty() && !self.text_tracked(el) {
                        None
                    } else {
                        Some(Box::new(text_content(spans, &paint, &self.director)))
                    }
                } else {
                    if el
                        .children()
                        .any(|n| n.value().as_text().is_some_and(|t| !t.trim().is_empty()))
                    {
                        return Err(CompileError::Input(format!(
                            "{context}: mixed block and inline content requires extraction"
                        )));
                    }
                    None
                }
            }
            _ => {
                return Err(CompileError::Input(format!(
                    "unsupported HTML element '{tag}'"
                )))
            }
        };
        let stretch_width = !root && style::width_stretches(&values, parent);
        let child_block = style::content_box(&layout, style_block, stretch_width);
        let gap_metrics = Metrics {
            font_size: paint.font_size,
            ..metrics
        };
        style::resolve_gaps(&mut layout, &values, gap_metrics, child_block).map_err(|errors| {
            CompileError::Diagnostics(
                errors
                    .into_iter()
                    .map(|error| diagnostic(&context, error))
                    .collect(),
            )
        })?;
        paint.box_node.style = layout.clone();
        let has_content = content.is_some();
        // Absolute atomic boxes and positioned children beside the text need the text in a leaf of its own.
        let anonymous_text = has_content
            && (matches!(layout.display, taffy::Display::Flex | taffy::Display::Grid)
                || !atomics.is_empty()
                || !positioned.is_empty())
            && !matches!(tag, "img" | "svg" | "video" | "model");
        let mut element = HtmlElement::new(
            layout,
            paint.clone(),
            content,
            Metrics {
                font_size: paint.font_size,
                ..metrics
            },
        );
        element.start = start;
        element.end = end;
        element.hidden = el.value().attr_text("data-hidden").is_some_and(|v| v != "false");
        element.media_start = numeric("data-media-start", 0.0)?;
        element.playback_rate = numeric("data-playback-rate", 1.0)?;
        if element.media_start < 0.0 || element.playback_rate <= 0.0 {
            return Err(CompileError::Input(
                "media start must be nonnegative and rate positive".into(),
            ));
        }
        if tag == "audio" && element.playback_rate != 1.0 {
            return Err(CompileError::Input(
                "audio playback-rate translation unsupported".into(),
            ));
        }
        element.animations = animation::compile(&values, &self.css, &paint, metrics)
            .map_err(|e| CompileError::Diagnostics(vec![diagnostic(&context, e)]))?;
        let path = crate::timeline::element_path(el, self.root_element);
        let mut tracks = Vec::new();
        for key in el.value().attr_text("id").into_iter().chain([path.as_str()]) {
            tracks.extend(self.timeline.remove(key).unwrap_or_default());
        }
        if let Some(mut series) = self.motion.series.remove(&el.id()) {
            if promoted {
                // Transforms and sizes do not apply to inline boxes.
                series.retain(|property, _| !matches!(property.as_str(), "transform" | "width" | "height"));
            }
            let steps = self.motion.steps.remove(&el.id()).unwrap_or_default();
            let target = el.value().attr_text("id").map_or_else(|| path.clone(), str::to_string);
            let base = |channel: &str| {
                crate::gsap::base_value(channel, &element.base_style, &element.base_paint)
            };
            let shadows = |text: &str| crate::style::parse_box_shadows(text, metrics, element.base_paint.color);
            let text_shadows = |text: &str| crate::style::parse_text_shadows(text, metrics, element.base_paint.color);
            let clip = |text: Option<&str>| match text {
                Some(text) => crate::style::clip_points(text, &values, metrics),
                None => Ok(element.base_paint.box_node.clip_path.clone()),
            };
            let positions = |text: &str| crate::style::background_position_layers(text, metrics);
            let sizes = |text: Option<&str>| match text {
                Some(text) => crate::style::background_size_layers(text, metrics),
                None => Ok(element.base_paint.background_size.clone()),
            };
            let shapes = crate::gsap::Shapes {
                shadows: &shadows,
                text_shadows: &text_shadows,
                clip: &clip,
                positions: &positions,
                sizes: &sizes,
            };
            let mut emitted = crate::gsap::tracks(&series, &steps, &target, &base, &shapes, &self.motion.times)
                .map_err(|e| CompileError::Diagnostics(vec![diagnostic(&context, e)]))?;
            // A side whose border style is none or hidden draws no border at any width.
            emitted.retain(|track| {
                let Some((side, "width")) = crate::animation::border_side(&track.property) else { return true };
                let side = ["top", "right", "bottom", "left"][side];
                values.get(&format!("border-{side}-style")).is_some_and(|s| s != "none" && s != "hidden")
            });
            tracks.extend(emitted);
        }
        for (i, track) in tracks.iter().enumerate() {
            if tracks[..i].iter().any(|t| t.property == track.property) {
                return Err(CompileError::Diagnostics(vec![diagnostic(
                    &context,
                    format!("the timeline animates '{}' twice on one element", track.property),
                )]));
            }
        }
        // A time track gives the element's descendants their own clock.
        if let Some(i) = tracks.iter().position(|track| track.property == "time") {
            let track = tracks.remove(i);
            if has_content || tag == "audio" {
                return Err(CompileError::Diagnostics(vec![diagnostic(
                    &context,
                    "a time track clocks child elements, so its element needs children and no own content"
                        .to_string(),
                )]));
            }
            element.time_map = Some(
                animation::TimeMap::from_track(&track)
                    .map_err(|e| CompileError::Diagnostics(vec![diagnostic(&context, e)]))?,
            );
            self.emitted.push(track);
        }
        let clocks = element.time_map.is_some();
        // Timeline tracks play like inline styles, so a running @keyframes
        // animation of the same property applies after them and wins.
        let keyframes = std::mem::take(&mut element.animations);
        element.animations = self.inherit_text(el, specified_font.as_deref(), paint.font_size, tag);
        for track in tracks {
            let animation = animation::from_track(&track, &paint, metrics)
                .map_err(|e| CompileError::Diagnostics(vec![diagnostic(&context, e)]))?;
            element.animations.push(animation);
            self.emitted.push(track);
        }
        animation::check_components(&element.animations, &element.base_paint.transform)
            .map_err(|e| CompileError::Diagnostics(vec![diagnostic(&context, e)]))?;
        element.animations.extend(keyframes);
        // Descendants inherit every text animation that reaches this element.
        let passed: Vec<animation::Animation> =
            element.animations.iter().filter_map(|a| a.inherited(|_| true, 1.0)).collect();
        if clocks && !passed.is_empty() {
            return Err(CompileError::Diagnostics(vec![diagnostic(
                &context,
                "text animations cannot pass through a time track to its descendants".to_string(),
            )]));
        }
        self.inherited.push((paint.font_size, passed));
        self.clocked += usize::from(clocks);
        let id = self.director.scene.add_node(Box::new(element));
        if let Some(probe) = &mut self.probe {
            probe.push(ProbeEntry {
                element: el.id(),
                node: id,
                css: (*values).clone(),
                transform: values.get("transform").cloned().unwrap_or_else(|| "none".into()),
                origin: paint.origin,
                origin_z: paint.origin_z,
                font_size: paint.font_size,
            });
        }
        if root {
            self.root_id = Some(id);
        }
        if values.get("position").map(String::as_str) == Some("fixed") {
            self.fixed.insert(id);
        }
        self.nodes += 1;
        self.director.scene.get_node_mut(id).unwrap().z_index = paint.z_index;
        if anonymous_text {
            let parent = self
                .director
                .scene
                .get_node_mut(id)
                .unwrap()
                .element
                .as_any_mut()
                .downcast_mut::<HtmlElement>()
                .unwrap();
            let content = parent.content.take();
            // The leaf's text inherits the element's animated text styles.
            let text_parts: Vec<_> = parent.animations.iter().filter_map(crate::animation::Animation::text_part).collect();
            let mut anonymous = paint.clone();
            anonymous.box_node = director_core::node::BoxNode::new();
            anonymous.opacity = 1.0;
            anonymous.transform = Default::default();
            anonymous.corners = [(taffy::prelude::zero(), taffy::prelude::zero()); 4];
            anonymous.filters = Vec::new();
            anonymous.filter = Default::default();
            anonymous.shadows.clear();
            anonymous.layers.clear();
            anonymous.decoration = Default::default();
            anonymous.mask = None;
            anonymous.mask_luminance = false;
            let child = self.director.scene.add_node(Box::new(HtmlElement::new(
                taffy::Style {
                    display: taffy::Display::Block,
                    ..Default::default()
                },
                anonymous,
                content,
                metrics,
            )));
            if !text_parts.is_empty() {
                let leaf = self.director.scene.get_node_mut(child).unwrap().element.as_any_mut();
                leaf.downcast_mut::<HtmlElement>().unwrap().animations = text_parts;
            }
            self.director.scene.add_child(id, child);
            self.leaves.insert(child);
            self.nodes += 1;
            if !atomics.is_empty() {
                let next_start = if !root && el.value().attr_text("data-composition-id").is_some() {
                    start
                } else {
                    composition_start
                };
                self.place_atomics(id, child, &atomics, next_start, child_block, depth)?;
            }
        }
        let next_start = if !root && el.value().attr_text("data-composition-id").is_some() {
            start
        } else {
            composition_start
        };
        let children = if has_content {
            positioned
        } else if tag == "audio" {
            Vec::new()
        } else {
            el.children()
                .filter_map(ElementRef::wrap)
                .filter(|child| !matches!(child.value().name(), "script" | "style" | "link"))
                .collect()
        };
        if let Some(name) = el.value().attr_text("id") {
            if self.ids.insert(name.into(), id).is_some() {
                return Err(CompileError::Input(format!("duplicate HTML id '{name}'")));
            }
        }
        for child in children {
            let child_id = self.node(child, &values, cascaded.remove(&child.id()), next_start, false, child_block, depth + 1)?;
            // Taffy absolute offsets use the parent node, so fixed is a child of the composition root.
            if !root && self.fixed.contains(&child_id) {
                if self.clocked > 0 {
                    return Err(CompileError::Diagnostics(vec![diagnostic(
                        &context,
                        "a fixed box inside a time track would leave its clock".to_string(),
                    )]));
                }
                self.director.scene.add_child(
                    self.root_id
                        .expect("composition root exists before a nested fixed box"),
                    child_id,
                );
            } else {
                self.director.scene.add_child(id, child_id);
            }
        }
        if let Some(fallback) = svg_viewport {
            let next_start = if !root && el.value().attr_text("data-composition-id").is_some() {
                start
            } else {
                composition_start
            };
            self.place_foreign_objects(el, id, &values, fallback, next_start, depth)?;
        }
        self.inherited.pop();
        self.clocked -= usize::from(clocks);
        Ok(id)
    }

    /// The parent's text animations that reach `el`. A local `color`,
    /// `letter-spacing`, `word-spacing`, `font-stretch`, `font-variation-settings`,
    /// `line-height` or `text-shadow` stops its channel. A
    /// local relative font size scales the parent's, and an absolute one stops
    /// it. Replaced elements inherit nothing.
    fn inherit_text(&self, el: ElementRef<'_>, specified_font: Option<&str>, font_size: f32, tag: &str) -> Vec<animation::Animation> {
        let Some((parent_font, list)) = self.inherited.last() else { return Vec::new() };
        if list.is_empty() || matches!(tag, "img" | "svg" | "video" | "audio" | "model") {
            return Vec::new();
        }
        let local = |names: &[&str]| names.iter().any(|n| self.css.has_local_property(el, n));
        let relative = specified_font.is_some_and(|s| {
            let s = s.trim();
            (s.ends_with("em") && !s.ends_with("rem")) || s.ends_with('%') || matches!(s, "larger" | "smaller")
        });
        let font_scale = if !local(&["font-size", "font"]) {
            Some(1.0)
        } else if relative {
            Some(font_size / parent_font.max(f32::EPSILON))
        } else {
            None
        };
        let keep = |name: &str| match name {
            "color" => !local(&["color"]),
            "font-size" => font_scale.is_some(),
            "letter-spacing" => !local(&["letter-spacing"]),
            "word-spacing" => !local(&["word-spacing"]),
            "font-stretch" => !local(&["font-stretch", "font"]),
            name if animation::variation_axis(name).is_some() => !local(&["font-variation-settings"]),
            "line-height" => !local(&["line-height", "font"]),
            _ => !local(&["text-shadow"]),
        };
        list.iter().filter_map(|a| a.inherited(keep, font_scale.unwrap_or(1.0))).collect()
    }

    fn place_foreign_objects(
        &mut self,
        svg: ElementRef<'_>,
        parent: NodeId,
        svg_values: &ComputedStyle,
        fallback: (f32, f32),
        start: f64,
        depth: usize,
    ) -> Result<(), CompileError> {
        let selector = Selector::parse("foreignObject").unwrap();
        if svg.select(&selector).next().is_none() {
            return Ok(());
        }
        let font_size = svg_values
            .get("font-size")
            .and_then(|size| {
                style::px(
                    size,
                    Metrics {
                        width: fallback.0,
                        height: fallback.1,
                        font_size: 16.0,
                    },
                )
                .ok()
            })
            .unwrap_or(16.0);
        let metrics = Metrics {
            width: fallback.0,
            height: fallback.1,
            font_size,
        };
        let viewport = (
            definite_svg_axis(svg_values.get("width"), fallback.0, metrics)?,
            definite_svg_axis(svg_values.get("height"), fallback.1, metrics)?,
        );
        let space = svg_space(svg, viewport)?;
        for foreign in svg.select(&selector) {
            self.place_one_foreign(svg, foreign, parent, svg_values, &space, start, depth)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn place_one_foreign(
        &mut self,
        svg: ElementRef<'_>,
        foreign: ElementRef<'_>,
        parent: NodeId,
        svg_values: &ComputedStyle,
        space: &SvgSpace,
        start: f64,
        depth: usize,
    ) -> Result<(), CompileError> {
        for ancestor in foreign.ancestors().filter_map(ElementRef::wrap) {
            if ancestor.id() == svg.id() {
                break;
            }
            let name = ancestor.value().name();
            if name == "foreignobject" {
                return Err(CompileError::Input(
                    "nested foreignObject requires extraction".into(),
                ));
            }
            if name == "svg" {
                return Err(CompileError::Input(
                    "foreignObject in a nested svg requires extraction".into(),
                ));
            }
            if matches!(
                name,
                "symbol" | "defs" | "marker" | "pattern" | "clippath" | "mask"
            ) {
                return Err(CompileError::Input(
                    "foreignObject in a reusable SVG fragment requires extraction".into(),
                ));
            }
            if specifies_transform(&self.css, ancestor)? {
                return Err(CompileError::Input(
                    "transformed foreignObject requires extraction".into(),
                ));
            }
        }
        if specifies_transform(&self.css, foreign)? {
            return Err(CompileError::Input(
                "transformed foreignObject requires extraction".into(),
            ));
        }
        let cascaded = self.css.cascade(foreign, svg_values)?;
        let geometry = |name: &str, default: &str| -> Result<SvgLen, CompileError> {
            let raw = cascaded
                .get(name)
                .map(String::as_str)
                .or_else(|| foreign.value().attr_text(name))
                .unwrap_or(default);
            parse_svg_len(raw)
        };
        let x = resolve_svg_x(geometry("x", "0")?, space);
        let y = resolve_svg_y(geometry("y", "0")?, space);
        let width = resolve_svg_width(geometry("width", "0")?, space);
        let height = resolve_svg_height(geometry("height", "0")?, space);
        if !width.is_finite() || !height.is_finite() || !x.is_finite() || !y.is_finite() {
            return Err(CompileError::Input(
                "foreignObject length requires a number, px, or a percent".into(),
            ));
        }
        if width <= 0.0 || height <= 0.0 {
            return Ok(());
        }
        if foreign
            .children()
            .any(|node| node.value().as_text().is_some_and(|text| !text.trim().is_empty()))
        {
            return Err(CompileError::Input(format!(
                "{}: foreignObject text requires an HTML element",
                label(foreign)
            )));
        }
        let metrics = Metrics {
            width,
            height,
            font_size: svg_values
                .get("font-size")
                .and_then(|size| {
                    style::px(
                        size,
                        Metrics {
                            width,
                            height,
                            font_size: 16.0,
                        },
                    )
                    .ok()
                })
                .unwrap_or(16.0),
        };
        let (mut layout, mut paint, _grid_scope) = style::translate(
            &ComputedStyle::default(),
            metrics,
            style::ContainingBlock::from_axes(width, height),
            &self.assets,
        )
        .map_err(|errors| {
            CompileError::Diagnostics(
                errors
                    .into_iter()
                    .map(|error| diagnostic(&label(foreign), error))
                    .collect(),
            )
        })?;
        layout.position = taffy::style::Position::Absolute;
        layout.inset = taffy::geometry::Rect {
            left: taffy::prelude::length(x),
            right: taffy::prelude::auto(),
            top: taffy::prelude::length(y),
            bottom: taffy::prelude::auto(),
        };
        layout.size = taffy::geometry::Size {
            width: taffy::prelude::length(width),
            height: taffy::prelude::length(height),
        };
        layout.margin = taffy::geometry::Rect::zero();
        layout.overflow = taffy::geometry::Point {
            x: taffy::style::Overflow::Hidden,
            y: taffy::style::Overflow::Hidden,
        };
        paint.box_node.overflow = "hidden".into();
        let mut element = HtmlElement::new(layout, paint.clone(), None, metrics);
        element.start = start;
        element.end = self.options.duration;
        let box_id = self.director.scene.add_node(Box::new(element));
        self.nodes += 1;
        self.director.scene.get_node_mut(box_id).unwrap().z_index = paint.z_index;
        if let Some(name) = foreign.value().attr_text("id") {
            if self.ids.insert(name.into(), box_id).is_some() {
                return Err(CompileError::Input(format!("duplicate HTML id '{name}'")));
            }
        }
        self.director.scene.add_child(parent, box_id);
        let block = style::ContainingBlock::from_axes(width, height);
        for child in foreign.children().filter_map(ElementRef::wrap) {
            if matches!(child.value().name(), "script" | "style" | "link") {
                continue;
            }
            let child_id = self.node(child, &cascaded, None, start, false, block, depth + 1)?;
            if self.fixed.contains(&child_id) {
                self.director.scene.add_child(
                    self.root_id
                        .expect("composition root exists before a nested fixed box"),
                    child_id,
                );
            } else {
                self.director.scene.add_child(box_id, child_id);
            }
        }
        Ok(())
    }
    /// Compiles the atomic inline boxes of the text block `parent`, whose text
    /// is the leaf `text`, as absolute children placed on the text's lines.
    fn place_atomics<'a>(
        &mut self,
        parent: NodeId,
        text: NodeId,
        atomics: &[(ElementRef<'a>, ComputedStyle)],
        start: f64,
        block: style::ContainingBlock,
        depth: usize,
    ) -> Result<(), CompileError> {
        let flow = InlineFlow::new(self.flows);
        self.flows += 1;
        let mut boxes = Vec::new();
        for (slot, (child, values)) in atomics.iter().enumerate() {
            self.atomic.insert(child.id());
            let id = self.node(*child, values, None, start, false, block, depth + 1);
            self.atomic.remove(&child.id());
            let id = id?;
            let (placeholder, sizing) = self.size_atomic(id, *child)?;
            boxes.push(placeholder);
            let element = html_mut(&mut self.director, id);
            element.inline = Some(InlineLink::Box(flow.clone(), slot, sizing));
            self.director.scene.add_child(parent, id);
        }
        let leaf = html_mut(&mut self.director, text);
        leaf.paint.placeholders = boxes.clone();
        leaf.base_paint.placeholders = boxes;
        leaf.inline = Some(InlineLink::Text(flow));
        if let Some(node) = leaf
            .content
            .as_mut()
            .and_then(|content| content.as_any_mut().downcast_mut::<TextNode>())
        {
            prepare_paragraph(node, &leaf.paint);
        }
        Ok(())
    }

    /// Sizes the compiled atomic box `id` to its max-content text, makes it
    /// absolute, and returns its margin box for the parent's line and how its
    /// size follows live text.
    fn size_atomic(
        &mut self,
        id: NodeId,
        el: ElementRef<'_>,
    ) -> Result<(director_scene::html::InlinePlaceholder, AtomicSizing), CompileError> {
        let children = self.director.scene.get_node(id).map(|n| n.children.clone()).unwrap_or_default();
        let mut text = html_mut(&mut self.director, id).inline_text_metrics();
        for (i, child) in children.iter().enumerate() {
            let leaf = self.leaves.contains(child);
            let child = html_mut(&mut self.director, *child);
            if i == 0 && text.is_none() && (leaf || matches!(child.inline, Some(InlineLink::Text(_)))) {
                text = child.inline_text_metrics();
            } else if child.base_style.position != taffy::style::Position::Absolute {
                return Err(CompileError::Input(format!(
                    "{}: inline-block with block children requires extraction",
                    label(el)
                )));
            }
        }
        let element = html_mut(&mut self.director, id);
        let mut style = element.base_style.clone();
        let edge = |padding: taffy::style::LengthPercentage, border: taffy::style::LengthPercentage| {
            style::resolve(padding, 0.0) + style::resolve(border, 0.0)
        };
        let left = edge(style.padding.left, style.border.left);
        let right = edge(style.padding.right, style.border.right);
        let top = edge(style.padding.top, style.border.top);
        let bottom = edge(style.padding.bottom, style.border.bottom);
        let (width, height, baseline) = match text {
            Some((w, h, b)) => (w, h, Some(b)),
            None => (0.0, 0.0, None),
        };
        let border_box = |size: taffy::style::Dimension, content: f32, edges: f32| match size.into_option() {
            Some(length) if style.box_sizing == taffy::style::BoxSizing::BorderBox => length,
            Some(length) => length + edges,
            None => content + edges,
        };
        let sizing = AtomicSizing {
            auto_width: style.size.width.into_option().is_none(),
            auto_height: style.size.height.into_option().is_none(),
            content_box: style.box_sizing != taffy::style::BoxSizing::BorderBox,
        };
        let width = border_box(style.size.width, width, left + right);
        let height = border_box(style.size.height, height, top + bottom);
        // A box without text sits on its bottom margin edge.
        let baseline = baseline.map_or(height, |b| top + b);
        let margin = style.margin.map(crate::element::margin_px);
        style.position = taffy::style::Position::Absolute;
        style.box_sizing = taffy::style::BoxSizing::BorderBox;
        style.size = taffy::geometry::Size {
            width: taffy::prelude::length(width),
            height: taffy::prelude::length(height),
        };
        style.inset = taffy::geometry::Rect::auto();
        element.set_layout_style(style.clone());
        element.paint.box_node.style = style.clone();
        element.base_paint.box_node.style = style;
        let placeholder = director_scene::html::InlinePlaceholder {
            width: width + margin.left + margin.right,
            height: height + margin.top + margin.bottom,
            baseline: margin.top + baseline,
        };
        Ok((placeholder, sizing))
    }

    #[allow(clippy::too_many_arguments)]
    fn rich_text<'a>(
        &self,
        el: ElementRef<'a>,
        values: &ComputedStyle,
        paint: &PaintStyle,
        block: style::ContainingBlock,
        spans: &mut Vec<TextSpan>,
        atomics: &mut Vec<(ElementRef<'a>, ComputedStyle)>,
        cascaded: &mut HashMap<ego_tree::NodeId, ComputedStyle>,
        top: bool,
        visible: bool,
    ) -> Result<(), CompileError> {
        for node in el.children() {
            if let Some(text) = node.value().as_text() {
                // A hidden span inside a visible block keeps its advance; the block's own visibility hides the rest.
                spans.push(TextSpan { hidden: visible && !paint.visible, ..span(text.to_string(), paint) });
            } else if let Some(child) = ElementRef::wrap(node) {
                if child.value().name() == "br" {
                    spans.push(span(HARD_BREAK.to_string(), paint));
                    continue;
                }
                if matches!(child.value().name(), "script" | "style" | "link") {
                    continue;
                }
                let mut child_values = match cascaded.remove(&child.id()) {
                    Some(child_values) => child_values,
                    None => self.css.cascade(child, values)?,
                };
                match self.level(child, &child_values) {
                    Level::Skip => continue,
                    Level::Atomic => {
                        spans.push(span(OBJECT.into(), paint));
                        atomics.push((child, values.clone()));
                        continue;
                    }
                    Level::Block => {
                        return Err(CompileError::Input(format!(
                            "{}: mixed block and inline content requires extraction",
                            label(child)
                        )))
                    }
                    // The text block compiles its own positioned children beside its text.
                    Level::Positioned if top => {
                        cascaded.insert(child.id(), child_values);
                        continue;
                    }
                    Level::Positioned => {
                        return Err(CompileError::Input(format!(
                            "{}: a positioned box inside an inline box requires extraction",
                            label(child)
                        )))
                    }
                    Level::Inline => {}
                }
                child_values.remove("display");
                if child_values.iter().any(|(k, v)| {
                    !crate::css::inherited(k) && !k.starts_with("font-") && !inert_on_inline(k, v)
                }) {
                    return Err(CompileError::Input(format!(
                        "{}: styled inline boxes require extraction",
                        label(child)
                    )));
                }
                let (_, child_paint, _grid_scope) = style::translate(
                    &child_values,
                    Metrics {
                        width: self.options.width as f32,
                        height: self.options.height as f32,
                        font_size: paint.font_size,
                    },
                    block,
                    &self.assets,
                )
                .map_err(|e| CompileError::Input(e.join("; ")))?;
                let before = spans.len();
                self.rich_text(child, &child_values, &child_paint, block, spans, atomics, &mut HashMap::new(), false, visible)?;
                if self.css.has_local_property(child, "color") {
                    for span in &mut spans[before..] {
                        if span.color.is_none() {
                            span.color = Some(child_paint.color);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// Stands in for `<br>` until `normalize`. Parsed body text never holds U+0000, so
/// the break survives the space collapsing that turns source newlines into spaces.
const HARD_BREAK: char = '\0';

/// A declaration an inline box ignores, such as the `* { margin: 0; padding: 0;
/// box-sizing: border-box }` reset that every span matches.
fn inert_on_inline(name: &str, value: &str) -> bool {
    name == "box-sizing"
        || (name == "position" && matches!(value.trim(), "static" | "relative"))
        || ((name.starts_with("margin-") || name.starts_with("padding-"))
            && matches!(value.trim(), "0" | "0px"))
}

fn normalize(spans: &mut Vec<TextSpan>, preserve: bool) {
    if preserve {
        for span in spans.iter_mut() {
            span.text = span.text.replace(HARD_BREAK, "\n");
        }
        return;
    }
    let mut last_space = true;
    for span in spans.iter_mut() {
        let mut text = String::new();
        for c in span.text.chars() {
            if c == HARD_BREAK {
                if text.ends_with(' ') {
                    text.pop();
                }
                text.push('\n');
                last_space = true;
            } else if c.is_whitespace() {
                if !last_space {
                    text.push(' ');
                }
                last_space = true;
            } else {
                text.push(c);
                last_space = false;
            }
        }
        span.text = text;
    }
    spans.retain(|s| !s.text.is_empty());
    // A collapsed space before a hard break would hang past the line and shift centred text.
    for i in 1..spans.len() {
        if spans[i].text.starts_with('\n') && spans[i - 1].text.ends_with(' ') {
            spans[i - 1].text.pop();
        }
    }
    spans.retain(|s| !s.text.is_empty());
    if let Some(last) = spans.last_mut() {
        last.text = last.text.trim_end().into();
    }
    spans.retain(|s| !s.text.is_empty());
}
pub fn fit(values: &Values) -> ObjectFit {
    match values.get("object-fit").map(String::as_str) {
        Some("contain") => ObjectFit::Contain,
        Some("cover") => ObjectFit::Cover,
        Some("none") => ObjectFit::None,
        Some("scale-down") => ObjectFit::ScaleDown,
        _ => ObjectFit::Fill,
    }
}

/// File duration for `<video loop>`. Metadata only; the existing decoder seeks.
pub(crate) fn media_duration(path: &Path) -> Result<f64, CompileError> {
    let probe = |entries: &[&str]| -> Result<Option<f64>, CompileError> {
        let output = director_core::video_wrapper::ffprobe()
            .arg("-v")
            .arg("error")
            .args(entries)
            .arg("-of")
            .arg("default=noprint_wrappers=1:nokey=1")
            .arg(path)
            .output()
            .map_err(|error| CompileError::Input(format!("video duration: {error}")))?;
        if !output.status.success() {
            return Ok(None);
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| {
                let duration = line.trim().parse::<f64>().ok()?;
                (duration.is_finite() && duration > 0.0).then_some(duration)
            }))
    };
    if let Some(duration) = probe(&["-select_streams", "v:0", "-show_entries", "stream=duration"])?
    {
        return Ok(duration);
    }
    if let Some(duration) = probe(&["-show_entries", "format=duration"])? {
        return Ok(duration);
    }
    Err(CompileError::Input(
        "video loop requires a positive media duration".into(),
    ))
}

/// `time` is already `media_start + max(elapsed, 0) * playback_rate`.
/// Loop wraps that position into `[0, duration)`.
pub(crate) fn looped_media_time(time: f64, duration: f64) -> f64 {
    let time = if time.is_finite() { time.max(0.0) } else { 0.0 };
    if !duration.is_finite() || duration <= 0.0 {
        return time;
    }
    let wrapped = time % duration;
    if wrapped < 0.0 {
        wrapped + duration
    } else {
        wrapped
    }
}

/// One owned decoder. `HtmlElement` applies media start and rate before `update`.
#[derive(Debug, Clone)]
struct LoopedVideo {
    node: VideoNode,
    duration: f64,
}

impl Element for LoopedVideo {
    fn kind(&self) -> &'static str {
        "html_video"
    }
    fn export_body(
        &self,
        cx: &mut director_core::scene_export::ExportContext,
    ) -> Result<director_core::scene_export::ElementExport, director_core::scene_export::ExportError> {
        Ok(director_core::scene_export::ElementExport {
            opacity: director_core::scene_export::channel(&self.node.opacity),
            body: director_scene::NodeBody::HtmlVideo(director_scene::HtmlVideoBody {
                video: director_core::scene_export::nodes::video_media(&self.node, cx)?,
                looped: true,
                duration: self.duration,
            }),
        })
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn layout_style(&self) -> taffy::Style {
        self.node.layout_style()
    }
    fn set_layout_style(&mut self, style: taffy::Style) {
        self.node.set_layout_style(style);
    }
    fn set_opacity(&mut self, opacity: f32) {
        self.node.set_opacity(opacity);
    }
    fn update(&mut self, time: f64) -> bool {
        self.node.update(looped_media_time(time, self.duration))
    }
    fn update_motion(&mut self, time: f64) -> bool {
        self.node.update_motion(looped_media_time(time, self.duration))
    }
    fn render(
        &self,
        canvas: &skia_safe::Canvas,
        rect: skia_safe::Rect,
        opacity: f32,
        draw_children: &mut dyn FnMut(&skia_safe::Canvas),
    ) -> Result<(), RenderError> {
        self.node.render(canvas, rect, opacity, draw_children)
    }
    fn animate_property(
        &mut self,
        property: &str,
        start: f32,
        target: f32,
        duration: f64,
        easing: &str,
    ) -> Result<(), String> {
        self.node
            .animate_property(property, start, target, duration, easing)
    }
}

pub fn compile_file(
    path: &Path,
    mut options: CompileOptions,
) -> Result<CompiledHtml, CompileError> {
    options.base_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let source = std::fs::read_to_string(path)
        .map_err(|e| CompileError::Input(format!("HTML {}: {e}", path.display())))?;
    compile(&source, options)
}

/// Solves the cameras of the footage at `plate` for a `<model track>`:
/// `track-fov` fixes the lens in degrees, and without it the solve refines
/// its lens from 40 degrees. `track-distortion` fixes the radial distortion
/// as "k1 k2", and without it the solve finds it. `track-height` is frame
/// 0's camera height above the ground in metres, which sets the scale.
/// `track-mask` names a matte of the footage, white over whatever moves on
/// its own, and the solve tracks nothing there. `track-pitch` is frame 0's
/// tilt below the horizon in degrees, which footage from a camera that only
/// turns needs. `track-zoom` gives the camera a lens that zooms, solved
/// frame by frame from `track-fov` as frame 0's. `track-shutter` is the
/// footage's shutter angle in degrees, along which the model blurs. With
/// `light` it also
/// stitches the plate through the solved cameras into a light probe and
/// returns it as a Radiance `.hdr` file, and with `sun` it reads the sun
/// from the shadows on the plate's ground, lit by that probe's sky.
fn solve_track(
    assets: &AssetStore,
    (plate, mask, tilt): (&str, Option<&str>, Option<&str>),
    numeric: &dyn Fn(&str, f64) -> Result<f64, CompileError>,
    (fixed_lens, distortion, zoom): (bool, Option<&str>, bool),
    (light, sun): (bool, bool),
    context: &str,
) -> Result<SolvedTrack, CompileError> {
    let distortion = match distortion {
        None => None,
        Some(v) => match v.split_whitespace().map(str::parse::<f64>).collect::<Result<Vec<_>, _>>().as_deref() {
            Ok(&[k1, k2]) if k1.is_finite() && k2.is_finite() => Some([k1, k2]),
            _ => return Err(CompileError::Input(format!("{context}: track-distortion must be two numbers, k1 and k2"))),
        },
    };
    let fov = numeric("track-fov", 40.0)?;
    if !(1.0..=179.0).contains(&fov) {
        return Err(CompileError::Input(format!("{context}: track-fov must be from 1 to 179 degrees")));
    }
    let height = numeric("track-height", 1.6)?;
    let shutter = numeric("track-shutter", 0.0)?;
    if !(0.0..=360.0).contains(&shutter) {
        return Err(CompileError::Input(format!("{context}: track-shutter must be from 0 to 360 degrees")));
    }
    let pitch = match tilt {
        None => None,
        Some(_) => {
            let pitch = numeric("track-pitch", 0.0)?;
            if !(-89.0..=89.0).contains(&pitch) {
                return Err(CompileError::Input(format!("{context}: track-pitch must be from -89 to 89 degrees")));
            }
            Some(pitch.to_radians())
        }
    };
    if height.is_nan() || height <= 0.0 {
        return Err(CompileError::Input(format!("{context}: track-height must be above 0 metres")));
    }
    let path = assets.media_path(plate)?;
    let input = |e: String| CompileError::Input(format!("{context}: track '{plate}': {e}"));
    let mut footage = director_track::read_footage(&path, 960).map_err(input)?;
    if let Some(mask) = mask {
        let path = assets.media_path(mask)?;
        footage.read_matte(&path).map_err(|e| CompileError::Input(format!("{context}: track-mask '{mask}': {e}")))?;
    }
    let options = director_track::SolveOptions { yfov: fov.to_radians(), refine_focal: !fixed_lens, distortion, camera_height: height, pitch, zoom };
    let solved = director_track::solve(&footage, &options).map_err(input)?;
    let probe = match light || sun {
        true => Some(director_track::light_probe(&path, &solved, PROBE_WIDTH).map_err(input)?),
        false => None,
    };
    let sun = match (sun, &probe) {
        (true, Some(probe)) => Some(director_track::estimate_sun(&path, &solved, probe).map_err(input)?),
        _ => None,
    };
    let probe = probe.filter(|_| light).map(|p| p.hdr());
    let track = director_scene::ModelTrack {
        fps: solved.fps,
        fov: solved.yfov.to_degrees() as f32,
        distortion: solved.distortion.map(|k| k as f32),
        shutter: shutter as f32,
        frames: solved
            .frames
            .iter()
            .map(|f| director_scene::ModelTrackFrame { position: f.position, rotation: f.rotation, fov: f.yfov.map(|v| v.to_degrees() as f32) })
            .collect(),
    };
    Ok((track, probe, sun))
}

/// A plate's solved cameras, its light probe as a Radiance file and its sun.
type SolvedTrack = (director_scene::ModelTrack, Option<Vec<u8>>, Option<director_track::Sun>);

/// The width in texels of the light probe `track-light` stitches.
const PROBE_WIDTH: usize = 512;

/// The sun `track-sun` read from the plate as a shadow-casting sun light.
/// The plate's linear light renders back at its own value with `unity` lux
/// per unit of irradiance, so a white card facing the light reads on the
/// model as it would on the plate.
fn sun_light(direction: [f64; 3], irradiance: [f64; 3], unity: f64) -> director_scene::ModelLight {
    let peak = irradiance.iter().copied().fold(1e-9, f64::max);
    let encode = |v: f64| if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    director_scene::ModelLight {
        kind: director_scene::ModelLightKind::Sun,
        color: irradiance.map(|c| encode(c / peak) as f32),
        intensity: (peak * unity) as f32,
        position: Some(direction.map(|v| v as f32)),
        target: Some([0.0; 3]),
        radius: 0.0,
        angle: 0.0,
        cone: 0.0,
        width: 0.0,
        height: 0.0,
        range: 0.0,
        shadows: true,
    }
}

/// The `<splat>` children of a `<model>`: Gaussian splat `.ply` files read
/// through the compile asset store and placed in the scene's metres.
fn model_splats(model: ElementRef<'_>, assets: &AssetStore, context: &str) -> Result<Vec<director_3d::Splat>, CompileError> {
    let fail = |e: String| CompileError::Input(format!("{context}: {e}"));
    let mut splats = Vec::new();
    for el in model.children().filter_map(ElementRef::wrap).filter(|el| el.value().name() == "splat") {
        let attr = |name: &str| el.value().attr_text(name).map(str::trim);
        let src = attr("src").ok_or_else(|| fail("splat without src".into()))?;
        let path = (!src.starts_with("data:")).then(|| src.to_string());
        let cloud = director_3d::Asset::splat(assets.read(src)?, path).map_err(fail)?;
        let triple = |name: &str, default: [f32; 3], unit: &str| -> Result<[f32; 3], CompileError> {
            let Some(v) = attr(name) else { return Ok(default) };
            let parts: Vec<f32> = v.split_whitespace().filter_map(|n| n.parse().ok()).collect();
            match parts[..] {
                [x, y, z] if parts.iter().all(|n| n.is_finite()) && v.split_whitespace().count() == 3 => Ok([x, y, z]),
                _ => Err(fail(format!("splat {name} '{v}' must be three numbers in {unit}: x y z"))),
            }
        };
        let number = |name: &str, default: f32, max: f32| -> Result<f32, CompileError> {
            let Some(v) = attr(name) else { return Ok(default) };
            match v.parse::<f32>() {
                Ok(n) if n >= 0.0 && n <= max && n.is_finite() => Ok(n),
                _ if max.is_finite() => Err(fail(format!("splat {name} '{v}' must be a number from 0 to {max}"))),
                _ => Err(fail(format!("splat {name} '{v}' must be a number from 0"))),
            }
        };
        splats.push(director_3d::Splat {
            cloud,
            position: triple("position", [0.0; 3], "metres")?,
            rotation: triple("rotation", [0.0; 3], "degrees")?,
            scale: number("scale", 1.0, f32::INFINITY)?,
            opacity: number("opacity", 1.0, 1.0)?,
        });
    }
    Ok(splats)
}

/// The `<text3d>` children of a `<model>`: lines of extruded type in the
/// scene's metres, from a font file or a family the page or Director knows.
fn model_texts(
    model: ElementRef<'_>,
    assets: &AssetStore,
    fonts: &std::sync::Mutex<skia_safe::textlayout::FontCollection>,
) -> Result<Vec<director_3d::Text3d>, String> {
    let mut texts = Vec::new();
    for el in model.children().filter_map(ElementRef::wrap).filter(|el| el.value().name() == "text3d") {
        let attr = |name: &str| el.value().attr_text(name).map(str::trim);
        let text = el.text().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            return Err("text3d has no text".into());
        }
        let weight = match attr("font-weight").unwrap_or("normal") {
            "normal" => 400.0,
            "bold" => 700.0,
            v => match v.parse::<f32>() {
                Ok(n) if (1.0..=1000.0).contains(&n) => n,
                _ => return Err(format!("text3d font-weight '{v}' must be normal, bold or a number from 1 to 1000")),
            },
        };
        let stretch = match attr("font-stretch") {
            Some(v) => crate::style::stretch_percent(v).map_err(|e| format!("text3d {e}"))?,
            None => 100.0,
        };
        let italic = match attr("font-style").unwrap_or("normal") {
            "normal" => false,
            "italic" | "oblique" => true,
            v => return Err(format!("text3d font-style '{v}' must be normal, italic or oblique")),
        };
        let font = match attr("font") {
            Some(src) => {
                let path = (!src.starts_with("data:")).then(|| src.to_string());
                let bytes = assets.read(src).map_err(|e| format!("text3d font: {e}"))?;
                director_3d::Asset::font(bytes, path)?
            }
            None => {
                let families = attr("font-family").unwrap_or("Inter");
                let style = span_font_style(weight.round() as u16, italic);
                let mut fonts = fonts.lock().unwrap();
                let face = families
                    .split(',')
                    .map(|f| f.trim().trim_matches(['\'', '"']))
                    .filter(|f| !f.is_empty())
                    .find_map(|family| {
                        let face = fonts.find_typefaces(&[family], style).into_iter().next()?;
                        face.family_name().eq_ignore_ascii_case(family).then_some(face)
                    })
                    .ok_or_else(|| format!("text3d font-family '{families}' names no font Director has"))?;
                let (bytes, index) = face.to_font_bytes().ok_or("text3d font has no readable file")?;
                if index != 0 {
                    return Err(format!(
                        "text3d font-family '{}' is face {index} of a font collection; give its file through font",
                        face.family_name()
                    ));
                }
                director_3d::Asset::font(bytes, None)?
            }
        };
        let number = |name: &str, default: f32, min: f32, max: f32| -> Result<f32, String> {
            let Some(v) = attr(name) else { return Ok(default) };
            match v.parse::<f32>() {
                Ok(n) if n >= min && n <= max && n.is_finite() => Ok(n),
                _ if max.is_finite() => Err(format!("text3d {name} '{v}' must be a number from {min} to {max}")),
                _ => Err(format!("text3d {name} '{v}' must be a number from {min}")),
            }
        };
        let triple = |name: &str, unit: &str| -> Result<[f32; 3], String> {
            let Some(v) = attr(name) else { return Ok([0.0; 3]) };
            let parts: Vec<f32> = v.split_whitespace().filter_map(|n| n.parse().ok()).collect();
            match parts[..] {
                [x, y, z] if parts.iter().all(|n| n.is_finite()) && v.split_whitespace().count() == 3 => Ok([x, y, z]),
                _ => Err(format!("text3d {name} '{v}' must be three numbers in {unit}: x y z")),
            }
        };
        let rgb = |name: &str| -> Result<Option<[f32; 3]>, String> {
            let Some(c) = attr(name) else { return Ok(None) };
            let color = crate::style::color(c, director_core::types::Color::WHITE).map_err(|e| format!("text3d {e}"))?;
            Ok(Some([color.r, color.g, color.b]))
        };
        let size = number("size", 1.0, 0.0, f32::INFINITY)?;
        if size == 0.0 {
            return Err("text3d size must be above 0".into());
        }
        let depth = number("depth", 0.2 * size, 0.0, f32::INFINITY)?;
        let most = (depth / 2.0).min(0.05 * size);
        let bevel = number("bevel", (0.02 * size).min(depth / 2.0), 0.0, f32::INFINITY)?;
        if bevel > most {
            return Err(format!("text3d bevel {bevel} must be at most {most}, the smaller of half the depth and a twentieth of the size"));
        }
        texts.push(director_3d::Text3d {
            font,
            text,
            weight,
            stretch,
            size,
            depth,
            bevel,
            position: triple("position", "metres")?,
            rotation: triple("rotation", "degrees")?,
            color: rgb("color")?.unwrap_or([1.0; 3]),
            side_color: rgb("side-color")?,
            metallic: number("metallic", 0.0, 0.0, 1.0)?,
            roughness: number("roughness", 0.4, 0.0, 1.0)?,
        });
    }
    Ok(texts)
}

/// A `<model>`'s lens effects from its `vignette`, `chromatic-aberration`
/// and `lens-distortion` attributes, `None` without any.
fn model_optics(el: ElementRef<'_>) -> Result<Option<director_scene::ModelOptics>, String> {
    let attr = |name: &str| el.value().attr_text(name);
    let number = |name: &str, range: std::ops::RangeInclusive<f32>| -> Result<f32, String> {
        let Some(v) = attr(name).map(str::trim) else { return Ok(0.0) };
        let parsed = match v.strip_suffix('%') {
            Some(percent) => percent.trim().parse::<f32>().map(|p| p / 100.0),
            None => v.parse::<f32>(),
        };
        match parsed {
            Ok(n) if range.contains(&n) => Ok(n),
            _ => Err(format!("{name} '{v}' must be a number from {} to {}", range.start(), range.end())),
        }
    };
    let optics = director_scene::ModelOptics {
        vignette: number("vignette", 0.0..=1.0)?,
        aberration: number("chromatic-aberration", 0.0..=1.0)?,
        distortion: number("lens-distortion", -1.0..=1.0)?,
    };
    Ok((optics != director_scene::ModelOptics::default()).then_some(optics))
}

/// The `<fog>` child of a `<model>`.
fn model_fog(model: ElementRef<'_>) -> Result<Option<director_scene::ModelFog>, String> {
    let mut fogs = model.children().filter_map(ElementRef::wrap).filter(|el| el.value().name() == "fog");
    let Some(el) = fogs.next() else { return Ok(None) };
    if fogs.next().is_some() {
        return Err("a model takes one <fog>".into());
    }
    let attr = |name: &str| el.value().attr_text(name).map(str::trim);
    let number = |name: &str| -> Result<f32, String> {
        match attr(name) {
            None => Ok(0.0),
            Some(v) => match v.trim_end_matches('m').parse::<f32>() {
                Ok(n) if n >= 0.0 && n.is_finite() => Ok(n),
                _ => Err(format!("fog {name} '{v}' must be a number from 0")),
            },
        }
    };
    let haze = director_core::types::Color::new(0.78, 0.82, 0.86, 1.0);
    let color = match attr("color") {
        Some(c) => crate::style::color(c, haze).map_err(|e| format!("fog {e}"))?,
        None => haze,
    };
    let (visibility, density) = (number("visibility")?, number("density")?);
    if visibility == 0.0 && density == 0.0 {
        return Err("fog needs a visibility or a density above 0".into());
    }
    Ok(Some(director_scene::ModelFog { color: [color.r, color.g, color.b], visibility, density }))
}

/// The `<sky>` child of a `<model>`: the sky's options and the sun's lux
/// above the atmosphere.
fn model_sky(model: ElementRef<'_>) -> Result<Option<(director_3d::SkyOptions, f64)>, String> {
    let mut skies = model.children().filter_map(ElementRef::wrap).filter(|el| el.value().name() == "sky");
    let Some(el) = skies.next() else { return Ok(None) };
    if skies.next().is_some() {
        return Err("a model takes one <sky>".into());
    }
    let attr = |name: &str| el.value().attr_text(name).map(str::trim);
    let number = |name: &str, default: f64, range: std::ops::RangeInclusive<f64>, unit: &str| -> Result<f64, String> {
        let Some(v) = attr(name) else { return Ok(default) };
        match v.trim_end_matches(unit).parse::<f64>() {
            Ok(n) if range.contains(&n) => Ok(n),
            _ => Err(format!("sky {name} '{v}' must be a number from {} to {}", range.start(), range.end())),
        }
    };
    let elevation = number("elevation", 30.0, -10.0..=90.0, "deg")?;
    let azimuth = number("azimuth", 45.0, -360.0..=360.0, "deg")?;
    let haze = number("haze", 1.0, 0.0..=50.0, "")?;
    let lux = number("intensity", 130_000.0, 0.0..=1e7, "")?;
    let grey = director_core::types::Color::new(0.58, 0.58, 0.58, 1.0);
    let ground = match attr("ground") {
        Some(c) => crate::style::color(c, grey).map_err(|e| format!("sky ground {e}"))?,
        None => grey,
    };
    let linear = |c: f32| {
        let c = c as f64;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let ground = [linear(ground.r), linear(ground.g), linear(ground.b)];
    Ok(Some((director_3d::SkyOptions { elevation, azimuth, haze, ground }, lux)))
}

/// The width in texels of the probe a `<sky>` renders.
const SKY_WIDTH: usize = 1024;

/// A `<sky>` as a Radiance probe in radiance per lux of sunlight above the
/// atmosphere, and its sun as a shadow-casting light while it is up.
fn sky_probe(options: &director_3d::SkyOptions, lux: f64) -> (Vec<u8>, Option<director_scene::ModelLight>) {
    let sky = director_3d::sky(options, SKY_WIDTH);
    let texels = sky.texels.len();
    let probe = director_track::Probe { width: sky.width, height: sky.height, texels: sky.texels, coverage: vec![1.0; texels] };
    let lit = sky.irradiance.iter().any(|&c| c > 1e-6);
    (probe.hdr(), lit.then(|| sun_light(sky.sun, sky.irradiance, lux)))
}

/// The `<light>` children of a `<model>`.
fn model_lights(model: ElementRef<'_>) -> Result<Vec<director_scene::ModelLight>, String> {
    use director_scene::{ModelLight, ModelLightKind};
    let mut lights = Vec::new();
    for el in model.children().filter_map(ElementRef::wrap).filter(|el| el.value().name() == "light") {
        let attr = |name: &str| el.value().attr_text(name).map(str::trim);
        let kind = match attr("type").unwrap_or("point") {
            "sun" => ModelLightKind::Sun,
            "point" => ModelLightKind::Point,
            "spot" => ModelLightKind::Spot,
            "rect" => ModelLightKind::Rect,
            other => return Err(format!("light type '{other}' must be sun, point, spot or rect")),
        };
        let number = |name: &str, default: f32| -> Result<f32, String> {
            match attr(name) {
                None => Ok(default),
                Some(v) => match v.parse::<f32>() {
                    Ok(n) if n >= 0.0 && n.is_finite() => Ok(n),
                    _ => Err(format!("light {name} '{v}' must be a number from 0")),
                },
            }
        };
        let point = |name: &str| -> Result<Option<[f32; 3]>, String> {
            let Some(v) = attr(name) else { return Ok(None) };
            let parts: Vec<f32> = v.split_whitespace().filter_map(|n| n.parse().ok()).collect();
            match parts[..] {
                [x, y, z] if parts.iter().all(|n| n.is_finite()) && v.split_whitespace().count() == 3 => Ok(Some([x, y, z])),
                _ => Err(format!("light {name} '{v}' must be three numbers in metres: x y z")),
            }
        };
        let color = match attr("color") {
            Some(c) => crate::style::color(c, director_core::types::Color::WHITE).map_err(|e| format!("light {e}"))?,
            None => director_core::types::Color::WHITE,
        };
        let intensity = number("intensity", if kind == ModelLightKind::Sun { 2000.0 } else { 1_000_000.0 })?;
        lights.push(ModelLight {
            kind,
            color: [color.r, color.g, color.b],
            intensity,
            position: point("position")?,
            target: point("target")?,
            radius: number("radius", 0.0)?,
            angle: number("angle", 0.0)?,
            cone: number("cone", 0.0)?,
            width: number("width", 0.0)?,
            height: number("height", 0.0)?,
            range: number("range", 0.0)?,
            shadows: attr("shadows") != Some("false"),
        });
    }
    Ok(lights)
}
