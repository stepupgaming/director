# Purpose

- Own command-line binaries for rendering Director scripts and reviewing visual outputs.

## Ownership

- `src/main.rs` owns the `director-engine` CLI: subcommands `render`, `still`, `compositions`, `preview`, `html`, plus legacy bare `SCRIPT [OUTPUT]`.
- `src/html.rs` owns `html check|still|render|capture|benchmark`. HTML and capture JSON compile through `director-html`; browser extraction is explicit and is never invoked by native render/still.
- `src/preview/` owns the Axum live preview server and embedded HTML player.
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
- HTML `render` and `benchmark` accept independent `--gpu` and `--encoder software|nvenc`; `still` accepts `--gpu`. An explicit GPU request requires the Vulkan build and a GPU surface; failures exit 2. Default HTML rendering/encoding remains CPU/software.
- HTML benchmark FPS includes layout, paint, submission and RGBA readback on both CPU and GPU. Report the backend, codec and setup cost; PNG encoding remains outside the frame timer.
- CLI changes that expose or require Rhai API behavior must stay aligned with `director-core`.
- `director-engine` registers the Rhai API with `Export` as the default mode for `render` so 3-argument `new_director(width, height, fps)` scripts render export-ready video nodes. `still` / `compositions` use `Preview`.
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
- `cargo run -p director-cli --bin director-engine -- --help`
- `cargo run -p director-cli --bin director-engine -- compositions examples/features/compositions.rhai --json`
- `cargo run -p director-cli --bin director-engine -- still examples/features/compositions.rhai --composition intro -t 1 -o output/intro.png`
- `cargo run -p director-cli --bin director-engine -- preview examples/features/compositions.rhai --composition intro --no-open`
- `cargo run -p director-cli --bin director-manifest -- from-script examples/foo.rhai`

## Child DOX Index

- No child AGENTS.md files currently required.
