//! One-frame comparison against a golden PNG.

use crate::input::Input;
use crate::renderer::{self, frame_within_limit, paint_rgba};
use clap::Parser;
use director_core::video_wrapper::RenderMode;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
pub struct SnapshotArgs {
    #[command(flatten)]
    pub input: Input,
    /// Golden PNG. `--update` writes this path.
    #[arg(long)]
    pub golden: PathBuf,
    /// Time in seconds
    #[arg(short = 't', long, default_value_t = 0.0)]
    pub time: f64,
    /// A channel delta above this counts as a different pixel
    #[arg(long, default_value_t = 16)]
    pub threshold: u8,
    /// Fail when the different-pixel ratio is above this
    #[arg(long, default_value_t = 0.001)]
    pub max_diff: f64,
    /// Replace the golden with the rendered frame
    #[arg(long, default_value_t = false)]
    pub update: bool,
    #[arg(long, default_value_t = false)]
    pub gpu: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PixelDiff {
    pub different: usize,
    pub total: usize,
    pub difference: f64,
    pub diff_pixels: Vec<u8>,
}

pub fn diff_limit(max_diff: f64) -> Result<(), String> {
    if !max_diff.is_finite() || max_diff < 0.0 || max_diff > 1.0 {
        Err("max diff must be finite and in [0, 1]".into())
    } else {
        Ok(())
    }
}

pub fn side_path(path: &Path, label: &str) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let name = format!("{stem}.{label}.png");
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}

pub fn compare_rgba(
    actual: &[u8],
    expected: &[u8],
    width: u32,
    height: u32,
    expected_w: u32,
    expected_h: u32,
    threshold: u8,
) -> Result<PixelDiff, String> {
    let total = width as usize * height as usize;
    if actual.len() != total * 4 {
        return Err("actual buffer does not match its dimensions".into());
    }
    if width == 0 || height == 0 {
        return Err("frame size must be positive".into());
    }
    frame_within_limit(width as i32, height as i32)?;
    if width != expected_w || height != expected_h {
        return Err(format!(
            "size mismatch: actual {width}x{height}, golden {expected_w}x{expected_h}"
        ));
    }
    if expected.len() != total * 4 {
        return Err("golden buffer does not match its dimensions".into());
    }
    let mut different = 0usize;
    let mut diff = vec![0u8; actual.len()];
    for pixel in 0..total {
        let index = pixel * 4;
        let delta = (0..4)
            .map(|channel| actual[index + channel].abs_diff(expected[index + channel]))
            .max()
            .unwrap_or(0);
        if delta > threshold {
            different += 1;
            diff[index] = 255;
            diff[index + 3] = 255;
        }
    }
    Ok(PixelDiff {
        different,
        total,
        difference: different as f64 / total as f64,
        diff_pixels: diff,
    })
}

pub fn run(args: SnapshotArgs) -> i32 {
    if let Err(error) = diff_limit(args.max_diff) {
        eprintln!("{error}");
        return 1;
    }
    if !args.time.is_finite() || args.time < 0.0 {
        eprintln!("frame time must be finite and nonnegative");
        return 1;
    }
    let loaded = match args.input.load(RenderMode::Export) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    let mut director = match loaded.movie.director.lock() {
        Ok(director) => director,
        Err(_) => {
            eprintln!("Director lock poisoned");
            return 1;
        }
    };
    let (pixels, width, height) = match paint_rgba(&mut director, args.time, args.gpu) {
        Ok(frame) => frame,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    let info = skia_safe::ImageInfo::new(
        (width as i32, height as i32),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let png = match renderer::encode_png(info.clone(), &pixels) {
        Ok(png) => png,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    if args.update {
        if let Err(error) = std::fs::write(&args.golden, &png) {
            eprintln!("{error}");
            return 2;
        }
        println!(
            "{}",
            serde_json::json!({
                "passed": true,
                "exit_code": 0,
                "updated": true,
                "width": width,
                "height": height,
                "golden": args.golden.display().to_string(),
                "actual": serde_json::Value::Null,
                "diff": serde_json::Value::Null,
            })
        );
        return 0;
    }
    if !args.golden.is_file() {
        let actual = side_path(&args.golden, "actual");
        if let Err(error) = std::fs::write(&actual, &png) {
            eprintln!("{error}");
            return 2;
        }
        eprintln!("golden missing");
        print_report(&args, width, height, false, 2, "golden missing", None, Some(&actual), None);
        return 2;
    }
    let golden = match image::open(&args.golden) {
        Ok(image) => image.into_rgba8(),
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    let (golden_w, golden_h) = golden.dimensions();
    let compared = match compare_rgba(
        &pixels,
        golden.as_raw(),
        width,
        height,
        golden_w,
        golden_h,
        args.threshold,
    ) {
        Ok(compared) => compared,
        Err(error) => {
            let actual = side_path(&args.golden, "actual");
            if let Err(write_error) = std::fs::write(&actual, &png) {
                eprintln!("{write_error}");
                return 2;
            }
            eprintln!("{error}");
            print_report(
                &args,
                width,
                height,
                false,
                2,
                &error,
                None,
                Some(&actual),
                None,
            );
            return 2;
        }
    };
    let failed = compared.difference > args.max_diff;
    let code = if failed { 2 } else { 0 };
    let actual = side_path(&args.golden, "actual");
    let diff = side_path(&args.golden, "diff");
    if failed {
        if let Err(error) = std::fs::write(&actual, &png) {
            eprintln!("{error}");
            return 2;
        }
        let diff_png = match renderer::encode_png(info, &compared.diff_pixels) {
            Ok(png) => png,
            Err(error) => {
                eprintln!("{error}");
                return 2;
            }
        };
        if let Err(error) = std::fs::write(&diff, diff_png) {
            eprintln!("{error}");
            return 2;
        }
        eprintln!(
            "{} pixels differ ({:.4})",
            compared.different, compared.difference
        );
    }
    print_report(
        &args,
        width,
        height,
        !failed,
        code,
        if failed { "difference above max" } else { "" },
        Some(&compared),
        failed.then_some(&actual),
        failed.then_some(&diff),
    );
    code
}

fn print_report(
    args: &SnapshotArgs,
    width: u32,
    height: u32,
    passed: bool,
    code: i32,
    reason: &str,
    compared: Option<&PixelDiff>,
    actual: Option<&Path>,
    diff: Option<&Path>,
) {
    println!(
        "{}",
        serde_json::json!({
            "passed": passed,
            "exit_code": code,
            "reason": if reason.is_empty() { serde_json::Value::Null } else { serde_json::json!(reason) },
            "width": width,
            "height": height,
            "different_pixels": compared.map(|diff| diff.different),
            "total_pixels": compared.map(|diff| diff.total),
            "difference": compared.map(|diff| diff.difference),
            "threshold": args.threshold,
            "max_diff": args.max_diff,
            "golden": args.golden.display().to_string(),
            "actual": actual.map(|path| path.display().to_string()),
            "diff": diff.map(|path| path.display().to_string()),
        })
    );
}

#[cfg(test)]
mod tests {
    use super::{compare_rgba, diff_limit, side_path};
    use std::path::Path;

    #[test]
    fn agent_snapshot_counts_a_channel_delta_above_16() {
        let actual = vec![0u8, 0, 0, 255, 255, 0, 0, 255];
        let expected = vec![0u8, 0, 0, 255, 240, 0, 0, 255];
        let same = compare_rgba(&actual, &expected, 2, 1, 2, 1, 16).unwrap();
        assert_eq!(same.different, 0);
        let changed = compare_rgba(&actual, &expected, 2, 1, 2, 1, 10).unwrap();
        assert_eq!(changed.different, 1);
        assert!(changed.difference > 0.1);
        let mismatch = compare_rgba(&actual, &expected, 2, 1, 1, 1, 16).unwrap_err();
        assert!(mismatch.contains("size"), "{mismatch}");
        assert!(diff_limit(f64::NAN).is_err());
        assert_eq!(
            side_path(Path::new("frames/shot.png"), "actual"),
            Path::new("frames/shot.actual.png")
        );
    }

    #[test]
    fn agent_snapshot_matches_its_own_frame() {
        let mut compiled = director_html::compile(
            "<style>body{margin:0}div{width:64px;height:64px;background:red}</style><div></div>",
            director_html::CompileOptions {
                width: 64,
                height: 64,
                duration: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
        let (pixels, width, height) =
            crate::renderer::paint_rgba(&mut compiled.director, 0.0, false).unwrap();
        let compared = compare_rgba(&pixels, &pixels, width, height, width, height, 16).unwrap();
        assert_eq!(compared.different, 0);
        assert!(compared.difference <= 0.001);
    }

    #[test]
    fn agent_half_scale_paints_red_on_half_the_pixels() {
        let mut compiled = director_html::compile(
            "<style>body{margin:0}div{width:64px;height:64px;background:red}</style><div></div>",
            director_html::CompileOptions {
                width: 64,
                height: 64,
                duration: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
        let (width, height) =
            director_core::export::set_output_scale(&mut compiled.director, 0.5).unwrap();
        assert_eq!((width, height), (32, 32));
        assert_eq!(compiled.director.width, 64);
        let mut surface = skia_safe::surfaces::raster_n32_premul((width, height)).unwrap();
        let mut layout = director_core::systems::layout::LayoutEngine::new();
        director_core::systems::renderer::render_sampled_at_time(
            &mut compiled.director,
            &mut layout,
            0.0,
            &mut surface,
            &mut None,
            &mut None,
        )
        .unwrap();
        let info = skia_safe::ImageInfo::new(
            (width, height),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        assert!(surface.read_pixels(&info, &mut pixels, (width * 4) as usize, (0, 0)));
        let pixel = &pixels[(2 * width as usize + 2) * 4..][..4];
        assert!(
            pixel[0] > 200 && pixel[1] < 30 && pixel[2] < 30,
            "{pixel:?}"
        );
    }
}
