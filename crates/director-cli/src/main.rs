mod audio_report;
mod batch;
mod check;
mod gpu;
mod html;
mod input;
mod preview;
mod project;
mod renderer;
mod run;
mod onion;
mod sample_times;
mod sheet;
mod snapshot;

use clap::{Parser, Subcommand, ValueEnum};
use director_core::scripting::MovieHandle;
use director_core::video_wrapper::RenderMode;
use input::{Encoder, Input};
use run::{default_video_output, eval_script};
use std::fs;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use tracing::{error, info};
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Parser, Debug)]
#[command(
    name = "director-engine",
    author,
    version,
    about = "Director rendering CLI",
    long_about = None,
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Legacy: path to the Rhai script (when not using a subcommand)
    #[arg(value_name = "SCRIPT")]
    script: Option<PathBuf>,

    /// Legacy: output video path
    #[arg(value_name = "OUTPUT")]
    output: Option<PathBuf>,

    /// Enable alpha-channel output (VP9/WebM with transparency)
    #[arg(long, default_value_t = false)]
    alpha: bool,

    /// Enable Vulkan GPU-accelerated rendering
    #[arg(long, default_value_t = false)]
    gpu: bool,

    /// Log level
    #[arg(long, value_enum, default_value_t = LogLevel::Info)]
    log_level: LogLevel,

    /// Log format
    #[arg(long, value_enum, default_value_t = LogFormat::Pretty)]
    log_format: LogFormat,

    /// Write layout-time render manifest JSON alongside the video
    #[arg(long, value_name = "PATH")]
    manifest: Option<PathBuf>,

    /// Timeline instant (seconds) for manifest bounds sampling
    #[arg(long, default_value_t = 0.0)]
    manifest_time: f64,

    /// Named composition (legacy bare invocation)
    #[arg(long)]
    composition: Option<String>,

    /// Props JSON file path or inline object (legacy bare invocation)
    #[arg(long)]
    props: Option<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Compile HTML/CSS/HyperFrames to native scenes, capture dynamic input, or render
    Html(html::HtmlArgs),
    /// Pack, edit, or repair a portable native project
    Project(project::ProjectArgs),
    /// Inspect source IDs, layout, timeline and audio as JSON
    Inspect(InspectArgs),
    /// Render a script (or named composition) to video
    Render(RenderArgs),
    /// Render each input with the software encoder into one directory
    Batch(BatchArgs),
    /// Render a single still frame to PNG
    Still(StillArgs),
    /// Render a contact sheet of frames from any loaded input
    Sheet(sheet::SheetArgs),
    /// Report loudness, clipping, and silence for loaded audio
    Audio(audio_report::AudioArgs),
    /// Report elements outside the canvas and missing fonts
    Check(check::CheckArgs),
    /// Compare one frame to a golden PNG
    Snapshot(snapshot::SnapshotArgs),
    /// Blend frames so later frames count more
    Onion(onion::OnionArgs),
    /// List compositions registered by a script
    Compositions(CompositionsArgs),
    /// Preview API server for editors and agents (frames, inspect, audio, reload)
    Preview(CliPreviewArgs),
}

#[derive(Parser, Debug)]
struct RenderArgs {
    #[command(flatten)]
    input: Input,

    /// Output video path
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,

    #[arg(long, value_enum, default_value_t = Encoder::Software)]
    encoder: Encoder,

    /// Enable alpha-channel output (VP9/WebM with transparency)
    #[arg(long, default_value_t = false)]
    alpha: bool,

    /// Enable Vulkan GPU-accelerated rendering
    #[arg(long, default_value_t = false)]
    gpu: bool,

    /// Write layout-time render manifest JSON
    #[arg(long, value_name = "PATH")]
    manifest: Option<PathBuf>,

    /// Timeline instant (seconds) for manifest bounds sampling
    #[arg(long, default_value_t = 0.0)]
    manifest_time: f64,

    /// Draft raster scale in (0, 1]. Layout stays at the composition size.
    #[arg(long, default_value_t = 1.0)]
    scale: f32,

    /// Software x264 preset ultrafast and CRF 30. Raster size stays on --scale.
    #[arg(long, default_value_t = false)]
    draft_encode: bool,
}

#[derive(Parser, Debug)]
struct BatchArgs {
    /// Input path. Repeat for each item.
    #[arg(long, required = true)]
    input: Vec<PathBuf>,

    /// Directory that receives one software-encoded video per input
    #[arg(short, long)]
    output: PathBuf,

    /// Emit one JSON object instead of ok/fail lines
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Parser, Debug)]
struct StillArgs {
    #[command(flatten)]
    input: Input,

    /// Output PNG path
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,

    /// Time in seconds to render
    #[arg(short = 't', long, default_value_t = 0.0)]
    time: f64,

    /// Frame index (overrides --time when set)
    #[arg(long)]
    frame: Option<u64>,

    #[arg(long)]
    gpu: bool,
}

#[derive(Parser, Debug)]
struct InspectArgs {
    #[command(flatten)]
    input: Input,
    #[arg(short = 't', long, default_value_t = 0.0)]
    time: f64,
    /// Sample time in seconds. Repeat for a samples array. Omit to inspect --time once.
    #[arg(long, value_name = "SECONDS", action = clap::ArgAction::Append)]
    sample: Vec<f64>,
}

#[derive(Parser, Debug)]
struct CompositionsArgs {
    /// Path to the Rhai script
    script: PathBuf,

    /// Emit JSON instead of a human table
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Parser, Debug)]
struct CliPreviewArgs {
    #[command(flatten)]
    input: Input,
    #[arg(long)]
    gpu: bool,

    /// Bind address (default 127.0.0.1:7788)
    #[arg(long, default_value = "127.0.0.1:7788")]
    bind: String,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Error => write!(f, "error"),
            LogLevel::Warn => write!(f, "warn"),
            LogLevel::Info => write!(f, "info"),
            LogLevel::Debug => write!(f, "debug"),
            LogLevel::Trace => write!(f, "trace"),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum LogFormat {
    Pretty,
    Json,
}

fn init_logging(level: LogLevel, format: LogFormat) {
    let filter = EnvFilter::builder()
        .with_default_directive(level.to_string().parse().unwrap())
        .from_env_lossy();

    let subscriber_builder = fmt::Subscriber::builder()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false);

    match format {
        LogFormat::Json => {
            subscriber_builder.json().init();
        }
        LogFormat::Pretty => {
            subscriber_builder.pretty().init();
        }
    }
}

fn write_manifest(movie: &MovieHandle, path: &PathBuf, time: f64) -> Result<(), String> {
    let mut director = movie
        .director
        .lock()
        .map_err(|_| "Director lock poisoned".to_string())?;
    let json = director_core::render_manifest_json(&mut director, time);
    fs::write(path, json).map_err(|e| format!("failed to write manifest: {}", e))?;
    info!("Wrote render manifest to {:?}", path);
    Ok(())
}

fn cmd_render(args: RenderArgs) {
    if let Err(error) = director_render::validate_scale(args.scale) {
        eprintln!("{error}");
        std::process::exit(1);
    }
    if args.alpha && matches!(args.encoder, Encoder::Nvenc) {
        eprintln!("alpha export requires VP9/WebM; --encoder nvenc selects H.264 without alpha");
        std::process::exit(1);
    }
    let loaded = args.input.load(RenderMode::Export).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    let output = args.output.unwrap_or_else(|| {
        default_video_output(
            &args.input.input,
            args.input.composition.as_deref(),
            args.alpha,
        )
    });
    if let Some(path) = &args.manifest {
        if let Err(e) = write_manifest(&loaded.movie, path, args.manifest_time) {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
    let mut context = gpu::required_context(args.gpu).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    let mut director = loaded.movie.director.lock().unwrap_or_else(|_| {
        eprintln!("Director lock poisoned");
        std::process::exit(2);
    });
    director.encoder_mode = args.encoder.mode();
    if let Err(e) = renderer::export_video(
        &mut director,
        &output,
        context.as_mut(),
        args.alpha,
        args.scale,
        args.draft_encode,
    ) {
        eprintln!("{e}");
        std::process::exit(2);
    }
    drop(director);
    drop(loaded);
    println!("{}", output.display());
}

fn cmd_batch(args: BatchArgs) -> i32 {
    if let Err(error) = fs::create_dir_all(&args.output) {
        eprintln!("output {}: {error}", args.output.display());
        return flush_status(2);
    }
    let plans = batch::plan_batch(&args.input, &args.output);
    let mut items = Vec::with_capacity(plans.len());
    for plan in plans {
        let name = plan.input.display().to_string();
        let output = plan.output.display().to_string();
        match render_software_item(&plan.input, &plan.output) {
            Ok(()) => items.push(batch::BatchItem {
                name,
                output,
                error: None,
                exit_code: 0,
            }),
            Err(failure) => items.push(batch::BatchItem {
                name,
                output,
                error: Some(failure.message),
                exit_code: failure.exit_code,
            }),
        }
    }
    if args.json {
        let report = batch::batch_report_json(&items);
        println!("{}", report.json);
        return flush_status(report.exit_code);
    }
    let report = batch::batch_report(&items);
    print!("{}", report.text);
    flush_status(report.exit_code)
}

struct ItemFailure {
    exit_code: i32,
    message: String,
}

/// Software encoder, CPU paint, no alpha. Same export entry as `render`.
fn render_software_item(input_path: &Path, output: &Path) -> Result<(), ItemFailure> {
    let input = Input::new(input_path.to_path_buf());
    let loaded = input.load(RenderMode::Export).map_err(|message| ItemFailure {
        exit_code: 1,
        message,
    })?;
    let mut context = gpu::required_context(false).map_err(|message| ItemFailure {
        exit_code: 2,
        message,
    })?;
    let mut director = loaded.movie.director.lock().map_err(|_| ItemFailure {
        exit_code: 2,
        message: "Director lock poisoned".into(),
    })?;
    director.encoder_mode = Encoder::Software.mode();
    renderer::export_video(&mut director, output, context.as_mut(), false, 1.0, false)
    .map_err(|error| ItemFailure {
        exit_code: 2,
        message: error.to_string(),
    })?;
    drop(director);
    drop(loaded);
    Ok(())
}

fn flush_status(code: i32) -> i32 {
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    code
}

fn cmd_still(args: StillArgs) {
    let loaded = args.input.load(RenderMode::Export).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    let mut director = loaded.movie.director.lock().unwrap_or_else(|_| {
        eprintln!("Director lock poisoned");
        std::process::exit(2);
    });
    let time = args.frame.map_or(args.time, |f| {
        director_core::timeline::frame_to_time(f, director.fps)
    });
    let output = args.output.unwrap_or_else(|| {
        let time_label = format!("{time:.3}").replace('.', "_");
        let stem = args
            .input
            .input
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        let suffix = args
            .input
            .composition
            .as_ref()
            .map(|c| format!("_{c}"))
            .unwrap_or_default();
        args.input
            .input
            .with_file_name(format!("frame_{stem}{suffix}_{time_label}.png"))
    });
    let mut renderer = renderer::Renderer::new(args.gpu).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    let png = renderer.png(&mut director, time).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    if let Err(e) = fs::write(&output, png) {
        eprintln!("{e}");
        std::process::exit(2);
    }
    drop(director);
    drop(loaded);
    drop(renderer);
    println!("{}", output.display());
}

fn cmd_inspect(args: InspectArgs) {
    let plan = match sample_times::check_sample_times(&args.sample) {
        Ok(plan) => plan,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    match plan {
        sample_times::SamplePlan::Single => {
            let inspection = args
                .input
                .load(RenderMode::Export)
                .and_then(|loaded| loaded.inspect(args.time))
                .unwrap_or_else(|e| {
                    eprintln!("{e}");
                    std::process::exit(1);
                });
            println!("{inspection}");
        }
        sample_times::SamplePlan::Repeated(times) => {
            let loaded = args.input.load(RenderMode::Export).unwrap_or_else(|e| {
                eprintln!("{e}");
                std::process::exit(1);
            });
            let mut samples = Vec::with_capacity(times.len());
            for time in times {
                match loaded.inspect(time) {
                    Ok(inspection) => samples.push(serde_json::json!({
                        "time": time,
                        "inspection": inspection,
                    })),
                    Err(error) => {
                        eprintln!("{error}");
                        std::process::exit(1);
                    }
                }
            }
            println!("{}", serde_json::json!({ "samples": samples }));
        }
    }
}
fn cmd_compositions(args: CompositionsArgs) {
    let evaluated = match eval_script(&args.script, RenderMode::Preview) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    if evaluated.registry.is_empty() {
        eprintln!(
            "No compositions registered by {}.\n\
             Call register_composition(name, meta, |props| {{ ... }}) in the script.",
            args.script.display()
        );
        std::process::exit(1);
    }

    if args.json {
        println!("{}", evaluated.registry.to_json_string_pretty());
        return;
    }

    println!(
        "{:<24} {:>5} {:>5} {:>8} {:>10}",
        "NAME", "W", "H", "FPS", "DURATION"
    );
    for entry in evaluated.registry.entries() {
        println!(
            "{:<24} {:>5} {:>5} {:>8.3} {:>10.2}",
            entry.name, entry.width, entry.height, entry.fps, entry.duration
        );
    }
}

fn cmd_preview(args: CliPreviewArgs) {
    let bind: SocketAddr = match args.bind.parse() {
        Ok(a) => a,
        Err(e) => {
            error!("invalid --bind '{}': {}", args.bind, e);
            std::process::exit(1);
        }
    };
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            error!("failed to start tokio runtime: {}", e);
            std::process::exit(1);
        }
    };
    if let Err(e) = rt.block_on(preview::run_preview(preview::PreviewArgs {
        input: args.input,
        gpu: args.gpu,
        bind,
    })) {
        error!("{}", e);
        std::process::exit(1);
    }
}

fn main() {
    let cli = Cli::parse();

    init_logging(cli.log_level, cli.log_format);

    match cli.command {
        Some(Commands::Render(args)) => cmd_render(args),
        Some(Commands::Batch(args)) => {
            let code = cmd_batch(args);
            if code != 0 {
                std::process::exit(code);
            }
        }
        Some(Commands::Still(args)) => cmd_still(args),
        Some(Commands::Sheet(args)) => {
            let code = sheet::run(args);
            if code != 0 {
                std::process::exit(code);
            }
        }
        Some(Commands::Audio(args)) => {
            let code = audio_report::run(args);
            if code != 0 {
                std::process::exit(code);
            }
        }
        Some(Commands::Check(args)) => {
            let code = check::run(args);
            if code != 0 {
                std::process::exit(code);
            }
        }
        Some(Commands::Snapshot(args)) => {
            let code = snapshot::run(args);
            if code != 0 {
                std::process::exit(code);
            }
        }
        Some(Commands::Onion(args)) => {
            let code = onion::run(args);
            if code != 0 {
                std::process::exit(code);
            }
        }
        Some(Commands::Compositions(args)) => cmd_compositions(args),
        Some(Commands::Preview(args)) => cmd_preview(args),
        Some(Commands::Html(args)) => html::run(args),
        Some(Commands::Project(args)) => project::run(args),
        Some(Commands::Inspect(args)) => cmd_inspect(args),
        None => {
            let script = match cli.script {
                Some(s) => s,
                None => {
                    eprintln!(
                        "Usage: director-engine <SCRIPT> [OUTPUT]\n\
                         Or:    director-engine render|still|sheet|audio|check|snapshot|onion|batch|compositions|preview --help\n\
                         Note: a script named 'render.rhai' needs the explicit form:\n\
                               director-engine render render.rhai"
                    );
                    std::process::exit(1);
                }
            };
            cmd_render(RenderArgs {
                input: Input {
                    composition: cli.composition,
                    props: cli.props,
                    ..Input::new(script)
                },
                output: cli.output,
                encoder: Encoder::Software,
                alpha: cli.alpha,
                gpu: cli.gpu,
                manifest: cli.manifest,
                manifest_time: cli.manifest_time,
                scale: 1.0,
                draft_encode: false,
            });
        }
    }
}

#[cfg(test)]
mod draft_encode_tests {
    use super::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn draft_encode_stays_off_the_scale_flag() {
        let scaled = Cli::try_parse_from([
            "director-engine",
            "render",
            "shot.html",
            "--scale",
            "0.5",
        ])
        .expect("scale parse");
        let Commands::Render(args) = scaled.command.expect("render") else {
            panic!("render");
        };
        assert!(!args.draft_encode);
        assert!((args.scale - 0.5).abs() < 1e-6);

        let drafted = Cli::try_parse_from([
            "director-engine",
            "render",
            "shot.html",
            "--draft-encode",
        ])
        .expect("draft parse");
        let Commands::Render(args) = drafted.command.expect("render") else {
            panic!("render");
        };
        assert!(args.draft_encode);
        assert!((args.scale - 1.0).abs() < 1e-6);
    }
}
