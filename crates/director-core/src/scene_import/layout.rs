//! The document's `LayoutStyle` into a Taffy `Style`.

use super::ImportError;
use director_scene::layout as doc;
use taffy::geometry::{Line, Rect, Size};
use taffy::style::{self as tf};

/// The Taffy style a document layout records.
pub fn import_layout(style: &doc::LayoutStyle) -> Result<tf::Style, ImportError> {
    Ok(tf::Style {
        display: match style.display {
            doc::Display::Block => tf::Display::Block,
            doc::Display::Flex => tf::Display::Flex,
            doc::Display::Grid => tf::Display::Grid,
            doc::Display::None => tf::Display::None,
        },
        item_is_table: style.item_is_table,
        item_is_replaced: style.item_is_replaced,
        box_sizing: match style.box_sizing {
            doc::BoxSizing::BorderBox => tf::BoxSizing::BorderBox,
            doc::BoxSizing::ContentBox => tf::BoxSizing::ContentBox,
        },
        overflow: taffy::geometry::Point {
            x: overflow(style.overflow.x),
            y: overflow(style.overflow.y),
        },
        scrollbar_width: style.scrollbar_width,
        position: match style.position {
            doc::Position::Relative => tf::Position::Relative,
            doc::Position::Absolute => tf::Position::Absolute,
        },
        inset: edges(&style.inset, length_auto),
        size: size(&style.size, dimension),
        min_size: size(&style.min_size, dimension),
        max_size: size(&style.max_size, dimension),
        aspect_ratio: style.aspect_ratio,
        margin: edges(&style.margin, length_auto),
        padding: edges(&style.padding, length),
        border: edges(&style.border, length),
        align_items: style.align_items.map(align_items),
        align_self: style.align_self.map(align_items),
        justify_items: style.justify_items.map(align_items),
        justify_self: style.justify_self.map(align_items),
        align_content: style.align_content.map(align_content),
        justify_content: style.justify_content.map(align_content),
        gap: size(&style.gap, length),
        text_align: match style.text_align {
            doc::BlockTextAlign::Auto => tf::TextAlign::Auto,
            doc::BlockTextAlign::LegacyLeft => tf::TextAlign::LegacyLeft,
            doc::BlockTextAlign::LegacyRight => tf::TextAlign::LegacyRight,
            doc::BlockTextAlign::LegacyCenter => tf::TextAlign::LegacyCenter,
        },
        flex_direction: match style.flex_direction {
            doc::FlexDirection::Row => tf::FlexDirection::Row,
            doc::FlexDirection::Column => tf::FlexDirection::Column,
            doc::FlexDirection::RowReverse => tf::FlexDirection::RowReverse,
            doc::FlexDirection::ColumnReverse => tf::FlexDirection::ColumnReverse,
        },
        flex_wrap: match style.flex_wrap {
            doc::FlexWrap::NoWrap => tf::FlexWrap::NoWrap,
            doc::FlexWrap::Wrap => tf::FlexWrap::Wrap,
            doc::FlexWrap::WrapReverse => tf::FlexWrap::WrapReverse,
        },
        flex_basis: dimension(style.flex_basis),
        flex_grow: style.flex_grow,
        flex_shrink: style.flex_shrink,
        grid_template_rows: style.grid_template_rows.iter().map(template_component).collect(),
        grid_template_columns: style.grid_template_columns.iter().map(template_component).collect(),
        grid_auto_rows: style.grid_auto_rows.iter().map(track_size).collect(),
        grid_auto_columns: style.grid_auto_columns.iter().map(track_size).collect(),
        grid_auto_flow: match style.grid_auto_flow {
            doc::GridAutoFlow::Row => tf::GridAutoFlow::Row,
            doc::GridAutoFlow::Column => tf::GridAutoFlow::Column,
            doc::GridAutoFlow::RowDense => tf::GridAutoFlow::RowDense,
            doc::GridAutoFlow::ColumnDense => tf::GridAutoFlow::ColumnDense,
        },
        grid_template_areas: style
            .grid_template_areas
            .iter()
            .map(|area| tf::GridTemplateArea {
                name: area.name.clone(),
                row_start: area.row_start,
                row_end: area.row_end,
                column_start: area.column_start,
                column_end: area.column_end,
            })
            .collect(),
        grid_template_column_names: style.grid_template_column_names.clone(),
        grid_template_row_names: style.grid_template_row_names.clone(),
        grid_row: Line {
            start: placement(&style.grid_row.start),
            end: placement(&style.grid_row.end),
        },
        grid_column: Line {
            start: placement(&style.grid_column.start),
            end: placement(&style.grid_column.end),
        },
        ..tf::Style::DEFAULT
    })
}

fn overflow(value: doc::Overflow) -> tf::Overflow {
    match value {
        doc::Overflow::Visible => tf::Overflow::Visible,
        doc::Overflow::Clip => tf::Overflow::Clip,
        doc::Overflow::Hidden => tf::Overflow::Hidden,
        doc::Overflow::Scroll => tf::Overflow::Scroll,
    }
}

fn edges<T: Copy, U>(value: &doc::Edges<T>, convert: impl Fn(T) -> U) -> Rect<U> {
    Rect {
        left: convert(value.left),
        right: convert(value.right),
        top: convert(value.top),
        bottom: convert(value.bottom),
    }
}

fn size<T: Copy, U>(value: &doc::Size<T>, convert: impl Fn(T) -> U) -> Size<U> {
    Size {
        width: convert(value.width),
        height: convert(value.height),
    }
}

fn dimension(value: doc::Dimension) -> tf::Dimension {
    match value {
        doc::Dimension::Auto => tf::Dimension::auto(),
        doc::Dimension::Px(px) => tf::Dimension::length(px),
        doc::Dimension::Percent(fraction) => tf::Dimension::percent(fraction),
    }
}

fn length_auto(value: doc::Dimension) -> tf::LengthPercentageAuto {
    match value {
        doc::Dimension::Auto => tf::LengthPercentageAuto::auto(),
        doc::Dimension::Px(px) => tf::LengthPercentageAuto::length(px),
        doc::Dimension::Percent(fraction) => tf::LengthPercentageAuto::percent(fraction),
    }
}

/// A Taffy px or percent length.
pub fn length(value: doc::Length) -> tf::LengthPercentage {
    match value {
        doc::Length::Px(px) => tf::LengthPercentage::length(px),
        doc::Length::Percent(fraction) => tf::LengthPercentage::percent(fraction),
    }
}

fn align_items(value: doc::AlignItems) -> tf::AlignItems {
    match value {
        doc::AlignItems::Start => tf::AlignItems::Start,
        doc::AlignItems::End => tf::AlignItems::End,
        doc::AlignItems::FlexStart => tf::AlignItems::FlexStart,
        doc::AlignItems::FlexEnd => tf::AlignItems::FlexEnd,
        doc::AlignItems::Center => tf::AlignItems::Center,
        doc::AlignItems::Baseline => tf::AlignItems::Baseline,
        doc::AlignItems::Stretch => tf::AlignItems::Stretch,
    }
}

fn align_content(value: doc::AlignContent) -> tf::AlignContent {
    match value {
        doc::AlignContent::Start => tf::AlignContent::Start,
        doc::AlignContent::End => tf::AlignContent::End,
        doc::AlignContent::FlexStart => tf::AlignContent::FlexStart,
        doc::AlignContent::FlexEnd => tf::AlignContent::FlexEnd,
        doc::AlignContent::Center => tf::AlignContent::Center,
        doc::AlignContent::Stretch => tf::AlignContent::Stretch,
        doc::AlignContent::SpaceBetween => tf::AlignContent::SpaceBetween,
        doc::AlignContent::SpaceEvenly => tf::AlignContent::SpaceEvenly,
        doc::AlignContent::SpaceAround => tf::AlignContent::SpaceAround,
    }
}

fn template_component(component: &doc::GridTemplateComponent) -> tf::GridTemplateComponent<String> {
    match component {
        doc::GridTemplateComponent::Single(track) => tf::GridTemplateComponent::Single(track_size(track)),
        doc::GridTemplateComponent::Repeat {
            count,
            tracks,
            line_names,
        } => tf::GridTemplateComponent::Repeat(tf::GridTemplateRepetition {
            count: match count {
                doc::Repetition::AutoFill => tf::RepetitionCount::AutoFill,
                doc::Repetition::AutoFit => tf::RepetitionCount::AutoFit,
                doc::Repetition::Count(count) => tf::RepetitionCount::Count(*count),
            },
            tracks: tracks.iter().map(track_size).collect(),
            line_names: line_names.clone(),
        }),
    }
}

fn track_size(track: &doc::TrackSize) -> tf::TrackSizingFunction {
    tf::TrackSizingFunction {
        min: match track.min {
            doc::TrackMin::Auto => tf::MinTrackSizingFunction::auto(),
            doc::TrackMin::MinContent => tf::MinTrackSizingFunction::min_content(),
            doc::TrackMin::MaxContent => tf::MinTrackSizingFunction::max_content(),
            doc::TrackMin::Px(px) => tf::MinTrackSizingFunction::length(px),
            doc::TrackMin::Percent(fraction) => tf::MinTrackSizingFunction::percent(fraction),
        },
        max: match track.max {
            doc::TrackMax::Auto => tf::MaxTrackSizingFunction::auto(),
            doc::TrackMax::MinContent => tf::MaxTrackSizingFunction::min_content(),
            doc::TrackMax::MaxContent => tf::MaxTrackSizingFunction::max_content(),
            doc::TrackMax::Px(px) => tf::MaxTrackSizingFunction::length(px),
            doc::TrackMax::Percent(fraction) => tf::MaxTrackSizingFunction::percent(fraction),
            doc::TrackMax::FitContentPx(px) => tf::MaxTrackSizingFunction::fit_content_px(px),
            doc::TrackMax::FitContentPercent(fraction) => tf::MaxTrackSizingFunction::fit_content_percent(fraction),
            doc::TrackMax::Fr(fr) => tf::MaxTrackSizingFunction::fr(fr),
        },
    }
}

fn placement(value: &doc::GridPlacement) -> tf::GridPlacement<String> {
    match value {
        doc::GridPlacement::Auto => tf::GridPlacement::Auto,
        doc::GridPlacement::Line(line) => tf::GridPlacement::Line((*line).into()),
        doc::GridPlacement::NamedLine { name, index } => tf::GridPlacement::NamedLine(name.clone(), *index),
        doc::GridPlacement::Span(span) => tf::GridPlacement::Span(*span),
        doc::GridPlacement::NamedSpan { name, count } => tf::GridPlacement::NamedSpan(name.clone(), *count),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_export::export_layout;

    #[test]
    fn the_default_layout_is_the_default_taffy_style() {
        assert_eq!(import_layout(&doc::LayoutStyle::default()).unwrap(), tf::Style::DEFAULT);
    }

    #[test]
    fn a_grid_layout_round_trips() {
        let style = tf::Style {
            display: tf::Display::Grid,
            size: Size {
                width: tf::Dimension::length(320.0),
                height: tf::Dimension::percent(0.5),
            },
            margin: Rect {
                left: tf::LengthPercentageAuto::auto(),
                right: tf::LengthPercentageAuto::length(4.0),
                top: tf::LengthPercentageAuto::percent(0.1),
                bottom: tf::LengthPercentageAuto::length(0.0),
            },
            grid_template_columns: vec![
                tf::GridTemplateComponent::Single(tf::TrackSizingFunction {
                    min: tf::MinTrackSizingFunction::min_content(),
                    max: tf::MaxTrackSizingFunction::fr(1.0),
                }),
                tf::GridTemplateComponent::Repeat(tf::GridTemplateRepetition {
                    count: tf::RepetitionCount::AutoFill,
                    tracks: vec![tf::TrackSizingFunction {
                        min: tf::MinTrackSizingFunction::length(40.0),
                        max: tf::MaxTrackSizingFunction::fit_content_px(80.0),
                    }],
                    line_names: vec![vec!["a".to_string()], vec![]],
                }),
            ],
            grid_column: Line {
                start: tf::GridPlacement::NamedLine("a".to_string(), 1),
                end: tf::GridPlacement::Span(2),
            },
            ..tf::Style::DEFAULT
        };
        let exported = export_layout(&style).unwrap();
        assert_eq!(import_layout(&exported).unwrap(), style);
    }
}
