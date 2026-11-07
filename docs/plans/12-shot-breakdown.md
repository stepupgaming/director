# Plan 12: Shot breakdown

> **Status:** In progress. Plan 10 stays the active plan and holds test counts and its next action; this plan records its own phases and parity results.
> **Inputs:** GitHub issue 15 (video DNA) and rea-video (JuneYaooo/rea-video at c9359c4, Apache-2.0), read on 2026-11-06.

## Why

- Agents replicate a reference video by first measuring it: where the shots cut, how they join, how the camera moves, what text sits on screen, how the audio is built. Director's render side covers the second half; this plan covers the measuring half.
- rea-video does the measuring in Python with OpenCV and a set of model integrations. Its detector is careful and well tuned, so Director ports it to Rust rather than redesigning it, and keeps the Python version as the parity baseline.

## Scope

- Everything up to replication: measurements and descriptions of a reference video in `breakdown.json`. Replication (rea-video's `plan`, `remake` and `production` stages) stays out.
- Every measuring step runs locally. Steps that need a language or vision model may call a cloud LLM, and they live in callers of `director-breakdown`, which links no model runtime.
- rea-video's China-specific choices give way to global local ones: PaddleOCR (Chinese first) to a multilingual local OCR, FunASR and SenseVoice to plan 11's `director-asr` (Whisper, Parakeet v3), Qwen models to whichever cloud LLM the caller picks. Platform labels read TikTok, YouTube, Instagram and the other global platforms beside Douyin.
- Later items: EmbeddingGemma 2 embeddings of shots and transcripts for retrieval, and ASR and TTS through plan 11.

## Phases

### B1: Shots and provenance (done 2026-11-06)

- `crates/director-breakdown` ports rea-video's `shots.py` and `probe.py`: OpenCV-exact gray and HSV histograms, the cut, fade, dissolve, flash and beat rules, and the container signature scan with GB 45438 and TikTok AIGC labels. Farneback flow becomes an 8 px block matcher.
- `director-engine breakdown VIDEO -o DIR` writes `breakdown.json` (`director-breakdown/1`).
- The signature table adds global generators (Grok Imagine, Midjourney, Higgsfield, LTX, Hunyuan, Stable Video, Firefly), editors, renderers (HyperFrames, Director, Cinema 4D, Houdini, Unreal), screen recorders, platforms and FFmpeg, HandBrake and yt-dlp.

### B2: Picture per shot

- Camera move per shot (static, pan, tilt, zoom, handheld shake) from rea-video's `motion.py`, on the block matcher or `director-track`'s KLT tracks.
- Keyframes, palette and grade (luma, contrast, saturation, white balance) from `visual.py`.
- Faces and framing with a global ONNX face detector, and on-screen text regions with a multilingual local OCR, from `faces.py` and `overlay.py`.

### B3: Sound

- Loudness, speech and music segments from `audio.py`, the beat grid from `director-html`'s beat detector, stems from `stems.py`, and word timings from `director-asr`.

### B4: Descriptions

- Shot classes (live footage, AI video, avatar, 3D, code or template, screen recording) from `classify.py`'s rules, then a caller-side vision-language step for shot descriptions, with a cloud LLM allowed. EmbeddingGemma 2 embeddings follow.

## Parity

The clip set is 15 local clips from stykanimate-app, content-rewards, archive.org and the Director benchmark, listed in the session scratchpad and kept off the repo. rea-video baselines sit in `F:/director-spikes/rea-video/baseline`, Director's output in `F:/director-spikes/rea-video/director`.

| Measure | Result (2026-11-06) |
| --- | --- |
| Frames decoded | identical on all 15 clips (90 to 12391 frames) |
| Boundaries | 245 of rea-video's 247 found at the same frame (within 2) and of the same kind; no extra boundaries |
| Differences | styk-ref 24.1 s: rea-video's cut scores 0.301 against a 0.300 threshold, and Director reads a dissolve 2 frames earlier. yt-LjTND1eHBcg 300.2 s: one dissolve the block matcher reads as motion |
| Beats | equal on 13 clips, one extra on two |
| Wall time, all 15 clips | Director 12.6 s (whole process, probe included), rea-video 17.5 s (decode and detect) |
| Synthetic edits | a cut, a dissolve, a fade with one black frame and a held fade give the same boundaries in both, within 1 frame |

## Verification

- `cargo test -p director-breakdown --release`
- The parity run in `crates/director-breakdown/AGENTS.md`.
