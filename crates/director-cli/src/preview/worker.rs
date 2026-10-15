use crate::{
    input::{Input, LoadedInput},
    renderer::Renderer,
};
use director_core::video_wrapper::RenderMode;
use std::{path::PathBuf, sync::Arc, thread, time::SystemTime};
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
                let mut session = match Session::new(input, gpu) {
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
                    session.refresh();
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

pub(super) struct Session {
    pub loaded: LoadedInput,
    pub renderer: Renderer,
    input: Input,
    revision: u64,
    fingerprints: Vec<(PathBuf, Option<(SystemTime, u64)>)>,
    last_reload_error: Option<String>,
    audio: Option<Vec<u8>>,
}

impl Session {
    fn new(input: Input, gpu: bool) -> Result<Self, String> {
        let loaded = input.load(RenderMode::Export)?;
        let renderer = Renderer::new(gpu)?;
        let mut session = Self {
            loaded,
            renderer,
            input,
            revision: 1,
            fingerprints: Vec::new(),
            last_reload_error: None,
            audio: None,
        };
        session.fingerprints = session.fingerprints();
        Ok(session)
    }

    pub fn metadata(&self) -> Result<serde_json::Value, String> {
        let director = self
            .loaded
            .movie
            .director
            .lock()
            .map_err(|_| "Director lock poisoned")?;
        Ok(serde_json::json!({
            "width":director.width,"height":director.height,"fps":director.fps,"duration":director.timeline_duration(),
            "script":self.input.input,"composition":self.input.composition,"compositions":self.loaded.compositions,
            "source_kind":self.loaded.source_kind,"sample_rate":self.loaded.sample_rate,"renderer":self.renderer.backend(),
            "revision":self.revision,"reload_error":self.last_reload_error,"element_ids":self.loaded.element_ids
        }))
    }

    pub fn frame(&mut self, time: f64) -> Result<Vec<u8>, String> {
        let mut director = self
            .loaded
            .movie
            .director
            .lock()
            .map_err(|_| "Director lock poisoned")?;
        self.renderer.png(&mut director, time)
    }

    pub fn reload(&mut self) -> Result<(), String> {
        let candidate = self.input.load(RenderMode::Export)?;
        self.renderer.reset();
        self.loaded = candidate;
        self.revision += 1;
        self.audio = None;
        self.last_reload_error = None;
        self.fingerprints = self.fingerprints();
        Ok(())
    }

    fn refresh(&mut self) {
        let fingerprints = self.fingerprints();
        if fingerprints != self.fingerprints {
            if let Err(error) = self.reload() {
                // Keep the accepted scene and retry when the source changes again.
                self.fingerprints = fingerprints;
                self.last_reload_error = Some(error);
            }
        }
    }

    fn fingerprints(&self) -> Vec<(PathBuf, Option<(SystemTime, u64)>)> {
        std::iter::once(self.input.input.clone())
            .chain(self.loaded.dependency_paths.iter().cloned())
            .map(|path| {
                let metadata = std::fs::metadata(&path)
                    .ok()
                    .and_then(|m| m.modified().ok().map(|t| (t, m.len())));
                (path, metadata)
            })
            .collect()
    }

    pub fn audio(&mut self) -> Result<Vec<u8>, String> {
        if let Some(audio) = &self.audio {
            return Ok(audio.clone());
        }
        let mut director = self
            .loaded
            .movie
            .director
            .lock()
            .map_err(|_| "Director lock poisoned")?;
        let rate = director.audio_mixer.sample_rate;
        let frames = director_core::timeline::duration_to_frame_count(
            director.timeline_duration(),
            director.fps,
        );
        let samples = director_core::timeline::audio_samples_per_video_frame(rate, director.fps, 1);
        let data_size = frames
            .checked_mul(samples as u64)
            .and_then(|n| n.checked_mul(8))
            .filter(|n| *n <= (u32::MAX - 36) as u64)
            .ok_or("audio exceeds RIFF/WAV size limit")? as u32;
        let mut wav = Vec::with_capacity(data_size as usize + 44);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(data_size + 36).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&3u16.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 8).to_le_bytes());
        wav.extend_from_slice(&8u16.to_le_bytes());
        wav.extend_from_slice(&32u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_size.to_le_bytes());
        let fps = director.fps;
        for frame in 0..frames {
            for sample in director.mix_audio(samples, frame as f64 / fps) {
                wav.extend_from_slice(&sample.to_le_bytes());
            }
        }
        self.audio = Some(wav.clone());
        Ok(wav)
    }
}
