//! `<captions src="words.json" preset="karaoke" ...>`: word-highlight
//! captions as a core caption node. Attributes are the caption key table's
//! keys in kebab case (`highlight-color`, `max-lines`, `emphasis-words`), the
//! same keys Rhai `add_caption` takes, and `<position at>` children are the
//! placement track.

use crate::Attr;
use crate::CompileError;
use director_core::node::caption::{caption_lines_from_file, CueRules};
use director_core::node::caption_keys::{
    apply_caption_entries, caption_key, caption_key_names, caption_placement, caption_value_from_text, CaptionValue,
    PLACEMENT_KEYS,
};
use director_core::node::CaptionNode;
use scraper::ElementRef;
use skia_safe::textlayout::FontCollection;
use std::sync::{Arc, Mutex};

/// Attributes besides the style keys: the transcript, the look, the cue rules
/// and the element's own id and class.
pub const CAPTIONS_ATTRIBUTES: &[&str] = &[
    "src",
    "preset",
    "max-cue-chars",
    "max-cue-duration",
    "max-cue-gap",
    "cue-hold",
    "break-on-sentence",
    "id",
    "class",
];

/// CSS properties authors reach for that captions take as attributes.
pub const CAPTIONS_CSS: &[&str] = &[
    "color",
    "font-size",
    "font-family",
    "font-weight",
    "text-transform",
    "text-shadow",
    "-webkit-text-stroke",
    "background",
    "background-color",
    "position",
    "top",
    "bottom",
    "left",
    "right",
    "width",
    "max-width",
    "padding",
    "border-radius",
];

/// What `<captions>` needs from the compile.
pub struct CaptionsInput<'a> {
    pub read: &'a dyn Fn(&str) -> Result<Vec<u8>, CompileError>,
    /// Whether the element has a local declaration of a CSS property.
    pub has_css: &'a dyn Fn(&str) -> bool,
    pub fonts: Arc<Mutex<FontCollection>>,
    pub width: f32,
    pub height: f32,
    /// Composition seconds the transcript's zero lands on.
    pub start: f64,
}

/// Build the caption node `el` describes. Every attribute, child and value is
/// checked; a wrong one fails with the valid names or the range.
pub fn captions_node(el: ElementRef<'_>, input: &CaptionsInput<'_>) -> Result<CaptionNode, CompileError> {
    let context = el.value().attr_text("id").map(|id| format!("#{id}")).unwrap_or_else(|| "captions".into());
    let fail = |e: String| CompileError::Input(format!("{context}: {e}"));
    if let Some(prop) = CAPTIONS_CSS.iter().find(|p| (input.has_css)(p)) {
        return Err(fail(format!(
            "CSS {prop} does not style captions; set caption keys as attributes such as preset, size, color, \
             highlight-color, caption-position and bottom-margin (docs/reference/captions.md)"
        )));
    }

    let src = el.value().attr_text("src").ok_or_else(|| fail("captions need src, a .json, .vtt or .srt transcript".into()))?;
    let preset = el.value().attr_text("preset");
    let mut rules = CueRules::default();
    let mut entries: Vec<(&'static str, CaptionValue)> = Vec::new();
    for (name, value) in el.value().attrs() {
        let number = || -> Result<f64, CompileError> {
            value
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite() && *n > 0.0)
                .ok_or_else(|| fail(format!("{name} must be a number above 0, got {value:?}")))
        };
        match name {
            "max-cue-chars" => rules.max_chars = number()?.max(1.0) as usize,
            "max-cue-duration" => rules.max_duration = number()?,
            "max-cue-gap" => rules.max_gap = number()?,
            "cue-hold" => {
                rules.hold = value
                    .trim()
                    .strip_suffix('s')
                    .unwrap_or(value.trim())
                    .parse::<f64>()
                    .ok()
                    .filter(|n| n.is_finite() && *n >= 0.0)
                    .ok_or_else(|| fail(format!("cue-hold must be seconds of at least 0, got {value:?}")))?
            }
            "break-on-sentence" => {
                rules.break_on_sentence = match value.trim() {
                    "" | "true" => true,
                    "false" => false,
                    _ => return Err(fail(format!("break-on-sentence must be true or false, got {value:?}"))),
                }
            }
            _ if CAPTIONS_ATTRIBUTES.contains(&name) => {}
            _ => {
                let key = caption_key(&name.replace('-', "_")).ok_or_else(|| {
                    let mut valid: Vec<String> = CAPTIONS_ATTRIBUTES.iter().map(|a| a.to_string()).collect();
                    valid.extend(caption_key_names().iter().map(|k| k.replace('_', "-")));
                    fail(format!("'{name}' is not a captions attribute; valid: {}", valid.join(", ")))
                })?;
                entries.push((key.name, caption_value_from_text(key.name, value).map_err(fail)?));
            }
        }
    }

    let bytes = (input.read)(src)?;
    let mut lines = caption_lines_from_file(src, &String::from_utf8_lossy(&bytes), &rules).map_err(fail)?;
    if input.start != 0.0 {
        for line in &mut lines {
            line.start_sec += input.start;
            line.end_sec += input.start;
            for word in &mut line.words {
                word.start_sec += input.start;
                word.end_sec += input.start;
            }
        }
    }

    let mut node = CaptionNode::new(lines, input.fonts.clone());
    let mut style = node.style.clone();
    apply_caption_entries(&mut style, preset, &entries, input.height).map_err(fail)?;
    node.style = style;
    node.rebuild_layout_style();

    let mut track = Vec::new();
    for child in el.children() {
        if child.value().as_text().is_some_and(|t| t.trim().is_empty()) || child.value().is_comment() {
            continue;
        }
        let Some(position) = ElementRef::wrap(child).filter(|c| c.value().name() == "position") else {
            return Err(fail("captions hold only <position at=\"seconds\" ...> children".into()));
        };
        let at = position
            .value()
            .attr_text("at")
            .and_then(|v| v.trim().strip_suffix('s').unwrap_or(v.trim()).parse::<f64>().ok())
            .ok_or_else(|| fail("<position> needs at, a time in seconds".into()))?;
        let mut keys: Vec<(&str, CaptionValue)> = Vec::new();
        for (name, value) in position.value().attrs() {
            if name == "at" {
                continue;
            }
            let key = name.replace('-', "_");
            let Some(key) = PLACEMENT_KEYS.iter().find(|k| **k == key).copied() else {
                let valid: Vec<String> = PLACEMENT_KEYS.iter().map(|k| k.replace('_', "-")).collect();
                return Err(fail(format!("<position> takes at, {}; got '{name}'", valid.join(", "))));
            };
            keys.push((key, caption_value_from_text(key, value).map_err(fail)?));
        }
        track.push(caption_placement(&node.style, at + input.start, &keys).map_err(fail)?);
    }
    if !track.is_empty() {
        node.set_positions(track);
    }
    node.after_style_changed();
    // Measured line breaks need the column width before the first layout.
    let width = node.box_width_in(input.width);
    node.set_box_width(Some(width));
    Ok(node)
}
