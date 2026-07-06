use clap::Parser;
use director_core::scripting::register_rhai_api;
use director_core::systems::renderer::render_frame;
use director_core::DefaultAssetLoader;
use rhai::Engine;
use skia_safe::{AlphaType, ColorSpace, ColorType, EncodedImageFormat};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "preview-frame", about = "Render a single frame from a Rhai script")]
struct Cli {
    /// Path to the Rhai script
    script: PathBuf,

    /// Time in seconds to render
    #[arg(short = 't', long, default_value_t = 0.0)]
    time: f64,

    /// Output PNG path (defaults to frame_{time}.png next to script)
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,
}

fn main() {
    let cli = Cli::parse();

    let script = match fs::read_to_string(&cli.script) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading script '{}': {}", cli.script.display(), e);
            std::process::exit(1);
        }
    };

    let output_path = cli.output.unwrap_or_else(|| {
        let time_label = format!("{:.3}", cli.time).replace('.', "_");
        let mut p = cli.script.clone();
        p.set_file_name(format!(
            "frame_{}{}",
            time_label,
            p.extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default()
        ));
        p.set_extension("png");
        p
    });

    let mut engine = Engine::new();
    register_rhai_api(&mut engine, Arc::new(DefaultAssetLoader));

    let movie = match engine.eval::<director_core::scripting::MovieHandle>(&script) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Script error: {}", e);
            std::process::exit(1);
        }
    };

    let mut director = movie.director.lock().unwrap();

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

    if let Err(e) = render_frame(&mut director, cli.time, surface.canvas()) {
        eprintln!("Render error at t={}: {}", cli.time, e);
        std::process::exit(1);
    }

    let image = surface.image_snapshot();
    let data = image
        .encode(None, EncodedImageFormat::PNG, 100)
        .expect("Failed to encode PNG");

    fs::write(&output_path, data.as_bytes()).expect("Failed to write output file");

    println!("Frame at t={} saved to {:?}", cli.time, output_path);
}
