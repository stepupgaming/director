# Plan 11: Transcription engine and competitive gaps

> **Status:** Spec. Plan 10 stays the active plan and holds current status, test counts and the next action. Part A may start beside plan 10 phase 4 because it lives in a new crate; Part B items marked for plan 10 stay there.
> **Inputs:** `pluggableasrengine.txt` (a proposed `director-asr` spec) and `fframesvsdirector.txt` (a Director and FFrames comparison), both at the repo root and both checked against the tree and public sources on 2026-10-25. Part C reads the raea repository and site on 2026-11-05.

## Why

- Talking-head and long-form editing needs word timings: captions from speech, text-based cuts, filler and silence removal, and b-roll snapped to words. Director's caption engine (`CaptionNode`, `CaptionWord`, SRT and VTT parsing, smart captions, the entity registry and active-speaker reframing) consumes word timings and produces none. HyperFrames ships `transcribe`; FFrames ships a WebVTT parser. Native, offline ASR makes the caption and reframing stack self-sufficient.
- The FFrames comparison mixes accurate gaps with claims the tree has since overtaken. This plan records the corrected scorecard and assigns each real gap to a plan 10 phase or a plan 11 phase.
- Product decision: the earlier non-goal "in-engine STT" (plan 00, archived audit) gives way to an optional `director-asr` crate. The render core keeps consuming word timings and gains no ASR dependency, matching the root rule that research and analysis inputs live outside the render core.

## Part A: `director-asr`

### Engine: the Bark 3.0 family through `bark-rs`

Decided 2026-11-07: Director transcribes with all four Underdog Bark 3.0 models through `bark-rs` (`C:\Projects\bark-rs`, the user's crate, published as `bark-asr` 0.1.0 with library `bark_asr`). Whisper and Parakeet v3 move to the backlog.

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

These belong in `bark-rs` itself. `bark-asr` 0.1.0 (commit `12da335`) ships token and word times (80 ms frames, the lead-in subtracted, punctuation joined to the word before it), any input rate, long audio cut at pauses into pieces of at most 30 s, Plus and Clean loading, and an int8 graph set. Still open there:

- **Pro.** Build initializers from BF16 safetensors. Pack the encoder into the exact 2-bit form when its weights hold few levels per row, and use the int8 graphs otherwise, so Pro stays under about 2 GB peak.
- **One environment per process.** `Bark::load` calls `ort::init().build()`, and ort records the first environment in a process-wide `OnceLock`, so a second load in one process fails with "only one environment is allowed per process", even after the first model drops. A long-lived host (MCP server, editor) can load one model per process until `bark-asr` builds its environment once and shares it across loads.
- **Token probabilities.** The joint's probability per token, for word confidence.

### Design

```rust
pub fn decode(path: &Path) -> Result<Vec<f32>, AsrError>;          // ffmpeg CLI, 16 kHz mono
pub fn to_mono_16k(audio: Audio) -> Result<Vec<f32>, AsrError>;    // bark-asr downmix and resampler

pub struct Word { pub text: String, pub start: f64, pub end: f64 }
pub struct Transcript { pub model: Model, pub words: Vec<Word> }
pub enum Model { Bark, BarkPlus, BarkClean, BarkPro }
pub enum Graphs { Exact, Int8 }

impl Engine {
    pub fn open(store: &Store, model: Model, graphs: Graphs, threads: usize) -> Result<Engine, AsrError>;
    pub fn transcribe(&mut self, samples: &[f32], progress: &mut dyn FnMut(f64, f64), cancel: &AtomicBool)
        -> Result<Transcript, AsrError>;
}
```

- **Contracts:** `transcribe` blocks, reports seconds done and total after each piece and checks the cancel flag before each one; async callers wrap it in `spawn_blocking`. Times are `f64` seconds of the source media, matching `CaptionWord::start_sec`. Models a build cannot run return `AsrError::Unsupported`.
- **Store:** each model's weight file pinned by Hugging Face revision, size and SHA-256, and each graph set's five files pinned by `bark-rs` commit and hash and fetched from GitHub raw. `import` installs a file already on disk after the same checks, as a hard link on one volume.
- **Features:** `director-cli` links `director-asr` only with its opt-in `asr` feature (decided 2026-11-07); a default build leaves out ONNX Runtime (138.9 MB against 162.6 MB on Windows) and keeps a stub that names the feature. Core and render stay ASR-free. License texts and notices for the models, graphs and ONNX Runtime live in `crates/director-asr/licenses/`; a license review comes later. CUDA and DirectML execution providers wait on `bark-asr`.
- **Long media:** `bark-asr`'s `segments` cuts audio longer than the piece limit (30 s) at the quietest smoothed 10 ms frame between a third of the limit and the limit, so no word is split and no stitching is needed. Shorter pieces read with less context: at 6 s the 20 s fixture keeps every word and loses casing and punctuation in its later pieces.
- **Boundary refinement:** move each word start and end to the nearest low-energy point within half an encoder frame, using 10 ms RMS on the 16 kHz signal. Report timing error against forced alignment before and after.
- **Disfluencies:** transcription runs verbatim, since a word missing from the output cannot be cut from the video. The disfluency pass aligns the verbatim words with a Clean transcript of the same audio and flags each word Clean omitted.
- **Scene document:** `director-scene` gains a transcript asset (words in source-media seconds, model id) attached to an audio or video asset. Caption nodes and edit operations reference it, so a project re-renders and re-edits without running ASR again.
- **Edit operations** through `director-host`: `captions_from_transcript`, `cut_words`, `remove_disfluencies`, `remove_silences(min_gap)` and `tighten_pauses(max_gap)`. Cuts split `TrackClip`s with source in and out points, snap to frame times through the timeline helpers, and map caption words from source time to timeline time through clip speed and trims.
- **CLI:** `director-engine asr models`, `asr fetch <model> [--graphs] [--from] [--graphs-from]`, `asr transcribe <media> [--model] [--graphs] [--format json|srt|vtt|txt] [-o]`, and later `asr captions <project> --asset <id>` writing the transcript asset and caption nodes. Exit codes follow the CLI contract.

### Phases

**A1. Crate, audio and Bark 3.0.** `director-asr` with the types, audio input, the model store and Bark 3.0 through `bark-asr` with word timings; `asr models`, `asr fetch` and `asr transcribe` with JSON, SRT and VTT output.
Gate: model-free tests cover audio and the store; an `#[ignore]` test with an explicit run command transcribes a fixture, and the same audio cut into several pieces keeps every word once and in order.
Status 2026-11-07: landed. Bark, Plus and Clean transcribe on the exact and int8 graphs; Pro returns `Unsupported`. Matching the official transcript stays with `bark-rs`, which checks it on 13 clips.

**A2. Pro and providers.** Pro loads and transcribes once `bark-asr` reads safetensors; CUDA and DirectML providers.
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

1. Model delivery: explicit `asr fetch` from Hugging Face into a user cache (recommended, and the weights stay under their own CC BY-NC terms), or weights bundled in release archives.

## Verification

- `cargo test -p director-asr --release` for model-free tests (audio, store, engine errors, subtitles).
- `DIRECTOR_ASR_SPEECH=<speech file> DIRECTOR_ASR_MODEL=<model> DIRECTOR_ASR_GRAPHS=<exact|int8> cargo test -p director-asr --release --test integration engine:: -- --include-ignored`, once per model, with the model in the store.
- While plan 10 is active, plan 11 checks record their counts and measurements in the plan 10 measurement table.
