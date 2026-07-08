//! Text direction helpers for SkParagraph (LTR / RTL / auto-detect).

use skia_safe::textlayout::TextDirection;

/// How to resolve paragraph text direction before layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextDirectionMode {
    /// Infer from the first strong bidi character in the content.
    #[default]
    Auto,
    /// Force left-to-right layout.
    Ltr,
    /// Force right-to-left layout.
    Rtl,
}

impl TextDirectionMode {
    pub fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "rtl" | "right" | "right_to_left" | "right-to-left" => Self::Rtl,
            "ltr" | "left" | "left_to_right" | "left-to-right" => Self::Ltr,
            _ => Self::Auto,
        }
    }
}

/// Parse an optional Rhai/script `text_direction` value.
pub fn parse_text_direction_mode(value: &str) -> TextDirectionMode {
    TextDirectionMode::from_str(value)
}

/// Detect paragraph direction from content using the Unicode first-strong rule.
pub fn detect_text_direction(text: &str) -> TextDirection {
    for ch in text.chars() {
        if let Some(dir) = strong_bidi_direction(ch) {
            return dir;
        }
    }
    TextDirection::LTR
}

/// Resolve the Skia paragraph direction for a mode + content string.
pub fn resolve_text_direction(mode: TextDirectionMode, text: &str) -> TextDirection {
    match mode {
        TextDirectionMode::Ltr => TextDirection::LTR,
        TextDirectionMode::Rtl => TextDirection::RTL,
        TextDirectionMode::Auto => detect_text_direction(text),
    }
}

/// Returns `Some(LTR|RTL)` for characters with a strong bidi class.
fn strong_bidi_direction(ch: char) -> Option<TextDirection> {
    match ch {
        '\u{200E}' => Some(TextDirection::LTR), // LRM
        '\u{200F}' | '\u{061C}' => Some(TextDirection::RTL), // RLM / ALM
        '\u{0590}'..='\u{05FF}' => Some(TextDirection::RTL), // Hebrew
        '\u{0600}'..='\u{06FF}' => Some(TextDirection::RTL), // Arabic
        '\u{0700}'..='\u{074F}' => Some(TextDirection::RTL), // Syriac
        '\u{0750}'..='\u{077F}' => Some(TextDirection::RTL), // Arabic Supplement
        '\u{07C0}'..='\u{07FF}' => Some(TextDirection::RTL), // NKo / Samaritan / Mandaic
        '\u{08A0}'..='\u{08FF}' => Some(TextDirection::RTL), // Arabic Extended-A
        '\u{FB1D}'..='\u{FB4F}' => Some(TextDirection::RTL), // Hebrew Presentation Forms
        '\u{FB50}'..='\u{FDFF}' => Some(TextDirection::RTL), // Arabic Presentation Forms-A
        '\u{FE70}'..='\u{FEFF}' => Some(TextDirection::RTL), // Arabic Presentation Forms-B
        '\u{10800}'..='\u{10FFF}' => Some(TextDirection::RTL), // Other RTL scripts
        'A'..='Z' | 'a'..='z' => Some(TextDirection::LTR),
        '\u{00C0}'..='\u{02FF}' => Some(TextDirection::LTR), // Latin-1 / Latin Extended
        '\u{0370}'..='\u{03FF}' => Some(TextDirection::LTR), // Greek
        '\u{0400}'..='\u{04FF}' => Some(TextDirection::LTR), // Cyrillic
        '\u{3040}'..='\u{30FF}' => Some(TextDirection::LTR), // Hiragana / Katakana
        '\u{4E00}'..='\u{9FFF}' => Some(TextDirection::LTR), // CJK
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_ltr_from_english() {
        assert_eq!(detect_text_direction("Hello world"), TextDirection::LTR);
    }

    #[test]
    fn detect_rtl_from_arabic() {
        assert_eq!(detect_text_direction("مرحبا"), TextDirection::RTL);
    }

    #[test]
    fn detect_rtl_from_hebrew() {
        assert_eq!(detect_text_direction("שלום"), TextDirection::RTL);
    }

    #[test]
    fn detect_skips_leading_neutral() {
        assert_eq!(detect_text_direction("  \n\tمرحبا"), TextDirection::RTL);
        assert_eq!(detect_text_direction("123 Hello"), TextDirection::LTR);
    }

    #[test]
    fn resolve_mode_overrides_detection() {
        assert_eq!(
            resolve_text_direction(TextDirectionMode::Rtl, "Hello"),
            TextDirection::RTL
        );
        assert_eq!(
            resolve_text_direction(TextDirectionMode::Ltr, "مرحبا"),
            TextDirection::LTR
        );
    }

    #[test]
    fn parse_text_direction_keywords() {
        assert_eq!(parse_text_direction_mode("rtl"), TextDirectionMode::Rtl);
        assert_eq!(parse_text_direction_mode("LTR"), TextDirectionMode::Ltr);
        assert_eq!(parse_text_direction_mode("auto"), TextDirectionMode::Auto);
    }
}