//! Director on-device test: compile an HTML scene with video and audio, export
//! it through MediaCodec, export a motion-graphics scene, and decode results back.
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use director_core::scene_import::ImportRegistry;
use director_core::video_wrapper::{EncoderMode, EncoderSettings, HardwareAccel};
use director_core::DefaultAssetLoader;

static LOG: Mutex<Option<fs::File>> = Mutex::new(None);

#[cfg(target_os = "android")]
#[link(name = "log")]
extern "C" {
    fn __android_log_write(prio: i32, tag: *const std::ffi::c_char, text: *const std::ffi::c_char) -> i32;
}

fn log(msg: String) {
    #[cfg(target_os = "android")]
    unsafe {
        let t = std::ffi::CString::new("dirapk").unwrap();
        let m = std::ffi::CString::new(msg.replace('\0', " ")).unwrap();
        __android_log_write(4, t.as_ptr(), m.as_ptr());
    }
    #[cfg(not(target_os = "android"))]
    println!("{msg}");
    if let Some(f) = LOG.lock().unwrap().as_mut() {
        let _ = writeln!(f, "{msg}");
    }
}

macro_rules! say { ($($a:tt)*) => { log(format!($($a)*)) } }

const SCENE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><style>
  html, body { margin:0; background:#0b0d12; }
  #stage { width:100%; height:100%; position:relative; font-family:Inter, sans-serif; color:#f4f1ea; }
  .bg { position:absolute; inset:0; }
  .bg video { width:100%; height:100%; object-fit:cover; }
  .pip { position:absolute; right:40px; top:40px; width:384px; height:216px; }
  .pip video { width:100%; height:100%; object-fit:cover; border-radius:12px; --grade:teal-orange; }
  h1 { position:absolute; left:60px; bottom:60px; margin:0; font-size:64px; font-weight:800;
       animation: rise 0.8s ease-out 0.3s both; }
  @keyframes rise { from { transform: translateY(80px); opacity:0; } to { transform: translateY(0); opacity:1; } }
</style></head><body>
<div id="stage" data-composition-id="android-test" data-no-timeline data-width="1280" data-height="720" data-fps="30" data-duration="6">
  <div class="bg"><video muted src="src.mp4"></video></div>
  <div class="pip"><video muted src="src.mp4"></video></div>
  <h1>Made on the phone</h1>
  <audio src="bed.mp3" data-start="0" data-duration="6" data-volume="0.4"></audio>
  <audio src="ogg.webm" data-start="1" data-duration="4" data-volume="0.6"></audio>
</div>
</body></html>"#;

const REEL: &str = include_str!("../scenes/reel.json");

fn registry() -> ImportRegistry {
    let mut reg = ImportRegistry::core();
    director_html::register_import(&mut reg);
    reg
}

fn mb(path: &Path) -> f64 {
    fs::metadata(path).map(|m| m.len() as f64 / 1e6).unwrap_or(0.0)
}

fn compile_and_export(dir: &Path) -> anyhow::Result<PathBuf> {
    let t = Instant::now();
    let mut opts = director_html::CompileOptions::default();
    opts.base_dir = dir.to_path_buf();
    let mut c = director_html::compile(SCENE, opts).map_err(|e| anyhow::anyhow!("compile: {e:?}"))?;
    say!("compiled HTML on device: {} nodes, {}x{} in {:?}", c.node_count, c.director.width, c.director.height, t.elapsed());
    let out = dir.join("scene.mp4");
    let t = Instant::now();
    director_render::export_director(&mut c.director, registry(), &out, false, 1.0, false, None)?;
    say!("exported scene.mp4 (6 s, 1280x720, video+2 audio) in {:?}, {:.2} MB", t.elapsed(), mb(&out));
    Ok(out)
}

fn export_reel(dir: &Path) -> anyhow::Result<PathBuf> {
    let doc = director_scene::SceneDocument::from_json(REEL)?;
    say!("reel document: {}x{} @ {} fps, {} nodes", doc.width, doc.height, doc.fps, doc.nodes.len());
    let r = director_render::Renderer::new(Arc::new(doc), Arc::new(DefaultAssetLoader)).with_registry(registry());
    let out = dir.join("reel.mp4");
    let t = Instant::now();
    r.export(&out, EncoderMode::Auto)?;
    say!("exported reel.mp4 in {:?}, {:.2} MB", t.elapsed(), mb(&out));
    Ok(out)
}

fn encode_speed(dir: &Path) -> anyhow::Result<()> {
    let (w, h, n) = (1920usize, 1080usize, 150usize);
    let settings = EncoderSettings {
        width: w, height: h, sample_rate: 48000, hardware_accel: HardwareAccel::Auto, fps: 30.0,
        encoder_mode: EncoderMode::Auto, alpha: false, draft: false, underlay: None,
    };
    let out = dir.join("speed1080.mp4");
    let tone: Vec<f32> = (0..48000 * 5).flat_map(|i| {
        let s = (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / 48000.0).sin() * 0.3;
        [s, s]
    }).collect();
    let mut e = director_core::video_wrapper::Encoder::with_audio(&out, settings, &tone)?;
    let mut frame = vec![0u8; w * h * 4];
    let t = Instant::now();
    for i in 0..n {
        for y in 0..h {
            for x in (0..w).step_by(1) {
                let p = (y * w + x) * 4;
                frame[p] = ((x + i * 8) % 256) as u8;
                frame[p + 1] = (y % 256) as u8;
                frame[p + 2] = ((i * 2) % 256) as u8;
                frame[p + 3] = 255;
            }
        }
        e.encode(&frame, i as f64 / 30.0)?;
    }
    e.finish()?;
    say!("encoded {n} frames 1920x1080 + 5 s tone in {:?} ({:.1} fps incl. drawing), {:.2} MB", t.elapsed(), n as f64 / t.elapsed().as_secs_f64(), mb(&out));
    Ok(())
}

fn decode_check(path: &Path, fps: f64) -> anyhow::Result<()> {
    let p = director_core::android_media::probe(path)?;
    let dur = director_core::android_media::duration(path);
    say!("probe {}: {}x{} {:.3} fps, audio {:?}, duration {:?}", path.file_name().unwrap().to_string_lossy(), p.width, p.height, p.fps, p.audio, dur);
    let (tx, rx) = crossbeam_channel::bounded(4);
    let (spare_tx, spare_rx) = crossbeam_channel::bounded(8);
    let stop = Arc::new(AtomicBool::new(false));
    let t = Instant::now();
    let handle = director_core::android_media::spawn_decode(path.to_path_buf(), 0.0, fps, (p.width, p.height), tx, spare_rx, stop);
    let mut count = 0usize;
    let mut means = Vec::new();
    let name = path.file_stem().unwrap().to_string_lossy().into_owned();
    loop {
        let f = match rx.recv_timeout(std::time::Duration::from_secs(20)) {
            Ok(f) => f?,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            Err(_) => anyhow::bail!("decoder stalled after {count} frames"),
        };
        if count == 45 {
            let mut ppm = format!("P6 {} {} 255
", p.width, p.height).into_bytes();
            ppm.extend(f.chunks_exact(4).flat_map(|c| [c[0], c[1], c[2]]));
            fs::write(path.with_file_name(format!("{name}_45.ppm")), ppm)?;
        }
        if count % 30 == 0 {
            let px = f.len() / 4;
            let (mut r, mut g, mut b) = (0u64, 0u64, 0u64);
            for c in f.chunks_exact(4).step_by(97) { r += c[0] as u64; g += c[1] as u64; b += c[2] as u64; }
            let k = (px / 97).max(1) as u64;
            means.push(format!("{}:({},{},{})", count, r / k, g / k, b / k));
        }
        count += 1;
        let _ = spare_tx.try_send(f);
    }
    let _ = handle.join();
    say!("decoded {count} frames at {fps} fps in {:?}; mean rgb {}", t.elapsed(), means.join(" "));
    let t = Instant::now();
    match director_core::android_media::decode_audio(path) {
        Ok((s, rate)) => {
            let rms = (s.iter().map(|v| (v * v) as f64).sum::<f64>() / s.len().max(1) as f64).sqrt();
            say!("decoded audio: {} stereo samples at {rate} Hz ({:.2} s), rms {:.3}, in {:?}", s.len() / 2, s.len() as f64 / 2.0 / rate as f64, rms, t.elapsed());
        }
        Err(e) => say!("audio decode: {e:#}"),
    }
    Ok(())
}

fn run(dir: PathBuf) {
    let _ = fs::create_dir_all(&dir);
    *LOG.lock().unwrap() = fs::File::create(dir.join("log.txt")).ok();
    say!("DIRAPK START, files in {}", dir.display());
    for (name, bytes) in [
        ("src.mp4", &include_bytes!("../assets/src.mp4")[..]),
        ("bed.mp3", &include_bytes!("../assets/bed.mp3")[..]),
        ("ogg.webm", &include_bytes!("../assets/ogg.webm")[..]),
        ("tag709.mp4", &include_bytes!("../assets/tag709.mp4")[..]),
        ("full601.mp4", &include_bytes!("../assets/full601.mp4")[..]),
    ] {
        let _ = fs::write(dir.join(name), bytes);
    }
    let steps: Vec<(&str, Box<dyn Fn() -> anyhow::Result<()>>)> = vec![
        ("decode source", Box::new(|| decode_check(&dir.join("src.mp4"), 30.0))),
        ("colour tags", Box::new(|| { decode_check(&dir.join("tag709.mp4"), 30.0)?; decode_check(&dir.join("full601.mp4"), 30.0) })),
        ("encode speed", Box::new(|| encode_speed(&dir))),
        ("html scene", Box::new(|| compile_and_export(&dir).and_then(|p| decode_check(&p, 30.0)))),
        ("reel", Box::new(|| { std::env::set_current_dir(&dir)?; export_reel(&dir) }.and_then(|p| decode_check(&p, 30.0)))),
    ];
    let quick = dir.join("quick").exists();
    for (name, step) in steps {
        if quick && !matches!(name, "decode source" | "colour tags") {
            continue;
        }
        say!("== {name}");
        let t = Instant::now();
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| step())) {
            Ok(Ok(())) => say!("== {name} OK in {:?}", t.elapsed()),
            Ok(Err(e)) => say!("== {name} FAILED: {e:#}"),
            Err(p) => say!("== {name} PANICKED: {:?}", p.downcast_ref::<String>().map(|s| s.as_str()).or(p.downcast_ref::<&str>().copied())),
        }
    }
    say!("DIRAPK DONE");
}

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: android_activity::AndroidApp) {
    let dir = app.external_data_path().unwrap_or_else(|| PathBuf::from("/data/local/tmp/dirapk"));
    let worker = std::thread::spawn(move || run(dir));
    let _ = worker;
    loop {
        app.poll_events(Some(std::time::Duration::from_millis(500)), |_| {});
    }
}
