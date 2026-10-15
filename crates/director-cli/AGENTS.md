# Purpose

- Own command-line binaries for rendering Director scripts and reviewing visual outputs.

## Ownership

- `src/main.rs` owns the `director-engine` CLI: `render`, `still`, `inspect`, `project`, `compositions`, `preview`, `html`, plus legacy bare `SCRIPT [OUTPUT]`.
- `src/input.rs` owns common loading for Rhai, HTML, captured DOM, portable native projects and versioned core projects, plus encoder selection.
- `src/project.rs` owns portable project packing and element-ID keyed JSON edits.
- `src/renderer.rs` owns persistent native frame targets, shared core shutter sampling and synchronized PNG readback.
- `src/html.rs` owns `html check|still|render|capture|benchmark`. HTML and capture JSON compile through `director-html`; browser extraction is explicit and is never invoked by native render/still.
- `src/preview/` owns Axum endpoints and the native worker; its child DOX owns thread lifetime, accepted revisions, reload and audio caching. The existing player is an optional client.
- `src/run.rs` owns shared script eval + composition/movie resolution (`eval_script`, `resolve_movie`).
- `src/gpu.rs` owns Vulkan context creation, explicit HTML GPU requests, native render-surface creation and synchronous RGBA readback.
- `src/bin/preview_frame.rs` is a **deprecated** still-frame shim; prefer `director-engine still`.
- `src/bin/review_visuals.rs` owns the visual review helper binary.
- `src/bin/director_manifest.rs` owns agent-facing JSON manifest export (`api`, `schema`, `from-json`, `from-script`).
- `README.md` owns CLI usage and exit-code documentation.

## Local Contracts

- CLI behavior must remain consistent with `README.md`.
- User-facing errors should distinguish script failures (exit 1) from render failures (exit 2).
- HTML compile/extraction failures exit 1; native paint/export/output failures exit 2. Capture JSON declares its FPS and sample rate; stored metadata controls playback.
- Top-level `render`, `still`, `inspect` and `preview` share input loading. `--props` belongs to Rhai composition builders; native project edits use `project edit --patch`. Rhai packing is outside the native authoring format.
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
- `cargo test -p director-cli --features vulkan --bin director-engine gpu_preview_worker -- --ignored` verifies live GPU ownership with concurrent requests and shutter sampling.
- `cargo run -p director-cli --bin director-engine -- --help`
- `cargo run -p director-cli --bin director-engine -- compositions examples/features/compositions.rhai --json`
- `cargo run -p director-cli --bin director-engine -- still examples/features/compositions.rhai --composition intro -t 1 -o output/intro.png`
- `cargo run -p director-cli --bin director-engine -- preview examples/features/compositions.rhai --composition intro --no-open`
- `cargo run -p director-cli --bin director-manifest -- from-script examples/foo.rhai`

## Child DOX Index

- `src/preview/AGENTS.md` — native worker ownership, endpoint contracts, reload, audio and backend verification.
