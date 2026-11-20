//! Video and audio on Android through the NDK media APIs (MediaExtractor,
//! MediaCodec, MediaMuxer) in place of the `ffmpeg` and `ffprobe` binaries,
//! which an Android app cannot run. The rest of the crate sees the same
//! probe, decode and [`Encoder`] interface as on the desktop:
//!
//! - [`probe`] and [`duration`] read a container's track formats.
//! - [`spawn_decode`] decodes a video track from a seek time onto a frame grid
//!   as RGBA, as `ffmpeg -ss <t> -i <file> -vf fps=<fps>:start_time=0` does.
//! - [`decode_audio`] decodes the first sound track to interleaved stereo f32.
//! - [`Encoder`] writes H.264 + AAC MP4 with the device's hardware encoders.
#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use crossbeam_channel::{Receiver, Sender};

use crate::video_wrapper::{EncoderSettings, VideoProbe};

#[repr(C)]
struct AMediaCodec {
    _p: [u8; 0],
}
#[repr(C)]
struct AMediaFormat {
    _p: [u8; 0],
}
#[repr(C)]
struct AMediaMuxer {
    _p: [u8; 0],
}
#[repr(C)]
struct AMediaExtractor {
    _p: [u8; 0],
}

/// `AMediaCodecBufferInfo`.
#[repr(C)]
#[derive(Default, Clone, Copy, Debug)]
struct BufInfo {
    offset: i32,
    size: i32,
    pts_us: i64,
    flags: u32,
}

const FLAG_CODEC_CONFIG: u32 = 2;
const FLAG_END_OF_STREAM: u32 = 4;
const INFO_TRY_AGAIN: isize = -1;
const INFO_FORMAT_CHANGED: isize = -2;
const INFO_BUFFERS_CHANGED: isize = -3;
const SEEK_PREVIOUS_SYNC: c_int = 0;
/// `MediaCodecInfo.CodecCapabilities` colour formats.
const COLOR_YUV420_PLANAR: i32 = 19;
const COLOR_YUV420_SEMI_PLANAR: i32 = 21;
const COLOR_YUV420_FLEXIBLE: i32 = 0x7F42_0888;
/// Qualcomm's NV12 with 128-byte stride and 32-row slice alignment.
const COLOR_QCOM_NV12_32M: i32 = 0x7FA3_0C04;
const COLOR_QCOM_NV12: i32 = 0x7FA3_0C00;

#[link(name = "mediandk")]
extern "C" {
    fn AMediaCodec_createEncoderByType(mime: *const c_char) -> *mut AMediaCodec;
    fn AMediaCodec_createDecoderByType(mime: *const c_char) -> *mut AMediaCodec;
    fn AMediaCodec_createCodecByName(name: *const c_char) -> *mut AMediaCodec;
    fn AMediaCodec_configure(
        c: *mut AMediaCodec,
        f: *const AMediaFormat,
        surface: *mut c_void,
        crypto: *mut c_void,
        flags: u32,
    ) -> c_int;
    fn AMediaCodec_start(c: *mut AMediaCodec) -> c_int;
    fn AMediaCodec_stop(c: *mut AMediaCodec) -> c_int;
    fn AMediaCodec_delete(c: *mut AMediaCodec) -> c_int;
    fn AMediaCodec_getInputFormat(c: *mut AMediaCodec) -> *mut AMediaFormat;
    fn AMediaCodec_getName(c: *mut AMediaCodec, out: *mut *mut c_char) -> c_int;
    fn AMediaCodec_releaseName(c: *mut AMediaCodec, name: *mut c_char);
    fn AMediaCodec_dequeueInputBuffer(c: *mut AMediaCodec, timeout_us: i64) -> isize;
    fn AMediaCodec_getInputBuffer(c: *mut AMediaCodec, idx: usize, size: *mut usize) -> *mut u8;
    fn AMediaCodec_queueInputBuffer(c: *mut AMediaCodec, idx: usize, off: i64, size: usize, pts: u64, flags: u32)
        -> c_int;
    fn AMediaCodec_dequeueOutputBuffer(c: *mut AMediaCodec, info: *mut BufInfo, timeout_us: i64) -> isize;
    fn AMediaCodec_getOutputFormat(c: *mut AMediaCodec) -> *mut AMediaFormat;
    fn AMediaCodec_getOutputBuffer(c: *mut AMediaCodec, idx: usize, size: *mut usize) -> *mut u8;
    fn AMediaCodec_releaseOutputBuffer(c: *mut AMediaCodec, idx: usize, render: bool) -> c_int;

    fn AMediaFormat_new() -> *mut AMediaFormat;
    fn AMediaFormat_delete(f: *mut AMediaFormat) -> c_int;
    fn AMediaFormat_setString(f: *mut AMediaFormat, k: *const c_char, v: *const c_char);
    fn AMediaFormat_setInt32(f: *mut AMediaFormat, k: *const c_char, v: i32);
    fn AMediaFormat_getInt32(f: *mut AMediaFormat, k: *const c_char, out: *mut i32) -> bool;
    fn AMediaFormat_getInt64(f: *mut AMediaFormat, k: *const c_char, out: *mut i64) -> bool;
    fn AMediaFormat_getBuffer(f: *mut AMediaFormat, k: *const c_char, data: *mut *mut c_void, size: *mut usize) -> bool;
    fn AMediaFormat_getFloat(f: *mut AMediaFormat, k: *const c_char, out: *mut f32) -> bool;
    fn AMediaFormat_getString(f: *mut AMediaFormat, k: *const c_char, out: *mut *const c_char) -> bool;
    fn AMediaFormat_getRect(
        f: *mut AMediaFormat,
        k: *const c_char,
        left: *mut i32,
        top: *mut i32,
        right: *mut i32,
        bottom: *mut i32,
    ) -> bool;

    fn AMediaMuxer_new(fd: c_int, format: c_int) -> *mut AMediaMuxer;
    fn AMediaMuxer_addTrack(m: *mut AMediaMuxer, f: *const AMediaFormat) -> isize;
    fn AMediaMuxer_start(m: *mut AMediaMuxer) -> c_int;
    fn AMediaMuxer_writeSampleData(m: *mut AMediaMuxer, track: usize, data: *const u8, info: *const BufInfo) -> c_int;
    fn AMediaMuxer_stop(m: *mut AMediaMuxer) -> c_int;
    fn AMediaMuxer_delete(m: *mut AMediaMuxer) -> c_int;

    fn AMediaExtractor_new() -> *mut AMediaExtractor;
    fn AMediaExtractor_delete(e: *mut AMediaExtractor) -> c_int;
    fn AMediaExtractor_setDataSourceFd(e: *mut AMediaExtractor, fd: c_int, offset: i64, length: i64) -> c_int;
    fn AMediaExtractor_getTrackCount(e: *mut AMediaExtractor) -> usize;
    fn AMediaExtractor_getTrackFormat(e: *mut AMediaExtractor, idx: usize) -> *mut AMediaFormat;
    fn AMediaExtractor_selectTrack(e: *mut AMediaExtractor, idx: usize) -> c_int;
    fn AMediaExtractor_readSampleData(e: *mut AMediaExtractor, buffer: *mut u8, capacity: usize) -> isize;
    fn AMediaExtractor_getSampleTime(e: *mut AMediaExtractor) -> i64;
    fn AMediaExtractor_advance(e: *mut AMediaExtractor) -> bool;
    fn AMediaExtractor_seekTo(e: *mut AMediaExtractor, pos_us: i64, mode: c_int) -> c_int;
}

fn key(s: &str) -> CString {
    CString::new(s).expect("format keys have no NUL")
}

/// An owned `AMediaFormat`.
struct Format(*mut AMediaFormat);

impl Format {
    fn new() -> Self {
        Self(unsafe { AMediaFormat_new() })
    }

    fn owned(ptr: *mut AMediaFormat) -> Option<Self> {
        (!ptr.is_null()).then_some(Self(ptr))
    }

    fn set_str(&self, k: &str, v: &str) {
        let v = CString::new(v).expect("format values have no NUL");
        unsafe { AMediaFormat_setString(self.0, key(k).as_ptr(), v.as_ptr()) }
    }

    fn set_i32(&self, k: &str, v: i32) {
        unsafe { AMediaFormat_setInt32(self.0, key(k).as_ptr(), v) }
    }

    fn i32(&self, k: &str) -> Option<i32> {
        let mut out = 0;
        unsafe { AMediaFormat_getInt32(self.0, key(k).as_ptr(), &mut out) }.then_some(out)
    }

    fn i64(&self, k: &str) -> Option<i64> {
        let mut out = 0;
        unsafe { AMediaFormat_getInt64(self.0, key(k).as_ptr(), &mut out) }.then_some(out)
    }

    fn f32(&self, k: &str) -> Option<f32> {
        let mut out = 0.0;
        unsafe { AMediaFormat_getFloat(self.0, key(k).as_ptr(), &mut out) }.then_some(out)
    }

    /// The bytes under `k`, copied out: the format owns the buffer.
    fn bytes(&self, k: &str) -> Option<Vec<u8>> {
        let (mut data, mut size): (*mut c_void, usize) = (std::ptr::null_mut(), 0);
        let found = unsafe { AMediaFormat_getBuffer(self.0, key(k).as_ptr(), &mut data, &mut size) };
        (found && !data.is_null()).then(|| unsafe { std::slice::from_raw_parts(data as *const u8, size) }.to_vec())
    }

    /// The string under `k`, copied out: the format owns the pointer.
    fn string(&self, k: &str) -> Option<String> {
        let mut out: *const c_char = std::ptr::null();
        let found = unsafe { AMediaFormat_getString(self.0, key(k).as_ptr(), &mut out) };
        (found && !out.is_null()).then(|| unsafe { CStr::from_ptr(out) }.to_string_lossy().into_owned())
    }

    /// `(left, top, right, bottom)`, inclusive, of the `crop` rectangle.
    fn crop(&self) -> Option<(i32, i32, i32, i32)> {
        let (mut l, mut t, mut r, mut b) = (0, 0, 0, 0);
        unsafe { AMediaFormat_getRect(self.0, key("crop").as_ptr(), &mut l, &mut t, &mut r, &mut b) }
            .then_some((l, t, r, b))
    }

    /// Frames a second, stored as an integer or a float depending on the container.
    fn frame_rate(&self) -> Option<f64> {
        let rate = self.f32("frame-rate").map(f64::from).or_else(|| self.i32("frame-rate").map(f64::from))?;
        (rate.is_finite() && rate > 0.0).then_some(rate)
    }
}

impl Drop for Format {
    fn drop(&mut self) {
        unsafe { AMediaFormat_delete(self.0) };
    }
}

/// An owned, started `AMediaCodec`.
struct Codec(*mut AMediaCodec);

// The NDK codec handle may be used from any one thread at a time.
unsafe impl Send for Codec {}

impl Codec {
    /// The codec configured with `format` and started, or the error MediaCodec gave.
    fn start(ptr: *mut AMediaCodec, format: &Format, encoder: bool) -> Result<Self> {
        if ptr.is_null() {
            bail!("no codec for {}", format.string("mime").unwrap_or_default());
        }
        let codec = Self(ptr);
        let status = unsafe {
            AMediaCodec_configure(ptr, format.0, std::ptr::null_mut(), std::ptr::null_mut(), u32::from(encoder))
        };
        if status != 0 {
            bail!("{} refused its format ({status})", codec.name());
        }
        let status = unsafe { AMediaCodec_start(ptr) };
        if status != 0 {
            bail!("{} did not start ({status})", codec.name());
        }
        Ok(codec)
    }

    fn name(&self) -> String {
        let mut out: *mut c_char = std::ptr::null_mut();
        unsafe {
            if AMediaCodec_getName(self.0, &mut out) != 0 || out.is_null() {
                return "codec".into();
            }
            let name = CStr::from_ptr(out).to_string_lossy().into_owned();
            AMediaCodec_releaseName(self.0, out);
            name
        }
    }

    fn input_format(&self) -> Option<Format> {
        Format::owned(unsafe { AMediaCodec_getInputFormat(self.0) })
    }

    fn output_format(&self) -> Option<Format> {
        Format::owned(unsafe { AMediaCodec_getOutputFormat(self.0) })
    }

    /// An input buffer index and its memory, or `None` when none frees up
    /// within `timeout_us`. The memory stays valid until the buffer is
    /// queued; it is not tied to `self` so callers may drain meanwhile.
    fn input(&self, timeout_us: i64) -> Option<(usize, &'static mut [u8])> {
        let index = unsafe { AMediaCodec_dequeueInputBuffer(self.0, timeout_us) };
        if index < 0 {
            return None;
        }
        let mut size = 0usize;
        let ptr = unsafe { AMediaCodec_getInputBuffer(self.0, index as usize, &mut size) };
        if ptr.is_null() {
            return None;
        }
        Some((index as usize, unsafe { std::slice::from_raw_parts_mut(ptr, size) }))
    }

    fn queue(&self, index: usize, size: usize, pts_us: i64, flags: u32) -> Result<()> {
        let status = unsafe { AMediaCodec_queueInputBuffer(self.0, index, 0, size, pts_us.max(0) as u64, flags) };
        if status != 0 {
            bail!("{} refused an input buffer ({status})", self.name());
        }
        Ok(())
    }

    fn dequeue_output(&self, timeout_us: i64) -> (isize, BufInfo) {
        let mut info = BufInfo::default();
        let index = unsafe { AMediaCodec_dequeueOutputBuffer(self.0, &mut info, timeout_us) };
        (index, info)
    }

    fn output(&self, index: usize, info: &BufInfo) -> &[u8] {
        let mut size = 0usize;
        let ptr = unsafe { AMediaCodec_getOutputBuffer(self.0, index, &mut size) };
        if ptr.is_null() || info.size <= 0 {
            return &[];
        }
        let start = (info.offset.max(0) as usize).min(size);
        let end = (start + info.size as usize).min(size);
        unsafe { &std::slice::from_raw_parts(ptr, size)[start..end] }
    }

    fn release(&self, index: usize) {
        unsafe { AMediaCodec_releaseOutputBuffer(self.0, index, false) };
    }
}

impl Drop for Codec {
    fn drop(&mut self) {
        unsafe {
            AMediaCodec_stop(self.0);
            AMediaCodec_delete(self.0);
        }
    }
}

/// An owned `AMediaExtractor` reading an open file.
struct Extractor {
    ptr: *mut AMediaExtractor,
    _file: File,
}

unsafe impl Send for Extractor {}

impl Extractor {
    fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        let length = file.metadata().map(|m| m.len() as i64).unwrap_or(i64::MAX);
        let ptr = unsafe { AMediaExtractor_new() };
        if ptr.is_null() {
            bail!("MediaExtractor unavailable");
        }
        let extractor = Self { ptr, _file: file };
        let status = unsafe { AMediaExtractor_setDataSourceFd(ptr, extractor._file.as_raw_fd(), 0, length) };
        if status != 0 {
            bail!("MediaExtractor cannot read {} ({status})", path.display());
        }
        Ok(extractor)
    }

    fn tracks(&self) -> impl Iterator<Item = (usize, Format)> + '_ {
        let count = unsafe { AMediaExtractor_getTrackCount(self.ptr) };
        (0..count).filter_map(|i| Format::owned(unsafe { AMediaExtractor_getTrackFormat(self.ptr, i) }).map(|f| (i, f)))
    }

    /// The first track whose MIME type starts with `kind` (`video/`, `audio/`).
    fn track(&self, kind: &str) -> Option<(usize, Format)> {
        self.tracks().find(|(_, format)| format.string("mime").is_some_and(|mime| mime.starts_with(kind)))
    }

    fn select(&self, track: usize) {
        unsafe { AMediaExtractor_selectTrack(self.ptr, track) };
    }

    fn seek(&self, time_us: i64) {
        unsafe { AMediaExtractor_seekTo(self.ptr, time_us.max(0), SEEK_PREVIOUS_SYNC) };
    }

    /// Copies the current sample into `buffer` and returns its size and time,
    /// or `None` at the end of the selected tracks.
    fn read(&self, buffer: &mut [u8]) -> Option<(usize, i64)> {
        let size = unsafe { AMediaExtractor_readSampleData(self.ptr, buffer.as_mut_ptr(), buffer.len()) };
        if size < 0 {
            return None;
        }
        Some((size as usize, unsafe { AMediaExtractor_getSampleTime(self.ptr) }))
    }

    fn sample_time(&self) -> i64 {
        unsafe { AMediaExtractor_getSampleTime(self.ptr) }
    }

    fn advance(&self) -> bool {
        unsafe { AMediaExtractor_advance(self.ptr) }
    }
}

impl Drop for Extractor {
    fn drop(&mut self) {
        unsafe { AMediaExtractor_delete(self.ptr) };
    }
}

/// The FFmpeg-style codec name of a MediaFormat MIME type.
fn codec_name(mime: &str) -> String {
    match mime {
        "audio/mp4a-latm" => "aac".into(),
        "audio/mpeg" => "mp3".into(),
        "audio/raw" => "pcm".into(),
        "audio/opus" => "opus".into(),
        "audio/vorbis" => "vorbis".into(),
        "audio/flac" => "flac".into(),
        "audio/ac3" => "ac3".into(),
        "audio/eac3" => "eac3".into(),
        other => other.rsplit('/').next().unwrap_or(other).to_string(),
    }
}

/// Size, frame rate and sound codec of the first video track. Containers
/// that store no frame rate get the average of the first sample times.
pub fn probe(path: &Path) -> Result<VideoProbe> {
    let extractor = Extractor::open(path)?;
    let (track, format) = extractor.track("video/").ok_or_else(|| anyhow!("no video track in {}", path.display()))?;
    let width = format.i32("width").filter(|w| *w > 0).ok_or_else(|| anyhow!("video track has no width"))?;
    let height = format.i32("height").filter(|h| *h > 0).ok_or_else(|| anyhow!("video track has no height"))?;
    let audio = extractor.track("audio/").and_then(|(_, f)| f.string("mime")).map(|mime| Arc::from(codec_name(&mime)));
    let fps = format.frame_rate().unwrap_or_else(|| sampled_rate(&extractor, track).unwrap_or(30.0));
    Ok(VideoProbe { width: width as u32, height: height as u32, fps, color: None, audio })
}

/// Frames a second from the first 60 sample times of `track`.
fn sampled_rate(extractor: &Extractor, track: usize) -> Option<f64> {
    extractor.select(track);
    let mut times = Vec::new();
    loop {
        let time = extractor.sample_time();
        if time < 0 || times.len() >= 60 {
            break;
        }
        times.push(time);
        if !extractor.advance() {
            break;
        }
    }
    times.sort_unstable();
    let span = (*times.last()? - *times.first()?) as f64 / 1e6;
    (times.len() > 1 && span > 0.0).then(|| (times.len() - 1) as f64 / span)
}

/// The longest track duration in seconds.
pub fn duration(path: &Path) -> Option<f64> {
    let extractor = Extractor::open(path).ok()?;
    let longest = extractor.tracks().filter_map(|(_, f)| f.i64("durationUs")).max()?;
    let seconds = longest as f64 / 1e6;
    (seconds.is_finite() && seconds > 0.0).then_some(seconds)
}

/// How far a pending input or output may wait before a codec counts as stuck.
const STALL: Duration = Duration::from_secs(5);

/// The layout of a decoder's YUV output buffers.
#[derive(Clone, Copy, Debug)]
struct Layout {
    width: usize,
    height: usize,
    stride: usize,
    slice: usize,
    left: usize,
    top: usize,
    planar: bool,
    full_range: bool,
    matrix: Matrix,
}

/// YUV to RGB matrices a decoded frame may use.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Matrix {
    Bt601,
    Bt709,
    Bt2020,
}

/// Matrix and range of a video track: the container's `colr` box when the
/// extractor read one, else the H.264 sequence header, else BT.601 limited
/// range, as FFmpeg reads an untagged stream. A decoder's own output format
/// is no guide: it reports BT.709 for any untagged HD stream.
fn track_colour(track: &Format) -> (Matrix, bool) {
    let sps = track
        .string("mime")
        .filter(|m| m == "video/avc")
        .and_then(|_| track.bytes("csd-0"))
        .and_then(|csd| avc_colour(&csd));
    // MediaFormat COLOR_STANDARD_BT709 = 1, BT601 = 2 and 4, BT2020 = 6.
    let matrix = match track.i32("color-standard") {
        Some(1) => Some(Matrix::Bt709),
        Some(2 | 4) => Some(Matrix::Bt601),
        Some(6) => Some(Matrix::Bt2020),
        _ => None,
    }
    .or_else(|| sps.and_then(|(m, _)| m))
    .unwrap_or(Matrix::Bt601);
    // COLOR_RANGE_FULL = 1, LIMITED = 2.
    let full = match track.i32("color-range") {
        Some(1) => true,
        Some(2) => false,
        _ => sps.and_then(|(_, f)| f).unwrap_or(false),
    };
    (matrix, full)
}

/// Bit reader over an RBSP (emulation prevention bytes removed).
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Bits<'_> {
    fn bit(&mut self) -> Option<u32> {
        let byte = *self.data.get(self.pos / 8)?;
        let bit = (byte >> (7 - self.pos % 8)) & 1;
        self.pos += 1;
        Some(u32::from(bit))
    }

    fn bits(&mut self, n: u32) -> Option<u32> {
        (0..n).try_fold(0, |acc, _| Some((acc << 1) | self.bit()?))
    }

    fn ue(&mut self) -> Option<u32> {
        let mut zeros = 0;
        while self.bit()? == 0 {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        Some((1u32 << zeros) - 1 + self.bits(zeros)?)
    }

    fn se(&mut self) -> Option<i32> {
        let k = self.ue()?;
        Some(if k % 2 == 1 { k.div_ceil(2) as i32 } else { -((k / 2) as i32) })
    }
}

/// `(matrix, full range)` from the VUI of the H.264 sequence parameter set in
/// `csd` (Annex B, as MediaExtractor gives `csd-0`). Each is `None` when the
/// stream leaves it unsaid.
fn avc_colour(csd: &[u8]) -> Option<(Option<Matrix>, Option<bool>)> {
    // Skip start codes up to the SPS NAL (type 7), then drop emulation bytes.
    let start = csd.windows(3).position(|w| w == [0, 0, 1]).map_or(0, |i| i + 3);
    let nal = csd.get(start..)?;
    if nal.first()? & 0x1f != 7 {
        return None;
    }
    let mut rbsp = Vec::with_capacity(nal.len());
    let mut zeros = 0;
    for &b in &nal[1..] {
        if zeros >= 2 && b == 3 {
            zeros = 0;
            continue;
        }
        zeros = if b == 0 { zeros + 1 } else { 0 };
        rbsp.push(b);
    }
    let mut r = Bits { data: &rbsp, pos: 0 };
    let profile = r.bits(8)?;
    r.bits(16)?; // constraint flags, level
    r.ue()?; // seq_parameter_set_id
    if matches!(profile, 100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135) {
        let chroma = r.ue()?;
        if chroma == 3 {
            r.bit()?;
        }
        r.ue()?;
        r.ue()?;
        r.bit()?;
        if r.bit()? == 1 {
            for i in 0..if chroma == 3 { 12 } else { 8 } {
                if r.bit()? == 1 {
                    let size = if i < 6 { 16 } else { 64 };
                    let (mut last, mut next) = (8i32, 8i32);
                    for _ in 0..size {
                        if next != 0 {
                            next = (last + r.se()? + 256) % 256;
                        }
                        if next != 0 {
                            last = next;
                        }
                    }
                }
            }
        }
    }
    r.ue()?; // log2_max_frame_num_minus4
    match r.ue()? {
        0 => {
            r.ue()?;
        }
        1 => {
            r.bit()?;
            r.se()?;
            r.se()?;
            for _ in 0..r.ue()? {
                r.se()?;
            }
        }
        _ => {}
    }
    r.ue()?; // max_num_ref_frames
    r.bit()?;
    r.ue()?;
    r.ue()?;
    if r.bit()? == 0 {
        r.bit()?; // mb_adaptive_frame_field_flag
    }
    r.bit()?;
    if r.bit()? == 1 {
        for _ in 0..4 {
            r.ue()?;
        }
    }
    if r.bit()? == 0 {
        return Some((None, None));
    }
    if r.bit()? == 1 && r.bits(8)? == 255 {
        r.bits(32)?; // sar width and height
    }
    if r.bit()? == 1 {
        r.bit()?; // overscan_appropriate_flag
    }
    if r.bit()? == 0 {
        return Some((None, None));
    }
    r.bits(3)?; // video_format
    let full = r.bit()? == 1;
    if r.bit()? == 0 {
        return Some((None, Some(full)));
    }
    r.bits(16)?; // primaries, transfer
    let matrix = match r.bits(8)? {
        1 => Some(Matrix::Bt709),
        5 | 6 => Some(Matrix::Bt601),
        9 | 10 => Some(Matrix::Bt2020),
        _ => None,
    };
    Some((matrix, Some(full)))
}

impl Layout {
    /// `format` is the decoder's output format; `colour` comes from [`track_colour`].
    fn read(format: &Format, colour: (Matrix, bool), width: usize, height: usize) -> Result<Self> {
        let color = format.i32("color-format").unwrap_or(COLOR_YUV420_SEMI_PLANAR);
        let planar = match color {
            COLOR_YUV420_PLANAR => true,
            COLOR_YUV420_SEMI_PLANAR | COLOR_QCOM_NV12 | COLOR_QCOM_NV12_32M | COLOR_YUV420_FLEXIBLE => false,
            other => bail!("decoder output colour format {other:#x} is not supported"),
        };
        let coded_w = format.i32("width").unwrap_or(width as i32).max(1) as usize;
        let coded_h = format.i32("height").unwrap_or(height as i32).max(1) as usize;
        let (left, top) = format.crop().map_or((0, 0), |(l, t, _, _)| (l.max(0) as usize, t.max(0) as usize));
        let stride = format.i32("stride").filter(|s| *s > 0).map_or(coded_w, |s| s as usize).max(coded_w);
        let slice = format.i32("slice-height").filter(|s| *s > 0).map_or(coded_h, |s| s as usize).max(coded_h);
        Ok(Self {
            width,
            height,
            stride,
            slice,
            left,
            top,
            planar,
            full_range: colour.1,
            matrix: colour.0,
        })
    }

    /// Converts one output buffer to straight RGBA in `rgba`.
    fn write_rgba(self, yuv: &[u8], rgba: &mut [u8]) -> Result<()> {
        let chroma_rows = self.slice.div_ceil(2);
        if yuv.len() < self.stride * (self.top + self.height) {
            bail!("decoder buffer of {} bytes is smaller than its {}x{} frame", yuv.len(), self.width, self.height);
        }
        // Integer coefficients scaled by 256.
        let (rv, gu, gv, bu) = match self.matrix {
            Matrix::Bt601 => (359, 88, 183, 454),
            Matrix::Bt709 => (403, 48, 120, 475),
            Matrix::Bt2020 => (377, 42, 146, 482),
        };
        let (ys, yo) = if self.full_range { (256, 0) } else { (298, 16) };
        let cs = if self.full_range { 256 } else { 292 };
        let (y_plane, chroma) = yuv.split_at((self.stride * self.slice).min(yuv.len()));
        let half = self.stride / 2;
        let at = |plane: &[u8], i: usize| plane.get(i).copied().unwrap_or(128) as i32;
        for row in 0..self.height {
            let sy = row + self.top;
            let cy = sy / 2;
            let out = &mut rgba[row * self.width * 4..(row + 1) * self.width * 4];
            for col in 0..self.width {
                let sx = col + self.left;
                let y = (at(y_plane, sy * self.stride + sx) - yo) * ys;
                let (u, v) = if self.planar {
                    let u = at(chroma, cy * half + sx / 2);
                    let v = at(chroma, half * chroma_rows + cy * half + sx / 2);
                    (u, v)
                } else {
                    let i = cy * self.stride + (sx & !1);
                    (at(chroma, i), at(chroma, i + 1))
                };
                let (u, v) = ((u - 128) * cs / 256, (v - 128) * cs / 256);
                let px = &mut out[col * 4..col * 4 + 4];
                px[0] = ((y + rv * v + 128) >> 8).clamp(0, 255) as u8;
                px[1] = ((y - gu * u - gv * v + 128) >> 8).clamp(0, 255) as u8;
                px[2] = ((y + bu * u + 128) >> 8).clamp(0, 255) as u8;
                px[3] = 255;
            }
        }
        Ok(())
    }
}

/// Decodes the first video track of `path` from `seek` seconds onto a grid
/// of `fps` frames a second, as RGBA frames of `width` x `height`, on a
/// thread of its own. Frame `k` is the last source frame whose time rounds to
/// grid slot `k` or before it, as FFmpeg's `fps` filter picks them. The
/// thread stops at the end of the track, when `frames` closes, or when `stop`
/// is set. `spare` returns used buffers for the next frames.
pub fn spawn_decode(
    path: PathBuf,
    seek: f64,
    fps: f64,
    (width, height): (u32, u32),
    frames: Sender<std::io::Result<Vec<u8>>>,
    spare: Receiver<Vec<u8>>,
    stop: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        if let Err(error) = decode_video(&path, seek, fps, (width as usize, height as usize), &frames, &spare, &stop) {
            let _ = frames.send(Err(std::io::Error::other(format!("{error:#}"))));
        }
    })
}

fn decode_video(
    path: &Path,
    seek: f64,
    fps: f64,
    (width, height): (usize, usize),
    frames: &Sender<std::io::Result<Vec<u8>>>,
    spare: &Receiver<Vec<u8>>,
    stop: &AtomicBool,
) -> Result<()> {
    let frame_len = width * height * 4;
    let extractor = Extractor::open(path)?;
    let (track, track_format) = extractor.track("video/").ok_or_else(|| anyhow!("no video track in {}", path.display()))?;
    let mime = track_format.string("mime").unwrap_or_default();
    let colour = track_colour(&track_format);
    extractor.select(track);
    extractor.seek((seek * 1e6) as i64);
    let codec = Codec::start(unsafe { AMediaCodec_createDecoderByType(key(&mime).as_ptr()) }, &track_format, false)
        .with_context(|| format!("decode {}", path.display()))?;

    let mut layout: Option<Layout> = None;
    let mut held: Option<Vec<u8>> = None;
    let mut next_slot: u64 = 0;
    let mut input_done = false;
    let mut idle = Instant::now();
    let half = 0.5 / fps;
    let emit = |frame: Vec<u8>| frames.send(Ok(frame)).is_ok();
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        if !input_done {
            if let Some((index, buffer)) = codec.input(2_000) {
                match extractor.read(buffer) {
                    Some((size, time)) => {
                        codec.queue(index, size, time, 0)?;
                        extractor.advance();
                    }
                    None => {
                        codec.queue(index, 0, 0, FLAG_END_OF_STREAM)?;
                        input_done = true;
                    }
                }
                idle = Instant::now();
            }
        }
        let (index, info) = codec.dequeue_output(2_000);
        match index {
            INFO_FORMAT_CHANGED => {
                if let Some(format) = codec.output_format() {
                    layout = Some(Layout::read(&format, colour, width, height)?);
                }
                continue;
            }
            INFO_TRY_AGAIN | INFO_BUFFERS_CHANGED => {
                if idle.elapsed() > STALL {
                    bail!("{} stalled decoding {}", codec.name(), path.display());
                }
                continue;
            }
            i if i < 0 => continue,
            _ => {}
        }
        idle = Instant::now();
        let index = index as usize;
        let end = info.flags & FLAG_END_OF_STREAM != 0;
        let relative = info.pts_us as f64 / 1e6 - seek;
        // Frames before the seek point only prime the decoder.
        if info.size > 0 && relative >= -1e-6 {
            if layout.is_none() {
                let format = codec.output_format().ok_or_else(|| anyhow!("decoder has no output format"))?;
                layout = Some(Layout::read(&format, colour, width, height)?);
            }
            let current = layout.expect("set above");
            let mut rgba = spare.try_recv().unwrap_or_default();
            rgba.resize(frame_len, 0);
            current.write_rgba(codec.output(index, &info), &mut rgba)?;
            codec.release(index);
            let slot = ((relative + half) * fps).floor().max(0.0) as u64;
            while next_slot < slot {
                let fill = match &held {
                    Some(frame) => frame.clone(),
                    None => rgba.clone(),
                };
                if !emit(fill) {
                    return Ok(());
                }
                next_slot += 1;
            }
            held = Some(rgba);
        } else {
            codec.release(index);
        }
        if end {
            break;
        }
    }
    if let Some(frame) = held {
        let _ = emit(frame);
    }
    Ok(())
}

/// The first sound track of `path` as interleaved stereo f32 at its own
/// rate, and that rate. A track that starts late opens with silence, as
/// players and `aresample=first_pts=0` place it.
pub fn decode_audio(path: &Path) -> Result<(Vec<f32>, u32)> {
    let extractor = Extractor::open(path)?;
    let (track, format) = extractor.track("audio/").ok_or_else(|| anyhow!("no audio stream"))?;
    let mime = format.string("mime").unwrap_or_default();
    extractor.select(track);
    let codec = Codec::start(unsafe { AMediaCodec_createDecoderByType(key(&mime).as_ptr()) }, &format, false)?;
    let mut rate = format.i32("sample-rate").unwrap_or(48_000).max(1) as u32;
    let mut channels = format.i32("channel-count").unwrap_or(2).max(1) as usize;
    let mut float = false;
    let mut samples: Vec<f32> = Vec::new();
    let mut first_pts: Option<i64> = None;
    let mut input_done = false;
    let mut idle = Instant::now();
    loop {
        if !input_done {
            if let Some((index, buffer)) = codec.input(2_000) {
                match extractor.read(buffer) {
                    Some((size, time)) => {
                        codec.queue(index, size, time, 0)?;
                        extractor.advance();
                    }
                    None => {
                        codec.queue(index, 0, 0, FLAG_END_OF_STREAM)?;
                        input_done = true;
                    }
                }
                idle = Instant::now();
            }
        }
        let (index, info) = codec.dequeue_output(2_000);
        if index == INFO_FORMAT_CHANGED {
            if let Some(format) = codec.output_format() {
                rate = format.i32("sample-rate").unwrap_or(rate as i32).max(1) as u32;
                channels = format.i32("channel-count").unwrap_or(channels as i32).max(1) as usize;
                // AudioFormat.ENCODING_PCM_FLOAT = 4; the default is 16-bit.
                float = format.i32("pcm-encoding") == Some(4);
            }
            continue;
        }
        if index < 0 {
            if idle.elapsed() > STALL {
                bail!("{} stalled decoding audio", codec.name());
            }
            continue;
        }
        idle = Instant::now();
        let index = index as usize;
        let data = codec.output(index, &info);
        if !data.is_empty() {
            let start = *first_pts.get_or_insert(info.pts_us);
            if samples.is_empty() && start > 0 {
                samples.resize((start as f64 / 1e6 * rate as f64) as usize * 2, 0.0);
            }
            let frame = |i: usize| -> f32 {
                if float {
                    f32::from_le_bytes(data[i * 4..i * 4 + 4].try_into().unwrap())
                } else {
                    i16::from_le_bytes([data[i * 2], data[i * 2 + 1]]) as f32 / 32768.0
                }
            };
            let count = data.len() / if float { 4 } else { 2 } / channels;
            samples.reserve(count * 2);
            for f in 0..count {
                let left = frame(f * channels);
                let right = if channels > 1 { frame(f * channels + 1) } else { left };
                samples.push(left);
                samples.push(right);
            }
        }
        codec.release(index);
        if info.flags & FLAG_END_OF_STREAM != 0 {
            break;
        }
    }
    if samples.is_empty() {
        bail!("decoded no audio samples");
    }
    Ok((samples, rate))
}

/// AAC bitrate, as `AUDIO_BITRATE` on the desktop.
const AUDIO_BITRATE: i32 = 256_000;

/// An encoded sample waiting for the muxer to start.
struct Pending {
    audio: bool,
    data: Vec<u8>,
    info: BufInfo,
}

/// H.264 + AAC MP4 through the device's MediaCodec encoders and MediaMuxer,
/// with the interface of the desktop FFmpeg `Encoder`. Audio, streamed per
/// frame or premixed, is fed to the AAC encoder in step with the video, so
/// the muxer interleaves the two.
pub struct Encoder {
    video: Codec,
    audio: Option<Codec>,
    muxer: *mut AMediaMuxer,
    _file: File,
    dest: PathBuf,
    video_track: Option<usize>,
    audio_track: Option<usize>,
    /// Output formats kept until both encoders reported one, so tracks are added in order.
    video_format: Option<Format>,
    audio_format: Option<Format>,
    started: bool,
    pending: Vec<Pending>,
    width: usize,
    height: usize,
    stride: usize,
    slice: usize,
    planar: bool,
    fps: f64,
    frames: u64,
    sample_rate: u32,
    /// Interleaved stereo samples given and not yet queued to the AAC encoder.
    audio_queue: std::collections::VecDeque<f32>,
    /// Sample frames queued to the AAC encoder.
    audio_queued: u64,
    /// Whether the whole track was given (premixed), so it may run ahead.
    premixed: bool,
    audio_done: bool,
    video_done: bool,
}

unsafe impl Send for Encoder {}

impl Encoder {
    /// Stream audio per frame with [`Encoder::encode_audio`].
    pub fn new(dest: &Path, settings: EncoderSettings) -> Result<Self> {
        Self::open(dest, settings, true, false)
    }

    /// Video frames with no audio stream in the output.
    pub fn video_only(dest: &Path, settings: EncoderSettings) -> Result<Self> {
        Self::open(dest, settings, false, false)
    }

    /// Encode `samples` (interleaved stereo f32 at `settings.sample_rate`) with the video.
    pub fn with_audio(dest: &Path, settings: EncoderSettings, samples: &[f32]) -> Result<Self> {
        let mut encoder = Self::with_audio_input(dest, settings)?;
        encoder.send_audio(samples.to_vec())?;
        Ok(encoder)
    }

    /// An encoder whose whole premixed track arrives later through [`Encoder::send_audio`].
    pub fn with_audio_input(dest: &Path, settings: EncoderSettings) -> Result<Self> {
        Self::open(dest, settings, true, true)
    }

    /// The whole premixed track for an encoder from [`Encoder::with_audio_input`].
    pub fn send_audio(&mut self, samples: Vec<f32>) -> Result<()> {
        if self.audio.is_none() || !self.premixed {
            bail!("this encoder streams its audio per frame");
        }
        self.audio_queue.extend(samples);
        self.feed_audio(false)
    }

    fn open(dest: &Path, settings: EncoderSettings, with_audio: bool, premixed: bool) -> Result<Self> {
        if settings.alpha {
            bail!("transparent (VP9 WebM) export is not available on Android; export without alpha");
        }
        if settings.underlay.is_some() {
            bail!("underlay export needs FFmpeg, which Android does not have");
        }
        let (width, height) = (settings.width, settings.height);
        if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
            bail!("Android's H.264 encoders take even frame sizes; got {width}x{height}");
        }
        let fps = if settings.fps.is_finite() && settings.fps > 0.0 { settings.fps } else { 30.0 };
        let (video, planar) = Self::video_codec(width, height, fps, settings.draft)?;
        let (mut stride, mut slice) = (width, height);
        if let Some(format) = video.input_format() {
            stride = format.i32("stride").filter(|s| *s > 0).map_or(width, |s| s as usize).max(width);
            slice = format.i32("slice-height").filter(|s| *s > 0).map_or(height, |s| s as usize).max(height);
        }
        let sample_rate = settings.sample_rate.max(8_000) as u32;
        let audio = if with_audio { Some(Self::audio_codec(sample_rate)?) } else { None };
        if let Some(parent) = dest.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(dest)
            .with_context(|| format!("create {}", dest.display()))?;
        // MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4 = 0.
        let muxer = unsafe { AMediaMuxer_new(file.as_raw_fd(), 0) };
        if muxer.is_null() {
            bail!("MediaMuxer cannot write {}", dest.display());
        }
        tracing::info!("Android encoder: {} {}x{} @ {fps} fps", video.name(), width, height);
        Ok(Self {
            video,
            audio,
            muxer,
            _file: file,
            dest: dest.to_path_buf(),
            video_track: None,
            audio_track: None,
            video_format: None,
            audio_format: None,
            started: false,
            pending: Vec::new(),
            width,
            height,
            stride,
            slice,
            planar,
            fps,
            frames: 0,
            sample_rate,
            audio_queue: Default::default(),
            audio_queued: 0,
            premixed,
            audio_done: false,
            video_done: false,
        })
    }

    /// The H.264 encoder, taking NV12 where it can and I420 where it cannot.
    fn video_codec(width: usize, height: usize, fps: f64, draft: bool) -> Result<(Codec, bool)> {
        // About 0.15 bits a pixel at full quality: 9 Mbit/s for 1080p30.
        let bits = (width * height) as f64 * fps * if draft { 0.06 } else { 0.15 };
        let mut last = None;
        for (color, planar) in [(COLOR_YUV420_SEMI_PLANAR, false), (COLOR_YUV420_PLANAR, true)] {
            let format = Format::new();
            format.set_str("mime", "video/avc");
            format.set_i32("width", width as i32);
            format.set_i32("height", height as i32);
            format.set_i32("bitrate", bits.clamp(500_000.0, 80_000_000.0) as i32);
            format.set_i32("frame-rate", fps.round().max(1.0) as i32);
            format.set_i32("i-frame-interval", 2);
            format.set_i32("color-format", color);
            // BT.709, limited range, SDR: what the RGB conversion below writes.
            format.set_i32("color-standard", 1);
            format.set_i32("color-range", 2);
            format.set_i32("color-transfer", 3);
            let ptr = match std::env::var("DIRECTOR_ANDROID_ENCODER") {
                Ok(name) => unsafe { AMediaCodec_createCodecByName(key(&name).as_ptr()) },
                Err(_) => unsafe { AMediaCodec_createEncoderByType(key("video/avc").as_ptr()) },
            };
            match Codec::start(ptr, &format, true) {
                Ok(codec) => return Ok((codec, planar)),
                Err(error) => last = Some(error),
            }
        }
        Err(last.unwrap_or_else(|| anyhow!("no H.264 encoder")))
    }

    fn audio_codec(sample_rate: u32) -> Result<Codec> {
        let format = Format::new();
        format.set_str("mime", "audio/mp4a-latm");
        format.set_i32("sample-rate", sample_rate as i32);
        format.set_i32("channel-count", 2);
        format.set_i32("bitrate", AUDIO_BITRATE);
        // MediaCodecInfo.CodecProfileLevel.AACObjectLC = 2.
        format.set_i32("aac-profile", 2);
        format.set_i32("max-input-size", 16_384);
        Codec::start(unsafe { AMediaCodec_createEncoderByType(key("audio/mp4a-latm").as_ptr()) }, &format, true)
            .context("AAC encoder")
    }

    /// Queues one RGBA frame.
    pub fn encode(&mut self, frame_data: &[u8], _time: f64) -> Result<()> {
        let frame_len = self.width * self.height * 4;
        if frame_data.len() < frame_len {
            bail!("frame of {} bytes is smaller than {}x{} RGBA", frame_data.len(), self.width, self.height);
        }
        let started = Instant::now();
        let (index, buffer) = loop {
            if let Some(input) = self.video.input(5_000) {
                break input;
            }
            self.drain(false)?;
            self.drain(true)?;
            if started.elapsed() > STALL {
                bail!("{} took no frame for {} s", self.video.name(), STALL.as_secs());
            }
        };
        let need = rgba_to_yuv(&frame_data[..frame_len], self.width, self.height, self.stride, self.slice, self.planar, buffer)
            .with_context(|| format!("{} input buffer", self.video.name()))?;
        let pts = (self.frames as f64 * 1e6 / self.fps).round() as i64;
        self.video.queue(index, need, pts, 0)?;
        self.frames += 1;
        self.drain(false)?;
        self.feed_audio(false)
    }

    /// Streams the audio of one frame (interleaved stereo f32).
    pub fn encode_audio(&mut self, samples: &[f32], _time: f64) -> Result<()> {
        if self.audio.is_none() || self.premixed {
            return Ok(());
        }
        self.audio_queue.extend(samples.iter().copied());
        self.feed_audio(false)
    }

    /// Queues given audio to the AAC encoder: all of it when `all`, otherwise
    /// up to a quarter second past the video, so the two stay interleaved.
    fn feed_audio(&mut self, all: bool) -> Result<()> {
        let Some(audio) = &self.audio else { return Ok(()) };
        if self.audio_done {
            return Ok(());
        }
        let target = if all {
            u64::MAX
        } else {
            ((self.frames as f64 / self.fps + 0.25) * self.sample_rate as f64) as u64
        };
        let name = audio.name();
        let mut waited = Instant::now();
        while self.audio_queued < target && self.audio_queue.len() >= 2 {
            let audio = self.audio.as_ref().expect("checked above");
            let Some((index, buffer)) = audio.input(5_000) else {
                self.drain(true)?;
                if !all {
                    return Ok(());
                }
                if waited.elapsed() > STALL {
                    bail!("{name} took no audio for {} s", STALL.as_secs());
                }
                continue;
            };
            waited = Instant::now();
            let room = (buffer.len() / 4).min(2048);
            let frames = (self.audio_queue.len() / 2).min(room).min((target - self.audio_queued) as usize).max(1);
            for (i, sample) in self.audio_queue.drain(..frames * 2).enumerate() {
                let value = (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                buffer[i * 2..i * 2 + 2].copy_from_slice(&value.to_le_bytes());
            }
            let pts = (self.audio_queued as f64 * 1e6 / self.sample_rate as f64).round() as i64;
            audio.queue(index, frames * 4, pts, 0)?;
            self.audio_queued += frames as u64;
            self.drain(true)?;
        }
        Ok(())
    }

    /// Writes every encoded sample of the video (or `audio`) encoder that is ready.
    fn drain(&mut self, audio: bool) -> Result<()> {
        self.drain_until(audio, false)
    }

    /// As [`Encoder::drain`]; with `to_end`, waits for the encoder's end of stream.
    fn drain_until(&mut self, audio: bool, to_end: bool) -> Result<()> {
        let mut waited = Instant::now();
        loop {
            let codec = if audio {
                match &self.audio {
                    Some(codec) => codec,
                    None => return Ok(()),
                }
            } else {
                &self.video
            };
            let (index, info) = codec.dequeue_output(if to_end { 10_000 } else { 0 });
            if index == INFO_FORMAT_CHANGED {
                let format = codec.output_format().ok_or_else(|| anyhow!("{} has no output format", codec.name()))?;
                if audio {
                    self.audio_format = Some(format);
                } else {
                    self.video_format = Some(format);
                }
                self.start_muxer()?;
                continue;
            }
            if index < 0 {
                if !to_end {
                    return Ok(());
                }
                if waited.elapsed() > STALL {
                    bail!("{} gave no end of stream in {} s", codec.name(), STALL.as_secs());
                }
                continue;
            }
            waited = Instant::now();
            let index = index as usize;
            if info.size > 0 && info.flags & FLAG_CODEC_CONFIG == 0 {
                let data = codec.output(index, &info);
                let mut sample = info;
                sample.offset = 0;
                if self.started {
                    let track = if audio { self.audio_track } else { self.video_track }.expect("started");
                    let status = unsafe { AMediaMuxer_writeSampleData(self.muxer, track, data.as_ptr(), &sample) };
                    if status != 0 {
                        codec.release(index);
                        bail!("MediaMuxer refused a sample ({status})");
                    }
                } else {
                    self.pending.push(Pending { audio, data: data.to_vec(), info: sample });
                }
            }
            codec.release(index);
            if info.flags & FLAG_END_OF_STREAM != 0 {
                return Ok(());
            }
        }
    }

    /// Starts the muxer once every track has its format, then writes the samples that waited.
    fn start_muxer(&mut self) -> Result<()> {
        if self.started || self.video_format.is_none() || (self.audio.is_some() && self.audio_format.is_none()) {
            return Ok(());
        }
        // Video is added first so it is track 0, as players and FFmpeg's output expect.
        for (audio, format) in [(false, self.video_format.take()), (true, self.audio_format.take())] {
            let Some(format) = format else { continue };
            let track = unsafe { AMediaMuxer_addTrack(self.muxer, format.0) };
            if track < 0 {
                bail!("MediaMuxer refused the {} track ({track})", if audio { "audio" } else { "video" });
            }
            if audio {
                self.audio_track = Some(track as usize);
            } else {
                self.video_track = Some(track as usize);
            }
        }
        let status = unsafe { AMediaMuxer_start(self.muxer) };
        if status != 0 {
            bail!("MediaMuxer did not start ({status})");
        }
        self.started = true;
        for sample in std::mem::take(&mut self.pending) {
            let track = if sample.audio { self.audio_track } else { self.video_track }.expect("both tracks");
            unsafe { AMediaMuxer_writeSampleData(self.muxer, track, sample.data.as_ptr(), &sample.info) };
        }
        Ok(())
    }

    fn end_stream(&mut self, audio: bool) -> Result<()> {
        let started = Instant::now();
        let name = if audio { self.audio.as_ref().map(Codec::name).unwrap_or_default() } else { self.video.name() };
        loop {
            let codec = if audio { self.audio.as_ref() } else { Some(&self.video) };
            let Some(codec) = codec else { return Ok(()) };
            let pts = if audio {
                (self.audio_queued as f64 * 1e6 / self.sample_rate as f64) as i64
            } else {
                (self.frames as f64 * 1e6 / self.fps) as i64
            };
            if let Some((index, _)) = codec.input(5_000) {
                codec.queue(index, 0, pts, FLAG_END_OF_STREAM)?;
                return self.drain_until(audio, true);
            }
            self.drain(audio)?;
            if started.elapsed() > STALL {
                bail!("{name} took no end of stream");
            }
        }
    }

    /// Stops the encoders and removes the partial file.
    pub fn abort(mut self) {
        let _ = self.close();
        let _ = std::fs::remove_file(&self.dest);
    }

    /// Writes the rest of both streams and closes the MP4.
    pub fn finish(mut self) -> Result<()> {
        let result = (|| {
            if self.frames == 0 {
                bail!("no frames were encoded");
            }
            if self.audio.is_some() {
                // An encoder given no sound still needs one block to report its format.
                if self.audio_queued == 0 && self.audio_queue.is_empty() {
                    self.audio_queue.extend(std::iter::repeat_n(0.0, 2048));
                }
                self.feed_audio(true)?;
            }
            self.end_stream(false)?;
            self.video_done = true;
            if self.audio.is_some() {
                self.end_stream(true)?;
                self.audio_done = true;
            }
            if !self.started {
                bail!("the encoders gave no stream formats");
            }
            Ok(())
        })();
        let stopped = self.close();
        match (result, stopped) {
            (Err(error), _) | (Ok(()), Err(error)) => {
                let _ = std::fs::remove_file(&self.dest);
                Err(error)
            }
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    fn close(&mut self) -> Result<()> {
        if self.muxer.is_null() {
            return Ok(());
        }
        let status = if self.started { unsafe { AMediaMuxer_stop(self.muxer) } } else { 0 };
        unsafe { AMediaMuxer_delete(self.muxer) };
        self.muxer = std::ptr::null_mut();
        if status != 0 {
            bail!("MediaMuxer did not close {} ({status})", self.dest.display());
        }
        Ok(())
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// Writes `rgba` as BT.709 limited-range NV12 (or I420 when `planar`) in a
/// codec input buffer of `stride` x `slice` luma, and returns the bytes used.
fn rgba_to_yuv(
    rgba: &[u8],
    w: usize,
    h: usize,
    stride: usize,
    slice: usize,
    planar: bool,
    out: &mut [u8],
) -> Result<usize> {
    let luma = stride * slice;
    let need = luma + if planar { 2 * (stride / 2) * (h / 2) } else { stride * (h / 2) };
    if out.len() < need {
        bail!("{} bytes, {need} needed", out.len());
    }
    let (y_plane, chroma) = out.split_at_mut(luma);
    for y in 0..h {
        let row = &rgba[y * w * 4..(y + 1) * w * 4];
        let yr = &mut y_plane[y * stride..y * stride + w];
        for x in 0..w {
            let (r, g, b) = (row[x * 4] as i32, row[x * 4 + 1] as i32, row[x * 4 + 2] as i32);
            yr[x] = ((47 * r + 157 * g + 16 * b + 128) / 256 + 16) as u8;
        }
    }
    let half = stride / 2;
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            let mut acc = [0i32; 3];
            for (dy, dx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                let p = ((y + dy) * w + x + dx) * 4;
                acc[0] += rgba[p] as i32;
                acc[1] += rgba[p + 1] as i32;
                acc[2] += rgba[p + 2] as i32;
            }
            let (r, g, b) = (acc[0] / 4, acc[1] / 4, acc[2] / 4);
            let u = ((-26 * r - 87 * g + 112 * b + 128) / 256 + 128).clamp(0, 255) as u8;
            let v = ((112 * r - 102 * g - 10 * b + 128) / 256 + 128).clamp(0, 255) as u8;
            if planar {
                chroma[(y / 2) * half + x / 2] = u;
                chroma[half * (h / 2) + (y / 2) * half + x / 2] = v;
            } else {
                let i = (y / 2) * stride + x;
                chroma[i] = u;
                chroma[i + 1] = v;
            }
        }
    }
    Ok(need)
}
