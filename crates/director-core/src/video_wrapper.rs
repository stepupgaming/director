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
use std::io::{BufWriter, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
// use std::sync::{Arc, Mutex};
// actually logs say unused.

use std::thread;
use std::time::SystemTime;

/// Global FFmpeg driver.
pub struct FFmpegDriver;

impl FFmpegDriver {
    /// Checks once per process that `ffmpeg` runs, and warns below 7.1.
    pub fn ensure_available() -> Result<()> {
        static CHECKED: OnceLock<std::result::Result<(), String>> = OnceLock::new();
        CHECKED.get_or_init(|| Self::check().map_err(|error| format!("{error:#}"))).clone().map_err(anyhow::Error::msg)
    }

    fn check() -> Result<()> {
        let output = Self::command()
            .arg("-version")
            .output()
            .context("Failed to execute 'ffmpeg'. Is it in your PATH?")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("ffmpeg returned error status"));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        // FFmpeg 7.1 ships libavcodec 61.19.
        if !matches!(libavcodec_version(&stdout), Some(v) if v >= (61, 19)) {
            tracing::warn!("FFmpeg version might be older than 7.1. Recommended: 7.1+");
        }
        Ok(())
    }

    pub fn binary() -> &'static str {
        "ffmpeg"
    }

    /// An FFmpeg command, counted as a spawn.
    pub fn command() -> Command {
        crate::counters::count(crate::counters::Counter::ffmpeg_spawns);
        Command::new(Self::binary())
    }
}

/// An ffprobe command, counted as a spawn.
pub fn ffprobe() -> Command {
    crate::counters::count(crate::counters::Counter::ffprobe_spawns);
    Command::new("ffprobe")
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
/// ffmpeg's 128k AAC default smears music; 256k is transparent for stereo AAC and Opus.
const AUDIO_BITRATE: &str = "256k";

fn test_encoder_available(encoder: &str) -> bool {
    let output = FFmpegDriver::command()
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
    let output = FFmpegDriver::command()
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
    let output = FFmpegDriver::command()
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

/// Where an [`Encoder`] takes its audio from.
enum AudioSink {
    /// Streamed per frame into a raw file that `finish` muxes in a second FFmpeg pass.
    Streamed(BufWriter<File>),
    /// Read by the encoding FFmpeg process as a second input.
    Input(AudioInput),
    /// No audio stream: FFmpeg writes the video alone to the destination.
    Silent,
}

/// How [`Encoder::spawn`] takes its audio.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AudioMode {
    Streamed,
    Premixed,
    Silent,
}

/// FFmpeg flags that keep its stderr to errors: no banner, no encoder stats.
const QUIET: [&str; 4] = ["-hide_banner", "-nostats", "-loglevel", "error"];

/// A loopback TCP listener that the encoding FFmpeg process connects to at
/// startup and reads interleaved stereo f32 from, so FFmpeg starts before the
/// audio is mixed.
struct AudioInput {
    listener: Option<TcpListener>,
    address: SocketAddr,
    closed: Arc<AtomicBool>,
    feeder: Option<thread::JoinHandle<Result<()>>>,
}

impl AudioInput {
    fn bind() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").context("bind the FFmpeg audio input")?;
        Ok(Self {
            address: listener.local_addr()?,
            listener: Some(listener),
            closed: Arc::new(AtomicBool::new(false)),
            feeder: None,
        })
    }

    fn url(&self) -> String {
        format!("tcp://{}", self.address)
    }

    /// Accept the FFmpeg connection on a thread and write `samples` to it, then close it.
    fn send(&mut self, samples: Vec<f32>) -> Result<()> {
        let listener = self.listener.take().context("the audio was already sent")?;
        let closed = self.closed.clone();
        self.feeder = Some(thread::spawn(move || {
            let (stream, _) = listener.accept()?;
            if closed.load(Ordering::Acquire) {
                return Ok(());
            }
            let mut stream = BufWriter::with_capacity(1 << 16, stream);
            for sample in &samples {
                stream.write_all(&sample.to_le_bytes())?;
            }
            stream.flush()?;
            Ok(())
        }));
        Ok(())
    }

    /// Wake a feeder still waiting for FFmpeg and return how it ended.
    fn close(&mut self) -> Result<()> {
        self.closed.store(true, Ordering::Release);
        let Some(feeder) = self.feeder.take() else {
            return Ok(());
        };
        if !feeder.is_finished() {
            let _ = TcpStream::connect(self.address);
        }
        feeder.join().map_err(|_| anyhow::anyhow!("the audio feeder panicked"))?
    }
}

/// Encodes video via pipe. Audio is either streamed to a temp file and muxed
/// after encoding ([`Encoder::new`]) or premixed and encoded in the same
/// FFmpeg process ([`Encoder::with_audio`], [`Encoder::with_audio_input`]).
#[allow(dead_code)]
pub struct Encoder {
    video_process: std::process::Child,
    audio: AudioSink,
    audio_path: PathBuf,
    video_temp_path: PathBuf,
    dest_path: PathBuf,
    width: usize,
    height: usize,
    sample_rate: i32,
    audio_codec: &'static str,
}

impl Encoder {
    /// Stream audio per frame with [`Encoder::encode_audio`]; `finish` muxes it in a second pass.
    pub fn new(dest: &Path, settings: EncoderSettings) -> Result<Self> {
        Self::spawn(dest, settings, AudioMode::Streamed)
    }

    /// Encode video frames with no audio stream in the output.
    pub fn video_only(dest: &Path, settings: EncoderSettings) -> Result<Self> {
        Self::spawn(dest, settings, AudioMode::Silent)
    }

    /// Encode `samples` (interleaved stereo f32 at `settings.sample_rate`) in the
    /// same FFmpeg process as the video, so `finish` only waits for it.
    pub fn with_audio(dest: &Path, settings: EncoderSettings, samples: &[f32]) -> Result<Self> {
        let mut encoder = Self::with_audio_input(dest, settings)?;
        encoder.send_audio(samples.to_vec())?;
        Ok(encoder)
    }

    /// Start FFmpeg before the audio exists. Pass the whole track (interleaved
    /// stereo f32 at `settings.sample_rate`) to [`Encoder::send_audio`] once,
    /// before [`Encoder::finish`]; video frames may be encoded before and after.
    pub fn with_audio_input(dest: &Path, settings: EncoderSettings) -> Result<Self> {
        Self::spawn(dest, settings, AudioMode::Premixed)
    }

    /// Feed the whole premixed track to an encoder from [`Encoder::with_audio_input`].
    pub fn send_audio(&mut self, samples: Vec<f32>) -> Result<()> {
        let AudioSink::Input(input) = &mut self.audio else {
            anyhow::bail!("this encoder streams its audio per frame");
        };
        input.send(samples)
    }

    fn spawn(dest: &Path, settings: EncoderSettings, mode: AudioMode) -> Result<Self> {
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

        let mut cmd = FFmpegDriver::command();
        cmd.args(QUIET)
            .arg("-y")
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
        let input = if mode == AudioMode::Premixed { Some(AudioInput::bind()?) } else { None };
        if let Some(input) = &input {
            cmd.arg("-f")
                .arg("f32le")
                .arg("-ar")
                .arg(settings.sample_rate.to_string())
                .arg("-ac")
                .arg("2")
                .arg("-i")
                .arg(input.url());
        }

        if settings.alpha {
            let codec = "libvpx-vp9";
            tracing::info!("Initializing video encoder: {} (Software, Alpha)", codec);
            // FFmpeg before 7.1 only tags the output range and converts to
            // limited range, so the scale filter converts to full range itself.
            cmd.arg("-vf")
                .arg("scale=out_range=full")
                .arg("-c:v")
                .arg(codec)
                .arg("-pix_fmt")
                .arg("yuva420p")
                .arg("-color_range")
                .arg("pc")
                .arg("-lossless")
                .arg("1")
                // Lossless output decodes to the same pixels at every speed; the
                // realtime speed only grows the file. Speed 2 took 18 s for a
                // 30 s 1080x1920 layer, realtime speed 8 takes about 3 s.
                .arg("-deadline")
                .arg("realtime")
                .arg("-cpu-used")
                .arg("8")
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

            // FFmpeg 7.1+ takes primaries and transfer from the frames and
            // ignores the output flags below, so tag the frames first. FFmpeg
            // before 7.1 converts to limited-range BT.601 whatever the flags
            // say, so the scale filter names the range and matrix.
            cmd.arg("-vf")
                .arg("setparams=color_primaries=bt709:color_trc=bt709,scale=out_color_matrix=bt709:out_range=full")
                .arg("-c:v")
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

        let audio = if let Some(input) = input {
            cmd.arg("-map")
                .arg("0:v")
                .arg("-map")
                .arg("1:a")
                .arg("-c:a")
                .arg(audio_codec)
                .arg("-b:a")
                .arg(AUDIO_BITRATE)
                .arg(dest);
            AudioSink::Input(input)
        } else if mode == AudioMode::Silent {
            cmd.arg("-map").arg("0:v").arg(dest);
            AudioSink::Silent
        } else {
            cmd.arg(&video_temp_path);
            AudioSink::Streamed(BufWriter::new(File::create(&audio_path)?))
        };
        let video_process = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Failed to spawn ffmpeg video encoder")?;

        Ok(Self {
            video_process,
            audio,
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
        let AudioSink::Streamed(audio_file) = &mut self.audio else {
            anyhow::bail!("this encoder took its audio premixed");
        };
        // Write as raw f32le
        for sample in samples {
            audio_file.write_all(&sample.to_le_bytes())?;
        }
        Ok(())
    }

    /// Stop FFmpeg and delete every file this encoder wrote, including a partial `dest`.
    pub fn abort(mut self) {
        let _ = self.video_process.kill();
        let _ = self.video_process.wait();
        let _ = std::fs::remove_file(&self.audio_path);
        let _ = std::fs::remove_file(&self.video_temp_path);
        match &mut self.audio {
            AudioSink::Input(input) => {
                let _ = input.close();
                let _ = std::fs::remove_file(&self.dest_path);
            }
            AudioSink::Silent => {
                let _ = std::fs::remove_file(&self.dest_path);
            }
            AudioSink::Streamed(_) => {}
        }
    }

    pub fn finish(mut self) -> Result<()> {
        if let AudioSink::Input(input) = &self.audio {
            if input.listener.is_some() {
                self.abort();
                anyhow::bail!("the premixed audio was never sent");
            }
        }
        // 1. Finish Video
        self.video_process.stdin = None; // EOF
        let status = self.video_process.wait()?;
        let mut audio_file = match self.audio {
            AudioSink::Streamed(audio_file) => audio_file,
            AudioSink::Input(mut input) => {
                let fed = input.close();
                anyhow::ensure!(status.success(), "Video encoding failed");
                return fed.context("feed the audio to FFmpeg");
            }
            AudioSink::Silent => {
                anyhow::ensure!(status.success(), "Video encoding failed");
                return Ok(());
            }
        };
        if !status.success() {
            return Err(anyhow::anyhow!("Video encoding failed"));
        }

        // 2. Finish Audio
        audio_file.flush()?;
        drop(audio_file); // Close file

        // 3. Mux
        // ffmpeg -y -i video.mp4 -f f32le -ar 48000 -ac 2 -i audio.raw -c:v copy -c:a aac output.mp4
        tracing::info!("Muxing audio and video...");
        let mux_status = FFmpegDriver::command()
            .args(QUIET)
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
            .arg("-b:a")
            .arg(AUDIO_BITRATE)
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
        let status = FFmpegDriver::command()
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
    Frame(f64, Arc<Vec<u8>>, u32, u32),
    Error(String),
}

/// Fewest frames cached per decode cursor of a source. Covers freeze frames
/// and the motion-blur sub-samples that re-request nearby timestamps.
const MIN_CACHED_FRAMES: usize = 8;
/// Most frames cached per decode cursor. Export workers ask for neighbouring
/// frames out of order, and a request behind the cache restarts a decoder.
const MAX_CACHED_FRAMES: usize = 32;
/// Bytes the cache of one cursor may hold between those bounds: 32 frames at
/// 1080p and 8 at 4K.
const CACHE_BYTES: usize = 256 << 20;
/// Sequential reads farther ahead than this restart the decode pipe instead
/// of discarding intermediate frames one by one.
const MAX_SKIP_AHEAD: u64 = 60;
/// Decode cursors one source keeps open: elements that play the file at
/// different positions each read their own pipe.
const MAX_CURSORS: usize = 6;
/// A cursor no request used within this many requests to its source closes.
const IDLE_REQUESTS: u64 = 240;
/// Spare frame buffers a source keeps for the next reads.
const SPARE_BUFFERS: usize = 4;

/// Frames one cursor of a source with `frame_len` byte frames caches.
fn cache_frames(frame_len: usize) -> usize {
    (CACHE_BYTES / frame_len).clamp(MIN_CACHED_FRAMES, MAX_CACHED_FRAMES)
}

/// FFmpeg decode sessions this process has started, for waste reports.
pub fn decode_sessions_started() -> u64 {
    crate::counters::snapshot().decode_sessions
}

/// Frames the reader thread of a decode session holds ahead of its requests,
/// so the pipe keeps moving while the requesters paint.
const READ_AHEAD: usize = 4;

/// A persistent `ffmpeg` decode pipe for one video source.
///
/// Export renders in timeline order, so the common case is a sequential read
/// from a single long-lived child process: one spawn per source position
/// instead of one per frame. Random seeks (timeline jumps, scrubbing) open a
/// session at the new position.
struct DecodeSession {
    child: std::process::Child,
    /// Frames its reader thread has read, in order. Closed at the stream's end.
    frames: Option<Receiver<std::io::Result<Vec<u8>>>>,
    /// Buffers handed back for the reader's next frames.
    spare: Sender<Vec<u8>>,
    reader: Option<thread::JoinHandle<()>>,
    /// Source frame index the session's first frame carries.
    start_index: u64,
    /// Source frame index the next `read_frame` call will return.
    next_index: u64,
}

impl DecodeSession {
    fn start(path: &Path, seek_time: f64, start_index: u64, frame_len: usize) -> Result<Self> {
        let mut child = FFmpegDriver::command()
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
        crate::counters::count(crate::counters::Counter::decode_sessions);
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow::anyhow!("ffmpeg decoder stdout unavailable"));
        };
        let (frame_tx, frames) = bounded(READ_AHEAD);
        let (spare, spare_rx) = unbounded::<Vec<u8>>();
        let mut pipe = stdout;
        let reader = thread::spawn(move || loop {
            let mut buffer = spare_rx.try_recv().unwrap_or_else(|_| vec![0; frame_len]);
            let read = match pipe.read_exact(&mut buffer) {
                Ok(()) => {
                    crate::counters::count(crate::counters::Counter::frames_decoded);
                    crate::counters::add(crate::counters::Counter::decoded_bytes, buffer.len() as u64);
                    Ok(buffer)
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => Err(e),
            };
            let failed = read.is_err();
            if frame_tx.send(read).is_err() || failed {
                break;
            }
        });
        Ok(Self {
            child,
            frames: Some(frames),
            spare,
            reader: Some(reader),
            start_index,
            next_index: start_index,
        })
    }

    /// The next frame, or `None` once the stream has ended. `buffer` goes
    /// back to the reader for a later frame.
    fn read_frame(&mut self, buffer: Vec<u8>) -> Result<Option<Vec<u8>>> {
        let _ = self.spare.send(buffer);
        match self.frames.as_ref().map(Receiver::recv) {
            Some(Ok(Ok(frame))) => {
                self.next_index += 1;
                Ok(Some(frame))
            }
            Some(Ok(Err(e))) => {
                Err(e).with_context(|| format!("ffmpeg decode pipe failed at source frame {}", self.next_index))
            }
            _ => Ok(None),
        }
    }
}

impl Drop for DecodeSession {
    fn drop(&mut self) {
        // Frames read ahead that nobody took. Closing the channel frees a
        // reader waiting to hand one over.
        if let Some(frames) = self.frames.take() {
            let unread = frames.try_iter().filter(Result::is_ok).count();
            crate::counters::add(crate::counters::Counter::frames_discarded, unread as u64);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

/// Frames before the container's duration where `FrameSource::find_end`
/// starts decoding the tail.
const TAIL_FRAMES: u64 = 8;

/// A decode session and the request count when it last served a frame.
#[derive(Debug)]
struct Cursor {
    session: DecodeSession,
    used: u64,
}

/// The frames of one source by index: up to [`MAX_CURSORS`] decode pipes at
/// different positions, a cache of `cached` frames per cursor
/// and the stream's end once a read runs past it. A request reads from the
/// nearest cursor at or up to [`MAX_SKIP_AHEAD`] frames before it, or opens
/// a cursor at its frame, closing the least recently used one when all are
/// open.
#[derive(Debug)]
struct FrameSource {
    path: PathBuf,
    fps: f64,
    frame_len: usize,
    cursors: Vec<Cursor>,
    /// Requests served, the clock of `Cursor::used`.
    requests: u64,
    /// Frames the cache holds per cursor, from [`CACHE_BYTES`].
    cached: usize,
    cache: HashMap<u64, Arc<Vec<u8>>>,
    order: VecDeque<u64>,
    /// Buffers of evicted or skipped frames, reused by the next reads.
    spare: Vec<Vec<u8>>,
    /// One past the last frame FFmpeg decodes, learned when a read first
    /// runs off the end. Requests at or past it get the last frame, as an
    /// ended video holds its last frame, without another decode.
    end: Option<u64>,
    /// Decode sessions this source started, shared with its registry.
    started: Arc<AtomicU64>,
}

impl FrameSource {
    fn new(path: PathBuf, probed: &VideoProbe, started: Arc<AtomicU64>) -> Result<Self> {
        let frame_len = probed.width as usize * probed.height as usize * 4;
        if frame_len == 0 {
            return Err(anyhow::anyhow!("Probed zero-size video stream for {}", path.display()));
        }
        Ok(Self {
            path,
            fps: probed.fps,
            frame_len,
            cursors: Vec::new(),
            requests: 0,
            cached: cache_frames(frame_len),
            cache: HashMap::new(),
            order: VecDeque::new(),
            spare: Vec::new(),
            end: None,
            started,
        })
    }

    /// The frame at `index`, or the last frame when `index` is past the end.
    fn frame(&mut self, index: u64) -> Result<Arc<Vec<u8>>> {
        let held = |end: Option<u64>| end.map_or(index, |end| index.min(end.saturating_sub(1)));
        if let Some(bytes) = self.decode(held(self.end))? {
            return Ok(bytes);
        }
        if self.end.is_none() {
            self.end = self.find_end()?;
        }
        match self.end {
            Some(end) if end > 0 && index >= end => self.decode(end - 1)?.ok_or_else(|| {
                anyhow::anyhow!("ffmpeg decode pipe ended at source frame {}", end - 1)
            }),
            _ => Err(anyhow::anyhow!("ffmpeg decode pipe ended at source frame {index}")),
        }
    }

    fn start_session(&self, index: u64) -> Result<DecodeSession> {
        // Seek to the exact frame-grid time, not the raw request time.
        let seek = crate::timeline::frame_to_time(index, self.fps);
        let session = DecodeSession::start(&self.path, seek, index, self.frame_len)?;
        self.started.fetch_add(1, Ordering::Relaxed);
        Ok(session)
    }

    fn buffer(&mut self) -> Vec<u8> {
        self.spare.pop().unwrap_or_else(|| vec![0; self.frame_len])
    }

    fn recycle(&mut self, buffer: Vec<u8>) {
        if self.spare.len() < SPARE_BUFFERS {
            self.spare.push(buffer);
        }
    }

    fn cache_insert(&mut self, index: u64, frame: Arc<Vec<u8>>) {
        if self.cache.contains_key(&index) {
            return;
        }
        self.cache.insert(index, frame);
        self.order.push_back(index);
        while self.order.len() > self.cached * self.cursors.len().max(1) {
            let Some(old) = self.order.pop_front() else { break };
            if let Some(frame) = self.cache.remove(&old).and_then(|frame| Arc::try_unwrap(frame).ok()) {
                self.recycle(frame);
            }
        }
    }

    /// The cursor that reaches `index` by reading forward, or a new one
    /// opened at `index`.
    fn cursor_for(&mut self, index: u64) -> Result<usize> {
        let now = self.requests;
        self.cursors.retain(|cursor| now - cursor.used <= IDLE_REQUESTS);
        let reaches = |cursor: &Cursor| {
            let next = cursor.session.next_index;
            next <= index && index - next <= MAX_SKIP_AHEAD
        };
        let nearest = (0..self.cursors.len())
            .filter(|&i| reaches(&self.cursors[i]))
            .max_by_key(|&i| self.cursors[i].session.next_index);
        if let Some(i) = nearest {
            return Ok(i);
        }
        if self.cursors.len() >= MAX_CURSORS {
            let oldest = (0..self.cursors.len()).min_by_key(|&i| self.cursors[i].used).expect("open cursors");
            self.cursors.swap_remove(oldest);
        }
        let session = self.start_session(index)?;
        self.cursors.push(Cursor { session, used: now });
        Ok(self.cursors.len() - 1)
    }

    /// The frame at `index`, or `None` when the stream ends before it. A
    /// session that decoded frames before ending marks the exact end.
    fn decode(&mut self, index: u64) -> Result<Option<Arc<Vec<u8>>>> {
        self.requests += 1;
        if let Some(hit) = self.cache.get(&index) {
            return Ok(Some(Arc::clone(hit)));
        }
        let slot = self.cursor_for(index)?;
        self.cursors[slot].used = self.requests;
        loop {
            let buffer = self.buffer();
            let session = &mut self.cursors[slot].session;
            let read = session.next_index;
            match session.read_frame(buffer) {
                Ok(Some(bytes)) if read == index => {
                    let bytes = Arc::new(bytes);
                    self.cache_insert(index, Arc::clone(&bytes));
                    return Ok(Some(bytes));
                }
                // A skipped frame just before the target may be the next
                // request of a worker running behind; farther ones go back.
                Ok(Some(bytes)) if index - read < self.cached as u64 && !self.cache.contains_key(&read) => {
                    self.cache_insert(read, Arc::new(bytes));
                }
                Ok(Some(bytes)) => {
                    crate::counters::count(crate::counters::Counter::frames_discarded);
                    self.recycle(bytes)
                }
                Ok(None) => {
                    let session = self.cursors.swap_remove(slot).session;
                    if session.next_index > session.start_index {
                        self.end = Some(session.next_index);
                    }
                    return Ok(None);
                }
                Err(e) => {
                    self.cursors.swap_remove(slot);
                    return Err(e);
                }
            }
        }
    }

    /// One past the last frame, found by decoding the tail from a little
    /// before the container's duration, or from the start when that seek
    /// lands past the end. Keeps the last frame in the cache.
    fn find_end(&mut self) -> Result<Option<u64>> {
        let guess = probe_duration(&self.path).map(|d| ((d * self.fps).floor() as u64).saturating_sub(TAIL_FRAMES));
        let mut starts = vec![guess.unwrap_or(0), 0];
        starts.dedup();
        for start in starts {
            let mut session = self.start_session(start)?;
            let (mut last, mut next) = (None, vec![0; self.frame_len]);
            while let Some(bytes) = session.read_frame(next)? {
                next = last.replace(bytes).unwrap_or_else(|| vec![0; self.frame_len]);
            }
            if let Some(bytes) = last {
                self.cache_insert(session.next_index - 1, Arc::new(bytes));
                return Ok(Some(session.next_index));
            }
        }
        Ok(None)
    }
}

/// One probed source and its frames, shared by every element that plays it.
#[derive(Debug)]
pub struct SharedSource {
    width: u32,
    height: u32,
    fps: f64,
    frames: Mutex<FrameSource>,
}

impl SharedSource {
    fn open(path: PathBuf, started: Arc<AtomicU64>) -> Result<Self> {
        let probed = probe_video(&path)?;
        Ok(Self {
            width: probed.width,
            height: probed.height,
            fps: probed.fps,
            frames: Mutex::new(FrameSource::new(path, &probed, started)?),
        })
    }

    fn frames(&self) -> std::sync::MutexGuard<'_, FrameSource> {
        self.frames.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Export-mode video sources by canonical path. Every element that plays a
/// file through one registry probes it once and reads one [`SharedSource`]:
/// elements at the same position share a decode pipe and its cached frames,
/// and elements at other positions read their own cursors of it. A
/// `director-render` `Renderer` hands one registry to all its export
/// workers. The registry holds sources weakly, so a source closes with the
/// last element that plays it.
#[derive(Clone, Debug, Default)]
pub struct FrameSources {
    sources: Arc<Mutex<HashMap<PathBuf, Weak<SharedSource>>>>,
    started: Arc<AtomicU64>,
    audio: Arc<Mutex<HashMap<AudioKey, Weak<Vec<f32>>>>>,
    audio_decoded: Arc<AtomicU64>,
}

/// Asset id, skipped source seconds as bits, and sample rate of decoded audio.
type AudioKey = (String, u64, u32);

impl FrameSources {
    /// The open source of `path`, probed on first use.
    pub fn source(&self, path: &Path) -> Result<Arc<SharedSource>> {
        let key = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let mut sources = self.sources.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(source) = sources.get(&key).and_then(Weak::upgrade) {
            return Ok(source);
        }
        let source = Arc::new(SharedSource::open(path.to_path_buf(), Arc::clone(&self.started))?);
        sources.retain(|_, source| source.strong_count() > 0);
        sources.insert(key, Arc::downgrade(&source));
        Ok(source)
    }

    /// Sources open now.
    pub fn open_sources(&self) -> usize {
        let sources = self.sources.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        sources.values().filter(|source| source.strong_count() > 0).count()
    }

    /// FFmpeg decode sessions the sources of this registry have started.
    pub fn sessions_started(&self) -> u64 {
        self.started.load(Ordering::Relaxed)
    }

    /// Decoded samples of audio asset `id` of the one document this registry
    /// serves, with `source_start` seconds skipped at `sample_rate`. `decode`
    /// runs for the first import and again once every track holding the
    /// samples is gone; imports that arrive meanwhile wait for it.
    pub fn audio_samples<E>(
        &self,
        id: &str,
        source_start: f64,
        sample_rate: u32,
        decode: impl FnOnce() -> std::result::Result<Vec<f32>, E>,
    ) -> std::result::Result<Arc<Vec<f32>>, E> {
        let key = (id.to_string(), source_start.to_bits(), sample_rate);
        let mut audio = self.audio.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(samples) = audio.get(&key).and_then(Weak::upgrade) {
            return Ok(samples);
        }
        let samples = Arc::new(decode()?);
        self.audio_decoded.fetch_add(1, Ordering::Relaxed);
        audio.retain(|_, samples| samples.strong_count() > 0);
        audio.insert(key, Arc::downgrade(&samples));
        Ok(samples)
    }

    /// Audio assets this registry has decoded.
    pub fn audio_decoded(&self) -> u64 {
        self.audio_decoded.load(Ordering::Relaxed)
    }
}

/// The container duration in seconds from ffprobe.
fn probe_duration(path: &Path) -> Option<f64> {
    let output = ffprobe()
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok().filter(|d: &f64| d.is_finite() && *d > 0.0)
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
        let mut source = FrameSource::new(video_path, &probed, Arc::default())?;

        let worker = thread::spawn(move || {
            // Coalesce queued seeks: only the latest scrub position matters.
            while let Ok(VideoCommand::GetFrame(mut target_time)) = cmd_rx.recv() {
                while let Ok(VideoCommand::GetFrame(t)) = cmd_rx.try_recv() {
                    target_time = t;
                }
                let index = crate::timeline::video_source_frame_index(target_time.max(0.0), fps);
                let response = match source.frame(index) {
                    Ok(bytes) => VideoResponse::Frame(target_time, bytes, width, height),
                    Err(e) => VideoResponse::Error(e.to_string()),
                };
                let _ = resp_tx.send(response);
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

/// Reads `(major, minor)` from the `libavcodec` line of `ffmpeg -version`.
/// Release and git builds (`N-123778-g...`) both print it.
pub fn libavcodec_version(version_output: &str) -> Option<(u32, u32)> {
    let rest = version_output
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("libavcodec"))?;
    // "     62. 29.101 / 62. 29.101": compile-time version before the slash.
    let mut fields = rest.split('/').next()?.split('.').map(str::trim);
    let major = fields.next()?.parse().ok()?;
    let minor = fields.next()?.parse().ok()?;
    Some((major, minor))
}

/// Parses an ffprobe `avg_frame_rate` / `r_frame_rate` token (`"num/den"`).
/// Returns `None` for `0/0`, zero denominators, and unparsable input.
/// A stream with side data (an ICC profile, a display matrix) makes the CSV
/// writer append an empty field (`"30/1,"`), so only the first field counts.
pub fn parse_frame_rate(token: &str) -> Option<f64> {
    let token = token.trim().split(',').next()?.trim();
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
        let output = ffprobe()
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
    let output = ffprobe()
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
    let (width, height) =
        parse_video_size(&out).ok_or_else(|| anyhow::anyhow!("Failed to probe video size"))?;
    let fps = probe_frame_rate(path);
    Ok(VideoProbe { width, height, fps })
}

/// Parses ffprobe `stream=width,height` output in `csv=s=x:p=0` form
/// (`"1920x1080"`). Side data adds an empty trailing field (`"1920x1080x"`),
/// so the first two fields are read and the rest ignored.
pub fn parse_video_size(out: &str) -> Option<(u32, u32)> {
    let mut fields = out.trim().split('x').map(str::trim);
    let width = fields.next()?.parse().ok()?;
    let height = fields.next()?.parse().ok()?;
    Some((width, height))
}

pub fn probe(path: &Path) -> Result<(u32, u32)> {
    let probed = probe_video(path)?;
    Ok((probed.width, probed.height))
}

/// Export-mode frames of one source, read on the calling thread. Clones
/// share the source; [`FrameSources`] shares it across elements.
#[derive(Clone, Debug)]
pub struct SyncDecoder {
    source: Arc<SharedSource>,
}

// `DecodeSession` holds a live child process + pipe reader, driven under its
// `SharedSource` lock.
impl std::fmt::Debug for DecodeSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecodeSession")
            .field("start_index", &self.start_index)
            .field("next_index", &self.next_index)
            .finish()
    }
}

impl SyncDecoder {
    /// A decoder with a source of its own.
    pub fn new(path: PathBuf) -> Result<Self> {
        Ok(Self {
            source: Arc::new(SharedSource::open(path, Arc::default())?),
        })
    }

    /// A decoder reading the source of `path` in `sources`.
    pub fn shared(path: &Path, sources: &FrameSources) -> Result<Self> {
        Ok(Self {
            source: sources.source(path)?,
        })
    }

    /// Probed source frame rate driving seek quantization.
    pub fn source_fps(&self) -> f64 {
        self.source.fps
    }

    /// The frame at `time`, or the last frame once `time` is past the end.
    pub fn get_frame_at(&mut self, time: f64) -> Result<(f64, Arc<Vec<u8>>, u32, u32)> {
        let index = crate::timeline::video_source_frame_index(time.max(0.0), self.source.fps);
        let bytes = self.source.frames().frame(index)?;
        Ok((time, bytes, self.source.width, self.source.height))
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

    /// A loader whose export frames come from the source of `path` in
    /// `sources`. Preview keeps a threaded decoder of its own.
    pub fn shared(path: PathBuf, mode: RenderMode, sources: &FrameSources) -> Result<Self> {
        FFmpegDriver::ensure_available()?;
        match mode {
            RenderMode::Preview => Ok(Self::Threaded(ThreadedDecoder::new(path, mode)?)),
            RenderMode::Export => Ok(Self::Sync(SyncDecoder::shared(&path, sources)?)),
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
        libavcodec_version, parse_frame_rate, parse_video_size, DecodeSession, FFmpegDriver, FrameSources,
        RenderMode, SyncDecoder, ThreadedDecoder, VideoResponse, cache_frames, IDLE_REQUESTS, MAX_CACHED_FRAMES, MAX_CURSORS, MIN_CACHED_FRAMES,
        MAX_SKIP_AHEAD,
    };
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
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
    fn a_video_only_encoder_writes_no_audio_stream() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.mp4");
        let settings = super::EncoderSettings::preset_h264_yuv420p(16, 16).with_fps(30.0);
        let mut encoder = super::Encoder::video_only(&path, settings).unwrap();
        for frame in 0..3 {
            encoder.encode(&[0; 16 * 16 * 4], frame as f64 / 30.0).unwrap();
        }
        encoder.finish().unwrap();
        let probe = Command::new("ffprobe")
            .args(["-v", "error", "-show_entries", "stream=codec_type", "-of", "csv=p=0"])
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&probe.stdout).split_whitespace().collect::<Vec<_>>(), ["video"]);
    }

    #[test]
    fn an_audio_input_encoder_fails_without_its_track_and_aborts_without_waiting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.mp4");
        let settings = || super::EncoderSettings::preset_h264_yuv420p(16, 16).with_fps(30.0);
        let mut encoder = super::Encoder::with_audio_input(&path, settings()).unwrap();
        encoder.encode(&[0; 16 * 16 * 4], 0.0).unwrap();
        assert!(encoder.finish().is_err());
        assert!(!path.exists());
        for sent in [false, true] {
            let mut encoder = super::Encoder::with_audio_input(&path, settings()).unwrap();
            if sent {
                encoder.send_audio(vec![0.0; 1 << 20]).unwrap();
            }
            encoder.abort();
            assert!(!path.exists());
        }
        let mut encoder = super::Encoder::with_audio_input(&path, settings()).unwrap();
        encoder.encode(&[0; 16 * 16 * 4], 0.0).unwrap();
        encoder.send_audio(vec![0.0; 3200]).unwrap();
        encoder.encode(&[0; 16 * 16 * 4], 1.0 / 30.0).unwrap();
        encoder.finish().unwrap();
        let probe = Command::new("ffprobe")
            .args(["-v", "error", "-show_entries", "stream=codec_type", "-of", "csv=p=0"])
            .arg(&path)
            .output()
            .expect("ffprobe must be installed for encoder tests");
        assert_eq!(String::from_utf8_lossy(&probe.stdout).split_whitespace().collect::<Vec<_>>(), ["video", "audio"]);
    }

    #[test]
    fn parses_ffprobe_frame_rates() {
        assert!((parse_frame_rate("30/1").unwrap() - 30.0).abs() < 1e-9);
        assert!((parse_frame_rate("30000/1001").unwrap() - 29.97002997).abs() < 1e-6);
        assert!((parse_frame_rate("24000/1001").unwrap() - 23.976023976).abs() < 1e-6);
        assert!((parse_frame_rate("25/1").unwrap() - 25.0).abs() < 1e-9);
    }

    #[test]
    fn reads_probe_output_with_a_side_data_field() {
        assert!((parse_frame_rate("30/1,\r\n").unwrap() - 30.0).abs() < 1e-9);
        assert_eq!(parse_video_size("1920x1080x\r\n"), Some((1920, 1080)));
        assert_eq!(parse_video_size("1280x720\n"), Some((1280, 720)));
        assert_eq!(parse_video_size("x\n"), None);
        assert_eq!(parse_video_size(""), None);
    }

    #[test]
    fn reads_libavcodec_version_from_release_and_git_builds() {
        let git = "ffmpeg version N-123778-g3b55818764-20260331 Copyright (c) 2000-2026\r\nlibavutil      60. 26.100 / 60. 26.100\r\nlibavcodec     62. 29.101 / 62. 29.101\r\n";
        assert_eq!(libavcodec_version(git), Some((62, 29)));
        let release = "ffmpeg version 7.1 Copyright (c) 2000-2024\nlibavcodec     61. 19.100 / 61. 19.100\n";
        assert_eq!(libavcodec_version(release), Some((61, 19)));
        assert!(libavcodec_version("ffmpeg version 7.0\nlibavcodec     61.  3.100 / 61.  3.100\n").unwrap() < (61, 19));
        assert_eq!(libavcodec_version("ffmpeg version 7.1\n"), None);
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

    // Windows can list a killed process for a moment while the system
    // releases it, so the check allows two seconds under a loaded test run.
    fn assert_process_exited(pid: u32) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while process_exists(pid) {
            assert!(
                std::time::Instant::now() < deadline,
                "FFmpeg process {pid} survived session cleanup"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    fn assert_decoded_frame(decoder: &mut SyncDecoder, index: u64) {
        let time = index as f64 / decoder.source_fps();
        let (returned_time, bytes, width, height) = decoder
            .get_frame_at(time)
            .unwrap_or_else(|err| panic!("decode frame {index}: {err:#}"));
        assert_eq!(returned_time, time);
        assert_eq!((width, height), (TEST_WIDTH, TEST_HEIGHT));
        assert_eq!(*bytes, expected_frame(index), "wrong source frame at {time}s");
    }

    /// FFmpeg processes of the decoder's open cursors.
    fn pids(decoder: &SyncDecoder) -> Vec<u32> {
        decoder.source.frames().cursors.iter().map(|cursor| cursor.session.child.id()).collect()
    }

    fn started(decoder: &SyncDecoder) -> u64 {
        decoder.source.frames().started.load(Ordering::Relaxed)
    }

    fn check_sync_decoder(rate: &str, fps: f64) {
        let (_dir, src) = video_fixture(rate);
        let mut decoder = SyncDecoder::new(src).expect("decoder");
        // The fixture's small frames would cache the most; test eviction at the least.
        decoder.source.frames().cached = MIN_CACHED_FRAMES;
        assert!((decoder.source_fps() - fps).abs() < 1e-6);
        assert_decoded_frame(&mut decoder, 0);
        let first = pids(&decoder);
        assert_eq!(first.len(), 1);
        for index in 1..=MIN_CACHED_FRAMES as u64 {
            assert_decoded_frame(&mut decoder, index);
            assert_eq!(pids(&decoder), first, "sequential decode must reuse the child process");
        }
        {
            let source = decoder.source.frames();
            assert_eq!(source.cache.len(), MIN_CACHED_FRAMES);
            assert!(!source.cache.contains_key(&0), "the oldest frame must leave the cache");
        }
        let next = |decoder: &SyncDecoder| {
            decoder.source.frames().cursors.iter().map(|cursor| cursor.session.next_index).collect::<Vec<_>>()
        };
        let before = next(&decoder);
        assert_decoded_frame(&mut decoder, MIN_CACHED_FRAMES as u64);
        assert_eq!(next(&decoder), before, "cache hit must not consume the pipe");

        // An uncached backward seek opens a second cursor and keeps the first.
        assert_decoded_frame(&mut decoder, 0);
        let both = pids(&decoder);
        assert_eq!(both.len(), 2, "backward seek must open a cursor");
        assert!(both.contains(&first[0]), "backward seek must keep the first cursor");

        // A small forward skip reads on from the nearest cursor behind it.
        let near = MIN_CACHED_FRAMES as u64 + 4;
        assert_decoded_frame(&mut decoder, near);
        assert_eq!(pids(&decoder), both, "small forward skip must reuse a pipe");
        let far = near + MAX_SKIP_AHEAD + 10;
        assert_decoded_frame(&mut decoder, far);
        let three = pids(&decoder);
        assert_eq!(three.len(), 3, "large forward seek must open a cursor");
        assert_decoded_frame(&mut decoder, far + 1);
        assert_eq!(pids(&decoder), three);
        assert_eq!(started(&decoder), 3);
        drop(decoder);
        for pid in three {
            assert_process_exited(pid);
        }
    }

    #[test]
    fn the_frame_cache_of_a_cursor_follows_its_byte_budget() {
        assert_eq!(cache_frames(64 * 64 * 4), MAX_CACHED_FRAMES);
        assert_eq!(cache_frames(1920 * 1080 * 4), 32);
        assert_eq!(cache_frames(2560 * 1440 * 4), 18);
        assert_eq!(cache_frames(3840 * 2160 * 4), MIN_CACHED_FRAMES);
    }

    #[test]
    fn interleaved_positions_and_out_of_order_requests_keep_their_pipes() {
        let (_dir, src) = video_fixture("30");
        let mut decoder = SyncDecoder::new(src).expect("decoder");
        // Two elements playing positions farther apart than a skip each read their own pipe.
        for step in 0..6 {
            assert_decoded_frame(&mut decoder, step);
            assert_decoded_frame(&mut decoder, 70 + step);
        }
        let two = pids(&decoder);
        assert_eq!(two.len(), 2);
        // Export workers ask for neighbouring frames out of order.
        for index in [9, 8, 7, 6, 11, 10, 13, 12] {
            assert_decoded_frame(&mut decoder, index);
        }
        assert_eq!(pids(&decoder), two);
        assert_eq!(started(&decoder), 2, "no request may reopen a pipe");
    }

    #[test]
    fn a_source_closes_its_least_recently_used_and_idle_cursors() {
        let (_dir, src) = video_fixture("30");
        let mut decoder = SyncDecoder::new(src).expect("decoder");
        // Each request lies behind every open cursor, so each opens one.
        let starts: Vec<u64> = (0..=MAX_CURSORS as u64).map(|k| 88 - 8 * k).collect();
        let mut opened = Vec::new();
        for &index in &starts {
            assert_decoded_frame(&mut decoder, index);
            let new: Vec<u32> = pids(&decoder).into_iter().filter(|pid| !opened.contains(pid)).collect();
            opened.extend(new);
        }
        assert_eq!(pids(&decoder).len(), MAX_CURSORS);
        assert!(!pids(&decoder).contains(&opened[0]), "the least recently used cursor must close");
        assert_process_exited(opened[0]);
        // Cursors nobody reads close once a request opens or advances one.
        let last = *starts.last().unwrap();
        for k in 0..=2 * IDLE_REQUESTS {
            assert_decoded_frame(&mut decoder, last + k / 16);
        }
        let open = pids(&decoder);
        assert_eq!(open.len(), 1, "idle cursors must close");
        for pid in opened.iter().filter(|pid| !open.contains(pid)) {
            assert_process_exited(*pid);
        }
    }

    #[test]
    fn decoders_of_one_file_share_a_source_through_a_registry() {
        let (_dir, src) = video_fixture("30");
        let sources = FrameSources::default();
        let mut a = SyncDecoder::shared(&src, &sources).expect("decoder");
        let mut b = SyncDecoder::shared(&src, &sources).expect("decoder");
        assert!(Arc::ptr_eq(&a.source, &b.source));
        assert_eq!(sources.open_sources(), 1);
        for index in 0..20 {
            assert_decoded_frame(&mut a, index);
            assert_decoded_frame(&mut b, index);
        }
        assert_eq!(sources.sessions_started(), 1);
        let open = pids(&a);
        drop(a);
        assert_eq!(sources.open_sources(), 1);
        drop(b);
        assert_eq!(sources.open_sources(), 0);
        for pid in open {
            assert_process_exited(pid);
        }
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
    fn video_nodes_paint_the_decoded_frame_without_copying_it() {
        use crate::element::Element;
        use crate::node::video_node::{VideoNode, VideoSource};
        let (_dir, src) = video_fixture("30");
        let mut first = VideoNode::new(VideoSource::Path(src), RenderMode::Export);
        let mut second = first.clone();
        first.update(0.0);
        second.update(0.0);
        let (_, image) = first.get_current_frame().expect("a decoded frame");
        let (_, other) = second.get_current_frame().expect("a decoded frame");
        let pixels = image.peek_pixels().expect("raster pixels");
        assert_eq!(pixels.addr(), other.peek_pixels().expect("raster pixels").addr(), "both nodes read the decoder's buffer");
        // The image keeps the buffer after the nodes and their decoder go away.
        drop((first, second, other));
        assert_eq!(pixels.bytes().expect("pixel bytes"), expected_frame(0));
    }

    #[test]
    fn decode_session_drop_stops_a_live_child() {
        let (_dir, src) = video_fixture("30");
        let session =
            DecodeSession::start(&src, 0.0, 0, (TEST_WIDTH * TEST_HEIGHT * 4) as usize).expect("decode session");
        let pid = session.child.id();
        assert!(process_exists(pid), "session must own a live FFmpeg child");
        drop(session);
        assert_process_exited(pid);
    }

    #[test]
    fn sync_decoder_holds_the_last_frame_past_the_end_and_recovers() {
        let (_dir, src) = video_fixture("30");
        let last = TEST_FRAME_COUNT - 1;
        let frame = |decoder: &mut SyncDecoder, index: u64| {
            decoder
                .get_frame_at(index as f64 / 30.0)
                .unwrap_or_else(|err| panic!("decode frame {index}: {err:#}"))
                .1
        };
        // A sequential read runs off the end: the end is exact and the
        // session that found it is gone.
        let mut decoder = SyncDecoder::new(src.clone()).expect("decoder");
        assert_decoded_frame(&mut decoder, last);
        let pid = pids(&decoder)[0];
        assert_eq!(*frame(&mut decoder, TEST_FRAME_COUNT), expected_frame(last));
        assert_eq!(decoder.source.frames().end, Some(TEST_FRAME_COUNT));
        assert!(pids(&decoder).is_empty(), "EOF must discard the finished session");
        assert_process_exited(pid);
        // Later requests past the end hold the cached last frame without a decode.
        for index in [TEST_FRAME_COUNT + 1, TEST_FRAME_COUNT * 4] {
            assert_eq!(*frame(&mut decoder, index), expected_frame(last));
            assert!(pids(&decoder).is_empty(), "frame {index} spawned a decoder");
        }
        assert_decoded_frame(&mut decoder, 0);
        assert_decoded_frame(&mut decoder, MAX_SKIP_AHEAD);
        // A cold seek past the end decodes nothing, so the end comes from the tail.
        let mut cold = SyncDecoder::new(src).expect("decoder");
        assert_eq!(*frame(&mut cold, TEST_FRAME_COUNT * 3), expected_frame(last));
        assert_eq!(cold.source.frames().end, Some(TEST_FRAME_COUNT));
        assert_decoded_frame(&mut cold, 1);
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
                assert_eq!(*bytes, expected_frame(0));
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
