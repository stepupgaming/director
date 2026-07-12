// Visual regression test coverage: text, images, animations, effects, elements, layout.
use director_core::{video_wrapper::RenderMode, DefaultAssetLoader, Director};
use image::{Pixel, Rgba};
use skia_safe::{AlphaType, ColorSpace, ColorType, EncodedImageFormat};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Configuration for visual comparison
pub struct VisualConfig {
    pub tolerance: u8,  // Max channel difference (0-255) to consider "equal"
    pub threshold: f64, // Max percentage of pixels that can differ
}

impl Default for VisualConfig {
    fn default() -> Self {
        Self {
            tolerance: 2,   // Allow small rendering noises (e.g. 1-2 value diffs)
            threshold: 0.1, // 0.1% pixels allowed to differ (anti-aliasing edges)
        }
    }
}

/// Parse `@snapshot:` annotations from a Rhai fixture script.
/// Scans the first 20 lines; any `@snapshot:` in string literals is
/// extremely unlikely at the very top of a file.
/// Defaults to [0.0] if no annotation found.
pub fn parse_snapshot_times(script: &str) -> Vec<f64> {
    for line in script.lines().take(20) {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("// @snapshot:") {
            return rest
                .split(',')
                .filter_map(|s| s.trim().parse::<f64>().ok())
                .collect();
        }
    }
    vec![0.0]
}

/// Returns true if the fixture script starts with `// @skip`.
pub fn fixture_is_skipped(script: &str) -> bool {
    script.lines().next().map_or(false, |l| l.trim() == "// @skip")
}

/// Helper function to save a difference map between two images.
/// Returns `Ok(())` on success or `Err(description)` on I/O failure.
fn save_diff_image(
    reference: &image::RgbaImage,
    actual: &image::RgbaImage,
    path: &PathBuf,
    tolerance: u8,
) -> Result<(), String> {
    let width = reference.width();
    let height = reference.height();

    let mut diff_img = image::RgbaImage::new(width, height);

    for x in 0..width {
        for y in 0..height {
            let p1 = reference.get_pixel(x, y);
            let p2 = actual.get_pixel(x, y);

            if !pixels_match(p1, p2, tolerance) {
                // Mismatch: Magenta (Full Opacity)
                diff_img.put_pixel(x, y, Rgba([255, 0, 255, 255]));
            } else {
                // Match: Ghost (Dimmed Original)
                let mut dim = *p1;
                dim.0[3] = 64; // ~25% Alpha
                diff_img.put_pixel(x, y, dim);
            }
        }
    }

    diff_img
        .save(path)
        .map_err(|e| format!("Failed to save diff image to {:?}: {}", path, e))
}

fn pixels_match(p1: &Rgba<u8>, p2: &Rgba<u8>, tolerance: u8) -> bool {
    let c1 = p1.channels();
    let c2 = p2.channels();

    for i in 0..4 {
        if c1[i].abs_diff(c2[i]) > tolerance {
            return false;
        }
    }
    true
}

/// Helper to setup a Director with consistent defaults and bundled fonts
pub fn setup_test_director(width: u32, height: u32) -> Arc<Mutex<Director>> {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_max_level(tracing::Level::WARN)
        .try_init();

    use director_core::director::DirectorContext;
    use director_core::systems::assets::AssetManager;
    use director_core::AssetLoader;
    use skia_safe::textlayout::{FontCollection, TypefaceFontProvider};
    use skia_safe::{Data, FontMgr};
    use std::collections::HashMap;

    // 1. Setup Assets
    let loader = Arc::new(DefaultAssetLoader);
    let mut font_collection = FontCollection::new();
    let mut font_provider = TypefaceFontProvider::new();

    // 2. Load Bundled Font (Roboto)
    // We try to load from assets/fonts/Roboto-Regular.ttf
    // We need to ensure we can find it.
    let font_path = "fonts/Roboto-Regular.ttf";
    match loader.load_bytes(font_path) {
        Ok(bytes) => {
            let data = Data::new_copy(&bytes);
            if let Some(tf) = FontMgr::new().new_from_data(&data, 0) {
                // Register under all fallback aliases for consistent behavior
                // with the CLI path (Director::new() with None context).
                font_provider.register_typeface(tf.clone(), Some("Roboto"));
                font_provider.register_typeface(tf.clone(), Some("Sans Serif"));
                font_provider.register_typeface(tf, Some("Arial"));
                println!("Loaded bundled font: Roboto");
            } else {
                eprintln!("Failed to create Typeface from bundled font data");
            }
        }
        Err(e) => {
            eprintln!(
                "Failed to load bundled font '{}': {}. ensure assets/fonts/ exists.",
                font_path, e
            );
        }
    }

    // 3. Configure Font Collection
    font_collection.set_asset_font_manager(Some(font_provider.clone().into()));
    // Fallback to system fonts if needed, but bundled font is preferred for "Roboto"
    font_collection.set_default_font_manager(FontMgr::default(), None);

    let assets = AssetManager::new(
        loader.clone(),
        Arc::new(Mutex::new(font_collection)),
        Arc::new(Mutex::new(font_provider)),
        Arc::new(Mutex::new(HashMap::new())),
    );

    let ctx = DirectorContext { assets };

    let director = Director::new(
        width as i32,
        height as i32,
        30,
        loader,
        RenderMode::Preview,
        Some(ctx),
    );

    Arc::new(Mutex::new(director))
}

/// Visual assertion function. Returns `Ok(())` when the rendered image matches
/// the reference snapshot within tolerance, or `Err(message)` on mismatch so
/// callers can collect multiple failures instead of stopping at the first one.
///
/// When `UPDATE_SNAPSHOTS` is set, the reference is overwritten and `Ok(())`
/// is always returned.
pub fn assert_visual_match(
    director: &mut Director,
    time: f64,
    test_suite: &str,
    test_case: &str,
) -> Result<(), String> {
    let width = director.width;
    let height = director.height;

    // 1. Setup Skia Surface
    let info = skia_safe::ImageInfo::new(
        (width, height),
        ColorType::RGBA8888,
        AlphaType::Premul,
        Some(ColorSpace::new_srgb()),
    );

    let mut surface =
        skia_safe::surfaces::raster(&info, None, None).expect("Failed to create Skia surface");

    // 2. Render Frame
    director_core::systems::renderer::render_frame(director, time, surface.canvas())
        .expect("render_frame failed");

    // 3. Encode to PNG
    let image = surface.image_snapshot();
    let data = image
        .encode(None, EncodedImageFormat::PNG, 100)
        .expect("Failed to encode image to PNG");
    let rendered_bytes = data.as_bytes();

    // Paths
    // We want: tests/snapshots/{suite}_{case}.png (platform-independent)
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let snapshot_dir = PathBuf::from(manifest_dir.clone()).join("tests/snapshots");

    let snapshot_filename = format!("{}_{}.png", test_suite, test_case);
    let snapshot_path = snapshot_dir.join(&snapshot_filename);

    // 4. Handle Snapshot Update
    if env::var("UPDATE_SNAPSHOTS").is_ok() {
        if !snapshot_dir.exists() {
            fs::create_dir_all(&snapshot_dir).expect("Failed to create snapshot directory");
        }
        fs::write(&snapshot_path, rendered_bytes).expect("Failed to write snapshot");
        println!("Updated snapshot: {:?}", snapshot_path);
        return Ok(());
    }

    // 5. Load Reference
    if !snapshot_path.exists() {
        return Err(format!(
            "Snapshot not found for {}_{}. Run with UPDATE_SNAPSHOTS=1 to generate.",
            test_suite, test_case
        ));
    }

    let reference_img = image::open(&snapshot_path)
        .map_err(|e| format!("Failed to open reference snapshot {:?}: {}", snapshot_path, e))?
        .to_rgba8();

    let rendered_img = image::load_from_memory(rendered_bytes)
        .map_err(|e| format!("Failed to load rendered image: {}", e))?
        .to_rgba8();

    // 6. Compare Dimensions
    if reference_img.dimensions() != rendered_img.dimensions() {
        let fail_dir = PathBuf::from(manifest_dir.clone()).join("target/visual_regression_failures");
        if !fail_dir.exists() {
            fs::create_dir_all(&fail_dir).ok();
        }

        let actual_path = fail_dir.join(format!(
            "{}_{}_actual.png",
            test_suite, test_case
        ));
        if let Err(e) = fs::write(&actual_path, rendered_bytes) {
            return Err(format!(
                "Dimension mismatch for {}_{}! Reference: {:?}, Rendered: {:?}. Also failed to save artifact: {}",
                test_suite, test_case, reference_img.dimensions(), rendered_img.dimensions(), e
            ));
        }

        return Err(format!(
            "Dimension mismatch for {}_{}! Reference: {:?}, Rendered: {:?}. Artifact saved to {:?}",
            test_suite,
            test_case,
            reference_img.dimensions(),
            rendered_img.dimensions(),
            actual_path
        ));
    }

    // 7. Pixel Comparison with Config
    let config = VisualConfig::default();
    let mut diff_pixels: u64 = 0;
    let total_pixels = (width * height) as u64;

    for (x, y, ref_pixel) in reference_img.enumerate_pixels() {
        let render_pixel = rendered_img.get_pixel(x, y);
        if !pixels_match(ref_pixel, render_pixel, config.tolerance) {
            diff_pixels += 1;
        }
    }

    let diff_percent = (diff_pixels as f64 / total_pixels as f64) * 100.0;

    if diff_percent > config.threshold {
        println!(
            "Visual Difference for {}_{}: {:.4}% ({} / {} pixels)",
            test_suite, test_case, diff_percent, diff_pixels, total_pixels
        );

        // Save artifacts
        let fail_dir = PathBuf::from(manifest_dir).join("target/visual_regression_failures");
        if !fail_dir.exists() {
            fs::create_dir_all(&fail_dir).ok();
        }

        let actual_path = fail_dir.join(format!(
            "{}_{}_actual.png",
            test_suite, test_case
        ));
        let diff_path = fail_dir.join(format!(
            "{}_{}_diff.png",
            test_suite, test_case
        ));

        let mut artifact_notes = Vec::new();

        if let Err(e) = fs::write(&actual_path, rendered_bytes) {
            artifact_notes.push(format!("Failed to save actual artifact: {}", e));
        }
        if let Err(e) = save_diff_image(&reference_img, &rendered_img, &diff_path, config.tolerance)
        {
            artifact_notes.push(e);
        }

        let mut msg = format!(
            "Visual regression failed for {}_{}! Image differed by {:.4}%.\n  Actual: {:?}\n  Diff:   {:?}",
            test_suite,
            test_case,
            diff_percent,
            actual_path,
            diff_path
        );
        if !artifact_notes.is_empty() {
            msg.push_str("\n  Artifact notes:");
            for note in &artifact_notes {
                msg.push_str(&format!("\n    - {}", note));
            }
        }
        return Err(msg);
    }

    Ok(())
}

/// Evaluate a Rhai visual fixture script and assert snapshots at the annotated capture times.
///
/// The script must return a `MovieHandle` (i.e., end with the `movie` variable).
/// Capture times are read from a `// @snapshot: t1, t2, ...` comment at the top of the file.
/// If no annotation is present, a single snapshot at `t=0.0` is captured.
/// Evaluate a Rhai visual fixture script and assert snapshots at the annotated capture times.
///
/// The script must return a `MovieHandle` (i.e., end with the `movie` variable).
/// Capture times are read from a `// @snapshot: t1, t2, ...` comment at the top of the file.
/// If no annotation is present, a single snapshot at `t=0.0` is captured.
///
/// Returns `Ok(())` on success or `Err(message)` on failure so callers can
/// collect multiple failures instead of stopping at the first one.
pub fn run_rhai_visual_fixture(script_path: &str, test_suite: &str) -> Result<(), String> {
    use director_core::scripting::register_rhai_api;
    use director_core::AssetLoader;
    use director_core::DefaultAssetLoader;
    use rhai::Engine;
    use std::path::PathBuf;

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let fixture_path = PathBuf::from(&manifest_dir).join(script_path);
    let script = fs::read_to_string(&fixture_path)
        .map_err(|e| format!("Failed to read fixture {}: {}", fixture_path.display(), e))?;

    if fixture_is_skipped(&script) {
        println!("Skipping fixture {}", fixture_path.display());
        return Ok(());
    }

    let snapshot_times = parse_snapshot_times(&script);

    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let movie = engine
        .eval::<director_core::scripting::MovieHandle>(&script)
        .map_err(|e| format!("Fixture {} failed to evaluate: {}", fixture_path.display(), e))?;

    // Director::new() already loads the bundled Roboto font under "Roboto",
    // "Sans Serif", and "Arial" aliases. No post-eval font replacement is
    // needed here, and replacing would discard any aliases or fallback fonts
    // registered during Director construction.

    let mut director = movie.director.lock().unwrap();

    let test_case = fixture_path
        .file_stem()
        .expect("fixture must have a filename")
        .to_string_lossy()
        .to_string();

    let mut failures = Vec::new();
    for time in snapshot_times {
        let time_label = format!("{}_t{:.2}", test_case, time).replace('.', "_");
        if let Err(msg) = assert_visual_match(&mut director, time, test_suite, &time_label) {
            failures.push(msg);
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        let mut msg = format!(
            "Fixture {} had {} snapshot mismatch(es):\n",
            fixture_path.display(),
            failures.len()
        );
        for f in &failures {
            msg.push_str(&format!("  - {}\n", f));
        }
        Err(msg)
    }
}

/// Macro to generate a matrix of tests based on input variations.
///
/// # Example
///
/// ```rust
/// visual_test_matrix!(
///     name: blend_modes,
///     suite: "elements",
///     variations: [
///         (Multiply, skia_safe::BlendMode::Multiply),
///         (Screen, skia_safe::BlendMode::Screen)
///     ],
///     setup: |director, value| {
///         // setup code using `value`
///     }
/// );
/// ```
#[macro_export]
macro_rules! visual_test_matrix {
    (
        name: $matrix_name:ident,
        suite: $suite_name:expr,
        variations: [ $( ($case_name:ident, $value:expr) ),* $(,)? ],
        setup: $setup_block:expr
    ) => {
        $(
            #[test]
            fn $case_name() {
                use $crate::visual::setup_test_director;
                use $crate::visual::assert_visual_match;
                use director_core::Director;
                // use std::sync::{Arc, Mutex};

                // Setup
                let director_arc = setup_test_director(500, 500);
                let value = $value;

                // Execute User Setup
                {
                    let mut director = director_arc.lock().unwrap();
                    let setup_fn = $setup_block;
                    setup_fn(&mut director, value);
                }

            // Assert
            {
                let mut director = director_arc.lock().unwrap();
                let case_name_str = stringify!($case_name);
                assert_visual_match(&mut director, 0.0, $suite_name, case_name_str)
                    .expect("visual mismatch");
            }
            }
        )*
    };
}
