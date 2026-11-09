//! `asr`: the Bark 3.0 model store and word-timed transcripts as JSON, SRT, VTT or text.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use clap::{Parser, Subcommand, ValueEnum};
use director_asr::audio::decode;
use director_asr::store::sha256_file;
use director_asr::subtitle::{cues, srt, vtt};
use director_asr::{AsrError, CueOptions, Engine, Graphs, Model, Status, Store, Transcript, SAMPLE_RATE};
use director_core::node::caption::TimedWord;
use director_core::transcribe::TranscribeRequest;

#[derive(Parser, Debug)]
pub struct AsrArgs {
    /// Model cache directory; defaults to DIRECTOR_ASR_CACHE, else director/asr under the platform cache directory
    #[arg(long, global = true)]
    pub cache: Option<PathBuf>,
    #[command(subcommand)]
    pub command: AsrCommand,
}

#[derive(Subcommand, Debug)]
pub enum AsrCommand {
    /// List the models and graph sets with what the cache holds, as JSON
    Models,
    /// Install a model and a graph set: download them, or import copies already on disk
    Fetch(FetchArgs),
    /// Transcribe the first audio stream of a media file
    Transcribe(TranscribeArgs),
}

#[derive(Parser, Debug)]
pub struct FetchArgs {
    /// bark-3.0, bark-3.0-plus, bark-3.0-clean or bark-3.0-pro
    pub model: Model,
    /// Graph set to install beside the model: exact or int8
    #[arg(long, default_value_t)]
    pub graphs: Graphs,
    /// Import the model file from this path in place of downloading it
    #[arg(long)]
    pub from: Option<PathBuf>,
    /// Import the graph set from this folder (a bark-rs graphs or graphs-int8 folder) in place of downloading it
    #[arg(long)]
    pub graphs_from: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Format {
    Json,
    Srt,
    Vtt,
    Txt,
}

#[derive(Parser, Debug)]
pub struct TranscribeArgs {
    /// Audio or video file ffmpeg reads
    pub media: PathBuf,
    #[arg(long, default_value = "bark-3.0")]
    pub model: Model,
    /// exact matches the official transcript; int8 is about 3x faster
    #[arg(long, default_value_t)]
    pub graphs: Graphs,
    /// ONNX Runtime threads; 0 lets it choose
    #[arg(long, default_value_t = 0)]
    pub threads: usize,
    /// Longest piece read in one pass, in seconds
    #[arg(long, default_value_t = 30.0)]
    pub max_piece: f64,
    #[arg(long, value_enum, default_value_t = Format::Json)]
    pub format: Format,
    /// Output file; without it the transcript prints on stdout
    #[arg(short = 'o', long)]
    pub output: Option<PathBuf>,
}

/// Exit 0 on success, 1 for bad input, a missing model or a failed fetch, 2 when the engine or
/// the output write fails.
pub fn run(args: AsrArgs) -> i32 {
    let store = match args.cache {
        Some(dir) => Store::new(dir),
        None => match Store::open_default() {
            Ok(store) => store,
            Err(e) => return fail(&e),
        },
    };
    match args.command {
        AsrCommand::Models => models(&store),
        AsrCommand::Fetch(f) => fetch(&store, f),
        AsrCommand::Transcribe(t) => transcribe(&store, t),
    }
}

fn fail(e: &AsrError) -> i32 {
    eprintln!("asr: {e}");
    match e {
        AsrError::Engine(_) | AsrError::Cancelled => 2,
        _ => 1,
    }
}

fn status_name(status: Status) -> &'static str {
    match status {
        Status::Missing => "missing",
        Status::WrongSize(_) => "partial",
        Status::Present => "present",
    }
}

fn models(store: &Store) -> i32 {
    let models: Vec<_> = Model::ALL
        .into_iter()
        .map(|m| {
            let asset = m.asset();
            serde_json::json!({
                "id": m.id(),
                "description": m.description(),
                "repo": m.repo(),
                "revision": m.revision(),
                "file": m.file().name,
                "bytes": asset.size,
                "license": m.license(),
                "supported": m != Model::BarkPro,
                "status": status_name(store.status(&asset)),
                "path": store.path(&asset).display().to_string(),
            })
        })
        .collect();
    let graphs: Vec<_> = Graphs::ALL
        .into_iter()
        .map(|g| {
            let assets = g.assets();
            let present = assets.iter().all(|a| store.status(a) == Status::Present);
            serde_json::json!({
                "id": g.id(),
                "bytes": assets.iter().map(|a| a.size).sum::<u64>(),
                "status": if present { "present" } else { "missing" },
                "path": store.root().join(g.dir()).display().to_string(),
            })
        })
        .collect();
    let doc = serde_json::json!({ "cache": store.root().display().to_string(), "models": models, "graphs": graphs });
    println!("{}", serde_json::to_string_pretty(&doc).expect("models serialize"));
    0
}

fn fetch(store: &Store, args: FetchArgs) -> i32 {
    let mut installed = Vec::new();
    let model = args.model.asset();
    let result = match &args.from {
        Some(source) => store.import(&model, source),
        None => {
            eprintln!("asr: downloading {} ({} MB)", model.url, model.size / 1_000_000);
            store.fetch(&model)
        }
    };
    match result {
        Ok(path) => installed.push(path),
        Err(e) => return fail(&e),
    }
    for asset in args.graphs.assets() {
        let result = match &args.graphs_from {
            Some(dir) => store.import(&asset, &dir.join(asset.path.file_name().expect("graph files have names"))),
            None => store.fetch(&asset),
        };
        match result {
            Ok(path) => installed.push(path),
            Err(e) => return fail(&e),
        }
    }
    let doc = serde_json::json!({
        "model": args.model.id(),
        "graphs": args.graphs.id(),
        "license": args.model.license(),
        "files": installed.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
    });
    println!("{doc}");
    0
}

fn transcribe(store: &Store, args: TranscribeArgs) -> i32 {
    if !(args.max_piece.is_finite() && args.max_piece >= 1.0) {
        eprintln!("asr: --max-piece must be at least 1 second");
        return 1;
    }
    let started = Instant::now();
    let samples = match decode(&args.media) {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };
    let decoded = started.elapsed().as_secs_f64();
    let mut engine = match Engine::open(store, args.model, args.graphs, args.threads) {
        Ok(e) => e,
        Err(e) => return fail(&e),
    };
    engine.set_max_piece(args.max_piece);
    let loaded = started.elapsed().as_secs_f64();
    let mut progress = |done: f64, total: f64| eprintln!("asr: {done:.1} / {total:.1} s");
    let transcript = match engine.transcribe(&samples, &mut progress, &AtomicBool::new(false)) {
        Ok(t) => t,
        Err(e) => return fail(&e),
    };
    let finished = started.elapsed().as_secs_f64();
    let text = match args.format {
        Format::Json => serde_json::to_string_pretty(&transcript).expect("transcripts serialize") + "\n",
        Format::Srt => srt(&cues(&transcript.words, &CueOptions::default())),
        Format::Vtt => vtt(&cues(&transcript.words, &CueOptions::default())),
        Format::Txt => transcript.text() + "\n",
    };
    let Some(path) = args.output else {
        print!("{text}");
        return 0;
    };
    if let Err(e) = write(&path, &text) {
        eprintln!("asr: writing {}: {e}", path.display());
        return 2;
    }
    let audio = samples.len() as f64 / f64::from(SAMPLE_RATE);
    let summary = serde_json::json!({
        "output": path.display().to_string(),
        "model": args.model.id(),
        "graphs": args.graphs.id(),
        "words": transcript.words.len(),
        "audio_seconds": audio,
        "seconds": { "decode": decoded, "load": loaded - decoded, "transcribe": finished - loaded },
        "realtime": audio / (finished - loaded).max(1e-9),
    });
    println!("{summary}");
    0
}

/// The engine `add_caption(#{ media })` transcribes with. `bark-asr` builds one
/// ONNX Runtime environment per process, so the first model loaded stays.
static CAPTION_ENGINE: Mutex<Option<Engine>> = Mutex::new(None);

/// Installs the Bark transcriber that `add_caption(#{ media })` runs in every
/// command of this process.
pub fn install_transcriber() {
    director_core::transcribe::set_transcriber(Arc::new(transcribe_for_captions));
}

/// The requested model, else the first installed of 3.0, Plus and Clean; the
/// requested graph set, else the default when installed, else any installed.
fn caption_model(store: &Store, model: Option<&str>, graphs: Option<&str>) -> Result<(Model, Graphs), String> {
    let installed = |m: Model| store.status(&m.asset()) == Status::Present;
    let graphs_installed = |g: Graphs| g.assets().iter().all(|a| store.status(a) == Status::Present);
    let model = match model {
        Some(id) => {
            let m = Model::from_str(id).map_err(|e| e.to_string())?;
            if !installed(m) {
                return Err(format!("{id} is not installed; run `director-engine asr fetch {id}`"));
            }
            m
        }
        None => [Model::Bark, Model::BarkPlus, Model::BarkClean]
            .into_iter()
            .find(|m| installed(*m))
            .ok_or("no Bark model is installed; run `director-engine asr fetch bark-3.0`")?,
    };
    let graphs = match graphs {
        Some(id) => {
            let g = Graphs::from_str(id).map_err(|e| e.to_string())?;
            if !graphs_installed(g) {
                return Err(format!("graph set {id} is not installed; run `director-engine asr fetch {} --graphs {id}`", model.id()));
            }
            g
        }
        None => std::iter::once(Graphs::default())
            .chain(Graphs::ALL)
            .find(|g| graphs_installed(*g))
            .ok_or_else(|| format!("no graph set is installed; run `director-engine asr fetch {}`", model.id()))?,
    };
    Ok((model, graphs))
}

/// Words of `request.media`, read from `<cache>/transcripts/<media sha256>-<model>-<graphs>.json`
/// when an earlier run wrote it, else transcribed and written there.
fn transcribe_for_captions(request: &TranscribeRequest<'_>) -> Result<Vec<TimedWord>, String> {
    let store = Store::open_default().map_err(|e| e.to_string())?;
    let (model, graphs) = caption_model(&store, request.model, request.graphs)?;
    let hash = sha256_file(request.media).map_err(|e| e.to_string())?;
    let cached = store.root().join("transcripts").join(format!("{hash}-{}-{}.json", model.id(), graphs.id()));
    let timed = |transcript: Transcript| -> Vec<TimedWord> {
        transcript.words.into_iter().map(|w| TimedWord { text: w.text, start: w.start, end: w.end }).collect()
    };
    if let Some(transcript) = std::fs::read_to_string(&cached).ok().and_then(|text| serde_json::from_str::<Transcript>(&text).ok()) {
        tracing::info!(media = %request.media.display(), model = model.id(), graphs = graphs.id(), "caption words from the transcript cache");
        return Ok(timed(transcript));
    }
    let samples = decode(request.media).map_err(|e| e.to_string())?;
    let mut slot = CAPTION_ENGINE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(engine) = slot.as_ref().filter(|e| e.model() != model || e.graphs() != graphs) {
        return Err(format!(
            "this process already loaded {} with {} graphs and loads one Bark model; transcribe {} with {model} in another run",
            engine.model(),
            engine.graphs(),
            request.media.display()
        ));
    }
    if slot.is_none() {
        *slot = Some(Engine::open(&store, model, graphs, 0).map_err(|e| e.to_string())?);
    }
    let engine = slot.as_mut().expect("the engine was just loaded");
    let started = Instant::now();
    let mut progress = |done: f64, total: f64| tracing::info!("asr: {done:.1} / {total:.1} s");
    let transcript = engine.transcribe(&samples, &mut progress, &AtomicBool::new(false)).map_err(|e| e.to_string())?;
    tracing::info!(
        media = %request.media.display(),
        model = model.id(),
        graphs = graphs.id(),
        words = transcript.words.len(),
        seconds = started.elapsed().as_secs_f64(),
        "transcribed caption words"
    );
    let json = serde_json::to_string_pretty(&transcript).expect("transcripts serialize");
    if let Err(e) = write(&cached, &json) {
        tracing::warn!("could not cache the transcript at {}: {e}", cached.display());
    }
    Ok(timed(transcript))
}

fn write(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, text)
}
