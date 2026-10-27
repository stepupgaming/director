# Purpose

- Own command-line binaries for rendering, inspecting and editing Director inputs.

## Ownership

- `src/main.rs` owns the `director-engine` CLI: `render`, `batch`, `still`, `sheet`, `audio`, `check`, `qa`, `snapshot`, `onion`, `inspect`, `outline`, `diff`, `editor`, `mcp`, `skills`, `project`, `compositions`, `html`, plus legacy bare `SCRIPT [OUTPUT]`.
- `src/sheet.rs` owns the contact-sheet plan and PNG. `src/audio_report.rs` owns the mixer loudness report. `src/check.rs` owns canvas-edge findings on the host `sample_times` grid. `src/qa.rs` owns the `qa` command over `Session::qa`. `src/snapshot.rs` owns golden PNG comparison. `src/onion.rs` owns the weighted frame blend. `src/outline.rs` owns `outline`, `diff` and the error envelope exit mapping. `src/editor.rs` owns the `editor` command over `director_host::Editor`. `src/mcp.rs` owns the stdio MCP server: JSON-RPC framing, protocol negotiation, the session table and tool schemas generated from the argument types. `src/skills.rs` owns the `director-mcp` skill generated from those schemas and the host error codes.
- `src/batch.rs` owns batch output names and the status report. It stays free of other crate dependencies so `tests/main.rs` can include it for `tests/repair_batch.rs`.
- `src/input.rs` owns the shared input flags (`Input`, converted to `director_host::Source`, which loads every input kind), `Input::open` into a `director_host::Session`, and encoder selection.
- `src/project.rs` owns portable project packing and `project edit|repair`, which apply element-ID keyed JSON edits through `director_host::Session::edit_project`.
- `src/renderer.rs` owns `use_gpu` (a required Vulkan context handed to a session), `paint` under the frame pixel limit, PNG encoding, and `export_video` (with `director_host::import_registry`) for `html render` and `html benchmark --export`.
- `src/html.rs` owns `html check|still|render|capture|benchmark|timeline`. `html timeline` prints the `director-timeline/1` tracks a compile attached, from timeline JSON and GSAP. HTML and capture JSON compile through `director-html`; browser extraction is explicit and is never invoked by native render/still.
- `src/run.rs` owns default output naming (`default_video_output`). Script evaluation and movie resolution live in `director_host::script`.
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
- `qa` prints `Session::qa` as one JSON object (`revision`, `score`, `pass`, `errors`, `warnings`, `samples`, `issues`, `audio`) with `--every`, `--safe-zone` and `--loudness-target` (default -14 LUFS). It exits 0 when the report passes and 2 when an issue is an error. Failures print the error envelope with the `outline` exit mapping.
- `snapshot --golden <png>` compares one frame. A channel delta above 16 counts. A difference above 0.001 exits 2 and writes `<stem>.actual.png` and a red `<stem>.diff.png`. `--update` replaces the golden and exits 0. Load and bad arguments exit 1.
- `outline` prints `Session::outline` as one JSON object. `diff BEFORE AFTER` opens both inputs with the same input flags and prints `director_host::diff`. Both print a failure as the `{"error":{"code","message"}}` envelope on stdout and exit 2 for `render`, `export`, `cancelled` and `missing_font`, else 1.
- `mcp` reads one JSON-RPC 2.0 message per line on stdin and writes one reply per request on stdout; logs stay on stderr. It negotiates protocol revisions 2025-11-25, 2025-06-18, 2025-03-26 and 2024-11-05, answering the newest when the client's is unknown, and serves tools only: `open` (session ids `s1`, `s2`, ...), `close`, `outline`, `node` (`Session::node`: a record or a JSON pointer inside it, with `leaves`), `diff` (two session ids), `patch` (the base revision defaults to the current one), `inspect`, `qa` (`every`, `safe_zone` and `loudness_target` default to the host options), `render_frame` (PNG image content, or a file with `output`) and `export`. Input schemas come from the argument types through `schemars` with subschemas inline, and unknown fields are rejected. A failed tool returns `isError` with the host envelope as text and structured content; unknown methods and tools are JSON-RPC errors -32601 and -32602. Requests run one at a time. Paths resolve against the server's working directory. `render_frame` and `export` outputs must resolve (lexically and through links in the existing parent) inside the working directory, the session's input directory or an `--allow-dir` root, else `output_outside_root`. A `tools/call` with `_meta.progressToken` on `qa` or `export` writes `notifications/progress` (`progress`, `total`) to stdout before the reply. Logs default to plain `warn` on stderr. `--gpu` gives each session and each export a required Vulkan context.
- `editor INPUT --frames RING` creates the frame ring file, prints `opened` and then one `director_host::Event` per JSON line on stdout, and reads one `director_host::Command` per JSON line on stdin. A line that does not parse takes the next generation and reports an `invalid_argument` error event. EOF finishes queued commands and exits 0; a failed open prints the error envelope with the `outline` exit mapping. `--gpu` paints on a required Vulkan context on the owner thread.
- `skills DIR` writes `DIR/director-mcp/SKILL.md` (setup, loop, one argument bullet per schema property with type, requirement and default, tagged variants such as patch ops, and error codes) and `DIR/director-mcp/tools.json` (the `tools/list` result), then prints the files as JSON. The checked-in copy in `.agent/skills/director-mcp/` must match the generator.
- `onion -n N` blends frames with weights 1 through N, capped at 8, and prints JSON.
- `render --scale` accepts a finite value in `(0, 1]`. Layout width and height stay put, and the raster rounds each side down to an even size. Scale 1 is unchanged, including odd sizes. `--draft-encode` sets software x264 to ultrafast and CRF 30 without changing the raster size. Hardware qp and the alpha VP9 path stay put. The default stays slow and CRF 18.
- Top-level `render`, `batch`, `still`, `sheet`, `audio`, `check`, `qa`, `snapshot`, `onion`, `inspect`, `outline`, `diff`, `editor`, `mcp` and `project edit|repair` open their input as a `director_host::Session`. Frames paint through `Session::render_frame`, on the session's Vulkan context under `--gpu`. Inspect JSON is `Session::inspect`. `--props` belongs to Rhai composition builders; native project edits use `project edit --patch`. Rhai packing is outside the native authoring format.
- `project repair` uses the `project edit` patch, saves only after validation, then prints inspect JSON at t=0. A rejected repair leaves the saved file unchanged.
- `render` and `batch` export through `Session::export`; `html render` and `html benchmark --export` through `export_video`. Both run the pipelined document export, which refuses missing required fonts and encodes in one FFmpeg pass while it paints on a CPU worker pool, or with `--gpu` on one session on the Vulkan context. Alpha, `--scale` and `--draft-encode` behave the same on both.
- `batch --input <path> --output <dir>` renders each input with the software encoder. One failure still reports the other items and exits with the highest item code. `batch --json` prints `{"exit_code","items"}` in place of the `ok`/`fail` lines and keeps that exit code.
- `inspect --sample <seconds>` may be repeated. With no sample, inspect prints one object at `--time`. With samples, it loads once and prints `{"samples":[{"time","inspection"}]}` in order. A non-finite or negative sample exits 1.
- Explicit `--gpu` requires Vulkan across top-level render/still/editor as well as HTML commands. Render accepts independent `--encoder software|nvenc`.
- HTML `render` and `benchmark` accept independent `--gpu` and `--encoder software|nvenc`; `still` accepts `--gpu`. An explicit GPU request requires the Vulkan build and a GPU surface; failures exit 2. Default HTML rendering/encoding remains CPU/software.
- HTML benchmark FPS includes layout, paint, submission and RGBA readback on both CPU and GPU. Report the backend, codec and setup cost; PNG encoding remains outside the frame timer.
- CLI changes that expose or require Rhai API behavior must stay aligned with `director-core`.
- Rendering, stills, inspection and the editor load Rhai with `Export` semantics so media seek behavior matches export. Composition listing uses `Preview`.
- Named compositions: host keeps `CompositionRegistry` + compiled `AST`; builders are invoked via `FnPtr::call`, never `call_fn` by name.
- Props merge is shallow (document in README). JSON object only.
- Legacy scripts that return a `Movie` still work when no compositions are registered.
- `args_conflicts_with_subcommands` keeps bare positionals from colliding with subcommands; a script file named `render.rhai` needs the explicit `render` subcommand form.

## Work Guidance

- Keep argument parsing explicit and documented.
- Prefer package-scoped checks during CLI work.
- Route props JSON through `director_core::scripting::props_from_*` helpers; do not reimplement JSON→Map in the CLI.

## Verification

- `cargo test -p director-cli --release`
- `cargo test -p director-cli --release --bin director-engine gpu_readback -- --ignored` requires a live Vulkan device.
- `tests/html_export.rs` verifies real FFmpeg/ffprobe frame count and audio/video/container duration alignment.
- `tests/project_workflow.rs` verifies pack → remove sources → inspect/edit/still/export, including atomic rejection and native command parity.
- `tests/outline_diff.rs` verifies outline and diff JSON and the load error envelope.
- `src/skills.rs` tests fail when `.agent/skills/director-mcp/` is stale and check that every tool argument and error code is documented.
- `tests/editor.rs` verifies seek, a rejected line and a patch repaint over the `editor` stdio protocol, reading frames from the ring file.
- `tests/mcp.rs` verifies that an agent opens, patches, inspects, scores, paints and exports a project over stdio MCP alone.
- `tests/repair_batch.rs` verifies a rejected repair does not write, and the batch report for one failure plus one success.
- `cargo run -p director-cli --bin director-engine -- --help`
- `cargo run -p director-cli --bin director-engine -- compositions examples/features/compositions.rhai --json`
- `cargo run -p director-cli --bin director-engine -- still examples/features/compositions.rhai --composition intro -t 1 -o output/intro.png`
- `cargo run -p director-cli --bin director-manifest -- from-script examples/foo.rhai`

## Child DOX Index

- None.
