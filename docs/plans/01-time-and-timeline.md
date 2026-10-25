# Plan 01 — Time & Timeline Unification

> **Status:** Implemented (2026-07-19)  
> **Priority:** P0 — first implementation phase  
> **Depends on:** none  
> **Blocks:** 02 (partial), 03, 04, 06

## Problem

Director has **two timeline representations** and an **integer FPS** type:

1. Legacy `Director.timeline: Vec<TimelineItem>` — what `add_scene`, `add_loop`, `add_freeze` still write (`scripting/api/lifecycle.rs`).
2. Multi-track `Director.tracks: Vec<Track>` / `TrackClip` — richer (speed, source in/out, fades) but not the default authoring path.
3. `collect_active_roots` prefers tracks if any clip exists, else falls back to legacy — dual semantics.
4. `Director.fps: u32` (also encoder, manifest, schema). Only `project.rs` stores `f64`. Fractional rates (23.976, 29.97) cannot round-trip.
5. `add_freeze` builds a legacy item and **discards `freeze_at`**.
6. `add_loop` copies roots onto legacy timeline with **empty `audio_tracks`**.
7. Most visual tests push `TimelineItem` directly — cementing the legacy path.

## Goals

- **One** timeline model for update, render, audio, export, project I/O.
- Clip-local time fully defined: speed, source window, freeze, fades.
- `f64` FPS end-to-end with documented frame↔time mapping.
- No silent stubs for freeze/loop.

## Non-goals

- Preview UI, compositions registry, new transition types.
- Changing FFmpeg container choices.
- Audio ducking graph (only preserve bindings on loop/freeze).

## Target model

### Single timeline

```text
Director
  tracks: Vec<Track>          // only sequencing structure
  default_track_idx: usize
  fps: f64
  // timeline: REMOVED (or private migration-only during PR, then deleted)
```

`TrackClip` becomes the only clip type:

| Field | Role |
|-------|------|
| `scene_root` | Root `NodeId` |
| `name` | Optional label |
| `start_time` / `duration` | Placement on global timeline (seconds) |
| `source_start` / `source_end` | Source window (`source_end == 0` ⇒ open end / full) |
| `speed` | Playback rate (`> 0`; document `<= 0` as error or freeze-at-source_start) |
| `freeze_time: Option<f64>` | If `Some(t)`, pin source local time to `t` for entire clip |
| `fade_in` / `fade_out` | Opacity ramps (already present) |
| `z_index` | Order within track |
| `audio_tracks` | Mixer track indices tied to this clip |

### Local time function (normative)

```text
fn local_time(clip, global_t) -> f64:
  if freeze_time is Some(t):
    return t
  if speed <= 0:
    return source_start   // or hard-error at authoring time — pick one and test it
  elapsed = global_t - start_time
  raw = source_start + elapsed * speed
  if source_end > source_start:
    return clamp(raw, source_start, source_end)
  return raw
```

Apply the same function in:

- `Director::collect_active_roots` / `update`
- Export frame loop
- Audio association windows for clip-tied tracks
- Nested `CompositionNode` offset math (parent local → child global)

### FPS & frame addressing

```rust
// End-to-end
Director.fps: f64
EncoderSettings.fps: f64
RenderManifest.fps: f64
MovieRequest.fps: f64          // director-schema
Project.fps: f64               // already

fn frame_to_time(frame: u64, fps: f64) -> f64 { frame as f64 / fps }
fn time_to_frame(time: f64, fps: f64) -> u64 {
  // Documented policy: round half away from zero OR floor — choose ONE.
  // Recommended: floor(time * fps + 1e-9) for stable export counts.
}
```

Rhai: `movie_fps`, `frame_to_time`, `time_to_frame` (add if missing; wire to real `f64`).

Export frame count: `total_frames = time_to_frame(duration, fps)` with the same policy as seeking.

## API changes

### Rhai / lifecycle

| API | Change |
|-----|--------|
| `add_scene` | Push `TrackClip` onto `default_track` (not `timeline`) |
| `add_scene_on_track` | Already track-oriented; ensure no dual write |
| `add_loop` | Append N-1 clips on same track; **copy `audio_tracks`**; share or clone root per policy (document: shared root OK if immutable during play) |
| `add_freeze` | Append clip with `freeze_time: Some(freeze_at)`, `duration: hold_duration` |
| `set_clip_freeze` | New or complete: set/clear `freeze_time` on a clip handle |
| Sequence helpers | Advance using track clips only |

Deprecate any public exposure of `TimelineItem` after migration. Internal test helpers may use a `push_clip(director, …)` utility.

### Rust

- Delete `TimelineItem` **or** type-alias it to a thin adapter during one PR, then remove.
- `collect_active_roots`: tracks only.
- Project save/load: write tracks only; readers that see legacy `timeline` JSON migrate once then drop.
- Schema: fps `number` (float); document integer still accepted on ingest.

## Implementation steps

1. **Audit call sites** of `timeline.push`, `TimelineItem`, `.timeline` (core, tests, visual fixtures, pipeline, project).
2. Add `freeze_time` to `TrackClip` + unit tests for `local_time`.
3. Switch `add_scene` / sequence / loop / freeze to tracks.
4. Implement real freeze in time mapping; wire audio copy on loop.
5. Widen `fps` to `f64` through director → export → manifest → schema → Rhai.
6. Add `frame_to_time` / `time_to_frame` with fixed rounding tests.
7. Migrate visual tests to track helper; remove legacy branch in `collect_active_roots`.
8. Project JSON: migrate-on-load for old `timeline` arrays; stop emitting them.
9. Update examples that document freeze/loop; fix `api_catalog` strings that claim freeze works if they already do.

## Tests (acceptance)

| Test | Asserts |
|------|---------|
| `track_local_time_speed` | speed 0.5 / 2.0 maps global→local correctly |
| `track_local_time_source_window` | clamp to `[source_start, source_end]` |
| `track_freeze_pins_local_time` | entire hold window samples same local `t` |
| `add_freeze_rhai_pins_frame` | pixel or inspected local_time stable across hold |
| `add_loop_copies_audio_track_ids` | audio_tracks non-empty on clones |
| `fps_ntsc_roundtrip` | 23.976 / 29.97 survive project save/load and encoder settings |
| `frame_time_inverses` | documented near-inverse within 1e-6 for integer frames |
| `no_legacy_timeline_in_update` | unit: empty tracks ⇒ no roots even if someone sets removed field (or field gone) |
| Visual: freeze hold | two frames mid-hold identical (mock_video OK) |

## Files likely touched

- `crates/director-core/src/timeline.rs`
- `crates/director-core/src/director.rs`
- `crates/director-core/src/scripting/api/lifecycle.rs`
- `crates/director-render/src/live.rs` (export loop), `crates/director-core/src/video_wrapper.rs`
- `crates/director-core/src/render_manifest.rs`, `project.rs`
- `crates/director-schema/**`
- `crates/director-core/tests/visual/**` (helper migration)
- `crates/director-core/src/api_catalog.rs` (fps/freeze blurbs)

## Risks

- Mass visual-test churn when switching off `TimelineItem` — use one test helper to limit diff noise.
- Nested compositions: ensure inner director fps/time offset still defined after outer uses tracks only.
- Breaking JSON: version bump project format; migrate old files once.

## Done when

- [x] `Director.timeline` gone from public API and update/render paths  
- [x] `TrackClip.freeze_time` implemented and used by `add_freeze`  
- [x] `add_loop` preserves audio track ids  
- [x] `fps: f64` everywhere that participates in export/manifest/schema  
- [x] Acceptance tests above green (`crates/director-core/tests/time_timeline.rs`)  
- [x] `docs/architecture/core-contracts.md` time section matches code  
- [x] Archive note: any old audit claim about freeze/fps updated via this plan’s status  

### Landed notes

- `TimelineItem` remains as a **derived DTO** from `Director::flat_timeline()` (transitions/manifests/tests), not a second stored timeline.
- Project JSON still accepts `legacy_timeline` on load and migrates into default-track clips; saves emit tracks only (`legacy_timeline: []`).
- Frame policy: `time_to_frame = floor(time * fps + 1e-9)`; export frame count uses `duration_to_frame_count`.
- Known unrelated failure observed while verifying: `node::caption::tests::test_measure_height_matches_block_layout` (not caused by this plan).  
