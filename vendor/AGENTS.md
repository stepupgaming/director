# Purpose

- Own third-party source patches required by Director's native runtime.

## Ownership

- `skia-safe/` is upstream skia-safe 0.153.3 from revision `b7f043e0b1e2a850e702245de811f01df1f6b0ff`, with the Vulkan resolver lifetime patch.
- Its upstream manifest, license and source remain intact except for the three documented edits.
- `bevy_pbr/` is upstream bevy_pbr 0.20.1 from the crates.io package, with two volumetric fog edits and three screen-space reflection edits marked `Director:` in the source. `LICENSE` is upstream's MIT license file, which the crates.io package omits.
- `bevy_post_process/` is upstream bevy_post_process 0.20.1 from the crates.io package, with one effect stack edit marked `Director:` in its shaders.

## Local Contracts

- Root Cargo.toml selects this copy with a local crates.io patch.
- The Vulkan backend constructor and both Ganesh Vulkan context constructors retain their thread-local resolver after native construction. Skia resolves more functions during later rendering, readback and flushing.
- Director's CLI supplies a process-lifetime resolver and uses the context on its creating thread. Do not shorten that resolver lifetime while this upstream workaround is present.
- skia-safe 0.153 requires a rendering engine beside `vulkan`. director-core's `vulkan` feature enables `skia-safe/ganesh`, which selects the `ganesh-jpegd-jpege-pdf-textlayout-vulkan` prebuilt.
- On an upstream upgrade, verify lazy readback/flush resolution before removing the patch.
- `bevy_pbr`'s fog composite pass in `src/volumetric_fog/render.rs` blends alpha with src One and dst OneMinusSrcAlpha (upstream keeps the destination alpha), so a fog volume's coverage lands in alpha over `director-3d`'s transparent sky.
- `src/volumetric_fog/volumetric_fog.wesl` starts the accumulated colour at zero and adds the ambient in-scatter at each step of the second raymarch loop, scaled by density and step length (upstream adds one transmittance-weighted constant), so a volume lit only by the probe thickens with its density.
- `src/ssr.wesl` averages `SSR_RAYS` (16) GGX rays per pixel, each with its own blue noise layer at `frame_count * SSR_RAYS + ray` (upstream traces one and leaves the noise to TAA), so a frame rendered once reflects without salt-and-pepper misses. Its `evaluate_ssr` marches with `march_behind_surfaces` on (upstream ends a ray at the first surface it passes behind). Its environment term adds the probe's specular light only to surfaces inside the SSR roughness range, because the deferred lighting pass already adds it outside that range and upstream counts it twice there.
- On a Bevy upgrade, copy the new bevy_pbr package, reapply the edits and rerun `fog::` and `screen_reflections::` before keeping the patch. Root Cargo.toml lists `vendor/bevy_pbr` in the workspace `exclude`.
- `bevy_post_process`'s effect stack (vignette, chromatic aberration, lens distortion) writes the source's alpha (upstream writes 1): `chromatic_aberration` in `src/effect_stack/chromatic_aberration.wesl` returns the mean alpha of its samples beside the colour, and `fragment_main` in `post_process.wesl` writes it, so `director-3d`'s transparent sky stays transparent through the lens effects. On a Bevy upgrade, reapply the edit and rerun `optics::`. Root Cargo.toml patches and excludes `vendor/bevy_post_process` like `bevy_pbr`.

## Work Guidance

- Keep the upstream diff restricted to `std::mem::forget(resolver)` in `src/gpu/vk/vulkan_backend_context.rs` and `std::mem::forget(end_resolving)` in `direct_contexts::make_vulkan` and `contexts::make_ganesh` in `src/gpu/ganesh/vk/vk_direct_context.rs`.
- The Graphite constructors in `src/gpu/graphite/vk/graphite_context.rs` keep upstream's `drop(end_resolving)`. Director does not enable Graphite.

## Verification

- `cargo test -p director-html --release --test integration fog::` for the `bevy_pbr` fog edits.
- `cargo test -p director-html --release --test integration screen_reflections::` for the `bevy_pbr` SSR edits.
- `cargo test -p director-html --release --test integration optics::` for the `bevy_post_process` edit.

- `cargo check -p director-cli --features vulkan --bin director-engine --release`
- Render real GPU frames and compare them with CPU frames through the HTML GPU benchmark.

## Child DOX Index

- None.
