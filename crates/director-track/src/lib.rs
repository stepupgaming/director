//! Camera solving from footage.
//!
//! `read_footage` decodes a video's luma through the `ffmpeg` CLI. `track`
//! follows Shi-Tomasi corners from frame to frame with pyramidal
//! Lucas-Kanade and a forward-backward check, and corrects each step with an
//! affine match against the track's first appearance. `solve` turns the
//! tracks into a camera per frame: the essential matrix or the homography
//! between two frames with enough parallax starts the scene, each further frame registers against the
//! points seen so far, new tracks triangulate as they gain parallax, and a
//! Levenberg-Marquardt bundle adjustment with a Schur complement refines
//! every camera, every point and, on request, the focal length. The dominant
//! ground plane under the first camera then sets the world: the ground at
//! y = 0, the origin where frame 0's optical axis meets it, frame 0's view
//! along -Z, and the scale from frame 0's camera height.

mod bundle;
mod features;
mod footage;
mod geometry;
mod image;
mod klt;
mod solve;
mod tracks;

pub use footage::{read_footage, Footage};
pub use image::Gray;
pub use klt::track_point;
pub use solve::{solve, CameraSolve, Pose, SolveOptions};
pub use tracks::{track, Track};
