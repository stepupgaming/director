//! Times the serial export of a compiled HTML `Director` through
//! `render_export` against its scene document exported through
//! `Renderer::export`, and writes both MP4s.
//!
//! `cargo run -p director-render --release --example export_bench -- [html] [out_dir] [runs] [auto|software|nvenc]`
//!
//! `DIRECTOR_BENCH_WORKERS` overrides the pool size that `Renderer::pool_for` picks.

use director_core::scene_export::export_scene;
use director_core::scene_import::ImportRegistry;
use director_core::timeline::audio_samples_per_video_frame;
use director_core::video_wrapper::{Encoder, EncoderMode, EncoderSettings};
use director_core::Director;
use director_html::{compile_file, register_import, CompileOptions};
use director_render::{frame_times, render_export, Frame, FramePool, FrameState, Renderer};
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

/// Pool size from `DIRECTOR_BENCH_WORKERS`, or `Renderer::pool_for` when unset.
fn workers() -> Option<usize> {
    std::env::var("DIRECTOR_BENCH_WORKERS").ok().and_then(|workers| workers.parse().ok())
}

fn pool(renderer: &Renderer, frames: usize) -> FramePool {
    workers().map_or_else(|| renderer.pool_for(frames), |workers| renderer.pool_with(workers))
}

/// Paint `times` one frame per worker per call through one warm pool, as the export does.
fn pool_frames(renderer: &Renderer, times: &[f64]) -> anyhow::Result<()> {
    let mut pool = pool(renderer, times.len());
    let mut frames = Vec::new();
    for chunk in times.chunks(pool.workers()) {
        pool.render_into(chunk, &mut frames)?;
    }
    Ok(())
}

/// Encoder floor: pipe prepainted `frames` with premixed silence through the single-pass encoder.
fn encode(frames: &[Frame], director: &Director, path: &Path, mode: EncoderMode) -> anyhow::Result<()> {
    let settings = EncoderSettings::preset_h264_yuv420p(frames[0].width as usize, frames[0].height as usize)
        .with_fps(director.fps)
        .with_encoder_mode(mode);
    let block = audio_samples_per_video_frame(director.audio_mixer.sample_rate, director.fps.max(1e-6), 2);
    let mut encoder = Encoder::with_audio(path, settings, &vec![0.0; block * frames.len()])?;
    for (frame, time) in frames.iter().zip(frame_times(director)) {
        encoder.encode(&frame.rgba, time)?;
    }
    encoder.finish()
}

/// Serial frame loop without encoding: layout, paint and readback.
fn serial_render_only(director: &mut Director) -> anyhow::Result<()> {
    let info = skia_safe::ImageInfo::new(
        (director.width, director.height),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        Some(skia_safe::ColorSpace::new_srgb()),
    );
    let mut surface = skia_safe::surfaces::raster(&info, None, None).ok_or_else(|| anyhow::anyhow!("surface"))?;
    let mut state = FrameState::default();
    let mut pixels = vec![0u8; info.compute_min_byte_size()];
    for time in frame_times(director) {
        state.render(director, time, &mut surface)?;
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
    let serial_path = out_dir.join("serial.mp4");
    let render_path = out_dir.join("director-render.mp4");

    let probe = compiled(&input, mode)?;
    let frames = frame_times(&probe).len();
    let started = Instant::now();
    let session = renderer(&probe)?.session()?;
    let session_ms = ms(started);
    drop(session);

    let (mut serial_export, mut render_export_ms, mut serial_render, mut render_render, mut one_session) =
        (vec![], vec![], vec![], vec![], vec![]);
    let (mut live_session, mut one_call, mut first_frame, mut encode_only) = (vec![], vec![], vec![], vec![]);
    let encode_frames = {
        let mut session = renderer(&probe)?.session()?;
        frame_times(&probe).into_iter().map(|time| session.render(time)).collect::<Result<Vec<_>, _>>()?
    };
    for _ in 0..runs {
        let mut director = compiled(&input, mode)?;
        let started = Instant::now();
        render_export(&mut director, serial_path.clone(), None, None, false)?;
        serial_export.push(ms(started));

        let director = compiled(&input, mode)?;
        let document = renderer(&director)?;
        let started = Instant::now();
        document.export_with(&render_path, mode, workers())?;
        render_export_ms.push(ms(started));

        let started = Instant::now();
        encode(&encode_frames, &probe, &out_dir.join("encode-only.mp4"), mode)?;
        encode_only.push(ms(started));

        let mut director = compiled(&input, mode)?;
        let started = Instant::now();
        serial_render_only(&mut director)?;
        serial_render.push(ms(started));

        let director = compiled(&input, mode)?;
        let started = Instant::now();
        pool_frames(&renderer(&director)?, &frame_times(&director))?;
        render_render.push(ms(started));

        let director = compiled(&input, mode)?;
        let started = Instant::now();
        renderer(&director)?.render_frames(&frame_times(&director))?;
        one_call.push(ms(started));

        let director = compiled(&input, mode)?;
        let started = Instant::now();
        let mut session = renderer(&director)?.session()?;
        let mut frame = session.render(0.0)?;
        for (i, time) in frame_times(&director).into_iter().enumerate() {
            session.render_into(time, &mut frame)?;
            if i == 0 {
                first_frame.push(ms(started));
            }
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

    let (serial_export, render_export_ms) = (median(&mut serial_export), median(&mut render_export_ms));
    let (serial_render, render_render) = (median(&mut serial_render), median(&mut render_render));
    let (one_session, live_session, one_call) =
        (median(&mut one_session), median(&mut live_session), median(&mut one_call));
    let first_frame = median(&mut first_frame);
    let encode_only = median(&mut encode_only);
    println!(
        "{}",
        serde_json::json!({
            "input": input,
            "size": [probe.width, probe.height],
            "fps": probe.fps,
            "frames": frames,
            "threads": rayon::current_num_threads(),
            "workers": pool(&renderer(&probe)?, frames).workers(),
            "encoder": format!("{mode:?}"),
            "runs": runs,
            "session_import_ms": session_ms,
            "export_ms": {"encode_only": encode_only, "serial": serial_export, "director_render": render_export_ms, "speedup": serial_export / render_export_ms},
            "render_only_ms": {"serial": serial_render, "director_render": render_render, "speedup": serial_render / render_render, "one_session": one_session, "live_session": live_session, "one_call": one_call, "one_session_first_frame": first_frame},
            "render_only_fps": {"serial": frames as f64 * 1000.0 / serial_render, "director_render": frames as f64 * 1000.0 / render_render},
            "outputs": [std::fs::canonicalize(&serial_path)?, std::fs::canonicalize(&render_path)?],
        })
    );
    Ok(())
}
