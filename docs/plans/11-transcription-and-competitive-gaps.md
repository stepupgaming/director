# Plan 11: Transcription engine and competitive gaps

> **Status:** Spec. Plan 10 stays the active plan and holds current status, test counts and the next action. Part A may start beside plan 10 phase 4 because it lives in a new crate; Part B items marked for plan 10 stay there.
> **Inputs:** `pluggableasrengine.txt` (a proposed `director-asr` spec) and `fframesvsdirector.txt` (a Director and FFrames comparison), both at the repo root and both checked against the tree and public sources on 2026-10-25. Part C reads the raea repository and site on 2026-11-05.

## Why

- Talking-head and long-form editing needs word timings: captions from speech, text-based cuts, filler and silence removal, and b-roll snapped to words. Director's caption engine (`CaptionNode`, `CaptionWord`, SRT and VTT parsing, smart captions, the entity registry and active-speaker reframing) consumes word timings and produces none. HyperFrames ships `transcribe`; FFrames ships a WebVTT parser. Native, offline ASR makes the caption and reframing stack self-sufficient.
- The FFrames comparison mixes accurate gaps with claims the tree has since overtaken. This plan records the corrected scorecard and assigns each real gap to a plan 10 phase or a plan 11 phase.
- Product decision: the earlier non-goal "in-engine STT" (plan 00, archived audit) gives way to an optional `director-asr` crate. The render core keeps consuming word timings and gains no ASR dependency, matching the root rule that research and analysis inputs live outside the render core.

## Part A: `director-asr`

### Engine: the Bark 3.0 family through `bark-rs`

Decided 2026-11-07: Director transcribes with all four Underdog Bark 3.0 models through `bark-rs` (`C:\Projects\bark-rs`, the user's crate). Whisper and Parakeet v3 move to the backlog.

`bark-rs` runs Bark 3.0 on ONNX Runtime (pyke `ort` 2.0.0-rc.15) with no Python at run time. It reads `model.bark` (format `bark-levels-v3`) and builds the weights of three committed, weightless graphs (encoder, one LSTM decoder step, joint; 2.3 MB) at load. The default graphs pack each encoder weight into 2-bit ternary `MatMulNBits` matrices and stay bit-exact with fp32. Greedy TDT decoding follows `model.generate` after a 0.25 s silent lead-in. On the 67.2 s fixture the desktop CPU transcribes in 3.53 s at 1.43 GB peak, and the transcript matches the official model on 13 clips.

| Model | Hugging Face repo | Weights | Role | Vendor WER (7-set Open ASR average) |
| --- | --- | --- | --- | --- |
| Bark 3.0 | `ConwayResearch/Underdog-Bark-3.0` | `model.bark`, 163 MB | Verbatim default | 4.94 |
| Bark 3.0 Plus | `ConwayResearch/Underdog-Bark-3.0-Plus` | `model.bark`, 180 MB | Verbatim, more accurate | 4.76 |
| Bark 3.0 Clean | `ConwayResearch/Underdog-Bark-3.0-Clean` | `model.bark`, 180 MB | Drops fillers, false starts and repeats; built on Plus | 5.83 against verbatim references |
| Bark 3.0 Pro | `ConwayResearch/Underdog-Bark-3.0-Pro` | BF16 `model.safetensors`, about 1.25 GB | Most accurate | 4.29 |

- All four are 0.6 B English post-trainings of `nvidia/parakeet-tdt-0.6b-v3` with the same encoder, decoder and joint, so one graph set serves every model.
- All four are CC BY-NC 4.0. Director's code stays MIT OR Apache-2.0 and ships no weights. `asr fetch` prints the license, and commercial use of the weights needs a license from Conway (`license@conway.tech`, per each card).
- WER figures are the vendor's own, scored with the Open ASR Leaderboard code (cards read 2026-11-07).

### Work in `bark-rs`

These belong in `bark-rs` itself, and Director depends on them:

- **Token timings.** Record the encoder frame and duration of every emitted token (80 ms frames, the 0.25 s lead-in subtracted) and the joint's probability for it, then group SentencePiece pieces into words at `▁`. `<unk>` (quotation marks) stays out of text and timings and is still fed to the decoder.
- **Plus and Clean.** Load their `model.bark` files and match the official transcript on the fixture.
- **Pro.** Build initializers from BF16 safetensors. Pack the encoder into the exact 2-bit form when its weights hold few levels per row, and use the q8 graphs otherwise, so Pro stays under about 2 GB peak.
- **Publishing.** A crates.io release under a free name with `[lib] name = "bark"` and the graphs inside the package, as the lottie crates do. Until then `director-asr` takes a path dependency and stays out of `cargo publish`.

### Design

```rust
pub struct Audio<'a> { pub samples: &'a [f32], pub sample_rate: u32 }

pub struct Word { pub text: String, pub start: f64, pub end: f64, pub confidence: f32, pub disfluent: bool }
pub struct Transcript { pub words: Vec<Word>, pub model: Model }

pub struct Options { pub refine_boundaries: bool, pub tag_disfluencies: bool, pub graphs: Graphs }
pub enum Model { Bark, BarkPlus, BarkClean, BarkPro }
pub enum Graphs { Exact, Q8 { int8_activations: bool } }

pub fn open(model: Model, store: &Store, threads: usize) -> Result<Engine, AsrError>;
impl Engine {
    pub fn transcribe(&mut self, audio: Audio, options: &Options, progress: &mut dyn FnMut(f32), cancel: &AtomicBool)
        -> Result<Transcript, AsrError>;
}
```

- **Contracts:** `transcribe` blocks and takes a progress callback and a cancel flag; async callers wrap it in `spawn_blocking`. Times are `f64` seconds of the source media, matching `CaptionWord::start_sec`. Input is any rate and channel count; the crate downmixes and resamples to 16 kHz mono, and decodes media through `director_core::audio::load_audio_bytes`. Options a model lacks return `AsrError::Unsupported`.
- **Crate layout:** `src/lib.rs` (types, errors, `open`), `src/audio.rs` (decode, downmix, resample), `src/chunk.rs` (voice-activity windows with overlap and word stitching), `src/refine.rs` (boundary refinement), `src/disfluency.rs` (verbatim and Clean alignment), `src/store.rs` (model cache, pinned URLs, SHA-256, licenses).
- **Features:** `director-cli` gains an opt-in `asr` feature, like `vulkan`. Core and render stay ASR-free. `director-asr` exposes `cuda` and `directml` execution providers.
- **Long media:** windows of a few minutes cut at voice-activity gaps with overlap; stitching keeps each word from the window where it sits farther from the edge.
- **Boundary refinement:** move each word start and end to the nearest low-energy point within half an encoder frame, using 10 ms RMS on the 16 kHz signal. Report timing error against forced alignment before and after.
- **Disfluencies:** transcription runs verbatim, since a word missing from the output cannot be cut from the video. The disfluency pass aligns the verbatim words with a Clean transcript of the same audio and flags each word Clean omitted.
- **Scene document:** `director-scene` gains a transcript asset (words in source-media seconds, model id) attached to an audio or video asset. Caption nodes and edit operations reference it, so a project re-renders and re-edits without running ASR again.
- **Edit operations** through `director-host`: `captions_from_transcript`, `cut_words`, `remove_disfluencies`, `remove_silences(min_gap)` and `tighten_pauses(max_gap)`. Cuts split `TrackClip`s with source in and out points, snap to frame times through the timeline helpers, and map caption words from source time to timeline time through clip speed and trims.
- **CLI:** `director-engine asr models`, `asr fetch <model>`, `asr transcribe <media> --model <model> [--json] [--srt|--vtt]`, and `asr captions <project> --asset <id>` writing the transcript asset and caption nodes. Exit codes follow the CLI contract.

### Phases

**A1. Crate, audio and Bark 3.0.** `director-asr` with the types, audio input, windows for long media, the model store and Bark 3.0 through `bark-rs` with word timings; `asr models`, `asr fetch` and `asr transcribe` with JSON, SRT and VTT output.
Gate: model-free tests cover audio, windows, stitching and the store; an `#[ignore]` test with an explicit run command transcribes the 67.2 s fixture to the official transcript, and a long fixture across several windows keeps every word once and in order.

**A2. Plus, Clean and Pro.** All four models load and transcribe; exact and q8 graphs with optional int8 activations; CUDA and DirectML providers.
Gate: each model matches its official transcript on the fixture, and the bench records WER, real-time factor and peak memory for every model and graph set on CPU and GPU.

**A3. Timing quality and disfluencies.** Boundary refinement, the verbatim and Clean alignment, and a timing benchmark against forced alignment.
Gate: median word-boundary error is recorded per model with and without refinement, and a fixture with planted fillers flags every filler and no content word.

**A4. Document and edit integration.** The transcript asset in `director-scene`, `captions_from_transcript`, and the cut and tighten operations through the host session.
Gate: removing fillers and long pauses from a talking-head fixture exports with audio and video in sync (framemd5 and sample checks at every cut), and captions follow the retimed words.

Backlog: Whisper through `whisper-rs` for 99 languages (libclang and CMake on Windows, DTW word timing), Parakeet v3 for 25 European languages on the same graphs, speaker diarization feeding the `Speaker` entities, streaming transcription for the preview session, and keyword boosting in a TDT beam search.

## Part B: FFrames comparison

Sources: the FFrames README at `github.com/dmtrKovalenko/fframes` (3.0k stars, MIT, no tagged release shown) read on 2026-10-25, and this tree at commit `8bb2b8d`.

### Corrected scorecard

| Area | FFrames | Director today | Owner |
| --- | --- | --- | --- |
| Authoring | Rust returning SVG; edits recompile, with live reload in the editor | HTML/CSS (primary) and Rhai, no compile step, native HyperFrames-style HTML | Plan 10 phase 5 |
| Layout and text | Hand-placed SVG, SVG `<text>` | Taffy flexbox and grid, Skia paragraph text with spans, fit, per-glyph animators, strokes and shadows | Holds |
| Captions and speech | WebVTT parser and a subtitle example | Caption engine, smart captions, entity registry, active-speaker reframing; SRT and VTT input; no ASR | Plan 11 Part A |
| Rendering | Skia on the GPU by default (Metal, Vulkan), CPU fallback, author claims about 10x | Pipelined CPU pool export by default, Vulkan session opt-in; measured in the plan 10 table | Plan 10 phase 3 next options |
| Static reuse | Expression-free SVG parts hashed at compile time and drawn once | Static layer cache in `director-render`; `director-html` marks no node static, so HTML scenes repaint every node | Plan 10 phase 3 layer cache |
| Encoding | libav linked in process | One FFmpeg process per export, RGBA pipe, premixed audio over a loopback input in the same pass | Plan 10 phase 3 encoder path |
| Preview and editing | GPU window with sound and frame stepping; WASM browser editor with scrub timeline and live reload | Editor protocol with a shared-memory frame ring, cancelled seeks and audio windows, in-process or over stdio; no shipped client, and the editor is a future Tauri app | Tauri editor over `director_host::Editor` |
| Agent CLI | `inspect`, `strip`, `frame`, `onion`, `audio analyze`, `timeline`, `render --draft`, `--json`, `npx skills add` | `inspect --sample`, `check` with safe zones, `sheet`, `still`, `onion`, `audio`, `snapshot`, `render --scale --draft-encode`, `batch --json`, `project edit` and `repair`, `director-manifest` | Plan 10 phase 4 (MCP, generated skills, `outline`, `diff`) |
| Audio QA | LUFS, true peak, clipping, silence per scene, waveform image | `audio`: integrated LUFS, true peak, loudness range, clipped samples, silence per track; no normalization | Plan 10 phase 6 (LUFS normalization, ducking) |
| 3D and shaders | `fframes-three-d` crate; SkSL and pasted Shadertoy GLSL layers | CSS `preserve-3d` with depth sorting; built-in SkSL effects and transitions; Paper shader translator (frozen) | Plan 11 B4 candidate |
| Distribution | `cargo install cargo-fframes`, crates.io, prebuilt Skia and FFmpeg on macOS and Linux; Windows needs an FFmpeg shared build and LLVM | No release or binaries; Windows-first builds; Linux CI fails 12 text goldens | Plan 11 B2 |
| Speed claim | One clip: 128 s rendered in 36 s | HyperFrames head-to-head on the B3 clip: 30.4 s against 118.2 s on software x264, 21.1 s against 100.0 s on GPU paths; FFrames side not run | Plan 11 B3, FFrames side |

### Workstreams

**B1. Keep the scorecard true.** This table replaces `fframesvsdirector.txt`. Re-read the FFrames README and re-check each Director cell whenever a plan 10 phase closes, and update the Owner column as gaps close.

**B2. Distribution.** Tagged releases with `director-engine` binaries for Windows, Linux and macOS (CPU build plus a Vulkan build), FFmpeg detection with a clear install message, `cargo install` from crates.io once `director-*` crate names and the vendored skia-safe patch allow publishing, and Linux text goldens so CI is green on Linux.
Gate: a clean machine installs a release binary and renders the four HTML examples with no Rust toolchain.

**B3. Head-to-head protocol.** Define the comparison clip for plan 10 phase 6 against FFrames and raea: the same 1080p30 content authored in each (text, shapes, an image, a video layer and audio, about 120 s), each tool's fastest documented path on the same machine, wall time from source to finished MP4, plus output size and a VMAF or SSIM check against a shared reference. The run itself needs the user's explicit yes.
Gate: the protocol, both sources and the scripts are committed before any number is published.
Status: the HTML source is `examples/html/benchmark/` and the HyperFrames side ran 2026-11-06 through `crates/director-html/tools/hyperframes-benchmark.mjs` (numbers in the plan 10 measurement table). The raea source is the TSX port `examples/html/benchmark/raea/edit.tsx`, staged by `raea/stage.py`, and the raea side ran 2026-11-06 on raea 0.1.0-canary.2 for Windows (numbers in the plan 10 measurement table). That build needed four workarounds: its HTML path reads SDK files from a CI path and a missing `runtime/js/js/build.ts`, so the port is TSX; it misreads Windows font paths, so the fonts load as data URLs; usvg rejects a `deg` transform on `<svg>`, so the star turns inside a div; and it refuses footage with B-frames, so staging re-encodes `clip20.mp4` with `-bf 0`. The FFrames source is not written and FFrames is not installed.

**B4. Shader authoring candidate.** A user-authored SkSL layer for HTML (`<canvas data-sksl>` or a CSS custom property) with a GLSL-to-SkSL path for common Shadertoy uniforms. Decide after plan 10 phase 5, since plan 10 freezes the Paper translator.

## Part C: raea comparison

Sources: `github.com/shinyobjectz/raea` at `925bc03` (created 2026-10-18, 198 stars, canary, FSL-1.1-ALv2) and `raea.io` (`/`, `/why/`, `/learn/`), read on 2026-11-05; the repository at `5e1c192` (520 stars, Windows builds since 2026-11-05) and the 0.1.0-canary.2 Windows zip, run on 2026-11-06.

raea is a Rust engine on Bevy 0.20. Compositions are React/TSX on a Remotion-shaped API (`AbsoluteFill`, `useCurrentFrame`, `interpolate`, `Sequence`, `registerRoot`) run by JavaScriptCore on macOS and QuickJS elsewhere, with HTML laid out by patched Stylo, Blitz and Parley. One file renders to video, plays live as a game, or exports as a game. Its agent layer is an installable skill, craft guides printed by `raea guide`, and `raea check` on a finished export.

**License.** FSL-1.1-ALv2 lets anyone use raea free, including companies and commercial videos, and forbids a commercial product or service that competes with raea. Each version becomes Apache-2.0 two years after release. End users face fewer limits than under Remotion's company license; platforms, hosted renderers and editors cannot build on it. Director is MIT. Director is a competing use, so Director takes ideas and public platform facts from raea and copies no raea code, tests or text.

### Scorecard

| Area | raea | Director today | Owner |
| --- | --- | --- | --- |
| Authoring | React/TSX on a Remotion-shaped API, Tailwind, npm packages through its own installer | HTML/CSS (primary), GSAP translated to native tracks, Rhai | Holds; Remotion compatibility is out of scope |
| 3D | Bevy PBR, glTF, Gaussian splats, matchmove through COLMAP | `<model>`: Bevy PBR glTF with IBL, shadows, fog, sky, SSR and lens effects, Gaussian splats, the Hero path tracer, and matchmove solved in process by `director-track` | Plan 10 issue 9 |
| Export QA | `raea check`: loudness and true peak, dead stops, static stretches, dark blotching, voice against bed, clipped text, platform zones, reading time, caption contrast, cues on words, text over faces, taste rules | `Session::qa`: clipped text, safe zones, captions over faces, reframed subjects, static stretches, loudness, peak and clipping, caption contrast, reading time, cues on words, voice against bed, voiceless swells, dead stops and blotching in a finished export; `qa_video` runs the static and mix checks on a file without its project | C2, plan 10 phase 6 |
| Agent craft guides | A floor (verify, refuse, never), a P0 to P3 rubric with model-judged questions, named edit moves, brief and storyboard gate | Generated skills, MCP, QA score | C3 |
| Music timing | Song map with beats, downbeats, builds and drops | Beat grid as beat times, `director.beat` | Plan 10 issue 8 |
| Media CLI | `raea ffmpeg` and `raea ffprobe` read ffmpeg command lines in process | One FFmpeg process per export | C4 candidate |
| Speed claim | Pong 1080p at 5.3 ms a frame and a platformer at 4.4 ms, M5 Pro with VideoToolbox H.264; no named competitor | B3 clip end to end on one RTX 5060 Ti: Director 19.1 s on Vulkan and NVENC and 22.9 s on Vulkan and x264, raea 23.6 s on Vulkan and OpenH264, with matching SSIM against HyperFrames | Keep the lead as raea matures |

### Workstreams

**C1. Keep the scorecard true.** Re-read the raea README, `apps/check` docs and site whenever a plan 10 phase closes, as B1 does for FFrames.

**C2. Export QA checks.** The checks listed under plan 10 phase 6. Thresholds follow published platform guidance and the standards they cite (WCAG contrast, EBU R128 loudness) and are calibrated on Director's own fixtures.
Gate: each check has a fixture that fails it and a fixed variant that passes, through `Session::qa` and on a finished MP4. Status 2026-11-05: contrast, reading time, cues, voice against bed and dead stops pass the gate through `Session::qa`, and blotching passes it on a finished MP4 through `Session::qa`; project rules and the remaining checks on a finished MP4 remain.

**C3. Craft guides in the generated skills.** A quality floor an agent reads before each edit, a P0 to P3 rubric, and named edit moves mapping a person's note to one playbook. A brief and storyboard gate belongs at the orchestration layer outside the render core.

**C4. ffmpeg-style media commands candidate.** `director-engine media` accepting common ffmpeg and ffprobe command lines over Director's own decode and encode. Decide after the plan 10 phase 3 encoder path.

## Open decisions

1. ASR default: the opt-in `asr` CLI feature (recommended, keeps default builds lean), or ASR on by default in release binaries.
2. Model delivery: explicit `asr fetch` from Hugging Face into a user cache (recommended, and the weights stay under their own CC BY-NC terms), or weights bundled in release archives.
3. `bark-rs` on crates.io: publish it under its own name (recommended, it also serves the Android app), or move it into the workspace as a Director crate.

## Verification

- `cargo test -p director-asr --release` for model-free tests (audio, chunking, stitching, refinement, alignment, store).
- `cargo test -p director-asr --release -- --ignored` with fetched models for transcription, WER and timing tests.
- While plan 10 is active, plan 11 checks record their counts and measurements in the plan 10 measurement table.
