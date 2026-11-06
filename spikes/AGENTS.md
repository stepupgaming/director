# Purpose

- Own throwaway measurement code that settles an engine or library choice before it enters the workspace. Results land in the plan 10 measurement table; the code stays so a row can be re-run.

## Ownership

- `engine-3d/` - issue 9 engine spike. Each candidate renders the same glTF scene under IBL with shadows and bloom, composites a Skia 2D layer on the same GPU, and prints per-frame medians and p95 at the requested size.
  - `scene/make_scene.py` - Blender script that writes `spike.glb`: bevelled type, chrome, plastic, clearcoat and glass spheres, an emissive ring, a monkey head, a sun, a spot light and a 4 s camera orbit.
  - `bevy/` - Bevy 0.20 on wgpu Vulkan. Skia wraps the render target image on Bevy's device.
  - `filament/` - Filament 1.80 through a C++ shim (`shim.cpp`, `stub/bluevk/BlueVK.h` stands in for the header the SDK lacks). Skia runs on Filament's Vulkan device; an `ash` copy moves each headless swapchain image into a Skia texture, because those images lack SAMPLED usage.
  - `threejs/` - Three.js r185 in headless Chromium with ANGLE on Vulkan. `run.mjs` serves `page.html`, times render and full-frame `readPixels` in the page, and times CDP screenshots as the hand-off a browser process pays.

## Local Contracts

- Spikes stay outside the Cargo workspace: every spike crate declares its own `[workspace]` and patches `skia-safe` to `vendor/skia-safe`.
- Build outputs and assets live on `F:/director-spikes`: `target/`, `assets/` (`spike.glb`, Blender's `forest.exr` studiolight, `forest.hdr`, Filament `cmgen` output in `assets/filament/`), `out/` (PNGs) and the Filament SDK in `filament/`. None of it is committed.
- Rust spikes build with `cargo build --release` and `CARGO_TARGET_DIR=F:/director-spikes/target`; `SKIA_BINARIES_URL=file://F:/director-spikes/skia-cache/skia-binaries-{key}.tar.gz` reuses the prebuilt Skia archive.
- Spike code is never linked from `crates/`. A chosen engine gets a proper crate under `crates/` with its own AGENTS.md.

## Work Guidance

- Ask the user before GPU-heavy runs, Chrome launches and downloads.
- Run every candidate on the same scene, size and frame count, and record differences in post effects (AA, tone mapper, shadow filter) beside the numbers.

## Verification

- `spike-bevy <w> <h> <frames> <assets> <out>` and `spike-filament <w> <h> <frames> <assets> <out>` from `F:/director-spikes/target/release`; `node spikes/engine-3d/threejs/run.mjs <w> <h> <frames>`. Each prints one timing line and writes PNGs to `<out>`.

## Child DOX Index

- No child AGENTS.md files required.
