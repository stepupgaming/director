//! Times an HTML export on the core `Director` path against its scene document
//! painted through `Renderer::render_frames`, and writes both MP4s.
//!
//! `cargo run -p director-render --release --example export_bench -- [html] [out_dir] [runs] [auto|software|nvenc]`

use director_core::export::render_export;
use director_core::scene_export::export_scene;
use director_core::scene_import::{import_scene, ImportRegistry};
use director_core::systems::layout::LayoutEngine;
use director_core::systems::renderer::render_sampled_at_time;
use director_core::timeline::{audio_samples_per_video_frame, duration_to_frame_count, frame_to_time};
use director_core::video_wrapper::{Encoder, EncoderMode, EncoderSettings};
use director_core::Director;
use director_html::{compile_file, register_import, CompileOptions};
use director_render::Renderer;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

fn registry() -> ImportRegistry {
    let mut registry = ImportRegistry::core();
    register_import(&mut registry);
    registry
}

fn compiled(input: &Path, mode: EncoderMode) -> anyhow::Result<Director> {
    let mut director = compile_file(input, CompileOptions::default())?.director;
    director.encoder_mode = mode;
    Ok(director)
}

fn renderer(director: &Director) -> anyhow::Result<Renderer> {
    Ok(Renderer::new(Arc::new(export_scene(director)?), director.assets.loader.clone())
        .with_registry(registry())
        .with_render_mode(director.render_mode))
}

fn frame_times(director: &Director) -> Vec<f64> {
    let fps = director.fps.max(1e-6);
    let mut duration = director.timeline_duration();
    if duration == 0.0 {
        duration = 5.0;
    }
    (0..duration_to_frame_count(duration, fps)).map(|i| frame_to_time(i, fps)).collect()
}

/// Paint `times` in chunks through `render_frames` and stream them to the encoder in order.
fn document_export(director: &Director, path: &Path, mode: EncoderMode) -> anyhow::Result<()> {
    let renderer = renderer(director)?;
    let times = frame_times(director);
    let (width, height) = renderer.output_size();
    let settings = EncoderSettings::preset_h264_yuv420p(width as usize, height as usize)
        .with_fps(director.fps)
        .with_encoder_mode(mode);
    let mut encoder = Encoder::new(path, settings)?;
    let mut audio = import_scene(renderer.document(), director.assets.loader.clone(), &registry(), director.render_mode)?;
    let block = audio_samples_per_video_frame(audio.audio_mixer.sample_rate, director.fps.max(1e-6), 1);
    for chunk in times.chunks(rayon::current_num_threads() * 4) {
        for (frame, &time) in renderer.render_frames(chunk).into_iter().zip(chunk) {
            encoder.encode(&frame?.rgba, time)?;
            encoder.encode_audio(&audio.mix_audio(block, time), time)?;
        }
    }
    encoder.finish()
}

/// Core frame loop without encoding: layout, paint and readback.
fn core_render_only(director: &mut Director) -> anyhow::Result<()> {
    let info = skia_safe::ImageInfo::new(
        (director.width, director.height),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        Some(skia_safe::ColorSpace::new_srgb()),
    );
    let raster = || skia_safe::surfaces::raster(&info, None, None).ok_or_else(|| anyhow::anyhow!("surface"));
    let mut surface = raster()?;
    let mut transitions = if director.transitions.is_empty() { None } else { Some((raster()?, raster()?)) };
    let mut accumulation = None;
    let mut layout = LayoutEngine::new();
    let mut pixels = vec![0u8; info.compute_min_byte_size()];
    for time in frame_times(director) {
        render_sampled_at_time(director, &mut layout, time, &mut surface, &mut accumulation, &mut transitions)?;
        anyhow::ensure!(surface.read_pixels(&info, &mut pixels, info.min_row_bytes(), (0, 0)));
    }
    Ok(())
}

fn ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let input = PathBuf::from(args.get(1).map_or("examples/html/native-motion.html", String::as_str));
    let out_dir = PathBuf::from(args.get(2).map_or("target/export-bench", String::as_str));
    let runs: usize = args.get(3).map_or(Ok(3), |runs| runs.parse())?;
    let mode = match args.get(4).map_or("software", String::as_str) {
        "auto" => EncoderMode::Auto,
        "nvenc" => EncoderMode::Nvenc,
        _ => EncoderMode::Software,
    };
    std::fs::create_dir_all(&out_dir)?;
    let core_path = out_dir.join("core.mp4");
    let render_path = out_dir.join("director-render.mp4");

    let probe = compiled(&input, mode)?;
    let frames = frame_times(&probe).len();
    let started = Instant::now();
    let session = renderer(&probe)?.session()?;
    let session_ms = ms(started);
    drop(session);

    let (mut core_export, mut render_export_ms, mut core_render, mut render_render, mut one_session) =
        (vec![], vec![], vec![], vec![], vec![]);
    let mut live_session = vec![];
    for _ in 0..runs {
        let mut director = compiled(&input, mode)?;
        let started = Instant::now();
        render_export(&mut director, core_path.clone(), None, None, false)?;
        core_export.push(ms(started));

        let director = compiled(&input, mode)?;
        let started = Instant::now();
        document_export(&director, &render_path, mode)?;
        render_export_ms.push(ms(started));

        let mut director = compiled(&input, mode)?;
        let started = Instant::now();
        core_render_only(&mut director)?;
        core_render.push(ms(started));

        let director = compiled(&input, mode)?;
        let started = Instant::now();
        for frame in renderer(&director)?.render_frames(&frame_times(&director)) {
            frame?;
        }
        render_render.push(ms(started));

        let director = compiled(&input, mode)?;
        let started = Instant::now();
        let mut session = renderer(&director)?.session()?;
        for time in frame_times(&director) {
            session.render(time)?;
        }
        one_session.push(ms(started));

        let director = compiled(&input, mode)?;
        let size = renderer(&director)?.output_size();
        let times = frame_times(&director);
        let started = Instant::now();
        let mut session = director_render::FrameSession::new(director, size)?;
        for time in times {
            session.render(time)?;
        }
        live_session.push(ms(started));
    }

    let (core_export, render_export_ms) = (median(&mut core_export), median(&mut render_export_ms));
    let (core_render, render_render) = (median(&mut core_render), median(&mut render_render));
    let (one_session, live_session) = (median(&mut one_session), median(&mut live_session));
    println!(
        "{}",
        serde_json::json!({
            "input": input,
            "size": [probe.width, probe.height],
            "fps": probe.fps,
            "frames": frames,
            "threads": rayon::current_num_threads(),
            "encoder": format!("{mode:?}"),
            "runs": runs,
            "session_import_ms": session_ms,
            "export_ms": {"core": core_export, "director_render": render_export_ms, "speedup": core_export / render_export_ms},
            "render_only_ms": {"core": core_render, "director_render": render_render, "speedup": core_render / render_render, "one_session": one_session, "live_session": live_session},
            "render_only_fps": {"core": frames as f64 * 1000.0 / core_render, "director_render": frames as f64 * 1000.0 / render_render},
            "outputs": [std::fs::canonicalize(&core_path)?, std::fs::canonicalize(&render_path)?],
        })
    );
    Ok(())
}
