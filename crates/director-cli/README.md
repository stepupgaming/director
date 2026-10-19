# director-cli

Native CLI and backend for Rhai, HTML/CSS/HyperFrames, sampled captures and portable projects.

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

# Several inputs, software encoder, one directory
director-engine batch --input a.rhai --input b.html --output target/batch

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

## Native HTML commands

```bash
director-engine html check examples/html/native-motion.html
director-engine html still examples/html/native-motion.html -t 1 -o target/native.png
director-engine html render examples/html/native-motion.html -o target/native.mp4
director-engine html benchmark examples/html/native-motion.html --export target/bench.mp4
director-engine html capture composition.html -o target/scene.json --chrome /path/to/chrome
director-engine html render target/scene.json -o target/captured-native.mp4

# Vulkan rendering; independent encoder selection
cargo build --release -p director-cli --bin director-engine --features vulkan
director-engine html render examples/html/native-motion.html --gpu -o target/native-gpu.mp4
director-engine html render examples/html/native-motion.html --gpu --encoder nvenc -o target/native-gpu-nvenc.mp4
director-engine html still examples/html/native-motion.html --gpu -t 1 -o target/native-gpu.png
director-engine html benchmark examples/html/native-motion.html --gpu --encoder software --export target/gpu-bench.mp4
```

Install the optional Node dependencies in `../director-html/tools` for capture. `--modules` selects another dependency directory. Native commands need neither Node nor Chrome. See [compiler compatibility and parity](../director-html/README.md).

`--gpu` selects Vulkan painting. `--encoder nvenc` selects NVIDIA H.264 encoding and also works with CPU painting. Defaults are CPU rendering and software H.264. Explicit requests fail when unavailable. Benchmarks report backend, codec, setup time and synchronized RGBA readback; frame FPS excludes PNG encoding. The upstream Skia resolver patch is selected from `vendor/skia-safe` by Cargo.

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Script / composition / HTML compilation / extraction / args error |
| 2 | Render error |


## Portable native projects

```bash
director-engine project pack examples/html/native-motion.html -o target/movie.dirproj
director-engine inspect target/movie.dirproj -t 1
director-engine inspect target/movie.dirproj --sample 0 --sample 1.5
director-engine project edit target/movie.dirproj --patch edits.json
director-engine project repair target/movie.dirproj --patch edits.json
director-engine still target/movie.dirproj --gpu -t 1 -o target/frame.png
director-engine render target/movie.dirproj --gpu --encoder software -o target/movie.mp4
```

Packing embeds source and consumed fonts/stylesheets/images/audio/video as deduplicated SHA-256 blobs. The project can move without its original files. Loading recompiles native nodes; it needs no browser. Rhai uses its existing core project contract and is not packaged by `project pack`.

An edit file maps authored element IDs to literal text, CSS overrides and supported attributes:

```json
{
  "title": { "text": "Updated title", "styles": { "color": "#ffffff" } },
  "scene": { "attributes": { "data-duration": "8" } }
}
```

Edits validate before atomic save. `-o` writes another project; otherwise the input is replaced. `project repair` uses the same patch, then prints inspect JSON at t=0. A rejected repair does not write. Captures allow text/paint edits but retain fixed geometry and sampled motion. Unsupported edits fail; inspected bounds describe layout rather than final composited pixels.

`batch` renders every `--input` with the software encoder into `--output`. The file name is the input stem plus `.mp4`. A repeated stem becomes `stem-2.mp4`. One item failure still renders the rest. The exit code is the highest item code. `--json` prints one object, `{"exit_code","items"}`, instead of the status lines. `inspect --sample` repeats. Without it, inspect prints one object at `--time`. With it, one load prints `{"samples":[{"time","inspection"}]}`. A bad sample exits 1.

Top-level `render`, `batch`, `still`, `sheet`, `audio`, `check`, `snapshot`, `onion`, `inspect` and `preview` accept the same inputs. `sheet` writes a PNG grid and prints frame times as JSON. It rejects more than 64 frames or a raster above 16777216 pixels. `audio` prints per-track LUFS, true peak, clipped samples, and silence. Non-finite levels are null. A non-finite sample or a track above 57600000 samples sets `error` and skips analysis. `check` prints elements that leave the canvas and missing fonts. `--safe-zone tiktok` warns when text, caption, or smart_caption enters that inset. Other nodes stay on the canvas check. An unknown preset exits 1. `snapshot --golden shot.png -t 0` compares one frame. A channel delta above 16 counts. More than 0.1% different pixels exits 2 and writes `shot.actual.png` and `shot.diff.png`. `--update` replaces the golden. `onion -n 4 -o onion.png` blends frames with weights 1 through n. `render --scale 0.5` keeps the layout size and encodes even pixel dimensions at or below that scale. `--draft-encode` sets software x264 to ultrafast and CRF 30 and leaves the raster size on `--scale`. Hardware qp and alpha VP9 stay put. `html` commands additionally retain compile/capture/benchmark tooling. GPU rendering and encoder choice are independent; explicit GPU requests fail when unavailable.

## Native preview API

```bash
director-engine preview script.rhai
director-engine preview script.rhai --composition intro --props '{"title":"X"}'
director-engine preview script.rhai --bind 127.0.0.1:7788
director-engine preview target/movie.dirproj --gpu
```

The server exposes JSON and binary endpoints for editors and agents; the repo ships no browser client. A bounded queue serializes native work on one thread, including Vulkan construction/rendering/destruction. Native input and consumed asset modification time/length are checked before requests; invalid reloads keep the accepted scene and expose `reload_error`. Rhai/core project reload watches the input file.

| Endpoint | Response |
| --- | --- |
| `GET /api/meta` | Canvas, timeline, source kind, backend, revision and reload diagnostics |
| `GET /api/frame?t=1` or `?frame=30` | Native PNG with export shutter sampling and transitions |
| `GET /api/inspect?t=1` | Layout/timeline, authored IDs, sampled elements and audio metadata |
| `GET /api/audio` | Full float-stereo WAV mixed on the export clock, cached per revision. A `Range: bytes=` header returns 206 and a slice of that WAV. An unsatisfiable range returns 416 |
| `POST /api/reload` | Force compile/load and accept the candidate only on success |

Audio transport is one cached WAV per revision. Range requests slice it. Frontend playback controls are separate work. Projects are JSON with base64 media, so large media increases archive and loading memory. Shader/layout/media compatibility remains documented in `director-html`.

## Installation

```bash
cargo build --release -p director-cli
# binary: target/release/director-engine
```
