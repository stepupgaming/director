//! Gaussian splats drawn display-referred, after tonemapping.
//!
//! Splat captures hold their camera's display colours, so a filmic curve or
//! bloom would change them. `DisplayPlugin` moves each view's splat items
//! out of the transparent phase once it is sorted, so meshes alone feed
//! bloom and the filmic curve, then draws them over the tonemapped image
//! against the main pass depth, before SMAA.

use std::collections::HashMap;

use bevy::{
    anti_alias::smaa::smaa,
    camera::Viewport,
    core_pipeline::{
        core_3d::Transparent3d,
        schedule::{Core3d, Core3dSystems},
        tonemapping::tonemapping,
    },
    prelude::*,
    render::{
        camera::ExtractedCamera,
        render_phase::{sort_phase_system, SortedRenderPhase, ViewSortedRenderPhases},
        render_resource::{RenderPassDescriptor, StoreOp},
        renderer::{RenderContext, ViewQuery},
        view::{ExtractedView, RetainedViewEntity, ViewDepthStencilTexture, ViewTarget},
        Render, RenderApp, RenderSystems,
    },
};
use bevy_gaussian_splatting::PlanarGaussian3dHandle;

pub(crate) struct DisplayPlugin;

impl Plugin for DisplayPlugin {
    fn build(&self, app: &mut App) {
        let Some(render) = app.get_sub_app_mut(RenderApp) else { return };
        render
            .init_resource::<Splats>()
            .add_systems(
                Render,
                stash
                    .in_set(RenderSystems::PhaseSort)
                    .after(sort_phase_system::<Transparent3d>),
            )
            .add_systems(
                Core3d,
                display
                    .after(tonemapping)
                    .before(smaa)
                    .in_set(Core3dSystems::PostProcess),
            );
    }
}

/// Each view's sorted splat items for this frame.
#[derive(Resource, Default)]
struct Splats(HashMap<RetainedViewEntity, SortedRenderPhase<Transparent3d>>);

fn stash(
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    clouds: Query<(), With<PlanarGaussian3dHandle>>,
    mut splats: ResMut<Splats>,
) {
    splats.0.clear();
    for (view, phase) in phases.iter_mut() {
        let mut moved = SortedRenderPhase::default();
        let keys: Vec<_> = phase.items.keys().filter(|key| clouds.contains(key.0)).copied().collect();
        for key in keys {
            let item = phase.items.shift_remove(&key).expect("a listed item");
            moved.items.insert(key, item);
        }
        if !moved.items.is_empty() {
            splats.0.insert(*view, moved);
        }
    }
}

fn display(
    world: &World,
    view: ViewQuery<(&ExtractedCamera, &ExtractedView, &ViewTarget, &ViewDepthStencilTexture)>,
    splats: Res<Splats>,
    mut ctx: RenderContext,
) {
    let view_entity = view.entity();
    let (camera, extracted, target, depth) = view.into_inner();
    let Some(phase) = splats.0.get(&extracted.retained_view_entity) else { return };
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("director_display_splats"),
        color_attachments: &[Some(target.get_color_attachment())],
        depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Some(viewport) = Viewport::from_viewport_and_override(camera.viewport.as_ref(), None) {
        pass.set_camera_viewport(&viewport);
    }
    if let Err(err) = phase.render(&mut pass, world, view_entity) {
        error!("drawing splats after tonemapping: {err:?}");
    }
}
