//! Golden frames live in `tests/visual/golden/<module>/<name>.png`,
//! resolved from `CARGO_MANIFEST_DIR`.
//! Set `DIRECTOR_UPDATE_GOLDENS=1` to write or replace them.
//! A missing golden fails.
use director_core::video_wrapper::RenderMode;
use director_core::{AssetLoader, Director};
use skia_safe::{AlphaType, ColorSpace, ColorType, EncodedImageFormat};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Per-pixel channel delta plus the fraction of pixels allowed to exceed it.
///
/// A pixel counts as different when any channel's absolute delta is above
/// `channel_delta`. The frame fails when `different / total` is above
/// `max_fraction`.
pub struct VisualConfig {
    pub channel_delta: u8,
    pub max_fraction: f64,
}

impl Default for VisualConfig {
    fn default() -> Self {
        Self {
            channel_delta: 16,
            max_fraction: 0.001,
        }
    }
}

pub struct PixelDiff {
    pub different: u64,
    pub total: u64,
    pub fraction: f64,
}

const GENERATE_HINT: &str = "Set DIRECTOR_UPDATE_GOLDENS=1 and run: cargo test -p director-core --release --test visual_suite";

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn workspace_root() -> PathBuf {
    manifest_dir().join("..").join("..")
}

fn updating_goldens() -> bool {
    std::env::var("DIRECTOR_UPDATE_GOLDENS")
        .map(|value| value.trim() == "1")
        .unwrap_or(false)
}

pub fn golden_path(module: &str, name: &str) -> PathBuf {
    manifest_dir()
        .join("tests")
        .join("visual")
        .join("golden")
        .join(module)
        .join(format!("{name}.png"))
}

fn artifact_dir(module: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(std::env::temp_dir);
    base.join("director-visual").join(module)
}

struct ManifestAssetLoader;

impl ManifestAssetLoader {
    fn candidates(&self, path: &str) -> Vec<PathBuf> {
        let requested = PathBuf::from(path);
        let manifest = manifest_dir();
        let workspace = workspace_root();
        let mut found = Vec::new();
        if requested.is_absolute() {
            found.push(requested.clone());
        }
        found.push(manifest.join("assets").join(&requested));
        found.push(manifest.join(&requested));
        found.push(
            workspace
                .join("crates")
                .join("director-core")
                .join("assets")
                .join(&requested),
        );
        found.push(workspace.join(&requested));
        if !requested.is_absolute() {
            found.push(requested);
        }
        found
    }
}

impl AssetLoader for ManifestAssetLoader {
    fn load_bytes(&self, path: &str) -> anyhow::Result<Vec<u8>> {
        let candidates = self.candidates(path);
        for candidate in &candidates {
            if let Ok(bytes) = std::fs::read(candidate) {
                return Ok(bytes);
            }
        }
        let listed = candidates
            .iter()
            .map(|candidate| candidate.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!("asset not found: {path} (checked {listed})")
    }
}

/// Parse `@snapshot:` annotations from a Rhai fixture script.
/// Scans the first 20 lines. Defaults to [0.0] if no annotation is found.
pub fn parse_snapshot_times(script: &str) -> Vec<f64> {
    for line in script.lines().take(20) {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("// @snapshot:") {
            return rest
                .split(',')
                .filter_map(|part| part.trim().parse::<f64>().ok())
                .collect();
        }
    }
    vec![0.0]
}

/// Returns true if the fixture script starts with `// @skip`.
pub fn fixture_is_skipped(script: &str) -> bool {
    script
        .lines()
        .next()
        .is_some_and(|line| line.trim() == "// @skip")
}

fn channel_delta(left: [u8; 4], right: [u8; 4]) -> u8 {
    left.iter()
        .zip(right.iter())
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0)
}

/// Compare tightly packed RGBA buffers of `width` by `height`.
pub fn compare_rgba(
    actual: &[u8],
    expected: &[u8],
    width: u32,
    height: u32,
    channel_delta_limit: u8,
) -> Result<PixelDiff, String> {
    if width == 0 || height == 0 {
        return Err("frame size must be positive".into());
    }
    let total = (width as u64)
        .checked_mul(height as u64)
        .ok_or_else(|| "frame is too large".to_string())?;
    let bytes = (total as usize)
        .checked_mul(4)
        .ok_or_else(|| "frame is too large".to_string())?;
    if actual.len() != bytes || expected.len() != bytes {
        return Err(format!(
            "buffer length {} or {} does not match {width}x{height}",
            actual.len(),
            expected.len()
        ));
    }
    let mut different = 0u64;
    for pixel in 0..total as usize {
        let index = pixel * 4;
        let left = [
            actual[index],
            actual[index + 1],
            actual[index + 2],
            actual[index + 3],
        ];
        let right = [
            expected[index],
            expected[index + 1],
            expected[index + 2],
            expected[index + 3],
        ];
        if channel_delta(left, right) > channel_delta_limit {
            different += 1;
        }
    }
    Ok(PixelDiff {
        different,
        total,
        fraction: different as f64 / total as f64,
    })
}

pub fn exceeds_tolerance(diff: &PixelDiff, max_fraction: f64) -> bool {
    diff.fraction > max_fraction
}

fn save_diff_image(
    reference: &image::RgbaImage,
    actual: &image::RgbaImage,
    path: &Path,
    channel_delta_limit: u8,
) -> Result<(), String> {
    let mut diff_img = image::RgbaImage::new(reference.width(), reference.height());
    for (x, y, ref_px) in reference.enumerate_pixels() {
        let actual_px = actual.get_pixel(x, y);
        if channel_delta(ref_px.0, actual_px.0) > channel_delta_limit {
            diff_img.put_pixel(x, y, image::Rgba([255, 0, 255, 255]));
        } else {
            let mut dim = *ref_px;
            dim.0[3] = 64;
            diff_img.put_pixel(x, y, dim);
        }
    }
    diff_img.save(path).map_err(|err| {
        format!("failed to save diff image {}: {err}", path.display())
    })
}

fn require_bundled_fonts(director: &Director) -> Result<(), String> {
    director.ensure_required_fonts().map_err(|err| {
        let fonts = manifest_dir().join("assets").join("fonts");
        format!(
            "{err}. Bundled fonts resolve from {}.",
            fonts.display()
        )
    })
}

fn render_png(director: &mut Director, time: f64) -> Result<Vec<u8>, String> {
    let info = skia_safe::ImageInfo::new(
        (director.width, director.height),
        ColorType::RGBA8888,
        AlphaType::Premul,
        Some(ColorSpace::new_srgb()),
    );
    let mut surface = skia_safe::surfaces::raster(&info, None, None)
        .ok_or_else(|| "failed to create Skia surface".to_string())?;
    let mut transition_surfaces = if director.transitions.is_empty() {
        None
    } else {
        let first = skia_safe::surfaces::raster(&info, None, None)
            .ok_or_else(|| "failed to create transition surface".to_string())?;
        let second = skia_safe::surfaces::raster(&info, None, None)
            .ok_or_else(|| "failed to create transition surface".to_string())?;
        Some((first, second))
    };
    let mut layout = director_core::systems::layout::LayoutEngine::new();
    director_core::systems::renderer::render_at_time(
        director,
        &mut layout,
        time,
        surface.canvas(),
        &mut transition_surfaces,
    )
    .map_err(|err| format!("render_at_time failed: {err}"))?;
    let image = surface.image_snapshot();
    let data = image
        .encode(None, EncodedImageFormat::PNG, 100)
        .ok_or_else(|| "failed to encode PNG".to_string())?;
    Ok(data.as_bytes().to_vec())
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }
    std::fs::write(path, bytes)
        .map_err(|err| format!("failed to write {}: {err}", path.display()))
}

/// Build a director whose asset loader reads bundled fonts from the crate
/// manifest (`assets/fonts`) and the workspace root derived from it.
pub fn setup_test_director(width: u32, height: u32) -> Arc<Mutex<Director>> {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_max_level(tracing::Level::WARN)
        .try_init();

    let loader: Arc<dyn AssetLoader> = Arc::new(ManifestAssetLoader);
    let director = Director::new(
        width as i32,
        height as i32,
        30.0,
        loader,
        RenderMode::Preview,
        None,
    );
    Arc::new(Mutex::new(director))
}

/// Compare `module`/`name` using [`VisualConfig::default`].
pub fn assert_visual_match(
    director: &mut Director,
    time: f64,
    module: &str,
    name: &str,
) -> Result<(), String> {
    assert_visual_match_with(director, time, module, name, &VisualConfig::default())
}

/// Compare `module`/`name` with a per-test tolerance override.
pub fn assert_visual_match_with(
    director: &mut Director,
    time: f64,
    module: &str,
    name: &str,
    config: &VisualConfig,
) -> Result<(), String> {
    require_bundled_fonts(director)?;
    let rendered_bytes = render_png(director, time)?;
    let golden = golden_path(module, name);

    if updating_goldens() {
        write_bytes(&golden, &rendered_bytes)?;
        println!("updated golden {}", golden.display());
        return Ok(());
    }

    let actual_path = artifact_dir(module).join(format!("{name}.actual.png"));
    let diff_path = artifact_dir(module).join(format!("{name}.diff.png"));

    if !golden.is_file() {
        let mut notes = Vec::new();
        if let Err(err) = write_bytes(&actual_path, &rendered_bytes) {
            notes.push(err);
        } else {
            eprintln!("actual: {}", actual_path.display());
        }
        let mut message = format!(
            "missing golden {}\n{GENERATE_HINT}\nactual: {}",
            golden.display(),
            actual_path.display()
        );
        for note in notes {
            message.push_str("\n");
            message.push_str(&note);
        }
        return Err(message);
    }

    let reference = image::open(&golden)
        .map_err(|err| format!("failed to open golden {}: {err}", golden.display()))?
        .to_rgba8();
    let rendered = image::load_from_memory(&rendered_bytes)
        .map_err(|err| format!("failed to decode rendered frame: {err}"))?
        .to_rgba8();

    if reference.dimensions() != rendered.dimensions() {
        write_bytes(&actual_path, &rendered_bytes)?;
        eprintln!("actual: {}", actual_path.display());
        return Err(format!(
            "size mismatch for {module}/{name}: golden {:?} rendered {:?}\nactual: {}\n{GENERATE_HINT}",
            reference.dimensions(),
            rendered.dimensions(),
            actual_path.display()
        ));
    }

    let (width, height) = rendered.dimensions();
    let diff = compare_rgba(
        rendered.as_raw(),
        reference.as_raw(),
        width,
        height,
        config.channel_delta,
    )?;
    if exceeds_tolerance(&diff, config.max_fraction) {
        write_bytes(&actual_path, &rendered_bytes)?;
        save_diff_image(&reference, &rendered, &diff_path, config.channel_delta)?;
        eprintln!("actual: {}", actual_path.display());
        eprintln!("diff: {}", diff_path.display());
        return Err(format!(
            "visual mismatch for {module}/{name}: {:.6} of pixels differ ({} / {}), channel delta {}, max fraction {}\nactual: {}\ndiff: {}\ngolden: {}",
            diff.fraction,
            diff.different,
            diff.total,
            config.channel_delta,
            config.max_fraction,
            actual_path.display(),
            diff_path.display(),
            golden.display()
        ));
    }
    Ok(())
}

/// Evaluate a Rhai visual fixture and compare snapshots at the annotated times.
///
/// The script must return a `MovieHandle`. Capture times come from
/// `// @snapshot: t1, t2`. With no annotation, one frame at `t=0.0` is captured.
pub fn run_rhai_visual_fixture(script_path: &str, module: &str) -> Result<(), String> {
    use director_core::scripting::register_rhai_api;
    use rhai::Engine;

    let fixture_path = manifest_dir().join(script_path);
    let script = std::fs::read_to_string(&fixture_path)
        .map_err(|err| format!("failed to read fixture {}: {err}", fixture_path.display()))?;

    if fixture_is_skipped(&script) {
        println!("skipping fixture {}", fixture_path.display());
        return Ok(());
    }

    let snapshot_times = parse_snapshot_times(&script);
    let mut engine = Engine::new();
    let loader: Arc<dyn AssetLoader> = Arc::new(ManifestAssetLoader);
    register_rhai_api(&mut engine, loader);

    let movie = engine
        .eval::<director_core::scripting::MovieHandle>(&script)
        .map_err(|err| format!("fixture {} failed to evaluate: {err}", fixture_path.display()))?;

    let mut director = movie
        .director
        .lock()
        .map_err(|_| "director lock poisoned".to_string())?;

    let test_case = fixture_path
        .file_stem()
        .ok_or_else(|| "fixture must have a filename".to_string())?
        .to_string_lossy()
        .to_string();

    let mut failures = Vec::new();
    for time in snapshot_times {
        let time_label = format!("{test_case}_t{time:.2}").replace('.', "_");
        if let Err(message) = assert_visual_match(&mut director, time, module, &time_label) {
            failures.push(message);
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        let mut message = format!(
            "fixture {} had {} snapshot mismatch(es):\n",
            fixture_path.display(),
            failures.len()
        );
        for failure in &failures {
            message.push_str(&format!("  - {failure}\n"));
        }
        Err(message)
    }
}

/// A one-pixel edit must fail a zero-tolerance compare. No golden file is used.
#[test]
fn harness_rejects_one_pixel_change() {
    use director_core::animation::Animated;
    use director_core::node::BoxNode;
    use director_core::types::Color;
    use image::Rgba;
    use taffy::style::Dimension;

    let director_arc = setup_test_director(8, 8);
    let mut director = director_arc.lock().expect("director lock");

    let mut root = BoxNode::new();
    root.style.size.width = Dimension::percent(1.0);
    root.style.size.height = Dimension::percent(1.0);
    root.bg_color = Some(Animated::new(Color::new(0.8, 0.1, 0.1, 1.0)));
    let root_id = director.scene.add_node(Box::new(root));
    director.push_scene_root(root_id, 1.0);

    let png = render_png(&mut director, 0.0).expect("render tiny scene");
    let image = image::load_from_memory(&png)
        .expect("decode tiny scene")
        .to_rgba8();
    let (width, height) = image.dimensions();

    let same = compare_rgba(image.as_raw(), image.as_raw(), width, height, 0).expect("compare");
    assert!(
        !exceeds_tolerance(&same, 0.0),
        "identical frames must pass zero tolerance"
    );

    let mut mutated = image.clone();
    let before = mutated.get_pixel(3, 3).0;
    let forced = if before[0] > 127 { 0 } else { 255 };
    let delta = before[0].abs_diff(forced);
    assert!(delta >= 128, "mutation delta {delta} is too small");
    mutated.put_pixel(3, 3, Rgba([forced, before[1], before[2], before[3]]));

    let diff = compare_rgba(mutated.as_raw(), image.as_raw(), width, height, 0).expect("compare");
    assert!(
        exceeds_tolerance(&diff, 0.0),
        "one-pixel change must fail zero tolerance"
    );
    assert_eq!(diff.different, 1);
}

/// Macro to generate a matrix of tests based on input variations.
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

                let _matrix = stringify!($matrix_name);
                let director_arc = setup_test_director(500, 500);
                let value = $value;

                {
                    let mut director = director_arc.lock().unwrap();
                    let setup_fn = $setup_block;
                    setup_fn(&mut director, value);
                }

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
