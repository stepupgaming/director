use crate::animation::Animated;
use crate::element::Element;
use crate::errors::RenderError;
use crate::node::parse_easing;
use skia_safe::{images, Canvas, ColorType, Data, Image, ImageInfo, Paint, Rect, AlphaType, ColorSpace};
use std::any::Any;
use std::sync::Mutex;
use taffy::style::Style;
use tracing::warn;

const DEFAULT_VERTEX: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec3<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOutput {
    let positions = array<vec3<f32>, 36>(
        vec3(-0.5, -0.5,  0.5), vec3( 0.5, -0.5,  0.5), vec3( 0.5,  0.5,  0.5),
        vec3(-0.5, -0.5,  0.5), vec3( 0.5,  0.5,  0.5), vec3(-0.5,  0.5,  0.5),
        vec3( 0.5, -0.5, -0.5), vec3(-0.5, -0.5, -0.5), vec3(-0.5,  0.5, -0.5),
        vec3( 0.5, -0.5, -0.5), vec3(-0.5,  0.5, -0.5), vec3( 0.5,  0.5, -0.5),
        vec3(-0.5,  0.5,  0.5), vec3( 0.5,  0.5,  0.5), vec3( 0.5,  0.5, -0.5),
        vec3(-0.5,  0.5,  0.5), vec3( 0.5,  0.5, -0.5), vec3(-0.5,  0.5, -0.5),
        vec3(-0.5, -0.5, -0.5), vec3( 0.5, -0.5, -0.5), vec3( 0.5, -0.5,  0.5),
        vec3(-0.5, -0.5, -0.5), vec3( 0.5, -0.5,  0.5), vec3(-0.5, -0.5,  0.5),
        vec3( 0.5, -0.5,  0.5), vec3( 0.5, -0.5, -0.5), vec3( 0.5,  0.5, -0.5),
        vec3( 0.5, -0.5,  0.5), vec3( 0.5,  0.5, -0.5), vec3( 0.5,  0.5,  0.5),
        vec3(-0.5, -0.5, -0.5), vec3(-0.5, -0.5,  0.5), vec3(-0.5,  0.5,  0.5),
        vec3(-0.5, -0.5, -0.5), vec3(-0.5,  0.5,  0.5), vec3(-0.5,  0.5, -0.5),
    );
    let colors = array<vec3<f32>, 36>(
        vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, 1.0),
        vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0), vec3(1.0, 1.0, 0.0),
        vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, 1.0),
        vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0), vec3(1.0, 1.0, 0.0),
        vec3(0.0, 1.0, 0.0), vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0),
        vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, 1.0), vec3(1.0, 1.0, 0.0),
        vec3(0.0, 0.0, 1.0), vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0),
        vec3(0.0, 0.0, 1.0), vec3(0.0, 1.0, 0.0), vec3(1.0, 1.0, 0.0),
        vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, 1.0), vec3(1.0, 0.0, 0.0),
        vec3(0.0, 1.0, 0.0), vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0),
        vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, 1.0),
        vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0), vec3(1.0, 1.0, 0.0),
    );
    var out: VertexOutput;
    out.position = vec4(positions[idx], 1.0);
    out.color = colors[idx];
    return out;
}
"#;

const DEFAULT_FRAGMENT: &str = r#"
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4(in.color, 1.0);
}
"#;

pub struct WgpuNode {
    pub style: Style,
    pub opacity: Animated<f32>,
    vertex_src: String,
    fragment_src: String,
    width: u32,
    height: u32,
    inner: Option<Box<WgpuInner>>,
    cache: Mutex<Option<(u32, u32, Image)>>,
}

impl std::fmt::Debug for WgpuNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WgpuNode")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("opacity", &self.opacity)
            .finish()
    }
}

impl Clone for WgpuNode {
    fn clone(&self) -> Self {
        Self {
            style: self.style.clone(),
            opacity: self.opacity.clone(),
            vertex_src: self.vertex_src.clone(),
            fragment_src: self.fragment_src.clone(),
            width: self.width,
            height: self.height,
            inner: None,
            cache: Mutex::new(None),
        }
    }
}

struct WgpuInner {
    device: wgpu::Device,
    queue: wgpu::Queue,
    render_pipeline: wgpu::RenderPipeline,
    texture: wgpu::Texture,
    texture_view: wgpu::TextureView,
}

impl WgpuNode {
    pub fn new(vertex_src: String, fragment_src: String) -> Self {
        Self {
            style: Style::DEFAULT,
            opacity: Animated::new(1.0),
            vertex_src,
            fragment_src,
            width: 256,
            height: 256,
            inner: None,
            cache: Mutex::new(None),
        }
    }

    pub fn new_default() -> Self {
        Self::new(DEFAULT_VERTEX.to_string(), DEFAULT_FRAGMENT.to_string())
    }

    fn ensure_wgpu(&mut self, width: u32, height: u32) {
        if let Some(ref inner) = self.inner {
            if inner.texture.width() >= width && inner.texture.height() >= height {
                return;
            }
        }
        let inner = pollster::block_on(async {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions::default())
                .await
                .expect("Failed to find a GPU adapter");
            let (device, queue) = adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: Some("director-wgpu-device"),
                        ..Default::default()
                    },
                    None,
                )
                .await
                .expect("Failed to create wgpu device");

            let shader_vs = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("vertex_shader"),
                source: wgpu::ShaderSource::Wgsl(self.vertex_src.clone().into()),
            });
            let shader_fs = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("fragment_shader"),
                source: wgpu::ShaderSource::Wgsl(self.fragment_src.clone().into()),
            });

            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("wgpu_pipeline_layout"),
                bind_group_layouts: &[],
                push_constant_ranges: &[],
            });

            let render_pipeline =
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("wgpu_render_pipeline"),
                    layout: Some(&pipeline_layout),
                    cache: None,
                    vertex: wgpu::VertexState {
                        module: &shader_vs,
                        entry_point: Some("vs_main"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader_fs,
                        entry_point: Some("fs_main"),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::Rgba8UnormSrgb,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        ..Default::default()
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                });

            let texture_size = width.max(64).max(height.max(64));
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("wgpu_output"),
                size: wgpu::Extent3d {
                    width: texture_size,
                    height: texture_size,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

            WgpuInner {
                device,
                queue,
                render_pipeline,
                texture,
                texture_view,
            }
        });
        self.inner = Some(Box::new(inner));
    }

    fn render_gpu(&mut self, width: u32, height: u32) -> Option<Image> {
        self.ensure_wgpu(width, height);
        let inner = self.inner.as_mut()?;

        let texture_size = inner.texture.width();

        let mut command_encoder =
            inner
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("wgpu_render_encoder"),
                });

        {
            let mut render_pass = command_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("wgpu_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &inner.texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 0.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            render_pass.set_pipeline(&inner.render_pipeline);
            render_pass.draw(0..36, 0..1);
        }

        let buffer_size = (texture_size * texture_size * 4) as u64;
        let output_buffer = inner.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("wgpu_output_buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let texture_extent = wgpu::Extent3d {
            width: texture_size,
            height: texture_size,
            depth_or_array_layers: 1,
        };
        let bytes_per_row = texture_size * 4;
        command_encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &inner.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &output_buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(texture_size),
                },
            },
            texture_extent,
        );

        inner.queue.submit(Some(command_encoder.finish()));

        let buffer_slice = output_buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |v| {
            let _ = tx.send(v);
        });
        inner.device.poll(wgpu::Maintain::Wait);
        if let Ok(Ok(())) = rx.recv() {
            let data = buffer_slice.get_mapped_range();
            let mut pixels = Vec::with_capacity((width * height * 4) as usize);
            for y in 0..height {
                let row_start = (y * bytes_per_row) as usize;
                let row_end = row_start + (width * 4) as usize;
                pixels.extend_from_slice(&data[row_start..row_end]);
            }
            drop(data);
            output_buffer.unmap();

            let info = ImageInfo::new(
                (width as i32, height as i32),
                ColorType::RGBA8888,
                AlphaType::Premul,
                Some(ColorSpace::new_srgb()),
            );
            let skia_data = Data::new_copy(&pixels);
            images::raster_from_data(&info, skia_data, (width * 4) as usize)
        } else {
            warn!("wgpu buffer mapping failed");
            None
        }
    }
}

impl Element for WgpuNode {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn kind(&self) -> &'static str {
        "wgpu"
    }

    fn layout_style(&self) -> Style {
        self.style.clone()
    }

    fn set_layout_style(&mut self, style: Style) {
        self.style = style;
    }

    fn set_opacity(&mut self, opacity: f32) {
        self.opacity.current_value = opacity;
    }

    fn needs_measure(&self) -> bool {
        false
    }

    fn update(&mut self, time: f64) -> bool {
        self.opacity.update(time);
        true
    }

    fn render(
        &self,
        canvas: &Canvas,
        rect: Rect,
        opacity: f32,
        draw_children: &mut dyn FnMut(&Canvas),
    ) -> Result<(), RenderError> {
        let w = rect.width() as u32;
        let h = rect.height() as u32;
        if w == 0 || h == 0 {
            return Ok(());
        }

        let local_opacity = self.opacity.current_value * opacity;

        let mut node = self.clone();
        if let Some(image) = node.render_gpu(w.max(1), h.max(1)) {
            let mut paint = Paint::default();
            paint.set_alpha_f(local_opacity);
            canvas.draw_image_rect(&image, None, rect, &paint);
        }

        draw_children(canvas);
        Ok(())
    }

    fn animate_property(
        &mut self,
        property: &str,
        start: f32,
        target: f32,
        duration: f64,
        easing: &str,
    ) -> Result<(), String> {
        let ease_fn = crate::scripting::utils::parse_easing_result(easing)?;
        match property {
            "opacity" => {
                self.opacity.add_segment(start, target, duration, ease_fn);
                Ok(())
            }
            other => Err(format!(
                "unknown property '{other}' on wgpu_node; valid: opacity"
            )),
        }
    }

    fn animate_property_spring(
        &mut self,
        property: &str,
        start: Option<f32>,
        target: f32,
        config: crate::animation::SpringConfig,
    ) -> Result<(), String> {
        match property {
            "opacity" => {
                if let Some(s) = start {
                    self.opacity.add_spring_with_start(s, target, config);
                } else {
                    self.opacity.add_spring(target, config);
                }
                Ok(())
            }
            other => Err(format!(
                "spring: unknown property '{other}' on wgpu_node; valid: opacity"
            )),
        }
    }
}
