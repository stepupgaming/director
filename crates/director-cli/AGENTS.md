# Purpose

- Own command-line binaries for rendering, inspecting and previewing Director inputs.

## Ownership

- `src/main.rs` owns the `director-engine` CLI: `render`, `batch`, `still`, `sheet`, `audio`, `check`, `snapshot`, `onion`, `inspect`, `project`, `compositions`, `preview`, `html`, plus legacy bare `SCRIPT [OUTPUT]`.
- `src/sheet.rs` owns the contact-sheet plan and PNG. `src/audio_report.rs` owns the mixer loudness report. `src/check.rs` owns canvas-edge findings. `src/snapshot.rs` owns golden PNG comparison. `src/onion.rs` owns the weighted frame blend.
- `src/batch.rs` owns batch output names and the status report. It stays free of other crate dependencies so `tests/repair_batch.rs` can include it.
- `src/input.rs` owns common loading for Rhai, HTML, captured DOM, portable native projects and versioned core projects, plus encoder selection.
- `src/project.rs` owns portable project packing, element-ID keyed JSON edits, and `project repair`.
- `src/renderer.rs` owns persistent native frame targets, a `director_render::FrameState` per renderer, synchronized PNG readback, the import registry (core plus `director-html` kinds) and `export_video`, the one video export entry for `render`, `batch`, `html render` and `html benchmark --export`.
- `src/html.rs` owns `html check|still|render|capture|benchmark`. HTML and capture JSON compile through `director-html`; browser extraction is explicit and is never invoked by native render/still.
- `src/preview/` owns the preview API server (Axum endpoints and the native worker) for editors and agents; its child DOX owns thread lifetime, accepted revisions, reload and audio caching. The repo ships no browser client.
- `src/run.rs` owns shared script eval + composition/movie resolution (`eval_script`, `resolve_movie`).
- `src/gpu.rs` owns Vulkan context creation (`director_render::GpuContext`), explicit HTML GPU requests, native render-surface creation and synchronous RGBA readback.
- `src/bin/director_manifest.rs` owns agent-facing JSON manifest export (`api`, `schema`, `from-json`, `from-script`).
- `README.md` owns CLI usage and exit-code documentation.

## Local Contracts

- CLI behavior must remain consistent with `README.md`.
- User-facing errors should distinguish script failures (exit 1) from render failures (exit 2).
- HTML compile/extraction failures exit 1; native paint/export/output failures exit 2. Capture JSON declares its FPS and sample rate; stored metadata controls playback.
- `sheet` prints one JSON object and writes one PNG. Frames are `i * timeline_duration / n`, painted at full size, then scaled into the cell. Frame count is capped at 64 and the raster at 16777216 pixels. Load or a rejected plan exits 1; paint or write failure exits 2.
- `audio` prints `{"tracks":[...]}` with integrated LUFS, true peak, loudness range, `clipped_samples`, and `silent`. Non-finite levels are JSON null. A non-finite sample or more than 57600000 samples sets `error` and skips analysis. It exits 0 when the report prints, including silence or clipping, and 1 on load failure.
- `check` samples the timeline, reports `off_canvas` (error), `clipped` (warning), and `missing_font` (error), and exits 2 when `--fail-on` matches. `--safe-zone <preset>` warns with `safe_zone` when `text`, `caption`, or `smart_caption` enters that inset; other nodes stay on the canvas check. An unknown preset exits 1 with no JSON. Without the flag there is no inset. Zero-size and non-finite bounds are ignored.
- `snapshot --golden <png>` compares one frame. A channel delta above 16 counts. A difference above 0.001 exits 2 and writes `<stem>.actual.png` and a red `<stem>.diff.png`. `--update` replaces the golden and exits 0. Load and bad arguments exit 1.
- `onion -n N` blends frames with weights 1 through N, capped at 8, and prints JSON.
- `render --scale` accepts a finite value in `(0, 1]`. Layout width and height stay put, and the raster rounds each side down to an even size. Scale 1 is unchanged, including odd sizes. `--draft-encode` sets software x264 to ultrafast and CRF 30 without changing the raster size. Hardware qp and the alpha VP9 path stay put. The default stays slow and CRF 18.
- Top-level `render`, `batch`, `still`, `sheet`, `audio`, `check`, `snapshot`, `onion`, `inspect` and `preview` share input loading. `--props` belongs to Rhai composition builders; native project edits use `project edit --patch`. Rhai packing is outside the native authoring format.
- `project repair` uses the `project edit` patch, saves only after validation, then prints inspect JSON at t=0. A rejected repair leaves the saved file unchanged.
- Video export runs the pipelined `director_render::export_director`, which encodes in one FFmpeg pass while it paints on a CPU worker pool, or with `--gpu` on one session on the Vulkan context. Alpha, `--scale` and `--draft-encode` behave the same on both.
- `batch --input <path> --output <dir>` renders each input with the software encoder. One failure still reports the other items and exits with the highest item code. `batch --json` prints `{"exit_code","items"}` in place of the `ok`/`fail` lines and keeps that exit code.
- `inspect --sample <seconds>` may be repeated. With no sample, inspect prints one object at `--time`. With samples, it loads once and prints `{"samples":[{"time","inspection"}]}` in order. A non-finite or negative sample exits 1.
- Explicit `--gpu` requires Vulkan across top-level render/still/preview as well as HTML commands. Render accepts independent `--encoder software|nvenc`.
- Preview frames and audio use export mode and clocks. `/api/inspect` returns native layout plus authored IDs and sampled capture elements; bounds describe layout, not final composited pixels.
- HTML `render` and `benchmark` accept independent `--gpu` and `--encoder software|nvenc`; `still` accepts `--gpu`. An explicit GPU request requires the Vulkan build and a GPU surface; failures exit 2. Default HTML rendering/encoding remains CPU/software.
- HTML benchmark FPS includes layout, paint, submission and RGBA readback on both CPU and GPU. Report the backend, codec and setup cost; PNG encoding remains outside the frame timer.
- CLI changes that expose or require Rhai API behavior must stay aligned with `director-core`.
- Rendering, stills, inspection and the native preview worker load Rhai with `Export` semantics so media seek behavior matches export. Composition listing uses `Preview`.
- Named compositions: host keeps `CompositionRegistry` + compiled `AST`; builders are invoked via `FnPtr::call`, never `call_fn` by name.
- Props merge is shallow (document in README). JSON object only.
- Legacy scripts that return a `Movie` still work when no compositions are registered.
- `args_conflicts_with_subcommands` keeps bare positionals from colliding with subcommands; a script file named `render.rhai` needs the explicit `render` subcommand form.

## Work Guidance

- Keep argument parsing explicit and documented.
- Prefer package-scoped checks during CLI work.
- Route props JSON through `director_core::scripting::props_from_*` helpers; do not reimplement JSON→Map in the CLI.

## Verification

- `cargo test -p director-cli`
- `cargo test -p director-cli --features vulkan --bin director-engine gpu_readback -- --ignored` requires a live Vulkan device.
- `tests/html_export.rs` verifies real FFmpeg/ffprobe frame count and audio/video/container duration alignment.
- `tests/project_workflow.rs` verifies pack → remove sources → inspect/edit/still/export, including atomic rejection and native command parity.
- `tests/repair_batch.rs` verifies a rejected repair does not write, and the batch report for one failure plus one success.
- `cargo test -p director-cli --features vulkan --bin director-engine gpu_preview_worker -- --ignored` verifies live GPU ownership with concurrent requests and shutter sampling.
- `cargo run -p director-cli --bin director-engine -- --help`
- `cargo run -p director-cli --bin director-engine -- compositions examples/features/compositions.rhai --json`
- `cargo run -p director-cli --bin director-engine -- still examples/features/compositions.rhai --composition intro -t 1 -o output/intro.png`
- `cargo run -p director-cli --bin director-engine -- preview examples/features/compositions.rhai --composition intro`
- `cargo run -p director-cli --bin director-manifest -- from-script examples/foo.rhai`

## Child DOX Index

- `src/preview/AGENTS.md` — native worker ownership, endpoint contracts, reload, audio and backend verification.
