# Purpose

- Speech to text for Director: word-timed transcripts from the four Underdog Bark 3.0 models, with SRT, VTT and JSON output. Plan 11 Part A tracks the work. The Bark engine plugs in through `bark-rs` once its word timings land there.

## Ownership

- `src/lib.rs` - `Word` (text, start, end, confidence, `disfluent`), `Transcript` (model and words, `text()`) and `AsrError`.
- `src/audio.rs` - `SAMPLE_RATE` (16 kHz), `decode` through the `ffmpeg` CLI (first audio stream, mono, 16 kHz, f32), and `to_mono_16k` and `resample` for samples already in memory, through rubato's `FftFixedInOut` with its output delay trimmed.
- `src/chunk.rs` - `windows` splits long audio into windows of at most `max_len` (60 s) that overlap by `overlap` (4 s), each cut at the quietest smoothed point in the last `search` (10 s) before the window's limit. `stitch` keeps each word whose midpoint falls in its window's keep range.
- `src/store.rs` - `Model` (`bark-3.0`, `bark-3.0-plus`, `bark-3.0-clean`, `bark-3.0-pro`) with the Hugging Face repo, pinned revision, file, size and SHA-256 of each, and `Store`, the cache at `DIRECTOR_ASR_CACHE` or `director/asr` under the platform cache directory. `fetch` downloads through the `curl` CLI with resume, and `verify` checks size and hash.
- `src/subtitle.rs` - `cues` groups words into captions by length, duration, pause and sentence end, and `srt` and `vtt` write them.

## Local Contracts

- The crate links no Skia and no render crate. Decoding needs `ffmpeg` in PATH, and fetching needs `curl`.
- Times are `f64` seconds of the source media, the unit of `CaptionWord::start_sec`.
- Director ships no weights. All four models are CC BY-NC 4.0; `Model::license` says so, and commercial use needs a license from Conway.
- A model download is explicit (`Store::fetch`); nothing downloads as a side effect of transcribing.
- A pinned revision and hash change together, read from the Hugging Face API for that commit.

## Work Guidance

- Do not edit `bark-rs` (`C:\Projects\bark-rs`); its owner adds word timings and model loading there.

## Verification

- `cargo test -p director-asr --release` runs the model-free tests: resampling timing and level, downmix, ffmpeg decode of synthetic lavfi media, windows cut in pauses, stitching, the store's status, verify and hash, cue splitting and the SRT, VTT and JSON text.

## Child DOX Index

- None.
