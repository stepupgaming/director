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
    /// Out-point within the source scene (seconds). 0 = use full scene (open end).
    pub source_end: f64,
    /// Playback speed multiplier (1.0 = normal, 0.5 = half speed, 2.0 = double).
    /// Values `<= 0.0` pin local time at `source_start` (hold first source frame).
    pub speed: f64,
    /// When set, pin source local time to this value for the entire clip (freeze hold).
    pub freeze_time: Option<f64>,
    /// Z-index for rendering order within the track.
    pub z_index: i32,
    /// Associated audio track IDs.
    pub audio_tracks: Vec<usize>,
    /// Fade-in duration in seconds (0 = no fade).
    pub fade_in: f64,
    /// Fade-out duration in seconds (0 = no fade).
    pub fade_out: f64,
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
            freeze_time: None,
            z_index: 0,
            audio_tracks: Vec::new(),
            fade_in: 0.0,
            fade_out: 0.0,
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
    ///
    /// Normative mapping (plan 01):
    /// - `freeze_time: Some(t)` → always `t`
    /// - `speed <= 0` → `source_start`
    /// - else `source_start + (global - start_time) * speed`, clamped to
    ///   `[source_start, source_end]` when `source_end > source_start`
    pub fn local_time(&self, global_time: f64) -> f64 {
        if let Some(t) = self.freeze_time {
            return t;
        }
        if self.speed <= 0.0 {
            return self.source_start;
        }
        let elapsed = global_time - self.start_time;
        let raw = self.source_start + elapsed * self.speed;
        if self.source_end > self.source_start {
            raw.clamp(self.source_start, self.source_end)
        } else {
            raw
        }
    }

    /// Whether this clip is active at `global_time` (half-open interval).
    pub fn is_active_at(&self, global_time: f64) -> bool {
        global_time >= self.start_time && global_time < self.start_time + self.duration
    }

    /// End time on the global timeline.
    pub fn end_time(&self) -> f64 {
        self.start_time + self.duration
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
            .filter(|c| self.enabled && c.is_active_at(global_time))
            .collect()
    }

    /// Returns mutable clips active at the given global time.
    pub fn active_clips_mut(&mut self, global_time: f64) -> Vec<&mut TrackClip> {
        self.clips
            .iter_mut()
            .filter(|c| c.is_active_at(global_time))
            .filter(|_| self.enabled)
            .collect()
    }
}

/// Convert a global frame index to time in seconds.
///
/// Policy: `frame / fps` (exact for the rational relationship).
pub fn frame_to_time(frame: u64, fps: f64) -> f64 {
    if fps <= 0.0 {
        return 0.0;
    }
    frame as f64 / fps
}

/// Convert a time in seconds to a frame index.
///
/// Policy: `floor(time * fps + 1e-9)` so export frame counts stay stable at
/// exact frame boundaries (avoids off-by-one from float noise).
pub fn time_to_frame(time: f64, fps: f64) -> u64 {
    if fps <= 0.0 || time <= 0.0 {
        return 0;
    }
    (time * fps + 1e-9).floor() as u64
}

/// Number of frames covering `[0, duration)` at `fps` (ceil of duration in frames).
pub fn duration_to_frame_count(duration: f64, fps: f64) -> u64 {
    if fps <= 0.0 || duration <= 0.0 {
        return 0;
    }
    (duration * fps - 1e-9).ceil().max(0.0) as u64
}

/// Interleaved stereo sample count for one video frame at `sample_rate` / `fps`.
///
/// Policy: `round(sample_rate / fps) * channels` with `channels = 2`.
/// Export mixes this many samples per frame so A/V length tracks
/// `duration_to_frame_count` (Plan 06).
pub fn audio_samples_per_video_frame(sample_rate: u32, fps: f64, channels: u32) -> usize {
    if fps <= 0.0 || sample_rate == 0 || channels == 0 {
        return 0;
    }
    let per_ch = (sample_rate as f64 / fps).round().max(0.0) as usize;
    per_ch.saturating_mul(channels as usize)
}

/// Total interleaved audio samples for a full export of `duration` seconds.
pub fn export_audio_sample_count(
    duration: f64,
    fps: f64,
    sample_rate: u32,
    channels: u32,
) -> usize {
    let frames = duration_to_frame_count(duration, fps) as usize;
    frames.saturating_mul(audio_samples_per_video_frame(sample_rate, fps, channels))
}

/// Map continuous source time (seconds) to a discrete video frame index.
///
/// **Policy: Floor** — `floor(time * source_fps + 1e-9)`.
/// Used by video decode seek so freeze / speed share one quantum story.
pub fn video_source_frame_index(source_time: f64, source_fps: f64) -> u64 {
    time_to_frame(source_time, source_fps)
}

/// Quantize continuous source time onto the source frame grid (floor policy).
///
/// Decode requests should use this time so identical floors hit the same cache key.
pub fn video_seek_time(source_time: f64, source_fps: f64) -> f64 {
    if source_fps <= 0.0 {
        return source_time.max(0.0);
    }
    let idx = video_source_frame_index(source_time.max(0.0), source_fps);
    frame_to_time(idx, source_fps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_local_time_speed() {
        let mut clip = TrackClip::new(NodeId::at(0), 10.0, 4.0);
        clip.speed = 2.0;
        assert!((clip.local_time(10.0) - 0.0).abs() < 1e-9);
        assert!((clip.local_time(11.0) - 2.0).abs() < 1e-9);

        clip.speed = 0.5;
        assert!((clip.local_time(10.0) - 0.0).abs() < 1e-9);
        assert!((clip.local_time(12.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn track_local_time_source_window() {
        let mut clip = TrackClip::new(NodeId::at(0), 0.0, 10.0);
        clip.source_start = 1.0;
        clip.source_end = 3.0;
        clip.speed = 1.0;
        assert!((clip.local_time(0.0) - 1.0).abs() < 1e-9);
        assert!((clip.local_time(1.0) - 2.0).abs() < 1e-9);
        assert!((clip.local_time(5.0) - 3.0).abs() < 1e-9); // clamped
    }

    #[test]
    fn track_freeze_pins_local_time() {
        let mut clip = TrackClip::new(NodeId::at(0), 5.0, 2.0);
        clip.speed = 1.0;
        clip.freeze_time = Some(1.25);
        assert!((clip.local_time(5.0) - 1.25).abs() < 1e-9);
        assert!((clip.local_time(6.5) - 1.25).abs() < 1e-9);
    }

    #[test]
    fn track_non_positive_speed_pins_source_start() {
        let mut clip = TrackClip::new(NodeId::at(0), 0.0, 2.0);
        clip.source_start = 0.5;
        clip.speed = 0.0;
        assert!((clip.local_time(1.0) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn frame_time_inverses() {
        let fps = 29.97;
        for frame in [0u64, 1, 30, 2997, 10000] {
            let t = frame_to_time(frame, fps);
            assert_eq!(time_to_frame(t, fps), frame);
        }
        // Near-boundary float: just below next frame stays on current.
        let t = frame_to_time(10, 24.0);
        assert_eq!(time_to_frame(t + 1e-12, 24.0), 10);
    }

    #[test]
    fn fps_ntsc_duration_frames() {
        let fps = 23.976;
        let duration = 100.0;
        let n = duration_to_frame_count(duration, fps);
        assert!(n > 0);
        // time of last frame index should be < duration
        assert!(frame_to_time(n - 1, fps) < duration);
        assert!(frame_to_time(n, fps) >= duration - 1e-6);
    }
}
