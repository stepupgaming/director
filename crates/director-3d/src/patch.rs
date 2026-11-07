//! Director's edits to Bevy's screen-space reflection, volumetric fog and
//! lens effect shaders. The `patch/*.wesl` files are Bevy 0.20.1's shaders
//! (MIT OR Apache-2.0) with each edit marked `Director:`. They replace the
//! embedded upstream sources at the same asset paths, so every import and
//! pipeline that names those paths compiles the edited shader.

use std::path::{Path, PathBuf};

use bevy::{
    asset::io::embedded::EmbeddedAssetRegistry,
    prelude::*,
    shader::{Shader, Source},
};

/// Embedded asset path under `embedded://` and the edited source.
const PATCHES: [(&str, &str); 4] = [
    ("bevy_pbr/ssr.wesl", include_str!("patch/ssr.wesl")),
    ("bevy_pbr/volumetric_fog/volumetric_fog.wesl", include_str!("patch/volumetric_fog.wesl")),
    (
        "bevy_post_process/effect_stack/chromatic_aberration.wesl",
        include_str!("patch/chromatic_aberration.wesl"),
    ),
    ("bevy_post_process/effect_stack/post_process.wesl", include_str!("patch/post_process.wesl")),
];

/// Swaps the edited shaders in. Add it after `DefaultPlugins`, whose plugins
/// register the upstream sources this overwrites.
pub(crate) struct PatchPlugin;

#[derive(Resource)]
struct Patched(Vec<(Handle<Shader>, &'static str)>);

impl Plugin for PatchPlugin {
    fn build(&self, app: &mut App) {
        let registry = app.world().resource::<EmbeddedAssetRegistry>();
        for (path, source) in PATCHES {
            registry.insert_asset(PathBuf::from(path), Path::new(path), source.as_bytes());
        }
        let server = app.world().resource::<AssetServer>().clone();
        let patched = PATCHES
            .iter()
            .map(|&(path, source)| (server.load(format!("embedded://{path}")), source))
            .collect();
        app.insert_resource(Patched(patched));
        app.add_systems(First, keep_patched);
    }
}

/// Upstream's own load of a shader may have read its bytes before
/// `PatchPlugin` replaced them. Reload any patched shader whose source is not
/// the edited one; the reload reads the replaced bytes.
fn keep_patched(
    mut events: MessageReader<AssetEvent<Shader>>,
    shaders: Res<Assets<Shader>>,
    patched: Res<Patched>,
    server: Res<AssetServer>,
) {
    for event in events.read() {
        let (AssetEvent::Added { id } | AssetEvent::Modified { id } | AssetEvent::LoadedWithDependencies { id }) = event else {
            continue;
        };
        let Some((handle, source)) = patched.0.iter().find(|(handle, _)| handle.id() == *id) else {
            continue;
        };
        let stale = shaders
            .get(*id)
            .is_some_and(|shader| !matches!(&shader.source, Source::Wesl(current) if current == source));
        if let Some(path) = handle.path().filter(|_| stale) {
            server.reload(path.clone());
        }
    }
}

/// Whether every patched shader has loaded with its edited source.
pub(crate) fn ready(world: &World) -> bool {
    let shaders = world.resource::<Assets<Shader>>();
    world.resource::<Patched>().0.iter().all(|(handle, source)| {
        shaders
            .get(handle)
            .is_some_and(|shader| matches!(&shader.source, Source::Wesl(current) if current == source))
    })
}
