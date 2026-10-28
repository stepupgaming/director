use crate::{
    animation,
    assets::AssetStore,
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
use std::{
    any::Any,
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
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
            Node::Text(text) => out.push_str(&escape_xml(text, false)),
            Node::Element(_) => {
                let child = ElementRef::wrap(child).unwrap();
                let child_path = crate::smil::child_path(parent_path, index);
                index += 1;
                let values = css.cascade(child, parent)?;
                let mut extra = String::new();
                for name in SVG_EXTERNAL_PRESENTATION {
                    // lightningcss stores the mask shorthand as mask-image.
                    // usvg reads the presentation name mask.
                    let lookup = if *name == "mask" && !css.has_stylesheet_property(child, name) {
                        "mask-image"
                    } else {
                        *name
                    };
                    if css.has_stylesheet_property(child, lookup) {
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
                    if name == "style" && !extra.is_empty() {
                        let mut style = value.to_string();
                        if !style.is_empty() && !style.trim_end().ends_with(';') {
                            style.push(';');
                        }
                        style.push_str(&extra);
                        out.push_str(&format!(" {name}=\"{}\"", escape_xml(&style, true)));
                        wrote_style = true;
                    } else {
                        out.push_str(&format!(" {name}=\"{}\"", escape_xml(value, true)));
                    }
                }
                if !wrote_style && !extra.is_empty() {
                    out.push_str(&format!(" style=\"{}\"", escape_xml(&extra, true)));
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
    let (min_x, min_y, width, height) = if let Some(raw) = svg.value().attr("viewBox") {
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
    if el.value().attr("transform").is_some_and(|value| {
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
            .find(|r| r.value().attr("data-composition-id") == Some(id.as_str()))
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
    if let Some(v) = root.value().attr("data-motion-blur") {
        blur.0 = v
            .parse()
            .ok()
            .filter(|n| (1..=64).contains(n))
            .ok_or_else(|| CompileError::Input(format!("invalid data-motion-blur: {v} (1..=64 samples)")))?;
    }
    if let Some(v) = root.value().attr("data-shutter-angle") {
        blur.1 = v
            .parse()
            .ok()
            .filter(|a: &f32| (0.0..=360.0).contains(a))
            .ok_or_else(|| CompileError::Input(format!("invalid data-shutter-angle: {v} (0..=360 degrees)")))?;
    }
    Ok(blur)
}

/// `options` with the root's size, duration and frame rate applied.
fn resolve_options(dom: &Html, mut options: CompileOptions) -> Result<CompileOptions, CompileError> {
    let root = composition_root(dom, &options)?;
    for (attr, target) in [
        ("data-width", &mut options.width),
        ("data-height", &mut options.height),
    ] {
        if let Some(v) = root.value().attr(attr) {
            *target = v
                .parse()
                .map_err(|_| CompileError::Input(format!("invalid {attr}: {v}")))?;
        }
    }
    for (attr, target) in [
        ("data-duration", &mut options.duration),
        ("data-fps", &mut options.fps),
    ] {
        if let Some(v) = root.value().attr(attr) {
            *target = v
                .parse()
                .map_err(|_| CompileError::Input(format!("invalid {attr}: {v}")))?;
        }
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
    let options = resolve_options(&dom, options)?;
    let blur = motion_blur(composition_root(&dom, &options)?)?;
    let motion = crate::gsap::run(&mut dom, &options, blur, &assets)
        .map_err(|e| CompileError::Diagnostics(vec![diagnostic("script", e)]))?;
    let (mut builder, root_id) = build(&dom, options, assets, motion, false)?;
    if !builder.errors.is_empty() {
        return Err(CompileError::Diagnostics(builder.errors));
    }
    if let Some(target) = builder.timeline.keys().next() {
        return Err(CompileError::Input(format!(
            "timeline target '{target}' matches no animatable element"
        )));
    }
    if let Some(el) = builder.motion.series.keys().find_map(|id| dom.tree.get(*id).and_then(ElementRef::wrap)) {
        let name = match el.value().attr("id") {
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
    })
}

/// Lay out `dom` at time 0 for the computed style and geometry page scripts
/// read while they set up.
pub(crate) fn probe(
    dom: &Html,
    options: &CompileOptions,
    assets: &Arc<AssetStore>,
) -> Result<crate::gsap::Probe, CompileError> {
    let (mut builder, root_id) = build(dom, options.clone(), assets.clone(), Default::default(), true)?;
    if !builder.errors.is_empty() {
        return Err(CompileError::Diagnostics(builder.errors));
    }
    let entries = builder.probe.take().unwrap_or_default();
    let director = &mut builder.director;
    director.push_scene_root(root_id, builder.options.duration);
    director.update(0.0, None);
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
) -> Result<(Builder, NodeId), CompileError> {
    let root = composition_root(dom, &options)?;
    let mut css = Css::default();
    // Explicit video-authoring UA defaults. No host/browser-dependent styles.
    css.append("html, body { display:block; font-size:16px; color:black; font-family:Roboto; } body { margin:8px; } div,section,article,main,header,footer,p,h1,h2,h3,ul,li { display:block; } h1 {font-size:2em;font-weight:bold;margin-top:.67em;margin-bottom:.67em} h2 {font-size:1.5em;font-weight:bold;margin-top:.83em;margin-bottom:.83em} p {margin-top:1em;margin-bottom:1em} b,strong {font-weight:bold} em,i {font-style:italic} img,svg,video {display:block}", options.width, options.height)?;
    for el in dom.select(&Selector::parse("style, link, script").unwrap()) {
        match el.value().name() {
            "style" if !el.ancestors().filter_map(ElementRef::wrap).any(|p|p.value().name()=="svg")=>css.append(&el.text().collect::<String>(), options.width, options.height)?,
            "link" if el.value().attr("rel")==Some("stylesheet")=>{
                let href=el.value().attr("href").ok_or_else(||CompileError::Input("stylesheet without href".into()))?;
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
    let timeline = if probing { BTreeMap::new() } else { crate::timeline::collect(dom)? };
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
        flows: 0,
        inherited: Vec::new(),
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
    /// Inline formatting contexts with atomic boxes so far.
    flows: u32,
    /// Per open ancestor: its font size and the text animations its
    /// descendants inherit.
    inherited: Vec<(f32, Vec<animation::Animation>)>,
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
        .attr("id")
        .map(|id| format!("#{id}"))
        .unwrap_or_else(|| el.value().name().into())
}
fn inline(tag: &str) -> bool {
    matches!(tag, "span" | "b" | "strong" | "em" | "i" | "br" | "small")
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
    paint: &PaintStyle,
    width: f32,
) -> skia_safe::textlayout::Paragraph {
    let wrap = wrap_width(node, paint, width);
    build_paragraph_colored(node, paint, width, wrap, false, TextPass::Combined)
}

/// The width `append_wrapped` breaks lines at. It adds up unkerned token
/// advances, which run wider than the shaped line, and the gap grows with font
/// size. Text whose shaped lines already fit `width` keeps its own breaks, so a
/// box sized to its max-content width holds the same lines it was measured with.
fn wrap_width(node: &TextNode, paint: &PaintStyle, width: f32) -> f32 {
    if paint.no_wrap || !width.is_finite() || width <= 0.0 {
        return width;
    }
    let mut probe = build_paragraph_colored(node, paint, width, f32::INFINITY, false, TextPass::Fill);
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
    paint: &PaintStyle,
    width: f32,
) -> (
    skia_safe::textlayout::Paragraph,
    Option<skia_safe::textlayout::Paragraph>,
) {
    if separate_stroke(node, paint) {
        let wrap = wrap_width(node, paint, width);
        (
            build_paragraph_colored(node, paint, width, wrap, false, TextPass::Fill),
            Some(build_paragraph_colored(node, paint, width, wrap, false, TextPass::Stroke)),
        )
    } else {
        (build_paragraph(node, paint, width), None)
    }
}

fn build_paragraph_colored(
    node: &TextNode,
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
    let mut collection = node.font_collection.lock().unwrap().clone();
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
        let font = collection
            .find_typefaces(&[family], style)
            .into_iter()
            .next()
            .map(|face| skia_safe::Font::from_typeface(face, size));
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
        let color = if force_white {
            director_core::types::Color::WHITE
        } else {
            css_color
        };
        text.set_color(color.to_skia());
        text.set_font_families(&[family]);
        text.set_font_style(style);
        text.set_letter_spacing(spacing);
        text.set_word_spacing(word);
        if paint.tabular_nums {
            text.add_font_feature("tnum", 1);
        }
        if paint.text_stroke_width > 0.0 && pass != TextPass::Fill {
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
    let wrap = wrap_width(node, paint, width);
    let mut paragraph = build_paragraph_colored(node, paint, width, wrap, true, TextPass::Combined);
    fit_paragraph(&mut paragraph, paint, width);
    paragraph
}

pub fn prepare_paragraph(node: &mut TextNode, paint: &PaintStyle) {
    let (paragraph, stroke) = text_paragraphs(node, paint, f32::INFINITY);
    *node.paragraph.lock().unwrap() = Some(paragraph);
    *node.stroke_paragraph.lock().unwrap() = stroke;
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
pub fn layout_paragraph(node: &TextNode, paint: &PaintStyle, width: f32) -> f32 {
    let (mut paragraph, stroke) = text_paragraphs(node, paint, width);
    let used = fit_paragraph(&mut paragraph, paint, width);
    *node.paragraph.lock().unwrap() = Some(paragraph);
    *node.stroke_paragraph.lock().unwrap() = stroke;
    used
}

/// Lays `paragraph` out for a box `width` wide, as `layout_paragraph` describes, and returns the width used.
fn fit_paragraph(paragraph: &mut skia_safe::textlayout::Paragraph, paint: &PaintStyle, width: f32) -> f32 {
    let finite = width.is_finite() && width > 0.0;
    let ellipsis = paint.no_wrap && matches!(paint.text_overflow, style::TextOverflow::Ellipsis);
    let clamped = paint.line_clamp.is_some();
    if !finite {
        paragraph.layout(f32::INFINITY);
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
                .attr("id")
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
        match values.get("display").map(|d| d.trim()) {
            Some("none") => Level::Skip,
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
            || el.value().attr("id").into_iter().chain([path.as_str()]).any(|key| {
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

    fn node(
        &mut self,
        el: ElementRef<'_>,
        parent: &ComputedStyle,
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
        let mut values = self.css.cascade(el, parent)?;
        if tag == "svg" && !self.css.has_local_property(el, "color") {
            if let Some(color) = el.value().attr("color") {
                values.insert("color", color);
            }
        }
        if matches!(tag, "img" | "svg" | "video") {
            for dimension in ["width", "height"] {
                if !values.contains_key(dimension) {
                    if let Some(value) = el.value().attr(dimension) {
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
                .attr(name)
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
        let start = if root {
            0.0
        } else {
            composition_start + numeric("data-start", 0.0)?
        };
        let end = if root {
            self.options.duration
        } else if el.value().attr("data-duration").is_some() {
            start + numeric("data-duration", 0.0)?
        } else if el.value().attr("data-end").is_some() {
            composition_start + numeric("data-end", self.options.duration)?
        } else {
            self.options.duration
        };
        if start < 0.0 || end <= start {
            return Err(CompileError::Input(format!(
                "{context}: invalid clip window [{start}, {end})"
            )));
        }
        let mut svg_viewport: Option<(f32, f32)> = None;
        let mut atomics = Vec::new();
        let content: Option<Box<dyn Element>> = match tag {
            "img" => {
                let src = el
                    .value()
                    .attr("src")
                    .ok_or_else(|| CompileError::Input(format!("{context}: image without src")))?;
                if el.value().attr("srcset").is_some() {
                    return Err(CompileError::Input(
                        "responsive srcset requires extraction".into(),
                    ));
                }
                let image = if let Some(image) = self.images.get(src) {
                    image.clone()
                } else {
                    let bytes = self.assets.read(src)?;
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
                    for property in SVG_ANIMATION_PROPERTIES {
                        if self.css.has_stylesheet_property(child, property) {
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
                if el.value().attr("xmlns").is_none() {
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
                if el.value().attr("muted").is_none() {
                    return Err(CompileError::Input(
                        "video audio needs an explicit audio element; author muted video".into(),
                    ));
                }
                let src = el
                    .value()
                    .attr("src")
                    .ok_or_else(|| CompileError::Input("video without src".into()))?;
                let path = self.assets.media_path(src)?;
                let looped = el.value().attr("loop").is_some();
                let duration = if looped { media_duration(&path)? } else { 0.0 };
                let mut video = VideoNode::new(VideoSource::Path(path), RenderMode::Export);
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
            "audio" => {
                let src = el
                    .value()
                    .attr("src")
                    .ok_or_else(|| CompileError::Input("audio without src".into()))?;
                let bytes = self.assets.read(src)?;
                let mut samples = director_core::audio::load_audio_bytes(&bytes, 48000)
                    .map_err(|e| CompileError::Input(format!("audio '{src}': {e}")))?;
                let media_start = numeric("data-media-start", 0.0)?;
                if media_start < 0.0 {
                    return Err(CompileError::Input("negative audio media start".into()));
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
                        .any(|el| el.value().attr("data-hidden").is_some_and(|v| v != "false"));
                    track.volume = director_core::animation::Animated::new(if hidden {
                        0.0
                    } else {
                        volume as f32
                    });
                    track.loop_audio = el.value().attr("loop").is_some();
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
            | "h3" | "p" | "span" | "b" | "strong" | "em" | "i" | "small" => {
                let children: Vec<_> = el
                    .children()
                    .filter_map(ElementRef::wrap)
                    .filter(|e| !matches!(e.value().name(), "script" | "style" | "link"))
                    .collect();
                let mut text_block = true;
                for child in &children {
                    let child_values = self.css.cascade(*child, &values)?;
                    if self.level(*child, &child_values) == Level::Block {
                        text_block = false;
                        break;
                    }
                }
                if text_block {
                    let mut spans = Vec::new();
                    self.rich_text(el, &values, &paint, style_block, &mut spans, &mut atomics)?;
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
        // Absolute atomic boxes beside the text need the text in a leaf of its own.
        let anonymous_text = has_content
            && (matches!(layout.display, taffy::Display::Flex | taffy::Display::Grid) || !atomics.is_empty())
            && !matches!(tag, "img" | "svg" | "video");
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
        element.hidden = el.value().attr("data-hidden").is_some_and(|v| v != "false");
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
        for key in el.value().attr("id").into_iter().chain([path.as_str()]) {
            tracks.extend(self.timeline.remove(key).unwrap_or_default());
        }
        if let Some(mut series) = self.motion.series.remove(&el.id()) {
            if promoted {
                // Transforms and sizes do not apply to inline boxes.
                series.retain(|property, _| !matches!(property.as_str(), "transform" | "width" | "height"));
            }
            let steps = self.motion.steps.remove(&el.id()).unwrap_or_default();
            let target = el.value().attr("id").map_or_else(|| path.clone(), str::to_string);
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
        self.inherited.push((paint.font_size, passed));
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
            self.nodes += 1;
            if !atomics.is_empty() {
                let next_start = if !root && el.value().attr("data-composition-id").is_some() {
                    start
                } else {
                    composition_start
                };
                self.place_atomics(id, child, &atomics, next_start, child_block, depth)?;
            }
        }
        if let Some(name) = el.value().attr("id") {
            if self.ids.insert(name.into(), id).is_some() {
                return Err(CompileError::Input(format!("duplicate HTML id '{name}'")));
            }
        }
        if !has_content && tag != "audio" {
            let next_start = if !root && el.value().attr("data-composition-id").is_some() {
                start
            } else {
                composition_start
            };
            for child in el.children().filter_map(ElementRef::wrap) {
                if matches!(child.value().name(), "script" | "style" | "link") {
                    continue;
                }
                let child_id =
                    self.node(child, &values, next_start, false, child_block, depth + 1)?;
                // Taffy absolute offsets use the parent node, so fixed is a child of the composition root.
                if !root && self.fixed.contains(&child_id) {
                    self.director.scene.add_child(
                        self.root_id
                            .expect("composition root exists before a nested fixed box"),
                        child_id,
                    );
                } else {
                    self.director.scene.add_child(id, child_id);
                }
            }
        }
        if let Some(fallback) = svg_viewport {
            let next_start = if !root && el.value().attr("data-composition-id").is_some() {
                start
            } else {
                composition_start
            };
            self.place_foreign_objects(el, id, &values, fallback, next_start, depth)?;
        }
        self.inherited.pop();
        Ok(id)
    }

    /// The parent's text animations that reach `el`. A local `color`,
    /// `letter-spacing`, `word-spacing`, `line-height` or `text-shadow` stops its channel. A
    /// local relative font size scales the parent's, and an absolute one stops
    /// it. Replaced elements inherit nothing.
    fn inherit_text(&self, el: ElementRef<'_>, specified_font: Option<&str>, font_size: f32, tag: &str) -> Vec<animation::Animation> {
        let Some((parent_font, list)) = self.inherited.last() else { return Vec::new() };
        if list.is_empty() || matches!(tag, "img" | "svg" | "video" | "audio") {
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
                .or_else(|| foreign.value().attr(name))
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
        if let Some(name) = foreign.value().attr("id") {
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
            let child_id = self.node(child, &cascaded, start, false, block, depth + 1)?;
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
            let id = self.node(*child, values, start, false, block, depth + 1);
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
            let child = html_mut(&mut self.director, *child);
            if i == 0 && text.is_none() && matches!(child.inline, Some(InlineLink::Text(_))) {
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

    fn rich_text<'a>(
        &self,
        el: ElementRef<'a>,
        values: &ComputedStyle,
        paint: &PaintStyle,
        block: style::ContainingBlock,
        spans: &mut Vec<TextSpan>,
        atomics: &mut Vec<(ElementRef<'a>, ComputedStyle)>,
    ) -> Result<(), CompileError> {
        for node in el.children() {
            if let Some(text) = node.value().as_text() {
                spans.push(span(text.to_string(), paint));
            } else if let Some(child) = ElementRef::wrap(node) {
                if child.value().name() == "br" {
                    spans.push(span(HARD_BREAK.to_string(), paint));
                    continue;
                }
                if matches!(child.value().name(), "script" | "style" | "link") {
                    continue;
                }
                let mut child_values = self.css.cascade(child, values)?;
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
                self.rich_text(child, &child_values, &child_paint, block, spans, atomics)?;
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
        let output = std::process::Command::new("ffprobe")
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
