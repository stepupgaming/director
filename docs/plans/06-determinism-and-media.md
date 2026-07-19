# Plan 06 — Determinism & Media Clock

> **Status:** Spec  
> **Priority:** P1 (continuous; fonts/CI can land immediately)  
> **Depends on:** benefits from 01 (fps/time); independent pieces can ship anytime  
> **Blocks:** reliable visual goldens and reproducible exports

## Problem

1. **Fonts:** `BUNDLED_FONTS` marks only Roboto-Regular as required; Bold/Italic/Inter/Poppins warn if missing. Bold text and caption defaults (Poppins) can still hit host fonts → non-reproducible pixels.
2. **RNG:** seedable path landed — must verify all noise/random call sites honor it (no stray `thread_rng`).
3. **Video decode seek:** frame selection policy (nearest/floor/continuous) not locked; speed ≠ 1 and freeze interact with decoder cache.
4. **Audio vs video clocks:** mixer sample rate (e.g. 48000) vs `fps` frame grid — duration alignment and A/V sync under speed/freeze need a single story.
5. **CI:** visual suite and font pinning must run on PR/push; OS variance needs policy (mock_video + pinned fonts minimum).
6. **Color:** export tags improved; keep tests so they don’t regress; premul sRGB compositing remains documented default.

## Goals

- Same script + same assets + same seed ⇒ **same pixels** on CI reference OS (and best-effort elsewhere).
- Media time mapping documented for video nodes and audio mix under track speed/freeze.
- CI gates core determinism.

## Non-goals

- Bit-identical pixels across Windows/macOS/Linux Skia builds (document tolerance or OS-suffixed goldens if needed).
- Full color-managed linear workflow (optional later).
- Online font download inside the engine at render time.

## Target model

### Fonts

| Family | Weights | Required in CI |
|--------|---------|----------------|
| Roboto | Regular, Bold, Italic | yes |
| Poppins | Bold (caption default) | yes if captions in goldens |
| Inter | Regular, Bold | yes if examples use them |
| Emoji fallback | single ttf | yes if any golden has emoji |

- Missing **required** font ⇒ hard error at `Director` construction in `RenderMode::Export` and in CI.
- Dev may warn-only behind explicit env only if needed — default fail-closed for export.

### Seed

- Default seed documented (`0xC0FFEE` or current).
- `set_seed` resets all engine RNGs used by `rand_float`, noise, any effect that samples random.
- Grep guard test: no `thread_rng` in `director-core/src` outside tests.

### Video clock

```text
video_source_time = clip_local_time  // after plan 01 mapping
frame_policy = Floor  // or Nearest — pick one
decode(frame_policy(video_source_time))
```

- Cache key: `(asset, policy_time_quantum)`  
- Under `freeze_time`, decoder must not advance.  
- Test with short fixture video: known frame indices at t0, t1.

### Audio clock

- Mix in domain of `sample_rate`.
- Clip-tied audio respects start/duration/speed (and freeze ⇒ hold or silence — **document choice**; recommend silence or last-window policy explicitly).
- Export: number of audio samples derived from same duration as video frame count policy (plan 01).

### CI

- `ci.yml`: on pull_request + push to main.
- Install/setup fonts via `setup_assets.sh` (or committed fonts under force-kept path).
- `cargo test -p director-core` + clippy + fmt.
- Visual suite with `mock_video` on all OS; real ffmpeg job on one Linux runner if available.

## Implementation steps

1. Make caption/default families’ fonts required; fail export if missing.
2. Align `setup_assets.sh` + CI + `.gitignore` force-keep with `BUNDLED_FONTS`.
3. Grep RNG; eliminate `thread_rng` in library code.
4. Document + test video seek policy; freeze interaction.
5. Audio duration vs video duration unit test on short export (mock OK if it records sample counts).
6. CI workflow triggers + font step.
7. Optional: OS suffix for goldens only if cross-OS fails after pin.

## Tests (acceptance)

| Test | Asserts |
|------|---------|
| `required_fonts_present_in_export` | construction/export fails if Bold missing (feature-gated temp) |
| `no_thread_rng_in_core` | lint or grep test |
| `seed_stable_noise_frames` | two runs identical |
| `video_seek_floor_policy` | expected frame ids |
| `freeze_does_not_advance_video` | same decode time |
| `export_av_duration_alignment` | audio samples ↔ video frames policy |
| CI green on PR | process |

## Files likely touched

- `crates/director-core/src/director.rs` (`BUNDLED_FONTS`)
- `setup_assets.sh`, `.github/workflows/*`, `.gitignore`
- `crates/director-core/src/node/video_node.rs`, `video_wrapper.rs`
- `crates/director-core/src/audio.rs`, `export/*`
- `crates/director-core/src/scripting/api/lifecycle.rs` (seed)

## Risks

- Font license/binary size — keep minimal set.  
- Cross-OS Skia differences — don’t block core on perfect cross-OS; pin CI OS first.

## Done when

- [ ] Required fonts fail-closed in export/CI  
- [ ] No unseeded RNG in core lib  
- [ ] Video seek + freeze policy tested  
- [ ] A/V duration policy documented + tested  
- [ ] CI runs on PR/push with fonts  
