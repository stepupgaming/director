mod gpu;
mod preview;
mod run;

use clap::{Parser, Subcommand, ValueEnum};
use director_core::export::render_export;
use director_core::scripting::MovieHandle;
use director_core::systems::renderer::{render_frame, GpuContext};
use director_core::video_wrapper::RenderMode;
use run::{default_video_output, eval_script, resolve_movie, ResolveOpts};
use skia_safe::{AlphaType, ColorSpace, ColorType, EncodedImageFormat};
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::{error, info, warn};
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
    /// Path to the Rhai script
    script: PathBuf,

    /// Output video path
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,

    /// Named composition to build
    #[arg(long)]
    composition: Option<String>,

    /// Props JSON file path or inline object string
    #[arg(long)]
    props: Option<String>,

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
    /// Path to the Rhai script
    script: PathBuf,

    /// Output PNG path
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,

    /// Time in seconds to render
    #[arg(short = 't', long, default_value_t = 0.0)]
    time: f64,

    /// Frame index (overrides --time when set)
    #[arg(long)]
    frame: Option<u64>,

    /// Named composition to build
    #[arg(long)]
    composition: Option<String>,

    /// Props JSON file path or inline object string
    #[arg(long)]
    props: Option<String>,
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
    /// Path to the Rhai script
    script: PathBuf,

    /// Named composition to build
    #[arg(long)]
    composition: Option<String>,

    /// Props JSON file path or inline object string
    #[arg(long)]
    props: Option<String>,

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

fn maybe_gpu(enable: bool) -> Option<GpuContext> {
    if !enable {
        return None;
    }
    #[cfg(feature = "vulkan")]
    {
        match gpu::create_vulkan_context() {
            Some(ctx) => {
                info!("Vulkan GPU acceleration enabled");
                Some(ctx)
            }
            None => {
                warn!("Failed to initialize Vulkan, falling back to software rendering");
                None
            }
        }
    }
    #[cfg(not(feature = "vulkan"))]
    {
        warn!("Vulkan support not compiled. Rebuild with --features=vulkan");
        let _ = gpu::create_vulkan_context();
        None
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
    info!("Initializing Director Engine...");
    info!("Script: {:?}", args.script);

    let evaluated = match eval_script(&args.script, RenderMode::Export) {
        Ok(e) => e,
        Err(e) => {
            error!("{}", e);
            std::process::exit(1);
        }
    };

    let movie = match resolve_movie(
        &evaluated,
        ResolveOpts {
            composition: args.composition.as_deref(),
            props_spec: args.props.as_deref(),
        },
    ) {
        Ok(m) => m,
        Err(e) => {
            error!("{}", e);
            std::process::exit(1);
        }
    };

    let output_path = args.output.unwrap_or_else(|| {
        default_video_output(&args.script, args.composition.as_deref(), args.alpha)
    });
    info!("Output: {:?}", output_path);

    if let Some(manifest_path) = &args.manifest {
        if let Err(e) = write_manifest(&movie, manifest_path, args.manifest_time) {
            error!("{}", e);
            std::process::exit(1);
        }
    }

    let mut gpu_context = maybe_gpu(args.gpu);
    let mut director = movie.director.lock().unwrap();
    match render_export(
        &mut director,
        output_path,
        gpu_context.as_mut(),
        None,
        args.alpha,
    ) {
        Ok(_) => info!("Render complete."),
        Err(e) => {
            error!("Render failed: {}", e);
            std::process::exit(2);
        }
    }
}

fn cmd_still(args: StillArgs) {
    let evaluated = match eval_script(&args.script, RenderMode::Preview) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    let movie = match resolve_movie(
        &evaluated,
        ResolveOpts {
            composition: args.composition.as_deref(),
            props_spec: args.props.as_deref(),
        },
    ) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    let mut director = movie.director.lock().unwrap();
    let time = if let Some(frame) = args.frame {
        director_core::timeline::frame_to_time(frame, director.fps)
    } else {
        args.time
    };

    let output_path = args.output.unwrap_or_else(|| {
        let time_label = format!("{:.3}", time).replace('.', "_");
        let mut p = args.script.clone();
        let stem = p
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "frame".into());
        let suffix = args
            .composition
            .as_deref()
            .map(|c| format!("_{}", c))
            .unwrap_or_default();
        p.set_file_name(format!("frame_{}{}_{}.png", stem, suffix, time_label));
        p
    });

    let width = director.width;
    let height = director.height;
    let info = skia_safe::ImageInfo::new(
        (width, height),
        ColorType::RGBA8888,
        AlphaType::Premul,
        Some(ColorSpace::new_srgb()),
    );
    let mut surface =
        skia_safe::surfaces::raster(&info, None, None).expect("Failed to create Skia surface");

    if let Err(e) = render_frame(&mut director, time, surface.canvas()) {
        eprintln!("Render error at t={}: {}", time, e);
        std::process::exit(2);
    }

    let image = surface.image_snapshot();
    let data = image
        .encode(None, EncodedImageFormat::PNG, 100)
        .expect("Failed to encode PNG");

    if let Err(e) = fs::write(&output_path, data.as_bytes()) {
        eprintln!("Failed to write {}: {}", output_path.display(), e);
        std::process::exit(2);
    }

    println!("Frame at t={} saved to {:?}", time, output_path);
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
        script: args.script,
        composition: args.composition,
        props: args.props,
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
                script,
                output: cli.output,
                composition: cli.composition,
                props: cli.props,
                alpha: cli.alpha,
                gpu: cli.gpu,
                manifest: cli.manifest,
                manifest_time: cli.manifest_time,
            });
        }
    }
}
