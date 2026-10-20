# Purpose

- Own the serializable scene document: nodes, channels, keyframes, and kind-specific fields, with no renderer types.

## Ownership

- `src/lib.rs` owns `SceneDocument`, `NodeRecord`, `Channel`, `NodeBody`, JSON read/write, and the JSON Schema.

## Local Contracts

- This crate does not depend on `director-core`, Skia, Taffy, or FFmpeg.
- [`VERSION`](src/lib.rs) is the only document version this crate reads. A different version is `SceneError::Version`.
- `NodeRef` is the generational id (`index`, `generation`).
- `kind` is the `Element::kind` tag. `NodeBody::Unspecified` means the exporter has not copied that kind's fields yet.
- `Channel` keyframes are in time order. A still value is one keyframe at time 0.
- `director-core::scene_export` writes this document. It does not read one back into a `Director`.

## Work Guidance

- Add a `NodeBody` variant when an exporter starts copying that kind. Keep the tag snake_case.
- Do not put Skia, Taffy, or image buffers in this crate.

## Verification

- `cargo test -p director-scene --release`
- `cargo clippy -p director-scene --all-targets --release -- -D warnings`

## Child DOX Index

- None.
