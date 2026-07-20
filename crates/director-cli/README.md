# director-cli

Command-line interface for rendering Rhai scripts to video and stills.

Binary name: **`director-engine`**.

## Commands

```bash
# List named compositions registered by a script
director-engine compositions examples/features/compositions.rhai
director-engine compositions examples/features/compositions.rhai --json

# Still frame (PNG)
director-engine still script.rhai -t 1.0 -o frame.png
director-engine still script.rhai --frame 30 --composition intro -o out.png
director-engine still script.rhai --composition intro --props '{"title":"X"}' -o out.png

# Full video render
director-engine render script.rhai -o out.mp4
director-engine render script.rhai --composition intro --props props.json -o out.mp4
director-engine render script.rhai --alpha --gpu --manifest manifest.json

# Legacy bare invocation (still supported)
director-engine script.rhai out.mp4
director-engine script.rhai --composition intro --props '{"title":"X"}'
```

### Script named `render.rhai`

Because `render` is a subcommand, a file literally named `render.rhai` must use the explicit form:

```bash
director-engine render render.rhai -o out.mp4
```

### Props

`--props` accepts either a path to a JSON **object** file or an inline JSON object string. Merge is **shallow** over the composition's `default_props` (top-level keys only).

### Defaults

- `render` without `-o` writes next to the script (`.mp4` / `.webm` with `--alpha`). If `--composition` is set, the filename gets `_<composition>` before the extension.
- `still` without `-o` writes a `frame_*.png` next to the script.
- `compositions` exits nonzero when the script registers none.

### Deprecated

`preview_frame` remains as a thin shim; prefer `director-engine still`.

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Script / composition / args error |
| 2 | Render error |


# Live preview (browser)

```bash
director-engine preview script.rhai
director-engine preview script.rhai --composition intro --props '{"title":"X"}'
director-engine preview script.rhai --bind 127.0.0.1:7788 --no-open
```

Opens a minimal player at `http://127.0.0.1:7788/` (scrub, play, reload).
Endpoints: `GET /api/meta`, `GET /api/frame?t=`, `POST /api/reload`.

## Installation

```bash
cargo build --release -p director-cli
# binary: target/release/director-engine
```
