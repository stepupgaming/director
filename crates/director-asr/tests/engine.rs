//! Engine tests. The model runs need an installed store (`DIRECTOR_ASR_CACHE` or the default
//! cache) and a speech recording in `DIRECTOR_ASR_SPEECH`; run them with `--ignored`.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use director_asr::audio::decode;
use director_asr::{AsrError, Engine, Graphs, Model, Store, SAMPLE_RATE};

#[test]
fn pro_is_unsupported_until_bark_asr_loads_safetensors() {
    let dir = tempfile::tempdir().unwrap();
    let err = Engine::open(&Store::new(dir.path()), Model::BarkPro, Graphs::Exact, 0).err().unwrap();
    assert!(matches!(err, AsrError::Unsupported(_)), "{err}");
}

#[test]
fn a_missing_model_names_the_fetch_command() {
    let dir = tempfile::tempdir().unwrap();
    let err = Engine::open(&Store::new(dir.path()), Model::BarkClean, Graphs::Int8, 0).err().unwrap();
    assert!(err.to_string().contains("asr fetch bark-3.0-clean"), "{err}");
}

/// The speech recording, decoded to 16 kHz mono.
fn speech() -> Vec<f32> {
    let path = PathBuf::from(std::env::var_os("DIRECTOR_ASR_SPEECH").expect("set DIRECTOR_ASR_SPEECH to a speech recording"));
    decode(&path).unwrap()
}

/// `DIRECTOR_ASR_MODEL` (default bark-3.0) and `DIRECTOR_ASR_GRAPHS` (default exact).
fn chosen() -> (Model, Graphs) {
    let var = |name: &str, default: &str| std::env::var(name).unwrap_or_else(|_| default.to_string());
    (var("DIRECTOR_ASR_MODEL", "bark-3.0").parse().unwrap(), var("DIRECTOR_ASR_GRAPHS", "exact").parse().unwrap())
}

/// bark-asr 0.1.0 builds one ONNX Runtime environment per process, so this test loads the
/// model once and runs every check on it. Pick the model and graphs with `DIRECTOR_ASR_MODEL`
/// and `DIRECTOR_ASR_GRAPHS`.
#[test]
#[ignore = "needs the model in the store and DIRECTOR_ASR_SPEECH"]
fn words_are_ordered_survive_short_pieces_and_cancel_stops() {
    let samples = speech();
    let seconds = samples.len() as f64 / f64::from(SAMPLE_RATE);
    let (model, graphs) = chosen();
    let mut engine = Engine::open(&Store::open_default().unwrap(), model, graphs, 0).unwrap();
    assert_eq!((engine.model(), engine.graphs()), (model, graphs));
    let never = AtomicBool::new(false);

    let mut seen = Vec::new();
    let whole = engine.transcribe(&samples, &mut |done, total| seen.push((done, total)), &never).unwrap();
    assert_eq!(whole.model, model);
    assert!(whole.words.len() as f64 > seconds, "{} words in {seconds} s: {}", whole.words.len(), whole.text());
    for pair in whole.words.windows(2) {
        assert!(pair[0].start <= pair[1].start, "{:?} then {:?}", pair[0], pair[1]);
    }
    for w in &whole.words {
        assert!(0.0 <= w.start && w.start <= w.end && w.end <= seconds + 0.5, "{w:?}");
        assert!(w.text.chars().any(char::is_alphanumeric), "{w:?}");
    }
    let (done, total) = *seen.last().unwrap();
    assert_eq!(done, total);
    println!("{model} {graphs}: {}", whole.text());

    engine.set_max_piece(6.0);
    let mut pieces = 0;
    let cut = engine.transcribe(&samples, &mut |_, _| pieces += 1, &never).unwrap();
    assert!(pieces > 1, "one piece");
    let (a, b) = (whole.words.len() as f64, cut.words.len() as f64);
    assert!((a - b).abs() <= (a * 0.1).max(3.0), "{a} words whole, {b} in pieces:\n{}\n{}", whole.text(), cut.text());
    println!("{model} {graphs} in {pieces} pieces: {}", cut.text());

    let err = engine.transcribe(&samples, &mut |_, _| {}, &AtomicBool::new(true)).unwrap_err();
    assert!(matches!(err, AsrError::Cancelled), "{err}");
}
