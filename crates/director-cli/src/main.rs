mod gpu;
mod html;
mod input;
mod preview;
mod project;
mod renderer;
mod run;

use clap::{Parser, Subcommand, ValueEnum};
use director_core::export::render_export;
use director_core::scripting::MovieHandle;
use director_core::video_wrapper::RenderMode;
use input::{Encoder, Input};
use run::{default_video_output, eval_script};
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
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
    /// Pack or edit a portable native project
    Project(project::ProjectArgs),
    /// Inspect source IDs, layout, timeline and audio as JSON
    Inspect(InspectArgs),
    /// Render a script (or named composition) to video
    Render(RenderArgs),
    /// Render a single still frame to PNG
    Still(StillArgs),
    /// List compositions registered by a script
    Compositions(CompositionsArgs),
    /// Live web preview (scrub / play frames in the browser)
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

    /// Open the default browser to the preview URL
    #[arg(long, default_value_t = true)]
    open: bool,

    /// Do not open a browser
    #[arg(long, default_value_t = false)]
    no_open: bool,
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
    if let Err(e) = render_export(
        &mut director,
        output.clone(),
        context.as_mut(),
        None,
        args.alpha,
    ) {
        eprintln!("{e}");
        std::process::exit(2);
    }
    drop(director);
    drop(loaded);
    drop(context);
    println!("{}", output.display());
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
    let open_browser = args.open && !args.no_open;
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
        open_browser,
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
        Some(Commands::Still(args)) => cmd_still(args),
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
                         Or:    director-engine render|still|compositions|preview --help\n\
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
            });
        }
    }
}
