use crate::input::{Encoder, Input};
use clap::{Args, Subcommand};
use std::{path::PathBuf, time::Instant};

#[derive(Debug, Args)]
pub struct HtmlArgs {
    #[command(subcommand)]
    command: HtmlCommand,
}
#[derive(Debug, Subcommand)]
enum HtmlCommand {
    /// Measure first/warm native runs, write parity frames, and optionally time MP4 export
    Benchmark {
        #[command(flatten)]
        input: Input,
        #[command(flatten)]
        rendering: Rendering,
        #[arg(long, default_value_t = 2)]
        runs: usize,
        #[arg(long)]
        frames: Option<usize>,
        #[arg(long)]
        frames_dir: Option<PathBuf>,
        #[arg(long)]
        export: Option<PathBuf>,
    },
    /// Print the beat grid of an audio file or of an HTML page's tempo or music track as JSON
    Beats {
        input: PathBuf,
        /// Known tempo of an audio file; settles octave readings such as 87 for 174 BPM
        #[arg(long)]
        bpm: Option<f64>,
        /// Beats per bar of an audio file
        #[arg(long)]
        beats_per_bar: Option<u32>,
        #[arg(long)]
        composition: Option<String>,
        /// Write the audio file with a click on every beat to this WAV, accented on downbeats and the drop
        #[arg(long)]
        clicks: Option<PathBuf>,
    },
    /// Freeze dynamic JS/GSAP/Hyper CN DOM into native scene data using a compile-time browser
    Capture {
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long)]
        modules: Option<PathBuf>,
        #[arg(long)]
        chrome: Option<PathBuf>,
        #[arg(long, default_value_t = 30.0)]
        fps: f64,
        #[arg(long)]
        duration: Option<f64>,
        #[arg(long)]
        composition: Option<String>,
    },
    /// Compile and report native scene metadata and incompatibilities
    Check(Input),
    /// Compile and export MP4 using native Skia + FFmpeg
    Render {
        #[command(flatten)]
        input: Input,
        #[command(flatten)]
        rendering: Rendering,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Print the director-timeline/1 tracks the page compiles to, from timeline JSON and translated GSAP
    Timeline(Input),
    /// Render a deterministic timestamp to PNG
    Still {
        #[command(flatten)]
        input: Input,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(short = 't', long, default_value_t = 0.0)]
        time: f64,
        /// Render on Vulkan. Requires a build with --features vulkan
        #[arg(long)]
        gpu: bool,
    },
}
#[derive(Debug, Args, Clone, Copy, Default)]
struct Rendering {
    /// Render on Vulkan. Requires a build with --features vulkan
    #[arg(long)]
    gpu: bool,
    /// MP4 encoder, independent of the rendering backend
    #[arg(long, value_enum, default_value_t = Encoder::Software)]
    encoder: Encoder,
}
pub fn run(args: HtmlArgs) {
    if let Err((code, error)) = execute(args.command) {
        eprintln!("{error}");
        std::process::exit(code);
    }
}
fn execute(command: HtmlCommand) -> Result<(), (i32, String)> {
    if let HtmlCommand::Benchmark {
        input,
        rendering,
        runs,
        frames,
        frames_dir,
        export,
    } = command
    {
        return benchmark(input, rendering, runs, frames, frames_dir, export);
    }
    if let HtmlCommand::Beats {
        input,
        bpm,
        beats_per_bar,
        composition,
        clicks,
    } = command
    {
        return beats(input, bpm, beats_per_bar, composition, clicks);
    }
    if let HtmlCommand::Capture {
        input,
        output,
        modules,
        chrome,
        fps,
        duration,
        composition,
    } = command
    {
        let tool =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../director-html/tools/capture.mjs");
        let mut node = std::process::Command::new("node");
        node.arg(tool)
            .arg("--input")
            .arg(input)
            .arg("--output")
            .arg(&output)
            .arg("--fps")
            .arg(fps.to_string());
        if let Some(modules) = modules {
            node.arg("--modules").arg(modules);
        }
        if let Some(chrome) = chrome {
            node.arg("--chrome").arg(chrome);
        }
        if let Some(duration) = duration {
            node.arg("--duration").arg(duration.to_string());
        }
        if let Some(composition) = composition {
            node.arg("--composition").arg(composition);
        }
        let status = node
            .status()
            .map_err(|e| (1, format!("compile-time extraction could not start: {e}")))?;
        if !status.success() {
            return Err((1, "compile-time extraction failed".into()));
        }
        director_html::compile_capture_file(&output).map_err(|e| (1, e.to_string()))?;
        println!("{}", output.display());
        return Ok(());
    }
    if let HtmlCommand::Timeline(input) = command {
        let compiled = input.load_html().map_err(|e| (1, e.to_string()))?;
        let timeline = compiled.timeline.unwrap_or_else(|| director_html::Timeline {
            format: director_html::TIMELINE_FORMAT.into(),
            tracks: Vec::new(),
        });
        let json = serde_json::to_string_pretty(&timeline).map_err(|e| (1, e.to_string()))?;
        println!("{json}");
        return Ok(());
    }
    let (input, output, time, rendering) = match command {
        HtmlCommand::Check(input) => (input, None, None, Rendering::default()),
        HtmlCommand::Render {
            input,
            output,
            rendering,
        } => (input, Some(output), None, rendering),
        HtmlCommand::Still {
            input,
            output,
            time,
            gpu,
        } => (
            input,
            Some(output),
            Some(time),
            Rendering {
                gpu,
                ..Default::default()
            },
        ),
        HtmlCommand::Beats { .. }
        | HtmlCommand::Capture { .. }
        | HtmlCommand::Benchmark { .. }
        | HtmlCommand::Timeline(_) => {
            unreachable!()
        }
    };
    let started = Instant::now();
    let mut compiled = input.load_html().map_err(|e| (1, e.to_string()))?;
    let compile_ms = started.elapsed().as_secs_f64() * 1000.0;
    if let Some(output) = output {
        compiled.director.encoder_mode = rendering.encoder.mode();
        let mut context = crate::gpu::required_context(rendering.gpu).map_err(|e| (2, e))?;
        if let Some(time) = time {
            if !time.is_finite() || time < 0.0 {
                return Err((1, "time must be finite and nonnegative".into()));
            }
            let mut surface = crate::gpu::render_surface(
                compiled.director.width,
                compiled.director.height,
                &mut context,
            )
            .map_err(|e| (2, e))?;
            director_render::render_frame(&mut compiled.director, time, &mut surface)
                .map_err(|e| (2, e.to_string()))?;
            let mut pixels =
                vec![0; compiled.director.width as usize * compiled.director.height as usize * 4];
            crate::gpu::read_pixels(&mut surface, &mut context, &mut pixels).map_err(|e| (2, e))?;
            write_png(surface.image_info(), &pixels, &output)?;
        } else {
            let started = Instant::now();
            crate::renderer::export_video(&mut compiled.director, &output, context.as_mut(), false, 1.0, false)
                .map_err(|e| (2, e.to_string()))?;
            eprintln!(
                "HTML export: renderer={} encoder={} export_ms={:.3}",
                if rendering.gpu { "vulkan" } else { "cpu" },
                rendering.encoder.codec(),
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
        drop(compiled);
        println!("{}", output.display());
    } else {
        println!(
            "{}",
            serde_json::json!({"source_kind":compiled.source_kind,"width":compiled.director.width,"height":compiled.director.height,"fps":compiled.director.fps,"duration":compiled.director.timeline_duration(),"nodes":compiled.node_count,"compile_ms":compile_ms,"sample_rate":compiled.sample_rate,"browser_required_for_render":false})
        );
    }
    Ok(())
}

fn benchmark(
    input: Input,
    rendering: Rendering,
    runs: usize,
    frames: Option<usize>,
    frames_dir: Option<PathBuf>,
    export: Option<PathBuf>,
) -> Result<(), (i32, String)> {
    if runs == 0 || runs > 100 {
        return Err((1, "runs must be in 1..100".into()));
    }
    let started = Instant::now();
    let mut compiled = input.load_html().map_err(|e| (1, e.to_string()))?;
    let compile_ms = started.elapsed().as_secs_f64() * 1000.0;
    compiled.director.encoder_mode = rendering.encoder.mode();
    let setup = Instant::now();
    let mut context = crate::gpu::required_context(rendering.gpu).map_err(|e| (2, e))?;
    let mut surface = crate::gpu::render_surface(
        compiled.director.width,
        compiled.director.height,
        &mut context,
    )
    .map_err(|e| (2, e))?;
    let renderer = if surface.recording_context().is_some() {
        "vulkan"
    } else {
        "cpu"
    };
    let render_setup_ms = setup.elapsed().as_secs_f64() * 1000.0;
    let mut pixels =
        vec![0; compiled.director.width as usize * compiled.director.height as usize * 4];
    let duration = compiled.director.timeline_duration();
    let count = frames.unwrap_or_else(|| {
        director_core::timeline::duration_to_frame_count(duration, compiled.director.fps) as usize
    });
    if count == 0 || count as f64 / compiled.director.fps > duration + 1.0 / compiled.director.fps {
        return Err((1, "frame count must fit the composition duration".into()));
    }
    if let Some(dir) = &frames_dir {
        std::fs::create_dir(dir)
            .map_err(|e| (2, format!("create frame directory {}: {e}", dir.display())))?;
    }
    let mut state = director_render::FrameState::default();
    let mut measurements = Vec::new();
    for run in 0..runs {
        let mut render_seconds = 0.0;
        let mut readback_seconds = 0.0;
        let wall = Instant::now();
        for frame in 0..count {
            let started = Instant::now();
            let time = frame as f64 / compiled.director.fps;
            state
                .render(&mut compiled.director, time, &mut surface)
                .map_err(|e| (2, e.to_string()))?;
            let readback = Instant::now();
            crate::gpu::read_pixels(&mut surface, &mut context, &mut pixels).map_err(|e| (2, e))?;
            readback_seconds += readback.elapsed().as_secs_f64();
            render_seconds += started.elapsed().as_secs_f64();
            if run == 0 {
                if let Some(dir) = &frames_dir {
                    write_png(
                        surface.image_info(),
                        &pixels,
                        &dir.join(format!("{frame:06}.png")),
                    )?;
                }
            }
        }
        measurements.push(serde_json::json!({"kind":if run==0{"first"}else{"warm"},"frames":count,"render_ms":render_seconds*1000.0,"readback_ms":readback_seconds*1000.0,"fps":count as f64/render_seconds,"wall_ms":wall.elapsed().as_secs_f64()*1000.0}));
    }
    let warm_started = Instant::now();
    let warm = input.load_html().map_err(|e| (1, e.to_string()))?;
    let warm_compile_ms = warm_started.elapsed().as_secs_f64() * 1000.0;
    drop(warm);
    let export_ms = if let Some(path) = export {
        let started = Instant::now();
        crate::renderer::export_video(&mut compiled.director, &path, context.as_mut(), false, 1.0, false)
            .map_err(|e| (2, e.to_string()))?;
        Some(started.elapsed().as_secs_f64() * 1000.0)
    } else {
        None
    };
    println!(
        "{}",
        serde_json::json!({"width":compiled.director.width,"height":compiled.director.height,"fps":compiled.director.fps,"duration":duration,"source_kind":compiled.source_kind,"sample_rate":compiled.sample_rate,"nodes":compiled.node_count,"compile_ms":compile_ms,"warm_compile_ms":warm_compile_ms,"render_setup_ms":render_setup_ms,"renderer":renderer,"encoder":rendering.encoder.codec(),"timing_scope":"layout, paint, submission and RGBA readback; excludes PNG encoding","runs":measurements,"export_ms":export_ms,"peak_renderer_rss_bytes":peak_memory(),"browser_required_for_render":false})
    );
    drop(compiled);
    drop(state);
    drop(surface);
    Ok(())
}

fn write_png(
    info: skia_safe::ImageInfo,
    pixels: &[u8],
    output: &std::path::Path,
) -> Result<(), (i32, String)> {
    let data = crate::renderer::encode_png(info, pixels).map_err(|e| (2, e))?;
    std::fs::write(output, data).map_err(|e| (2, e.to_string()))
}

#[cfg(windows)]
fn peak_memory() -> Option<usize> {
    use windows_sys::Win32::System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        Threading::GetCurrentProcess,
    };
    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    // Windows supplies the process lifetime high-water mark; the buffer has the exact API layout.
    let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    if ok != 0 {
        Some(counters.PeakWorkingSetSize)
    } else {
        None
    }
}

#[cfg(not(windows))]
fn peak_memory() -> Option<usize> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmHWM:")
                .and_then(|s| s.split_whitespace().next()?.parse::<usize>().ok())
                .map(|kb| kb * 1024)
        })
}

/// Print the beat grid of an audio file or HTML page, and write the click overlay.
fn beats(
    input: PathBuf,
    bpm: Option<f64>,
    beats_per_bar: Option<u32>,
    composition: Option<String>,
    clicks: Option<PathBuf>,
) -> Result<(), (i32, String)> {
    let page = input
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm"));
    let grid = if page {
        if bpm.is_some() || beats_per_bar.is_some() || clicks.is_some() {
            return Err((1, "a page sets its grid with data-bpm and data-beats-per-bar; --bpm, --beats-per-bar and --clicks read audio files".into()));
        }
        let options = director_html::CompileOptions {
            composition,
            ..Default::default()
        };
        director_html::beat_grid_file(&input, options).map_err(|e| (1, e.to_string()))?
    } else {
        let bytes = std::fs::read(&input).map_err(|e| (1, format!("{}: {e}", input.display())))?;
        let samples = director_core::audio::load_audio_bytes(&bytes, 48000).map_err(|e| (1, e.to_string()))?;
        let options = director_core::beats::BeatOptions {
            bpm,
            beats_per_bar: beats_per_bar.unwrap_or(4),
            ..Default::default()
        };
        let grid = director_core::beats::detect(&samples, 2, 48000, &options).map_err(|e| (1, e.to_string()))?;
        if let Some(clicks) = clicks {
            std::fs::write(&clicks, click_overlay(samples, &grid)).map_err(|e| (1, format!("{}: {e}", clicks.display())))?;
        }
        grid
    };
    let ms = |t: f64| (t * 10000.0).round() / 10000.0;
    let json = serde_json::json!({
        "bpm": (grid.bpm * 100.0).round() / 100.0,
        "beats_per_bar": grid.beats_per_bar,
        "beats": grid.beats.iter().map(|&t| ms(t)).collect::<Vec<_>>(),
        "downbeats": grid.downbeats().into_iter().map(ms).collect::<Vec<_>>(),
        "drop": grid.drop.map(|beat| serde_json::json!({ "beat": beat, "time": ms(grid.time(beat)) })),
    });
    println!("{}", serde_json::to_string_pretty(&json).map_err(|e| (1, e.to_string()))?);
    Ok(())
}

/// 16-bit stereo 48 kHz WAV bytes of `samples` with a short click on every
/// beat: high on downbeats, highest on the drop.
fn click_overlay(mut samples: Vec<f32>, grid: &director_core::beats::BeatGrid) -> Vec<u8> {
    const RATE: f64 = 48000.0;
    let bar = grid.beats_per_bar as i64;
    for (i, &time) in grid.beats.iter().enumerate() {
        let position = i as i64 - grid.origin as i64;
        let pitch = match position {
            p if grid.drop == Some(p as f64) => 2640.0,
            p if p.rem_euclid(bar) == 0 => 1760.0,
            _ => 1100.0,
        };
        let start = (time * RATE).round() as usize;
        for n in 0..(0.04 * RATE) as usize {
            let t = n as f64 / RATE;
            let click = (0.5 * (std::f64::consts::TAU * pitch * t).sin() * (-t * 120.0).exp()) as f32;
            for channel in 0..2 {
                if let Some(sample) = samples.get_mut((start + n) * 2 + channel) {
                    *sample += click;
                }
            }
        }
    }
    let data: Vec<u8> = samples
        .iter()
        .flat_map(|s| ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
        .collect();
    let mut wav = Vec::with_capacity(44 + data.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    for (value, bytes) in [(16, 4), (1, 2), (2, 2), (48000, 4), (48000 * 4, 4), (4, 2), (16, 2)] {
        wav.extend_from_slice(&u32::to_le_bytes(value)[..bytes]);
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
    wav.extend(data);
    wav
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        html: HtmlArgs,
    }
    #[test]
    fn parses_native_and_capture_commands() {
        for args in [
            vec!["test", "check", "scene.html"],
            vec!["test", "timeline", "scene.html"],
            vec!["test", "beats", "song.wav", "--bpm", "174", "--clicks", "clicks.wav"],
            vec!["test", "still", "scene.json", "-t", ".5", "-o", "frame.png"],
            vec!["test", "render", "scene.html", "-o", "movie.mp4"],
            vec![
                "test",
                "capture",
                "block.html",
                "-o",
                "scene.json",
                "--fps",
                "30",
            ],
            vec!["test", "benchmark", "scene.html", "--runs", "2"],
        ] {
            assert!(TestCli::try_parse_from(args).is_ok());
        }
    }
    #[test]
    fn render_requires_output() {
        assert!(TestCli::try_parse_from(["test", "render", "scene.html"]).is_err());
    }
    #[test]
    fn rendering_and_encoding_are_independent() {
        let cli = TestCli::try_parse_from([
            "test",
            "render",
            "scene.html",
            "-o",
            "movie.mp4",
            "--encoder",
            "nvenc",
        ])
        .unwrap();
        match cli.html.command {
            HtmlCommand::Render { rendering, .. } => {
                assert!(!rendering.gpu);
                assert!(matches!(rendering.encoder, Encoder::Nvenc));
            }
            _ => panic!("expected render"),
        }
        let cli = TestCli::try_parse_from(["test", "benchmark", "scene.html", "--gpu"]).unwrap();
        match cli.html.command {
            HtmlCommand::Benchmark { rendering, .. } => {
                assert!(rendering.gpu);
                assert!(matches!(rendering.encoder, Encoder::Software));
            }
            _ => panic!("expected benchmark"),
        }
    }
}
