use crate::{
    animation,
    assets::AssetStore,
    css::{diagnostic, Css, Values},
    element::HtmlElement,
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
    parent: &Values,
    plan: &crate::smil::SmilPlan,
) -> Result<String, CompileError> {
    let mut out = String::new();
    write_svg_children(el, css, parent, &mut out, "", plan)?;
    Ok(out)
}

fn write_svg_children(
    el: ElementRef<'_>,
    css: &Css,
    parent: &Values,
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
    let values = css.cascade(el, &Values::new())?;
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

pub(crate) fn compile_with_assets(
    source: &str,
    mut options: CompileOptions,
    assets: Arc<AssetStore>,
) -> Result<CompiledHtml, CompileError> {
    let dom = Html::parse_document(source);
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
            "script" if !matches!(el.value().attr("type"),Some("application/json"|"application/ld+json"))=>return Err(CompileError::Diagnostics(vec![diagnostic("script","executable JavaScript requires explicit compile-time extraction; run director-engine html capture")])),
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
    load_fonts(&mut director, &css.fonts, &assets)?;
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
    };
    // Compute inherited values along the full ancestor chain, including body resets.
    let mut ancestors: Vec<_> = root.ancestors().filter_map(ElementRef::wrap).collect();
    ancestors.reverse();
    let mut parent = Values::new();
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
    if !builder.errors.is_empty() {
        return Err(CompileError::Diagnostics(builder.errors));
    }
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
    })
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
    svg_fonts: Option<Arc<usvg::fontdb::Database>>,
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
    for span in &node.spans {
        let size = span.font_size.unwrap_or(paint.font_size);
        let family = span.font_family.as_deref().unwrap_or(&paint.font_family);
        let weight = span.font_weight.unwrap_or(paint.font_weight);
        let italic = span.font_style.as_deref().unwrap_or(&paint.font_style) == "italic";
        let spacing = span.letter_spacing.unwrap_or(paint.letter_spacing);
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
                advance + spacing * gaps
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
        builder.push_style(&text);
        builder.add_text(&piece);
        builder.pop();
    }
    builder.build()
}

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
        line_height: paint.line_height,
        ..Default::default()
    }
}

impl Builder {
    fn ensure_svg_fonts(&mut self) -> Result<Arc<usvg::fontdb::Database>, CompileError> {
        cached_svg_fonts(&mut self.svg_fonts, &self.assets, &self.css.fonts)
    }

    fn node(
        &mut self,
        el: ElementRef<'_>,
        parent: &Values,
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
                values.insert("color".into(), color.into());
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
                        values.insert(dimension.into(), value);
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
        values.insert("font-size".into(), format!("{}px", paint.font_size));
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
                let plan = crate::smil::collect(el)?;
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
            | "h3" | "p" | "span" => {
                let children: Vec<_> = el
                    .children()
                    .filter_map(ElementRef::wrap)
                    .filter(|e| !matches!(e.value().name(), "script" | "style" | "link"))
                    .collect();
                if children.iter().all(|e| inline(e.value().name())) {
                    let mut spans = Vec::new();
                    self.rich_text(el, &values, &paint, style_block, &mut spans)?;
                    normalize(&mut spans, paint.preserve_space);
                    if spans.is_empty() {
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
        let anonymous_text = has_content
            && matches!(layout.display, taffy::Display::Flex | taffy::Display::Grid)
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
        let id = self.director.scene.add_node(Box::new(element));
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
            self.director.scene.add_child(id, child);
            self.nodes += 1;
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
        Ok(id)
    }

    fn place_foreign_objects(
        &mut self,
        svg: ElementRef<'_>,
        parent: NodeId,
        svg_values: &Values,
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
        svg_values: &Values,
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
            &Values::new(),
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
    fn rich_text(
        &self,
        el: ElementRef<'_>,
        values: &Values,
        paint: &PaintStyle,
        block: style::ContainingBlock,
        spans: &mut Vec<TextSpan>,
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
                self.rich_text(child, &child_values, &child_paint, block, spans)?;
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
