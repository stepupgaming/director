# Purpose

- Compile HTML/CSS and HyperFrames compositions into Director scenes without browser frame rendering.

## Ownership

- `src/` owns parsing, cascade, style translation, deterministic animation, native element adapters, and captured-scene loading.
- `src/project.rs` owns `director-native-project/1`, editable source and validated element overrides; `src/assets.rs` owns asset recording, deduplicated SHA-256 blobs and temporary media-file leases.
- `tools/` owns optional compile-time browser extraction, Chromium and CPU/GPU parity, real HyperFrames CLI and CPU/GPU export benchmarks, and Hyper CN registry auditing.
- `tests/` owns executable compatibility and pixel regressions.
- `benches/` owns Criterion compile/render workloads.
- `README.md` owns supported features, incompatibilities, commands, and reproducible measured results.

## Local Contracts

- Depend on `director-core`; do not introduce HTML behavior into Rhai or the core dependency-graph compiler.
- Use upstream HTML, CSS, selector, layout, typography and SVG libraries. Unsupported syntax or rendering behavior must produce diagnostics; compilation fails when errors remain.
- Browser extraction is explicit and compile-time only. Capture layout, style, SVG and media references, never screenshots as native scene content. Captures declare their sample rate and cannot claim continuous animation equivalence.
- Do not treat Hyper CN WebGL/canvas content as supported until a native translation exists. Registry audits must distinguish renderable inputs from diagnosed incompatibilities.
- Generated captures, videos, reference checkouts and benchmark reports live under ignored `target/` or user-selected outputs.
- Native adapters must remain semantically Send under the core scene-node contract. Share captured graphs behind `Arc<Mutex<SceneGraph>>`, never `Rc` or an unprotected shared graph.
- Native projects preserve HTML/capture authoring data and compile it on load. Do not serialize HTML adapters through core project I/O, which owns a different node-kind contract.
- Pack every consumed local stylesheet, font, image, audio and video plus required bundled fonts. Frozen project compilation cannot read original files or substitute host files for missing blobs; hash mismatches fail.
- Apply edits by stable authored element ID and validate the complete candidate before acceptance or atomic save. Captures retain sampled timing and fixed geometry; reject edits requiring uncaptured layout/animation.
- Preserve unique authored capture IDs. Anonymous/duplicate IDs receive noncolliding structural paths, stable while sampled topology is stable; expose those paths in inspection and store them in packed captures.
- Keep extracted media files alive through the Director asset-loader lifetime, even when the project or compiler wrapper is dropped.
- Share immutable source and encoded blobs across edit candidates; validation must not clone entire media payloads merely to change an edit map.

## Work Guidance

- Preserve arbitrary-time seek determinism. Reuse immutable assets and resolve static styles before rendering.
- Keep measured parity and performance claims tied to reproducible commands and artifacts.
- CPU/GPU comparisons must synchronize GPU work through readback, verify the actual backend/encoder and probe every MP4's frame count and audio/video/container duration. GPU rendering and NVENC encoding are separate controls.
- Exercise real Hyper CN original and portrait registry files. Compilation/render smoke results are separate from measured pixel parity.

## Verification

- `cargo test -p director-html`
- `tests/project.rs` verifies portability after source deletion, checksums, asset deduplication, explicit fonts/images, literal text, edit rejection, sampled inspection and real media lifetime.
- Media tests require FFmpeg and ffprobe in PATH and fail when tools or fixtures are unavailable.
- `cargo bench -p director-html --bench html`
- `cargo test -p director-cli`
- `node crates/director-html/tools/parity.mjs --help`
- `node --test crates/director-html/tools/tools.test.mjs`
- `node crates/director-html/tools/gpu-benchmark.mjs --input examples/html/native-motion.html --director target/release/director-engine --output target/new-gpu-comparison` requires a Vulkan build and the selected encoders.

## Child DOX Index

- None.
