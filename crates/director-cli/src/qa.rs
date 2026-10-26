//! `qa`: the host QA report for an input, with a 0-100 score.

use crate::input::Input;
use crate::outline::fail;
use clap::Parser;
use director_host::{QaOptions, Session};

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
}

/// Print the report and exit 0 when it passes, 2 when an issue is an error.
pub fn run(args: QaArgs) -> i32 {
    let options = QaOptions {
        every: args.every,
        safe_zone: args.safe_zone,
        loudness_target: args.loudness_target,
    };
    match Session::open(&args.input.source()).and_then(|mut session| session.qa(&options)) {
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
}
