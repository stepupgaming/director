//! Real 3D for Director.
//!
//! A [`ModelNode`] paints a glTF binary scene through one headless Bevy app on
//! Vulkan. The studio thread owns that app; elements send it shots. On a Skia
//! context from [`gpu_context`], which shares the studio's Vulkan device, the
//! frame stays a GPU image Skia samples in place. Any other canvas gets
//! premultiplied sRGB pixels back, which Skia draws like any image.

mod accumulate;
mod catcher;
mod display;
mod glb;
mod lights;
mod gpu;
mod hero;
mod lens;
mod node;
mod sky;
mod studio;
mod text;

pub use glb::{inspect, node_names, GlbInfo};
pub use gpu::gpu_context;
pub use node::{decode, register_import, texture_draws, Asset, ModelNode, Splat, MAX_PATHS};
pub use hero::traceable;
pub use sky::{sky, sun_direction, texel_direction, Sky, SkyOptions};
pub use studio::available;
pub use text::{solid, Part, Solid, Text3d};
