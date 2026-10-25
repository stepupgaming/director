//! Mixer tracks and their effect chains from the document audio mix.

use super::{animated, ImportContext, ImportError};
use crate::audio::{load_audio_bytes, AudioMixer, AudioTrack};
use crate::audio_effects::{
    AudioEffect, AudioEffectChain, BiquadEQ, BiquadType, Compressor, Delay, EffectParamAnimation, Reverb,
};
use director_scene::AudioMix;

/// Rebuild `mix` into `mixer`, keeping each track at its slot.
pub(super) fn import_audio(mix: &AudioMix, mixer: &mut AudioMixer, cx: &ImportContext) -> Result<(), ImportError> {
    mixer.sample_rate = mix.sample_rate;
    for track in &mix.tracks {
        let samples = samples(&track.asset, track.source_start, mix.sample_rate, cx)?;
        let mut effects = AudioEffectChain::new();
        for effect in &track.effects {
            effects.add(audio_effect(effect, mix.sample_rate)?);
        }
        let effect_param_animations = track
            .param_animations
            .iter()
            .map(|animation| {
                if animation.effect >= track.effects.len() {
                    return Err(ImportError::Invalid(format!(
                        "audio track {} animates missing effect {}",
                        track.slot, animation.effect
                    )));
                }
                Ok(EffectParamAnimation {
                    effect_idx: animation.effect,
                    param_name: animation.param.clone(),
                    animated: animated(&animation.channel)?,
                })
            })
            .collect::<Result<_, _>>()?;
        if mixer.tracks.len() <= track.slot {
            mixer.tracks.resize_with(track.slot + 1, || None);
        }
        if mixer.tracks[track.slot].is_some() {
            return Err(ImportError::Invalid(format!(
                "audio slot {} holds two tracks",
                track.slot
            )));
        }
        mixer.tracks[track.slot] = Some(AudioTrack {
            samples,
            volume: animated(&track.volume)?,
            pan: animated(&track.pan)?,
            effects,
            effect_param_animations,
            start_time: track.start_time,
            duration: track.duration,
            loop_audio: track.loop_audio,
            source_path: cx.asset_path(&track.asset).map(str::to_string),
            source_start: track.source_start,
        });
    }
    Ok(())
}

/// Interleaved stereo samples: decoded f32 bytes, or a media file decoded at
/// `sample_rate` with `source_start` seconds skipped.
fn samples(id: &str, source_start: f64, sample_rate: u32, cx: &ImportContext) -> Result<Vec<f32>, ImportError> {
    let record = cx.document.asset(id).ok_or_else(|| ImportError::Asset {
        id: id.to_string(),
        reason: "missing from the asset table".to_string(),
    })?;
    let bytes = cx.asset_bytes(id)?;
    if record.media_type == "audio/x-f32le" {
        if bytes.len() % 4 != 0 {
            return Err(ImportError::Asset {
                id: id.to_string(),
                reason: "f32 samples with a partial trailing value".to_string(),
            });
        }
        return Ok(bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect());
    }
    let mut samples = load_audio_bytes(&bytes, sample_rate).map_err(|error| ImportError::Asset {
        id: id.to_string(),
        reason: format!("audio decode: {error}"),
    })?;
    let skip = (source_start * sample_rate as f64 * 2.0) as usize;
    samples.drain(..skip.min(samples.len()));
    Ok(samples)
}

fn audio_effect(effect: &director_scene::AudioEffect, sample_rate: u32) -> Result<Box<dyn AudioEffect>, ImportError> {
    let mut built: Box<dyn AudioEffect> = match effect.kind.as_str() {
        "reverb" => Box::new(Reverb::new(sample_rate)),
        "delay" => Box::new(Delay::new(sample_rate)),
        "compressor" => Box::new(Compressor::new(sample_rate)),
        "eq" => {
            let mut eq = BiquadEQ::new(sample_rate);
            eq.filter_type = match effect.filter.as_deref() {
                Some("low_pass") => BiquadType::LowPass,
                Some("high_pass") => BiquadType::HighPass,
                Some("band_pass") => BiquadType::BandPass,
                Some("peak") => BiquadType::Peak,
                Some("notch") => BiquadType::Notch,
                Some("low_shelf") => BiquadType::LowShelf,
                Some("high_shelf") => BiquadType::HighShelf,
                other => return Err(ImportError::Invalid(format!("eq filter {other:?}"))),
            };
            Box::new(eq)
        }
        other => {
            return Err(ImportError::Unsupported {
                kind: format!("audio effect {other}"),
            })
        }
    };
    for (name, value) in &effect.params {
        built.set_parameter(name, *value);
    }
    Ok(built)
}
