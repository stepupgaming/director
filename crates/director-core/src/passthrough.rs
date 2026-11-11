//! Passthrough of a base video. An export whose frame bottom is one opaque
//! video filling the output leaves that video inside FFmpeg, which decodes,
//! composites and encodes it, while the engine paints only what lies above.
//!
//! A frame painted between [`begin`] and [`end`] on one thread is an overlay
//! frame. A video node that fills the output on the base layer then skips its
//! decode, clears the surface under it and reports the source frame it stands
//! for. Paint that reads what lies behind it calls [`reads_backdrop`], which
//! voids the frame, because the video is absent from the surface.

use std::cell::RefCell;
use std::path::PathBuf;

/// The source frame a base video stands for in one overlay frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Punch {
    pub path: PathBuf,
    /// Source frame index the video would have shown.
    pub index: u64,
    /// Probed source frames per second.
    pub fps: f64,
    pub color: crate::video_wrapper::SourceColor,
}

struct Frame {
    /// Address of the output surface's pixels: paint on any other layer or
    /// raster composites later, so a video there cannot be the base.
    pixels: usize,
    size: (i32, i32),
    punch: Option<Punch>,
    void: bool,
}

thread_local! {
    static FRAME: RefCell<Option<Frame>> = const { RefCell::new(None) };
}

/// Start an overlay frame on this thread, painted into the surface whose
/// pixels sit at `pixels` and span `size`.
pub fn begin(pixels: usize, size: (i32, i32)) {
    FRAME.with(|frame| *frame.borrow_mut() = Some(Frame { pixels, size, punch: None, void: false }));
}

/// End the overlay frame. `Some` holds the base video the surface was cleared
/// under last; `None` means the frame has no base or paint read behind itself.
pub fn end() -> Option<Punch> {
    FRAME.with(|frame| frame.borrow_mut().take()).and_then(|frame| if frame.void { None } else { frame.punch })
}

/// The output surface of the overlay frame this thread paints, if any.
pub(crate) fn surface() -> Option<(usize, (i32, i32))> {
    FRAME.with(|frame| frame.borrow().as_ref().map(|frame| (frame.pixels, frame.size)))
}

/// Record that a base video cleared the surface. Whatever painted earlier is
/// gone with it, so an earlier backdrop read no longer matters.
pub(crate) fn punch(punch: Punch) {
    FRAME.with(|frame| {
        if let Some(frame) = frame.borrow_mut().as_mut() {
            frame.punch = Some(punch);
            frame.void = false;
        }
    });
}

/// Call before paint that reads the pixels behind it: a backdrop filter or a
/// blend mode other than source-over.
pub fn reads_backdrop() {
    FRAME.with(|frame| {
        if let Some(frame) = frame.borrow_mut().as_mut() {
            frame.void = true;
        }
    });
}
