//! The caption key table. Every caption style key that Rhai `add_caption`,
//! the HTML `<captions>` element and the scene tools read lives in
//! [`CAPTION_KEYS`] with its kind, default, group and doc, and
//! [`set_caption_key`] is the one setter they share. [`CAPTION_PRESETS`] are
//! named sets of keys written for a 1920 px tall frame.

use super::caption::{
    safe_area_presets, safe_zone, CaptionPlacement, CaptionPosition, CaptionStyle, LineBreak, PhraseLineMode,
    REFERENCE_SHORT_FORM_HEIGHT,
};
use crate::scripting::utils::parse_hex_color;
use crate::types::Color;

/// A key's value as an author wrote it, from any front door.
#[derive(Clone, Debug, PartialEq)]
pub enum CaptionValue {
    Number(f64),
    Bool(bool),
    Text(String),
    List(Vec<String>),
}

impl CaptionValue {
    fn type_name(&self) -> &'static str {
        match self {
            Self::Number(_) => "a number",
            Self::Bool(_) => "true or false",
            Self::Text(_) => "a string",
            Self::List(_) => "a list",
        }
    }
}

/// What a key takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KeyKind {
    /// Frame pixels at least `min`. Preset values scale with the frame height.
    Px { min: f64 },
    /// A number in `min..=max`.
    Number { min: f64, max: f64 },
    /// Seconds in `min..=max`.
    Seconds { min: f64, max: f64 },
    /// A whole number of at least `min`.
    Whole { min: u32 },
    Bool,
    /// `#RGB`, `#RGBA`, `#RRGGBB` or `#RRGGBBAA`.
    Color,
    /// One of the names.
    Choice(&'static [&'static str]),
    /// A font family name.
    Family,
    /// 100 to 900 or a name such as `bold`.
    Weight,
    /// An anchor such as `bottom` or `free_xy(540, 1500)`.
    Position,
    /// A platform safe area such as `tiktok`.
    Platform,
    /// A list of words, or one string of words split at commas.
    Words,
}

impl KeyKind {
    /// How the reference page names the kind.
    pub fn describe(&self) -> String {
        match self {
            Self::Px { .. } => "px".into(),
            Self::Number { min, max } => format!("{min} to {max}"),
            Self::Seconds { min, max } => format!("{min} to {max} s"),
            Self::Whole { min } => format!("whole, at least {min}"),
            Self::Bool => "true / false".into(),
            Self::Color => "hex colour".into(),
            Self::Choice(names) => names.join(" / "),
            Self::Family => "font family".into(),
            Self::Weight => "100 to 900 or a name".into(),
            Self::Position => "anchor".into(),
            Self::Platform => safe_area_presets().join(" / "),
            Self::Words => "words".into(),
        }
    }
}

/// One caption style key.
#[derive(Clone, Copy, Debug)]
pub struct CaptionKey {
    pub name: &'static str,
    /// Other names that set the same field.
    pub aliases: &'static [&'static str],
    pub kind: KeyKind,
    /// Section of the reference page.
    pub group: &'static str,
    /// The default as an author writes it; `unset` when the key has none.
    pub default: &'static str,
    pub doc: &'static str,
}

const fn key(
    name: &'static str,
    kind: KeyKind,
    group: &'static str,
    default: &'static str,
    doc: &'static str,
) -> CaptionKey {
    CaptionKey { name, aliases: &[], kind, group, default, doc }
}

const TYPE: &str = "Type";
const COLOUR: &str = "Colour";
const OUTLINE: &str = "Outline, shadow and glow";
const PILL: &str = "Pill";
const PLACEMENT: &str = "Placement";
const LINES: &str = "Lines";
const MOTION: &str = "Motion";
const EMPHASIS: &str = "Emphasis";

/// Every caption style key, in reference order.
pub const CAPTION_KEYS: &[CaptionKey] = &[
    key("size", KeyKind::Number { min: 0.005, max: 0.5 }, TYPE, "unset", "Font size as a fraction of the frame height; sets font_size. 0.035 is 67 px on a 1920 px frame."),
    key("font_size", KeyKind::Px { min: 1.0 }, TYPE, "42", "Font size in frame pixels."),
    key("font_family", KeyKind::Family, TYPE, "Poppins", "Font family. Bundled: Poppins (bold), Inter, Roboto, Archivo (variable weight), JetBrains Mono."),
    CaptionKey { aliases: &["weight"], ..key("font_weight", KeyKind::Weight, TYPE, "700", "Font weight.") },
    key("uppercase", KeyKind::Bool, TYPE, "false", "Paint every word in capitals."),
    key("text_direction", KeyKind::Choice(&["auto", "ltr", "rtl"]), TYPE, "auto", "Paragraph direction; auto reads the first strong character."),
    key("color", KeyKind::Color, COLOUR, "#FFFFFF", "Words not yet spoken."),
    key("highlight_color", KeyKind::Color, COLOUR, "#FFD60A", "The word being spoken."),
    key("spoken_color", KeyKind::Color, COLOUR, "#C7C7C7", "Words already spoken."),
    key("future_opacity", KeyKind::Number { min: 0.0, max: 1.0 }, COLOUR, "0.92", "Opacity of words not yet spoken."),
    key("active_opacity", KeyKind::Number { min: 0.0, max: 1.0 }, COLOUR, "1", "Opacity of the word being spoken."),
    key("spoken_opacity", KeyKind::Number { min: 0.0, max: 1.0 }, COLOUR, "0.55", "Opacity of words already spoken."),
    key("color_transition_duration", KeyKind::Seconds { min: 0.0, max: 1.0 }, COLOUR, "0", "Seconds the highlight fades in before a word starts; 0 lands it on the word's first frame."),
    key("stroke_enabled", KeyKind::Bool, OUTLINE, "true", "Paint the outline."),
    key("stroke_width", KeyKind::Px { min: 0.0 }, OUTLINE, "2", "Outline width outside the glyph edge, round joined."),
    key("stroke_color", KeyKind::Color, OUTLINE, "#000000EB", "Outline colour."),
    key("text_shadow_enabled", KeyKind::Bool, OUTLINE, "true", "Paint the drop shadow."),
    key("text_shadow_color", KeyKind::Color, OUTLINE, "#000000D9", "Drop shadow colour."),
    key("text_shadow_blur", KeyKind::Px { min: 0.0 }, OUTLINE, "4", "Drop shadow blur."),
    key("text_shadow_offset_x", KeyKind::Px { min: -1.0e6 }, OUTLINE, "0", "Drop shadow offset to the right."),
    key("text_shadow_offset_y", KeyKind::Px { min: -1.0e6 }, OUTLINE, "2", "Drop shadow offset down."),
    key("glow_enabled", KeyKind::Bool, OUTLINE, "true", "Glow around the word being spoken, in its highlight colour."),
    key("glow_blur", KeyKind::Px { min: 0.0 }, OUTLINE, "4", "Glow blur."),
    key("pill_enabled", KeyKind::Bool, PILL, "true", "Paint a rounded box behind each line."),
    key("pill_color", KeyKind::Color, PILL, "#0000009E", "Box colour."),
    key("pill_padding_h", KeyKind::Px { min: 0.0 }, PILL, "18", "Box padding left and right of the line."),
    key("pill_padding_v", KeyKind::Px { min: 0.0 }, PILL, "10", "Box padding above and below the line."),
    key("pill_radius", KeyKind::Px { min: 0.0 }, PILL, "14", "Box corner radius."),
    key("caption_position", KeyKind::Position, PLACEMENT, "bottom", "Anchor: bottom, top, center, left, right, top_left, top_right, bottom_left, bottom_right, free(y) with y from 0 to 1, or free_xy(x, y) in frame pixels."),
    key("top_margin", KeyKind::Px { min: -1.0e6 }, PLACEMENT, "60", "Gap above a top anchor."),
    key("bottom_margin", KeyKind::Px { min: -1.0e6 }, PLACEMENT, "110", "Gap below a bottom anchor."),
    key("left_margin", KeyKind::Px { min: -1.0e6 }, PLACEMENT, "60", "Gap left of a left anchor."),
    key("right_margin", KeyKind::Px { min: -1.0e6 }, PLACEMENT, "60", "Gap right of a right anchor."),
    CaptionKey { aliases: &["safe_area_preset"], ..key("safe_area", KeyKind::Platform, PLACEMENT, "unset", "Platform safe area; sets the top and bottom margins at its 1080x1920 size.") },
    key("layout_width_fraction", KeyKind::Number { min: 0.05, max: 1.0 }, PLACEMENT, "0.86", "Text column width as a fraction of the caption box."),
    key("auto_phrase_break", KeyKind::Bool, LINES, "true", "Split cues into display lines."),
    key("line_break", KeyKind::Choice(&["width", "words"]), LINES, "width", "width fits each line to the measured column, balanced, with no article, preposition or conjunction at a line end; words cuts fixed chunks of max_words_per_line."),
    key("max_words_per_line", KeyKind::Whole { min: 1 }, LINES, "4", "Most words on one line."),
    key("max_lines", KeyKind::Whole { min: 1 }, LINES, "1", "Lines shown together in replace mode; each page of lines replaces the last."),
    key("phrase_line_mode", KeyKind::Choice(&["replace", "stack"]), LINES, "replace", "replace shows one page of lines at a time; stack keeps every revealed line of the cue."),
    key("line_spacing", KeyKind::Px { min: -1.0e6 }, LINES, "16", "Gap between stacked lines."),
    key("line_reveal_offset_1", KeyKind::Number { min: 0.0, max: 1.0 }, LINES, "0.45", "When an estimated cue's second line appears, as a fraction of the cue."),
    key("line_reveal_duration_1", KeyKind::Seconds { min: 0.0, max: 2.0 }, LINES, "0.2", "Seconds the second line fades in."),
    key("active_pop_scale", KeyKind::Number { min: 1.0, max: 1.10 }, MOTION, "1.04", "Scale of the word being spoken on its first frames."),
    key("active_pop_duration", KeyKind::Seconds { min: 0.05, max: 0.15 }, MOTION, "0.08", "Seconds the pop settles over."),
    key("emphasis_color", KeyKind::Color, EMPHASIS, "unset", "Colour of the emphasis words while they are not being spoken."),
    key("emphasis_words", KeyKind::Words, EMPHASIS, "unset", "Words to paint in emphasis_color, matched without case or edge punctuation."),
];

/// The key `name` names, by its name or an alias.
pub fn caption_key(name: &str) -> Option<&'static CaptionKey> {
    CAPTION_KEYS.iter().find(|k| k.name == name || k.aliases.contains(&name))
}

/// Every style key name and alias.
pub fn caption_key_names() -> Vec<&'static str> {
    CAPTION_KEYS.iter().flat_map(|k| std::iter::once(k.name).chain(k.aliases.iter().copied())).collect()
}

/// A key's value from text, as an HTML attribute writes it. Number kinds
/// parse the number (pixel keys may end in `px`, seconds in `s`), flags take
/// `true`, `false` or an empty value for true, and every other kind keeps the
/// text for [`set_caption_key`] to check.
pub fn caption_value_from_text(name: &str, value: &str) -> Result<CaptionValue, String> {
    let key = caption_key(name).ok_or_else(|| format!("'{name}' is not a caption style key"))?;
    let t = value.trim();
    let parse = |t: &str| {
        t.parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .map(CaptionValue::Number)
            .ok_or_else(|| format!("{name} must be a number, got {value:?}"))
    };
    match key.kind {
        KeyKind::Px { .. } => parse(t.strip_suffix("px").unwrap_or(t).trim()),
        KeyKind::Seconds { .. } => parse(t.strip_suffix('s').unwrap_or(t).trim()),
        KeyKind::Number { .. } | KeyKind::Whole { .. } => parse(t),
        KeyKind::Bool => match t {
            "" | "true" => Ok(CaptionValue::Bool(true)),
            "false" => Ok(CaptionValue::Bool(false)),
            _ => Err(format!("{name} must be true or false, got {value:?}")),
        },
        _ => Ok(CaptionValue::Text(t.to_string())),
    }
}

fn number(name: &str, value: &CaptionValue) -> Result<f64, String> {
    match value {
        CaptionValue::Number(n) if n.is_finite() => Ok(*n),
        other => Err(format!("{name} must be a number, got {}", other.type_name())),
    }
}

fn text<'a>(name: &str, value: &'a CaptionValue) -> Result<&'a str, String> {
    match value {
        CaptionValue::Text(s) => Ok(s),
        other => Err(format!("{name} must be a string, got {}", other.type_name())),
    }
}

fn in_range(name: &str, v: f64, min: f64, max: f64, unit: &str) -> Result<f64, String> {
    if (min..=max).contains(&v) {
        Ok(v)
    } else {
        Err(format!("{name} must be between {min} and {max}{unit}, got {v}"))
    }
}

fn weight(name: &str, value: &CaptionValue) -> Result<u16, String> {
    let w = match value {
        CaptionValue::Number(n) if n.fract() == 0.0 => Some(*n),
        CaptionValue::Text(s) => match s.as_str() {
            "thin" => Some(100.0),
            "extra_light" | "extra-light" | "extralight" => Some(200.0),
            "light" => Some(300.0),
            "normal" | "regular" => Some(400.0),
            "medium" => Some(500.0),
            "semi_bold" | "semi-bold" | "semibold" => Some(600.0),
            "bold" => Some(700.0),
            "extra_bold" | "extra-bold" | "extrabold" => Some(800.0),
            "black" => Some(900.0),
            other => other.parse::<f64>().ok(),
        },
        _ => None,
    };
    match w {
        Some(w) if (1.0..=1000.0).contains(&w) => Ok(w as u16),
        _ => Err(format!("{name} must be 100 to 900 or a name such as \"bold\", got {value:?}")),
    }
}

/// A colour key's value.
pub fn caption_color(name: &str, value: &CaptionValue) -> Result<Color, String> {
    let s = text(name, value)?;
    parse_hex_color(s).ok_or_else(|| format!("{name} must be a hex colour such as \"#FFD400\" or \"#00000099\", got {s:?}"))
}

/// A `caption_position` value.
pub fn caption_position(name: &str, value: &CaptionValue) -> Result<CaptionPosition, String> {
    let s = text(name, value)?;
    CaptionPosition::try_from_str(s).map_err(|_| {
        format!("{name} must be bottom, top, center, left, right, top_left, top_right, bottom_left, bottom_right, free(y) or free_xy(x, y), got {s:?}")
    })
}

/// Set one style key. Pixel values multiply by `px_scale`; `size` multiplies
/// by `frame_height`. Every value is checked against the key's kind.
pub fn set_caption_key(
    style: &mut CaptionStyle,
    name: &str,
    value: &CaptionValue,
    px_scale: f32,
    frame_height: f32,
) -> Result<(), String> {
    let key = caption_key(name).ok_or_else(|| format!("'{name}' is not a caption style key"))?;
    let px = |v: &CaptionValue| -> Result<f32, String> {
        let KeyKind::Px { min } = key.kind else { unreachable!("px read of a non-px key") };
        let n = number(name, v)?;
        if n < min {
            return Err(format!("{name} must be at least {min}, got {n}"));
        }
        Ok(n as f32 * px_scale)
    };
    let num = |v: &CaptionValue| -> Result<f64, String> {
        let n = number(name, v)?;
        match key.kind {
            KeyKind::Number { min, max } => in_range(name, n, min, max, ""),
            KeyKind::Seconds { min, max } => in_range(name, n, min, max, " seconds"),
            _ => Ok(n),
        }
    };
    let flag = |v: &CaptionValue| -> Result<bool, String> {
        match v {
            CaptionValue::Bool(b) => Ok(*b),
            other => Err(format!("{name} must be true or false, got {}", other.type_name())),
        }
    };
    let whole = |v: &CaptionValue| -> Result<usize, String> {
        let n = number(name, v)?;
        let KeyKind::Whole { min } = key.kind else { unreachable!("whole read of a non-whole key") };
        if n.fract() != 0.0 || n < min as f64 {
            return Err(format!("{name} must be a whole number of at least {min}, got {n}"));
        }
        Ok(n as usize)
    };
    let choice = |v: &CaptionValue| -> Result<String, String> {
        let s = text(name, v)?;
        let KeyKind::Choice(names) = key.kind else { unreachable!("choice read of a non-choice key") };
        if names.contains(&s) {
            Ok(s.to_string())
        } else {
            Err(format!("{name} must be {}, got {s:?}", names.join(" or ")))
        }
    };
    let color = |v: &CaptionValue| caption_color(name, v);

    let s = style;
    match key.name {
        "size" => s.font_size = (num(value)? as f32) * frame_height,
        "font_size" => {
            let v = px(value)?;
            if v <= 0.0 {
                return Err(format!("font_size must be above 0, got {v}"));
            }
            s.font_size = v;
        }
        "font_family" => {
            let f = text(name, value)?.trim();
            if f.is_empty() {
                return Err("font_family must name a font".into());
            }
            s.font_family = f.to_string();
        }
        "font_weight" => s.font_weight = weight(name, value)?,
        "uppercase" => s.uppercase = flag(value)?,
        "text_direction" => {
            s.text_direction = crate::text_direction::parse_text_direction_mode(&choice(value)?);
        }
        "color" => s.color = color(value)?,
        "highlight_color" => s.highlight_color = color(value)?,
        "spoken_color" => s.spoken_color = color(value)?,
        "future_opacity" => s.future_opacity = num(value)? as f32,
        "active_opacity" => s.active_opacity = num(value)? as f32,
        "spoken_opacity" => s.spoken_color_opacity = num(value)? as f32,
        "color_transition_duration" => s.color_transition_duration = num(value)?,
        "stroke_enabled" => s.stroke_enabled = flag(value)?,
        "stroke_width" => s.stroke_width = px(value)?,
        "stroke_color" => s.stroke_color = color(value)?,
        "text_shadow_enabled" => s.text_shadow_enabled = flag(value)?,
        "text_shadow_color" => s.text_shadow_color = color(value)?,
        "text_shadow_blur" => s.text_shadow_blur = px(value)?,
        "text_shadow_offset_x" => s.text_shadow_offset_x = px(value)?,
        "text_shadow_offset_y" => s.text_shadow_offset_y = px(value)?,
        "glow_enabled" => s.glow_enabled = flag(value)?,
        "glow_blur" => s.glow_blur = px(value)?,
        "pill_enabled" => s.pill_enabled = flag(value)?,
        "pill_color" => s.pill_color = color(value)?,
        "pill_padding_h" => s.pill_padding_h = px(value)?,
        "pill_padding_v" => s.pill_padding_v = px(value)?,
        "pill_radius" => s.pill_radius = px(value)?,
        "caption_position" => s.position = caption_position(name, value)?,
        "top_margin" => s.top_margin = px(value)?,
        "bottom_margin" => s.bottom_margin = px(value)?,
        "left_margin" => s.left_margin = px(value)?,
        "right_margin" => s.right_margin = px(value)?,
        "safe_area" => {
            let p = text(name, value)?;
            if safe_zone(p).is_none() {
                return Err(format!("{name}: unknown platform {p:?}; valid: {}", safe_area_presets().join(", ")));
            }
            s.safe_area_preset = Some(p.to_string());
        }
        "layout_width_fraction" => s.layout_width_fraction = num(value)? as f32,
        "auto_phrase_break" => s.auto_phrase_break = flag(value)?,
        "line_break" => {
            s.line_break = if choice(value)? == "words" { LineBreak::Words } else { LineBreak::Width };
        }
        "max_words_per_line" => s.max_words_per_line = whole(value)?,
        "max_lines" => s.max_lines = whole(value)?,
        "phrase_line_mode" => {
            s.phrase_line_mode =
                if choice(value)? == "stack" { PhraseLineMode::Stack } else { PhraseLineMode::Replace };
        }
        "line_spacing" => s.line_spacing = px(value)?,
        "line_reveal_offset_1" => {
            let v = num(value)? as f32;
            if s.line_reveal_offsets.len() < 2 {
                s.line_reveal_offsets.resize(2, 0.0);
            }
            s.line_reveal_offsets[1] = v;
        }
        "line_reveal_duration_1" => {
            let v = num(value)? as f32;
            if s.line_reveal_durations.len() < 2 {
                s.line_reveal_durations.resize(2, 0.10);
            }
            s.line_reveal_durations[1] = v;
        }
        "active_pop_scale" => s.active_pop_scale = num(value)? as f32,
        "active_pop_duration" => s.active_pop_duration = num(value)?,
        "emphasis_color" => s.emphasis_color = Some(color(value)?),
        "emphasis_words" => {
            s.emphasis_words = match value {
                CaptionValue::List(words) => words.clone(),
                CaptionValue::Text(t) => t.split(',').map(|w| w.trim().to_string()).filter(|w| !w.is_empty()).collect(),
                other => return Err(format!("{name} must be a list of words, got {}", other.type_name())),
            };
        }
        other => unreachable!("caption key {other} has no setter"),
    }
    Ok(())
}

/// A preset value as written in the table.
#[derive(Clone, Copy, Debug)]
pub enum PresetValue {
    N(f64),
    B(bool),
    T(&'static str),
}

impl PresetValue {
    pub fn value(&self) -> CaptionValue {
        match *self {
            Self::N(n) => CaptionValue::Number(n),
            Self::B(b) => CaptionValue::Bool(b),
            Self::T(t) => CaptionValue::Text(t.to_string()),
        }
    }
}

/// A named caption look: keys written for a 1920 px tall frame.
#[derive(Clone, Copy, Debug)]
pub struct CaptionPreset {
    pub name: &'static str,
    pub doc: &'static str,
    pub values: &'static [(&'static str, PresetValue)],
}

use PresetValue::{B, N, T};

/// The built-in caption looks.
pub const CAPTION_PRESETS: &[CaptionPreset] = &[
    CaptionPreset {
        name: "bold-outline",
        doc: "Heavy capitals with a thick black outline and a yellow active word, three words a line.",
        values: &[
            ("font_family", T("Archivo")),
            ("font_weight", N(900.0)),
            ("font_size", N(84.0)),
            ("uppercase", B(true)),
            ("color", T("#FFFFFF")),
            ("highlight_color", T("#FFD60A")),
            ("spoken_color", T("#FFFFFF")),
            ("future_opacity", N(1.0)),
            ("spoken_opacity", N(1.0)),
            ("stroke_width", N(7.0)),
            ("stroke_color", T("#000000")),
            ("text_shadow_color", T("#00000099")),
            ("text_shadow_blur", N(10.0)),
            ("text_shadow_offset_y", N(6.0)),
            ("glow_enabled", B(false)),
            ("pill_enabled", B(false)),
            ("max_words_per_line", N(3.0)),
            ("bottom_margin", N(300.0)),
            ("active_pop_scale", N(1.06)),
        ],
    },
    CaptionPreset {
        name: "karaoke",
        doc: "Two-line pages; spoken words fill with the highlight colour as the voice reaches them.",
        values: &[
            ("font_family", T("Poppins")),
            ("font_size", N(68.0)),
            ("color", T("#FFFFFF")),
            ("highlight_color", T("#FFD60A")),
            ("spoken_color", T("#FFD60A")),
            ("future_opacity", N(1.0)),
            ("spoken_opacity", N(1.0)),
            ("stroke_width", N(5.0)),
            ("stroke_color", T("#000000")),
            ("glow_enabled", B(false)),
            ("pill_enabled", B(false)),
            ("max_words_per_line", N(5.0)),
            ("max_lines", N(2.0)),
            ("bottom_margin", N(280.0)),
            ("active_pop_scale", N(1.0)),
        ],
    },
    CaptionPreset {
        name: "pill",
        doc: "White words on a dark rounded box per line with a yellow active word.",
        values: &[
            ("font_family", T("Poppins")),
            ("font_size", N(64.0)),
            ("color", T("#FFFFFF")),
            ("highlight_color", T("#FFD60A")),
            ("spoken_color", T("#FFFFFF")),
            ("spoken_opacity", N(0.8)),
            ("stroke_enabled", B(false)),
            ("text_shadow_enabled", B(false)),
            ("glow_enabled", B(false)),
            ("pill_enabled", B(true)),
            ("pill_color", T("#000000B3")),
            ("pill_padding_h", N(28.0)),
            ("pill_padding_v", N(14.0)),
            ("pill_radius", N(22.0)),
            ("max_words_per_line", N(4.0)),
            ("bottom_margin", N(300.0)),
        ],
    },
    CaptionPreset {
        name: "pop",
        doc: "Two big words at a time, a green active word that pops, heavy outline and shadow.",
        values: &[
            ("font_family", T("Archivo")),
            ("font_weight", N(900.0)),
            ("font_size", N(110.0)),
            ("uppercase", B(true)),
            ("color", T("#FFFFFF")),
            ("highlight_color", T("#3DFF6E")),
            ("spoken_color", T("#FFFFFF")),
            ("future_opacity", N(1.0)),
            ("spoken_opacity", N(1.0)),
            ("stroke_width", N(8.0)),
            ("stroke_color", T("#000000")),
            ("text_shadow_color", T("#000000B3")),
            ("text_shadow_blur", N(14.0)),
            ("text_shadow_offset_y", N(8.0)),
            ("glow_enabled", B(false)),
            ("pill_enabled", B(false)),
            ("max_words_per_line", N(2.0)),
            ("caption_position", T("center")),
            ("active_pop_scale", N(1.10)),
            ("active_pop_duration", N(0.12)),
        ],
    },
    CaptionPreset {
        name: "subtitle",
        doc: "Plain two-line subtitles: regular weight, soft shadow, no highlight.",
        values: &[
            ("font_family", T("Inter")),
            ("font_weight", N(400.0)),
            ("font_size", N(52.0)),
            ("color", T("#FFFFFF")),
            ("highlight_color", T("#FFFFFF")),
            ("spoken_color", T("#FFFFFF")),
            ("future_opacity", N(1.0)),
            ("spoken_opacity", N(1.0)),
            ("stroke_enabled", B(false)),
            ("text_shadow_color", T("#000000E6")),
            ("text_shadow_blur", N(6.0)),
            ("text_shadow_offset_y", N(2.0)),
            ("glow_enabled", B(false)),
            ("pill_enabled", B(false)),
            ("max_words_per_line", N(8.0)),
            ("max_lines", N(2.0)),
            ("line_spacing", N(8.0)),
            ("layout_width_fraction", N(0.8)),
            ("bottom_margin", N(140.0)),
            ("active_pop_scale", N(1.0)),
        ],
    },
    CaptionPreset {
        name: "boxed",
        doc: "Black words on a white box per line with a red active word.",
        values: &[
            ("font_family", T("Inter")),
            ("font_weight", N(700.0)),
            ("font_size", N(60.0)),
            ("color", T("#111111")),
            ("highlight_color", T("#E5262E")),
            ("spoken_color", T("#111111")),
            ("future_opacity", N(1.0)),
            ("spoken_opacity", N(1.0)),
            ("stroke_enabled", B(false)),
            ("text_shadow_enabled", B(false)),
            ("glow_enabled", B(false)),
            ("pill_enabled", B(true)),
            ("pill_color", T("#FFFFFF")),
            ("pill_padding_h", N(22.0)),
            ("pill_padding_v", N(10.0)),
            ("pill_radius", N(6.0)),
            ("max_words_per_line", N(4.0)),
            ("bottom_margin", N(300.0)),
            ("active_pop_scale", N(1.0)),
        ],
    },
];

/// The preset `name`.
pub fn caption_preset(name: &str) -> Option<&'static CaptionPreset> {
    CAPTION_PRESETS.iter().find(|p| p.name == name)
}

/// Preset names, in table order.
pub fn caption_preset_names() -> Vec<&'static str> {
    CAPTION_PRESETS.iter().map(|p| p.name).collect()
}

/// Apply the preset `name` to `style` for a frame `frame_height` px tall.
pub fn apply_caption_preset(style: &mut CaptionStyle, name: &str, frame_height: f32) -> Result<(), String> {
    let preset = caption_preset(name)
        .ok_or_else(|| format!("preset: unknown caption preset {name:?}; valid: {}", caption_preset_names().join(", ")))?;
    let scale = frame_height / REFERENCE_SHORT_FORM_HEIGHT;
    for (key, value) in preset.values {
        set_caption_key(style, key, &value.value(), scale, frame_height)
            .map_err(|e| format!("preset {name}: {e}"))?;
    }
    Ok(())
}

/// Apply a preset and then style keys, as every front door does: `preset`
/// first, then each key in `entries`. `size` and `font_size` together fail.
pub fn apply_caption_entries(
    style: &mut CaptionStyle,
    preset: Option<&str>,
    entries: &[(&str, CaptionValue)],
    frame_height: f32,
) -> Result<(), String> {
    if let Some(name) = preset {
        apply_caption_preset(style, name, frame_height)?;
    }
    let has = |n: &str| entries.iter().any(|(k, _)| *k == n);
    if has("size") && has("font_size") {
        return Err("give size or font_size, not both".into());
    }
    for (name, value) in entries {
        set_caption_key(style, name, value, 1.0, frame_height)?;
    }
    Ok(())
}

/// Keys a `positions` entry takes besides `at`.
pub const PLACEMENT_KEYS: &[&str] = &["caption_position", "top_margin", "bottom_margin", "left_margin", "right_margin"];

/// One `positions` entry: from `at` seconds the caption takes these keys.
/// Keys left out keep the value `style` has.
pub fn caption_placement(style: &CaptionStyle, at: f64, entries: &[(&str, CaptionValue)]) -> Result<CaptionPlacement, String> {
    if !at.is_finite() || at < 0.0 {
        return Err(format!("positions: at must be a time of at least 0 seconds, got {at}"));
    }
    let mut s = style.clone();
    for (name, value) in entries {
        if !PLACEMENT_KEYS.contains(name) {
            return Err(format!("positions: '{name}' is not a placement key; valid: at, {}", PLACEMENT_KEYS.join(", ")));
        }
        set_caption_key(&mut s, name, value, 1.0, 0.0).map_err(|e| format!("positions: {e}"))?;
    }
    Ok(CaptionPlacement {
        at,
        position: s.position,
        top_margin: s.top_margin,
        bottom_margin: s.bottom_margin,
        left_margin: s.left_margin,
        right_margin: s.right_margin,
    })
}

/// The caption key reference as Markdown, generated from [`CAPTION_KEYS`]
/// and [`CAPTION_PRESETS`].
pub fn caption_reference_markdown() -> String {
    let mut out = String::from(
        "# Caption keys\n\nGenerated from `crates/director-core/src/node/caption_keys.rs` by \
         `director-engine captions keys --format markdown`. Do not edit by hand.\n\n\
         Pixel keys are frame pixels. `preset` applies first and its pixel values scale with the frame \
         height from a 1920 px frame; every other key overrides it.\n",
    );
    let mut group = "";
    for k in CAPTION_KEYS {
        if k.group != group {
            group = k.group;
            out.push_str(&format!("\n## {group}\n\n| Key | Takes | Default | What it does |\n|---|---|---|---|\n"));
        }
        let name = if k.aliases.is_empty() {
            format!("`{}`", k.name)
        } else {
            format!("`{}` (`{}`)", k.name, k.aliases.join("`, `"))
        };
        out.push_str(&format!("| {name} | {} | {} | {} |\n", k.kind.describe(), k.default, k.doc));
    }
    out.push_str("\n## Presets\n\n| Preset | Look |\n|---|---|\n");
    for p in CAPTION_PRESETS {
        out.push_str(&format!("| `{}` | {} |\n", p.name, p.doc));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value_of(k: &CaptionKey) -> Option<CaptionValue> {
        if k.default == "unset" {
            return None;
        }
        Some(match k.kind {
            KeyKind::Bool => CaptionValue::Bool(k.default == "true"),
            KeyKind::Px { .. } | KeyKind::Number { .. } | KeyKind::Seconds { .. } | KeyKind::Whole { .. } => {
                CaptionValue::Number(k.default.parse().expect("numeric default"))
            }
            KeyKind::Weight => CaptionValue::Number(k.default.parse().expect("weight default")),
            _ => CaptionValue::Text(k.default.to_string()),
        })
    }

    fn close(a: &serde_json::Value, b: &serde_json::Value) -> bool {
        match (a, b) {
            (serde_json::Value::Number(x), serde_json::Value::Number(y)) => {
                (x.as_f64().unwrap() - y.as_f64().unwrap()).abs() < 1.0 / 255.0 + 1e-6
            }
            (serde_json::Value::Array(x), serde_json::Value::Array(y)) => {
                x.len() == y.len() && x.iter().zip(y).all(|(a, b)| close(a, b))
            }
            (serde_json::Value::Object(x), serde_json::Value::Object(y)) => {
                x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| close(v, w)))
            }
            _ => a == b,
        }
    }

    #[test]
    fn every_documented_default_is_the_style_default() {
        let default = serde_json::to_value(CaptionStyle::default()).unwrap();
        for k in CAPTION_KEYS {
            let Some(v) = value_of(k) else { continue };
            let mut style = CaptionStyle::default();
            set_caption_key(&mut style, k.name, &v, 1.0, 1920.0).unwrap_or_else(|e| panic!("{}: {e}", k.name));
            let got = serde_json::to_value(&style).unwrap();
            assert!(close(&got, &default), "{} default {} differs from CaptionStyle::default", k.name, k.default);
        }
    }

    #[test]
    fn every_key_and_alias_has_a_setter() {
        for name in caption_key_names() {
            let k = caption_key(name).unwrap();
            let v = value_of(k).unwrap_or_else(|| match k.kind {
                KeyKind::Platform => CaptionValue::Text("tiktok".into()),
                KeyKind::Words => CaptionValue::List(vec!["billion".into()]),
                KeyKind::Color => CaptionValue::Text("#FF0000".into()),
                _ => CaptionValue::Number(0.04),
            });
            set_caption_key(&mut CaptionStyle::default(), name, &v, 1.0, 1920.0).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }

    #[test]
    fn every_preset_applies_and_scales_with_the_frame() {
        for p in CAPTION_PRESETS {
            let mut tall = CaptionStyle::default();
            apply_caption_preset(&mut tall, p.name, 1920.0).unwrap();
            let mut small = CaptionStyle::default();
            apply_caption_preset(&mut small, p.name, 960.0).unwrap();
            assert!((small.font_size * 2.0 - tall.font_size).abs() < 1e-3, "{}", p.name);
        }
        assert!(apply_caption_preset(&mut CaptionStyle::default(), "neon", 1920.0).unwrap_err().contains("bold-outline"));
    }

    #[test]
    fn size_is_a_fraction_of_the_frame_and_excludes_font_size() {
        let mut style = CaptionStyle::default();
        apply_caption_entries(&mut style, None, &[("size", CaptionValue::Number(0.05))], 1920.0).unwrap();
        assert!((style.font_size - 96.0).abs() < 1e-3);
        let both = [("size", CaptionValue::Number(0.05)), ("font_size", CaptionValue::Number(40.0))];
        assert!(apply_caption_entries(&mut style, None, &both, 1920.0).is_err());
    }

    #[test]
    fn explicit_keys_override_the_preset() {
        let mut style = CaptionStyle::default();
        let keys = [("highlight_color", CaptionValue::Text("#00FFFF".into()))];
        apply_caption_entries(&mut style, Some("bold-outline"), &keys, 1920.0).unwrap();
        assert!(style.uppercase);
        assert_eq!(style.highlight_color, parse_hex_color("#00FFFF").unwrap());
    }

    #[test]
    fn the_checked_in_reference_page_matches_the_table() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/reference/captions.md");
        let page = std::fs::read_to_string(path).expect("docs/reference/captions.md").replace("\r\n", "\n");
        assert!(
            page == caption_reference_markdown(),
            "docs/reference/captions.md is stale: run `director-engine captions keys --format markdown > docs/reference/captions.md`"
        );
    }
}
