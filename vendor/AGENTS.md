# Purpose

- Own third-party source patches required by Director's native runtime.

## Ownership

- `skia-safe/` is upstream skia-safe 0.153.3 from revision `b7f043e0b1e2a850e702245de811f01df1f6b0ff`, with the Vulkan resolver lifetime patch.
- Its upstream manifest, license and source remain intact except for the three documented edits. `LICENSE` is upstream's MIT license file, which the crates.io package omits.
- `boa_engine/` is upstream boa_engine 0.22.0 from crates.io with the operand-order patch. The `director-recorder` helper runs page scripts and GSAP on it.

## Local Contracts

- Root Cargo.toml selects this copy with a local crates.io patch.
- The Vulkan backend constructor and both Ganesh Vulkan context constructors retain their thread-local resolver after native construction. Skia resolves more functions during later rendering, readback and flushing.
- Director's CLI supplies a process-lifetime resolver and uses the context on its creating thread. Do not shorten that resolver lifetime while this upstream workaround is present.
- skia-safe 0.153 requires a rendering engine beside `vulkan`. director-core's `vulkan` feature enables `skia-safe/ganesh`, which selects the `ganesh-jpegd-jpege-pdf-textlayout-vulkan` prebuilt.
- On an upstream upgrade, verify lazy readback/flush resolution before removing the patch.
- Upstream Boa compiles a local variable on the left of a binary operator as its register, so a right operand that assigns it (`k < (k = 5)`, `k + k++`) changes the left value first. `compile_lhs_operand` in `boa_engine/src/bytecompiler/mod.rs` copies such a left local into a temporary when the right operand contains an assignment or update. The arithmetic, bitwise and relational sites in `src/bytecompiler/expression/binary.rs` and the fused comparison branch in `mod.rs` call it. GSAP's `grid: 'auto'` stagger depends on it. On an upgrade, drop the patch once upstream evaluates the left operand first.

## Work Guidance

- Keep the upstream diff restricted to `std::mem::forget(resolver)` in `src/gpu/vk/vulkan_backend_context.rs` and `std::mem::forget(end_resolving)` in `direct_contexts::make_vulkan` and `contexts::make_ganesh` in `src/gpu/ganesh/vk/vk_direct_context.rs`.
- The Graphite constructors in `src/gpu/graphite/vk/graphite_context.rs` keep upstream's `drop(end_resolving)`. Director does not enable Graphite.
- Keep the boa_engine diff restricted to the commented `compile_lhs_operand` helper and its call sites.

## Verification

- `cargo check -p director-cli --features vulkan --bin director-engine --release`
- Render real GPU frames and compare them with CPU frames through the HTML GPU benchmark.
- `cargo test -p director-html --release --test integration gsap::boa_reads_left_operands_before_the_right_assigns`

## Child DOX Index

- None.
