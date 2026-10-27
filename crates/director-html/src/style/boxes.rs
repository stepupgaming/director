//! Typed box longhands: opacity, visibility, overflow and stacking.
//! `object-fit`, `object-position` and `clip-path` stay on the text path in
//! `translate`.

use super::{unsupported, PaintStyle};
use crate::computed::Computed;
use lightningcss::properties::{
    display::Visibility, overflow::OverflowKeyword, position::ZIndex, Property,
};
use taffy::{prelude::*, style::Overflow};

const TYPED: &[&str] = &["opacity", "overflow-x", "overflow-y", "visibility", "z-index"];

pub(super) fn apply(
    name: &str,
    entry: Computed<'_>,
    style: &mut Style,
    paint: &mut PaintStyle,
) -> Option<Result<(), String>> {
    if !TYPED.contains(&name) {
        return None;
    }
    let s = entry.text;
    let result = match entry.value {
        Property::Opacity(alpha) => {
            paint.opacity = alpha.0.clamp(0.0, 1.0);
            Ok(())
        }
        Property::Visibility(visibility) => match visibility {
            Visibility::Visible | Visibility::Hidden => {
                paint.visible = *visibility == Visibility::Visible;
                Ok(())
            }
            Visibility::Collapse => Err("visibility collapse unsupported".into()),
        },
        Property::OverflowX(keyword) => overflow(keyword, paint).map(|o| style.overflow.x = o),
        Property::OverflowY(keyword) => overflow(keyword, paint).map(|o| style.overflow.y = o),
        Property::ZIndex(z) => {
            paint.z_index = match z {
                ZIndex::Auto => 0,
                ZIndex::Integer(n) => *n,
            };
            Ok(())
        }
        _ => Err(unsupported(name, s)),
    };
    Some(result)
}

fn overflow(keyword: &OverflowKeyword, paint: &mut PaintStyle) -> Result<Overflow, String> {
    match keyword {
        OverflowKeyword::Visible => Ok(Overflow::Visible),
        OverflowKeyword::Hidden | OverflowKeyword::Clip => {
            paint.box_node.overflow = "hidden".into();
            Ok(Overflow::Hidden)
        }
        OverflowKeyword::Scroll | OverflowKeyword::Auto => {
            Err("scrolling overflow unsupported".into())
        }
    }
}
