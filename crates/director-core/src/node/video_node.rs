use crate::animation::Animated;
use crate::element::Element;
use crate::errors::RenderError;
use crate::node::{calculate_object_fit_rect, parse_easing};
use crate::types::ObjectFit;
use crate::video_wrapper::{RenderMode, VideoLoader, VideoResponse};
use skia_safe::{AlphaType, Canvas, ClipOp, Color4f, ColorSpace, ColorType, Data, Image, Paint, Rect, RRect};
use std::any::Any;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use taffy::style::Style;
use tempfile::NamedTempFile;
use tracing::error;

/// Specifies the data source for a video node.
pub enum VideoSource {
    /// Load from a local file path.
    Path(PathBuf),
    /// Load from raw bytes (in-memory).
    Bytes(Vec<u8>),
}

// --- Video Node ---
/// A node that plays a video file.
///
/// Handles async decoding and frame buffering.
#[derive(Debug)]
pub struct VideoNode {
    pub opacity: Animated<f32>,
    pub style: Style,
    pub object_fit: ObjectFit,
    pub border_radius: Animated<f32>,
    /// Original asset path, if loaded from a file or blob path.
    pub source_path: Option<String>,
    current_frame: Mutex<Option<(f64, Image)>>,

    loader: Option<VideoLoader>,
    pub(crate) render_mode: RenderMode,

    // Keep temp file alive
    #[allow(dead_code)]
    temp_file: Option<Arc<NamedTempFile>>,
    // Also keep path for cloning if it was a file path
    pub(crate) path: PathBuf,
}

impl Clone for VideoNode {
    fn clone(&self) -> Self {
        let loader = if self.loader.is_some() {
            // Create new loader pointing to same file.
            VideoLoader::new(self.path.clone(), self.render_mode).ok()
        } else {
            None
        };

        Self {
            opacity: self.opacity.clone(),
            style: self.style.clone(),
            object_fit: self.object_fit,
            border_radius: self.border_radius.clone(),
            source_path: self.source_path.clone(),
            current_frame: Mutex::new(None),
            loader,
            render_mode: self.render_mode,
            temp_file: self.temp_file.clone(),
            path: self.path.clone(),
        }
    }
}

impl VideoNode {
    pub fn new(source: VideoSource, mode: RenderMode) -> Self {
        let (path, temp_file) = match source {
            VideoSource::Path(p) => (p, None),
            VideoSource::Bytes(data) => {
                let mut temp = NamedTempFile::new().expect("Failed to create temp file");
                temp.write_all(&data).expect("Failed to write video data");
                let p = temp.path().to_owned();
                (p, Some(Arc::new(temp)))
            }
        };

        let loader = match VideoLoader::new(path.clone(), mode) {
            Ok(l) => Some(l),
            Err(e) => {
                error!("Failed to create VideoLoader for {:?}: {}", path, e);
                None
            }
        };

        Self {
            opacity: Animated::new(1.0),
            style: Style::DEFAULT,
            object_fit: ObjectFit::Cover,
            border_radius: Animated::new(0.0),
            source_path: None,
            current_frame: Mutex::new(None),
            loader,
            render_mode: mode,
            temp_file,
            path,
        }
    }

    pub fn with_source_path(mut self, path: impl Into<String>) -> Self {
        self.source_path = Some(path.into());
        self
    }

    /// Returns the resolved path for this video node.
    pub fn path(&self) -> &std::path::PathBuf {
        &self.path
    }

    /// Returns the currently buffered video frame, if any.
    ///
    /// The returned tuple is `(presentation_time_seconds, image)`. This is
    /// intended for wrapper nodes such as `ActiveSpeakerCamera` that need to
    /// render the decoded frame with a custom source crop rectangle.
    pub fn get_current_frame(&self) -> Option<(f64, Image)> {
        self.current_frame.lock().unwrap().clone()
    }
}

impl Element for VideoNode {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn kind(&self) -> &'static str {
        "video"
    }

    fn layout_style(&self) -> Style {
        self.style.clone()
    }

    fn set_layout_style(&mut self, style: Style) {
        self.style = style;
    }

    fn set_opacity(&mut self, opacity: f32) {
        self.opacity.current_value = opacity;
    }

    fn update(&mut self, time: f64) -> bool {
        self.opacity.update(time);
        self.border_radius.update(time);

        // Floor seek onto a nominal source fps grid so freeze/speed share one
        // quantum (Plan 06). Source fps is not always known without probing;
        // use 30 as the default quantum — SyncDecoder still seeks by time.
        const DEFAULT_SOURCE_FPS: f64 = 30.0;
        let seek_time = crate::timeline::video_seek_time(time, DEFAULT_SOURCE_FPS);

        if let Some(loader) = &mut self.loader {
            match loader {
                VideoLoader::Threaded(decoder) => {
                    decoder.send_request(seek_time);

                    if let Some(resp) = decoder.get_response() {
                        match resp {
                            VideoResponse::Frame(t, data, w, h) => {
                                let data = Data::new_copy(&data);
                                let info = skia_safe::ImageInfo::new(
                                    (w as i32, h as i32),
                                    ColorType::RGBA8888,
                                    AlphaType::Premul,
                                    Some(ColorSpace::new_srgb()),
                                );

                                if let Some(img) = skia_safe::images::raster_from_data(
                                    &info,
                                    data,
                                    (w * 4) as usize,
                                ) {
                                    *self.current_frame.lock().unwrap() = Some((t, img));
                                } else {
                                    tracing::warn!("Failed to rasterize video frame at {:.3}s from threaded decoder ({}x{})", t, w, h);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                VideoLoader::Sync(decoder) => match decoder.get_frame_at(seek_time) {
                    Ok((t, data, w, h)) => {
                        let data = Data::new_copy(&data);
                        let info = skia_safe::ImageInfo::new(
                            (w as i32, h as i32),
                            ColorType::RGBA8888,
                            AlphaType::Premul,
                            Some(ColorSpace::new_srgb()),
                        );

                        if let Some(img) =
                            skia_safe::images::raster_from_data(&info, data, (w * 4) as usize)
                        {
                            *self.current_frame.lock().unwrap() = Some((t, img));
                        } else {
                            tracing::warn!("Failed to rasterize video frame at {:.3}s from sync decoder ({}x{})", t, w, h);
                        }
                    }
                    Err(e) => {
                        error!("Sync Video Error: {}", e);
                    }
                },
            }
        }
        true
    }

    fn render(
        &self,
        canvas: &Canvas,
        rect: Rect,
        parent_opacity: f32,
        draw_children: &mut dyn FnMut(&Canvas),
    ) -> Result<(), RenderError> {
        let op = self.opacity.current_value * parent_opacity;

        let current = self.current_frame.lock().unwrap();
        if let Some((_, img)) = current.as_ref() {
            let paint = Paint::new(Color4f::new(1.0, 1.0, 1.0, op), None);

            let draw_rect = calculate_object_fit_rect(
                img.width() as f32,
                img.height() as f32,
                rect,
                self.object_fit,
            );

            canvas.save();
            let radius = self.border_radius.current_value;
            if radius > 0.0 {
                let rrect = RRect::new_rect_xy(&rect, radius, radius);
                canvas.clip_rrect(rrect, ClipOp::Intersect, true);
            } else if self.object_fit == ObjectFit::Cover {
                canvas.clip_rect(rect, ClipOp::Intersect, true);
            }
            canvas.draw_image_rect(img, None, draw_rect, &paint);
            canvas.restore();
        } else {
            let mut p = Paint::new(Color4f::new(0.0, 0.0, 1.0, 1.0), None);
            p.set_alpha_f(op);
            canvas.draw_rect(rect, &p);
        }
        draw_children(canvas);
        Ok(())
    }

    fn animate_property(
        &mut self,
        property: &str,
        start: f32,
        target: f32,
        duration: f64,
        easing: &str,
    ) -> Result<(), String> {
        let ease_fn = crate::scripting::utils::parse_easing_result(easing)?;
        match property {
            "opacity" => self.opacity.add_segment(start, target, duration, ease_fn),
            "border_radius" => self.border_radius.add_segment(start, target, duration, ease_fn),
            other => {
                return Err(format!(
                    "unknown property '{other}' on video; valid: opacity, border_radius"
                ));
            }
        }
        Ok(())
    }

    fn animate_property_spring(
        &mut self,
        property: &str,
        start: Option<f32>,
        target: f32,
        config: crate::animation::SpringConfig,
    ) -> Result<(), String> {
        match property {
            "opacity" => {
                if let Some(s) = start {
                    self.opacity.add_spring_with_start(s, target, config);
                } else {
                    self.opacity.add_spring(target, config);
                }
            }
            "border_radius" => {
                if let Some(s) = start {
                    self.border_radius.add_spring_with_start(s, target, config);
                } else {
                    self.border_radius.add_spring(target, config);
                }
            }
            other => {
                return Err(format!(
                    "spring: unknown property '{other}' on video; valid: opacity, border_radius"
                ));
            }
        }
        Ok(())
    }
}
