// The Hero level: unbiased path tracing over Solari's ray traced scene.
// Each dispatch traces `hero.paths` paths per pixel for one sample of a shot
// and writes their mean, premultiplied, into the view target. Camera rays
// that miss every surface are transparent unless the shot paints the
// background. Paths are seeded by pixel, sample and path index, so a shot's
// frame depends only on its inputs.

import bevy_core_pipeline::tonemapping::tonemapping_luminance as luminance;
import bevy_pbr::render::utils::{rand_f, rand_vec2f};
import bevy_render::view::View;
import bevy_solari::scene::brdf::{evaluate_brdf, evaluate_and_sample_brdf, brdf_pdf, F_AB};
import bevy_solari::scene::sampling::{sample_random_light, random_emissive_light_pdf, power_heuristic, sample_environment_map_light};
import bevy_solari::scene::bindings::{trace_ray, resolve_ray_hit_full, RAY_T_MIN, RAY_T_MAX, MIRROR_ROUGHNESS_THRESHOLD};

enable wgpu_ray_query;

struct Hero {
    sample: u32,
    paths: u32,
    background: u32,
    bounces: u32,
}

@group(1) @binding(0) var view_output: texture_storage_2d<rgba16float, write>;
@group(1) @binding(1) var<uniform> view: View;
@group(1) @binding(2) var<uniform> hero: Hero;

fn hash(x: u32) -> u32 {
    let state = x * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

// One path from the camera through `pixel`: its radiance in rgb, and 1 in
// alpha when it covers the pixel.
fn trace_path(pixel: vec2<f32>, rng: ptr<function, u32>) -> vec4<f32> {
    let pixel_uv = (pixel + rand_vec2f(rng)) / view.viewport.zw;
    let pixel_ndc = (pixel_uv * 2.0) - 1.0;
    let primary_ray_target = view.world_from_clip * vec4(pixel_ndc.x, -pixel_ndc.y, 1.0, 1.0);
    var ray_origin = view.world_position;
    var ray_direction = normalize((primary_ray_target.xyz / primary_ray_target.w) - ray_origin);
    var ray_t_min = 0.0;

    var radiance = vec3(0.0);
    var throughput = vec3(1.0);
    var p_bounce = 0.0;
    var bounce = 0u;
    loop {
        let ray = trace_ray(ray_origin, ray_direction, ray_t_min, RAY_T_MAX, RAY_FLAG_NONE);
        if ray.kind == RAY_QUERY_INTERSECTION_NONE {
            if bounce == 0u {
                if hero.background == 0u {
                    return vec4(0.0);
                }
                return vec4(sample_environment_map_light(ray_direction), 1.0);
            }
            radiance += throughput * sample_environment_map_light(ray_direction);
            break;
        }
        let ray_hit = resolve_ray_hit_full(ray);
        let wo = -ray_direction;
        let NdotV = max(dot(ray_hit.world_normal, wo), 0.0001);
        let F_ab = F_AB(ray_hit.material.perceptual_roughness, NdotV);

        // Emitted light, weighted against the light sampling below.
        var mis_weight = 1.0;
        if p_bounce != 0.0 {
            let p_light = random_emissive_light_pdf(ray_hit, ray.t, NdotV);
            mis_weight = power_heuristic(p_bounce, p_light);
        }
        if dot(ray_hit.world_normal, wo) > 0.0 {
            radiance += mis_weight * throughput * ray_hit.material.emissive;
        }

        // Next event estimation, except on mirrors.
        let is_perfectly_specular = ray_hit.material.roughness <= MIRROR_ROUGHNESS_THRESHOLD && ray_hit.material.metallic > 0.9999;
        if !is_perfectly_specular {
            let direct_lighting = sample_random_light(ray_hit.world_position, ray_hit.world_normal, rng);
            mis_weight = 1.0;
            if direct_lighting.brdf_rays_can_hit {
                let pdf_of_bounce = brdf_pdf(wo, direct_lighting.wi, ray_hit.world_normal, ray_hit.material, F_ab);
                mis_weight = power_heuristic(1.0 / direct_lighting.inverse_solid_angle_pdf, pdf_of_bounce);
            }
            let direct_lighting_brdf = evaluate_brdf(wo, direct_lighting.wi, ray_hit.world_normal, ray_hit.material, F_ab);
            radiance += mis_weight * throughput * direct_lighting.radiance * direct_lighting.inverse_pdf * direct_lighting_brdf;
        }

        bounce += 1u;
        if bounce > hero.bounces { break; }
        let next_bounce = evaluate_and_sample_brdf(wo, ray_hit.world_normal, ray_hit.material, F_ab, rng);
        if next_bounce.pdf == 0.0 { break; }
        ray_direction = next_bounce.wi;
        ray_origin = ray_hit.world_position + (ray_hit.geometric_world_normal * RAY_T_MIN);
        ray_t_min = RAY_T_MIN;
        p_bounce = next_bounce.pdf;
        throughput *= next_bounce.throughput;

        // Russian roulette after the first bounce.
        let rr = saturate(luminance(throughput));
        if rand_f(rng) >= rr { break; }
        throughput /= rr;
    }
    return vec4(radiance, 1.0);
}

@compute @workgroup_size(8, 8, 1)
fn hero_trace(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if any(global_id.xy >= vec2u(view.viewport.zw)) {
        return;
    }
    let pixel_index = global_id.x + global_id.y * u32(view.viewport.z);
    var sum = vec4(0.0);
    for (var path = 0u; path < hero.paths; path++) {
        var rng = hash(pixel_index ^ hash(hero.sample * hero.paths + path));
        sum += trace_path(vec2<f32>(global_id.xy), &rng);
    }
    let mean = sum / f32(hero.paths);
    // Rgba16Float tops out near 65504.
    let rgb = min(mean.rgb * view.exposure, vec3(65000.0));
    textureStore(view_output, global_id.xy, vec4(rgb, mean.a));
}
