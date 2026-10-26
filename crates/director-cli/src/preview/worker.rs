use crate::input::Input;
use director_host::Session;
use std::{sync::Arc, thread};
use tokio::sync::{mpsc, oneshot};

type Job = Box<dyn FnOnce(&mut Session) + Send>;

#[derive(Clone)]
pub(super) struct Worker {
    inner: Arc<WorkerInner>,
}

struct WorkerInner {
    sender: Option<mpsc::Sender<Job>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Drop for WorkerInner {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Worker {
    pub fn start(input: Input, gpu: bool) -> Result<Self, String> {
        let (sender, mut receiver) = mpsc::channel::<Job>(16);
        let (ready_sender, ready_receiver) = std::sync::mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("director-native-preview".into())
            .spawn(move || {
                let mut session = match open(&input, gpu) {
                    Ok(session) => session,
                    Err(error) => {
                        let _ = ready_sender.send(Err(error));
                        return;
                    }
                };
                if ready_sender.send(Ok(())).is_err() {
                    return;
                }
                while let Some(job) = receiver.blocking_recv() {
                    if session.source_changed() {
                        // A failure keeps the accepted scene and shows as `reload_error`.
                        let _ = session.reload();
                    }
                    job(&mut session);
                }
            })
            .map_err(|e| format!("native worker: {e}"))?;
        ready_receiver
            .recv()
            .map_err(|_| "native worker stopped during initialization")??;
        Ok(Self {
            inner: Arc::new(WorkerInner {
                sender: Some(sender),
                thread: Some(thread),
            }),
        })
    }

    pub async fn request<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut Session) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let (sender, receiver) = oneshot::channel();
        self.inner
            .sender
            .as_ref()
            .unwrap()
            .send(Box::new(move |session| {
                let _ = sender.send(operation(session));
            }))
            .await
            .map_err(|_| "native worker stopped")?;
        receiver.await.map_err(|_| "native worker stopped")?
    }
}

/// Open the input on the worker thread, which then owns the GPU context.
fn open(input: &Input, gpu: bool) -> Result<Session, String> {
    let mut session = input.open()?;
    crate::renderer::use_gpu(&mut session, gpu)?;
    Ok(session)
}

pub(super) fn metadata(session: &Session) -> serde_json::Value {
    let document = session.document();
    let source = session.source();
    serde_json::json!({
        "width": document.width,
        "height": document.height,
        "fps": document.fps,
        "duration": session.duration(),
        "script": source.map(|source| &source.input),
        "composition": source.and_then(|source| source.composition.as_ref()),
        "compositions": session.compositions(),
        "source_kind": session.source_kind(),
        "sample_rate": session.sample_rate(),
        "renderer": session.backend(),
        "revision": session.revision(),
        "reload_error": session.reload_error().map(|error| &error.message),
        "element_ids": session.names(),
    })
}

pub(super) fn png(session: &mut Session, time: f64) -> Result<Vec<u8>, String> {
    let frame = session.render_frame(time).map_err(|error| error.to_string())?;
    crate::renderer::frame_png(&frame)
}
