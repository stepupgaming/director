# Director

> ⚠️ **Pre-release**. APIs may change before v1.0.

**A native video platform in Rust for agents and developers.**

Director renders video from Rhai scripts, HTML/CSS, HyperFrames compositions, and sampled browser captures. Taffy lays out, Skia paints on CPU or Vulkan, and FFmpeg encodes. HTML is compiled into native scene nodes, so rendering never drives a browser.

---

## Quick Start

```bash
cargo build --release -p director-cli --bin director-engine

# Rhai script
target/release/director-engine render examples/basics/hello_world.rhai -o output.mp4

# HTML/CSS composition
target/release/director-engine html render examples/html/native-motion.html -o target/native.mp4

# Portable native project: pack, inspect, edit, render
target/release/director-engine project pack examples/html/native-motion.html -o target/movie.dirproj
target/release/director-engine inspect target/movie.dirproj -t 1
target/release/director-engine render target/movie.dirproj -o target/movie.mp4
```

A minimal Rhai script:

```rhai
let movie = new_director(1920, 1080, 30);
let scene = movie.add_scene(5.0);

let root = scene.add_box(#{
    width: "100%",
    height: "100%",
    justify_content: "center",
    align_items: "center",
    bg_color: "#1a1a2e"
});

let title = root.add_text(#{
    content: "Hello, Director!",
    size: 72.0,
    color: "#ffffff",
    weight: "bold"
});

title.animate("scale", 0.8, 1.0, 1.0, "bounce_out");

movie
```

---

## Features

| Category | Features |
|----------|----------|
| **Authoring** | Rhai scripts with named compositions and props, a motion stdlib and recipes; HTML/CSS and HyperFrames compiled natively; sampled Hyper CN captures, including Paper Shader canvases translated to SKSL |
| **Layout** | Flexbox and Grid via Taffy, including CSS positioning, `calc()`, named lines and `subgrid` for HTML input |
| **Text** | SkParagraph with rich spans, weights, colors, backgrounds, shrink-to-fit, and kinetic captions with word highlighting |
| **Animation** | Keyframes and easing, spring physics, CSS keyframes, SVG SMIL |
| **Paint** | Gradients, layered backgrounds, borders, shadows, masks, filters, blend modes, 2D/3D transforms with preserve-3d depth, custom SkSL shaders |
| **Media** | Images, video, multi-track audio, Lottie |
| **Agent tooling** | `inspect`, `check`, `sheet`, `snapshot`, `onion`, `audio`, `project edit`/`repair`, `batch`, and a native preview backend |
| **Export** | CPU or Vulkan rendering, software x264 or NVENC encoding, draft encode and reduced-scale renders |

---

## Project Structure

```
director/
├── crates/
│   ├── director-core/       # Engine: scene graph, layout, animation, Rhai API, export
│   ├── director-html/       # HTML/CSS/HyperFrames compiler, native projects, capture tools
│   ├── director-cli/        # director-engine CLI and preview backend
│   ├── director-schema/     # Schema definitions
│   ├── lottie-core/         # Lottie animation logic
│   ├── lottie-data/         # Lottie data structures
│   └── lottie-skia/         # Lottie Skia renderer
├── examples/                # Rhai and HTML examples (executable reference)
├── docs/                    # Architecture, plans, contributing
└── vendor/                  # Patched third-party crates
```

---

## Building from Source

```bash
git clone https://github.com/stepupgaming/director.git
cd director
cargo build --release -p director-cli --bin director-engine
# The engine includes Vulkan rendering; pass --gpu to use it.
```

### System Dependencies

| Dependency | Ubuntu | macOS | Windows |
|------------|--------|-------|---------|
| FFmpeg | `apt install libavutil-dev libavformat-dev libavcodec-dev libswscale-dev` | `brew install ffmpeg` | [gyan.dev build](https://www.gyan.dev/ffmpeg/builds/) |
| Clang (for Skia) | `apt install clang` | Xcode | LLVM |

See [docs/contributing/development.md](docs/contributing/development.md) for detailed setup.

---

## Documentation

| Document | Description |
|----------|-------------|
| [docs/plans/10-native-platform-overhaul.md](docs/plans/10-native-platform-overhaul.md) | Current phase: status, latest test results, next action |
| [crates/director-cli/README.md](crates/director-cli/README.md) | CLI commands, exit codes, preview backend |
| [crates/director-html/README.md](crates/director-html/README.md) | HTML compiler support, capture, measurements |
| [docs/architecture/overview.md](docs/architecture/overview.md) | Engine internals |
| [docs/architecture/core-contracts.md](docs/architecture/core-contracts.md) | Normative engine spines (time, identity, pipeline, …) |
| [docs/plans/00-core-hardening-index.md](docs/plans/00-core-hardening-index.md) | Plan index |
| [docs/contributing/development.md](docs/contributing/development.md) | Build guide & contributing |
| [examples/README.md](examples/README.md) | Example scripts index |
| [docs/archive/](docs/archive/README.md) | Historical audit / deferred product plans |

---

## Embedding in Rust

```rust
use director_core::{scripting, DefaultAssetLoader};
use rhai::Engine;
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    let mut engine = Engine::new();
    scripting::register_rhai_api(&mut engine, Arc::new(DefaultAssetLoader));

    let script = r#"
        let movie = new_director(1920, 1080, 30);
        let scene = movie.add_scene(3.0);
        scene.add_text(#{ content: "Hello", size: 72.0, color: "#FFF" });
        movie
    "#;

    let movie = engine.eval::<scripting::MovieHandle>(script)?;
    let mut director = movie.director.lock().unwrap();

    director_render::render_export(
        &mut director,
        "output.mp4".into(),
        None,
        None,
        false,
    )?;

    Ok(())
}
```

Depend on `director-core` and `director-render` by path or git; the crates are not published. `director-render` paints and exports every frame.

---

## License

MIT
