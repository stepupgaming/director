# Purpose

- Own real 3D for Director: glTF binary scenes rendered by Bevy 0.20 on wgpu Vulkan and painted into Skia as one element.

## Ownership

- `src/glb.rs` - `inspect`: the clip count, the longest clip duration and the camera count of a GLB, read from its JSON chunk with no GPU.
- `src/node.rs` - `Asset` (bytes, content hash, loader extension, optional source path), `ModelNode` (the `model` element), its document export, `decode` and `register_import`.
- `src/studio.rs` - the studio: one headless Bevy app on a `director-3d` thread, its stages, the orbit camera system, render targets, readback, the equirectangular to cube conversion and the neutral studio probe.
- `tests/fixtures/make_model.py` - the Blender 5.2 script that writes `tests/fixtures/model.glb`: a turning cube, a rising sphere, a ground, a sun and a camera, exported as two 61-frame clips at 30 fps. Both files are owned source; rerun the script after editing it.

## Local Contracts

- Compiling a model reads its bytes and `inspect`s them. The GPU starts on the first frame a model paints. `available()` starts the studio and reports whether a Vulkan adapter exists; tests that paint a model skip without one.
- The studio is one process-wide Bevy app. Each distinct (model hash, environment hash) loads once as a stage: its glTF scene root, every clip in one `AnimationGraph`, a Director camera, and a probe cube (the environment `.hdr` or `.exr` converted to a 512 px Rgba16Float cube, or a 64 px neutral studio probe). glTF cameras are deactivated and lights get shadow maps.
- A shot carries every input of its frame: stage, size, clip time, view, exposure, probe intensity and background. It hides other stages, seeks every clip and places the camera, so frames render the same in any order. A new stage or target size steps the app three extra times before its first frame so pipelines and the filtered probe exist.
- The camera starts from the file's first camera at this frame's animated pose and lens, or from a framing camera with a 30 degree vertical field of view that fits the bounding sphere. `ModelView` then orbits it about the bounding sphere centre: yaw about world Y, pitch about the camera's right axis (clamped to 89 degrees, positive raises the camera) and distance scaling the offset.
- Frames render in HDR with TonyMcMapface, `Bloom::NATURAL`, SMAA and screen-space transmission into an `Rgba8UnormSrgb` target, read back as premultiplied sRGB RGBA8. Without `background` the clear is transparent and the page shows through the sky.
- `ModelNode` paints its frame at the device size of its layout box and caches the last frame by every shot input. Clip time is the element time, wrapped at the longest clip with `looped` and held at its ends otherwise.
- A document records the model as a `NodeBody::Model`: a `model/gltf-binary` asset (by path when the source has one, embedded otherwise), an optional `image/vnd.radiance` or `image/x-exr` environment, exposure, probe intensity, background, loop and the view before animation. `decode` rebuilds the same element.
- `director-3d` never links a second Skia: it depends on the workspace `skia-safe`, and Bevy's `wgpu` stays at the version Bevy pins.

## Work Guidance

- The engine choice, the Live, Export and Hero quality levels and the Filament speed reference are recorded in issue 9 of `docs/plans/10-native-platform-overhaul.md`.

## Verification

- `cargo test -p director-3d --release --test integration`
- `cargo test -p director-html --release --test integration model::` paints the fixture through `<model>`: transparency, clip time, render order, CSS and GSAP camera, and the document round trip.

## Child DOX Index

- No child AGENTS.md files required.
