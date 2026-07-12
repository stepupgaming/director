//! Render a contact-sheet grid of frames from a Rhai script.
//!
//! Useful for agent workflows and quick visual inspection of a scene
//! without encoding a full video.

use clap::Parser;
use director_core::scripting::register_rhai_api;
use director_core::systems::renderer::render_frame;
use director_core::DefaultAssetLoader;
use rhai::Engine;
use skia_safe::{AlphaType, ColorSpace, ColorType, EncodedImageFormat, Paint, Rect};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "contact-sheet", about = "Render a grid of frames from a Rhai script")]
struct Cli {
    /// Path to the Rhai script
    script: PathBuf,

    /// Number of frames to sample across the scene duration
    #[arg(short = 'n', long, default_value_t = 8)]
    frames: usize,

    /// Columns in the contact sheet grid
    #[arg(short = 'c', long, default_value_t = 4)]
    columns: usize,

    /// Output PNG path (defaults to <script>_contact_sheet.png)
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,
}

fn main() {
    let cli = Cli::parse();

    if cli.frames == 0 {
        eprintln!("frames must be > 0");
        std::process::exit(1);
    }
    if cli.columns == 0 {
        eprintln!("columns must be > 0");
        std::process::exit(1);
    }

    let script = match fs::read_to_string(&cli.script) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading script '{}': {}", cli.script.display(), e);
            std::process::exit(1);
        }
    };

    let output_path = cli.output.unwrap_or_else(|| {
        let mut p = cli.script.clone();
        p.set_file_name(format!(
            "{}_contact_sheet.png",
            p.file_stem()
                .map(|s| s.to_string_lossy())
                .unwrap_or_default()
        ));
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

    let width = director.width as i32;
    let height = director.height as i32;
    let timeline_duration = director
        .timeline
        .iter()
        .map(|item| item.start_time + item.duration)
        .fold(0.0_f64, f64::max);
    let track_duration = director
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .map(|clip| clip.start_time + clip.duration)
        .fold(0.0_f64, f64::max);
    let duration = timeline_duration.max(track_duration);

    let frames = cli.frames.max(1);
    let sample_times: Vec<f64> = (0..frames)
        .map(|i| {
            if frames == 1 {
                0.0
            } else {
                (i as f64 / (frames - 1) as f64) * duration
            }
        })
        .collect();

    let cols = cli.columns.min(frames);
    let rows = (frames + cols - 1) / cols;

    let sheet_width = width * cols as i32;
    let sheet_height = height * rows as i32;

    let info = skia_safe::ImageInfo::new(
        (sheet_width, sheet_height),
        ColorType::RGBA8888,
        AlphaType::Premul,
        Some(ColorSpace::new_srgb()),
    );
    let mut surface =
        skia_safe::surfaces::raster(&info, None, None).expect("Failed to create Skia surface");

    let canvas = surface.canvas();
    canvas.clear(skia_safe::Color::BLACK);

    for (idx, &time) in sample_times.iter().enumerate() {
        let col = idx % cols;
        let row = idx / cols;
        let x = (col as i32) * width;
        let y = (row as i32) * height;

        let frame_info = skia_safe::ImageInfo::new(
            (width, height),
            ColorType::RGBA8888,
            AlphaType::Premul,
            Some(ColorSpace::new_srgb()),
        );
        let mut frame_surface =
            skia_safe::surfaces::raster(&frame_info, None, None).expect("Failed to create frame surface");

        if let Err(e) = render_frame(&mut director, time, frame_surface.canvas()) {
            eprintln!("Render error at t={}: {}", time, e);
            std::process::exit(1);
        }

        let image = frame_surface.image_snapshot();
        let paint = Paint::default();
        canvas.draw_image(&image, (x as f32, y as f32), Some(&paint));

        let border = Rect::from_xywh(x as f32, y as f32, width as f32, height as f32);
        let mut border_paint = Paint::default();
        border_paint.set_color(skia_safe::Color::from_argb(128, 255, 255, 255));
        border_paint.set_style(skia_safe::paint::Style::Stroke);
        border_paint.set_stroke_width(1.0);
        canvas.draw_rect(border, &border_paint);
    }

    let image = surface.image_snapshot();
    let data = image
        .encode(None, EncodedImageFormat::PNG, 100)
        .expect("Failed to encode PNG");

    fs::write(&output_path, data.as_bytes()).expect("Failed to write output file");

    println!(
        "Contact sheet ({} frames, {}x{}) saved to {:?}",
        frames, sheet_width, sheet_height, output_path
    );
}
