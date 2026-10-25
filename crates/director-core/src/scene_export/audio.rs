//! Mixer tracks and their effect chains.

use super::{channel, ExportContext, ExportError};
use crate::audio::AudioMixer;
use director_scene::{AudioEffect, AudioMix, AudioParamAnimation, AudioTrack};

pub(super) fn export_audio(mixer: &AudioMixer, cx: &mut ExportContext) -> Result<AudioMix, ExportError> {
    let mut tracks = Vec::new();
    for (slot, track) in mixer.tracks.iter().enumerate() {
        let Some(track) = track else {
            continue;
        };
        let asset = match &track.source_path {
            Some(path) => cx.path_asset("audio", "audio/*", path),
            None => {
                let bytes: Vec<u8> = track.samples.iter().flat_map(|s| s.to_le_bytes()).collect();
                cx.embedded_asset("audio", "audio/x-f32le", &bytes)
            }
        };
        tracks.push(AudioTrack {
            slot,
            asset,
            volume: channel(&track.volume),
            pan: channel(&track.pan),
            start_time: track.start_time,
            duration: track.duration,
            loop_audio: track.loop_audio,
            effects: track
                .effects
                .effects
                .iter()
                .map(|effect| {
                    let description = effect.describe();
                    AudioEffect {
                        kind: description.kind.to_string(),
                        filter: description.filter.map(str::to_string),
                        params: description
                            .params
                            .into_iter()
                            .map(|(name, value)| (name.to_string(), value))
                            .collect(),
                    }
                })
                .collect(),
            param_animations: track
                .effect_param_animations
                .iter()
                .map(|animation| AudioParamAnimation {
                    effect: animation.effect_idx,
                    param: animation.param_name.clone(),
                    channel: channel(&animation.animated),
                })
                .collect(),
        });
    }
    Ok(AudioMix {
        sample_rate: mixer.sample_rate,
        tracks,
    })
}
