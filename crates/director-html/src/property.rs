//! The CSS longhands Director compiles: which ones inherit and which part of
//! `style::translate` owns each. A name missing from [`PROPERTIES`] (other than
//! a custom property) fails translation as unsupported.

/// The part of `style::translate` that turns a property into layout or paint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// Taffy layout: display, box model, flex, grid and insets.
    Layout,
    /// Background layers and their geometry.
    Background,
    /// Border widths, colors, styles and corner radii.
    Border,
    /// Opacity, visibility, overflow, stacking, replaced content and clip paths.
    Box,
    /// Shadows, filters, blending and masks.
    Effects,
    /// 2D and 3D transforms.
    Transform,
    /// Fonts, text layout and text decoration.
    Text,
    /// CSS animations and transitions, read by `animation.rs`.
    Animation,
}

/// One compiled longhand.
#[derive(Clone, Copy, Debug)]
pub struct PropertyInfo {
    pub name: &'static str,
    /// Children take the parent's value when they do not set it.
    pub inherited: bool,
    pub group: Group,
}

const fn p(name: &'static str, group: Group) -> PropertyInfo {
    PropertyInfo {
        name,
        inherited: false,
        group,
    }
}

const fn inherited(name: &'static str, group: Group) -> PropertyInfo {
    PropertyInfo {
        name,
        inherited: true,
        group,
    }
}

use Group::*;

/// Every compiled longhand, sorted by name.
pub static PROPERTIES: &[PropertyInfo] = &[
    p("-webkit-line-clamp", Text),
    p("-webkit-text-stroke", Text),
    p("-webkit-text-stroke-color", Text),
    p("-webkit-text-stroke-width", Text),
    p("align-content", Layout),
    p("align-items", Layout),
    p("align-self", Layout),
    p("animation-composition", Animation),
    p("animation-delay", Animation),
    p("animation-direction", Animation),
    p("animation-duration", Animation),
    p("animation-fill-mode", Animation),
    p("animation-iteration-count", Animation),
    p("animation-name", Animation),
    p("animation-play-state", Animation),
    p("animation-range-end", Animation),
    p("animation-range-start", Animation),
    p("animation-timeline", Animation),
    p("animation-timing-function", Animation),
    p("aspect-ratio", Layout),
    p("backdrop-filter", Effects),
    p("backface-visibility", Transform),
    p("background-attachment", Background),
    p("background-clip", Background),
    p("background-color", Background),
    p("background-image", Background),
    p("background-origin", Background),
    p("background-position", Background),
    p("background-position-x", Background),
    p("background-position-y", Background),
    p("background-repeat", Background),
    p("background-size", Background),
    p("border-bottom-color", Border),
    p("border-bottom-left-radius", Border),
    p("border-bottom-right-radius", Border),
    p("border-bottom-style", Border),
    p("border-bottom-width", Border),
    p("border-left-color", Border),
    p("border-left-style", Border),
    p("border-left-width", Border),
    p("border-right-color", Border),
    p("border-right-style", Border),
    p("border-right-width", Border),
    p("border-top-color", Border),
    p("border-top-left-radius", Border),
    p("border-top-right-radius", Border),
    p("border-top-style", Border),
    p("border-top-width", Border),
    p("bottom", Layout),
    p("box-shadow", Effects),
    p("box-sizing", Layout),
    p("clip-path", Box),
    inherited("color", Text),
    p("column-gap", Layout),
    inherited("direction", Text),
    p("display", Layout),
    p("filter", Effects),
    p("flex-basis", Layout),
    p("flex-direction", Layout),
    p("flex-grow", Layout),
    p("flex-shrink", Layout),
    p("flex-wrap", Layout),
    inherited("font-family", Text),
    inherited("font-size", Text),
    inherited("font-stretch", Text),
    inherited("font-style", Text),
    p("font-variant-caps", Text),
    p("font-variant-numeric", Text),
    inherited("font-variation-settings", Text),
    inherited("font-weight", Text),
    p("grid-auto-columns", Layout),
    p("grid-auto-flow", Layout),
    p("grid-auto-rows", Layout),
    p("grid-column-end", Layout),
    p("grid-column-start", Layout),
    p("grid-row-end", Layout),
    p("grid-row-start", Layout),
    p("grid-template-columns", Layout),
    p("grid-template-rows", Layout),
    p("height", Layout),
    p("justify-content", Layout),
    p("justify-items", Layout),
    p("justify-self", Layout),
    p("left", Layout),
    inherited("letter-spacing", Text),
    p("line-clamp", Text),
    inherited("line-height", Text),
    p("margin-bottom", Layout),
    p("margin-left", Layout),
    p("margin-right", Layout),
    p("margin-top", Layout),
    p("mask-image", Effects),
    p("mask-mode", Effects),
    p("max-height", Layout),
    p("max-width", Layout),
    p("min-height", Layout),
    p("min-width", Layout),
    p("mix-blend-mode", Effects),
    p("object-fit", Box),
    p("object-position", Box),
    p("opacity", Box),
    inherited("overflow-wrap", Text),
    p("overflow-x", Box),
    p("overflow-y", Box),
    p("padding-bottom", Layout),
    p("padding-left", Layout),
    p("padding-right", Layout),
    p("padding-top", Layout),
    p("paint-order", Text),
    p("perspective", Transform),
    p("perspective-origin", Transform),
    p("position", Layout),
    p("right", Layout),
    p("row-gap", Layout),
    inherited("text-align", Text),
    p("text-decoration", Text),
    p("text-decoration-color", Text),
    p("text-decoration-line", Text),
    p("text-decoration-style", Text),
    p("text-decoration-thickness", Text),
    p("text-indent", Text),
    p("text-overflow", Text),
    p("text-shadow", Text),
    inherited("text-transform", Text),
    p("text-underline-offset", Text),
    p("top", Layout),
    p("transform", Transform),
    p("transform-origin", Transform),
    p("transform-style", Transform),
    p("transition-delay", Animation),
    p("transition-duration", Animation),
    p("transition-property", Animation),
    p("transition-timing-function", Animation),
    inherited("visibility", Box),
    inherited("white-space", Text),
    p("width", Layout),
    inherited("word-break", Text),
    inherited("word-spacing", Text),
    inherited("word-wrap", Text),
    p("z-index", Box),
];

/// The table entry for `name`.
pub fn info(name: &str) -> Option<&'static PropertyInfo> {
    PROPERTIES
        .binary_search_by(|entry| entry.name.cmp(name))
        .ok()
        .map(|index| &PROPERTIES[index])
}

/// Whether `name` inherits: custom properties and the inherited longhands.
pub fn is_inherited(name: &str) -> bool {
    name.starts_with("--") || info(name).is_some_and(|entry| entry.inherited)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_and_unique() {
        assert!(PROPERTIES.windows(2).all(|pair| pair[0].name < pair[1].name));
    }

    #[test]
    fn lookups_find_every_entry() {
        for entry in PROPERTIES {
            assert_eq!(info(entry.name).map(|found| found.name), Some(entry.name));
        }
        assert!(info("float").is_none());
        assert!(is_inherited("--accent"));
        assert!(is_inherited("color"));
        assert!(!is_inherited("width"));
    }
}
