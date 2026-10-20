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
        HtmlCommand::Capture { .. } | HtmlCommand::Benchmark { .. } => unreachable!(),
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
            director_core::systems::renderer::render_frame(
                &mut compiled.director,
                time,
                surface.canvas(),
            )
            .map_err(|e| (2, e.to_string()))?;
            let mut pixels =
                vec![0; compiled.director.width as usize * compiled.director.height as usize * 4];
            crate::gpu::read_pixels(&mut surface, &mut context, &mut pixels).map_err(|e| (2, e))?;
            write_png(surface.image_info(), &pixels, &output)?;
        } else {
            let started = Instant::now();
            director_core::export::render_export(
                &mut compiled.director,
                output.clone(),
                context.as_mut(),
                None,
                false,
            )
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
    let mut layout = director_core::systems::layout::LayoutEngine::new();
    let mut surfaces = None;
    let mut measurements = Vec::new();
    for run in 0..runs {
        let mut render_seconds = 0.0;
        let mut readback_seconds = 0.0;
        let wall = Instant::now();
        for frame in 0..count {
            let started = Instant::now();
            let time = frame as f64 / compiled.director.fps;
            director_core::systems::renderer::render_at_time(
                &mut compiled.director,
                &mut layout,
                time,
                surface.canvas(),
                &mut surfaces,
            )
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
        director_core::export::render_export(
            &mut compiled.director,
            path,
            context.as_mut(),
            None,
            false,
        )
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
    drop(surfaces);
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
