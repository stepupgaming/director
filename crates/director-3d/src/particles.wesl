// Closed-form particles. Every particle's state is a pure function of the
// emitter, the seed, its index and the element time in `params.timing.x`,
// so a frame needs nothing from the frames before it and each sample of a
// shutter places the particles at its own time.
//
// The mesh holds one copy of a card or a shape per slot, with the slot in
// uv_b.x. Slot j shows particle n = j + k * slots, the latest particle of
// that residue born by the time on show: slots hold every particle that can
// be alive at once. A dead or unborn slot collapses to one point outside
// the clip volume.
//
// Position follows a ballistic path with linear drag k, closed form:
// x = p0 + v0 f + g (t - f) / k with f = (1 - e^(-k t)) / k, the drag-free
// p0 + v0 t + g t^2 / 2 as k goes to 0. Turbulence offsets that path by
// the change of a curl noise field sampled at the particle's path through
// space and a slow drift through the noise, so nearby particles swirl
// together and the offset stays bounded.

import bevy_pbr::render::mesh_view_bindings as view_bindings;
import bevy_pbr::render::mesh_functions;
import bevy_pbr::render::mesh_types;
import bevy_pbr::render::view_transformations::position_world_to_clip;
import bevy_pbr::render::pbr_types;
import bevy_pbr::render::pbr_functions;

struct Params {
    // xyz centre, w shape: 0 point, 1 sphere, 2 box, 3 surface.
    emitter: vec4<f32>,
    // xyz box half extents or x sphere radius, w surface point count.
    extent: vec4<f32>,
    // xyz unit axis (zero leaves along surface normals), w spread in radians.
    direction: vec4<f32>,
    // xyz acceleration, w drag per second.
    gravity: vec4<f32>,
    // speed, speed jitter, turbulence metres, turbulence scale metres.
    motion: vec4<f32>,
    // element seconds, rate per second (0 bursts), lifetime, lifetime jitter.
    timing: vec4<f32>,
    // start, stop, burst count, slots.
    window: vec4<f32>,
    // size at birth, size at death, size jitter, spin radians per second.
    size: vec4<f32>,
    // linear colour at birth, opacity.
    color: vec4<f32>,
    // linear colour at death, emissive strength.
    color_end: vec4<f32>,
    // fade in seconds, fade out seconds, 0 card 1 mesh, 1 / mesh diameter.
    fade: vec4<f32>,
    // seed.
    seed: vec4<u32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: Params;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var sprite: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var sprite_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var<storage, read> surface: array<vec4<f32>>;

const TAU: f32 = 6.283185307179586;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv_b: vec2<f32>,
}

struct Particle {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    // Linear colour and opacity.
    @location(3) color: vec4<f32>,
}

fn pcg(v: u32) -> u32 {
    let state = v * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

// A uniform number in [0, 1) for particle n and channel c.
fn rand(n: u32, c: u32) -> f32 {
    return f32(pcg(pcg(pcg(params.seed.x) ^ n) + c) >> 8u) / 16777216.0;
}

// Value noise in [-1, 1] with quintic smoothing.
fn lattice(c: vec3<i32>, salt: u32) -> f32 {
    let u = bitcast<vec3<u32>>(c);
    let h = pcg(u.x ^ pcg(u.y ^ pcg(u.z ^ pcg(salt ^ params.seed.x))));
    return f32(h >> 8u) / 8388608.0 - 1.0;
}

fn noise(q: vec3<f32>, salt: u32) -> f32 {
    let i = vec3<i32>(floor(q));
    let f = fract(q);
    let w = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let x00 = mix(lattice(i, salt), lattice(i + vec3(1, 0, 0), salt), w.x);
    let x10 = mix(lattice(i + vec3(0, 1, 0), salt), lattice(i + vec3(1, 1, 0), salt), w.x);
    let x01 = mix(lattice(i + vec3(0, 0, 1), salt), lattice(i + vec3(1, 0, 1), salt), w.x);
    let x11 = mix(lattice(i + vec3(0, 1, 1), salt), lattice(i + vec3(1, 1, 1), salt), w.x);
    return mix(mix(x00, x10, w.y), mix(x01, x11, w.y), w.z);
}

// The curl of a noise vector potential: a divergence-free swirl of about
// unit size.
fn curl(q: vec3<f32>) -> vec3<f32> {
    let e = 0.05;
    let dx = vec3(e, 0.0, 0.0);
    let dy = vec3(0.0, e, 0.0);
    let dz = vec3(0.0, 0.0, e);
    let dpx_dy = noise(q + dy, 1u) - noise(q - dy, 1u);
    let dpx_dz = noise(q + dz, 1u) - noise(q - dz, 1u);
    let dpy_dx = noise(q + dx, 2u) - noise(q - dx, 2u);
    let dpy_dz = noise(q + dz, 2u) - noise(q - dz, 2u);
    let dpz_dx = noise(q + dx, 3u) - noise(q - dx, 3u);
    let dpz_dy = noise(q + dy, 3u) - noise(q - dy, 3u);
    return vec3(dpz_dy - dpy_dz, dpx_dz - dpz_dx, dpy_dx - dpx_dy) / (2.0 * e) * 0.5;
}

// Two unit vectors perpendicular to unit `n` and each other.
fn basis(n: vec3<f32>) -> mat2x3<f32> {
    let s = select(-1.0, 1.0, n.z >= 0.0);
    let a = -1.0 / (s + n.z);
    let b = n.x * n.y * a;
    return mat2x3<f32>(vec3(1.0 + s * n.x * n.x * a, s * b, -s * n.x), vec3(b, s + n.y * n.y * a, -n.y));
}

// A unit vector from two uniform numbers.
fn sphere(u: f32, v: f32) -> vec3<f32> {
    let z = 1.0 - 2.0 * u;
    let r = sqrt(max(0.0, 1.0 - z * z));
    let phi = TAU * v;
    return vec3(r * cos(phi), r * sin(phi), z);
}

// Rotates `p` by `angle` radians about unit `axis`.
fn rotate(p: vec3<f32>, axis: vec3<f32>, angle: f32) -> vec3<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return p * c + cross(axis, p) * s + axis * dot(axis, p) * (1.0 - c);
}

fn dead() -> Particle {
    var out: Particle;
    out.position = vec4(2.0, 2.0, 2.0, 1.0);
    out.world_position = vec4(0.0, 0.0, 0.0, 1.0);
    out.world_normal = vec3(0.0, 0.0, 1.0);
    out.uv = vec2(0.0);
    out.color = vec4(0.0);
    return out;
}

@vertex
fn vertex(v: Vertex) -> Particle {
    let slot = u32(v.uv_b.x + 0.5);
    let slots = u32(params.window.w);
    let t = params.timing.x - params.window.x;
    if t < 0.0 {
        return dead();
    }
    var n = slot;
    var age = t;
    let rate = params.timing.y;
    if rate > 0.0 {
        let until = min(t, params.window.y - params.window.x);
        let latest = u32(floor(until * rate));
        if latest < slot {
            return dead();
        }
        n = slot + slots * ((latest - slot) / slots);
        age = max(t - f32(n) / rate, 0.0);
    }
    let life = params.timing.z * (1.0 - params.timing.w * rand(n, 0u));
    if age >= life {
        return dead();
    }

    // Birth place and the axis it leaves along.
    var p0 = params.emitter.xyz;
    var axis = params.direction.xyz;
    let shape = u32(params.emitter.w + 0.5);
    if shape == 1u {
        p0 += sphere(rand(n, 1u), rand(n, 2u)) * params.extent.x * pow(rand(n, 3u), 1.0 / 3.0);
    } else if shape == 2u {
        p0 += (vec3(rand(n, 1u), rand(n, 2u), rand(n, 3u)) * 2.0 - 1.0) * params.extent.xyz;
    } else if shape == 3u {
        let count = u32(params.extent.w);
        let i = min(u32(rand(n, 1u) * f32(count)), count - 1u);
        p0 = surface[2u * i].xyz;
        if dot(axis, axis) == 0.0 {
            axis = surface[2u * i + 1u].xyz;
        }
    }
    if dot(axis, axis) == 0.0 {
        axis = vec3(0.0, 1.0, 0.0);
    }

    // Launch velocity inside a cone of half-angle `spread` around the axis.
    let cos_t = 1.0 - rand(n, 4u) * (1.0 - cos(params.direction.w));
    let sin_t = sqrt(max(0.0, 1.0 - cos_t * cos_t));
    let phi = TAU * rand(n, 5u);
    let b = basis(axis);
    let heading = b[0] * (sin_t * cos(phi)) + b[1] * (sin_t * sin(phi)) + axis * cos_t;
    let speed = params.motion.x * (1.0 + params.motion.y * (2.0 * rand(n, 6u) - 1.0));
    let v0 = heading * speed;

    // Ballistic path under gravity and linear drag.
    let k = params.gravity.w;
    var f = age - 0.5 * k * age * age;
    var h = 0.5 * age * age - k * age * age * age / 6.0;
    if k * age > 1e-3 {
        f = (1.0 - exp(-k * age)) / k;
        h = (age - f) / k;
    }
    var p = p0 + v0 * f + params.gravity.xyz * h;

    let amount = params.motion.z;
    if amount > 0.0 {
        let scale = params.motion.w;
        let drift = vec3(0.23, 0.31, 0.17) * age;
        p += amount * (curl(p / scale + drift) - curl(p0 / scale));
    }

    let u = age / life;
    let size = mix(params.size.x, params.size.y, u) * (1.0 + params.size.z * (2.0 * rand(n, 7u) - 1.0));
    let angle = TAU * rand(n, 8u) + params.size.w * age;
    var fade = 1.0;
    if params.fade.x > 0.0 {
        fade *= saturate(age / params.fade.x);
    }
    if params.fade.y > 0.0 {
        fade *= saturate((life - age) / params.fade.y);
    }

    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    let view = view_bindings::view;
    var world: vec3<f32>;
    var normal: vec3<f32>;
    if params.fade.z == 0.0 {
        // A card facing the camera, turned in its plane.
        let center = (world_from_local * vec4(p, 1.0)).xyz;
        let right = normalize(view.world_from_view[0].xyz);
        let up = normalize(view.world_from_view[1].xyz);
        let corner = vec2(v.uv.x - 0.5, 0.5 - v.uv.y) * size;
        let c = cos(angle);
        let s = sin(angle);
        let turned = vec2(corner.x * c - corner.y * s, corner.x * s + corner.y * c);
        world = center + right * turned.x + up * turned.y;
        normal = normalize(view.world_from_view[2].xyz);
    } else {
        // A shape tumbling about its own random axis.
        let spin_axis = sphere(rand(n, 9u), rand(n, 10u));
        let local = rotate(v.position * size * params.fade.w, spin_axis, angle);
        world = (world_from_local * vec4(p + local, 1.0)).xyz;
        normal = normalize((world_from_local * vec4(rotate(v.normal, spin_axis, angle), 0.0)).xyz);
    }

    var out: Particle;
    out.world_position = vec4(world, 1.0);
    out.position = position_world_to_clip(world);
    out.world_normal = normal;
    out.uv = v.uv;
    out.color = vec4(mix(params.color.rgb, params.color_end.rgb, u), params.color.a * fade);
    return out;
}

@fragment
fn fragment(in: Particle, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    var texel = vec4(1.0);
    let card = params.fade.z == 0.0;
    if card {
        texel = textureSample(sprite, sprite_sampler, in.uv);
    }
    let alpha = saturate(in.color.a * texel.a);
    if alpha <= 0.0 {
        discard;
    }
    let albedo = in.color.rgb * texel.rgb;
    var N = normalize(in.world_normal);
    if !front {
        N = -N;
    }
    let view = view_bindings::view;
    var pbr = pbr_types::pbr_input_new();
    pbr.material.base_color = vec4(albedo, 1.0);
    // Emissive alpha 0 skips the exposure, so strength 1 renders the colour
    // at its own value before the tone curve.
    pbr.material.emissive = vec4(albedo * params.color_end.w, 0.0);
    pbr.material.perceptual_roughness = select(0.5, 1.0, card);
    pbr.material.reflectance = select(vec3(0.5), vec3(0.0), card);
    pbr.material.flags = pbr_types::STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT;
    pbr.frag_coord = in.position;
    pbr.world_position = in.world_position;
    pbr.world_normal = N;
    pbr.N = N;
    pbr.V = normalize(view.world_position.xyz - in.world_position.xyz);
    pbr.is_orthographic = view.clip_from_view[3].w == 1.0;
    pbr.flags = mesh_types::MESH_FLAGS_SHADOW_RECEIVER_BIT;
    var color = pbr_functions::apply_pbr_lighting(pbr);
    color = pbr_functions::main_pass_post_lighting_processing(pbr, vec4(color.rgb, 1.0));
    return vec4(color.rgb * alpha, alpha);
}
