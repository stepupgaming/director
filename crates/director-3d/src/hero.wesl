// The Hero level: unbiased path tracing over Solari's ray traced scene.
// Each dispatch traces `hero.paths` paths per pixel for one sample of a shot
// and writes their mean, premultiplied, into the view target. Camera rays
// that miss every surface are transparent unless the shot paints the
// background. Paths are seeded by pixel, sample and path index, so a shot's
// frame depends only on its inputs. Solari has no point or spot lights, so
// each hit also samples every `lights` entry with a shadow ray.
// `hero_guided` traces the same paths and instead writes the guides of the
// edge-aware filter in `denoise.wesl`: radiance over albedo with its
// variance, and the mean first-hit position, normal and albedo.

import bevy_core_pipeline::tonemapping::tonemapping_luminance as luminance;
import bevy_pbr::render::utils::{rand_f, rand_vec2f};
import bevy_render::view::View;
import bevy_solari::scene::brdf::{evaluate_brdf, evaluate_and_sample_brdf, brdf_pdf, F_AB};
import bevy_solari::scene::sampling::{sample_random_light, random_emissive_light_pdf, power_heuristic, sample_environment_map_light};
import bevy_solari::scene::bindings::{trace_ray, resolve_ray_hit_full, ResolvedRayHitFull, RAY_T_MIN, RAY_T_MAX, MIRROR_ROUGHNESS_THRESHOLD};

enable wgpu_ray_query;

struct Hero {
    sample: u32,
    paths: u32,
    background: u32,
    bounces: u32,
    lights: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

// A point or spot light in Bevy's raster units: `color` is linear colour
// times candela, and the cone falls off as saturate(cos * scale + offset)^2
// about `direction` (scale 0 and offset 1 for a point light).
struct HeroLight {
    position: vec3<f32>,
    radius: f32,
    direction: vec3<f32>,
    scale: f32,
    color: vec3<f32>,
    offset: f32,
    range: f32,
    pad0: f32,
    pad1: f32,
    pad2: f32,
}

@group(1) @binding(0) var view_output: texture_storage_2d<rgba16float, write>;
@group(1) @binding(1) var<uniform> view: View;
@group(1) @binding(2) var<uniform> hero: Hero;
@group(1) @binding(3) var<storage, read> lights: array<HeroLight>;

@group(2) @binding(0) var guide_radiance: texture_storage_2d<rgba32float, write>;
@group(2) @binding(1) var guide_position: texture_storage_2d<rgba32float, write>;
@group(2) @binding(2) var guide_normal: texture_storage_2d<rgba16float, write>;
@group(2) @binding(3) var guide_albedo: texture_storage_2d<rgba16float, write>;

// A path's first surface. `hit` is 0 when the camera ray misses.
struct First {
    position: vec3<f32>,
    normal: vec3<f32>,
    albedo: vec3<f32>,
    hit: f32,
}

// The albedo the filter divides out: the base colour, kept off black so
// dividing by it stays finite.
const MIN_ALBEDO: f32 = 0.02;

fn hash(x: u32) -> u32 {
    let state = x * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

// The light the point and spot lights send from `hit` toward `wo`, each
// from a point on its disc facing the hit when it has a radius, with Bevy's
// inverse square falloff, range window and cone.
fn sample_lights(hit: ResolvedRayHitFull, wo: vec3<f32>, F_ab: vec2<f32>, rng: ptr<function, u32>) -> vec3<f32> {
    var sum = vec3(0.0);
    for (var i = 0u; i < hero.lights; i++) {
        let light = lights[i];
        var position = light.position;
        if light.radius > 0.0 {
            let axis = normalize(hit.world_position - light.position);
            let helper = select(vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), abs(axis.x) > 0.9);
            let u = normalize(cross(helper, axis));
            let v = cross(axis, u);
            let r = rand_vec2f(rng);
            let rho = light.radius * sqrt(r.x);
            let phi = 6.2831853 * r.y;
            position += (u * cos(phi) + v * sin(phi)) * rho;
        }
        let to_light = position - hit.world_position;
        let d2 = dot(to_light, to_light);
        let distance = sqrt(d2);
        let wi = to_light / max(distance, 1e-6);
        if dot(hit.world_normal, wi) <= 0.0 {
            continue;
        }
        let factor = d2 / max(light.range * light.range, 1e-8);
        let window = saturate(1.0 - factor * factor);
        let cone = saturate(dot(light.direction, -wi) * light.scale + light.offset);
        let attenuation = window * window * cone * cone / max(d2, 0.0001);
        if attenuation <= 0.0 {
            continue;
        }
        let origin = hit.world_position + hit.geometric_world_normal * RAY_T_MIN;
        let shadow = trace_ray(origin, wi, RAY_T_MIN, distance - RAY_T_MIN, RAY_FLAG_TERMINATE_ON_FIRST_HIT);
        if shadow.kind != RAY_QUERY_INTERSECTION_NONE {
            continue;
        }
        sum += evaluate_brdf(wo, wi, hit.world_normal, hit.material, F_ab) * light.color * attenuation;
    }
    return sum;
}

// One path from the camera through `pixel`: its radiance in rgb, and 1 in
// alpha when it covers the pixel. `first` receives its first surface.
fn trace_path(pixel: vec2<f32>, rng: ptr<function, u32>, first: ptr<function, First>) -> vec4<f32> {
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
        if bounce == 0u {
            *first = First(ray_hit.world_position, ray_hit.world_normal, max(ray_hit.material.base_color, vec3(MIN_ALBEDO)), 1.0);
        }
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
            radiance += throughput * sample_lights(ray_hit, wo, F_ab, rng);
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
        var first = First(vec3(0.0), vec3(0.0), vec3(1.0), 0.0);
        sum += trace_path(vec2<f32>(global_id.xy), &rng, &first);
    }
    let mean = sum / f32(hero.paths);
    // Rgba16Float tops out near 65504.
    let rgb = min(mean.rgb * view.exposure, vec3(65000.0));
    textureStore(view_output, global_id.xy, vec4(rgb, mean.a));
}

// The same paths as `hero_trace`, written as the filter's guides. Radiance
// is exposed and divided by the mean first-hit albedo, so the filter blurs
// lighting and keeps texture. Its alpha is the variance of the mean's
// luminance across the paths. Position and normal average the paths that
// hit, and the position's alpha is the fraction that hit. The albedo's
// alpha is the pixel's coverage.
@compute @workgroup_size(8, 8, 1)
fn hero_guided(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if any(global_id.xy >= vec2u(view.viewport.zw)) {
        return;
    }
    let pixel_index = global_id.x + global_id.y * u32(view.viewport.z);
    var sum = vec4(0.0);
    var moments = vec2(0.0);
    var position = vec3(0.0);
    var normal = vec3(0.0);
    var albedo = vec3(0.0);
    var hits = 0.0;
    for (var path = 0u; path < hero.paths; path++) {
        var rng = hash(pixel_index ^ hash(hero.sample * hero.paths + path));
        var first = First(vec3(0.0), vec3(0.0), vec3(1.0), 0.0);
        let radiance = trace_path(vec2<f32>(global_id.xy), &rng, &first);
        sum += radiance;
        let l = luminance(min(radiance.rgb * view.exposure, vec3(65000.0)));
        moments += vec2(l, l * l);
        position += first.position * first.hit;
        normal += first.normal * first.hit;
        albedo += first.albedo * first.hit;
        hits += first.hit;
    }
    let n = f32(hero.paths);
    let mean = sum / n;
    let rgb = min(mean.rgb * view.exposure, vec3(65000.0));
    let a = select(vec3(1.0), albedo / max(hits, 1.0), hits > 0.0);
    let m = moments / n;
    let variance = max(m.y - m.x * m.x, 0.0) / n / pow(max(luminance(a), MIN_ALBEDO), 2.0);
    let length_n = length(normal);
    textureStore(guide_radiance, global_id.xy, vec4(rgb / a, variance));
    textureStore(guide_position, global_id.xy, vec4(position / max(hits, 1.0), hits / n));
    textureStore(guide_normal, global_id.xy, vec4(select(vec3(0.0), normal / length_n, length_n > 1e-6), 0.0));
    textureStore(guide_albedo, global_id.xy, vec4(a, mean.a));
}
