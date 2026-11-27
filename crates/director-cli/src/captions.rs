//! `captions keys|presets|sheet|pages`: the caption key table, the built-in
//! looks, a contact sheet that renders each look on the same timed words, and
//! every page an input's captions show.

use clap::{Parser, Subcommand, ValueEnum};
use director_core::node::caption_keys::{caption_reference_markdown, CAPTION_KEYS, CAPTION_PRESETS};
use director_core::scripting::{register_rhai_api, MovieHandle};
use director_core::DefaultAssetLoader;
use director_host::Session;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser, Debug)]
pub struct CaptionsArgs {
    #[command(subcommand)]
    pub command: CaptionsCommand,
}

#[derive(Subcommand, Debug)]
pub enum CaptionsCommand {
    /// Print every caption style key with its kind, default and doc
    Keys {
        #[arg(long, value_enum, default_value_t = KeysFormat::Json)]
        format: KeysFormat,
    },
    /// Print the built-in caption presets and the keys each sets
    Presets,
    /// Render each preset on the same words into one PNG
    Sheet(SheetArgs),
    /// Print every page each caption of an input shows, with its start, end and words, as JSON
    Pages(crate::input::Input),
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum KeysFormat {
    Json,
    Markdown,
}

#[derive(Parser, Debug)]
pub struct SheetArgs {
    /// PNG to write
    #[arg(short = 'o', long)]
    pub output: PathBuf,
    /// Presets to show, comma separated (default: all)
    #[arg(long, value_delimiter = ',')]
    pub preset: Vec<String>,
    /// Words to caption, timed 0.3 s apart
    #[arg(long, default_value = "Nvidia is now worth $600 billion to the whole market")]
    pub text: String,
    /// Seconds into the words to paint (default: the fourth word)
    #[arg(long)]
    pub time: Option<f64>,
    #[arg(long, default_value_t = 1080)]
    pub width: i32,
    #[arg(long, default_value_t = 1920)]
    pub height: i32,
    /// Size of each cell against the frame
    #[arg(long, default_value_t = 0.5)]
    pub scale: f32,
    /// Extra Rhai map entries for every caption, such as `emphasis_words: ["$600"]`
    #[arg(long, default_value = "")]
    pub keys: String,
}

const WORD_SECONDS: f64 = 0.3;

/// Exit 0 on success, 1 on bad arguments or a script error, 2 when painting or writing fails.
pub fn run(args: CaptionsArgs) -> i32 {
    match args.command {
        CaptionsCommand::Keys { format: KeysFormat::Markdown } => {
            print!("{}", caption_reference_markdown());
            0
        }
        CaptionsCommand::Keys { format: KeysFormat::Json } => {
            let keys: Vec<_> = CAPTION_KEYS
                .iter()
                .map(|k| {
                    json!({
                        "name": k.name,
                        "aliases": k.aliases,
                        "takes": k.kind.describe(),
                        "group": k.group,
                        "default": k.default,
                        "doc": k.doc,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&json!({ "keys": keys })).expect("keys serialize"));
            0
        }
        CaptionsCommand::Presets => {
            let presets: Vec<_> = CAPTION_PRESETS
                .iter()
                .map(|p| {
                    let values: serde_json::Map<String, serde_json::Value> = p
                        .values
                        .iter()
                        .map(|(k, v)| {
                            let value = match v.value() {
                                director_core::node::caption_keys::CaptionValue::Number(n) => json!(n),
                                director_core::node::caption_keys::CaptionValue::Bool(b) => json!(b),
                                director_core::node::caption_keys::CaptionValue::Text(t) => json!(t),
                                director_core::node::caption_keys::CaptionValue::List(l) => json!(l),
                            };
                            (k.to_string(), value)
                        })
                        .collect();
                    json!({ "name": p.name, "doc": p.doc, "keys": values })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({ "frame_height": 1920, "presets": presets }))
                    .expect("presets serialize")
            );
            0
        }
        CaptionsCommand::Pages(input) => {
            let pages = input.open().and_then(|mut session| session.caption_pages().map_err(|e| e.to_string()));
            match pages {
                Ok(captions) => {
                    println!("{}", serde_json::to_string_pretty(&json!({ "captions": captions })).expect("pages serialize"));
                    0
                }
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            }
        }
        CaptionsCommand::Sheet(args) => match sheet(&args) {
            Ok(()) => {
                println!("{}", args.output.display());
                0
            }
            Err((code, e)) => {
                eprintln!("{e}");
                code
            }
        },
    }
}

fn rhai_string(s: &str) -> String {
    serde_json::to_string(s).expect("string serializes")
}

/// The Rhai script that shows `preset` on the sheet's words.
fn sheet_script(args: &SheetArgs, preset: &str) -> String {
    let words: Vec<String> = args
        .text
        .split_whitespace()
        .enumerate()
        .map(|(i, w)| {
            let start = i as f64 * WORD_SECONDS;
            format!("#{{ text: {}, start: {start:.3}, end: {:.3} }}", rhai_string(w), start + WORD_SECONDS)
        })
        .collect();
    let duration = (words.len() as f64 * WORD_SECONDS).max(1.0) + 0.5;
    let extra = if args.keys.trim().is_empty() { String::new() } else { format!(", {}", args.keys) };
    format!(
        r##"
        let movie = new_director({w}, {h}, 30);
        let scene = movie.add_scene({duration});
        scene.add_box(#{{ width: "100%", height: "100%", position: "absolute", bg_gradient: #{{ type: "linear", angle: 160.0, colors: ["#5B6B7F", "#2B3240"] }} }});
        scene.add_text(#{{ content: {label}, position: "absolute", top: {pad}, left: {pad}, font_size: {label_size}, color: "#FFFFFFCC", font_family: "Inter" }});
        scene.add_caption(#{{ words: [{words}], preset: {name}{extra} }});
        movie
        "##,
        w = args.width,
        h = args.height,
        label = rhai_string(preset),
        name = rhai_string(preset),
        pad = args.height as f32 * 0.03,
        label_size = args.height as f32 * 0.025,
        words = words.join(", "),
    )
}

fn sheet(args: &SheetArgs) -> Result<(), (i32, String)> {
    let names: Vec<String> = if args.preset.is_empty() {
        CAPTION_PRESETS.iter().map(|p| p.name.to_string()).collect()
    } else {
        args.preset.clone()
    };
    if !(args.scale > 0.0 && args.scale <= 1.0) {
        return Err((1, format!("--scale must be in (0, 1], got {}", args.scale)));
    }
    let time = args.time.unwrap_or(3.0 * WORD_SECONDS + 0.05);
    let cell_w = ((args.width as f32 * args.scale) as i32).max(1);
    let cell_h = ((args.height as f32 * args.scale) as i32).max(1);
    let columns = names.len().clamp(1, 3) as i32;
    let rows = (names.len() as i32 + columns - 1) / columns;
    crate::renderer::frame_within_limit(cell_w * columns, cell_h * rows).map_err(|e| (1, e))?;
    let mut surface = skia_safe::surfaces::raster_n32_premul((cell_w * columns, cell_h * rows))
        .ok_or((2, "sheet surface allocation failed".to_string()))?;
    surface.canvas().clear(skia_safe::Color::BLACK);

    for (i, name) in names.iter().enumerate() {
        let mut engine = rhai::Engine::new();
        let loader = Arc::new(DefaultAssetLoader);
        register_rhai_api(&mut engine, loader.clone());
        let movie = engine
            .eval::<MovieHandle>(&sheet_script(args, name))
            .map_err(|e| (1, format!("{name}: {e}")))?;
        let document = {
            let director = movie.director.lock().map_err(|_| (2, "director lock poisoned".to_string()))?;
            director_core::scene_export::export_scene(&director).map_err(|e| (2, format!("{name}: {e}")))?
        };
        let mut session = Session::from_document(document, loader).map_err(|e| (2, format!("{name}: {e}")))?;
        let frame = crate::renderer::paint(&mut session, time).map_err(|e| (2, format!("{name}: {e}")))?;
        let info = skia_safe::ImageInfo::new(
            (frame.width as i32, frame.height as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let image = skia_safe::images::raster_from_data(
            &info,
            skia_safe::Data::new_copy(&frame.rgba),
            frame.width as usize * 4,
        )
        .ok_or((2, "frame raster allocation failed".to_string()))?;
        let (col, row) = (i as i32 % columns, i as i32 / columns);
        let dst = skia_safe::Rect::from_xywh(
            (col * cell_w) as f32,
            (row * cell_h) as f32,
            cell_w as f32,
            cell_h as f32,
        );
        surface.canvas().draw_image_rect_with_sampling_options(
            &image,
            None,
            dst,
            skia_safe::SamplingOptions::new(skia_safe::FilterMode::Linear, skia_safe::MipmapMode::Linear),
            &skia_safe::Paint::default(),
        );
    }
    let png = surface
        .image_snapshot()
        .encode(None, skia_safe::EncodedImageFormat::PNG, 100)
        .ok_or((2, "PNG encoding failed".to_string()))?;
    std::fs::write(&args.output, png.as_bytes()).map_err(|e| (2, format!("{}: {e}", args.output.display())))
}
