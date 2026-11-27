//! `captions pages` prints the pages an input's captions show, broken by
//! measured width after layout, with each page's time and the caption's name.

use std::process::Command;

fn cli(args: &[&str]) -> (i32, serde_json::Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_director-engine")).args(args).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|error| panic!("{error}: {stdout} {}", String::from_utf8_lossy(&output.stderr)));
    (output.status.code().unwrap(), json)
}

#[test]
fn pages_follow_the_measured_one_line_breaks() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("words.json"),
        r#"{"words":[{"text":"Root","start":0.0,"end":0.3},{"text":"beer,","start":0.3,"end":0.6},
            {"text":"investigation","start":0.7,"end":1.2},{"text":"semiconductors.","start":1.2,"end":1.9}]}"#,
    )
    .unwrap();
    let html = dir.path().join("captions.html");
    std::fs::write(
        &html,
        "<main data-composition-id='main' data-width='1080' data-height='1920' data-duration='4'>\
         <captions id='cc' src='words.json' size='0.06' uppercase max-lines='1' max-words-per-line='2'></captions></main>",
    )
    .unwrap();
    let (code, out) = cli(&["captions", "pages", html.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    let caption = &out["captions"][0];
    assert_eq!(caption["name"], "cc", "{out}");
    let pages = caption["pages"].as_array().unwrap();
    let texts: Vec<&str> = pages.iter().map(|p| p["text"].as_str().unwrap()).collect();
    assert_eq!(texts, ["Root beer,", "investigation", "semiconductors."], "two long words never share a line");
    assert_eq!((pages[1]["start"].as_f64(), pages[1]["end"].as_f64()), (Some(0.7), Some(1.2)));
    assert!((pages[2]["end"].as_f64().unwrap() - 2.4).abs() < 1e-9, "the last page holds 0.5 s: {out}");
}
