//! The Export level's sample average: each jittered render of a target adds
//! a 1/N share of its linear colour to an `Rgba16Float` sum, and the resolve
//! writes the sum back through the target's sRGB view.

use std::collections::HashMap;

const SHADER: &str = r"
@group(0) @binding(0) var source: texture_2d<f32>;

@vertex
fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fragment(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(source, vec2<i32>(at.xy), 0);
}
";

pub(crate) struct Accumulator {
    layout: wgpu::BindGroupLayout,
    add: wgpu::RenderPipeline,
    resolve: wgpu::RenderPipeline,
    sums: HashMap<(u32, u32), (wgpu::Texture, wgpu::TextureView)>,
}

const SUM: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

impl Accumulator {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("director-3d accumulate"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("director-3d accumulate"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("director-3d accumulate"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |format, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("director-3d accumulate"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vertex"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fragment"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let share = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Constant,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let add = pipeline(
            SUM,
            Some(wgpu::BlendState {
                color: share,
                alpha: share,
            }),
        );
        let resolve = pipeline(wgpu::TextureFormat::Rgba8UnormSrgb, None);
        Self {
            layout,
            add,
            resolve,
            sums: HashMap::new(),
        }
    }

    /// One full-target draw of `source` into `target`.
    fn pass(
        &self,
        device: &wgpu::Device,
        pipeline: &wgpu::RenderPipeline,
        source: &wgpu::TextureView,
        target: &wgpu::TextureView,
        load: wgpu::LoadOp<wgpu::Color>,
        share: f64,
    ) -> wgpu::CommandBuffer {
        let mut encoder = device.create_command_encoder(&Default::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("director-3d accumulate"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source),
            }],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("director-3d accumulate"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.set_blend_constant(wgpu::Color {
            r: share,
            g: share,
            b: share,
            a: share,
        });
        pass.draw(0..3, 0..1);
        drop(pass);
        encoder.finish()
    }

    /// Adds `1/samples` of the target's linear colour to the sum of its size,
    /// starting a new sum at sample 0.
    pub fn add(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::Texture,
        sample: u32,
        samples: u32,
    ) {
        let size = (target.width(), target.height());
        let (_, sum) = self.sums.entry(size).or_insert_with(|| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("director-3d sample sum"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SUM,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            (texture, view)
        });
        let source = target.create_view(&wgpu::TextureViewDescriptor {
            format: Some(wgpu::TextureFormat::Rgba8UnormSrgb),
            ..Default::default()
        });
        let load = if sample == 0 {
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
        } else {
            wgpu::LoadOp::Load
        };
        let sum = sum.clone();
        queue.submit([self.pass(device, &self.add, &source, &sum, load, 1.0 / samples as f64)]);
    }

    /// Writes the finished sum into the target.
    pub fn resolve(&self, device: &wgpu::Device, queue: &wgpu::Queue, target: &wgpu::Texture) {
        let (_, sum) = &self.sums[&(target.width(), target.height())];
        let view = target.create_view(&wgpu::TextureViewDescriptor {
            format: Some(wgpu::TextureFormat::Rgba8UnormSrgb),
            ..Default::default()
        });
        let resolve = self.pass(device, &self.resolve, sum, &view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), 1.0);
        queue.submit([resolve]);
    }
}
