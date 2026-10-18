//! CSS text-decoration line, style, and thickness values.

use crate::decoration_marks::DecorationStyle;
use crate::style::{length, LengthParts, Metrics};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DecorationThickness {
    Auto,
    Px(f32),
    FromFont,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecorationPaint {
    pub underline: bool,
    pub overline: bool,
    pub line_through: bool,
    pub style: DecorationStyle,
    pub thickness: DecorationThickness,
}

pub fn parse_lines(source: &str) -> Result<DecorationPaint, String> {
    if source.trim() == "none" {
        return Ok(blank());
    }
    let mut paint = blank();
    let mut any = false;
    for token in source.split_whitespace() {
        match token {
            "underline" => paint.underline = true,
            "overline" => paint.overline = true,
            "line-through" => paint.line_through = true,
            _ => return Err(format!("unsupported text decoration '{token}'")),
        }
        any = true;
    }
    if !any {
        return Err(format!("unsupported text decoration '{source}'"));
    }
    Ok(paint)
}

pub fn parse_style(source: &str) -> Result<DecorationStyle, String> {
    match source.trim() {
        "solid" => Ok(DecorationStyle::Solid),
        "double" => Ok(DecorationStyle::Double),
        "dotted" => Ok(DecorationStyle::Dotted),
        "dashed" => Ok(DecorationStyle::Dashed),
        "wavy" => Ok(DecorationStyle::Wavy),
        _ => Err(format!("unsupported text decoration style '{source}'")),
    }
}

pub fn parse_thickness(source: &str, metrics: Metrics) -> Result<DecorationThickness, String> {
    let trimmed = source.trim();
    if trimmed == "auto" {
        return Ok(DecorationThickness::Auto);
    }
    if trimmed == "from-font" {
        return Ok(DecorationThickness::FromFont);
    }
    let px = resolved_px(trimmed, metrics)?;
    if !px.is_finite() {
        return Err(format!("unsupported text decoration thickness '{source}'"));
    }
    Ok(DecorationThickness::Px(px))
}

pub fn parse_shorthand(source: &str, metrics: Metrics) -> Result<DecorationPaint, String> {
    if source.trim() == "none" {
        return Ok(blank());
    }
    let mut paint = blank();
    let mut style_set = false;
    let mut thickness_set = false;
    for token in source.split_whitespace() {
        match token {
            "underline" => paint.underline = true,
            "overline" => paint.overline = true,
            "line-through" => paint.line_through = true,
            "solid" | "double" | "dotted" | "dashed" | "wavy" => {
                let style = parse_style(token)?;
                if style_set && paint.style != style {
                    return Err(format!("unsupported text decoration style '{token}'"));
                }
                paint.style = style;
                style_set = true;
            }
            "auto" => {
                if thickness_set {
                    return Err(format!("unsupported text decoration thickness '{token}'"));
                }
                paint.thickness = DecorationThickness::Auto;
                thickness_set = true;
            }
            "from-font" => {
                if thickness_set {
                    return Err(format!("unsupported text decoration thickness '{token}'"));
                }
                paint.thickness = DecorationThickness::FromFont;
                thickness_set = true;
            }
            "none" => return Err(format!("unsupported text decoration 'none'")),
            "currentcolor" | "currentColor" => {}
            other => match resolved_px(other, metrics) {
                Ok(px) if px.is_finite() => {
                    if thickness_set {
                        return Err(format!("unsupported text decoration thickness '{other}'"));
                    }
                    paint.thickness = DecorationThickness::Px(px);
                    thickness_set = true;
                }
                Ok(_) => {
                    return Err(format!("unsupported text decoration thickness '{other}'"));
                }
                Err(_) => return Err(format!("unsupported text decoration '{other}'")),
            },
        }
    }
    Ok(paint)
}

fn blank() -> DecorationPaint {
    DecorationPaint {
        underline: false,
        overline: false,
        line_through: false,
        style: DecorationStyle::Solid,
        thickness: DecorationThickness::Auto,
    }
}

fn resolved_px(token: &str, metrics: Metrics) -> Result<f32, String> {
    let parsed = length(token, metrics)?;
    Ok(if parsed.is_percent() {
        parsed.value() * metrics.font_size
    } else {
        parsed.value()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics() -> Metrics {
        Metrics {
            width: 100.0,
            height: 40.0,
            font_size: 16.0,
        }
    }

    #[test]
    fn lines_underline_overline() {
        assert_eq!(
            parse_lines("underline overline").unwrap(),
            DecorationPaint {
                underline: true,
                overline: true,
                line_through: false,
                style: DecorationStyle::Solid,
                thickness: DecorationThickness::Auto,
            }
        );
    }

    #[test]
    fn lines_none() {
        let paint = parse_lines("none").unwrap();
        assert!(!paint.underline);
        assert!(!paint.overline);
        assert!(!paint.line_through);
    }

    #[test]
    fn lines_wavy() {
        assert!(parse_lines("wavy").is_err());
    }

    #[test]
    fn style_dotted_and_groove() {
        assert_eq!(parse_style("dotted").unwrap(), DecorationStyle::Dotted);
        let err = parse_style("groove").unwrap_err();
        assert!(err.contains("text decoration style"), "{err}");
    }

    #[test]
    fn thickness_auto_px_percent_and_from_font() {
        let metrics = metrics();
        assert_eq!(
            parse_thickness("auto", metrics).unwrap(),
            DecorationThickness::Auto
        );
        assert_eq!(
            parse_thickness("2px", metrics).unwrap(),
            DecorationThickness::Px(2.0)
        );
        assert_eq!(
            parse_thickness("50%", metrics).unwrap(),
            DecorationThickness::Px(8.0)
        );
        assert_eq!(
            parse_thickness("from-font", metrics).unwrap(),
            DecorationThickness::FromFont
        );
    }

    #[test]
    fn shorthand_underline_double_3px() {
        assert_eq!(
            parse_shorthand("underline double 3px", metrics()).unwrap(),
            DecorationPaint {
                underline: true,
                overline: false,
                line_through: false,
                style: DecorationStyle::Double,
                thickness: DecorationThickness::Px(3.0),
            }
        );
    }

    #[test]
    fn shorthand_line_through_dotted() {
        assert_eq!(
            parse_shorthand("line-through dotted", metrics()).unwrap(),
            DecorationPaint {
                underline: false,
                overline: false,
                line_through: true,
                style: DecorationStyle::Dotted,
                thickness: DecorationThickness::Auto,
            }
        );
    }

    #[test]
    fn shorthand_double() {
        let paint = parse_shorthand("double", metrics()).unwrap();
        assert!(!paint.underline);
        assert!(!paint.overline);
        assert!(!paint.line_through);
        assert_eq!(paint.style, DecorationStyle::Double);
    }

    #[test]
    fn shorthand_underline_red() {
        assert!(parse_shorthand("underline red", metrics()).is_err());
    }

    #[test]
    fn shorthand_none_underline() {
        assert!(parse_shorthand("none underline", metrics()).is_err());
    }
}
