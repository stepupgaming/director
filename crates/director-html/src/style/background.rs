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
        Property::BackgroundOrigin(list) => match list.as_slice() {
            [origin] => {
                paint.background_origin = match origin {
                    CssOrigin::PaddingBox => BackgroundOrigin::PaddingBox,
                    CssOrigin::BorderBox => BackgroundOrigin::BorderBox,
                    CssOrigin::ContentBox => BackgroundOrigin::ContentBox,
                };
                Ok(())
            }
            _ => Err(unsupported(name, s)),
        },
        Property::BackgroundClip(list, _) => match list.as_slice() {
            [clip] => {
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
            _ => Err(unsupported(name, s)),
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
