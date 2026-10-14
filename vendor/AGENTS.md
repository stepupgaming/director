# Purpose

- Own third-party source patches required by Director's native runtime.

## Ownership

- `skia-safe/` is upstream skia-safe 0.90.0 from revision `da4579b39b75fa2187c58af5d33d6c1499a2ed42`, with the Vulkan resolver lifetime patch.
- Its upstream manifest, license and source remain intact except for the two documented edits.

## Local Contracts

- Root Cargo.toml selects this copy with a local crates.io patch.
- The Vulkan backend constructor and `direct_contexts::make_vulkan` retain their thread-local resolver after native construction. Skia resolves more functions during later rendering, readback and flushing.
- Director's CLI supplies a process-lifetime resolver and uses the context on its creating thread. Do not shorten that resolver lifetime while this upstream workaround is present.
- On an upstream upgrade, verify lazy readback/flush resolution before removing the patch.

## Work Guidance

- Keep the upstream diff restricted to `std::mem::forget(resolver)` in `src/gpu/vk/vulkan_backend_context.rs` and `std::mem::forget(end_resolving)` in `src/gpu/ganesh/vk/vk_direct_context.rs`.

## Verification

- `cargo check -p director-cli --features vulkan --bin director-engine`
- Render real GPU frames and compare them with CPU frames through the HTML GPU benchmark.

## Child DOX Index

- None.
