# Purpose

- Speech to text for Director: word-timed transcripts from the Underdog Bark 3.0 models through `bark-asr` (the published `bark-rs`), with SRT, VTT and JSON output. Plan 11 Part A tracks the work.

## Ownership

- `src/lib.rs` - `Word` (text, start, end), `Transcript` (model and words, `text()`) and `AsrError`.
- `src/audio.rs` - `SAMPLE_RATE` (16 kHz), `decode` through the `ffmpeg` CLI (first audio stream, mono, 16 kHz, f32), and `to_mono_16k` for samples already in memory, through `bark_asr::audio::to_model_input`.
- `src/store.rs` - `Model` (`bark-3.0`, `bark-3.0-plus`, `bark-3.0-clean`, `bark-3.0-pro`) with the Hugging Face repo, pinned revision, file, size and SHA-256 of each; `Graphs` (`exact`, `int8`), the five `bark-asr` graph files of each pinned by `BARK_RS_COMMIT` and hash; `Asset`, one pinned file; and `Store`, the cache at `DIRECTOR_ASR_CACHE` or `director/asr` under the platform cache directory. Models live at `<id>/<file>` and graphs at `bark-rs-<commit>/<graphs>/`. `fetch` downloads through the `curl` CLI with resume, `import` installs a file already on disk (a hard link on one volume, else a copy), and `verify`, `model` and `graphs` check size and hash.
- `src/engine.rs` - `Engine`: `open` verifies a model and graph set and loads them with `bark_asr::Bark::load`; `transcribe` cuts the samples with `bark_asr::audio::segments` (pieces of at most `set_max_piece`, default 30 s), runs each piece, shifts its word times onto the recording, moves punctuation that opens a piece onto the word before it, reports progress after each piece and checks the cancel flag before each one.
- `src/subtitle.rs` - `cues` groups words into captions by length, duration, pause and sentence end, and `srt` and `vtt` write them.

## Local Contracts

- The crate links no Skia and no render crate. It links ONNX Runtime through `bark-asr`. Decoding needs `ffmpeg` in PATH, and fetching needs `curl`.
- Times are `f64` seconds of the source media, the unit of `CaptionWord::start_sec`.
- Director ships no weights. All four models are CC BY-NC 4.0 at the pinned revisions; `Model::license` says so, and commercial use needs a license from Conway.
- A download is explicit (`Store::fetch`); nothing downloads as a side effect of transcribing.
- A pinned revision and hash change together, read from the Hugging Face API for that commit. Graph hashes are of the committed bytes at `BARK_RS_COMMIT`; a Windows checkout with `core.autocrlf` rewrites the JSON files, so import graphs from `git show` output or the `bark-asr` package.
- `bark-asr` 0.1.0 builds one ONNX Runtime environment per process: a second `Engine::open` in one process fails with an `Engine` error. Pro returns `Unsupported` until `bark-asr` loads safetensors.

## Work Guidance

- Do not edit `bark-rs` (`C:\Projects\bark-rs`); its owner adds model loading and timing work there.

## Verification

- `cargo test -p director-asr --release` runs the model-free tests: resampling timing and level, downmix, ffmpeg decode of synthetic lavfi media, the store's status, verify, import and hash, graph pins, engine errors, cue splitting and the SRT, VTT and JSON text.
- `DIRECTOR_ASR_SPEECH=<speech file> DIRECTOR_ASR_MODEL=<model> DIRECTOR_ASR_GRAPHS=<exact|int8> cargo test -p director-asr --release --test integration engine:: -- --include-ignored` transcribes with an installed model: ordered words inside the clip, the same words in 6 s pieces, progress and cancel. Run it once per model, since one process loads one model.

## Child DOX Index

- None.
