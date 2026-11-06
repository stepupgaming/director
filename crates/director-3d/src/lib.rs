//! Real 3D for Director.
//!
//! A [`ModelNode`] paints a glTF binary scene through one headless Bevy app on
//! Vulkan. The studio thread owns that app; elements send it shots and get
//! premultiplied sRGB pixels back, which Skia draws like any image.

mod glb;
mod node;
mod studio;

pub use glb::{inspect, GlbInfo};
pub use node::{decode, register_import, Asset, ModelNode};
pub use studio::available;
