//! Insets for CSS `background-origin` and `background-clip`.
//! Percentage padding is already resolved by the caller.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxEdge {
    BorderBox,
    PaddingBox,
    ContentBox,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Insets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

pub fn parse_box_edge(source: &str) -> Result<BoxEdge, String> {
    match source.trim() {
        "border-box" => Ok(BoxEdge::BorderBox),
        "padding-box" => Ok(BoxEdge::PaddingBox),
        "content-box" => Ok(BoxEdge::ContentBox),
        trimmed => Err(format!("unsupported box edge '{trimmed}'")),
    }
}

pub fn edge_insets(
    edge: BoxEdge,
    border_left: f32,
    border_top: f32,
    border_right: f32,
    border_bottom: f32,
    padding_left: f32,
    padding_top: f32,
    padding_right: f32,
    padding_bottom: f32,
) -> Insets {
    match edge {
        BoxEdge::BorderBox => Insets {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        },
        BoxEdge::PaddingBox => Insets {
            left: border_left,
            top: border_top,
            right: border_right,
            bottom: border_bottom,
        },
        BoxEdge::ContentBox => Insets {
            left: border_left + padding_left,
            top: border_top + padding_top,
            right: border_right + padding_right,
            bottom: border_bottom + padding_bottom,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{edge_insets, parse_box_edge, BoxEdge, Insets};

    fn insets(left: f32, top: f32, right: f32, bottom: f32) -> Insets {
        Insets {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn parses_content_padding_and_border_box() {
        assert_eq!(parse_box_edge("content-box"), Ok(BoxEdge::ContentBox));
        assert_eq!(parse_box_edge(" padding-box "), Ok(BoxEdge::PaddingBox));
        assert_eq!(parse_box_edge("border-box"), Ok(BoxEdge::BorderBox));
    }

    #[test]
    fn rejects_margin_box() {
        let err = parse_box_edge("margin-box").expect_err("margin-box");
        assert!(err.contains("box edge"), "{err}");
    }

    #[test]
    fn border_box_insets_are_zero() {
        assert_eq!(
            edge_insets(BoxEdge::BorderBox, 8.0, 8.0, 8.0, 8.0, 16.0, 16.0, 16.0, 16.0),
            insets(0.0, 0.0, 0.0, 0.0)
        );
    }

    #[test]
    fn padding_box_insets_are_border_widths() {
        assert_eq!(
            edge_insets(BoxEdge::PaddingBox, 8.0, 2.0, 3.0, 4.0, 16.0, 16.0, 16.0, 16.0),
            insets(8.0, 2.0, 3.0, 4.0)
        );
    }

    #[test]
    fn content_box_adds_uniform_border_and_padding() {
        assert_eq!(
            edge_insets(BoxEdge::ContentBox, 8.0, 8.0, 8.0, 8.0, 16.0, 16.0, 16.0, 16.0),
            insets(24.0, 24.0, 24.0, 24.0)
        );
    }

    #[test]
    fn content_box_adds_per_side_border_and_padding() {
        assert_eq!(
            edge_insets(
                BoxEdge::ContentBox,
                1.0,
                2.0,
                3.0,
                4.0,
                10.0,
                20.0,
                30.0,
                40.0
            ),
            insets(11.0, 22.0, 33.0, 44.0)
        );
    }
}
