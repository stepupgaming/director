//! Tracks, clips and transitions.

use super::{export_easing, node_ref};
use crate::director::Director;
use crate::systems::transitions::TransitionType;
use director_scene::{Clip, Timeline, Track, Transition};
use std::collections::BTreeMap;

pub(super) fn export_timeline(director: &Director) -> Timeline {
    Timeline {
        tracks: director
            .tracks
            .iter()
            .map(|track| Track {
                name: track.name.clone(),
                enabled: track.enabled,
                opacity: track.opacity,
                clips: track
                    .clips
                    .iter()
                    .map(|clip| Clip {
                        scene_root: node_ref(clip.scene_root),
                        name: clip.name.clone(),
                        start_time: clip.start_time,
                        duration: clip.duration,
                        source_start: clip.source_start,
                        source_end: clip.source_end,
                        speed: clip.speed,
                        freeze_time: clip.freeze_time,
                        z_index: clip.z_index,
                        audio_tracks: clip.audio_tracks.clone(),
                        fade_in: clip.fade_in,
                        fade_out: clip.fade_out,
                    })
                    .collect(),
            })
            .collect(),
        default_track: director.default_track_idx,
        transitions: director
            .transitions
            .iter()
            .map(|transition| {
                let (kind, params) = transition_kind(&transition.kind);
                Transition {
                    from_scene: transition.from_scene_idx,
                    to_scene: transition.to_scene_idx,
                    start_time: transition.start_time,
                    duration: transition.duration,
                    kind: kind.to_string(),
                    params: params
                        .into_iter()
                        .map(|(name, value)| (name.to_string(), value))
                        .collect(),
                    easing: export_easing(&transition.easing),
                }
            })
            .collect(),
    }
}

/// The `TRANSITION_CATALOG` name and parameters of a transition.
pub(crate) fn transition_kind(kind: &TransitionType) -> (&'static str, BTreeMap<&'static str, f32>) {
    let mut params = BTreeMap::new();
    let name = match kind {
        TransitionType::Fade => "fade",
        TransitionType::SlideLeft => "slide_left",
        TransitionType::SlideRight => "slide_right",
        TransitionType::WipeLeft => "wipe_left",
        TransitionType::WipeRight => "wipe_right",
        TransitionType::CircleOpen => "circle_open",
        TransitionType::Wave {
            amplitude,
            frequency,
        } => {
            params.insert("amplitude", *amplitude);
            params.insert("frequency", *frequency);
            "wave"
        }
        TransitionType::Glitch { intensity } => {
            params.insert("intensity", *intensity);
            "glitch"
        }
        TransitionType::Iris {
            start_radius,
            end_radius,
        } => {
            params.insert("start_radius", *start_radius);
            params.insert("end_radius", *end_radius);
            "iris"
        }
        TransitionType::Spiral { rotations } => {
            params.insert("rotations", *rotations);
            "spiral"
        }
        TransitionType::ZoomIn => "zoom_in",
        TransitionType::RadialWipe => "radial_wipe",
        TransitionType::PixelateDissolve => "pixelate_dissolve",
        TransitionType::CrossZoom => "cross_zoom",
        TransitionType::Wind => "wind",
        TransitionType::Burn => "burn",
        TransitionType::Dissolve => "dissolve",
        TransitionType::Flip => "flip",
        TransitionType::ClockWipe => "clock_wipe",
        TransitionType::Cube => "cube",
    };
    (name, params)
}
