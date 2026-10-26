//! `outline` and `diff`: scene document structure as JSON, with failures in
//! the host error envelope.

use crate::input::Input;
use clap::Parser;
use director_host::{ErrorCode, HostError, Session};
use std::path::PathBuf;

#[derive(Parser, Debug)]
pub struct OutlineArgs {
    #[command(flatten)]
    input: Input,
}

#[derive(Parser, Debug)]
pub struct DiffArgs {
    /// The input to diff from. The input flags apply to both inputs.
    #[command(flatten)]
    before: Input,
    /// The input to diff to
    after: PathBuf,
}

pub fn run_outline(args: OutlineArgs) -> i32 {
    match Session::open(&args.input.source()) {
        Ok(session) => {
            println!("{}", serde_json::json!(session.outline()));
            0
        }
        Err(error) => fail(&error),
    }
}

pub fn run_diff(args: DiffArgs) -> i32 {
    let before = args.before.source();
    let after = director_host::Source {
        input: args.after,
        ..before.clone()
    };
    let opened = Session::open(&before).and_then(|before| Ok((before, Session::open(&after)?)));
    match opened {
        Ok((before, after)) => {
            println!("{}", serde_json::json!(director_host::diff(&before, &after)));
            0
        }
        Err(error) => fail(&error),
    }
}

/// Print `error` as `{"error":{"code","message"}}` on stdout. Load and
/// argument failures exit 1; render and export failures exit 2.
pub fn fail(error: &HostError) -> i32 {
    println!("{}", error.envelope());
    match error.code {
        ErrorCode::Render | ErrorCode::Export | ErrorCode::Cancelled | ErrorCode::MissingFont => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::DiffArgs;
    use clap::Parser;

    #[test]
    fn diff_takes_two_inputs_and_shared_flags() {
        let args = DiffArgs::try_parse_from(["diff", "a.html", "b.html", "--fps", "24"]).unwrap();
        assert_eq!(args.before.input, std::path::Path::new("a.html"));
        assert_eq!(args.after, std::path::Path::new("b.html"));
        assert_eq!(args.before.fps, 24.0);
    }
}
