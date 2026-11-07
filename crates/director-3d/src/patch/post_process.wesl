//! Miscellaneous postprocessing effects, currently just chromatic aberration.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;
import package::effect_stack::chromatic_aberration::chromatic_aberration;
import package::effect_stack::lens_distortion::lens_distortion;
import package::effect_stack::vignette::vignette;

@fragment
fn fragment_main(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let distorted_uv = lens_distortion(in.uv);
    // Director: keep the source's coverage in alpha (upstream writes 1), so a
    // transparent sky stays transparent through the effects.
    let color = chromatic_aberration(distorted_uv);
    return vec4(vignette(in.uv, color.rgb), color.a);
}
