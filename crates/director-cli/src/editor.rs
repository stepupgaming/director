//! `editor`: the host editor protocol for another process. Commands arrive as
//! JSON lines on stdin, events leave as JSON lines on stdout, and painted
//! frames land in the shared-memory ring file at `--frames`.

use crate::input::Input;
use crate::outline::fail;
use clap::Parser;
use director_host::{Command, Editor, EditorOptions, ErrorCode, Event, HostError};
use std::io::{BufRead, Write};
use std::path::PathBuf;

#[derive(Parser, Debug)]
pub struct EditorArgs {
    #[command(flatten)]
    input: Input,
    /// Frame ring file to create; the client maps it to read painted frames
    #[arg(long)]
    frames: PathBuf,
    /// Paint on a required Vulkan context
    #[arg(long)]
    gpu: bool,
}

pub fn run(args: EditorArgs) -> i32 {
    let gpu = args.gpu;
    let options = EditorOptions {
        frames: Some(args.frames),
        setup: Some(Box::new(move |session| {
            let document = session.document();
            crate::renderer::frame_within_limit(document.width, document.height)
                .and_then(|()| crate::renderer::use_gpu(session, gpu))
                .map_err(|message| HostError::new(ErrorCode::Render, message))
        })),
    };
    let (mut editor, events) = match Editor::open(args.input.source(), options) {
        Ok(opened) => opened,
        Err(error) => return fail(&error),
    };
    let printer = std::thread::spawn(move || {
        for event in events {
            emit(&event);
        }
    });
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Command>(&line) {
            Ok(command) => {
                editor.send(command);
            }
            Err(error) => emit(&Event::Error {
                generation: editor.next_generation(),
                error: HostError::new(ErrorCode::InvalidArgument, format!("command: {error}")),
            }),
        }
    }
    drop(editor);
    let _ = printer.join();
    0
}

/// One event per line, flushed so the client sees it at once.
fn emit(event: &Event) {
    let mut stdout = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut stdout, event);
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}

#[cfg(test)]
mod tests {
    use super::EditorArgs;
    use clap::Parser;

    #[test]
    fn editor_needs_a_frame_ring_path() {
        assert!(EditorArgs::try_parse_from(["editor", "scene.html"]).is_err());
        let args = EditorArgs::try_parse_from(["editor", "scene.html", "--frames", "ring", "--gpu"]).unwrap();
        assert_eq!(args.frames, std::path::Path::new("ring"));
        assert!(args.gpu);
    }
}
