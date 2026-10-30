# Director

Director renders video from HTML and CSS without a browser. It compiles a composition, GSAP timelines included, into a native scene. Taffy lays it out, Skia paints it on the CPU or on Vulkan, and FFmpeg encodes it.

Director is pre-release, and its APIs will change before 1.0.

## Quick start

```bash
git clone https://github.com/stepupgaming/director.git
cd director
cargo build --release -p director-cli --bin director-engine

target/release/director-engine html render examples/html/native-motion.html -o native.mp4
target/release/director-engine html render examples/html/native-motion.html -o native.mp4 --gpu --encoder nvenc
target/release/director-engine html still examples/html/native-motion.html -t 1.5 -o frame.png
```

`--gpu` renders on Vulkan and `--encoder nvenc` encodes on an NVIDIA card. Each flag works without the other.

## A composition

The root element sets the canvas size, frame rate and duration:

```html
<div data-composition-id="hello" data-width="1920" data-height="1080" data-fps="30" data-duration="3"
     style="display:flex; align-items:center; justify-content:center; background:#101820">
  <h1 class="title">Hello, Director</h1>
</div>
<style>
  .title { color: white; font: 700 120px sans-serif; animation: rise 1s ease-out both; }
  @keyframes rise { from { opacity: 0; transform: translateY(80px); } to { opacity: 1; transform: none; } }
</style>
```

```bash
target/release/director-engine html render hello.html -o hello.mp4
```

Director runs GSAP at compile time and keeps no browser for the render. A page that loads GSAP needs a local copy, either `npm install gsap` in the page's folder or a parent folder, or `DIRECTOR_GSAP_DIR` pointing at a `gsap/dist` folder.

## What it covers

- Layout: flexbox, grid with `subgrid` and named lines, absolute, fixed and sticky positioning, and `calc()`.
- Text: rich inline spans, inline boxes, text strokes and shadows, variable font axes, SplitText, rolling digit reels.
- Paint: gradients, layered backgrounds, borders, shadows, masks, clip paths, filters, blend modes, SVG, and 3D transforms with `preserve-3d` depth sorting.
- Animation: CSS keyframes and transitions, SVG SMIL, timeline JSON, beat grids from music, and GSAP with its MotionPath, MorphSVG, DrawSVG and SplitText plugins. All of it compiles to native keyframe tracks.
- Motion blur samples each frame's shutter. A still frame paints once, and fast motion gets more samples.
- Media: images, video, multi-track audio and Lottie.
- HyperFrames compositions compile natively.
- Rhai scripts generate scenes as an alternative to HTML (see [examples/](examples/README.md)).
- Export: x264 or NVENC MP4, VP9 with alpha, draft encodes and reduced-scale renders.

## Agent tooling

The CLI gives an agent ways to check a video without watching it:

| Command | What it reports |
|---------|-----------------|
| `inspect`, `outline` | Layout, timeline and audio as JSON, and the node tree |
| `check`, `qa` | Elements off canvas, missing fonts, safe zones, motion and loudness |
| `sheet`, `onion`, `still` | Contact sheets, blended motion trails and single frames |
| `snapshot`, `diff` | A frame against a golden PNG, and two scene documents against each other |
| `audio` | Loudness, clipping and silence |
| `mcp` | A Model Context Protocol server over host sessions |
| `editor` | A JSON-line editing protocol with frames in shared memory |

## Embedding

`director-host` opens any input as a session that you can patch, inspect, render and export:

```rust
use director_host::{ExportSettings, NodeSelector, PatchOp, Session, Source};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = Session::open(&Source::new("examples/html/native-motion.html".into()))?;
    let hide = PatchOp::Set { node: NodeSelector::Name("pulse".into()), path: "/body/hidden".into(), value: json!(true) };
    session.patch(session.revision(), &[hide])?;
    session.export("native.mp4".as_ref(), &ExportSettings::default(), None)?;
    Ok(())
}
```

The crates are not on crates.io yet. Depend on them by git or path.

## Building

| Dependency | Ubuntu | macOS | Windows |
|------------|--------|-------|---------|
| FFmpeg | `apt install libavutil-dev libavformat-dev libavcodec-dev libswscale-dev` | `brew install ffmpeg` | [gyan.dev build](https://www.gyan.dev/ffmpeg/builds/) |
| Clang (for Skia) | `apt install clang` | Xcode | LLVM |

Build with `--release`. Skia downloads as a prebuilt library. [docs/contributing/development.md](docs/contributing/development.md) has the full setup.

## Repository

| Path | Contents |
|------|----------|
| `crates/director-html` | HTML, CSS, HyperFrames and GSAP compiler |
| `crates/director-core` | Scene graph, layout, animation, text, media and the Rhai API |
| `crates/director-scene` | Serializable scene document |
| `crates/director-render` | Frame rendering, motion blur and export |
| `crates/director-host` | Session API: open, patch, inspect, render, export |
| `crates/director-cli` | The `director-engine` CLI, MCP server and editor protocol |
| `crates/director-schema` | Schema reflection |
| `crates/lottie-*` | Lottie data, animation and Skia rendering |
| `examples/` | HTML and Rhai examples |
| `docs/` | Architecture, plans and contributing |
| `vendor/` | Third-party crates with local patches |

Start with [docs/plans/10-native-platform-overhaul.md](docs/plans/10-native-platform-overhaul.md) for current status and measurements. The CLI and the HTML compiler have their own READMEs in [crates/director-cli](crates/director-cli/README.md) and [crates/director-html](crates/director-html/README.md). [docs/architecture/](docs/architecture/overview.md) covers engine internals.

## License

MIT
