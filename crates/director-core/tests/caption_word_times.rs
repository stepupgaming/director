//! Measured word times reach the caption node from every input (transcript
//! JSON, `words`, word-timed VTT, per-cue `content` words and `media` through
//! the transcriber hook) and survive phrase break, scene export and import,
//! and project save and load. SRT stays an even-split estimate.

use director_core::node::caption::{active_word_at, CaptionLine, CaptionNode, TimedWord};
use director_core::scene_export::export_scene;
use director_core::scene_import::{import_scene, ImportRegistry};
use director_core::scripting::{register_rhai_api, MovieHandle};
use director_core::transcribe::{set_transcriber, TranscribeRequest};
use director_core::video_wrapper::RenderMode;
use director_core::{DefaultAssetLoader, Director};
use director_scene::SceneDocument;
use rhai::Engine;
use std::path::Path;
use std::sync::Arc;

/// Uneven word spacing, as speech recognition reports it: (text, start, end).
const SPOKEN: [(&str, f64, f64); 10] = [
    ("Root", 0.32, 0.55),
    ("beer", 0.56, 0.88),
    ("is", 1.20, 1.31),
    ("the", 1.33, 1.41),
    ("best", 1.44, 1.97),
    ("drink.", 2.04, 2.40),
    ("Ever", 3.40, 3.71),
    ("since", 3.75, 4.02),
    ("I", 4.10, 4.18),
    ("was", 4.22, 4.60),
];

fn spoken() -> Vec<TimedWord> {
    SPOKEN.iter().map(|&(text, start, end)| TimedWord { text: text.to_string(), start, end }).collect()
}

fn rhai_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Runs a script that ends with `movie` after adding one caption with
/// `props` (Rhai map body) and phrase break at two words per line.
fn caption_movie(props: &str) -> Arc<std::sync::Mutex<Director>> {
    let mut engine = Engine::new();
    register_rhai_api(&mut engine, Arc::new(DefaultAssetLoader));
    let script = format!(
        r##"
        let movie = new_director(1080, 1920, 30);
        let scene = movie.add_scene(6.0);
        scene.add_caption(#{{ {props}, auto_phrase_break: true, max_words_per_line: 2, font_size: 64.0 }});
        movie
        "##
    );
    engine.eval::<MovieHandle>(&script).unwrap_or_else(|e| panic!("script failed: {e}")).director
}

fn caption_lines(d: &Director) -> Vec<CaptionLine> {
    d.scene
        .nodes
        .iter()
        .flatten()
        .find_map(|sn| sn.element.as_any().downcast_ref::<CaptionNode>().map(|c| c.lines.clone()))
        .expect("a caption node")
}

fn word_times(lines: &[CaptionLine]) -> Vec<(String, f64, f64)> {
    lines.iter().flat_map(|l| l.words.iter().map(|w| (w.text.clone(), w.start_sec, w.end_sec))).collect()
}

/// Every word keeps its measured times, every cue is measured, phrase break
/// split the cues into display lines, and each word's highlight starts on
/// the first 30 fps frame at or after its start.
fn assert_measured(lines: &[CaptionLine]) {
    let expected: Vec<(String, f64, f64)> = SPOKEN.iter().map(|&(t, s, e)| (t.to_string(), s, e)).collect();
    assert_eq!(word_times(lines), expected);
    assert!(lines.iter().all(|l| l.measured));
    assert!(lines.iter().any(|l| l.words.iter().any(|w| w.display_line_idx > 0)), "phrase break ran");
    let frame = 1.0 / 30.0;
    for (ci, line) in lines.iter().enumerate() {
        for (wi, word) in line.words.iter().enumerate() {
            let lit = (0..240)
                .map(|k| k as f64 / 30.0)
                .find(|&t| active_word_at(lines, t) == Some((ci, wi)))
                .unwrap_or_else(|| panic!("{:?} is never highlighted", word.text));
            assert!(
                lit >= word.start_sec - 1e-9 && lit - word.start_sec < frame,
                "{:?} starts at {} and lights at {lit}",
                word.text,
                word.start_sec
            );
        }
    }
}

#[test]
fn transcript_json_file_keeps_every_word_time() {
    let dir = tempfile::tempdir().unwrap();
    let words = dir.path().join("words.json");
    let json = serde_json::json!({ "model": "bark-3.0", "words": spoken() });
    std::fs::write(&words, json.to_string()).unwrap();
    let movie = caption_movie(&format!("file: \"{}\"", rhai_path(&words)));
    let lines = caption_lines(&movie.lock().unwrap());
    assert_eq!(lines.len(), 2, "the sentence end and the pause split two cues");
    assert_measured(&lines);

    // A JSON transcript without the .json extension is sniffed.
    let bare = dir.path().join("words.txt");
    std::fs::write(&bare, serde_json::to_string(&spoken()).unwrap()).unwrap();
    let movie = caption_movie(&format!("file: \"{}\"", rhai_path(&bare)));
    assert_measured(&caption_lines(&movie.lock().unwrap()));
}

#[test]
fn words_prop_keeps_every_word_time() {
    let items: Vec<String> =
        SPOKEN.iter().map(|(t, s, e)| format!("#{{ text: \"{t}\", start: {s}, end: {e} }}")).collect();
    let movie = caption_movie(&format!("words: [{}]", items.join(", ")));
    assert_measured(&caption_lines(&movie.lock().unwrap()));
}

#[test]
fn word_timed_vtt_file_keeps_every_word_time() {
    let dir = tempfile::tempdir().unwrap();
    let vtt = dir.path().join("words.vtt");
    // The cue layout `director-engine asr transcribe --format vtt` writes.
    std::fs::write(
        &vtt,
        "WEBVTT\n\n00:00:00.320 --> 00:00:02.400\nRoot <00:00:00.560>beer <00:00:01.200>is <00:00:01.330>the <00:00:01.440>best <00:00:02.040>drink.\n\n\
         00:00:03.400 --> 00:00:04.600\nEver <00:00:03.750>since <00:00:04.100>I <00:00:04.220>was\n\n",
    )
    .unwrap();
    let movie = caption_movie(&format!("file: \"{}\"", rhai_path(&vtt)));
    let lines = caption_lines(&movie.lock().unwrap());
    // VTT carries word starts; each word ends where the next starts.
    let starts: Vec<f64> = lines.iter().flat_map(|l| l.words.iter().map(|w| w.start_sec)).collect();
    assert_eq!(starts, SPOKEN.iter().map(|w| w.1).collect::<Vec<_>>());
    assert!(lines.iter().all(|l| l.measured));
}

#[test]
fn content_words_keep_their_times() {
    let cue = |range: std::ops::Range<usize>| {
        let words: Vec<String> = SPOKEN[range.clone()]
            .iter()
            .map(|(t, s, e)| format!("#{{ text: \"{t}\", start: {s}, end: {e} }}"))
            .collect();
        let text: Vec<&str> = SPOKEN[range.clone()].iter().map(|w| w.0).collect();
        format!(
            "#{{ text: \"{}\", start: {}, end: {}, words: [{}] }}",
            text.join(" "),
            SPOKEN[range.start].1,
            SPOKEN[range.end - 1].2,
            words.join(", ")
        )
    };
    let movie = caption_movie(&format!("content: [{}, {}]", cue(0..6), cue(6..10)));
    assert_measured(&caption_lines(&movie.lock().unwrap()));
}

#[test]
fn media_runs_the_installed_transcriber() {
    let dir = tempfile::tempdir().unwrap();
    let clip = dir.path().join("clip.mp4");
    std::fs::write(&clip, b"stand-in").unwrap();
    let expected = clip.clone();
    set_transcriber(Arc::new(move |request: &TranscribeRequest<'_>| {
        assert_eq!(request.media, expected.as_path());
        assert_eq!(request.model, Some("bark-3.0-clean"));
        assert_eq!(request.graphs, Some("int8"));
        Ok(spoken())
    }));
    let movie = caption_movie(&format!(
        "media: \"{}\", model: \"bark-3.0-clean\", graphs: \"int8\"",
        rhai_path(&clip)
    ));
    assert_measured(&caption_lines(&movie.lock().unwrap()));
}

#[test]
fn word_times_survive_scene_export_and_project_save() {
    let items: Vec<String> =
        SPOKEN.iter().map(|(t, s, e)| format!("#{{ text: \"{t}\", start: {s}, end: {e} }}")).collect();
    let movie = caption_movie(&format!("words: [{}]", items.join(", ")));
    let d = movie.lock().unwrap();

    let json = export_scene(&d).unwrap().to_json().unwrap();
    let document = SceneDocument::from_json(&json).unwrap();
    let imported =
        import_scene(&document, Arc::new(DefaultAssetLoader), &ImportRegistry::core(), RenderMode::Export).unwrap();
    assert_measured(&caption_lines(&imported));

    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("captions.json");
    d.save_project_json(&project).unwrap();
    let loaded = Director::load_project_json(&project).unwrap();
    assert_measured(&caption_lines(&loaded));
}

#[test]
fn srt_file_splits_cues_evenly_as_an_estimate() {
    let dir = tempfile::tempdir().unwrap();
    let srt = dir.path().join("cues.srt");
    std::fs::write(&srt, "1\n00:00:00,320 --> 00:00:02,400\nRoot beer is the best drink.\n\n").unwrap();
    let movie = caption_movie(&format!("file: \"{}\"", rhai_path(&srt)));
    let lines = caption_lines(&movie.lock().unwrap());
    assert!(!lines[0].measured);
    assert_eq!(lines[0].words.len(), 6);
}
