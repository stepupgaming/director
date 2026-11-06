// A shadow catcher: a surface that shows only the shadows cast on it.
// Each fragment lights a white diffuse surface with every point, spot and
// directional light, with and without its shadow map, plus the probe's and
// the ambient diffuse light, and writes black as premultiplied alpha. Pages
// and footage blend display-encoded, where a display curve near a 2.2 power
// turns a linear light ratio r into r^(1/2.2), so the alpha is 1 - r^(1/2.2)
// with r the shown fraction of the light. Whatever lies behind a transparent
// model then darkens where the model's shadows fall, as far as the light it
// loses, and shows unchanged elsewhere.

import bevy_core_pipeline::tonemapping::tonemapping_luminance as luminance;
import bevy_pbr::render::forward_io::VertexOutput;
import bevy_pbr::render::mesh_view_bindings as view_bindings;
import bevy_pbr::render::mesh_view_types;
import bevy_pbr::render::pbr_lighting as lighting;
import bevy_pbr::render::pbr_lighting::LAYER_BASE;
import bevy_pbr::render::clustered_forward as clustering;
import bevy_pbr::render::shadows;
import bevy_pbr::render::pbr_ambient as ambient;
@if(ENVIRONMENT_MAP) import bevy_pbr::light_probe::environment_map;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let view = view_bindings::view;
    let orthographic = view.clip_from_view[3].w == 1.0;
    var V = normalize(view.world_position.xyz - in.world_position.xyz);
    if orthographic {
        V = normalize(vec3(view.world_from_view[0].z, view.world_from_view[1].z, view.world_from_view[2].z));
    }
    var N = normalize(in.world_normal);
    if !front {
        N = -N;
    }

    var input: lighting::LightingInput;
    input.layers[LAYER_BASE].NdotV = max(dot(N, V), 0.0001);
    input.layers[LAYER_BASE].N = N;
    input.layers[LAYER_BASE].R = reflect(-V, N);
    input.layers[LAYER_BASE].perceptual_roughness = 1.0;
    input.layers[LAYER_BASE].roughness = 1.0;
    input.P = in.world_position.xyz;
    input.V = V;
    input.diffuse_color = vec3(1.0);
    input.metallic = 0.0;
    input.F0_dielectric = vec3(0.0);
    input.F0_metallic = vec3(0.0);
    input.F_ab = vec2(1.0, 0.0);

    let view_z = dot(vec4(
        view.view_from_world[0].z,
        view.view_from_world[1].z,
        view.view_from_world[2].z,
        view.view_from_world[3].z,
    ), in.world_position);
    let cluster = clustering::view_fragment_cluster_index(in.position.xy, view_z, orthographic);
    var ranges = clustering::unpack_clusterable_object_index_ranges(cluster);

    var lit = 0.0;
    var shown = 0.0;
    for (var i = ranges.first_point_light_index_offset; i < ranges.first_spot_light_index_offset; i += 1u) {
        let id = clustering::get_clusterable_object_id(i);
        let light = luminance(lighting::point_light(id, &input, true, true));
        var shadow = 1.0;
        if (view_bindings::clustered_lights.data[id].flags & mesh_view_types::POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u {
            shadow = shadows::fetch_point_shadow(id, in.world_position, N, in.position.xy);
        }
        lit += light;
        shown += light * shadow;
    }
    for (var i = ranges.first_spot_light_index_offset; i < ranges.first_rect_light_index_offset; i += 1u) {
        let id = clustering::get_clusterable_object_id(i);
        let light = luminance(lighting::spot_light(id, &input, true));
        var shadow = 1.0;
        if (view_bindings::clustered_lights.data[id].flags & mesh_view_types::POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u {
            shadow = shadows::fetch_spot_shadow(
                id,
                in.world_position,
                N,
                view_bindings::clustered_lights.data[id].shadow_map_near_z,
                in.position.xy,
            );
        }
        lit += light;
        shown += light * shadow;
    }
    for (var i = 0u; i < view_bindings::lights.n_directional_lights; i += 1u) {
        let light = luminance(lighting::directional_light(i, &input, true));
        var shadow = 1.0;
        if (view_bindings::lights.directional_lights[i].flags & mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u {
            shadow = shadows::fetch_directional_shadow(i, in.world_position, N, view_z, in.position.xy);
        }
        lit += light;
        shown += light * shadow;
    }

    var indirect = ambient::ambient_light(in.world_position, N, V, input.layers[LAYER_BASE].NdotV, vec3(1.0), vec3(0.0), 1.0, vec3(1.0));
@if(ENVIRONMENT_MAP) {
    indirect += environment_map::environment_map_light(&input, &ranges, false).diffuse;
}
    lit += luminance(indirect);
    shown += luminance(indirect);

    let alpha = select(0.0, saturate(1.0 - pow(shown / lit, 1.0 / 2.2)), lit > 0.0);
    return vec4(0.0, 0.0, 0.0, alpha);
}
