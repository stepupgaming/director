# Purpose

- Solve the camera of a piece of footage, so a 3D model composited over it moves with the plate.

## Ownership

- `src/footage.rs` - `read_footage`: decodes a video's luma through the `ffmpeg` CLI at most `max_width` wide, with its frame rate from `ffprobe`.
- `src/image.rs` - `Gray` frames, Sobel gradients, `Pyramid` levels, and bilinear and Catmull-Rom bicubic sampling.
- `src/features.rs` - Shi-Tomasi corners with a minimum spacing, away from existing tracks and the border.
- `src/klt.rs` - pyramidal Lucas-Kanade (`track_point`) and the affine Gauss-Newton match of a track against its first appearance (`template`, `align`).
- `src/tracks.rs` - `track`: frame-to-frame tracks over a whole shot.
- `src/geometry.rs` - normalized eight-point essential matrices and four-point homographies with RANSAC, their factoring into candidate poses, cheirality and Sampson support, triangulation and PnP resection.
- `src/bundle.rs` - Levenberg-Marquardt bundle adjustment with a Schur complement over cameras, points and optionally the focal length.
- `src/solve.rs` - `solve`, `SolveOptions`, `CameraSolve` and `Pose`: the incremental reconstruction and the ground-plane world.

## Local Contracts

- The crate links no Skia and no GPU. It depends on `nalgebra`, `rayon` and `serde`, and `read_footage` needs `ffmpeg` and `ffprobe` in PATH.
- `track` follows up to 500 corners at least 10 px apart through a 4-level pyramid with bicubic sampling. A step survives a forward-backward round trip within 0.5 px, then an affine match against the track's first 17x17 appearance corrects it, moving it at most 1.5 px with a mean absolute difference below 10, so long tracks do not drift. New corners seed where live ones thinned out. Tracks shorter than 3 frames drop.
- `solve` picks the start pair with the most parallax among frames whose shared tracks support a pose: the essential matrix and the homography each give candidate poses, and the candidate with the most tracks inside a Sampson error of 1/f pixels and in front of both cameras wins, so a planar yard starts as well as a deep one. Each further frame registers by PnP against the triangulated points, new tracks triangulate once their rays part by enough angle, and bundle adjustment with outlier pruning refines the whole solve. `refine_focal` lets the bundle refine the lens from `yfov`.
- The world puts the dominant ground plane under frame 0 at y = 0, the origin where frame 0's optical axis meets it, frame 0's view along -Z with +Y up, and scales it so frame 0's camera stands `camera_height` metres above the ground. A `Pose` is a camera position and a glTF rotation (x, y, z, w) looking down its local -Z. `error` is the rms reprojection error in pixels.
- Footage with too little motion or texture fails with a message, and so does a missing file, which names its path.
- `director-html` calls `solve` at compile time for `<model track>` and stores the cameras in `director_scene::ModelTrack`.

## Work Guidance

## Verification

- `cargo test -p director-track --release --test integration`. `klt::` checks sub-pixel tracking on a shifted pattern and refusal on a flat window. `solve::` solves `crates/director-3d/tests/fixtures/solve.mp4` against the true cameras in `solve.json` at the true lens and from a 36 degree guess: reprojection error, camera position and rotation, the image-space error of a grid of points near the origin, and the ground height, plus still footage and a missing file.

## Child DOX Index

- No child AGENTS.md files required.
