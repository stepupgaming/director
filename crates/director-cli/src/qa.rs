//! `qa`: the host QA report for an input, with a 0-100 score.

use crate::input::Input;
use crate::outline::fail;
use clap::Parser;
use director_host::{qa_video, HostError, QaOptions, QaWord, Session};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Parser, Debug)]
pub struct QaArgs {
    #[command(flatten)]
    input: Input,
    /// Seconds between layout samples
    #[arg(long, default_value_t = 0.25)]
    every: f64,
    /// Warn when text, caption, or smart_caption holds inside this platform inset
    #[arg(long)]
    safe_zone: Option<String>,
    /// Integrated loudness target of the audio mix in LUFS
    #[arg(long, default_value_t = -14.0, allow_hyphen_values = true)]
    loudness_target: f64,
    /// Transcript JSON: an array of `{text, start, end}` words, or an object
    /// with a `words` array, in timeline seconds
    #[arg(long)]
    words: Option<PathBuf>,
    /// A finished encode of the input, checked for encoder blotching in dark
    /// flat areas at the canvas size
    #[arg(long)]
    video: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Transcript {
    Words(Vec<QaWord>),
    Object { words: Vec<QaWord> },
}

fn read_words(path: &PathBuf) -> Result<Vec<QaWord>, HostError> {
    let invalid = |error: String| HostError::new(director_host::ErrorCode::InvalidArgument, format!("{}: {error}", path.display()));
    let text = std::fs::read_to_string(path).map_err(|error| invalid(error.to_string()))?;
    match serde_json::from_str(&text).map_err(|error| invalid(error.to_string()))? {
        Transcript::Words(words) | Transcript::Object { words } => Ok(words),
    }
}

/// Finished videos get the file report of `qa_video`.
fn is_video(path: &std::path::Path) -> bool {
    let extension = path.extension().and_then(|extension| extension.to_str()).unwrap_or_default().to_ascii_lowercase();
    matches!(extension.as_str(), "mp4" | "m4v" | "mov" | "webm" | "mkv")
}

/// Print the report and exit 0 when it passes, 2 when an issue is an error.
pub fn run(args: QaArgs) -> i32 {
    let words = match args.words.as_ref().map(read_words).transpose() {
        Ok(words) => words.unwrap_or_default(),
        Err(error) => return fail(&error),
    };
    let options = QaOptions {
        every: args.every,
        safe_zone: args.safe_zone,
        loudness_target: args.loudness_target,
        words,
        video: args.video,
    };
    let report = if is_video(&args.input.input) {
        qa_video(&args.input.input, &options, &mut |_, _| {})
    } else {
        Session::open(&args.input.source()).and_then(|mut session| session.qa(&options))
    };
    match report {
        Ok(report) => {
            println!("{}", serde_json::json!(report));
            if report.pass {
                0
            } else {
                2
            }
        }
        Err(error) => fail(&error),
    }
}

#[cfg(test)]
mod tests {
    use super::QaArgs;
    use clap::Parser;

    #[test]
    fn qa_takes_a_negative_loudness_target() {
        let args = QaArgs::try_parse_from(["qa", "a.html", "--loudness-target", "-16", "--safe-zone", "tiktok"]).unwrap();
        assert_eq!(args.loudness_target, -16.0);
        assert_eq!(args.safe_zone.as_deref(), Some("tiktok"));
    }

    #[test]
    fn words_load_from_an_array_or_a_words_object() {
        let dir = tempfile::tempdir().unwrap();
        let (array, object) = (dir.path().join("a.json"), dir.path().join("o.json"));
        std::fs::write(&array, r#"[{"text":"hi","start":0.5,"end":0.8}]"#).unwrap();
        std::fs::write(&object, r#"{"words":[{"word":"hi","start":0.5,"end":0.8}],"language":"en"}"#).unwrap();
        assert_eq!(super::read_words(&array).unwrap(), super::read_words(&object).unwrap());
        assert!(super::read_words(&dir.path().join("missing.json")).is_err());
    }
}
