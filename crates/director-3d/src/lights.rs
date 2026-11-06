//! Lights a model's HTML element or document adds to the glTF scene. A light
//! with an emitter size moves to a new point on its emitter for each of the
//! shot's samples and casts hard shadows from there, so the averaged frame
//! has the penumbra of the real source. A Hero shot samples point and spot
//! lights in its own pass from their emitter radius, and adds each rect
//! light as an emitting quad of the same power.

use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;
use bevy::solari::scene::RaytracingMesh3d;
use director_scene::{ModelLight, ModelLightKind};
use std::f32::consts::PI;

/// How the Hero pass reads a light spawned here: a point or spot light's
/// emitter radius, or a rect light's stand-in spot, which it skips because
/// the rect's quad emitter traces instead.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct Authored {
    pub radius: f32,
    pub rect: bool,
}

/// A spawned light and the emitter it samples.
pub(crate) struct Placed {
    entity: Entity,
    kind: ModelLightKind,
    from: Vec3,
    /// The emitter's facing, with local X and Y spanning a rect light.
    facing: Quat,
    radius: f32,
    /// Tangent of half a sun's angular diameter.
    spread: f32,
    width: f32,
    height: f32,
}

/// Whether any light has an emitter size, which the shot samples.
pub(crate) fn sized(lights: &[ModelLight]) -> bool {
    lights.iter().any(|l| match l.kind {
        ModelLightKind::Sun => l.angle > 0.0,
        ModelLightKind::Point | ModelLightKind::Spot => l.radius > 0.0,
        ModelLightKind::Rect => true,
    })
}

/// The spot cone a rect light's samples use: it stops just short of the
/// emitter plane, and its squared falloff stands in for a softbox's cosine.
const RECT_CONE: f32 = 85.0;

/// Where `light` sits and the way it faces, its default position and target
/// resolved against the bounding sphere.
fn aim(light: &ModelLight, center: Vec3, radius: f32) -> (Vec3, Quat) {
    let target = light.target.map_or(center, Vec3::from);
    let from = light.position.map_or(center + Vec3::new(1.0, 2.0, 2.0) * radius, Vec3::from);
    let forward = (target - from).try_normalize().unwrap_or(Vec3::NEG_Y);
    let up = if forward.y.abs() > 0.999 { Vec3::Z } else { Vec3::Y };
    (from, Transform::default().looking_to(forward, up).rotation)
}

/// Spawns `lights` under `root`, resolving default positions and targets
/// against the bounding sphere.
pub(crate) fn spawn(world: &mut World, root: Entity, lights: &[ModelLight], center: Vec3, radius: f32) -> Vec<Placed> {
    lights
        .iter()
        .map(|light| {
            let (from, facing) = aim(light, center, radius);
            let color = Color::srgb(light.color[0], light.color[1], light.color[2]);
            let range = if light.range > 0.0 {
                light.range
            } else {
                (10.0 * ((from - center).length() + radius)).max(20.0)
            };
            let transform = Transform::from_translation(from).with_rotation(facing);
            let authored = Authored {
                radius: light.radius.max(0.0),
                rect: light.kind == ModelLightKind::Rect,
            };
            let mut e = world.spawn((transform, authored, ChildOf(root)));
            match light.kind {
                ModelLightKind::Sun => {
                    e.insert((
                        DirectionalLight {
                            color,
                            illuminance: light.intensity,
                            shadow_maps_enabled: light.shadows,
                            ..default()
                        },
                        // The first cascade covers what lies within 4 bounding radii of the
                        // camera, at most Bevy's default 5 m, so a wide ground under a small
                        // subject keeps its contact shadows sharp; the last reaches past the
                        // scene and at least the default 1000 m a glTF sun's shadows reach.
                        CascadeShadowConfigBuilder {
                            first_cascade_far_bound: (radius * 4.0).min(5.0),
                            maximum_distance: (radius * 16.0).max(1000.0),
                            ..default()
                        }
                        .build(),
                    ));
                }
                ModelLightKind::Point => {
                    e.insert(PointLight {
                        color,
                        intensity: light.intensity,
                        range,
                        shadow_maps_enabled: light.shadows,
                        ..default()
                    });
                }
                ModelLightKind::Spot => {
                    let cone = if light.cone > 0.0 { light.cone } else { 60.0 };
                    let outer = (cone / 2.0).clamp(0.5, 89.0).to_radians();
                    e.insert(SpotLight {
                        color,
                        intensity: light.intensity,
                        range,
                        outer_angle: outer,
                        inner_angle: outer * 0.85,
                        shadow_maps_enabled: light.shadows,
                        ..default()
                    });
                }
                ModelLightKind::Rect => {
                    // A spot with no inner cone falls off as ((c - c0) / (1 - c0))²
                    // in the cosine c from its axis. That lobe holds 2π (1 - c0) / 3
                    // steradians of its peak intensity, and Bevy reads a spot's
                    // lumens as a point light's (peak candela = lumens / 4π).
                    let c0 = RECT_CONE.to_radians().cos();
                    e.insert(SpotLight {
                        color,
                        intensity: light.intensity * 6.0 / (1.0 - c0),
                        range,
                        outer_angle: RECT_CONE.to_radians(),
                        inner_angle: 0.0,
                        shadow_maps_enabled: light.shadows,
                        ..default()
                    });
                }
            }
            Placed {
                entity: e.id(),
                kind: light.kind,
                from,
                facing,
                radius: light.radius,
                spread: (light.angle.clamp(0.0, 90.0) / 2.0).to_radians().tan(),
                width: if light.width > 0.0 { light.width } else { 1.0 },
                height: if light.height > 0.0 { light.height } else { 1.0 },
            }
        })
        .collect()
}

/// Moves each light to sample point `at` on its emitter: `square` in the unit
/// square for a rect, `disk` on the unit disk for the others. `None` puts
/// every light at its emitter's centre.
pub(crate) fn place(world: &mut World, placed: &[Placed], at: Option<(Vec2, Vec2)>) {
    for p in placed {
        let (square, disk) = at.unwrap_or((Vec2::splat(0.5), Vec2::ZERO));
        let (right, up) = (p.facing * Vec3::X, p.facing * Vec3::Y);
        let transform = match p.kind {
            ModelLightKind::Sun => {
                let forward = p.facing * Vec3::NEG_Z;
                let tilted = (forward + (right * disk.x + up * disk.y) * p.spread).normalize();
                Transform::from_translation(p.from).looking_to(tilted, up)
            }
            ModelLightKind::Point | ModelLightKind::Spot => {
                let step = (right * disk.x + up * disk.y) * p.radius;
                Transform::from_translation(p.from + step).with_rotation(p.facing)
            }
            ModelLightKind::Rect => {
                let step = right * (square.x - 0.5) * p.width + up * (square.y - 0.5) * p.height;
                Transform::from_translation(p.from + step).with_rotation(p.facing)
            }
        };
        if let Some(mut t) = world.get_mut::<Transform>(p.entity) {
            *t = transform;
        }
    }
}

/// Despawns lights spawned earlier.
pub(crate) fn despawn(world: &mut World, placed: Vec<Placed>) {
    for p in placed {
        world.despawn(p.entity);
    }
}

/// Spawns the ray traced emitters of the rect lights in `lights` under
/// `root`: one-sided quads facing their targets, glowing with flux / (pi
/// area), the radiance that emits the light's lumens.
pub(crate) fn emitters(world: &mut World, root: Entity, lights: &[ModelLight], center: Vec3, radius: f32) -> Vec<Entity> {
    let mut spawned = Vec::new();
    for light in lights.iter().filter(|l| l.kind == ModelLightKind::Rect) {
        let (from, facing) = aim(light, center, radius);
        let w = if light.width > 0.0 { light.width } else { 1.0 };
        let h = if light.height > 0.0 { light.height } else { 1.0 };
        // The quad faces +Z, and the light shines down its local -Z.
        let rotation = facing * Quat::from_rotation_y(PI);
        let radiance = light.intensity / (PI * w * h);
        let Some(mesh) = crate::hero::solari_copy(&Mesh::from(Rectangle::new(w, h))) else { continue };
        let color = LinearRgba::from(Color::srgb(light.color[0], light.color[1], light.color[2]));
        let material = StandardMaterial {
            base_color: Color::BLACK,
            emissive: color * radiance,
            perceptual_roughness: 1.0,
            ..default()
        };
        let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
        let material = world.resource_mut::<Assets<StandardMaterial>>().add(material);
        let transform = Transform::from_translation(from).with_rotation(rotation);
        spawned.push(world.spawn((RaytracingMesh3d(mesh), MeshMaterial3d(material), transform, ChildOf(root))).id());
    }
    spawned
}
