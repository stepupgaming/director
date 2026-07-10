use crate::types::NodeId;

/// A clip on a track, referencing a scene root node with timing and playback parameters.
#[derive(Clone, Debug)]
pub struct TrackClip {
    /// The root node of the scene content.
    pub scene_root: NodeId,
    /// Optional label.
    pub name: Option<String>,
    /// Position on the track timeline in seconds.
    pub start_time: f64,
    /// Rendered duration in seconds (affected by speed).
    pub duration: f64,
    /// In-point within the source scene (seconds).
    pub source_start: f64,
    /// Out-point within the source scene (seconds). 0 = use full scene.
    pub source_end: f64,
    /// Playback speed multiplier (1.0 = normal, 0.5 = half speed, 2.0 = double).
    pub speed: f64,
    /// Z-index for rendering order within the track.
    pub z_index: i32,
    /// Associated audio track IDs.
    pub audio_tracks: Vec<usize>,
    /// Fade-in duration in seconds (0 = no fade).
    pub fade_in: f64,
    /// Fade-out duration in seconds (0 = no fade).
    pub fade_out: f64,
    /// When `Some(t)`, the renderer keeps sampling the source at local time
    /// `t` for the entire `duration` window — the canonical "freeze frame"
    /// mechanism used by `add_freeze`.
    pub freeze_time: Option<f64>,
}

impl TrackClip {
    pub fn new(scene_root: NodeId, start_time: f64, duration: f64) -> Self {
        Self {
            scene_root,
            name: None,
            start_time,
            duration,
            source_start: 0.0,
            source_end: 0.0,
            speed: 1.0,
            z_index: 0,
            audio_tracks: Vec::new(),
            fade_in: 0.0,
            fade_out: 0.0,
            freeze_time: None,
        }
    }

    /// Compute effective opacity contribution from fade-in/fade-out at a given global time.
    pub fn fade_opacity(&self, global_time: f64) -> f32 {
        let local = global_time - self.start_time;
        if self.fade_in > 0.0 && local < self.fade_in {
            (local / self.fade_in) as f32
        } else if self.fade_out > 0.0 && local > self.duration - self.fade_out {
            ((self.duration - local) / self.fade_out).max(0.0) as f32
        } else {
            1.0
        }
    }

    /// Returns the local time within the source for a given global time.
    pub fn local_time(&self, global_time: f64) -> f64 {
        // Freeze overrides everything: the clip keeps sampling the same
        // source frame for its entire visible window.
        if let Some(frozen) = self.freeze_time {
            return frozen;
        }
        if self.speed <= 0.0 {
            return self.source_start;
        }
        let elapsed = global_time - self.start_time;
        self.source_start + elapsed * self.speed
    }
}

/// A compositing track containing clips.
#[derive(Clone, Debug)]
pub struct Track {
    /// Clips ordered by start_time.
    pub clips: Vec<TrackClip>,
    /// Display name.
    pub name: String,
    /// Whether the track is enabled.
    pub enabled: bool,
    /// Track opacity.
    pub opacity: f32,
}

impl Track {
    pub fn new(name: &str) -> Self {
        Self {
            clips: Vec::new(),
            name: name.to_string(),
            enabled: true,
            opacity: 1.0,
        }
    }

    /// Returns clips active at the given global time.
    pub fn active_clips(&self, global_time: f64) -> Vec<&TrackClip> {
        self.clips
            .iter()
            .filter(|c| {
                self.enabled
                    && global_time >= c.start_time
                    && global_time < c.start_time + c.duration
            })
            .collect()
    }

    /// Returns mutable clips active at the given global time.
    pub fn active_clips_mut(&mut self, global_time: f64) -> Vec<&mut TrackClip> {
        self.clips
            .iter_mut()
            .filter(|c| {
                self.enabled
                    && global_time >= c.start_time
                    && global_time < c.start_time + c.duration
            })
            .collect()
    }
}
