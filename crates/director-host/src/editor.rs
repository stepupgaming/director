//! Editor protocol: a session on its own owner thread driven by generation
//! numbered commands, with frames in a shared-memory ring and events on a
//! channel. A Tauri editor links it in-process; `director-engine editor`
//! serves it to another process over stdio and a ring file.

use crate::error::{ErrorCode, HostError};
use crate::frames::{FrameStamp, SharedFrames};
use crate::load::Source;
use crate::patch::PatchOp;
use crate::session::{Outline, Session};
use base64::Engine;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, Serializer};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;

/// Frame slots in the ring. Three let the writer fill one while a reader copies
/// another and a third holds the newest.
pub const FRAME_SLOTS: usize = 3;

/// One control-plane request. Each gets the next generation; a seek is
/// cancelled when a newer seek arrives before it starts.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Command {
    /// Paint the frame at `time` seconds into the ring.
    Seek { time: f64 },
    /// Apply patch ops atomically against `base_revision` (default: current),
    /// then repaint the last seek.
    Patch {
        base_revision: Option<u64>,
        ops: Vec<PatchOp>,
    },
    /// Load the input again, then repaint the last seek.
    Reload,
    /// Layout, timeline and audio at `time` seconds.
    Inspect { time: f64 },
    /// The node tree with authored names.
    Outline,
    /// Mix `duration` seconds of audio from `start`.
    Audio { start: f64, duration: f64 },
}

/// What the owner thread reports, in command order. `generation` names the
/// command that caused the event.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// First event: the document and the ring.
    Opened {
        revision: u64,
        width: u32,
        height: u32,
        fps: f64,
        duration: f64,
        frames: Option<PathBuf>,
        slots: usize,
        capacity: usize,
    },
    /// A frame is the newest one in `slot` of the ring.
    Frame {
        generation: u64,
        revision: u64,
        time: f64,
        slot: usize,
        width: u32,
        height: u32,
    },
    /// A newer seek replaced this one before it painted.
    Cancelled { generation: u64 },
    Patched { generation: u64, revision: u64 },
    /// The input loaded again, on request or because a source file changed.
    Reloaded { generation: u64, revision: u64 },
    Inspected { generation: u64, inspection: serde_json::Value },
    Outline { generation: u64, outline: Outline },
    /// Interleaved float stereo; JSON carries it as base64 little-endian `f32`.
    Audio {
        generation: u64,
        revision: u64,
        start: f64,
        sample_rate: u32,
        #[serde(rename = "samples_f32le", serialize_with = "base64_f32")]
        samples: Vec<f32>,
    },
    Error { generation: u64, error: HostError },
}

fn base64_f32<S: Serializer>(samples: &[f32], serializer: S) -> Result<S::Ok, S::Error> {
    let bytes: Vec<u8> = samples.iter().flat_map(|sample| sample.to_le_bytes()).collect();
    serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// Pixel bytes for the largest frame the canvas paints at its output scale,
/// with room for rounding to even sizes.
fn frame_capacity(width: i32, height: i32, scale_x: f32, scale_y: f32) -> usize {
    let side = |pixels: i32, scale: f32| (pixels.max(1) as f32 * scale.max(1.0)).ceil() as usize + 2;
    side(width, scale_x) * side(height, scale_y) * 4
}

/// Runs on the owner thread right after the session opens, such as attaching a
/// GPU context that must stay on that thread.
pub type Setup = Box<dyn FnOnce(&mut Session) -> Result<(), HostError> + Send>;

#[derive(Default)]
pub struct EditorOptions {
    /// Back the frame ring with this file so another process can map it;
    /// without it the ring is anonymous memory.
    pub frames: Option<PathBuf>,
    pub setup: Option<Setup>,
}

/// The client side of an editor session. Dropping it stops the owner thread
/// after the commands already sent.
pub struct Editor {
    commands: Option<mpsc::Sender<(u64, Command)>>,
    frames: Arc<SharedFrames>,
    generation: u64,
    latest_seek: Arc<AtomicU64>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Editor {
    /// Open `source` on a new owner thread. The receiver yields
    /// [`Event::Opened`] first, then one or more events per command.
    pub fn open(source: Source, options: EditorOptions) -> Result<(Self, mpsc::Receiver<Event>), HostError> {
        let (commands, inbox) = mpsc::channel::<(u64, Command)>();
        let (events, outbox) = mpsc::channel();
        let (ready, opened) = mpsc::sync_channel(1);
        let latest_seek = Arc::new(AtomicU64::new(0));
        let seeks = Arc::clone(&latest_seek);
        let thread = thread::Builder::new()
            .name("director-editor".into())
            .spawn(move || {
                let started = (|| {
                    let mut session = Session::open(&source)?;
                    if let Some(setup) = options.setup {
                        setup(&mut session)?;
                    }
                    let document = session.document();
                    let capacity = frame_capacity(document.width, document.height, document.output_scale_x, document.output_scale_y);
                    let frames = Arc::new(SharedFrames::create(options.frames.as_deref(), FRAME_SLOTS, capacity)?);
                    let opened = Event::Opened {
                        revision: session.revision(),
                        width: document.width.max(0) as u32,
                        height: document.height.max(0) as u32,
                        fps: document.fps,
                        duration: session.duration(),
                        frames: options.frames,
                        slots: frames.slots(),
                        capacity,
                    };
                    Ok::<_, HostError>((session, frames, opened))
                })();
                let (session, frames, opened) = match started {
                    Ok(started) => started,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                if ready.send(Ok(Arc::clone(&frames))).is_err() || events.send(opened).is_err() {
                    return;
                }
                Owner {
                    session,
                    frames,
                    events,
                    latest_seek: seeks,
                    shown: None,
                }
                .serve(inbox);
            })
            .map_err(|error| HostError::new(ErrorCode::Load, format!("editor thread: {error}")))?;
        let frames = opened
            .recv()
            .map_err(|_| HostError::new(ErrorCode::Load, "the editor thread stopped while opening"))??;
        let editor = Self {
            commands: Some(commands),
            frames,
            generation: 0,
            latest_seek,
            thread: Some(thread),
        };
        Ok((editor, outbox))
    }

    /// Queue `command` and return its generation.
    pub fn send(&mut self, command: Command) -> u64 {
        let generation = self.next_generation();
        if matches!(command, Command::Seek { .. }) {
            self.latest_seek.store(generation, Ordering::Release);
        }
        if let Some(commands) = &self.commands {
            let _ = commands.send((generation, command));
        }
        generation
    }

    /// Take a generation without sending a command, for a request the client
    /// rejected itself, so generations keep counting requests.
    pub fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    /// The frame ring the owner thread paints into.
    pub fn frames(&self) -> &Arc<SharedFrames> {
        &self.frames
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        self.commands.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The owner thread's state.
struct Owner {
    session: Session,
    frames: Arc<SharedFrames>,
    events: mpsc::Sender<Event>,
    latest_seek: Arc<AtomicU64>,
    /// The time of the last painted seek, repainted after edits.
    shown: Option<f64>,
}

impl Owner {
    fn serve(mut self, inbox: mpsc::Receiver<(u64, Command)>) {
        while let Ok((generation, command)) = inbox.recv() {
            if self.session.source_changed() {
                match self.session.reload() {
                    Ok(revision) => {
                        self.emit(Event::Reloaded { generation, revision });
                        self.repaint(generation);
                    }
                    Err(error) => self.emit(Event::Error { generation, error }),
                }
            }
            let event = self.run(generation, command).unwrap_or_else(|error| Event::Error { generation, error });
            let edited = matches!(event, Event::Patched { .. } | Event::Reloaded { .. });
            self.emit(event);
            if edited {
                self.repaint(generation);
            }
        }
    }

    fn run(&mut self, generation: u64, command: Command) -> Result<Event, HostError> {
        let session = &mut self.session;
        Ok(match command {
            Command::Seek { time } => {
                if generation < self.latest_seek.load(Ordering::Acquire) {
                    return Ok(Event::Cancelled { generation });
                }
                let event = self.paint(generation, time)?;
                self.shown = Some(time);
                event
            }
            Command::Patch { base_revision, ops } => {
                let revision = session.patch(base_revision.unwrap_or(session.revision()), &ops)?;
                Event::Patched { generation, revision }
            }
            Command::Reload => Event::Reloaded {
                generation,
                revision: session.reload()?,
            },
            Command::Inspect { time } => Event::Inspected {
                generation,
                inspection: session.inspect(time)?,
            },
            Command::Outline => Event::Outline {
                generation,
                outline: session.outline(),
            },
            Command::Audio { start, duration } => {
                let window = session.audio_window(start, duration)?;
                Event::Audio {
                    generation,
                    revision: session.revision(),
                    start: window.start,
                    sample_rate: window.sample_rate,
                    samples: window.samples,
                }
            }
        })
    }

    /// Paint the last seek again for the new revision, unless a newer seek
    /// will paint anyway.
    fn repaint(&mut self, generation: u64) {
        let Some(time) = self.shown else {
            return;
        };
        if generation < self.latest_seek.load(Ordering::Acquire) {
            return;
        }
        let event = self.paint(generation, time).unwrap_or_else(|error| Event::Error { generation, error });
        self.emit(event);
    }

    fn paint(&mut self, generation: u64, time: f64) -> Result<Event, HostError> {
        let frame = self.session.render_frame(time)?;
        let revision = self.session.revision();
        let stamp = FrameStamp {
            generation,
            revision,
            time,
        };
        let slot = self.frames.publish(stamp, frame.width, frame.height, &frame.rgba)?;
        Ok(Event::Frame {
            generation,
            revision,
            time,
            slot,
            width: frame.width,
            height: frame.height,
        })
    }

    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }
}
