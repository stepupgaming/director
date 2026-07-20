# Purpose

- Own command-line binaries for rendering Director scripts and reviewing visual outputs.

## Ownership

- `src/main.rs` owns the `director-engine` CLI: subcommands `render`, `still`, `compositions`, `preview`, plus legacy bare `SCRIPT [OUTPUT]`.
- `src/preview/` owns the Axum live preview server and embedded HTML player.
- `src/run.rs` owns shared script eval + composition/movie resolution (`eval_script`, `resolve_movie`).
- `src/gpu.rs` owns optional Vulkan context creation.
- `src/bin/preview_frame.rs` is a **deprecated** still-frame shim; prefer `director-engine still`.
- `src/bin/review_visuals.rs` owns the visual review helper binary.
- `src/bin/director_manifest.rs` owns agent-facing JSON manifest export (`api`, `schema`, `from-json`, `from-script`).
- `README.md` owns CLI usage and exit-code documentation.

## Local Contracts

- CLI behavior must remain consistent with `README.md`.
- User-facing errors should distinguish script failures (exit 1) from render failures (exit 2).
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
- `cargo run -p director-cli --bin director-engine -- --help`
- `cargo run -p director-cli --bin director-engine -- compositions examples/features/compositions.rhai --json`
- `cargo run -p director-cli --bin director-engine -- still examples/features/compositions.rhai --composition intro -t 1 -o output/intro.png`
- `cargo run -p director-cli --bin director-engine -- preview examples/features/compositions.rhai --composition intro --no-open`
- `cargo run -p director-cli --bin director-manifest -- from-script examples/foo.rhai`

## Child DOX Index

- No child AGENTS.md files currently required.
