//! Layout style for one node, mirroring the CSS box, flex and grid model.
//!
//! Field defaults match Taffy's `Style::DEFAULT`. A field equal to its default
//! is left out of the JSON.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Layout inputs for one node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct LayoutStyle {
    #[serde(skip_serializing_if = "is_default")]
    pub display: Display,
    #[serde(skip_serializing_if = "is_false")]
    pub item_is_table: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub item_is_replaced: bool,
    #[serde(skip_serializing_if = "is_default")]
    pub box_sizing: BoxSizing,
    #[serde(skip_serializing_if = "is_default")]
    pub overflow: Axes<Overflow>,
    #[serde(skip_serializing_if = "is_zero")]
    pub scrollbar_width: f32,
    #[serde(skip_serializing_if = "is_default")]
    pub position: Position,
    #[serde(skip_serializing_if = "Edges::<Dimension>::is_auto")]
    pub inset: Edges<Dimension>,
    #[serde(skip_serializing_if = "Size::<Dimension>::is_auto")]
    pub size: Size<Dimension>,
    #[serde(skip_serializing_if = "Size::<Dimension>::is_auto")]
    pub min_size: Size<Dimension>,
    #[serde(skip_serializing_if = "Size::<Dimension>::is_auto")]
    pub max_size: Size<Dimension>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<f32>,
    #[serde(skip_serializing_if = "Edges::<Dimension>::is_zero")]
    pub margin: Edges<Dimension>,
    #[serde(skip_serializing_if = "Edges::<Length>::is_zero")]
    pub padding: Edges<Length>,
    #[serde(skip_serializing_if = "Edges::<Length>::is_zero")]
    pub border: Edges<Length>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align_items: Option<AlignItems>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align_self: Option<AlignItems>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub justify_items: Option<AlignItems>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub justify_self: Option<AlignItems>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align_content: Option<AlignContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub justify_content: Option<AlignContent>,
    #[serde(skip_serializing_if = "Size::<Length>::is_zero")]
    pub gap: Size<Length>,
    #[serde(skip_serializing_if = "is_default")]
    pub text_align: BlockTextAlign,
    #[serde(skip_serializing_if = "is_default")]
    pub flex_direction: FlexDirection,
    #[serde(skip_serializing_if = "is_default")]
    pub flex_wrap: FlexWrap,
    #[serde(skip_serializing_if = "is_default")]
    pub flex_basis: Dimension,
    #[serde(skip_serializing_if = "is_zero")]
    pub flex_grow: f32,
    #[serde(skip_serializing_if = "is_one")]
    pub flex_shrink: f32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub grid_template_rows: Vec<GridTemplateComponent>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub grid_template_columns: Vec<GridTemplateComponent>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub grid_auto_rows: Vec<TrackSize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub grid_auto_columns: Vec<TrackSize>,
    #[serde(skip_serializing_if = "is_default")]
    pub grid_auto_flow: GridAutoFlow,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub grid_template_areas: Vec<GridTemplateArea>,
    /// Line names before each column track and after the last one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub grid_template_column_names: Vec<Vec<String>>,
    /// Line names before each row track and after the last one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub grid_template_row_names: Vec<Vec<String>>,
    #[serde(skip_serializing_if = "Line::<GridPlacement>::is_auto")]
    pub grid_row: Line<GridPlacement>,
    #[serde(skip_serializing_if = "Line::<GridPlacement>::is_auto")]
    pub grid_column: Line<GridPlacement>,
}

impl Default for LayoutStyle {
    fn default() -> Self {
        Self {
            display: Display::default(),
            item_is_table: false,
            item_is_replaced: false,
            box_sizing: BoxSizing::default(),
            overflow: Axes::default(),
            scrollbar_width: 0.0,
            position: Position::default(),
            inset: Edges::all(Dimension::Auto),
            size: Size::all(Dimension::Auto),
            min_size: Size::all(Dimension::Auto),
            max_size: Size::all(Dimension::Auto),
            aspect_ratio: None,
            margin: Edges::all(Dimension::Px(0.0)),
            padding: Edges::all(Length::Px(0.0)),
            border: Edges::all(Length::Px(0.0)),
            align_items: None,
            align_self: None,
            justify_items: None,
            justify_self: None,
            align_content: None,
            justify_content: None,
            gap: Size::all(Length::Px(0.0)),
            text_align: BlockTextAlign::default(),
            flex_direction: FlexDirection::default(),
            flex_wrap: FlexWrap::default(),
            flex_basis: Dimension::Auto,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            grid_template_rows: Vec::new(),
            grid_template_columns: Vec::new(),
            grid_auto_rows: Vec::new(),
            grid_auto_columns: Vec::new(),
            grid_auto_flow: GridAutoFlow::default(),
            grid_template_areas: Vec::new(),
            grid_template_column_names: Vec::new(),
            grid_template_row_names: Vec::new(),
            grid_row: Line::all(GridPlacement::Auto),
            grid_column: Line::all(GridPlacement::Auto),
        }
    }
}

/// A length that may be `auto`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    #[default]
    Auto,
    /// Pixels.
    Px(f32),
    /// Fraction of the containing size, `0.5` for 50%.
    Percent(f32),
}

/// A length that is never `auto`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Length {
    /// Pixels.
    Px(f32),
    /// Fraction of the containing size, `0.5` for 50%.
    Percent(f32),
}

impl Default for Length {
    fn default() -> Self {
        Self::Px(0.0)
    }
}

/// Four sides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Edges<T> {
    pub left: T,
    pub right: T,
    pub top: T,
    pub bottom: T,
}

impl<T: Copy> Edges<T> {
    /// The same value on every side.
    pub fn all(value: T) -> Self {
        Self {
            left: value,
            right: value,
            top: value,
            bottom: value,
        }
    }
}

impl Edges<Dimension> {
    fn is_auto(&self) -> bool {
        *self == Self::all(Dimension::Auto)
    }

    fn is_zero(&self) -> bool {
        *self == Self::all(Dimension::Px(0.0))
    }
}

impl Edges<Length> {
    fn is_zero(&self) -> bool {
        *self == Self::all(Length::Px(0.0))
    }
}

/// Width and height.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Size<T> {
    pub width: T,
    pub height: T,
}

impl<T: Copy> Size<T> {
    /// The same value on both axes.
    pub fn all(value: T) -> Self {
        Self {
            width: value,
            height: value,
        }
    }
}

impl Size<Dimension> {
    fn is_auto(&self) -> bool {
        *self == Self::all(Dimension::Auto)
    }
}

impl Size<Length> {
    fn is_zero(&self) -> bool {
        *self == Self::all(Length::Px(0.0))
    }
}

/// Horizontal and vertical.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Axes<T> {
    pub x: T,
    pub y: T,
}

/// Grid line placement at the start and end of an item.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Line<T> {
    pub start: T,
    pub end: T,
}

impl<T: Clone> Line<T> {
    /// The same placement at both ends.
    pub fn all(value: T) -> Self {
        Self {
            start: value.clone(),
            end: value,
        }
    }
}

impl Line<GridPlacement> {
    fn is_auto(&self) -> bool {
        self.start == GridPlacement::Auto && self.end == GridPlacement::Auto
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Display {
    Block,
    #[default]
    Flex,
    Grid,
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BoxSizing {
    #[default]
    BorderBox,
    ContentBox,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Overflow {
    #[default]
    Visible,
    Clip,
    Hidden,
    Scroll,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    #[default]
    Relative,
    Absolute,
}

/// `align-items`, `align-self`, `justify-items` and `justify-self`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AlignItems {
    Start,
    End,
    FlexStart,
    FlexEnd,
    Center,
    Baseline,
    Stretch,
}

/// `align-content` and `justify-content`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AlignContent {
    Start,
    End,
    FlexStart,
    FlexEnd,
    Center,
    Stretch,
    SpaceBetween,
    SpaceEvenly,
    SpaceAround,
}

/// Taffy's legacy block `text-align` used for block-level child alignment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BlockTextAlign {
    #[default]
    Auto,
    LegacyLeft,
    LegacyRight,
    LegacyCenter,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FlexDirection {
    #[default]
    Row,
    Column,
    RowReverse,
    ColumnReverse,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FlexWrap {
    #[default]
    NoWrap,
    Wrap,
    WrapReverse,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GridAutoFlow {
    #[default]
    Row,
    Column,
    RowDense,
    ColumnDense,
}

/// One entry of `grid-template-rows` or `grid-template-columns`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GridTemplateComponent {
    /// One track.
    Single(TrackSize),
    /// `repeat(count, tracks)` with the line names inside it.
    Repeat {
        count: Repetition,
        tracks: Vec<TrackSize>,
        line_names: Vec<Vec<String>>,
    },
}

/// The count of a `repeat()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Repetition {
    AutoFill,
    AutoFit,
    Count(u16),
}

/// `minmax(min, max)`. A plain track size has the same value on both sides.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TrackSize {
    pub min: TrackMin,
    pub max: TrackMax,
}

/// Lower bound of a track.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrackMin {
    Auto,
    MinContent,
    MaxContent,
    Px(f32),
    Percent(f32),
}

/// Upper bound of a track.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrackMax {
    Auto,
    MinContent,
    MaxContent,
    Px(f32),
    Percent(f32),
    /// `fit-content(px)`.
    FitContentPx(f32),
    /// `fit-content(percent)`, as a fraction.
    FitContentPercent(f32),
    /// Flexible `fr` units.
    Fr(f32),
}

/// One named area from `grid-template-areas`. Lines are 1-based.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GridTemplateArea {
    pub name: String,
    pub row_start: u16,
    pub row_end: u16,
    pub column_start: u16,
    pub column_end: u16,
}

/// `grid-row-start` and friends.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GridPlacement {
    #[default]
    Auto,
    /// A numbered line. Negative counts from the end.
    Line(i16),
    /// The `index`th line named `name`.
    NamedLine { name: String, index: i16 },
    /// Span a number of tracks.
    Span(u16),
    /// Span until the `count`th line named `name`.
    NamedSpan { name: String, count: u16 },
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_zero(value: &f32) -> bool {
    *value == 0.0
}

fn is_one(value: &f32) -> bool {
    *value == 1.0
}
