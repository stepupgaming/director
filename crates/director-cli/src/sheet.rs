//! Contact sheet for every input `director-engine` already loads.

use crate::input::Input;
use crate::renderer::{self, Renderer};
use clap::Parser;
use director_core::video_wrapper::RenderMode;
use director_core::Director;
use std::path::PathBuf;

pub const MAX_SHEET_FRAMES: usize = 64;
pub const MAX_SHEET_PIXELS: usize = 16 * 1024 * 1024;

#[derive(Parser, Debug)]
pub struct SheetArgs {
    #[command(flatten)]
    pub input: Input,
    /// Frames sampled across the duration
    #[arg(short = 'n', long, default_value_t = 8)]
    pub frames: usize,
    #[arg(short = 'c', long, default_value_t = 4)]
    pub columns: usize,
    /// Width of one cell. 0 keeps the source width.
    #[arg(long, default_value_t = 320)]
    pub cell_width: i32,
    #[arg(short = 'o', long)]
    pub output: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    pub gpu: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrameSlot {
    pub index: usize,
    pub time: f64,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SheetPlan {
    pub columns: usize,
    pub rows: usize,
    pub cell_width: i32,
    pub cell_height: i32,
    pub frames: Vec<FrameSlot>,
}

pub fn plan_sheet(
    duration: f64,
    frame_count: usize,
    columns: usize,
    src_w: i32,
    src_h: i32,
    cell_width: i32,
) -> Result<SheetPlan, String> {
    if frame_count == 0 {
        return Err("frames must be > 0".into());
    }
    if frame_count > MAX_SHEET_FRAMES {
        return Err(format!("frames must be <= {MAX_SHEET_FRAMES}"));
    }
    if columns == 0 {
        return Err("columns must be > 0".into());
    }
    if !duration.is_finite() {
        return Err("duration must be finite".into());
    }
    if src_w <= 0 || src_h <= 0 {
        return Err("source size must be positive".into());
    }
    if cell_width < 0 {
        return Err("cell width must be >= 0".into());
    }
    let cell_w = if cell_width == 0 { src_w } else { cell_width };
    let cell_h = ((src_h as i64 * cell_w as i64) / src_w as i64).max(1);
    if cell_h > i64::from(i32::MAX) {
        return Err(format!("sheet exceeds {MAX_SHEET_PIXELS} pixels"));
    }
    let cell_h = cell_h as i32;
    let rows = frame_count.div_ceil(columns);
    let sheet_w = columns as i64 * i64::from(cell_w);
    let sheet_h = rows as i64 * i64::from(cell_h);
    sheet_raster_size(sheet_w, sheet_h)?;
    let frames = (0..frame_count)
        .map(|index| {
            let time = if duration.is_finite() && duration > 0.0 {
                index as f64 * duration / frame_count as f64
            } else {
                0.0
            };
            FrameSlot {
                index,
                time,
                x: (index % columns) as i32 * cell_w,
                y: (index / columns) as i32 * cell_h,
            }
        })
        .collect();
    Ok(SheetPlan {
        columns,
        rows,
        cell_width: cell_w,
        cell_height: cell_h,
        frames,
    })
}

pub fn sheet_raster_size(width: i64, height: i64) -> Result<(i32, i32), String> {
    if width <= 0 || height <= 0 || width > i64::from(i32::MAX) || height > i64::from(i32::MAX) {
        return Err(format!("sheet exceeds {MAX_SHEET_PIXELS} pixels"));
    }
    let pixels = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| format!("sheet exceeds {MAX_SHEET_PIXELS} pixels"))?;
    if pixels > MAX_SHEET_PIXELS {
        return Err(format!("sheet exceeds {MAX_SHEET_PIXELS} pixels"));
    }
    Ok((width as i32, height as i32))
}

pub fn render_sheet_png(director: &mut Director, plan: &SheetPlan, gpu: bool) -> Result<Vec<u8>, String> {
    let mut renderer = Renderer::new(gpu)?;
    let (sheet_w, sheet_h) = sheet_raster_size(
        plan.columns as i64 * i64::from(plan.cell_width),
        plan.rows as i64 * i64::from(plan.cell_height),
    )?;
    let bytes = (sheet_w as usize)
        .checked_mul(sheet_h as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| format!("sheet exceeds {MAX_SHEET_PIXELS} pixels"))?;
    let mut pixels = vec![0u8; bytes];
    for slot in &plan.frames {
        let png = renderer.png(director, slot.time)?;
        let decoded = image::load_from_memory(&png).map_err(|e| e.to_string())?;
        let cell = image::imageops::resize(
            &decoded,
            plan.cell_width as u32,
            plan.cell_height as u32,
            image::imageops::FilterType::Nearest,
        );
        blit(
            &mut pixels,
            sheet_w,
            sheet_h,
            &cell,
            slot.x,
            slot.y,
        );
    }
    let info = skia_safe::ImageInfo::new(
        (sheet_w, sheet_h),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    renderer::encode_png(info, &pixels)
}

pub fn run(args: SheetArgs) -> i32 {
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
            return 2;
        }
    };
    let duration = director.timeline_duration();
    let plan = match plan_sheet(
        duration,
        args.frames,
        args.columns,
        director.width,
        director.height,
        args.cell_width,
    ) {
        Ok(plan) => plan,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    let png = match render_sheet_png(&mut director, &plan, args.gpu) {
        Ok(png) => png,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    let output = args.output.unwrap_or_else(|| {
        let stem = args
            .input
            .input
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        args.input.input.with_file_name(format!("{stem}_sheet.png"))
    });
    if let Err(error) = std::fs::write(&output, png) {
        eprintln!("{error}");
        return 2;
    }
    let frames = plan
        .frames
        .iter()
        .map(|frame| {
            serde_json::json!({
                "index": frame.index,
                "time": frame.time,
                "x": frame.x,
                "y": frame.y,
            })
        })
        .collect::<Vec<_>>();
    println!(
        "{}",
        serde_json::json!({
            "output": output.display().to_string(),
            "columns": plan.columns,
            "rows": plan.rows,
            "cell_width": plan.cell_width,
            "cell_height": plan.cell_height,
            "frames": frames,
        })
    );
    0
}

fn blit(
    dst: &mut [u8],
    dst_w: i32,
    dst_h: i32,
    src: &image::RgbaImage,
    x0: i32,
    y0: i32,
) {
    for y in 0..src.height() {
        for x in 0..src.width() {
            let dx = x0 + x as i32;
            let dy = y0 + y as i32;
            if dx < 0 || dy < 0 || dx >= dst_w || dy >= dst_h {
                continue;
            }
            let i = (dy as usize * dst_w as usize + dx as usize) * 4;
            dst[i..i + 4].copy_from_slice(&src.get_pixel(x, y).0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{plan_sheet, render_sheet_png};

    #[test]
    fn agent_sheet_plan_splits_duration_and_grid() {
        let plan = plan_sheet(1.0, 4, 2, 64, 32, 16).unwrap();
        assert_eq!(plan.cell_width, 16);
        assert_eq!(plan.cell_height, 8);
        assert_eq!(plan.rows, 2);
        assert_eq!(
            plan.frames.iter().map(|frame| frame.time).collect::<Vec<_>>(),
            vec![0.0, 0.25, 0.5, 0.75]
        );
        assert_eq!(plan.frames[1].x, 16);
        assert_eq!(plan.frames[2].y, 8);
        assert!(plan_sheet(1.0, 0, 2, 64, 32, 16).is_err());
        assert!(plan_sheet(f64::NAN, 2, 2, 64, 32, 16).is_err());
        let huge = plan_sheet(1.0, 64, 1, 4096, 4096, 4096).unwrap_err();
        assert!(huge.contains("pixels") || huge.contains("frames"), "{huge}");
        let too_many = plan_sheet(1.0, 65, 1, 16, 16, 8).unwrap_err();
        assert!(too_many.contains("frames"), "{too_many}");
    }

    #[test]
    fn agent_sheet_renders_a_red_html_frame_into_two_cells() {
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
        let plan = plan_sheet(
            compiled.director.timeline_duration(),
            2,
            2,
            compiled.director.width,
            compiled.director.height,
            32,
        )
        .unwrap();
        let png = render_sheet_png(&mut compiled.director, &plan, false).unwrap();
        let image = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(image.width(), 64);
        assert_eq!(image.height(), plan.cell_height as u32);
        let left = image.get_pixel(2, 2).0;
        let right = image.get_pixel(34, 2).0;
        assert!(left[0] > 200 && left[1] < 30 && left[2] < 30, "{left:?}");
        assert!(right[0] > 200 && right[1] < 30 && right[2] < 30, "{right:?}");
    }
}
