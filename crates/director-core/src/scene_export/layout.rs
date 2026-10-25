//! Taffy `Style` into the document's `LayoutStyle`.

use super::ExportError;
use director_scene::layout as doc;
use taffy::style::{self as tf, CompactLength};

pub(super) fn export_layout(style: &tf::Style) -> Result<doc::LayoutStyle, ExportError> {
    Ok(doc::LayoutStyle {
        display: match style.display {
            tf::Display::Block => doc::Display::Block,
            tf::Display::Flex => doc::Display::Flex,
            tf::Display::Grid => doc::Display::Grid,
            tf::Display::None => doc::Display::None,
        },
        item_is_table: style.item_is_table,
        item_is_replaced: style.item_is_replaced,
        box_sizing: match style.box_sizing {
            tf::BoxSizing::BorderBox => doc::BoxSizing::BorderBox,
            tf::BoxSizing::ContentBox => doc::BoxSizing::ContentBox,
        },
        overflow: doc::Axes {
            x: overflow(style.overflow.x),
            y: overflow(style.overflow.y),
        },
        scrollbar_width: style.scrollbar_width,
        position: match style.position {
            tf::Position::Relative => doc::Position::Relative,
            tf::Position::Absolute => doc::Position::Absolute,
        },
        inset: edges(&style.inset, |v| dimension(v.into_raw()))?,
        size: size(&style.size, |v| dimension(v.into_raw()))?,
        min_size: size(&style.min_size, |v| dimension(v.into_raw()))?,
        max_size: size(&style.max_size, |v| dimension(v.into_raw()))?,
        aspect_ratio: style.aspect_ratio,
        margin: edges(&style.margin, |v| dimension(v.into_raw()))?,
        padding: edges(&style.padding, |v| length(v.into_raw()))?,
        border: edges(&style.border, |v| length(v.into_raw()))?,
        align_items: style.align_items.map(align_items),
        align_self: style.align_self.map(align_items),
        justify_items: style.justify_items.map(align_items),
        justify_self: style.justify_self.map(align_items),
        align_content: style.align_content.map(align_content),
        justify_content: style.justify_content.map(align_content),
        gap: size(&style.gap, |v| length(v.into_raw()))?,
        text_align: match style.text_align {
            tf::TextAlign::Auto => doc::BlockTextAlign::Auto,
            tf::TextAlign::LegacyLeft => doc::BlockTextAlign::LegacyLeft,
            tf::TextAlign::LegacyRight => doc::BlockTextAlign::LegacyRight,
            tf::TextAlign::LegacyCenter => doc::BlockTextAlign::LegacyCenter,
        },
        flex_direction: match style.flex_direction {
            tf::FlexDirection::Row => doc::FlexDirection::Row,
            tf::FlexDirection::Column => doc::FlexDirection::Column,
            tf::FlexDirection::RowReverse => doc::FlexDirection::RowReverse,
            tf::FlexDirection::ColumnReverse => doc::FlexDirection::ColumnReverse,
        },
        flex_wrap: match style.flex_wrap {
            tf::FlexWrap::NoWrap => doc::FlexWrap::NoWrap,
            tf::FlexWrap::Wrap => doc::FlexWrap::Wrap,
            tf::FlexWrap::WrapReverse => doc::FlexWrap::WrapReverse,
        },
        flex_basis: dimension(style.flex_basis.into_raw())?,
        flex_grow: style.flex_grow,
        flex_shrink: style.flex_shrink,
        grid_template_rows: style
            .grid_template_rows
            .iter()
            .map(template_component)
            .collect::<Result<_, _>>()?,
        grid_template_columns: style
            .grid_template_columns
            .iter()
            .map(template_component)
            .collect::<Result<_, _>>()?,
        grid_auto_rows: style
            .grid_auto_rows
            .iter()
            .map(track_size)
            .collect::<Result<_, _>>()?,
        grid_auto_columns: style
            .grid_auto_columns
            .iter()
            .map(track_size)
            .collect::<Result<_, _>>()?,
        grid_auto_flow: match style.grid_auto_flow {
            tf::GridAutoFlow::Row => doc::GridAutoFlow::Row,
            tf::GridAutoFlow::Column => doc::GridAutoFlow::Column,
            tf::GridAutoFlow::RowDense => doc::GridAutoFlow::RowDense,
            tf::GridAutoFlow::ColumnDense => doc::GridAutoFlow::ColumnDense,
        },
        grid_template_areas: style
            .grid_template_areas
            .iter()
            .map(|area| doc::GridTemplateArea {
                name: area.name.to_string(),
                row_start: area.row_start,
                row_end: area.row_end,
                column_start: area.column_start,
                column_end: area.column_end,
            })
            .collect(),
        grid_template_column_names: line_names(&style.grid_template_column_names),
        grid_template_row_names: line_names(&style.grid_template_row_names),
        grid_row: doc::Line {
            start: placement(&style.grid_row.start),
            end: placement(&style.grid_row.end),
        },
        grid_column: doc::Line {
            start: placement(&style.grid_column.start),
            end: placement(&style.grid_column.end),
        },
    })
}

fn overflow(value: tf::Overflow) -> doc::Overflow {
    match value {
        tf::Overflow::Visible => doc::Overflow::Visible,
        tf::Overflow::Clip => doc::Overflow::Clip,
        tf::Overflow::Hidden => doc::Overflow::Hidden,
        tf::Overflow::Scroll => doc::Overflow::Scroll,
    }
}

fn edges<T: Copy, U>(
    rect: &taffy::geometry::Rect<T>,
    convert: impl Fn(T) -> Result<U, ExportError>,
) -> Result<doc::Edges<U>, ExportError> {
    Ok(doc::Edges {
        left: convert(rect.left)?,
        right: convert(rect.right)?,
        top: convert(rect.top)?,
        bottom: convert(rect.bottom)?,
    })
}

fn size<T: Copy, U>(
    value: &taffy::geometry::Size<T>,
    convert: impl Fn(T) -> Result<U, ExportError>,
) -> Result<doc::Size<U>, ExportError> {
    Ok(doc::Size {
        width: convert(value.width)?,
        height: convert(value.height)?,
    })
}

fn dimension(raw: CompactLength) -> Result<doc::Dimension, ExportError> {
    match raw.tag() {
        CompactLength::AUTO_TAG => Ok(doc::Dimension::Auto),
        CompactLength::LENGTH_TAG => Ok(doc::Dimension::Px(raw.value())),
        CompactLength::PERCENT_TAG => Ok(doc::Dimension::Percent(raw.value())),
        _ => Err(unrepresentable(raw)),
    }
}

fn length(raw: CompactLength) -> Result<doc::Length, ExportError> {
    match raw.tag() {
        CompactLength::LENGTH_TAG => Ok(doc::Length::Px(raw.value())),
        CompactLength::PERCENT_TAG => Ok(doc::Length::Percent(raw.value())),
        _ => Err(unrepresentable(raw)),
    }
}

fn unrepresentable(raw: CompactLength) -> ExportError {
    ExportError::Unrepresentable(format!(
        "layout length with tag {:#b}; calc() must resolve before export",
        raw.tag()
    ))
}

fn align_items(value: tf::AlignItems) -> doc::AlignItems {
    match value {
        tf::AlignItems::Start => doc::AlignItems::Start,
        tf::AlignItems::End => doc::AlignItems::End,
        tf::AlignItems::FlexStart => doc::AlignItems::FlexStart,
        tf::AlignItems::FlexEnd => doc::AlignItems::FlexEnd,
        tf::AlignItems::Center => doc::AlignItems::Center,
        tf::AlignItems::Baseline => doc::AlignItems::Baseline,
        tf::AlignItems::Stretch => doc::AlignItems::Stretch,
    }
}

fn align_content(value: tf::AlignContent) -> doc::AlignContent {
    match value {
        tf::AlignContent::Start => doc::AlignContent::Start,
        tf::AlignContent::End => doc::AlignContent::End,
        tf::AlignContent::FlexStart => doc::AlignContent::FlexStart,
        tf::AlignContent::FlexEnd => doc::AlignContent::FlexEnd,
        tf::AlignContent::Center => doc::AlignContent::Center,
        tf::AlignContent::Stretch => doc::AlignContent::Stretch,
        tf::AlignContent::SpaceBetween => doc::AlignContent::SpaceBetween,
        tf::AlignContent::SpaceEvenly => doc::AlignContent::SpaceEvenly,
        tf::AlignContent::SpaceAround => doc::AlignContent::SpaceAround,
    }
}

fn template_component(
    component: &tf::GridTemplateComponent<String>,
) -> Result<doc::GridTemplateComponent, ExportError> {
    Ok(match component {
        tf::GridTemplateComponent::Single(track) => doc::GridTemplateComponent::Single(track_size(track)?),
        tf::GridTemplateComponent::Repeat(repeat) => doc::GridTemplateComponent::Repeat {
            count: match repeat.count {
                tf::RepetitionCount::AutoFill => doc::Repetition::AutoFill,
                tf::RepetitionCount::AutoFit => doc::Repetition::AutoFit,
                tf::RepetitionCount::Count(count) => doc::Repetition::Count(count),
            },
            tracks: repeat.tracks.iter().map(track_size).collect::<Result<_, _>>()?,
            line_names: line_names(&repeat.line_names),
        },
    })
}

fn track_size(track: &tf::TrackSizingFunction) -> Result<doc::TrackSize, ExportError> {
    let min = track.min.into_raw();
    let max = track.max.into_raw();
    Ok(doc::TrackSize {
        min: match min.tag() {
            CompactLength::AUTO_TAG => doc::TrackMin::Auto,
            CompactLength::MIN_CONTENT_TAG => doc::TrackMin::MinContent,
            CompactLength::MAX_CONTENT_TAG => doc::TrackMin::MaxContent,
            CompactLength::LENGTH_TAG => doc::TrackMin::Px(min.value()),
            CompactLength::PERCENT_TAG => doc::TrackMin::Percent(min.value()),
            _ => return Err(unrepresentable(min)),
        },
        max: match max.tag() {
            CompactLength::AUTO_TAG => doc::TrackMax::Auto,
            CompactLength::MIN_CONTENT_TAG => doc::TrackMax::MinContent,
            CompactLength::MAX_CONTENT_TAG => doc::TrackMax::MaxContent,
            CompactLength::LENGTH_TAG => doc::TrackMax::Px(max.value()),
            CompactLength::PERCENT_TAG => doc::TrackMax::Percent(max.value()),
            CompactLength::FIT_CONTENT_PX_TAG => doc::TrackMax::FitContentPx(max.value()),
            CompactLength::FIT_CONTENT_PERCENT_TAG => doc::TrackMax::FitContentPercent(max.value()),
            CompactLength::FR_TAG => doc::TrackMax::Fr(max.value()),
            _ => return Err(unrepresentable(max)),
        },
    })
}

fn line_names(names: &[Vec<String>]) -> Vec<Vec<String>> {
    names.to_vec()
}

fn placement(value: &tf::GridPlacement<String>) -> doc::GridPlacement {
    match value {
        tf::GridPlacement::Auto => doc::GridPlacement::Auto,
        tf::GridPlacement::Line(line) => doc::GridPlacement::Line(line.as_i16()),
        tf::GridPlacement::NamedLine(name, index) => doc::GridPlacement::NamedLine {
            name: name.clone(),
            index: *index,
        },
        tf::GridPlacement::Span(span) => doc::GridPlacement::Span(*span),
        tf::GridPlacement::NamedSpan(name, count) => doc::GridPlacement::NamedSpan {
            name: name.clone(),
            count: *count,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_taffy_style_is_the_default_layout() {
        let exported = export_layout(&tf::Style::DEFAULT).unwrap();
        assert_eq!(exported, doc::LayoutStyle::default());
    }
}
