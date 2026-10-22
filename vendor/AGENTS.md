# Purpose

- Own third-party source patches required by Director's native runtime.

## Ownership

- `skia-safe/` is upstream skia-safe 0.153.3 from revision `b7f043e0b1e2a850e702245de811f01df1f6b0ff`, with the Vulkan resolver lifetime patch.
- Its upstream manifest, license and source remain intact except for the three documented edits. `LICENSE` is upstream's MIT license file, which the crates.io package omits.

## Local Contracts

- Root Cargo.toml selects this copy with a local crates.io patch.
- The Vulkan backend constructor and both Ganesh Vulkan context constructors retain their thread-local resolver after native construction. Skia resolves more functions during later rendering, readback and flushing.
- Director's CLI supplies a process-lifetime resolver and uses the context on its creating thread. Do not shorten that resolver lifetime while this upstream workaround is present.
- skia-safe 0.153 requires a rendering engine beside `vulkan`. director-core's `vulkan` feature enables `skia-safe/ganesh`, which selects the `ganesh-jpegd-jpege-pdf-textlayout-vulkan` prebuilt.
- On an upstream upgrade, verify lazy readback/flush resolution before removing the patch.

## Work Guidance

- Keep the upstream diff restricted to `std::mem::forget(resolver)` in `src/gpu/vk/vulkan_backend_context.rs` and `std::mem::forget(end_resolving)` in `direct_contexts::make_vulkan` and `contexts::make_ganesh` in `src/gpu/ganesh/vk/vk_direct_context.rs`.
- The Graphite constructors in `src/gpu/graphite/vk/graphite_context.rs` keep upstream's `drop(end_resolving)`. Director does not enable Graphite.

## Verification

- `cargo check -p director-cli --features vulkan --bin director-engine --release`
- Render real GPU frames and compare them with CPU frames through the HTML GPU benchmark.

## Child DOX Index

- None.
