//! Typed layout longhands. Each arm reads a parsed Lightning CSS value and
//! writes the Taffy style, so no layout value round-trips through text.

use super::{convert_length_with_basis, length_dimension, ContainingBlock, LengthParts, Metrics};
use crate::computed::Computed;
use lightningcss::{
    properties::{
        align::{
            AlignContent, AlignItems as CssAlignItems, AlignSelf, BaselinePosition,
            ContentDistribution, ContentPosition, JustifyContent as CssJustifyContent,
            JustifyItems, JustifySelf, SelfPosition,
        },
        display::{Display as CssDisplay, DisplayInside, DisplayKeyword, DisplayOutside},
        flex::{FlexDirection as CssFlexDirection, FlexWrap as CssFlexWrap},
        grid::GridAutoFlow as CssGridAutoFlow,
        position::Position as CssPosition,
        size::{BoxSizing as CssBoxSizing, MaxSize, Size},
        Property,
    },
    values::length::{LengthPercentage as CssLength, LengthPercentageOrAuto},
};
use taffy::prelude::*;

/// The longhands this module owns. Text Lightning CSS cannot parse for one of
/// them fails here with that property's message.
const TYPED: &[&str] = &[
    "align-content",
    "align-items",
    "align-self",
    "aspect-ratio",
    "bottom",
    "box-sizing",
    "display",
    "flex-basis",
    "flex-direction",
    "flex-grow",
    "flex-shrink",
    "flex-wrap",
    "grid-auto-flow",
    "height",
    "justify-content",
    "justify-items",
    "justify-self",
    "left",
    "margin-bottom",
    "margin-left",
    "margin-right",
    "margin-top",
    "max-height",
    "max-width",
    "min-height",
    "min-width",
    "padding-bottom",
    "padding-left",
    "padding-right",
    "padding-top",
    "position",
    "right",
    "top",
    "width",
];

/// Apply one computed longhand to `style`. `None` leaves the property to the
/// text path in `translate`.
pub(super) fn apply(
    name: &str,
    entry: &Computed,
    style: &mut Style,
    m: Metrics,
    block: ContainingBlock,
) -> Option<Result<(), String>> {
    if !TYPED.contains(&name) {
        return None;
    }
    let s = entry.text.as_str();
    let length = |value: &CssLength, basis: Option<f32>| convert_length_with_basis(value, m, basis);
    let result = match &entry.value {
        Property::Display(display) => display_of(display, s).map(|d| style.display = d),
        Property::BoxSizing(sizing, _) => {
            style.box_sizing = match sizing {
                CssBoxSizing::BorderBox => BoxSizing::BorderBox,
                CssBoxSizing::ContentBox => BoxSizing::ContentBox,
            };
            Ok(())
        }
        Property::Position(position) => {
            style.position = match position {
                CssPosition::Static | CssPosition::Relative | CssPosition::Sticky(_) => {
                    Position::Relative
                }
                CssPosition::Absolute | CssPosition::Fixed => Position::Absolute,
            };
            Ok(())
        }
        Property::Width(size) => size_of(size, s, |v| length(v, block.width)).map(|d| style.size.width = d),
        Property::Height(size) => {
            size_of(size, s, |v| length(v, block.height)).map(|d| style.size.height = d)
        }
        Property::MinWidth(size) => {
            size_of(size, s, |v| length(v, block.width)).map(|d| style.min_size.width = d)
        }
        Property::MinHeight(size) => {
            size_of(size, s, |v| length(v, block.height)).map(|d| style.min_size.height = d)
        }
        Property::MaxWidth(size) => {
            max_size_of(size, s, |v| length(v, block.width)).map(|d| style.max_size.width = d)
        }
        Property::MaxHeight(size) => {
            max_size_of(size, s, |v| length(v, block.height)).map(|d| style.max_size.height = d)
        }
        Property::Left(value) => auto_of(value, |v| length(v, block.width)).map(|v| style.inset.left = v),
        Property::Right(value) => {
            auto_of(value, |v| length(v, block.width)).map(|v| style.inset.right = v)
        }
        Property::Top(value) => auto_of(value, |v| length(v, block.height)).map(|v| style.inset.top = v),
        Property::Bottom(value) => {
            auto_of(value, |v| length(v, block.height)).map(|v| style.inset.bottom = v)
        }
        // Margins and paddings resolve percentages against the containing block width.
        Property::MarginLeft(value) => {
            auto_of(value, |v| length(v, block.width)).map(|v| style.margin.left = v)
        }
        Property::MarginRight(value) => {
            auto_of(value, |v| length(v, block.width)).map(|v| style.margin.right = v)
        }
        Property::MarginTop(value) => {
            auto_of(value, |v| length(v, block.width)).map(|v| style.margin.top = v)
        }
        Property::MarginBottom(value) => {
            auto_of(value, |v| length(v, block.width)).map(|v| style.margin.bottom = v)
        }
        Property::PaddingLeft(value) => {
            padding_of(value, s, |v| length(v, block.width)).map(|v| style.padding.left = v)
        }
        Property::PaddingRight(value) => {
            padding_of(value, s, |v| length(v, block.width)).map(|v| style.padding.right = v)
        }
        Property::PaddingTop(value) => {
            padding_of(value, s, |v| length(v, block.width)).map(|v| style.padding.top = v)
        }
        Property::PaddingBottom(value) => {
            padding_of(value, s, |v| length(v, block.width)).map(|v| style.padding.bottom = v)
        }
        Property::FlexDirection(direction, _) => {
            style.flex_direction = match direction {
                CssFlexDirection::Row => FlexDirection::Row,
                CssFlexDirection::Column => FlexDirection::Column,
                CssFlexDirection::RowReverse => FlexDirection::RowReverse,
                CssFlexDirection::ColumnReverse => FlexDirection::ColumnReverse,
            };
            Ok(())
        }
        Property::FlexWrap(wrap, _) => {
            style.flex_wrap = match wrap {
                CssFlexWrap::NoWrap => FlexWrap::NoWrap,
                CssFlexWrap::Wrap => FlexWrap::Wrap,
                CssFlexWrap::WrapReverse => FlexWrap::WrapReverse,
            };
            Ok(())
        }
        Property::FlexGrow(n, _) => finite(*n, s).map(|n| style.flex_grow = n),
        Property::FlexShrink(n, _) => finite(*n, s).map(|n| style.flex_shrink = n),
        Property::FlexBasis(value, _) => match value {
            LengthPercentageOrAuto::Auto => {
                style.flex_basis = auto();
                Ok(())
            }
            LengthPercentageOrAuto::LengthPercentage(v) => {
                length(v, block.width).map(|v| style.flex_basis = dimension_from(v))
            }
        },
        Property::AlignItems(value, _) => align_items(value, s).map(|a| style.align_items = Some(a)),
        Property::AlignSelf(value, _) => align_self(value, s).map(|a| style.align_self = a),
        Property::JustifyItems(value) => {
            justify_items(value, s).map(|a| style.justify_items = Some(a))
        }
        Property::JustifySelf(value) => justify_self(value, s).map(|a| style.justify_self = a),
        Property::JustifyContent(value, _) => {
            justify_content(value, s).map(|c| style.justify_content = Some(c))
        }
        Property::AlignContent(value, _) => {
            align_content(value, s).map(|c| style.align_content = Some(c))
        }
        Property::AspectRatio(ratio) => match &ratio.ratio {
            Some(ratio) => finite(ratio.0, s)
                .and_then(|w| finite(ratio.1, s).map(|h| w / h))
                .map(|r| style.aspect_ratio = Some(r)),
            None => Ok(()),
        },
        Property::GridAutoFlow(flow) => {
            let dense = flow.contains(CssGridAutoFlow::Dense);
            style.grid_auto_flow = match (flow.contains(CssGridAutoFlow::Column), dense) {
                (false, false) => GridAutoFlow::Row,
                (true, false) => GridAutoFlow::Column,
                (false, true) => GridAutoFlow::RowDense,
                (true, true) => GridAutoFlow::ColumnDense,
            };
            Ok(())
        }
        _ => Err(invalid(name, s)),
    };
    Some(result)
}

/// The message for text that does not parse as `name`.
fn invalid(name: &str, s: &str) -> String {
    match name {
        "display" => format!("unsupported display '{s}'"),
        "position" => format!("unsupported position '{s}'"),
        "box-sizing" | "flex-direction" | "flex-wrap" | "grid-auto-flow" => {
            format!("invalid {name}")
        }
        "align-content" | "align-items" | "align-self" | "justify-content" | "justify-items"
        | "justify-self" => unsupported_alignment(s),
        "flex-grow" | "flex-shrink" | "aspect-ratio" => format!("expected number, got '{s}'"),
        _ => format!("length '{s}': invalid"),
    }
}

fn unsupported_alignment(s: &str) -> String {
    format!("unsupported alignment '{s}'")
}

fn finite(n: f32, s: &str) -> Result<f32, String> {
    if n.is_finite() {
        Ok(n)
    } else {
        Err(format!("non-finite number '{s}'"))
    }
}

fn display_of(display: &CssDisplay, s: &str) -> Result<Display, String> {
    match display {
        CssDisplay::Keyword(DisplayKeyword::None) => Ok(Display::None),
        CssDisplay::Pair(pair) if !pair.is_list_item => match (&pair.outside, &pair.inside) {
            (DisplayOutside::Block, DisplayInside::Flow) => Ok(Display::Block),
            (DisplayOutside::Block | DisplayOutside::Inline, DisplayInside::Flex(_)) => {
                Ok(Display::Flex)
            }
            (DisplayOutside::Block | DisplayOutside::Inline, DisplayInside::Grid) => {
                Ok(Display::Grid)
            }
            (DisplayOutside::Inline, DisplayInside::Flow) => {
                Err("inline formatting requires a rich text parent or extraction".into())
            }
            _ => Err(format!("unsupported display '{s}'")),
        },
        _ => Err(format!("unsupported display '{s}'")),
    }
}

fn dimension_from(v: LengthPercentage) -> Dimension {
    if v.is_percent() {
        percent(v.value())
    } else {
        length_dimension(v.value())
    }
}

fn size_of(
    size: &Size,
    s: &str,
    length: impl Fn(&CssLength) -> Result<LengthPercentage, String>,
) -> Result<Dimension, String> {
    match size {
        Size::Auto => Ok(auto()),
        Size::LengthPercentage(v) => length(v).map(dimension_from),
        _ => Err(format!("length '{s}': unsupported size")),
    }
}

/// `none` is no maximum.
fn max_size_of(
    size: &MaxSize,
    s: &str,
    length: impl Fn(&CssLength) -> Result<LengthPercentage, String>,
) -> Result<Dimension, String> {
    match size {
        MaxSize::None => Ok(auto()),
        MaxSize::LengthPercentage(v) => length(v).map(dimension_from),
        _ => Err(format!("length '{s}': unsupported size")),
    }
}

fn auto_of(
    value: &LengthPercentageOrAuto,
    length: impl Fn(&CssLength) -> Result<LengthPercentage, String>,
) -> Result<LengthPercentageAuto, String> {
    match value {
        LengthPercentageOrAuto::Auto => Ok(auto()),
        LengthPercentageOrAuto::LengthPercentage(v) => length(v).map(|v| {
            if v.is_percent() {
                percent(v.value())
            } else {
                LengthPercentageAuto::length(v.value())
            }
        }),
    }
}

/// Padding has no `auto`.
fn padding_of(
    value: &LengthPercentageOrAuto,
    s: &str,
    length: impl Fn(&CssLength) -> Result<LengthPercentage, String>,
) -> Result<LengthPercentage, String> {
    match value {
        LengthPercentageOrAuto::Auto => Err(format!("length '{s}': invalid")),
        LengthPercentageOrAuto::LengthPercentage(v) => length(v),
    }
}

fn self_position(value: &SelfPosition, s: &str) -> Result<AlignItems, String> {
    match value {
        SelfPosition::Start => Ok(AlignItems::Start),
        SelfPosition::End => Ok(AlignItems::End),
        SelfPosition::FlexStart => Ok(AlignItems::FlexStart),
        SelfPosition::FlexEnd => Ok(AlignItems::FlexEnd),
        SelfPosition::Center => Ok(AlignItems::Center),
        SelfPosition::SelfStart | SelfPosition::SelfEnd => Err(unsupported_alignment(s)),
    }
}

/// `normal` and `stretch` stretch. `last baseline` and overflow keywords are
/// refused.
fn align_items(value: &CssAlignItems, s: &str) -> Result<AlignItems, String> {
    match value {
        CssAlignItems::Normal | CssAlignItems::Stretch => Ok(AlignItems::Stretch),
        CssAlignItems::BaselinePosition(BaselinePosition::First) => {
            Ok(AlignItems::Baseline)
        }
        CssAlignItems::SelfPosition {
            overflow: None,
            value,
        } => self_position(value, s),
        _ => Err(unsupported_alignment(s)),
    }
}

/// `auto` defers to the parent's `align-items`.
fn align_self(value: &AlignSelf, s: &str) -> Result<Option<AlignItems>, String> {
    match value {
        AlignSelf::Auto => Ok(None),
        AlignSelf::Normal | AlignSelf::Stretch => Ok(Some(AlignItems::Stretch)),
        AlignSelf::BaselinePosition(BaselinePosition::First) => {
            Ok(Some(AlignItems::Baseline))
        }
        AlignSelf::SelfPosition {
            overflow: None,
            value,
        } => self_position(value, s).map(Some),
        _ => Err(unsupported_alignment(s)),
    }
}

fn justify_items(value: &JustifyItems, s: &str) -> Result<AlignItems, String> {
    match value {
        JustifyItems::Normal | JustifyItems::Stretch => Ok(AlignItems::Stretch),
        JustifyItems::BaselinePosition(BaselinePosition::First) => {
            Ok(AlignItems::Baseline)
        }
        JustifyItems::SelfPosition {
            overflow: None,
            value,
        } => self_position(value, s),
        _ => Err(unsupported_alignment(s)),
    }
}

fn justify_self(value: &JustifySelf, s: &str) -> Result<Option<AlignItems>, String> {
    match value {
        JustifySelf::Auto => Ok(None),
        JustifySelf::Normal | JustifySelf::Stretch => Ok(Some(AlignItems::Stretch)),
        JustifySelf::BaselinePosition(BaselinePosition::First) => {
            Ok(Some(AlignItems::Baseline))
        }
        JustifySelf::SelfPosition {
            overflow: None,
            value,
        } => self_position(value, s).map(Some),
        _ => Err(unsupported_alignment(s)),
    }
}

fn content_position(value: &ContentPosition) -> JustifyContent {
    match value {
        ContentPosition::Start => JustifyContent::Start,
        ContentPosition::End => JustifyContent::End,
        ContentPosition::FlexStart => JustifyContent::FlexStart,
        ContentPosition::FlexEnd => JustifyContent::FlexEnd,
        ContentPosition::Center => JustifyContent::Center,
    }
}

fn distribution(value: &ContentDistribution) -> JustifyContent {
    match value {
        ContentDistribution::SpaceBetween => JustifyContent::SpaceBetween,
        ContentDistribution::SpaceAround => JustifyContent::SpaceAround,
        ContentDistribution::SpaceEvenly => JustifyContent::SpaceEvenly,
        ContentDistribution::Stretch => JustifyContent::Stretch,
    }
}

fn justify_content(value: &CssJustifyContent, s: &str) -> Result<JustifyContent, String> {
    match value {
        CssJustifyContent::Normal => Ok(JustifyContent::Stretch),
        CssJustifyContent::ContentDistribution(value) => Ok(distribution(value)),
        CssJustifyContent::ContentPosition {
            overflow: None,
            value,
        } => Ok(content_position(value)),
        _ => Err(unsupported_alignment(s)),
    }
}

fn align_content(value: &AlignContent, s: &str) -> Result<JustifyContent, String> {
    match value {
        AlignContent::Normal => Ok(JustifyContent::Stretch),
        AlignContent::ContentDistribution(value) => Ok(distribution(value)),
        AlignContent::ContentPosition {
            overflow: None,
            value,
        } => Ok(content_position(value)),
        _ => Err(unsupported_alignment(s)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computed::ComputedStyle;

    fn applied(name: &str, text: &str) -> Result<Style, String> {
        let values = [(name.to_string(), text.to_string())].into();
        let computed = ComputedStyle::parse(&values);
        let mut style = Style::default();
        let m = Metrics {
            width: 200.0,
            height: 100.0,
            font_size: 10.0,
        };
        let block = ContainingBlock::from_axes(400.0, 300.0);
        apply(name, computed.get(name).unwrap(), &mut style, m, block)
            .expect("typed property")
            .map(|_| style)
    }

    #[test]
    fn display_maps_outer_and_inner_kinds() {
        assert_eq!(applied("display", "inline-flex").unwrap().display, Display::Flex);
        assert_eq!(applied("display", "grid").unwrap().display, Display::Grid);
        assert_eq!(applied("display", "none").unwrap().display, Display::None);
        assert!(applied("display", "inline").unwrap_err().contains("rich text"));
        assert_eq!(
            applied("display", "inline-block").unwrap_err(),
            "unsupported display 'inline-block'"
        );
    }

    #[test]
    fn lengths_resolve_against_the_containing_block() {
        assert_eq!(applied("width", "2em").unwrap().size.width, Dimension::length(20.0));
        assert_eq!(applied("height", "50%").unwrap().size.height, percent(0.5_f32));
        assert_eq!(
            applied("top", "calc(50% + 10px)").unwrap().inset.top,
            LengthPercentageAuto::length(160.0)
        );
        assert_eq!(applied("max-width", "none").unwrap().max_size.width, auto());
        assert!(applied("width", "min-content").is_err());
        assert!(applied("padding-left", "auto").is_err());
    }

    #[test]
    fn alignment_keeps_the_supported_keywords() {
        assert_eq!(
            applied("align-items", "first baseline").unwrap().align_items,
            Some(AlignItems::Baseline)
        );
        assert_eq!(applied("align-self", "auto").unwrap().align_self, None);
        assert_eq!(
            applied("justify-content", "space-evenly").unwrap().justify_content,
            Some(JustifyContent::SpaceEvenly)
        );
        assert!(applied("align-items", "safe center").is_err());
        assert!(applied("justify-self", "left").is_err());
        assert!(applied("align-content", "baseline").is_err());
    }

    #[test]
    fn flex_grid_and_ratio_values() {
        assert_eq!(applied("flex-grow", "2").unwrap().flex_grow, 2.0);
        assert_eq!(applied("flex-basis", "25%").unwrap().flex_basis, percent(0.25_f32));
        assert_eq!(
            applied("grid-auto-flow", "column dense").unwrap().grid_auto_flow,
            GridAutoFlow::ColumnDense
        );
        assert_eq!(applied("aspect-ratio", "16 / 8").unwrap().aspect_ratio, Some(2.0));
        assert_eq!(applied("aspect-ratio", "auto").unwrap().aspect_ratio, None);
        assert_eq!(applied("flex-wrap", "sideways").unwrap_err(), "invalid flex-wrap");
    }
}
