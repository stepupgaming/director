// One pass of the Hero level's edge-aware à-trous filter. `hero_guided`
// writes the guides: exposed radiance over albedo with the variance of its
// luminance, and each pixel's mean first-hit position, normal and albedo.
// Each pass blurs radiance with a 5x5 B3 spline kernel whose taps sit
// `step` pixels apart, weighted down across normals, off the centre's
// tangent plane, across hit fractions and across luminance differences
// beyond a few standard deviations. Variance carries through with squared
// weights, so later passes trust the smoother estimate more. The last pass
// multiplies the albedo back and writes the view target with the pixel's
// own coverage. A pixel whose camera rays all missed keeps its value. The
// filter reads only this sample's guides, so a frame still depends only on
// its inputs.

import bevy_core_pipeline::tonemapping::tonemapping_luminance as luminance;
import bevy_render::view::View;

struct Step {
    step: i32,
    last: u32,
    pad0: u32,
    pad1: u32,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> setting: Step;
@group(0) @binding(2) var radiance_in: texture_2d<f32>;
@group(0) @binding(3) var position: texture_2d<f32>;
@group(0) @binding(4) var normal: texture_2d<f32>;
@group(0) @binding(5) var albedo: texture_2d<f32>;
@group(0) @binding(6) var radiance_out: texture_storage_2d<rgba32float, write>;
@group(0) @binding(7) var view_output: texture_storage_2d<rgba16float, write>;

// The normal weight's power, the luminance tolerance in standard deviations
// and the tangent plane tolerance in pixel footprints at the centre.
const SIGMA_NORMAL: f32 = 128.0;
const SIGMA_LUMINANCE: f32 = 4.0;
const SIGMA_PLANE: f32 = 2.0;

fn kernel(i: i32) -> f32 {
    return select(select(0.0625, 0.25, abs(i) == 1), 0.375, i == 0);
}

@compute @workgroup_size(8, 8, 1)
fn atrous(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let size = vec2<i32>(view.viewport.zw);
    let p = vec2<i32>(global_id.xy);
    if any(p >= size) {
        return;
    }
    let center = textureLoad(radiance_in, p, 0);
    let x_p = textureLoad(position, p, 0);
    let n_p = textureLoad(normal, p, 0).xyz;
    var out = center;
    if x_p.w > 0.0 {
        // The variance blurred over 3x3 sets the luminance tolerance.
        var variance = 0.0;
        for (var dy = -1; dy <= 1; dy++) {
            for (var dx = -1; dx <= 1; dx++) {
                let q = clamp(p + vec2(dx, dy), vec2(0), size - 1);
                variance += select(0.25, 0.5, dx == 0) * select(0.25, 0.5, dy == 0) * textureLoad(radiance_in, q, 0).a;
            }
        }
        let tolerance = SIGMA_LUMINANCE * sqrt(variance) + 1e-6;
        let l_p = luminance(center.rgb);
        // The width one pixel covers at the centre's distance.
        let footprint = SIGMA_PLANE * 2.0 * length(x_p.xyz - view.world_position) / (view.clip_from_view[1][1] * f32(size.y));
        let w_center = kernel(0) * kernel(0);
        var sum = center.rgb * w_center;
        var variance_sum = center.a * w_center * w_center;
        var weight_sum = w_center;
        for (var j = -2; j <= 2; j++) {
            for (var i = -2; i <= 2; i++) {
                if i == 0 && j == 0 {
                    continue;
                }
                let q = p + vec2(i, j) * setting.step;
                if any(q < vec2(0)) || any(q >= size) {
                    continue;
                }
                let x_q = textureLoad(position, q, 0);
                if x_q.w <= 0.0 {
                    continue;
                }
                let c_q = textureLoad(radiance_in, q, 0);
                let n_q = textureLoad(normal, q, 0).xyz;
                let w_normal = pow(max(dot(n_p, n_q), 0.0), SIGMA_NORMAL);
                let w_plane = exp(-abs(dot(n_p, x_q.xyz - x_p.xyz)) / max(footprint, 1e-6));
                let w_hit = saturate(1.0 - 4.0 * abs(x_p.w - x_q.w));
                let w_luminance = exp(-abs(l_p - luminance(c_q.rgb)) / tolerance);
                let w = kernel(i) * kernel(j) * w_normal * w_plane * w_hit * w_luminance;
                sum += c_q.rgb * w;
                variance_sum += c_q.a * w * w;
                weight_sum += w;
            }
        }
        out = vec4(sum / weight_sum, variance_sum / (weight_sum * weight_sum));
    }
    if setting.last == 0u {
        textureStore(radiance_out, p, out);
        return;
    }
    let a = textureLoad(albedo, p, 0);
    textureStore(view_output, p, vec4(min(out.rgb * a.rgb, vec3(65000.0)), a.a));
}
