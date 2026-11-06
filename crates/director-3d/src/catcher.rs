//! Shadow catchers: meshes that show only the shadows cast on them.
//!
//! A matchmoved shot puts footage behind a transparent model and marks the
//! filmed ground's stand-in mesh as a catcher, so the model's shadows fall
//! on the footage. `catcher.wesl` writes black at the shadowed fraction of
//! the direct light as premultiplied alpha. The material blends, so it draws
//! in the transparent pass: over the Hero image too, which leaves catchers
//! out of the ray traced scene because Solari only traces standard
//! materials.

use bevy::{
    asset::embedded_asset,
    pbr::{Material, MaterialPlugin},
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
};

const SHADER: &str = "embedded://director_3d/catcher.wesl";

#[derive(Asset, TypePath, AsBindGroup, Clone, Default)]
pub(crate) struct ShadowCatcher {}

impl Material for ShadowCatcher {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }
}

pub(crate) struct CatcherPlugin;

impl Plugin for CatcherPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "catcher.wesl");
        app.add_plugins(MaterialPlugin::<ShadowCatcher>::default());
    }
}
