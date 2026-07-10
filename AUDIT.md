# Director Engine — Architecture & Agent-Readiness Audit

*Audit performed against the repository at `C:\Projects\test-app-3`. All findings are grounded in specific files, modules, and line numbers. The goal is to assess whether Director can become a genuinely capable foundation for an AI-driven / agent-driven video creation system.*

---

# Executive Summary

Director is a **competent, layout-aware, multi-track 2D motion-graphics engine** with genuinely good fundamentals: a real hierarchical scene graph, Taffy flexbox/grid layout with intrinsic text measurement, Skia rendering with blend modes / filters / masks / nested offscreen pre-comps, a broad Rhai authoring API, a thoughtful TikTok-style caption system, and an actual visual-regression test harness. For hand-authored scripts producing short social clips, it already works.

But it is **not yet a foundation for an agent-driven video system**, and it is **not ready to compete with Remotion / Motion Canvas / Hyperframes** on motion-graphics depth. The blocking problems are architectural, not cosmetic:

1. **No introspection/inspection API** — agents cannot query node bounds, durations, media metadata, or the scene tree. Director is a black box after authoring.
2. **No project (de)serialization** of the live graph; the JSON `director-schema` is one-way and already drifts from the Rhai API (only ~6 of ~20 transitions, no SKSL, no `add_loop`/`add_freeze`).
3. **Animation is shallow** — no bezier/hold keyframes, no reverse/yoyo/repeat, no expressions, silent no-ops on bad names, no multi-keyframe chaining, and `fps: u32` rules out 23.976/29.97.
4. **Text correctness risks** — forced LTR (bidi broken), only Roboto-Regular bundled (non-deterministic bold/non-Latin), caption height estimated by a 0.55 magic constant that can diverge from real wrap, and a `Shrink` fit that fights font-size animation.
5. **Render correctness bugs** — unnormalized motion-blur weights over-brighten multi-sample frames, SkSL effects assume node origin (0,0) so UV math breaks off-origin, and no color-range tags on FFmpeg exports risk washed-out video.
6. **Agent safety gaps** — `set_max_expr_depths(0,0)` allows runaway scripts, non-seedable RNG breaks determinism, `apply_effect`/`set_mask` silently rewrite handles.

**Maturity verdict:** a strong prototype / internal tool, not a v1-capable agentic platform. Ready today for deterministic, scripted, single-composition social clips authored by a person. Not ready for autonomous iterative authoring, reliable captioning at scale, or professional color-correct export.

> **Note:** Several findings below are stale. See **Progress Since Audit** (2026-07-08) before prioritizing work.

---

# Progress Since Audit

*Last updated: 2026-07-09. Tracks work landed after the original audit pass.*

## Closed / fixed

| Finding | What changed |
|---|---|
| **#3** Motion-blur weights | Normalized to equal `1/samples` in `systems/renderer.rs` and `export/video.rs` (was harmonic `1/(s+1)` → over-bright frames). |
| **#9** Caption height estimate | `CaptionNode::measure` and render share `build_block_layout` SkParagraph metrics; the `font_size * 0.55` line-count guess is gone. |
| **#11** FFmpeg color tags | H.264 export passes `-color_range pc`, `-colorspace bt709`, `-color_primaries bt709`, `-color_trc bt709` in `video_wrapper.rs`. |
| **Captions (general)** | Production-tier kinetic captions: pop-overlay flash fix (base pass + clipped overlay), overlap cues replace in one anchor (no Y-slot stack), `phrase_line_mode: Replace` (one phrase line at a time), 3+ line phrase timing via `display_line_window_fracs`, dimensional scaling from `reference_canvas_height` (1920) not the tight layout box. 23 unit tests + visual fixtures + `caption_god_tier_proof.rhai`. Commits through `1d8cce4`. |
| **#4** SkSL UV origin (likely) | `effect_uv_probe` harness asserts layer-local `pos`; audit claim that off-origin nodes break UVs may be obsolete — re-verify on Liquid/Acid presets, then close formally. |
| **#2** Keyframe round-trip | `project.rs` now serializes `Animated` **keyframes** (not just `current_value`) for transform channels (scale/rotation/skew/translate) and Box props (opacity, bg_color, border_*, shadow_*, blur, backdrop_blur); `load_project_json` rebuilds them so motion reproduces exactly after `update(t)`. First-keyframe easing preserved via `add_segment` semantics. `project_roundtrip_preserves_animation` test asserts eased mid-point + end values match the original. Taffy layout also round-trips (position/size/inset/flex/align/margin/padding/gap); non-Box kinds revive as Box placeholders with layout applied. `project_roundtrip_preserves_layout` asserts the full layout JSON is identical after reload. |
| **#5** Animation easing typos | `parse_easing` keeps warn+fall-back; new `parse_easing_loud` (used by all Rhai `animate`/`spring`/`path_animate` paths) emits `tracing::error!` + the full valid-name list on unknown easings. `parse_easing_result` returns `Result<EasingType, String>` for callers that want to throw. Unknown-property path in `animate` still emits `error!` (log loud + no-op). |
| **#12** `fps: u32` | Widened to `f64` end-to-end: `Director.fps`, `EncoderSettings.fps`, `RenderManifest.fps`, `MovieRequest.fps`, `Composition::fps`, `Project.fps`. `Director::frame_to_time` / `time_to_frame` exposed as Rhai `frame_to_time` / `time_to_frame` / `movie_fps`. 23.976/29.97 round-trip without truncation. |
| **#14** Rhai depth limits | Landed: `MAX_EXPR_DEPTH = MAX_EXPR_DEPTHS = 128` (not 0/0), and `set_script_limits(engine, depth, depths)` is the public override. |
| **#15** Seedable RNG | `set_seed(i64)` re-seeds a static `StdRng`; `rand_float` / `noise2d` / `noise3d` consume that seeded RNG. Default seed `0xC0FFEE` so cold runs are deterministic. |

## Partially addressed

| Finding | Progress | Still missing |
|---|---|---|
| **#1** Introspection API | Rhai: `node_id`, `node_bounds`, `node_children`, `scene_duration`, `list_scene_roots`; registry reflection (`list_effects`, `list_easings`, `get_effect_meta`, …). `render_manifest_json` + `director-manifest` CLI. `tests/inspect_api.rs`. | `media_metadata()`; manifest on every export by default; debug overlays / contact sheets; temporal diagnostics (overlap, clip, contrast). |
| **#2** Project serialization | `api_catalog` single source of truth started; `director-schema` gained `ProjectManifest` + combined schema bundle; platform manifest CLI. **Live `Director` round-trip now works** (`save_project_json`/`load_project_json`): topology, transform, opacity, z-index, blend, Box props, keyframe animation, and Taffy layout all round-trip; `project_roundtrip` suite (7 tests) guards it. **All major node kinds now round-trip as their kind, not as Box placeholders**: TextNode spans + style, ImageNode source path + object_fit + border_radius, VideoNode (path + loaded frame pixel sequence via SK renderer), CaptionNode (lines + words + style + safe_area), EffectNode (every `EffectType` variant tagged with `kind`), CompositionNode (inner `Director` JSON). | Schema still drifts (~6 vs ~20 transitions; no SKSL/`add_loop`/`add_freeze`); Vector/SpectrumVisualizer/Lottie kinds still rollback to Box placeholders. |
| **#7** Text direction / bidi | `text_direction.rs` + `text_direction` Rhai prop on text/caption; auto-detect first strong; caption RTL test. | Full bidi parity across all text nodes; deterministic emoji/complex-script shaping. |

## Still open (priority order)

1. **#2** Live project (de)serialization + schema generated from Rhai API. **All major node kinds now land as their kind, not Box placeholders** (`project.rs`, 8-test `project_roundtrip` suite covering Box, Text, Image, Video, Caption, Effect, Composition, layout, animation). Vector/Spectrum/Lottie kinds still rebuild as Box placeholders (low-priority per user direction).
2. **#1** Finish introspection: `media_metadata`, manifest-on-export, machine-readable validation diagnostics.
3. **#5–#6** Animation: loud errors implemented (find #5 closed); bezier/hold/reverse/yoyo/expressions unstarted.
4. **#8** Bundle and pin deterministic fonts (Regular/Bold + emoji/CJK fallback).
5. **#10, #12–#17** Text-fit decoupling, handle-rewrite semantics, static cache, richer masking (find #12 closed via `fps: f64`).
6. **#18, #20** CI on push/PR, audio/export depth.
7. **#19** Real `add_freeze` + audio-preserving `add_loop` — closed: `TimelineItem.freeze_time: Option<f64>` and `TrackClip.freeze_time: Option<f64>` make the renderer pin local_time to `freeze_at` for the entire hold window; `add_loop` copies source `audio_tracks` onto every cloned iteration; new `set_clip_freeze` Rhai binding. `freeze_and_loop_preserve_audio_and_freeze_time` test pins the contract.

Roadmap sections **Agent-first extensions** and **v1.0 architecture bets** (immutable draft model, parameterized components) remain entirely ahead. **Things Not To Build Yet** unchanged.

---

# Architecture Map

**Crates (root `Cargo.toml`, workspace `resolver=2`):**
- `director-core` — the "god object" `Director` (`director.rs:59`) owning `SceneGraph` (arena `Vec<Option<SceneNode>>`, `scene.rs:117`), `LayoutEngine`, timeline `tracks: Vec<Track>` (`director.rs:65`), assets, context, transitions, audio mixer, plus rendering (`systems/renderer.rs`, `systems/layout.rs`), nodes (`node/*`), the Rhai API (`scripting/*`), and export (`export/*`).
- `director-cli` — `main.rs` (render CLI), `preview-frame`, `review-visuals`, `verify-lottie`.
- `director-schema` — JSON types; consumed one-way by `director-pipeline::load_movie`.
- `director-pipeline` — schema→`Director` loader + parity/error tests.
- `lottie-core/-data/-skia` — separate Lottie import path with its *own* bezier/hold/expression engine (`lottie-core/src/animatable.rs`).
- `vendor/skia-safe` — patched skia-safe 0.90.0.

**Render flow:** `Director::update(time)` sets `local_time` per node and propagates it to children (`director.rs:331-339`) → `LayoutEngine::compute_layout` builds a Taffy tree each frame and writes `layout_rect` back (`systems/layout.rs:45-204`) → `run_post_layout` (text fit, etc.) → `render_recursive` applies transform stack and draws, opening `save_layer`s for mask/blend/filter (`systems/renderer.rs:43-167`).

**Scripting flow:** Rhai `Engine` registered via `register_rhai_api_with_default_mode` (`scripting/mod.rs:45`). Scripts hold `NodeHandle`/`SceneHandle` wrapping `Arc<Mutex<Director>>` and mutate the live `Director` directly (`scripting/types.rs:31-57`). No draft/immutable model.

**Layout flow:** Taffy leaf-per-node (`new_leaf_with_context`, `layout.rs:57`); `TextNode`/`CaptionNode`/`ImageNode` supply intrinsic `measure` callbacks (`layout.rs:136-150`). Layout recomputed every frame, gated by a `dirty_style` flag (`layout.rs:70-79`).

**Media/export flow:** `VideoNode`/`AudioBinding` read decoded frames; export spawns FFmpeg via `video_wrapper.rs` (H.264 yuv420p CRF18 or alpha VP9 WebM). Motion blur samples sub-frames and accumulates (`renderer.rs:261-284`).

**Ownership/lifecycle boundaries:** `NodeId = usize` arena index, recycled after `destroy_node` (`scene.rs:121,175-177`). `CompositionNode` embeds a *full second `Director`* with its own surface cache and LayoutEngine (`node/composition.rs:17-22,76-91`). No stable IDs beyond the recyclable index.

---

# Competitive Capability Matrix

| Capability | Director today | Remotion-style expectation | Hyperframes-style expectation | Gap severity | Notes |
|---|---|---|---|---|---|
| Declarative scene composition | Rhai imperative builder; JSON schema is one-way | `<Series>`/`<Composition>` components, props, parameterized | Node/visual graph, reusable blocks | **High** | No components/props; `director-schema` drifts (`pipeline/lib.rs:120-129`) |
| Stable/queryable node IDs | `usize` arena, recycled on destroy (`scene.rs:121`) | Stable refs, ids | Node ids | **Critical** | No `id()` on handles (`audio.rs:220` only) |
| Scene graph introspection | None in API | `getBoundingBox`, tree walk | Inspector | **Critical** | No `get_bounds`/`list_children`/`get_duration` anywhere in `scripting/*` |
| Project (de)serialization | Rhai-only; JSON one-way, no round-trip | Project JSON/serializable | Project state | **Critical** | No `save/load_project`; `Director` not `Serialize` |
| Keyframe interpolation | Linear + presets; **no bezier, no hold** (`animation.rs:23-40`) | `interpolate()`, cubic-bezier, hold | Tweening | **High** | Bezier/hold exist only in Lottie import (`lottie-core/src/animatable.rs:438`) |
| Easing coverage | Linear/Ease/Back/Bounce/Elastic | + expo/circ/sin + custom bezier | Rich | Medium | `EasingType` closed enum |
| Loops: reverse/yoyo/repeat | path-loop only; scene `add_loop` | `loop/reverse/yoyo` | Yes | **High** | No primitive (`grep` confirms) |
| Expressions / reactive | None native; audio FFT only | `useCurrentFrame`, derived | Reactive | **High** | Lottie expressions import-only, feature-gated |
| Animation errors | Silent no-op on bad name (animation.rs:74-82) | Throws/validates | Errors | **High** | Agent cannot detect ignored anim |
| FPS / time accuracy | `fps: u32` (`director.rs:76`) | fractional 23.976 | fps-agnostic | Medium | 23.976/29.97 impossible |
| Layout: flexbox/percentage/grid | Yes via Taffy | Full CSS | Full | Low | `flex_wrap` missing in scripting (`utils.rs` lacks it) |
| Responsive presets | Tokens only, fixed px, not auto-applied (`tokens.rs:84-189`) | applySafeArea, presets | Presets | Medium | No 1:1 preset; not resolution-scaled |
| Text: bidi/RTL | **Forced LTR** (`text.rs:175`,`caption.rs:345`) | Full bidi | Yes | **High** | RTL/emoji direction broken |
| Text: font fallback determinism | Only Roboto-Regular bundled (`director.rs:168-171`) | Bundled fonts | Yes | **High** | Bold/non-Latin → host fonts, non-deterministic |
| Caption robustness | Strong contract (`AGENTS.md`) but estimated wrap height (`caption.rs:781`) | Word highlight, karaoke | Yes | Medium | 0.55 magic constant vs real wrap can clip/overlap |
| Text-fit determinism | Shrink binary-search mutates `current_value` (`text.rs:703,721`) | fit modes | Yes | Medium | Fights font-size animation |
| Blend modes / masks | SrcOver+DstIn matte only (`renderer.rs:140-151`) | All blend + luma matte + multi-mask | Full | Medium | No luma matte/inv/multi |
| Custom shaders | SkSL RuntimeShader (`effect.rs:41-44`) | Shader props | Yes | Medium | UV origin bug (`effect.rs:932`); uniform errors ignored (`effect.rs:365`) |
| Color management | sRGB premul everywhere; no linear | Color-managed | Yes | Medium | No color-range tags on FFmpeg (`video_wrapper.rs:402`) |
| Motion blur | Accumulates `1/(s+1)` weights (`renderer.rs:275`) | Correct | Yes | **High** | Unnormalized → over-bright/over-opaque |
| Static-layer caching | Per-frame re-render; only comp/lottie/vector cache (`renderer.rs:43`) | Layer cache | Yes | Medium | No "is_static" short-circuit |
| Media: decode/seek/VFR/alpha | Basic video node | Frame-accurate, VFR, alpha | Yes | Medium | No VFR/seek-precision audit; alpha video untested |
| Audio: mixing/fades/duck | FFT reactivity + basic (`audio.rs`) | Full mixing, envelopes | Yes | Medium | No ducking/envelopes/trim/rate |
| Export formats | H.264 / alpha VP9 only (`video_wrapper.rs`) | MP4/WebM/ProRes/PNG seq/GIF | Multiple | Medium | No ProRes/image-seq/GIF; QP/CRF fixed |
| Deterministic seeds | `thread_rng` unseedable (`lifecycle.rs:28-45`) | Seeded | Yes | Medium | Non-reproducible across runs |
| Visual regression | Per-pixel golden (`visual/mod.rs`) | Snapshot + semantic | Yes | Medium | No OS suffix; font-fragile; CI off |
| Machine-readable errors / manifest | Strings; no manifest | JSON errors/manifest | Yes | **High** | No render manifest; thin `RenderError` (`errors.rs`) |
| Script expressiveness safety | `set_max_expr_depths(0,0)` (`mod.rs:53`) | Sandbox | Yes | Medium | Runaway-script risk for agents |

---

# Findings

## 1. No introspection API for agents
**Severity:** Critical
**Evidence:** `scripting/*` has no `get_bounds`, `list_children`, `get_scene_duration`, node `id()`, or `media_metadata`. `registry.rs` capability functions are Rust-only, never registered into Rhai (`registry.rs:430-475`). Only `AudioTrackHandle::id()` exists (`audio.rs:220`).
**Why it matters in real videos:** You cannot detect clipped text, overlapping captions, or hidden elements without bounds.
**Why it matters for agents:** An agent editing blindly cannot verify or fix its own output. This is the single largest blocker to the stated goal.
**Recommended direction:** Add `NodeHandle::id()`, `get_bounds()`, `list_children()`, `get_scene_duration()`, `media_metadata()` to Rhai; register `registry.rs` capabilities into the engine.
**Complexity:** Medium (public API + scripting bindings).

## 2. No live project serialization / schema drift
**Severity:** Critical
**Evidence:** No `save_project`/`load_project`/`to_ron` (`director-core/src`); `Director` not `Serialize` (`director.rs:59`). `director-schema` is one-way (`pipeline/lib.rs:29-117`), `MovieRequest` has no `version`, `TransitionType` schema has 6 variants vs ~20 engine variants (`pipeline/lib.rs:120-129`), and SKSL `apply_effect`/`add_loop`/`add_freeze` are not representable.
**Why it matters:** Agents need stable, re-loadable project state to iterate; humans need portable projects.
**Why it matters for agents:** Without reload + diffing, the "render → inspect → fix" loop is impossible in-process.
**Recommended direction:** Serialize the live `Director` (RON/JSON) with a `version` field; generate the JSON schema *from* the Rhai API to stop drift; expand transition/effect coverage.
**Complexity:** Large (internal infra + public API).

## 3. Motion-blur weights are unnormalized
**Severity:** High (correctness bug)
**Evidence:** `systems/renderer.rs:275` `let weight = 1.0 / (s as f32 + 1.0);` and `export/video.rs:183`. For `samples=2` weights sum to 1.5; the premultiplied alpha also over-accumulates.
**Why it matters:** Any motion-blur render with `samples>1` is over-brightened and over-opaque — visible in real exports.
**Why it matters for agents:** Silent quality regression they can't see without pixel inspection.
**Recommended direction:** Normalize weights (e.g. average: `1/samples`, or trapezoidal). Apply to both render paths.
**Complexity:** Small.

## 4. SkSL effects assume node origin (0,0)
**Severity:** High (correctness bug)
**Evidence:** `effect.rs:932` sets `u_resolution = rect.width/height`, but `main(float2 pos)` receives absolute canvas coords; UV math in Liquid/Acid/Halftone/Ascii/Mirror/VHS/NeonEdge (`effect_shaders.rs:544,575,609,654,749,827,874`) divides `pos/u_resolution`. For any off-origin node, UVs are wrong.
**Why it matters:** Effects placed anywhere but top-left render incorrectly.
**Why it matters for agents:** They'll place effects arbitrarily and get broken output.
**Recommended direction:** Translate the subtree to origin before filtering, or pass `rect.left/top` and offset in-shader.
**Complexity:** Medium.

## 5. Silent animation no-ops and linear fallback
**Severity:** High
**Evidence:** `animate` returns silently on lock failure (`animation.rs:32-35`); unknown property → `_` arm ignores (`animation.rs:74-82,121-124`); bad easing string → `Linear` (`node/mod.rs:50-67`). No getter for keyframe state; `Animated::Debug` redacts `raw_keyframes` (`animation.rs:341-350`).
**Why it matters:** A mistyped property/easing degrades motion with zero signal — exactly the failure mode that breaks agent reliability.
**Recommended direction:** Fail loudly (return `Result`/emit error) on unknown prop/easing/lock; add validation mode.
**Complexity:** Medium (public API change to return errors).

## 6. Animation primitives: no bezier/hold/reverse/repeat/expressions
**Severity:** High
**Evidence:** `EasingType` (`animation.rs:23-40`) has no cubic-bezier or hold; `grep` shows no reverse/alternate/yoyo; no per-frame expression language (audio FFT + `noise2d` helpers only, `lifecycle.rs:28-76`); Lottie import *does* have bezier/hold/expressions but is a separate path (`lottie-core/src/animatable.rs`).
**Why it matters:** TikTok/Reels "bounce back" loops, hold frames, and audio-reactive text are staples; hand-coding them is brittle.
**Recommended direction:** Promote Lottie's bezier/hold into native `Animated`; add `EasingType::CubicBezier`; add `reverse/yoyo/loop` and a lightweight per-frame expression hook.
**Complexity:** Large (core animation rework, but reuse Lottie math).

## 7. Text forced LTR — bidi/RTL broken
**Severity:** High
**Evidence:** `text.rs:175` `TextDirection::LTR`; `caption.rs:345` forces LTR. SkParagraph won't flip RTL runs.
**Why it matters:** Arabic/Hebrew/emoji-direction captions render incorrectly — a hard internationalization blocker.
**Recommended direction:** Respect paragraph direction; detect from text or expose `text_direction` prop.
**Complexity:** Small.

## 8. Non-deterministic fonts
**Severity:** High
**Evidence:** Only `Roboto-Regular.ttf` registered as a named family (`director.rs:168-171`). `font_weight:700`/italic/non-Latin fall back to host system fonts. Visual-regression CI even questions whether the font is committed (`visual_regression.yml:31-46`).
**Why it matters:** Bold text, CJK, emoji differ across machines → non-reproducible renders and unreliable golden tests.
**Recommended direction:** Bundle a small deterministic font set (Regular/Bold/Italic + emoji fallback) and pin them; assert presence at startup.
**Complexity:** Medium.

## 9. Caption height estimate can diverge from real wrap
**Severity:** Medium (high real-world impact)
**Evidence:** `CaptionNode::measure` estimates wrapped lines via `block_width / (font_size * 0.55)` (`caption.rs:781`); the real layout lays the full line through SkParagraph with internal wrap (`caption.rs:465-490`). Measure claims full canvas width while text draws at 86% (`layout_width_fraction`, `caption.rs:129`).
**Why it matters:** Underestimation drops captions below the bottom safe area / overlaps the next cue; overestimation wastes space. This is the most likely caption-clipping bug in production.
**Why it matters for agents:** Undetectable without bbox inspection.
**Recommended direction:** Compute caption height from the real laid-out Paragraph (reuse `build_block_layout` metrics) instead of the 0.55 guess; align measure width with `layout_width_fraction`.
**Complexity:** Medium.

## 10. `Shrink` text-fit mutates animated `current_value`
**Severity:** Medium
**Evidence:** `TextFit::Shrink` binary-searches font size and writes `default_font_size.current_value` directly (`text.rs:703,721`); `animate_property("font_size")` also writes `current_value` (`text.rs:871-876`). Coarse 5-iteration search, no "fit width only".
**Why it matters:** If both run, the fit search fights the animation; sizing jumps ±~3.75px.
**Why it matters for agents:** Unexplained size jitter.
**Recommended direction:** Separate fit-solved size from animation channel; add width-only fit; more iterations or analytic solve.
**Complexity:** Medium.

## 11. No color-range/matrix tags on FFmpeg export
**Severity:** Medium
**Evidence:** H.264 yuv420p (`video_wrapper.rs:402-403`) and alpha VP9 (`video_wrapper.rs:382-389`) encode without `-color_range`/`-colorspace`/`-color_primaries`/`-color_trc`. Source is full-range RGBA premul.
**Why it matters:** Likely washed-out / crushed blacks on playback (limited-range TV default mismatch).
**Why it matters for agents:** Unverifiable quality bug.
**Recommended direction:** Pass explicit color tags; expose a colorspace setting.
**Complexity:** Small.

## 12. `fps: u32` forbids 23.976 / 29.97
**Severity:** Medium
**Evidence:** `Director.fps: u32` (`director.rs:76`); used as `fps as f64` (`export/video.rs:91`).
**Why it matters:** YouTube/TikTok NTSC-accurate export is impossible; frame addressing requires manual `n/fps`.
**Recommended direction:** Use `f64` fps (or rational). Add `frame_to_time`/`time_to_frame` helpers.
**Complexity:** Small.

## 13. `apply_effect` / `set_mask` silently rewrite handles
**Severity:** Medium
**Evidence:** `apply_effect` wraps target in a new `EffectNode`, reparents children, and rewrites `timeline[].scene_root` (`utils.rs:29-89`); `set_mask` reparents the mask node (`properties.rs:119-135`). The returned/new handle id differs from input.
**Why it matters:** An agent holding the old handle silently operates on the wrong node.
**Recommended direction:** Document graph-rewriting semantics; return the new canonical handle and invalidate old ones; consider a non-mutating effect model.
**Complexity:** Medium.

## 14. `set_max_expr_depths(0, 0)` disables Rhai safety
**Severity:** Medium
**Evidence:** `scripting/mod.rs:53`.
**Why it matters:** An autonomous agent loop can hang/OOM the render process with no protection; no recursion/iteration limit.
**Why it matters for agents:** Direct reliability/safety risk in unattended operation.
**Recommended direction:** Make limits configurable with sane defaults; expose as a Director setting.
**Complexity:** Small.

## 15. Non-seedable RNG breaks determinism
**Severity:** Medium
**Evidence:** `rand_float`/`noise2d`/`noise3d` use `rand::thread_rng` (`lifecycle.rs:28-45`).
**Why it matters:** Same script yields different output across runs → unstable golden tests and unreproducible agent iterations.
**Recommended direction:** Thread a seed through `Director`; expose `set_seed`.
**Complexity:** Small.

## 16. No static-subtree caching
**Severity:** Medium
**Evidence:** `render_recursive` re-renders every node every frame (`renderer.rs:43`); only `CompositionNode`/`LottieNode`/`VectorNode` cache rasterized output, single-frame keyed (`composition.rs:108-197`, `lottie.rs:231-278`, `vector.rs:86-121`). No `is_animated` short-circuit.
**Why it matters:** Long timelines / many static layers in 4K vertical video do redundant work.
**Why it matters for agents:** Slow previews slow the iterate loop.
**Recommended direction:** Add an `is_static` flag per subtree → cache to a Surface, invalidate on mutation.
**Complexity:** Medium.

## 17. Masking / stacking contexts limited
**Severity:** Medium
**Evidence:** Only one mask node, `DstIn` alpha matte (`renderer.rs:140-151`); no luma matte, inverted mask, multi-mask, or mask-mode. Isolation happens only via `save_layer` triggered by blend/filter/mask (`renderer.rs:118-133`); no explicit `isolate` flag.
**Why it matters:** Common compositing (luma key, track matte, isolated blend groups) is impossible.
**Recommended direction:** Add mask-mode enum + luma matte + multi-mask; expose `isolate`/`stacking_context`.
**Complexity:** Medium.

## 18. CI does not run on push/PR
**Severity:** Medium
**Evidence:** `ci.yml:3-5` and `visual_regression.yml` use `workflow_dispatch` only. Real video path validated only on Linux (`ci.yml:42-48`); Windows/macOS use `mock_video`.
**Why it matters:** Regressions merge silently; visual tests are fragile across OS/font.
**Recommended direction:** Trigger on PR/push; validate real video on at least one OS; commit/pin fonts; add OS-suffixed snapshots or normalize rasterization.
**Complexity:** Small (CI config + asset pinning).

## 19. `add_freeze` is a misleading stub; `add_loop` drops audio
**Severity:** Low
**Evidence:** `lifecycle.rs:104-125` `add_freeze` just reuses last render state (comment admits no true freeze); `add_loop` (`lifecycle.rs:80-100`) copies `TimelineItem` with empty `audio_tracks`.
**Why it matters:** Agents expecting freeze/loop semantics get silent wrong behavior.
**Recommended direction:** Implement true freeze (`freeze_time` on renderer) or remove the API; copy audio bindings in loops.
**Complexity:** Medium.

## 20. No audio mixing depth / export ladder
**Severity:** Low
**Evidence:** Audio reactivity + basic bindings (`audio.rs`); no ducking, envelopes, trim, playback-rate, multi-track overlap semantics, loudness norm. Export: H.264/alpha VP9 only, fixed QP/CRF, no ProRes/PNG-seq/GIF (`video_wrapper.rs:377-430`); audio sample rate hardcoded 48000 with no mixer-rate validation (`video_wrapper.rs:101,494-500`).
**Why it matters:** Real social videos need music+voice ducking, fades, and platform-specific export ladders.
**Recommended direction:** Add an audio graph (gain/envelope/duck), and configurable export presets.
**Complexity:** Large.

---

# Prioritized Roadmap

## 1. Immediate hardening (make existing behavior reliable & observable)
- Fix motion-blur weight normalization (`renderer.rs:275`, `export/video.rs:183`) — Small, public-correctness.
- Fix SkSL UV origin (`effect.rs:932`) — Medium, internal+public.
- Make `animate`/easing/property errors loud — Medium, public API.
- Bundle & pin deterministic fonts; assert at startup (`director.rs:168`) — Medium, internal.
- Thread a seed through `Director` (`lifecycle.rs:28-45`) — Small, public API.
- Re-enable configurable Rhai expr-depth limits (`mod.rs:53`) — Small, internal.
- Add FFmpeg color-range tags (`video_wrapper.rs:402`) — Small, internal.
- Turn on CI for PR/push + pin fonts (`ci.yml`) — Small, CI.

## 2. v0.x capability upgrades (compete credibly)
- Native bezier + hold keyframes + `EasingType::CubicBezier`, reusing `lottie-core` math (`animation.rs`) — Large, internal+public.
- Reverse/yoyo/repeat/loop primitives + lightweight per-frame expression hook — Large, public API.
- Caption height from real Paragraph metrics (`caption.rs:781`) — Medium, internal.
- `Shrink` text-fit decoupled from animation channel — Medium, internal.
- bidi/`text_direction` support (`text.rs:175`) — Small, public.
- `flex_wrap` + `1:1` safe-area preset, resolution-scaled safe areas (`tokens.rs`, `utils.rs`) — Medium, public.
- Static-subtree caching (`renderer.rs:43`) — Medium, internal.
- Luma matte + multi-mask + `isolate` (`renderer.rs:118-151`) — Medium, public.

## 3. v1.0 architecture bets (settle before API freezes)
- **Introspection + manifest layer** (finding 1): `id()`, `get_bounds()`, `list_children()`, `get_scene_duration()`, `media_metadata()`, and a `render_manifest.json` (node tree, durations, assets, encoder, fps, per-node bbox). Public API + internal infra.
- **Live project (de)serialization with `version`**, schema generated *from* the Rhai API to kill drift (finding 2). Internal infra + public API.
- **Separate authoring API from render graph from runtime state**: introduce an immutable draft/scene model so `apply_effect`/`set_mask` (finding 13) don't silently rewrite handles. Architectural.
- **Deterministic time model**: `f64` fps, frame addressing, normalized motion-blur, seedable RNG as first-class. Internal+public.

## 4. Agent-first extensions (inspection, validation, preview, repair)
- Rhai-exposed `registry` capabilities + `inspect()` returning structured JSON.
- Rendered-frame capture + **contact-sheet/thumbnail** generator + **debug overlays** (bbox, safe-area, z-order) — CLI binary.
- **Visual/temporal diagnostics**: overlap/clip/empty-region/contrast checks producing machine-readable results.
- **OCR / text-visibility hook** interface (pluggable) to validate caption legibility automatically.
- **Git-style diff + repair suggestion** loop: compare two manifests/renders and emit targeted fix hints. Internal infra + CLI.

---

# Things Not To Build Yet

- **Full NLE timeline UI / non-linear editor** — premature; the scripted/JSON model is the right agent interface.
- **Node-based visual programming canvas** — Hyperframes-style graph editor is a product detour; invest in introspection first.
- **GPU-accelerated full-pipeline renderer replacement** — `wgpu_node` exists as a hook; rewriting the whole pipeline to GPU before CPU path is correct wastes effort.
- **ProRes / professional codec farm** — only needed once export quality is validated; current H.264/VP9 covers the agent loop.
- **Custom scripting language** — Rhai is fine; a TS/JSON API (generated from the same source as Rhai) is the higher-leverage move than a new language.
- **Audio separation / speech-to-text inside the engine** — keep external; the engine should consume transcribed caption data, not produce it.

---

# Proposed Target Architecture

Director's strengths (Rust + Skia + Taffy + Rhai, real layout, nested pre-comps, actual visual tests) are worth preserving. The gaps are *capability and observability*, not foundational. **No rewrite.** Recommended target:

1. **Two-layer authoring model.** Keep Rhai for human scripting, but make it a thin veneer over a **typed, serializable `SceneGraph` model** (`Composition` with `id: String`/UUID, `props: Map`, `duration: Option<f64>` derived from children). Generate the Rhai bindings *and* the JSON/`director-schema` *from this single model* so they can never drift. This closes findings 2, 13, and the duration-derivation gap at once.

2. **Parameterized, reusable components.** Introduce a `Component`/`Template` concept: a named scene factory taking typed `inputs` (props), producing a subgraph with stable internal ids. Agents instantiate `template("lower_third", #{name:"X"})` and later query/override by id. This is the missing Remotion equivalence and the key agent lever.

3. **Inspection + manifest as a first-class subsystem** (`director-inspect`), independent of rendering: a `SceneQuery` API returning structured JSON (bounds, durations, media metadata, z-order, safe-area violations) and a `RenderManifest` emitted on every export. This is what makes the agent loop possible (findings 1, 7-context).

4. **Animation core upgrade by reuse.** Merge `lottie-core`'s bezier/hold/spatial/temporal interpolation and expression evaluation into the native `Animated`/`EasingType` so the *same* robust math powers both import and authoring (finding 6). Add explicit loop/reverse/expression nodes on top.

5. **Deterministic, color-correct render core.** `f64` fps; seedable RNG; normalized motion blur; origin-correct shader UVs; explicit FFmpeg color tags; per-subtree static-cache with invalidation. Keep sRGB compositing (acceptable for short-form) but expose a linear option for correctness-critical effects.

6. **Text/responsive hardening.** Respect bidi; bundle a deterministic font set; derive caption height from real Paragraph metrics; decouple fit from animation; make safe-area presets resolution-relative and auto-applied.

7. **Agent safety envelope.** Configurable Rhai limits; loud errors; machine-readable `RenderError` codes; a `director` CLI with JSON stdout modes (`inspect`, `render --manifest`, `contact-sheet`, `diff`) so an external agent drives the engine via stable tool endpoints rather than parsing PNGs.

**Net effect:** Director keeps its engine, gains a *stable, introspectable, serializable scene contract* and an *agent-facing tool surface* — the two things that convert a good rendering library into an orchestratable video-creation platform. The highest-leverage single investment is the **serializable typed scene model that generates both the Rhai API and the JSON schema**, because it simultaneously fixes drift, enables introspection, enables iteration, and enables parameterized templates.
