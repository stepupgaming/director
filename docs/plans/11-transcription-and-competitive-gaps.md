# Plan 11: Transcription engine and competitive gaps

> **Status:** Spec. Plan 10 stays the active plan and holds current status, test counts and the next action. Part A may start beside plan 10 phase 4 because it lives in a new crate; Part B items marked for plan 10 stay there.
> **Inputs:** `pluggableasrengine.txt` (a proposed `director-asr` spec) and `fframesvsdirector.txt` (a Director and FFrames comparison), both at the repo root and both checked against the tree and public sources on 2026-10-25. Part C reads the raea repository and site on 2026-11-05.

## Why

- Talking-head and long-form editing needs word timings: captions from speech, text-based cuts, filler and silence removal, and b-roll snapped to words. Director's caption engine (`CaptionNode`, `CaptionWord`, SRT and VTT parsing, smart captions, the entity registry and active-speaker reframing) consumes word timings and produces none. HyperFrames ships `transcribe`; FFrames ships a WebVTT parser. Native, offline ASR makes the caption and reframing stack self-sufficient.
- The FFrames comparison mixes accurate gaps with claims the tree has since overtaken. This plan records the corrected scorecard and assigns each real gap to a plan 10 phase or a plan 11 phase.
- Product decision: the earlier non-goal "in-engine STT" (plan 00, archived audit) gives way to an optional `director-asr` crate. The render core keeps consuming word timings and gains no ASR dependency, matching the root rule that research and analysis inputs live outside the render core.

## Part A: `director-asr`

### Corrections to the proposed spec

| Proposed | Finding | Plan 11 design |
| --- | --- | --- |
| The trait lives in `director-core`, built into a `director` crate with `asr-whisper` on by default | The workspace has no `director` crate, and core ownership excludes analysis inputs. whisper.cpp and ONNX Runtime add native builds and large downloads to every workspace build and CI run. | A new `crates/director-asr` crate. `director-cli` gains an opt-in `asr` feature, like `vulkan`. Core and render stay ASR-free. |
| `async fn transcribe` through `async_trait` | Inference is CPU or GPU bound and blocking. The host session needs progress and cancellation, which async alone does not give. | A blocking `transcribe` that takes a progress callback and a cancel flag. Callers that need async wrap it in `spawn_blocking`. |
| `start_ms: u64`, `end_ms: u64` | The timeline runs on `f64` seconds (plan 01). Integer milliseconds round twice on the way to frames. | `start` and `end` in `f64` seconds of the source media, matching `CaptionWord::start_sec`. |
| `pcm_data: &[f32]` with no rate or channel contract | Every backend expects 16 kHz mono. | `Audio { samples, sample_rate }` mono input; the crate resamples and downmixes, and decodes media through `director_core::audio::load_audio_bytes`. |
| Parakeet TDT is English-focused, "sub-frame" timing | `parakeet-tdt-0.6b-v3` covers 25 European languages with automatic language detection. TDT timestamps come from 80 ms encoder frames, about 2.4 frames at 30 fps. | Parakeet v3 is a multilingual pack. Word boundaries get an optional sample-level refinement pass (below) measured against forced alignment before any frame-accuracy claim. |
| Bark is a separate engine on the `onnxruntime` 0.0.14 crate | Underdog Bark 3.0 is an English post-training of `parakeet-tdt-0.6b-v3` with the same blank id, window and decoder and joint contract. The `onnxruntime` crate is unmaintained. | One TDT transducer backend on `ort` runs both Parakeet v3 and Bark 3.0 as model packs. Bark's `<unk>` token (emitted for quotation marks) is dropped from text and timings and still fed to the decoder. |
| Bark "verbatim vs clean pipeline options" | Bark 3.0 Clean is a separate checkpoint that drops fillers, false starts and repeats from its output. A word missing from the output cannot be cut from the video. | Transcription always runs verbatim. A disfluency pass aligns the verbatim words with a Clean transcript of the same audio and flags each word Clean omitted. Edits then remove flagged words by time. |
| Whisper `large-v3` at 1.5 GB | `large-v3` has 1.5 B parameters; its f16 ggml file is about 3.1 GB. Quantized and `turbo` variants are smaller. Word timing comes from experimental DTW token timestamps. | Whisper is the broad-language pack (99 languages) through `whisper-rs`, with DTW timestamps on and the same refinement pass. |
| `keyword_biasing` on every backend | Whisper takes an initial prompt. Greedy TDT decoding has no biasing hook. | Each backend reports `Capabilities`. An option a backend lacks returns `AsrError::Unsupported`, in line with plan 10's fail-loud rule. |
| A factory `match` with a `_ =>` fallback arm | With every feature on, the fallback arm is unreachable and clippy `-D warnings` fails. | An always-compiled `Backend` enum, `available_backends()`, and an error that names the missing Cargo feature. |
| Model files left implicit | Downloads are hundreds of MB to GB, and licenses differ by pack. | A model store with pinned URLs, SHA-256 and license per pack. Downloads run only through an explicit `fetch`. Bark 3.0 base (CC-BY 4.0), Parakeet v3 (CC-BY 4.0) and Whisper (MIT) are first; Clean, Plus and Pro ship once their licenses are confirmed. |

### Design

```rust
pub struct Audio<'a> { pub samples: &'a [f32], pub sample_rate: u32 }

pub struct Word { pub text: String, pub start: f64, pub end: f64, pub confidence: f32, pub disfluent: bool }
pub struct Transcript { pub language: Option<String>, pub words: Vec<Word>, pub model: ModelId }

pub struct Options { pub language: Option<String>, pub prompt: Option<String>, pub refine_boundaries: bool, pub tag_disfluencies: bool }
pub struct Capabilities { pub languages: Languages, pub prompt: bool, pub word_timing: Timing }

pub trait Engine: Send {
    fn capabilities(&self) -> &Capabilities;
    fn transcribe(&mut self, audio: Audio, options: &Options, progress: &mut dyn FnMut(f32), cancel: &AtomicBool)
        -> Result<Transcript, AsrError>;
}

pub enum Backend { Whisper, Tdt }
pub fn available_backends() -> Vec<Backend>;
pub fn open(model: &ModelPack, device: Device) -> Result<Box<dyn Engine>, AsrError>;
```

- **Crate layout:** `src/lib.rs` (types, errors, `open`), `src/audio.rs` (decode, downmix, resample to 16 kHz), `src/chunk.rs` (voice-activity windows with overlap and word-level stitching for long media), `src/refine.rs` (boundary refinement), `src/disfluency.rs` (verbatim and Clean alignment), `src/store.rs` (model packs, cache directory, hashes, licenses), `src/whisper.rs` behind `whisper`, `src/tdt.rs` behind `tdt`.
- **Features:** `whisper` (`whisper-rs`), `tdt` (`ort` with prebuilt ONNX Runtime), `cuda` and `directml` execution providers for `tdt`, `vulkan` and `cuda` for whisper.cpp. Pin versions at implementation time; `whisper-rs-sys` needs libclang and CMake on Windows.
- **TDT backend:** encoder plus decoder and joint ONNX graphs, greedy TDT decoding with duration outputs, SentencePiece vocabulary from the pack. Bark 3.0 publishes transformers, MLX and Core ML weights, so the pack build exports ONNX with a `uv`-run script under `crates/director-asr/tools/`, and the exported graphs are compared against the transformers reference on a fixture before a pack is pinned.
- **Long media:** windows of up to 30 s for Whisper and a few minutes for TDT, cut at voice-activity gaps with overlap; stitching keeps the word from the window where it sits farther from the edge.
- **Boundary refinement:** move each word start and end to the nearest low-energy point within half an encoder frame, using 10 ms RMS on the 16 kHz signal. Report timing error against forced-alignment ground truth before and after.
- **Scene document:** `director-scene` gains a transcript asset (words in source-media seconds, model id, language) attached to an audio or video asset. Caption nodes and edit operations reference it, so a project re-renders and re-edits without running ASR again.
- **Edit operations** (through `director-host` once plan 10 phase 4 lands): `captions_from_transcript`, `cut_words`, `remove_disfluencies`, `remove_silences(min_gap)` and `tighten_pauses(max_gap)`. Cuts split `TrackClip`s with source in and out points, snap to frame times through the timeline helpers, and map caption words from source time to timeline time through clip speed and trims.
- **CLI:** `director-engine asr models`, `asr fetch <pack>` and `asr transcribe <media> --pack <pack> [--json] [--srt|--vtt]`, plus `asr captions <project> --asset <id>` writing the transcript asset and caption nodes. Exit codes follow the CLI contract.

### Phases

**A1. Crate, audio and Whisper.** `director-asr` with the types, audio input, chunking and the Whisper backend; `asr transcribe` with JSON, SRT and VTT output.
Gate: a CC-BY LibriSpeech fixture transcribes within a recorded WER, and model-dependent tests are `#[ignore]` with an explicit run command, like the live Vulkan tests.

**A2. TDT backend with Parakeet v3 and Bark 3.0.** The ONNX export script, both packs, `<unk>` handling and execution providers.
Gate: the exported graphs match the transformers reference transcript on the fixture, and the bench records WER, real-time factor and peak memory for every pack on CPU and GPU.

**A3. Timing quality and disfluencies.** Boundary refinement, the verbatim and Clean alignment, and a timing benchmark against forced alignment.
Gate: median word-boundary error is recorded per pack with and without refinement, and a fixture with planted fillers flags every filler and no content word.

**A4. Document and edit integration.** The transcript asset in `director-scene`, `captions_from_transcript`, and the cut and tighten operations through the host session.
Gate: removing fillers and long pauses from a talking-head fixture exports with audio and video in sync (framemd5 and sample checks at every cut), and captions follow the retimed words.

Later candidates: speaker diarization feeding the `Speaker` entities, streaming transcription for the preview session, and keyword boosting in a TDT beam search.

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
Status: the HTML source is `examples/html/benchmark/` and the HyperFrames side ran 2026-11-06 through `crates/director-html/tools/hyperframes-benchmark.mjs` (numbers in the plan 10 measurement table). The FFrames and raea sources are not written and neither tool is installed.

**B4. Shader authoring candidate.** A user-authored SkSL layer for HTML (`<canvas data-sksl>` or a CSS custom property) with a GLSL-to-SkSL path for common Shadertoy uniforms. Decide after plan 10 phase 5, since plan 10 freezes the Paper translator.

## Part C: raea comparison

Sources: `github.com/shinyobjectz/raea` at `925bc03` (created 2026-10-18, 198 stars, canary, FSL-1.1-ALv2) and `raea.io` (`/`, `/why/`, `/learn/`), read on 2026-11-05.

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
| Speed claim | Pong 1080p at 5.3 ms a frame and a platformer at 4.4 ms, M5 Pro with VideoToolbox H.264; no named competitor | B3 clip at 5.9 ms a frame end to end on Vulkan and NVENC (21.1 s for 3600 frames), 4.7 times HyperFrames; raea side not run | B3 raea side |

### Workstreams

**C1. Keep the scorecard true.** Re-read the raea README, `apps/check` docs and site whenever a plan 10 phase closes, as B1 does for FFrames.

**C2. Export QA checks.** The checks listed under plan 10 phase 6. Thresholds follow published platform guidance and the standards they cite (WCAG contrast, EBU R128 loudness) and are calibrated on Director's own fixtures.
Gate: each check has a fixture that fails it and a fixed variant that passes, through `Session::qa` and on a finished MP4. Status 2026-11-05: contrast, reading time, cues, voice against bed and dead stops pass the gate through `Session::qa`, and blotching passes it on a finished MP4 through `Session::qa`; project rules and the remaining checks on a finished MP4 remain.

**C3. Craft guides in the generated skills.** A quality floor an agent reads before each edit, a P0 to P3 rubric, and named edit moves mapping a person's note to one playbook. A brief and storyboard gate belongs at the orchestration layer outside the render core.

**C4. ffmpeg-style media commands candidate.** `director-engine media` accepting common ffmpeg and ffprobe command lines over Director's own decode and encode. Decide after the plan 10 phase 3 encoder path.

## Open decisions

1. ASR default: the opt-in `asr` CLI feature (recommended, keeps default builds lean), or ASR on by default in release binaries.
2. First backend: Whisper first for language reach (recommended), or TDT first for English quality per MB with Bark 3.0.
3. Model delivery: explicit `asr fetch` into a user cache (recommended), or packs bundled in release archives.

## Verification

- `cargo test -p director-asr --release` for model-free tests (audio, chunking, stitching, refinement, alignment, store).
- `cargo test -p director-asr --release --features whisper,tdt -- --ignored` with fetched packs for transcription, WER and timing tests.
- While plan 10 is active, plan 11 checks record their counts and measurements in the plan 10 measurement table.
