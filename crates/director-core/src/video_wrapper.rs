//! # Video Wrapper Module (CLI Version)
//!
//! Uses `std::process::Command` to invoke `ffmpeg` CLI.
//! Implements strict version checking and Proxy-based preview.
//!
//! ## Licensing
//! MIT/Apache compliant (No linking).

use anyhow::{Context, Result};
use crossbeam_channel::{bounded, unbounded, Receiver, Sender};

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::hash::Hash;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
// use std::sync::{Arc, Mutex};
// actually logs say unused.

use std::thread;
use std::time::SystemTime;

/// Global FFmpeg driver.
pub struct FFmpegDriver;

impl FFmpegDriver {
    pub fn ensure_available() -> Result<()> {
        let output = Command::new("ffmpeg")
            .arg("-version")
            .output()
            .context("Failed to execute 'ffmpeg'. Is it in your PATH?")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("ffmpeg returned error status"));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let first_line = stdout.lines().next().unwrap_or_default();
        let version_token = first_line.split_whitespace().nth(2).unwrap_or_default();
        let major_version = version_token
            .split('.')
            .next()
            .and_then(|value| value.parse::<u32>().ok());

        if !matches!(major_version, Some(7..)) {
            tracing::warn!("FFmpeg version might be older than 7.1. Recommended: 7.1+");
        }
        Ok(())
    }

    pub fn binary() -> &'static str {
        "ffmpeg"
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenderMode {
    Preview,
    Export,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum HardwareAccel {
    #[default]
    Auto,
    Software,
}

/// Encoder mode for video encoding with hardware acceleration support.
#[derive(Debug, Clone, Copy, Default)]
pub enum EncoderMode {
    /// Automatically select best available encoder (NVENC → QSV → AMF → libx264).
    #[default]
    Auto,
    /// Force software encoding (libx264).
    Software,
    /// Force NVIDIA NVENC hardware encoding.
    Nvenc,
    /// Force Intel QSV hardware encoding.
    Qsv,
    /// Force AMD AMF hardware encoding.
    Amf,
}

pub struct EncoderSettings {
    pub width: usize,
    pub height: usize,
    pub sample_rate: i32,
    pub hardware_accel: HardwareAccel,
    pub fps: f64,
    pub encoder_mode: EncoderMode,
    pub alpha: bool,
    /// Software x264 only. Hardware qp and the alpha VP9 path stay put.
    pub draft: bool,
}

/// Software x264 preset and CRF. Draft is ultrafast / 30. The default is slow / 18.
pub fn software_x264_quality(draft: bool) -> (&'static str, &'static str) {
    if draft {
        ("ultrafast", "30")
    } else {
        ("slow", "18")
    }
}

impl EncoderSettings {
    pub fn preset_h264_yuv420p(w: usize, h: usize) -> Self {
        Self {
            width: w,
            height: h,
            sample_rate: 48000,
            hardware_accel: HardwareAccel::Auto,
            fps: 30.0,
            encoder_mode: EncoderMode::Auto,
            alpha: false,
            draft: false,
        }
    }

    pub fn with_fps(mut self, fps: f64) -> Self {
        self.fps = if fps.is_finite() && fps > 0.0 { fps } else { 30.0 };
        self
    }

    pub fn with_encoder_mode(mut self, mode: EncoderMode) -> Self {
        self.encoder_mode = mode;
        self
    }

    pub fn with_alpha(mut self, alpha: bool) -> Self {
        self.alpha = alpha;
        self
    }

    pub fn with_draft(mut self, draft: bool) -> Self {
        self.draft = draft;
        self
    }
}

/// Information about the current encoder configuration.
#[derive(Debug, Clone)]
pub struct EncoderInfo {
    pub codec: String,
    pub is_hardware: bool,
    pub encoder_name: String,
    pub available_encoders: Vec<String>,
    /// Warnings about encoders that are available but not working.
    pub warnings: Vec<String>,
}

/// Detect available hardware encoders by testing FFmpeg.
/// Only returns encoders that actually work (driver compatibility verified).
pub fn detect_available_encoders() -> Vec<&'static str> {
    let mut available = vec![];

    // Test NVENC (NVIDIA) - verify it actually works
    if test_encoder_available("h264_nvenc") && test_encoder_works("h264_nvenc") {
        available.push("nvenc");
    }

    // Test Intel QSV - verify it actually works
    if test_encoder_available("h264_qsv") && test_encoder_works("h264_qsv") {
        available.push("qsv");
    }

    // Test AMD AMF - verify it actually works
    if test_encoder_available("h264_amf") && test_encoder_works("h264_amf") {
        available.push("amf");
    }

    // Software encoder is always available
    available.push("software");

    available
}

/// Test if a specific encoder is available in FFmpeg.
fn test_encoder_available(encoder: &str) -> bool {
    let output = Command::new(FFmpegDriver::binary())
        .arg("-encoders")
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.contains(encoder)
        }
        _ => false,
    }
}

/// Test if encoder actually works by running a test encode.
fn test_encoder_works(encoder: &str) -> bool {
    // Try to run ffmpeg with the encoder to verify it actually works
    let output = Command::new(FFmpegDriver::binary())
        .arg("-f")
        .arg("lavfi")
        .arg("-i")
        .arg("testsrc=duration=1:size=32x32:rate=1")
        .arg("-c:v")
        .arg(encoder)
        .arg("-frames:v")
        .arg("1")
        .arg("-f")
        .arg("null")
        .arg("-")
        .output();

    match output {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Check for common failure patterns
            let driver_errors = [
                "Driver does not support",
                "The minimum required Nvidia driver",
                "DLL amfrt64.dll failed to open",
                "Error while opening encoder",
            ];

            for error in &driver_errors {
                if stderr.contains(error) {
                    return false;
                }
            }
            output.status.success()
        }
        Err(_) => false,
    }
}

/// Select the best available encoder based on priority and actual testing.
fn select_best_encoder() -> &'static str {
    // Priority: NVENC → QSV → AMF → libx264
    // Test each encoder to ensure it actually works (driver compatibility)

    if test_encoder_available("h264_nvenc") && test_encoder_works("h264_nvenc") {
        return "h264_nvenc";
    }

    if test_encoder_available("h264_qsv") && test_encoder_works("h264_qsv") {
        return "h264_qsv";
    }

    if test_encoder_available("h264_amf") && test_encoder_works("h264_amf") {
        return "h264_amf";
    }

    "libx264"
}

/// Detect why an encoder failed and return a helpful message.
pub fn detect_encoder_failure_reason(encoder: &str) -> Option<String> {
    let output = Command::new(FFmpegDriver::binary())
        .arg("-f")
        .arg("lavfi")
        .arg("-i")
        .arg("testsrc=duration=1:size=32x32:rate=1")
        .arg("-c:v")
        .arg(encoder)
        .arg("-frames:v")
        .arg("1")
        .arg("-f")
        .arg("null")
        .arg("-")
        .output();

    match output {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);

            if stderr.contains("Driver does not support the required nvenc API version") {
                return Some(format!(
                    "NVIDIA driver outdated. {} Please update from nvidia.com/drivers",
                    stderr
                        .lines()
                        .find(|l| l.contains("Required:"))
                        .unwrap_or("")
                ));
            }

            if stderr.contains("DLL amfrt64.dll failed to open") {
                return Some("AMD GPU drivers not installed or runtime missing".to_string());
            }

            if stderr.contains("Error while opening encoder") {
                return Some(format!(
                    "{} encoder initialization failed (driver/hardware issue)",
                    encoder
                ));
            }

            None
        }
        Err(e) => Some(format!("Failed to test encoder: {}", e)),
    }
}

/// Get encoder codec string based on mode.
pub fn get_encoder_codec(mode: EncoderMode) -> &'static str {
    match mode {
        EncoderMode::Auto => select_best_encoder(),
        EncoderMode::Software => "libx264",
        EncoderMode::Nvenc => "h264_nvenc",
        EncoderMode::Qsv => "h264_qsv",
        EncoderMode::Amf => "h264_amf",
    }
}

/// Get human-readable encoder name.
pub fn get_encoder_name(codec: &str) -> &'static str {
    match codec {
        "h264_nvenc" => "NVIDIA NVENC",
        "h264_qsv" => "Intel Quick Sync",
        "h264_amf" => "AMD AMF",
        _ => "Software (libx264)",
    }
}

/// Get encoder info for diagnostics.
pub fn get_encoder_info(mode: EncoderMode) -> EncoderInfo {
    let available = detect_available_encoders();
    let codec = get_encoder_codec(mode);
    let is_hardware = codec != "libx264";
    let name = get_encoder_name(codec);

    // Collect warnings for encoders that are available but not working
    let mut warnings = vec![];

    // Check each potential hardware encoder
    if test_encoder_available("h264_nvenc") && !test_encoder_works("h264_nvenc") {
        if let Some(reason) = detect_encoder_failure_reason("h264_nvenc") {
            warnings.push(format!("NVENC unavailable: {}", reason));
        }
    }

    if test_encoder_available("h264_qsv") && !test_encoder_works("h264_qsv") {
        if let Some(reason) = detect_encoder_failure_reason("h264_qsv") {
            warnings.push(format!("QSV unavailable: {}", reason));
        }
    }

    if test_encoder_available("h264_amf") && !test_encoder_works("h264_amf") {
        if let Some(reason) = detect_encoder_failure_reason("h264_amf") {
            warnings.push(format!("AMF unavailable: {}", reason));
        }
    }

    EncoderInfo {
        codec: codec.to_string(),
        is_hardware,
        encoder_name: name.to_string(),
        available_encoders: available.iter().map(|s| s.to_string()).collect(),
        warnings,
    }
}

/// Encodes video via pipe, writes audio to temp file, then muxes.
#[allow(dead_code)]
pub struct Encoder {
    video_process: std::process::Child,
    audio_file: BufWriter<File>,
    audio_path: PathBuf,
    video_temp_path: PathBuf,
    dest_path: PathBuf,
    width: usize,
    height: usize,
    sample_rate: i32,
    audio_codec: &'static str,
}

impl Encoder {
    pub fn new(dest: &Path, settings: EncoderSettings) -> Result<Self> {
        let temp_dir = std::env::temp_dir();
        let uuid = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_nanos();

        let (video_temp_path, audio_codec) = if settings.alpha {
            (temp_dir.join(format!("director_vid_{}.webm", uuid)), "libopus")
        } else {
            (temp_dir.join(format!("director_vid_{}.mp4", uuid)), "aac")
        };
        let audio_path = temp_dir.join(format!("director_aud_{}.raw", uuid));

        let mut cmd = Command::new(FFmpegDriver::binary());
        cmd.arg("-y")
            .arg("-f")
            .arg("rawvideo")
            .arg("-pixel_format")
            .arg("rgba")
            .arg("-video_size")
            .arg(format!("{}x{}", settings.width, settings.height))
            .arg("-framerate")
            .arg(settings.fps.to_string())
            .arg("-i")
            .arg("pipe:0");

        if settings.alpha {
            let codec = "libvpx-vp9";
            tracing::info!("Initializing video encoder: {} (Software, Alpha)", codec);
            cmd.arg("-c:v")
                .arg(codec)
                .arg("-pix_fmt")
                .arg("yuva420p")
                .arg("-color_range")
                .arg("pc")
                .arg("-lossless")
                .arg("1")
                .arg("-cpu-used")
                .arg("2")
                .arg("-row-mt")
                .arg("1");
        } else {
            let codec = get_encoder_codec(settings.encoder_mode);
            let is_hardware = codec != "libx264";

            tracing::info!(
                "Initializing video encoder: {} ({})",
                codec,
                if is_hardware { "Hardware" } else { "Software" }
            );

            cmd.arg("-c:v")
                .arg(codec)
                .arg("-pix_fmt")
                .arg("yuv420p")
                .arg("-color_range")
                .arg("pc")
                .arg("-colorspace")
                .arg("bt709")
                .arg("-color_primaries")
                .arg("bt709")
                .arg("-color_trc")
                .arg("bt709");

            if is_hardware {
                match codec {
                    "h264_nvenc" => {
                        cmd.arg("-preset").arg("slow")
                            .arg("-rc").arg("constqp")
                            .arg("-qp").arg("20")
                            .arg("-bf").arg("2");
                    }
                    "h264_qsv" => {
                        cmd.arg("-preset").arg("veryslow")
                            .arg("-global_quality").arg("20");
                    }
                    "h264_amf" => {
                        cmd.arg("-rc").arg("cqp")
                            .arg("-qp_i").arg("20")
                            .arg("-qp_p").arg("20");
                    }
                    _ => {}
                }
            } else {
                let (preset, crf) = software_x264_quality(settings.draft);
                cmd.arg("-preset").arg(preset)
                    .arg("-crf").arg(crf)
                    .arg("-profile:v").arg("high")
                    .arg("-tune").arg("film");
            }
        }

        let video_process = cmd
            .arg(&video_temp_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Failed to spawn ffmpeg video encoder")?;

        let audio_file = File::create(&audio_path)?;

        Ok(Self {
            video_process,
            audio_file: BufWriter::new(audio_file),
            audio_path,
            video_temp_path,
            dest_path: dest.to_path_buf(),
            width: settings.width,
            height: settings.height,
            sample_rate: settings.sample_rate,
            audio_codec,
        })
    }

    /// Takes RGBA frame. Ignores timestamp (assumed constant FPS).
    pub fn encode(&mut self, frame_data: &[u8], _time: f64) -> Result<()> {
        if let Some(stdin) = self.video_process.stdin.as_mut() {
            stdin.write_all(frame_data)?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Encoder stdin closed"))
        }
    }

    /// Takes Audio samples (Interleaved Stereo F32).
    pub fn encode_audio(&mut self, samples: &[f32], _time: f64) -> Result<()> {
        // Write as raw f32le
        for sample in samples {
            self.audio_file.write_all(&sample.to_le_bytes())?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        // 1. Finish Video
        self.video_process.stdin = None; // EOF
        let status = self.video_process.wait()?;
        if !status.success() {
            return Err(anyhow::anyhow!("Video encoding failed"));
        }

        // 2. Finish Audio
        self.audio_file.flush()?;
        drop(self.audio_file); // Close file

        // 3. Mux
        // ffmpeg -y -i video.mp4 -f f32le -ar 48000 -ac 2 -i audio.raw -c:v copy -c:a aac output.mp4
        tracing::info!("Muxing audio and video...");
        let mux_status = Command::new(FFmpegDriver::binary())
            .arg("-y")
            .arg("-i")
            .arg(&self.video_temp_path)
            .arg("-f")
            .arg("f32le")
            .arg("-ar")
            .arg(self.sample_rate.to_string())
            .arg("-ac")
            .arg("2")
            .arg("-i")
            .arg(&self.audio_path)
            .arg("-c:v")
            .arg("copy")
            .arg("-c:a")
            .arg(self.audio_codec)
            .arg(&self.dest_path)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()?;

        // Cleanup
        let _ = std::fs::remove_file(&self.video_temp_path);
        let _ = std::fs::remove_file(&self.audio_path);

        if mux_status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Muxing failed"))
        }
    }
}

// --- Decoding & Proxy System ---

pub struct ProxyManager;

impl ProxyManager {
    pub fn get_or_create_proxy(original: &Path) -> PathBuf {
        let hash = match std::fs::metadata(original) {
            Ok(m) => calculate_hash(
                &original.to_string_lossy(),
                m.modified().unwrap_or(SystemTime::now()),
            ),
            Err(_) => return original.to_path_buf(),
        };

        let mut proxy_path = std::env::temp_dir();
        proxy_path.push("director_proxies");
        let _ = std::fs::create_dir_all(&proxy_path);
        proxy_path.push(format!("proxy_{:x}.mp4", hash));

        if proxy_path.exists() {
            return proxy_path;
        }

        tracing::info!("Generating I-frame proxy for: {:?}", original);
        // Using -g 1 for all-intra (instant seek)
        let status = Command::new(FFmpegDriver::binary())
            .arg("-i")
            .arg(original)
            .arg("-vf")
            .arg("scale=-2:720")
            .arg("-c:v")
            .arg("libx264")
            .arg("-g")
            .arg("1")
            .arg("-tune")
            .arg("fastdecode")
            .arg("-y")
            .arg(&proxy_path)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .status();

        match status {
            Ok(s) if s.success() => proxy_path,
            _ => {
                tracing::error!("Failed to create proxy, using original");
                original.to_path_buf()
            }
        }
    }
}

fn calculate_hash<T: std::hash::Hash>(t: &T, time: SystemTime) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    time.hash(&mut s);
    s.finish()
}

pub enum VideoCommand {
    GetFrame(f64),
}

#[derive(Debug)]
pub enum VideoResponse {
    Frame(f64, Vec<u8>, u32, u32),
    Error(String),
}

/// Maximum frames held per decoder cache. Covers freeze frames and the
/// motion-blur sub-samples that re-request nearby timestamps.
const MAX_CACHED_FRAMES: usize = 8;
/// Sequential reads farther ahead than this restart the decode pipe instead
/// of discarding intermediate frames one by one.
const MAX_SKIP_AHEAD: u64 = 60;

/// A persistent `ffmpeg` decode pipe for one video source.
///
/// Export renders in timeline order, so the common case is a sequential read
/// from a single long-lived child process — one spawn per source instead of
/// one per frame. Random seeks (timeline jumps, scrubbing) restart the child
/// at the new position.
struct DecodeSession {
    child: std::process::Child,
    reader: BufReader<std::process::ChildStdout>,
    /// Source frame index the next `read_frame` call will return.
    next_index: u64,
    frame_len: usize,
}

impl DecodeSession {
    fn start(path: &Path, seek_time: f64, start_index: u64, frame_len: usize) -> Result<Self> {
        let mut child = Command::new(FFmpegDriver::binary())
            .arg("-ss")
            .arg(seek_time.max(0.0).to_string())
            .arg("-i")
            .arg(path)
            .arg("-f")
            .arg("rawvideo")
            .arg("-pix_fmt")
            .arg("rgba")
            .arg("-")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("Failed to spawn ffmpeg decoder for {}", path.display()))?;
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow::anyhow!("ffmpeg decoder stdout unavailable"));
        };
        Ok(Self {
            child,
            reader: BufReader::new(stdout),
            next_index: start_index,
            frame_len,
        })
    }

    fn read_frame(&mut self) -> Result<Vec<u8>> {
        let mut buffer = vec![0u8; self.frame_len];
        self.reader.read_exact(&mut buffer).with_context(|| {
            format!(
                "ffmpeg decode pipe ended at source frame {}",
                self.next_index
            )
        })?;
        self.next_index += 1;
        Ok(buffer)
    }
}

impl Drop for DecodeSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn cache_insert(
    cache: &mut HashMap<u64, Vec<u8>>,
    order: &mut VecDeque<u64>,
    index: u64,
    frame: Vec<u8>,
) {
    if cache.contains_key(&index) {
        return;
    }
    cache.insert(index, frame);
    order.push_back(index);
    while order.len() > MAX_CACHED_FRAMES {
        if let Some(old) = order.pop_front() {
            cache.remove(&old);
        }
    }
}

#[derive(Debug)]
pub struct ThreadedDecoder {
    cmd_tx: Option<Sender<VideoCommand>>,
    resp_rx: Option<Receiver<VideoResponse>>,
    worker: Option<thread::JoinHandle<()>>,
    source_fps: f64,
    _mode: RenderMode,
}

impl ThreadedDecoder {
    pub fn new(path: PathBuf, mode: RenderMode) -> Result<Self> {
        let (cmd_tx, cmd_rx) = unbounded();
        let (resp_tx, resp_rx) = bounded(5);

        let video_path = if mode == RenderMode::Preview {
            ProxyManager::get_or_create_proxy(&path)
        } else {
            path
        };
        let probed = probe_video(&video_path)?;
        let (width, height, fps) = (probed.width, probed.height, probed.fps);
        let frame_len = width as usize * height as usize * 4;

        let worker = thread::spawn(move || {
            let mut session: Option<DecodeSession> = None;
            let mut cache: HashMap<u64, Vec<u8>> = HashMap::new();
            let mut order: VecDeque<u64> = VecDeque::new();

            let restart =
                |session: &mut Option<DecodeSession>, time: f64, index: u64| -> Result<()> {
                    *session = None;
                    *session = Some(DecodeSession::start(&video_path, time, index, frame_len)?);
                    Ok(())
                };

            // Coalesce queued seeks: only the latest scrub position matters.
            while let Ok(VideoCommand::GetFrame(mut target_time)) = cmd_rx.recv() {
                while let Ok(VideoCommand::GetFrame(t)) = cmd_rx.try_recv() {
                    target_time = t;
                }

                let index = crate::timeline::video_source_frame_index(target_time.max(0.0), fps);
                if let Some(hit) = cache.get(&index) {
                    let _ = resp_tx.send(VideoResponse::Frame(
                        target_time,
                        hit.clone(),
                        width,
                        height,
                    ));
                    continue;
                }

                let need_restart = match &session {
                    None => true,
                    Some(s) => index < s.next_index || index - s.next_index > MAX_SKIP_AHEAD,
                };
                if need_restart {
                    // Seek to the exact frame-grid time, not the raw request time.
                    let seek = crate::timeline::video_seek_time(target_time, fps);
                    if let Err(e) = restart(&mut session, seek, index) {
                        let _ = resp_tx.send(VideoResponse::Error(e.to_string()));
                        continue;
                    }
                }

                let mut failed: Option<String> = None;
                if let Some(s) = session.as_mut() {
                    while s.next_index < index {
                        if s.read_frame().is_err() {
                            failed = Some(format!(
                                "ffmpeg decode pipe ended while skipping to {:.3}s",
                                target_time
                            ));
                            break;
                        }
                    }
                }
                if let Some(err) = failed {
                    session = None;
                    let _ = resp_tx.send(VideoResponse::Error(err));
                    continue;
                }
                match session.as_mut().unwrap().read_frame() {
                    Ok(bytes) => {
                        if bytes.len() != frame_len {
                            session = None;
                            let _ = resp_tx.send(VideoResponse::Error(format!(
                                "Decoded frame had {} bytes, expected {} for {}x{} RGBA",
                                bytes.len(),
                                frame_len,
                                width,
                                height
                            )));
                        } else {
                            cache_insert(&mut cache, &mut order, index, bytes.clone());
                            let _ = resp_tx.send(VideoResponse::Frame(
                                target_time,
                                bytes,
                                width,
                                height,
                            ));
                        }
                    }
                    Err(e) => {
                        session = None;
                        let _ = resp_tx.send(VideoResponse::Error(e.to_string()));
                    }
                }
            }
        });

        Ok(Self {
            cmd_tx: Some(cmd_tx),
            resp_rx: Some(resp_rx),
            worker: Some(worker),
            source_fps: fps,
            _mode: mode,
        })
    }

    /// Probed source frame rate driving seek quantization.
    pub fn source_fps(&self) -> f64 {
        self.source_fps
    }

    pub fn send_request(&self, time: f64) {
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(VideoCommand::GetFrame(time));
        }
    }

    pub fn get_response(&self) -> Option<VideoResponse> {
        self.resp_rx.as_ref()?.try_recv().ok()
    }
}

impl Drop for ThreadedDecoder {
    fn drop(&mut self) {
        // Disconnect both channels before joining: the worker may be blocked
        // receiving a command or sending to a full response queue.
        self.cmd_tx.take();
        self.resp_rx.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Probed video-stream metadata: dimensions plus the real source frame rate.
///
/// The frame rate drives `video_seek_time` quantization in `VideoNode`, so a
/// 24/25/29.97/60 fps source must not be sampled on an assumed 30 fps grid.
#[derive(Debug, Clone, Copy)]
pub struct VideoProbe {
    pub width: u32,
    pub height: u32,
    /// Source frames per second. Falls back to 30.0 when ffprobe reports
    /// `0/0` or an unparsable rate.
    pub fps: f64,
}

/// Parses an ffprobe `avg_frame_rate` / `r_frame_rate` token (`"num/den"`).
/// Returns `None` for `0/0`, zero denominators, and unparsable input.
pub fn parse_frame_rate(token: &str) -> Option<f64> {
    let token = token.trim();
    let (num, den) = token.split_once('/')?;
    let num: f64 = num.trim().parse().ok()?;
    let den: f64 = den.trim().parse().ok()?;
    if !num.is_finite() || !den.is_finite() || num <= 0.0 || den <= 0.0 {
        return None;
    }
    let fps = num / den;
    if fps.is_finite() && fps > 0.0 {
        Some(fps)
    } else {
        None
    }
}

fn probe_frame_rate(path: &Path) -> f64 {
    for entry in ["avg_frame_rate", "r_frame_rate"] {
        let output = Command::new("ffprobe")
            .arg("-v")
            .arg("error")
            .arg("-select_streams")
            .arg("v:0")
            .arg("-show_entries")
            .arg(format!("stream={entry}"))
            .arg("-of")
            .arg("csv=p=0")
            .arg(path)
            .output();
        if let Ok(output) = output {
            if output.status.success() {
                let token = String::from_utf8_lossy(&output.stdout);
                if let Some(fps) = parse_frame_rate(token.trim()) {
                    return fps;
                }
            }
        }
    }
    tracing::warn!(
        "Could not probe source fps for {:?}; falling back to 30.0",
        path
    );
    30.0
}

/// Probes dimensions and source frame rate via ffprobe.
pub fn probe_video(path: &Path) -> Result<VideoProbe> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("stream=width,height")
        .arg("-of")
        .arg("csv=s=x:p=0")
        .arg(path)
        .output()?;

    let out = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = out.trim().split('x').collect();
    if parts.len() == 2 {
        let width = parts[0].parse()?;
        let height = parts[1].parse()?;
        let fps = probe_frame_rate(path);
        Ok(VideoProbe { width, height, fps })
    } else {
        Err(anyhow::anyhow!("Failed to probe video size"))
    }
}

pub fn probe(path: &Path) -> Result<(u32, u32)> {
    let probed = probe_video(path)?;
    Ok((probed.width, probed.height))
}

#[derive(Debug)]
pub struct SyncDecoder {
    path: PathBuf,
    width: u32,
    height: u32,
    fps: f64,
    frame_len: usize,
    session: Option<DecodeSession>,
    cache: HashMap<u64, Vec<u8>>,
    order: VecDeque<u64>,
}

// `DecodeSession` holds a live child process + pipe reader; `SyncDecoder` is
// only ever driven from the thread that owns its `VideoNode`.
impl std::fmt::Debug for DecodeSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecodeSession")
            .field("next_index", &self.next_index)
            .field("frame_len", &self.frame_len)
            .finish()
    }
}

impl SyncDecoder {
    pub fn new(path: PathBuf) -> Result<Self> {
        let probed = probe_video(&path)?;
        let frame_len = probed.width as usize * probed.height as usize * 4;
        if frame_len == 0 {
            return Err(anyhow::anyhow!(
                "Probed zero-size video stream for {}",
                path.display()
            ));
        }
        Ok(Self {
            path,
            width: probed.width,
            height: probed.height,
            fps: probed.fps,
            frame_len,
            session: None,
            cache: HashMap::new(),
            order: VecDeque::new(),
        })
    }

    /// Probed source frame rate driving seek quantization.
    pub fn source_fps(&self) -> f64 {
        self.fps
    }

    fn restart(&mut self, seek_time: f64, index: u64) -> Result<()> {
        self.session = None;
        self.session = Some(DecodeSession::start(
            &self.path,
            seek_time,
            index,
            self.frame_len,
        )?);
        Ok(())
    }

    pub fn get_frame_at(&mut self, time: f64) -> Result<(f64, Vec<u8>, u32, u32)> {
        let index = crate::timeline::video_source_frame_index(time.max(0.0), self.fps);
        if let Some(hit) = self.cache.get(&index) {
            return Ok((time, hit.clone(), self.width, self.height));
        }

        let need_restart = match &self.session {
            None => true,
            Some(s) => index < s.next_index || index - s.next_index > MAX_SKIP_AHEAD,
        };
        if need_restart {
            // Seek to the exact frame-grid time, not the raw request time.
            let seek = crate::timeline::video_seek_time(time, self.fps);
            self.restart(seek, index)?;
        }

        // Discard intermediate frames on small forward jumps.
        loop {
            let next = self.session.as_ref().map(|s| s.next_index).unwrap_or(index);
            if next >= index {
                break;
            }
            let discard = self.session.as_mut().expect("decode session").read_frame();
            if let Err(e) = discard {
                self.session = None;
                return Err(e);
            }
        }

        let bytes = match self.session.as_mut() {
            Some(s) => s.read_frame(),
            None => return Err(anyhow::anyhow!("decode session unavailable")),
        };
        match bytes {
            Ok(bytes) => {
                if bytes.len() != self.frame_len {
                    self.session = None;
                    return Err(anyhow::anyhow!(
                        "Decoded frame had {} bytes, expected {} for {}x{} RGBA",
                        bytes.len(),
                        self.frame_len,
                        self.width,
                        self.height
                    ));
                }
                cache_insert(&mut self.cache, &mut self.order, index, bytes.clone());
                Ok((time, bytes, self.width, self.height))
            }
            Err(e) => {
                self.session = None;
                Err(e)
            }
        }
    }
}

#[derive(Debug)]
pub enum VideoLoader {
    Threaded(ThreadedDecoder),
    Sync(SyncDecoder),
}

impl VideoLoader {
    pub fn new(path: PathBuf, mode: RenderMode) -> Result<Self> {
        FFmpegDriver::ensure_available()?;
        match mode {
            RenderMode::Preview => Ok(Self::Threaded(ThreadedDecoder::new(path, mode)?)),
            RenderMode::Export => Ok(Self::Sync(SyncDecoder::new(path)?)),
        }
    }

    /// Probed source frame rate of the underlying stream.
    pub fn source_fps(&self) -> f64 {
        match self {
            Self::Threaded(d) => d.source_fps(),
            Self::Sync(d) => d.source_fps(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        parse_frame_rate, DecodeSession, FFmpegDriver, RenderMode, SyncDecoder, ThreadedDecoder,
        VideoResponse, MAX_CACHED_FRAMES, MAX_SKIP_AHEAD,
    };
    use std::process::Command;
    #[cfg(unix)]
    use std::process::Stdio;
    use std::time::Duration;

    #[test]
    fn draft_software_x264_is_ultrafast_crf_30_and_the_default_stays_slow() {
        assert_eq!(super::software_x264_quality(false), ("slow", "18"));
        assert_eq!(super::software_x264_quality(true), ("ultrafast", "30"));
        let settings = super::EncoderSettings::preset_h264_yuv420p(64, 64);
        assert!(!settings.draft);
        assert!(settings.with_draft(true).draft);
    }

    #[test]
    fn parses_ffprobe_frame_rates() {
        assert!((parse_frame_rate("30/1").unwrap() - 30.0).abs() < 1e-9);
        assert!((parse_frame_rate("30000/1001").unwrap() - 29.97002997).abs() < 1e-6);
        assert!((parse_frame_rate("24000/1001").unwrap() - 23.976023976).abs() < 1e-6);
        assert!((parse_frame_rate("25/1").unwrap() - 25.0).abs() < 1e-9);
    }

    #[test]
    fn rejects_degenerate_frame_rates() {
        assert!(parse_frame_rate("0/0").is_none());
        assert!(parse_frame_rate("30/0").is_none());
        assert!(parse_frame_rate("0/1").is_none());
        assert!(parse_frame_rate("").is_none());
        assert!(parse_frame_rate("not-a-rate").is_none());
    }

    const TEST_WIDTH: u32 = 32;
    const TEST_HEIGHT: u32 = 32;
    const TEST_FRAME_COUNT: u64 = 90;

    fn expected_frame(index: u64) -> Vec<u8> {
        [index as u8, 255 - index as u8, 73, 255].repeat((TEST_WIDTH * TEST_HEIGHT) as usize)
    }

    fn video_fixture(rate: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        FFmpegDriver::ensure_available().expect("decoder tests require FFmpeg in PATH");
        let dir = tempfile::tempdir().expect("decoder fixture directory");
        let raw = dir.path().join("frames.rgba");
        let src = dir.path().join("src.nut");
        let frames: Vec<u8> = (0..TEST_FRAME_COUNT).flat_map(expected_frame).collect();
        std::fs::write(&raw, frames).expect("write numbered RGBA frames");
        let output = Command::new(FFmpegDriver::binary())
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "rawvideo",
                "-pixel_format",
                "rgba",
                "-video_size",
                "32x32",
                "-framerate",
                rate,
                "-i",
            ])
            .arg(raw)
            .args(["-c:v", "ffv1", "-pix_fmt", "bgr0"])
            .arg(&src)
            .output()
            .expect("generate lossless decoder fixture");
        assert!(
            output.status.success(),
            "fixture generation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        (dir, src)
    }

    // Observe the child independently of the decoder, including after its
    // owned Child handle has been dropped. No test-only decoder API is needed.
    fn process_exists(pid: u32) -> bool {
        #[cfg(windows)]
        {
            let output = Command::new("tasklist")
                .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
                .output()
                .expect("query decoder process");
            assert!(output.status.success(), "tasklist failed");
            String::from_utf8_lossy(&output.stdout).lines().any(|line| {
                line.split(',').nth(1).map(|field| field.trim_matches('"'))
                    == Some(pid.to_string().as_str())
            })
        }
        #[cfg(unix)]
        {
            Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("query decoder process")
                .success()
        }
    }

    fn assert_process_exited(pid: u32) {
        assert!(
            !process_exists(pid),
            "FFmpeg process {pid} survived session cleanup"
        );
    }

    fn assert_decoded_frame(decoder: &mut SyncDecoder, index: u64) {
        let time = index as f64 / decoder.source_fps();
        let (returned_time, bytes, width, height) = decoder
            .get_frame_at(time)
            .unwrap_or_else(|err| panic!("decode frame {index}: {err:#}"));
        assert_eq!(returned_time, time);
        assert_eq!((width, height), (TEST_WIDTH, TEST_HEIGHT));
        assert_eq!(
            bytes,
            expected_frame(index),
            "wrong source frame at {time}s"
        );
    }

    fn check_sync_decoder(rate: &str, fps: f64) {
        let (_dir, src) = video_fixture(rate);
        let mut decoder = SyncDecoder::new(src).expect("decoder");
        assert!((decoder.source_fps() - fps).abs() < 1e-6);
        assert_decoded_frame(&mut decoder, 0);
        let first_pid = decoder.session.as_ref().unwrap().child.id();
        for index in 1..=MAX_CACHED_FRAMES as u64 {
            assert_decoded_frame(&mut decoder, index);
            assert_eq!(
                decoder.session.as_ref().unwrap().child.id(),
                first_pid,
                "sequential decode must reuse the child process"
            );
        }
        assert_eq!(decoder.cache.len(), MAX_CACHED_FRAMES);
        assert!(
            !decoder.cache.contains_key(&0),
            "backward seek must miss the cache"
        );
        let next_index = decoder.session.as_ref().unwrap().next_index;
        assert_decoded_frame(&mut decoder, MAX_CACHED_FRAMES as u64);
        assert_eq!(
            decoder.session.as_ref().unwrap().next_index,
            next_index,
            "cache hit must not consume the pipe"
        );
        assert_decoded_frame(&mut decoder, 0);
        let backward_pid = decoder.session.as_ref().unwrap().child.id();
        assert_ne!(
            backward_pid, first_pid,
            "uncached backward seek must restart"
        );
        assert_process_exited(first_pid);

        assert_decoded_frame(&mut decoder, 3);
        assert_eq!(
            decoder.session.as_ref().unwrap().child.id(),
            backward_pid,
            "small forward skip must reuse the pipe"
        );
        let far_index = MAX_SKIP_AHEAD + 10;
        assert_decoded_frame(&mut decoder, far_index);
        let far_pid = decoder.session.as_ref().unwrap().child.id();
        assert_ne!(far_pid, backward_pid, "large forward seek must restart");
        assert_process_exited(backward_pid);
        assert_decoded_frame(&mut decoder, far_index + 1);
        assert_eq!(decoder.session.as_ref().unwrap().child.id(), far_pid);
        drop(decoder);
        assert_process_exited(far_pid);
    }

    #[test]
    fn sync_decoder_reuses_and_seeks_at_24_fps() {
        check_sync_decoder("24", 24.0);
    }

    #[test]
    fn sync_decoder_reuses_and_seeks_at_25_fps() {
        check_sync_decoder("25", 25.0);
    }

    #[test]
    fn sync_decoder_reuses_and_seeks_at_ntsc_fps() {
        check_sync_decoder("30000/1001", 30000.0 / 1001.0);
    }

    #[test]
    fn sync_decoder_reuses_and_seeks_at_30_fps() {
        check_sync_decoder("30", 30.0);
    }

    #[test]
    fn sync_decoder_reuses_and_seeks_at_60_fps() {
        check_sync_decoder("60", 60.0);
    }

    #[test]
    fn decode_session_drop_stops_a_live_child() {
        let (_dir, src) = video_fixture("30");
        let session =
            DecodeSession::start(&src, 0.0, 0, expected_frame(0).len()).expect("decode session");
        let pid = session.child.id();
        assert!(process_exists(pid), "session must own a live FFmpeg child");
        drop(session);
        assert_process_exited(pid);
    }

    #[test]
    fn sync_decoder_cleans_up_read_errors_and_recovers() {
        let (_dir, src) = video_fixture("30");
        let mut decoder = SyncDecoder::new(src).expect("decoder");
        assert_decoded_frame(&mut decoder, TEST_FRAME_COUNT - 1);
        let pid = decoder.session.as_ref().unwrap().child.id();
        assert!(decoder
            .get_frame_at(TEST_FRAME_COUNT as f64 / 30.0)
            .is_err());
        assert!(
            decoder.session.is_none(),
            "EOF must discard the failed session"
        );
        assert_process_exited(pid);
        assert_decoded_frame(&mut decoder, 0);
        assert_decoded_frame(&mut decoder, MAX_SKIP_AHEAD);
        let pid = decoder.session.as_ref().unwrap().child.id();
        // The same EOF cleanup must apply while discarding skipped frames.
        assert!(decoder
            .get_frame_at((TEST_FRAME_COUNT + 1) as f64 / 30.0)
            .is_err());
        assert!(decoder.session.is_none());
        assert_process_exited(pid);
        assert_decoded_frame(&mut decoder, 1);
    }

    #[test]
    fn threaded_decoder_decodes_and_joins_on_drop_with_full_response_queue() {
        let (_dir, src) = video_fixture("25");
        // Export mode avoids proxy generation so this tests the worker itself.
        let decoder = ThreadedDecoder::new(src, RenderMode::Export).expect("threaded decoder");
        assert!((decoder.source_fps() - 25.0).abs() < 1e-6);
        decoder.send_request(0.0);
        let rx = decoder.resp_rx.as_ref().unwrap();
        match rx
            .recv_timeout(Duration::from_secs(10))
            .expect("worker response")
        {
            VideoResponse::Frame(time, bytes, width, height) => {
                assert_eq!(time, 0.0);
                assert_eq!((width, height), (TEST_WIDTH, TEST_HEIGHT));
                assert_eq!(bytes, expected_frame(0));
            }
            VideoResponse::Error(err) => panic!("worker decode failed: {err}"),
        }
        // Fill the response queue without draining it. A join before dropping
        // the receiver would deadlock on the worker's next blocking send.
        for expected_len in 1..=5 {
            decoder.send_request(0.0);
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while rx.len() < expected_len {
                assert!(
                    std::time::Instant::now() < deadline,
                    "worker failed to fill queue"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        decoder.send_request(0.0);
        let (done_tx, done_rx) = crossbeam_channel::bounded(1);
        let dropper = std::thread::spawn(move || {
            drop(decoder);
            done_tx.send(()).unwrap();
        });
        done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("decoder drop must join without deadlock");
        dropper.join().unwrap();
    }
}
