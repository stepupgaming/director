//! Tracks, clips and transitions.

use super::{easing, resolve, ImportError};
use crate::director::Director;
use crate::systems::transitions::{Transition, TransitionType};
use crate::timeline::{Track, TrackClip};
use crate::types::NodeId;
use director_scene::Timeline;
use std::collections::{BTreeMap, HashMap};

pub(super) fn import_timeline(
    director: &mut Director,
    timeline: &Timeline,
    ids: &HashMap<(u32, u32), NodeId>,
) -> Result<(), ImportError> {
    let mut tracks = Vec::with_capacity(timeline.tracks.len());
    for track in &timeline.tracks {
        let mut imported = Track::new(&track.name);
        imported.enabled = track.enabled;
        imported.opacity = track.opacity;
        for clip in &track.clips {
            let mut placed = TrackClip::new(resolve(ids, clip.scene_root)?, clip.start_time, clip.duration);
            placed.name = clip.name.clone();
            placed.source_start = clip.source_start;
            placed.source_end = clip.source_end;
            placed.speed = clip.speed;
            placed.freeze_time = clip.freeze_time;
            placed.z_index = clip.z_index;
            placed.audio_tracks = clip.audio_tracks.clone();
            placed.fade_in = clip.fade_in;
            placed.fade_out = clip.fade_out;
            imported.clips.push(placed);
        }
        tracks.push(imported);
    }
    if !tracks.is_empty() {
        director.tracks = tracks;
    }
    director.default_track_idx = timeline.default_track;
    director.transitions = timeline
        .transitions
        .iter()
        .map(|transition| {
            Ok(Transition {
                from_scene_idx: transition.from_scene,
                to_scene_idx: transition.to_scene,
                start_time: transition.start_time,
                duration: transition.duration,
                kind: transition_kind(&transition.kind, &transition.params)?,
                easing: easing(&transition.easing),
            })
        })
        .collect::<Result<_, ImportError>>()?;
    Ok(())
}

/// The transition a catalog name and its parameters describe.
pub(crate) fn transition_kind(name: &str, params: &BTreeMap<String, f32>) -> Result<TransitionType, ImportError> {
    let param = |key: &str| {
        params
            .get(key)
            .copied()
            .ok_or_else(|| ImportError::Invalid(format!("transition `{name}` needs `{key}`")))
    };
    Ok(match name {
        "fade" => TransitionType::Fade,
        "slide_left" => TransitionType::SlideLeft,
        "slide_right" => TransitionType::SlideRight,
        "wipe_left" => TransitionType::WipeLeft,
        "wipe_right" => TransitionType::WipeRight,
        "circle_open" => TransitionType::CircleOpen,
        "wave" => TransitionType::Wave {
            amplitude: param("amplitude")?,
            frequency: param("frequency")?,
        },
        "glitch" => TransitionType::Glitch {
            intensity: param("intensity")?,
        },
        "iris" => TransitionType::Iris {
            start_radius: param("start_radius")?,
            end_radius: param("end_radius")?,
        },
        "spiral" => TransitionType::Spiral {
            rotations: param("rotations")?,
        },
        "zoom_in" => TransitionType::ZoomIn,
        "radial_wipe" => TransitionType::RadialWipe,
        "pixelate_dissolve" => TransitionType::PixelateDissolve,
        "cross_zoom" => TransitionType::CrossZoom,
        "wind" => TransitionType::Wind,
        "burn" => TransitionType::Burn,
        "dissolve" => TransitionType::Dissolve,
        "flip" => TransitionType::Flip,
        "clock_wipe" => TransitionType::ClockWipe,
        "cube" => TransitionType::Cube,
        other => return Err(ImportError::Invalid(format!("unknown transition `{other}`"))),
    })
}
