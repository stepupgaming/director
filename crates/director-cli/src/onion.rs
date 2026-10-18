//! Weighted blend of frames. Later frames have the higher weight.

use crate::input::Input;
use crate::renderer::{self, frame_within_limit, paint_rgba};
use clap::Parser;
use director_core::video_wrapper::RenderMode;
use std::path::PathBuf;

pub const MAX_ONION_FRAMES: usize = 8;

#[derive(Parser, Debug)]
pub struct OnionArgs {
    #[command(flatten)]
    pub input: Input,
    /// Frames sampled across the duration
    #[arg(short = 'n', long, default_value_t = 4)]
    pub frames: usize,
    #[arg(short = 'o', long)]
    pub output: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    pub gpu: bool,
}

pub fn frame_times(duration: f64, count: usize) -> Result<Vec<f64>, String> {
    if count == 0 {
        return Err("frames must be > 0".into());
    }
    if count > MAX_ONION_FRAMES {
        return Err(format!("frames must be <= {MAX_ONION_FRAMES}"));
    }
    if !duration.is_finite() {
        return Err("duration must be finite".into());
    }
    Ok((0..count)
        .map(|index| {
            if duration > 0.0 {
                index as f64 * duration / count as f64
            } else {
                0.0
            }
        })
        .collect())
}

pub fn blend_onion(frames: &[Vec<u8>]) -> Result<Vec<u8>, String> {
    let Some(first) = frames.first() else {
        return Err("onion needs at least one frame".into());
    };
    if frames.len() > MAX_ONION_FRAMES {
        return Err(format!("frames must be <= {MAX_ONION_FRAMES}"));
    }
    if first.is_empty() || first.len() % 4 != 0 {
        return Err("frame is empty".into());
    }
    if frames.iter().any(|frame| frame.len() != first.len()) {
        return Err("onion frames must share one size".into());
    }
    let pixels = first.len() / 4;
    if pixels > renderer::MAX_FRAME_PIXELS {
        return Err(format!(
            "frame exceeds {} pixels",
            renderer::MAX_FRAME_PIXELS
        ));
    }
    let weight_sum = (frames.len() * (frames.len() + 1) / 2) as f32;
    let mut out = vec![0u8; first.len()];
    for pixel in 0..pixels {
        let mut acc = [0f32; 4];
        for (index, frame) in frames.iter().enumerate() {
            let weight = (index + 1) as f32;
            let offset = pixel * 4;
            for channel in 0..4 {
                acc[channel] += frame[offset + channel] as f32 * weight;
            }
        }
        let offset = pixel * 4;
        for channel in 0..4 {
            out[offset + channel] = (acc[channel] / weight_sum).round().clamp(0.0, 255.0) as u8;
        }
    }
    Ok(out)
}

pub fn run(args: OnionArgs) -> i32 {
    let loaded = match args.input.load(RenderMode::Export) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    let mut director = match loaded.movie.director.lock() {
        Ok(director) => director,
        Err(_) => {
            eprintln!("Director lock poisoned");
            return 1;
        }
    };
    if let Err(error) = frame_within_limit(director.width, director.height) {
        eprintln!("{error}");
        return 1;
    }
    let times = match frame_times(director.timeline_duration(), args.frames) {
        Ok(times) => times,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    let mut frames = Vec::with_capacity(times.len());
    let mut width = 0u32;
    let mut height = 0u32;
    for time in &times {
        match paint_rgba(&mut director, *time, args.gpu) {
            Ok((pixels, frame_w, frame_h)) => {
                if !frames.is_empty() && (frame_w != width || frame_h != height) {
                    eprintln!("onion frames must share one size");
                    return 2;
                }
                width = frame_w;
                height = frame_h;
                frames.push(pixels);
            }
            Err(error) => {
                eprintln!("{error}");
                return 2;
            }
        }
    }
    let blended = match blend_onion(&frames) {
        Ok(blended) => blended,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    let info = skia_safe::ImageInfo::new(
        (width as i32, height as i32),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let png = match renderer::encode_png(info, &blended) {
        Ok(png) => png,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    let output = args.output.unwrap_or_else(|| {
        let stem = args
            .input
            .input
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        args.input.input.with_file_name(format!("{stem}_onion.png"))
    });
    if let Err(error) = std::fs::write(&output, png) {
        eprintln!("{error}");
        return 2;
    }
    let listed = times
        .iter()
        .enumerate()
        .map(|(index, time)| {
            serde_json::json!({
                "index": index,
                "time": time,
                "weight": index + 1,
            })
        })
        .collect::<Vec<_>>();
    println!(
        "{}",
        serde_json::json!({
            "output": output.display().to_string(),
            "width": width,
            "height": height,
            "frames": listed,
        })
    );
    0
}

#[cfg(test)]
mod tests {
    use super::{blend_onion, frame_times};

    #[test]
    fn agent_onion_weights_the_later_frame_more() {
        let err = frame_times(1.0, 0).unwrap_err();
        assert!(err.contains("frames"), "{err}");
        assert!(frame_times(1.0, 9).is_err());
        assert!(frame_times(f64::NAN, 2).is_err());
        let times = frame_times(1.0, 2).unwrap();
        assert_eq!(times, vec![0.0, 0.5]);
        let blended = blend_onion(&[vec![0, 0, 0, 255], vec![255, 255, 255, 255]]).unwrap();
        assert_eq!(blended, vec![170, 170, 170, 255]);
        let flat = blend_onion(&[vec![20, 30, 40, 255], vec![20, 30, 40, 255]]).unwrap();
        assert_eq!(flat, vec![20, 30, 40, 255]);
    }

    #[test]
    fn agent_onion_of_a_static_frame_stays_that_frame() {
        let mut compiled = director_html::compile(
            "<style>body{margin:0}div{width:32px;height:32px;background:red}</style><div></div>",
            director_html::CompileOptions {
                width: 32,
                height: 32,
                duration: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
        let (first, _, _) = crate::renderer::paint_rgba(&mut compiled.director, 0.0, false).unwrap();
        let (second, _, _) =
            crate::renderer::paint_rgba(&mut compiled.director, 0.5, false).unwrap();
        let blended = blend_onion(&[first.clone(), second]).unwrap();
        assert_eq!(blended, first);
    }
}
