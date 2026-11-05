use director_core::video_wrapper::EncoderMode;
use director_host::{ErrorCode, ExportSettings, QaIssue, QaOptions, QaReport, QaWord, Session, Source};
use serde_json::json;

/// A white 320x180 composition, three seconds long, holding `body`.
fn report(body: &str, words: Vec<QaWord>) -> QaReport {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("scene.html");
    let html = format!(
        "<style>body{{margin:0}}main{{background:#fff}}p{{position:absolute;margin:0;font-size:28px;left:10px}}</style>\
         <main data-composition-id='main' data-width='320' data-height='180' data-duration='3'>{body}</main>"
    );
    std::fs::write(&input, html).unwrap();
    let mut session = Session::open(&Source::new(input)).unwrap();
    session.qa(&QaOptions { words, ..QaOptions::default() }).unwrap()
}

fn issues<'a>(report: &'a QaReport, name: &str) -> Vec<&'a QaIssue> {
    report.issues.iter().filter(|issue| issue.name.as_deref() == Some(name)).collect()
}

fn kinds(report: &QaReport, name: &str) -> Vec<&'static str> {
    issues(report, name).iter().map(|issue| issue.kind).filter(|kind| *kind != "static").collect()
}

fn word(text: &str, start: f64) -> QaWord {
    QaWord {
        text: text.into(),
        start,
        end: start + 0.3,
    }
}

#[test]
fn text_runs_check_reading_time_and_contrast_per_node() {
    let report = report(
        "<p id='quick' class='clip' data-start='1' data-duration='0.4' style='top:10px;color:#000'>Read this quickly now</p>\
         <p id='held' class='clip' data-start='1' data-duration='1.5' style='top:60px;color:#000'>Read this slowly now</p>\
         <p id='faint' style='top:120px;color:#e8e8e8'>Faint words</p>",
        Vec::new(),
    );
    let quick = issues(&report, "quick");
    assert_eq!(kinds(&report, "quick"), ["reading_time"], "{}", json!(report));
    // The run starts and ends to frame precision.
    assert!((quick[0].start - 1.0).abs() <= 1.0 / 30.0 + 1e-6 && (quick[0].end - 1.4).abs() <= 1.0 / 30.0 + 1e-6, "{}", json!(quick));
    assert!(quick[0].fix.is_some());
    assert!(kinds(&report, "held").is_empty(), "{}", json!(report));
    assert_eq!(kinds(&report, "faint"), ["contrast"], "{}", json!(report));
    assert_eq!(report.warnings, 2);
}

#[test]
fn text_the_voice_reads_waives_reading_time_and_late_cues_warn() {
    let line = "<p id='line' class='clip' data-start='1' data-duration='0.6' style='top:60px;color:#000'>Hello world</p>";
    let late = report(line, vec![word("Hello", 0.5), word("world", 0.8)]);
    assert_eq!(kinds(&late, "line"), ["cue_late"], "{}", json!(late));
    assert!(issues(&late, "line")[0].fix.as_deref().unwrap().contains("0.50s"));

    let on_time = report(line, vec![word("Hello", 0.95), word("world", 1.25)]);
    assert!(kinds(&on_time, "line").is_empty(), "{}", json!(on_time));
    assert!(on_time.pass && on_time.warnings == 0);
}

/// QA of a dark radial gradient against its own software export, at draft
/// quality (x264 ultrafast, CRF 30) or the final quality.
fn encoded(draft: bool) -> QaReport {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("dark.html");
    let html = "<style>body{margin:0}main{background:radial-gradient(circle at 30% 40%,#2c313c,#06070a 70%)}</style>         <main data-composition-id='main' data-width='640' data-height='360' data-duration='2'></main>";
    std::fs::write(&input, html).unwrap();
    let mut session = Session::open(&Source::new(input)).unwrap();
    let video = dir.path().join("dark.mp4");
    let mut settings = ExportSettings::default();
    settings.options.draft = draft;
    settings.options.mode = EncoderMode::Software;
    session.export(&video, &settings, None).unwrap();
    session.qa(&QaOptions { video: Some(video), ..QaOptions::default() }).unwrap()
}

#[test]
fn draft_encodes_of_dark_gradients_blotch_and_final_encodes_pass() {
    let draft = encoded(true);
    let blotching: Vec<_> = draft.issues.iter().filter(|issue| issue.kind == "blotching").collect();
    assert_eq!(blotching.len(), 1, "{}", json!(draft));
    assert_eq!((blotching[0].start, blotching[0].count), (0.0, 5), "{}", json!(blotching));
    assert!(blotching[0].fix.is_some());

    let final_quality = encoded(false);
    assert!(final_quality.issues.iter().all(|issue| issue.kind != "blotching"), "{}", json!(final_quality));
    assert_eq!(final_quality.warnings, 0);
}

#[test]
fn a_missing_video_is_an_invalid_argument() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("scene.html");
    std::fs::write(&input, "<main data-composition-id='main' data-width='64' data-height='64' data-duration='1'></main>").unwrap();
    let mut session = Session::open(&Source::new(input)).unwrap();
    let options = QaOptions {
        video: Some(dir.path().join("missing.mp4")),
        ..QaOptions::default()
    };
    assert_eq!(session.qa(&options).unwrap_err().code, ErrorCode::InvalidArgument);
}
