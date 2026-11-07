use director_asr::subtitle::{cues, srt, vtt};
use director_asr::{Cue, CueOptions, Model, Transcript};

use crate::word;

#[test]
fn cues_break_at_sentences_pauses_length_and_duration() {
    let words = vec![
        word("Hello", 0.0, 0.4),
        word("there.", 0.45, 0.9),
        word("This", 1.0, 1.2),
        word("runs", 1.25, 1.5),
        word("on", 2.5, 2.6),
        word("abcdefghij", 2.65, 3.0),
        word("abcdefghij", 3.05, 3.4),
        word("abcdefghij", 3.45, 3.8),
        word("abcdefghij", 3.85, 4.2),
        word("tail", 4.25, 4.5),
    ];
    let got = cues(&words, &CueOptions::default());
    let texts: Vec<_> = got.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(texts, ["Hello there.", "This runs", "on abcdefghij abcdefghij abcdefghij", "abcdefghij tail"]);
    assert_eq!((got[1].start, got[1].end), (1.0, 1.5));

    let slow: Vec<_> = (0..6).map(|i| word("go", i as f64, i as f64 + 0.9)).collect();
    let got = cues(&slow, &CueOptions::default());
    assert_eq!(got.iter().map(|c| c.end - c.start).fold(0.0, f64::max), 4.9);
    assert_eq!(got.len(), 2);
}

#[test]
fn srt_and_vtt_text() {
    let cues = vec![
        Cue { start: 0.0, end: 1.2345, text: "Fish & <chips>".to_string() },
        Cue { start: 3725.5, end: 3727.0, text: "An hour in.".to_string() },
    ];
    assert_eq!(srt(&cues), "1\n00:00:00,000 --> 00:00:01,235\nFish & <chips>\n\n2\n01:02:05,500 --> 01:02:07,000\nAn hour in.\n\n");
    assert_eq!(
        vtt(&cues),
        "WEBVTT\n\n00:00:00.000 --> 00:00:01.235\nFish &amp; &lt;chips&gt;\n\n01:02:05.500 --> 01:02:07.000\nAn hour in.\n\n"
    );
}

#[test]
fn transcript_json_round_trips() {
    let t = Transcript { model: Model::BarkPlus, words: vec![word("Hi", 0.25, 0.5), word("all.", 0.55, 0.9)] };
    let json = serde_json::to_string(&t).unwrap();
    assert!(json.contains("\"model\":\"bark-3.0-plus\""), "{json}");
    assert_eq!(serde_json::from_str::<Transcript>(&json).unwrap(), t);
    assert_eq!(t.text(), "Hi all.");
    let legacy = r#"{"model":"bark-3.0","words":[{"text":"a","start":0.0,"end":0.1,"confidence":0.9}]}"#;
    assert!(!serde_json::from_str::<Transcript>(legacy).unwrap().words[0].disfluent);
}
