//! Messages between the compiler and a recorder helper. Each frame is a
//! little-endian `u32` length and a postcard body.
//!
//! The compiler sends a [`Load`] and a [`Span`]. The helper answers with
//! [`Ask`]s: each question waits for one [`Answer`], and `Ask::Done` ends the
//! run.

use crate::{Saved, Tree};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Script {
    pub name: String,
    pub source: String,
    /// The GSAP major version a CDN URL names.
    pub major: Option<u32>,
}

/// The page and its scripts.
#[derive(Debug, Serialize, Deserialize)]
pub struct Load {
    pub page: Tree,
    pub viewport: (f32, f32),
    pub scripts: Vec<Script>,
}

/// The samples one run records: `times[from..]`, after the earlier ones
/// warm it up.
#[derive(Debug, Serialize, Deserialize)]
pub struct Span {
    pub times: Vec<f64>,
    pub from: usize,
    /// Send the document back when the run ends.
    pub keep: bool,
}

/// One probed element box.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Placed {
    /// x, y relative to the parent box, then width and height.
    pub rect: [f32; 4],
    /// Position relative to the canvas.
    pub abs: [f32; 2],
    pub font_size: f32,
    /// `position: absolute` or `fixed`.
    pub positioned: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Ask {
    /// Probe a compacted page; the answer is [`Reply::Placed`] by arena index.
    Probe(Tree),
    /// `getComputedStyle` text of `prop` for node `node` of the last probe,
    /// or the initial value for `tag` when the probe holds no such box.
    Computed { node: Option<u32>, tag: String, prop: String },
    /// The computed matrix of a CSS transform at a box size.
    Matrix { value: String, size: (f32, f32) },
    /// The length of SVG path data, or the point at a distance along it.
    Path { d: String, at: Option<f32> },
    Media(String),
    Beat(f64),
    BeatAt(f64),
    BeatGrid,
    Done(Result<Finish, String>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Finish {
    /// The change record `dom.js` returns for each sample.
    pub records: Vec<String>,
    /// Random numbers the scripts drew while the timeline played.
    pub draws: u64,
    /// The document, with each probed box as its node in the last probe.
    pub page: Option<Saved<u32>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Reply {
    Placed(Vec<(u32, Placed)>),
    Text(String),
    Number(f64),
    Numbers(Vec<f32>),
    Flag(bool),
}

pub type Answer = Result<Reply, String>;

/// Write one frame and flush it.
pub fn write<T: Serialize>(out: &mut impl Write, message: &T) -> io::Result<()> {
    let body = postcard::to_stdvec(message).map_err(io::Error::other)?;
    let mut frame = Vec::with_capacity(body.len() + 4);
    frame.extend((body.len() as u32).to_le_bytes());
    frame.extend(body);
    out.write_all(&frame)?;
    out.flush()
}

/// Read one frame.
pub fn read<T: DeserializeOwned>(input: &mut impl Read) -> io::Result<T> {
    let mut len = [0; 4];
    input.read_exact(&mut len)?;
    let mut body = vec![0; u32::from_le_bytes(len) as usize];
    input.read_exact(&mut body)?;
    postcard::from_bytes(&body).map_err(io::Error::other)
}
