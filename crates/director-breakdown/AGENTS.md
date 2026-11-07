# Purpose

- Measure a reference video for replication: its shots, the transitions between them, motion-graphics beats inside them and what the container says about where it came from. The output is `breakdown.json`, the measured half of a video's DNA. Replicating the video belongs to later layers.

## Ownership

- `src/media.rs` - `VideoInfo::probe` through `ffprobe` (size, rotation, display aspect, fps, duration, frame count, codec, audio), and `for_each_frame`, which streams RGB frames scaled to a width through the `ffmpeg` CLI with `-fps_mode passthrough`. `frame_times` reads packet timestamps and returns them only when they stray from `frame / fps`.
- `src/track.rs` - `FrameTrack` and `TrackBuilder`: per frame, the HSV histogram distance to the previous frame, the mean pixel difference, the fraction of pixels that changed, mean luma, a gray thumb `THUMB_W` (96) px wide and its standard deviation.
- `src/shots.rs` - `detect_shots`, `ShotOptions`, `Shot` and `Transition`: hard cuts, fades through black or white, dissolves, flashes and motion-graphics beats, and `motion_explains`, the block matcher that tells camera or subject motion from new content.
- `src/probe.rs` - `provenance`: container format, encoder, handler names, creation time, camera tags, AIGC labels (`parse_aigc` for the GB 45438 and TikTok schemes) and `hints` from the signature table matched against tags and printable strings in the file's first and last 4 MB.
- `src/lib.rs` - `analyze` (probe, track, detect, provenance), the `Breakdown` document with `FORMAT` (`director-breakdown/1`), `Source`, `Timings` and `schema`.
- `NOTICE` and `LICENSE-APACHE` - attribution for the rea-video code the detector and probe port.

## Local Contracts

- The crate links no Skia, GPU or ML runtime. Decoding needs `ffmpeg` and `ffprobe` in PATH. Every step runs locally; language-model steps that read a breakdown live in callers.
- `detect_shots` ports rea-video's `detect_shots` (JuneYaooo/rea-video at c9359c4, Apache-2.0) and keeps its thresholds, so a breakdown matches the Python baseline boundary for boundary. Gray uses OpenCV's fixed-point BT.601 weights and the histogram OpenCV's fixed-point HSV with [16, 4, 4] bins and its Bhattacharyya distance.
- `motion_explains` replaces OpenCV's Farneback flow: 8 px blocks of the earlier thumb search the later one within a reach of an eighth of the width, and motion explains the change when the matched difference falls below 0.45 of the raw one inside a 4 px margin. A block whose range is 6 levels or less stays in place, as optical flow does where it has no gradient.
- A cut needs a score above `cut_threshold` that also stands well above the local median, a large changed fraction, or a one-frame swap after pixel-identical frames. A fade needs luma below the lesser of 10 and 0.35 of the clip's median luma (or above 248) for at most 2.5 s through a nearly uniform frame. A fade with one black frame reads as a cut followed by a dissolve. A held black reads as its own shot: the last faded frame cuts to it, and the fade in closes it. Shots shorter than `min_shot_s` merge into the shot before.
- `Shot` times are seconds from packet timestamps when `variable_frame_rate` is set, else `frame / fps`. `end_frame` of the last shot equals the frame count.
- Provenance hints are evidence, never proof: an absent signature says nothing. Platform labels are global (`TikTok/Douyin`, `YouTube`, `Instagram`), and `tool: FFmpeg` marks a Lavf or Lavc muxer.

## Work Guidance

- Change a detector threshold only with the parity run below and a note of what moved. The baseline clips, the rea-video clone and its venv live on `F:/director-spikes/rea-video` and are not committed.

## Verification

- `cargo test -p director-breakdown --release` builds synthetic edits with ffmpeg lavfi (a cut, a dissolve and a held fade; a pan; held slides) and tagged containers.
- Parity: run `director-engine breakdown <clip> -o <dir>` and rea-video's `detect_shots` on the same clips, and match boundaries within 2 frames.

## Child DOX Index

- None.
