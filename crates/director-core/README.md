# director-core

The heart of Director Engine. Contains rendering, layout, animation, and scripting logic.

## Overview

| Component | Purpose |
|-----------|---------|
| **Scene Graph** | Arena-based node storage with `NodeId` handles |
| **Timeline** | `tracks` / `TrackClip` (+ legacy `timeline` — being unified, plan 01) |
| **Layout** | Taffy-powered Flexbox/Grid |
| **Rendering** | Skia 2D rasterization, dirty/static cache paths |
| **Animation** | Keyframes, springs, easings (bezier/hold/expression) |
| **Scripting** | Rhai API bindings |
| **Project I/O** | Versioned JSON save/load (`project.rs`) |

## Usage

```toml
[dependencies]
director-core = "1.1"
```

```rust
use director_core::{scripting, DefaultAssetLoader};
use rhai::Engine;
use std::sync::Arc;

let mut engine = Engine::new();
scripting::register_rhai_api(&mut engine, Arc::new(DefaultAssetLoader));

let movie = engine.eval::<scripting::MovieHandle>(script)?;
```

## Module Map

| Module | Purpose |
|--------|---------|
| `director.rs` | Coordinator: tracks, update, compile/invalidate |
| `timeline.rs` | `Track` / `TrackClip` |
| `scene.rs` | Scene graph storage |
| `scripting/` | Rhai bindings |
| `animation.rs` | Animation system |
| `systems/` | Renderer, layout, assets, transitions |
| `node/` | Box, Text, Image, Caption, Effect, … |
| `project.rs` | Project JSON round-trip |
| `api_catalog.rs` | SSOT for effects/transitions/helpers |

## Feature Flags

| Flag | Purpose |
|------|---------|
| `mock_video` | Build without FFmpeg |
| `vulkan` | Enable Vulkan Skia backend |

## Core hardening

Active engine plans (time, identity, pipeline, …):  
[docs/plans/00-core-hardening-index.md](../../docs/plans/00-core-hardening-index.md)  
Contracts: [docs/architecture/core-contracts.md](../../docs/architecture/core-contracts.md)

---

*See root [AGENTS.md](../../AGENTS.md) and [crates/director-core/AGENTS.md](./AGENTS.md).*
