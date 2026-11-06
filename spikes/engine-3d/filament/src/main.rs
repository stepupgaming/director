//! Filament 1.80 engine spike for issue 9.
//!
//! Filament creates the Vulkan device and renders `spike.glb` under IBL from
//! cmgen KTX files into a headless swapchain (`../shim.cpp`). Skia then wraps
//! the swapchain VkImage the frame landed in, on the same VkDevice and VkQueue,
//! and composites it with a 2D overlay with no readback. The output line
//! matches the Bevy spike.
//!
//! Usage: spike-filament <width> <height> <frames> <assets dir> <out dir>

use std::ffi::{c_char, c_void, CString};
use std::time::Instant;

use ash::vk::Handle as _;
use skia_safe::{
    gpu::{self, vk as sk_vk, SurfaceOrigin, SyncCpu},
    AlphaType, Color, ColorType, Font, FontMgr, FontStyle, ImageInfo, Paint, RRect, Rect,
};

#[repr(C)]
#[derive(Default)]
struct FilInfo {
    instance: u64,
    physical: u64,
    device: u64,
    queue: u64,
    family: u32,
    queue_index: u32,
    format: u32,
    image_count: u32,
    images: [u64; 8],
}

extern "C" {
    fn fil_create(
        glb: *const c_char,
        ibl: *const c_char,
        sky: *const c_char,
        sh: *const c_char,
        width: u32,
        height: u32,
        info: *mut FilInfo,
    ) -> *mut c_void;
    fn fil_frame(handle: *mut c_void, t: f32) -> i32;
    fn fil_skipped(handle: *mut c_void) -> u32;
}

/// Layout Filament leaves a headless swapchain image in after its last pass.
const FILAMENT_LAYOUT: ash::vk::ImageLayout = ash::vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let width: u32 = args.get(1).map_or(Ok(1920), |s| s.parse())?;
    let height: u32 = args.get(2).map_or(Ok(1080), |s| s.parse())?;
    let frames: usize = args.get(3).map_or(Ok(120), |s| s.parse())?;
    let assets = args.get(4).cloned().unwrap_or_else(|| "F:/director-spikes/assets".into());
    let out = args.get(5).cloned().unwrap_or_else(|| "F:/director-spikes/out".into());
    std::fs::create_dir_all(&out)?;

    let t_boot = Instant::now();
    let c = |p: String| CString::new(p).unwrap();
    let glb = c(format!("{assets}/spike.glb"));
    let ibl = c(format!("{assets}/filament/_ibl.ktx"));
    let sky = c(format!("{assets}/filament/_skybox.ktx"));
    let sh = c(format!("{assets}/filament/sh.txt"));
    let mut info = FilInfo::default();
    let fil = unsafe {
        fil_create(glb.as_ptr(), ibl.as_ptr(), sky.as_ptr(), sh.as_ptr(), width, height, &mut info)
    };
    if fil.is_null() {
        anyhow::bail!("Filament did not start");
    }
    if info.image_count == 0 {
        anyhow::bail!("Filament reported no swapchain images");
    }
    // A few frames so shadow maps and pipelines exist.
    for i in 0..5 {
        unsafe { fil_frame(fil, i as f32 / 30.0) };
    }
    let boot_ms = t_boot.elapsed().as_secs_f64() * 1e3;
    let color_type = match info.format {
        37 => ColorType::RGBA8888,
        44 => ColorType::BGRA8888,
        f => anyhow::bail!("unhandled swapchain format {f}"),
    };
    eprintln!(
        "filament: {} swapchain images, VkFormat {}, family {}",
        info.image_count, info.format, info.family
    );

    // Skia on Filament's VkInstance, VkDevice and VkQueue.
    let entry = unsafe { ash::Entry::load()? };
    let instance_raw = info.instance;
    let proc_entry = entry.clone();
    let get_proc_impl = move |of: sk_vk::GetProcOf| -> *const c_void {
        let (inst, name) = match of {
            sk_vk::GetProcOf::Instance(i, name) => (ash::vk::Instance::from_raw(i as u64), name),
            sk_vk::GetProcOf::Device(_, name) => (ash::vk::Instance::from_raw(instance_raw), name),
        };
        unsafe {
            proc_entry
                .get_instance_proc_addr(inst, name)
                .map_or(std::ptr::null(), |f| f as *const c_void)
        }
    };
    let get_proc = Box::leak(Box::new(get_proc_impl));
    let backend = unsafe {
        sk_vk::BackendContext::new_builder(
            info.instance as sk_vk::Instance,
            info.physical as sk_vk::PhysicalDevice,
            info.device as sk_vk::Device,
            (info.queue as sk_vk::Queue, info.family as usize),
            &*get_proc,
            None,
        )
        .build()
    };
    let mut ctx = gpu::direct_contexts::make_vulkan(&backend, None)
        .ok_or_else(|| anyhow::anyhow!("Skia refused Filament's device"))?;

    // Filament's headless swapchain images lack SAMPLED usage, so Skia cannot
    // sample them. Each frame copies the finished image on the GPU into a
    // texture Skia owns, on the same queue, and the composite draws that.
    let scene_info = ImageInfo::new((width as i32, height as i32), color_type, AlphaType::Premul, None);
    let mut scene_surface = gpu::surfaces::render_target(
        &mut ctx,
        gpu::Budgeted::No,
        &scene_info,
        None,
        SurfaceOrigin::TopLeft,
        None,
        false,
        None,
    )
    .ok_or_else(|| anyhow::anyhow!("no Skia scene texture"))?;
    scene_surface.canvas().clear(Color::BLACK);
    ctx.flush_and_submit_surface(&mut scene_surface, SyncCpu::Yes);
    let scene_texture = gpu::surfaces::get_backend_texture(
        &mut scene_surface,
        skia_safe::surface::BackendHandleAccess::FlushRead,
    )
    .ok_or_else(|| anyhow::anyhow!("no backend texture for the scene"))?;
    let scene_vk = scene_texture
        .vulkan_image_info()
        .ok_or_else(|| anyhow::anyhow!("scene texture is not Vulkan"))?;
    let scene_image = scene_surface.image_snapshot();
    let copier = Copier::new(&entry, &info)?;
    let scene_dst = ash::vk::Image::from_raw(*scene_vk.image() as u64);

    let image_info = ImageInfo::new(
        (width as i32, height as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    let mut surface = gpu::surfaces::render_target(
        &mut ctx,
        gpu::Budgeted::Yes,
        &image_info,
        None,
        SurfaceOrigin::TopLeft,
        None,
        false,
        None,
    )
    .ok_or_else(|| anyhow::anyhow!("no Skia surface"))?;
    let typeface = FontMgr::new()
        .legacy_make_typeface(None, FontStyle::bold())
        .ok_or_else(|| anyhow::anyhow!("no typeface"))?;
    let scale = height as f32 / 1080.0;
    let font = Font::from_typeface(typeface, 54.0 * scale);
    let mut pixels = vec![0u8; (width * height * 4) as usize];

    let mut t3d = Vec::new();
    let mut tcomp = Vec::new();
    let mut tread = Vec::new();
    let warmup = 10;
    for i in 0..frames + warmup {
        let t = (i % 120) as f32 / 30.0;
        let a = Instant::now();
        let index = unsafe { fil_frame(fil, t) };
        let b = Instant::now();
        if index < 0 {
            continue;
        }
        // Skia's current layout for its texture, read from the shared state.
        let skia_layout = scene_texture.vulkan_image_info().map_or(scene_vk.layout, |v| v.layout);
        copier.copy(
            ash::vk::Image::from_raw(info.images[index as usize]),
            scene_dst,
            ash::vk::ImageLayout::from_raw(skia_layout as i32),
            width,
            height,
        )?;

        let canvas = surface.canvas();
        canvas.clear(Color::BLACK);
        canvas.draw_image(&scene_image, (0, 0), None);
        let mut panel = Paint::default();
        panel.set_anti_alias(true);
        panel.set_color(Color::from_argb(170, 12, 14, 22));
        let r = Rect::from_xywh(80.0 * scale, height as f32 - 230.0 * scale, 1100.0 * scale, 140.0 * scale);
        canvas.draw_rrect(RRect::new_rect_xy(r, 24.0 * scale, 24.0 * scale), &panel);
        let mut text = Paint::default();
        text.set_anti_alias(true);
        text.set_color(Color::WHITE);
        canvas.draw_str(
            format!("Filament 3D + Skia 2D, one device  t={t:.2}s"),
            (r.left + 36.0 * scale, r.top + 88.0 * scale),
            &font,
            &text,
        );
        ctx.flush_and_submit_surface(&mut surface, SyncCpu::Yes);
        let c = Instant::now();

        let ok = surface.read_pixels(&image_info, &mut pixels, (width * 4) as usize, (0, 0));
        let d = Instant::now();
        if !ok {
            anyhow::bail!("readback failed");
        }
        if i >= warmup {
            t3d.push((b - a).as_secs_f64() * 1e3);
            tcomp.push((c - b).as_secs_f64() * 1e3);
            tread.push((d - c).as_secs_f64() * 1e3);
        }
        let shot = i - warmup.min(i);
        if i >= warmup && (shot == 0 || shot == 60 || shot == 119) {
            let snapshot = surface.image_snapshot();
            if let Some(data) = snapshot.encode(Some(&mut ctx), skia_safe::EncodedImageFormat::PNG, 100) {
                std::fs::write(format!("{out}/filament-{width}x{height}-{shot:03}.png"), data.as_bytes())?;
            }
        }
    }
    if t3d.is_empty() {
        anyhow::bail!("every frame was skipped");
    }

    let total: Vec<f64> = t3d.iter().zip(&tcomp).map(|(a, b)| a + b).collect();
    println!(
        "filament {width}x{height} frames={} skipped={} boot_ms={boot_ms:.0} | 3d median {:.2} p95 {:.2} | composite median {:.2} p95 {:.2} | total median {:.2} p95 {:.2} | readback avoided median {:.2} p95 {:.2}",
        t3d.len(),
        unsafe { fil_skipped(fil) },
        pct(&t3d, 0.5),
        pct(&t3d, 0.95),
        pct(&tcomp, 0.5),
        pct(&tcomp, 0.95),
        pct(&total, 0.5),
        pct(&total, 0.95),
        pct(&tread, 0.5),
        pct(&tread, 0.95),
    );
    drop(scene_image);
    drop(scene_surface);
    drop(surface);
    ctx.abandon();
    std::process::exit(0);
}

fn pct(v: &[f64], p: f64) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    s[((s.len() - 1) as f64 * p).round() as usize]
}

/// Records and submits one GPU image copy on Filament's queue.
struct Copier {
    device: ash::Device,
    queue: ash::vk::Queue,
    cmd: ash::vk::CommandBuffer,
    fence: ash::vk::Fence,
}

impl Copier {
    fn new(entry: &ash::Entry, info: &FilInfo) -> anyhow::Result<Self> {
        use ash::vk;
        unsafe {
            let instance = ash::Instance::load(entry.static_fn(), vk::Instance::from_raw(info.instance));
            let device = ash::Device::load(instance.fp_v1_0(), vk::Device::from_raw(info.device));
            let pool = device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(info.family)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )?;
            let cmd = device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )?[0];
            let fence = device.create_fence(&vk::FenceCreateInfo::default(), None)?;
            Ok(Self { device, queue: vk::Queue::from_raw(info.queue), cmd, fence })
        }
    }

    fn copy(
        &self,
        src: ash::vk::Image,
        dst: ash::vk::Image,
        dst_layout: ash::vk::ImageLayout,
        width: u32,
        height: u32,
    ) -> anyhow::Result<()> {
        use ash::vk;
        let range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        let barrier = |image, old, new, src_access, dst_access| {
            vk::ImageMemoryBarrier::default()
                .image(image)
                .old_layout(old)
                .new_layout(new)
                .src_access_mask(src_access)
                .dst_access_mask(dst_access)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .subresource_range(range)
        };
        let all = vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE;
        let layers = vk::ImageSubresourceLayers::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .layer_count(1);
        let region = vk::ImageCopy::default()
            .src_subresource(layers)
            .dst_subresource(layers)
            .extent(vk::Extent3D { width, height, depth: 1 });
        unsafe {
            let d = &self.device;
            d.reset_command_buffer(self.cmd, vk::CommandBufferResetFlags::empty())?;
            d.begin_command_buffer(
                self.cmd,
                &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
            d.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[
                    barrier(src, FILAMENT_LAYOUT, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, all, vk::AccessFlags::TRANSFER_READ),
                    barrier(dst, dst_layout, vk::ImageLayout::TRANSFER_DST_OPTIMAL, all, vk::AccessFlags::TRANSFER_WRITE),
                ],
            );
            d.cmd_copy_image(
                self.cmd,
                src,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                dst,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[region],
            );
            d.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[
                    barrier(src, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, FILAMENT_LAYOUT, vk::AccessFlags::TRANSFER_READ, all),
                    barrier(dst, vk::ImageLayout::TRANSFER_DST_OPTIMAL, dst_layout, vk::AccessFlags::TRANSFER_WRITE, all),
                ],
            );
            d.end_command_buffer(self.cmd)?;
            let cmds = [self.cmd];
            d.queue_submit(self.queue, &[vk::SubmitInfo::default().command_buffers(&cmds)], self.fence)?;
            d.wait_for_fences(&[self.fence], true, u64::MAX)?;
            d.reset_fences(&[self.fence])?;
        }
        Ok(())
    }
}
