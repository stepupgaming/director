//! Typed background longhands. `background-image` stays on the text path in
//! `translate`, which owns gradient and `url()` layer parsing.

use super::{
    background_fit, css_color, horizontal_axis, unsupported, vertical_axis, BackgroundOrigin,
    Metrics, PaintStyle,
};
use crate::background_box::AxisRepeat;
use crate::box_edges::BoxEdge;
use crate::computed::Computed;
use director_core::animation::Animated;
use lightningcss::properties::{
    background::{
        BackgroundAttachment, BackgroundClip, BackgroundOrigin as CssOrigin,
        BackgroundRepeatKeyword,
    },
    Property,
};

/// Apply one background longhand to `paint`. `None` leaves it to the text path.
pub(super) fn apply(
    name: &str,
    entry: Computed<'_>,
    paint: &mut PaintStyle,
    m: Metrics,
) -> Option<Result<(), String>> {
    let s = entry.text;
    let result = match entry.value {
        Property::BackgroundColor(c) => {
            css_color(c, paint.color).map(|c| paint.box_node.bg_color = Some(Animated::new(c)))
        }
        Property::BackgroundPositionX(list) => list
            .iter()
            .map(|p| horizontal_axis(p, m))
            .collect::<Result<_, _>>()
            .map(|axes| paint.background_position_x = axes),
        Property::BackgroundPositionY(list) => list
            .iter()
            .map(|p| vertical_axis(p, m))
            .collect::<Result<_, _>>()
            .map(|axes| paint.background_position_y = axes),
        Property::BackgroundSize(list) => list
            .iter()
            .map(|size| background_fit(size, m))
            .collect::<Result<_, _>>()
            .map(|fits| paint.background_size = fits),
        Property::BackgroundRepeat(list) => {
            paint.background_repeat = list.iter().map(|r| (repeat(&r.x), repeat(&r.y))).collect();
            Ok(())
        }
        Property::BackgroundAttachment(list) => {
            if list.iter().all(|a| *a == BackgroundAttachment::Scroll) {
                Ok(())
            } else {
                Err(unsupported(name, s))
            }
        }
        Property::BackgroundOrigin(list) => match uniform(list) {
            Some(origin) => {
                paint.background_origin = match origin {
                    CssOrigin::PaddingBox => BackgroundOrigin::PaddingBox,
                    CssOrigin::BorderBox => BackgroundOrigin::BorderBox,
                    CssOrigin::ContentBox => BackgroundOrigin::ContentBox,
                };
                Ok(())
            }
            None => Err(unsupported(name, s)),
        },
        Property::BackgroundClip(list, _) => match uniform(list) {
            Some(clip) => {
                let edge = match clip {
                    BackgroundClip::BorderBox => Some(BoxEdge::BorderBox),
                    BackgroundClip::PaddingBox => Some(BoxEdge::PaddingBox),
                    BackgroundClip::ContentBox => Some(BoxEdge::ContentBox),
                    BackgroundClip::Text => Some(BoxEdge::Text),
                    BackgroundClip::Border => None,
                };
                edge.map(|edge| paint.background_clip = edge)
                    .ok_or_else(|| unsupported(name, s))
            }
            None => Err(unsupported(name, s)),
        },
        Property::Unparsed(_) if name != "background-image" => Err(unsupported(name, s)),
        _ => return None,
    };
    Some(result)
}

fn repeat(keyword: &BackgroundRepeatKeyword) -> AxisRepeat {
    match keyword {
        BackgroundRepeatKeyword::Repeat => AxisRepeat::Repeat,
        BackgroundRepeatKeyword::NoRepeat => AxisRepeat::NoRepeat,
        BackgroundRepeatKeyword::Space => AxisRepeat::Space,
        BackgroundRepeatKeyword::Round => AxisRepeat::Round,
    }
}

/// The one value of a per-layer list whose layers all agree. Paint keeps a
/// single origin and clip, so mixed lists stay unsupported.
fn uniform<T: PartialEq>(list: &[T]) -> Option<&T> {
    let first = list.first()?;
    list.iter().all(|v| v == first).then_some(first)
}

#[cfg(test)]
mod tests {
    use super::super::{translate, ContainingBlock, Metrics, PaintStyle};
    use super::{BackgroundOrigin, BoxEdge};
    use crate::{assets::AssetStore, computed::ComputedStyle};

    fn paint(name: &str, text: &str) -> Result<PaintStyle, Vec<String>> {
        let values = [(name.to_string(), text.to_string())].into();
        let m = Metrics {
            width: 200.0,
            height: 100.0,
            font_size: 10.0,
        };
        let assets = AssetStore::local(std::path::Path::new("."), false);
        translate(
            &ComputedStyle::from_text(&values),
            m,
            ContainingBlock::from_axes(200.0, 100.0),
            &assets,
        )
        .map(|(_, paint, _)| paint)
    }

    #[test]
    fn uniform_layer_lists_collapse_to_one_value() {
        let clip = paint("background-clip", "padding-box, padding-box").unwrap();
        assert_eq!(clip.background_clip, BoxEdge::PaddingBox);
        let origin = paint("background-origin", "content-box, content-box").unwrap();
        assert_eq!(origin.background_origin, BackgroundOrigin::ContentBox);
        assert!(paint("background-clip", "padding-box, content-box").is_err());
    }
}
