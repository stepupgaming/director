# Purpose

- Own the serializable scene document: nodes, channels, keyframes, kind-specific bodies, timeline, audio mix, semantic entities and the asset table, with no renderer types.

## Ownership

- `src/lib.rs` owns `SceneDocument`, `NodeRecord`, `Filter`, `PathAnimation`, `PathMorph`, `AudioBinding`, `AssetRecord`, `FontFace`, `VERSION`, JSON read/write and the JSON Schema.
- `src/body.rs` owns `NodeBody` and the box, text, media, shape, vector, Lottie, composition, effect and spectrum bodies.
- `src/caption.rs` owns the caption, smart caption and active speaker camera bodies.
- `src/channel.rs` owns `Channel`, `Keyframe`, `Easing` and `Rgba`.
- `src/effect.rs` owns `Effect`, `EffectParam` and `Uniform`.
- `src/layout.rs` owns `LayoutStyle`, the Taffy-free mirror of a node's layout style.
- `src/html.rs` owns the `director-html` bodies: `HtmlBody` with its computed CSS paint, content, CSS animations, Paper shader and `InlineFlow` link (a text leaf's placeholders, or an atomic box's slot with its auto axes and box sizing); `HtmlVideoBody`; `HtmlCaptureBody` with deduplicated sampled graphs, each carrying the sampled layout rect of every node in `rects`; and `SvgSmilBody` with SMIL animations.
- `src/timeline.rs` owns tracks, clips and transitions. `src/audio.rs` owns the mixer tracks and effect chains. `src/entity.rs` owns semantic entities and their samples.

## Local Contracts

- This crate does not depend on `director-core`, Skia, Taffy, FFmpeg or uuid.
- [`VERSION`](src/lib.rs) is the only document version this crate reads. A different version is `SceneError::Version`.
- `NodeRef` is the generational id (`index`, `generation`). `kind` is the `Element::kind` tag.
- Every kind has a `NodeBody` variant tagged by `type` in snake_case. The document has no placeholder body: an element without a document form fails export.
- `Channel` keyframes are in time order and carry their easing. A still value is one keyframe at time 0.
- Effects and transitions use their primary `api_catalog` names. Parameters are keyed by name.
- Media live in the `assets` table and nodes refer to them by `AssetId`. An asset is a source path or base64-embedded bytes with a media type. A path is the reference the scene's asset loader resolved.
- `fonts` lists font files by family and asset in registration order, omitted when empty. A renderer registers them on top of its bundled fonts before painting.
- An audio track's `source_start` skips that many seconds of its path asset. It is 0 and omitted for embedded samples.
- A Lottie body holds the authored Lottie JSON and `images`, the override images by Lottie asset id, omitted when empty.
- Entities are sorted by id so equal scenes write equal documents.
- Values JSON cannot hold have a finite form: an unbounded end time or `infinite` iteration count is `None`. CSS transforms and mixed `calc()` gradient stops are CSS text.
- `director-core::scene_export` writes this document and `director-core::scene_import` reads it back into a `Director`. `NodeBody::kind` returns the record kind tag of a body, and `TransformChannels::default` is the identity transform pivoting on the center.

## Work Guidance

- Add a body field when an element gains state that changes its pixels. Leave caches and derived state out.
- Do not put Skia, Taffy, or image buffers in this crate.

## Verification

- `cargo test -p director-scene --release`
- `cargo clippy -p director-scene --all-targets --release -- -D warnings`

## Child DOX Index

- None.
