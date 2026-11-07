# Purpose

- Own third-party source patches required by Director's native runtime.

## Ownership

- `skia-safe/` is upstream skia-safe 0.153.3 from revision `b7f043e0b1e2a850e702245de811f01df1f6b0ff`, with the Vulkan resolver lifetime patch, published on crates.io as `director-skia-safe` 0.153.3. Its library name stays `skia_safe`.
- Its license and source remain intact except for the three documented edits. The manifest differs from upstream in the package name, description and repository only.
- `bevy_pbr/` is upstream bevy_pbr 0.20.1 from the crates.io package, with one volumetric fog blend edit. `LICENSE` is upstream's MIT license file, which the crates.io package omits.
- Director's Bevy shader edits (screen-space reflections, the volumetric fog raymarch, lens effect alpha) live in `crates/director-3d/src/patch/`, which replaces the embedded shaders at run time.

## Local Contracts

- Root Cargo.toml depends on `skia-safe/` through the workspace `skia-safe` dependency, renamed to the `director-skia-safe` package, so published Director crates use the fork too. When its source changes, publish a new version before every crate that links Skia.
- The Vulkan backend constructor and both Ganesh Vulkan context constructors retain their thread-local resolver after native construction. Skia resolves more functions during later rendering, readback and flushing.
- Director's CLI supplies a process-lifetime resolver and uses the context on its creating thread. Do not shorten that resolver lifetime while this upstream workaround is present.
- skia-safe 0.153 requires a rendering engine beside `vulkan`. director-core's `vulkan` feature enables `skia-safe/ganesh`, which selects the `ganesh-jpegd-jpege-pdf-textlayout-vulkan` prebuilt.
- On an upstream upgrade, verify lazy readback/flush resolution before removing the patch.
- `bevy_pbr`'s fog composite pass in `src/volumetric_fog/render.rs` blends alpha with src One and dst OneMinusSrcAlpha (upstream keeps the destination alpha), so a fog volume's coverage lands in alpha over `director-3d`'s transparent sky. The edit is Rust, so a shader swap cannot carry it. Root Cargo.toml's `[patch.crates-io]` selects this copy for repository builds only. Crates installed from crates.io use upstream bevy_pbr, where a fog volume leaves the page under a transparent sky uncovered. Drop the copy once upstream Bevy blends fog alpha this way.
- On a Bevy upgrade, copy the new bevy_pbr package, reapply the edit and rerun `fog::` before keeping the patch. Root Cargo.toml lists `vendor/bevy_pbr` in the workspace `exclude`.

## Work Guidance

- Keep the upstream diff restricted to `std::mem::forget(resolver)` in `src/gpu/vk/vulkan_backend_context.rs` and `std::mem::forget(end_resolving)` in `direct_contexts::make_vulkan` and `contexts::make_ganesh` in `src/gpu/ganesh/vk/vk_direct_context.rs`.
- The Graphite constructors in `src/gpu/graphite/vk/graphite_context.rs` keep upstream's `drop(end_resolving)`. Director does not enable Graphite.

## Verification

- `cargo test -p director-html --release --test integration fog::` for the `bevy_pbr` fog blend edit.
- `cargo package --manifest-path vendor/skia-safe/Cargo.toml --no-verify` before publishing `director-skia-safe`.
- `cargo check -p director-cli --bin director-engine --release`
- Render real GPU frames and compare them with CPU frames through the HTML GPU benchmark.

## Child DOX Index

- None.
