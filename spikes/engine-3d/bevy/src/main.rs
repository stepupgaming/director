//! Bevy 0.20 engine spike for issue 9.
//!
//! Bevy renders `spike.glb` (made by `../scene/make_scene.py`) under image-based
//! light from an HDR into a Vulkan texture. Skia then borrows that VkImage on the
//! same VkDevice and VkQueue and composites it with a 2D overlay, with no
//! readback. The run prints median and p95 milliseconds for the 3D frame, the
//! Skia composite, their total, and a readback that the shared device avoids.
//!
//! Usage: spike-bevy <width> <height> <frames> <assets dir> <out dir>

use std::time::Instant;

use ash::vk::Handle as _;
use bevy::{
    app::{AppLabel, SubApps},
    anti_alias::smaa::Smaa,
    asset::{AssetPlugin, RenderAssetUsages},
    camera::{Exposure, Hdr, ImageRenderTarget, RenderTarget},
    core_pipeline::tonemapping::Tonemapping,
    gltf::GltfAssetLabel,
    image::Image,
    light::{GeneratedEnvironmentMapLight, Skybox},
    post_process::bloom::Bloom,
    prelude::*,
    render::{
        render_asset::RenderAssets,
        render_resource::{
            Extent3d, PollType, TextureDimension, TextureFormat, TextureViewDescriptor,
            TextureViewDimension,
        },
        renderer::RenderDevice,
        settings::{Backends, RenderCreation, WgpuSettings},
        texture::GpuImage,
        RenderApp, RenderPlugin,
    },
    window::ExitCondition,
    world_serialization::WorldAssetRoot,
};
use skia_safe::{
    gpu::{self, vk as sk_vk, DirectContext, MutableTextureState, SurfaceOrigin, SyncCpu},
    AlphaType, Color, ColorType, Font, FontMgr, FontStyle, ImageInfo, Paint, RRect, Rect,
};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let width: u32 = args.get(1).map_or(Ok(1920), |s| s.parse())?;
    let height: u32 = args.get(2).map_or(Ok(1080), |s| s.parse())?;
    let frames: usize = args.get(3).map_or(Ok(120), |s| s.parse())?;
    let assets = args.get(4).cloned().unwrap_or_else(|| "F:/director-spikes/assets".into());
    let out = args.get(5).cloned().unwrap_or_else(|| "F:/director-spikes/out".into());
    std::fs::create_dir_all(&out)?;

    let t_boot = Instant::now();
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
                    backends: Some(Backends::VULKAN),
                    ..default()
                })),
                synchronous_pipeline_compilation: true,
                ..default()
            })
            .set(AssetPlugin {
                file_path: assets.clone(),
                ..default()
            }),
    );
    app.finish();
    app.cleanup();
    let mut apps: SubApps = std::mem::take(app.sub_apps_mut());

    // Render target: sRGB for Bevy's tone-mapped output, with a UNORM view
    // format so the image carries MUTABLE_FORMAT and Skia can read raw bytes.
    let target = {
        let world = apps.main.world_mut();
        let mut image = Image::new_target_texture(
            width,
            height,
            TextureFormat::Rgba8UnormSrgb,
            None,
        );
        image.texture_descriptor.view_formats = &[TextureFormat::Rgba8Unorm];
        image.asset_usage = RenderAssetUsages::RENDER_WORLD;
        world.resource_mut::<Assets<Image>>().add(image)
    };

    // Load the scene, the clip and the HDR.
    let (graph, clip, hdr) = {
        let world = apps.main.world_mut();
        let server = world.resource::<AssetServer>().clone();
        let scene = server.load(GltfAssetLabel::Scene(0).from_asset("spike.glb"));
        let clip = server.load(GltfAssetLabel::Animation(0).from_asset("spike.glb"));
        let hdr: Handle<Image> = server.load("forest.exr");
        let (graph, index) = AnimationGraph::from_clip(clip);
        let graph = world.resource_mut::<Assets<AnimationGraph>>().add(graph);
        world.spawn(WorldAssetRoot(scene));
        (graph, index, hdr)
    };

    // Pump until the glTF camera, the animation player and the HDR exist.
    let mut camera = None;
    for _ in 0..2000 {
        step(&mut apps);
        let world = apps.main.world_mut();
        let cam = world
            .query_filtered::<Entity, With<Camera3d>>()
            .iter(world)
            .next();
        let player = world.query::<&AnimationPlayer>().iter(world).next().is_some();
        let hdr_ready = world.resource::<Assets<Image>>().get(&hdr).is_some();
        if let (Some(cam), true, true) = (cam, player, hdr_ready) {
            camera = Some(cam);
            break;
        }
    }
    let camera = camera.ok_or_else(|| anyhow::anyhow!("scene never loaded"))?;

    // Equirectangular HDR to a cubemap on the CPU (Bevy has no converter).
    let cube = {
        let world = apps.main.world_mut();
        let images = world.resource::<Assets<Image>>();
        let src = images.get(&hdr).unwrap();
        let cube = equirect_to_cube(src, 512);
        world.resource_mut::<Assets<Image>>().add(cube)
    };

    {
        let world = apps.main.world_mut();
        world.entity_mut(camera).insert((
            RenderTarget::Image(ImageRenderTarget {
                handle: target.clone(),
                scale_factor: 1.0,
            }),
            Hdr,
            Tonemapping::TonyMcMapface,
            Bloom::NATURAL,
            Exposure { ev100: 11.0 },
            Msaa::Off,
            Smaa::default(),
            bevy::pbr::ScreenSpaceTransmission::default(),
            GeneratedEnvironmentMapLight {
                environment_map: cube.clone(),
                intensity: 3000.0,
                ..default()
            },
            Skybox {
                image: Some(cube.clone()),
                brightness: 3000.0,
                ..default()
            },
        ));
        // Glass: glTF exports thin, double-sided transmission. Give it volume.
        for (_, m) in world.resource_mut::<Assets<StandardMaterial>>().iter_mut() {
            if m.specular_transmission > 0.0 {
                m.thickness = 1.1;
                m.double_sided = false;
                m.cull_mode = Some(bevy::render::render_resource::Face::Back);
            }
        }
        for mut light in world.query::<&mut DirectionalLight>().iter_mut(world) {
            light.shadow_maps_enabled = true;
        }
        for mut light in world.query::<&mut SpotLight>().iter_mut(world) {
            light.shadow_maps_enabled = true;
        }
        for mut light in world.query::<&mut PointLight>().iter_mut(world) {
            light.shadow_maps_enabled = true;
        }
        let players: Vec<Entity> = world
            .query_filtered::<Entity, With<AnimationPlayer>>()
            .iter(world)
            .collect();
        for entity in players {
            let mut e = world.entity_mut(entity);
            e.insert(AnimationGraphHandle(graph.clone()));
            let mut player = e.get_mut::<AnimationPlayer>().unwrap();
            player.play(clip).pause();
        }
    }
    // A few frames so pipelines, shadow maps and the filtered env map exist.
    for _ in 0..5 {
        step(&mut apps);
    }
    let boot_ms = t_boot.elapsed().as_secs_f64() * 1e3;

    // Skia on Bevy's VkDevice and VkQueue.
    let (mut ctx, family) = skia_on_bevy_device(&apps)?;
    let vk_image = raw_target_image(&apps, &target)?;
    let backend_texture = unsafe {
        let info = sk_vk::ImageInfo::new(
            vk_image.as_raw() as sk_vk::Image,
            sk_vk::Alloc::default(),
            sk_vk::ImageTiling::OPTIMAL,
            sk_vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            sk_vk::Format::R8G8B8A8_UNORM,
            1,
            family,
            None,
            None,
            None,
        );
        gpu::backend_textures::make_vk((width as i32, height as i32), &info, "bevy-target")
    };
    let scene_image = gpu::images::borrow_texture_from(
        &mut ctx,
        &backend_texture,
        SurfaceOrigin::TopLeft,
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    )
    .ok_or_else(|| anyhow::anyhow!("Skia could not borrow the Bevy VkImage"))?;
    let info = ImageInfo::new(
        (width as i32, height as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    let mut surface = gpu::surfaces::render_target(
        &mut ctx,
        gpu::Budgeted::Yes,
        &info,
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
    let restore = MutableTextureState::new_vk(sk_vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL, family);
    let mut pixels = vec![0u8; (width * height * 4) as usize];

    let mut t3d = Vec::new();
    let mut tcomp = Vec::new();
    let mut tread = Vec::new();
    let warmup = 10;
    for i in 0..frames + warmup {
        let t = (i % 120) as f32 / 30.0;
        {
            let world = apps.main.world_mut();
            for mut player in world.query::<&mut AnimationPlayer>().iter_mut(world) {
                if let Some(active) = player.animation_mut(clip) {
                    active.seek_to(t);
                }
            }
        }
        let a = Instant::now();
        step(&mut apps);
        let b = Instant::now();

        // Composite: the 3D frame plus a 2D lower third standing in for HTML.
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
            format!("Bevy 3D + Skia 2D, one device  t={t:.2}s"),
            (r.left + 36.0 * scale, r.top + 88.0 * scale),
            &font,
            &text,
        );
        ctx.flush_and_submit_surface(&mut surface, None);
        // Hand the image back to Bevy in the layout wgpu expects.
        ctx.set_backend_texture_state(&backend_texture, &restore);
        ctx.submit(SyncCpu::Yes);
        let c = Instant::now();

        // Readback: what a separate device or process would pay per frame.
        let ok = surface.read_pixels(&info, &mut pixels, (width * 4) as usize, (0, 0));
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
                std::fs::write(format!("{out}/bevy-{width}x{height}-{shot:03}.png"), data.as_bytes())?;
            }
        }
    }

    let total: Vec<f64> = t3d.iter().zip(&tcomp).map(|(a, b)| a + b).collect();
    println!(
        "bevy {width}x{height} frames={frames} boot_ms={boot_ms:.0} | 3d median {:.2} p95 {:.2} | composite median {:.2} p95 {:.2} | total median {:.2} p95 {:.2} | readback avoided median {:.2} p95 {:.2}",
        pct(&t3d, 0.5),
        pct(&t3d, 0.95),
        pct(&tcomp, 0.5),
        pct(&tcomp, 0.95),
        pct(&total, 0.5),
        pct(&total, 0.95),
        pct(&tread, 0.5),
        pct(&tread, 0.95),
    );
    // Skia must release its borrow before Bevy tears the device down.
    drop(scene_image);
    drop(surface);
    ctx.abandon();
    std::process::exit(0);
}

fn step(apps: &mut SubApps) {
    apps.update();
    apps.main
        .world()
        .resource::<RenderDevice>()
        .wgpu_device()
        .poll(PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .unwrap();
}

fn pct(v: &[f64], p: f64) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    s[((s.len() - 1) as f64 * p).round() as usize]
}

/// Builds a Skia direct context on the VkInstance, VkDevice and VkQueue that wgpu created for Bevy.
fn skia_on_bevy_device(apps: &SubApps) -> anyhow::Result<(DirectContext, u32)> {
    let device = apps.main.world().resource::<RenderDevice>().wgpu_device().clone();
    let hal = unsafe { device.as_hal::<wgpu::hal::api::Vulkan>() }
        .ok_or_else(|| anyhow::anyhow!("Bevy is not on Vulkan"))?;
    let shared = hal.shared_instance();
    let entry = shared.entry().clone();
    let instance = shared.raw_instance().handle();
    let physical = hal.raw_physical_device();
    let raw_device = hal.raw_device().handle();
    let queue = hal.raw_queue();
    let family = hal.queue_family_index();
    drop(hal);

    let instance_raw = instance.as_raw();
    let get_proc_impl = move |of: sk_vk::GetProcOf| -> *const std::ffi::c_void {
        let (inst, name) = match of {
            sk_vk::GetProcOf::Instance(i, name) => (ash::vk::Instance::from_raw(i as u64), name),
            sk_vk::GetProcOf::Device(_, name) => (ash::vk::Instance::from_raw(instance_raw), name),
        };
        unsafe {
            entry
                .get_instance_proc_addr(inst, name)
                .map_or(std::ptr::null(), |f| f as *const std::ffi::c_void)
        }
    };
    let get_proc = Box::leak(Box::new(get_proc_impl));
    let backend = unsafe {
        sk_vk::BackendContext::new_builder(
            instance.as_raw() as sk_vk::Instance,
            physical.as_raw() as sk_vk::PhysicalDevice,
            raw_device.as_raw() as sk_vk::Device,
            (queue.as_raw() as sk_vk::Queue, family as usize),
            &*get_proc,
            None,
        )
        .build()
    };
    // Keep the wgpu device alive for the process.
    std::mem::forget(device);
    let ctx = gpu::direct_contexts::make_vulkan(&backend, None)
        .ok_or_else(|| anyhow::anyhow!("Skia refused Bevy's device"))?;
    Ok((ctx, family))
}

/// The VkImage behind Bevy's render target.
fn raw_target_image(apps: &SubApps, target: &Handle<Image>) -> anyhow::Result<ash::vk::Image> {
    let render = apps
        .sub_apps
        .get(&RenderApp.intern())
        .ok_or_else(|| anyhow::anyhow!("no render app"))?;
    let gpu_images = render.world().resource::<RenderAssets<GpuImage>>();
    let gpu_image = gpu_images
        .get(target)
        .ok_or_else(|| anyhow::anyhow!("render target not on the GPU yet"))?;
    let hal = unsafe { gpu_image.texture.as_hal::<wgpu::hal::api::Vulkan>() }
        .ok_or_else(|| anyhow::anyhow!("texture is not Vulkan"))?;
    Ok(unsafe { hal.raw_handle() })
}

/// Samples an equirectangular Rgba32Float image into an Rgba16Float cubemap.
fn equirect_to_cube(src: &Image, size: u32) -> Image {
    let (sw, sh) = (src.width() as usize, src.height() as usize);
    let data = src.data.as_ref().expect("HDR has CPU data");
    let px = |x: usize, y: usize| -> [f32; 3] {
        let o = (y * sw + x) * 16;
        let f = |k: usize| f32::from_le_bytes(data[o + k * 4..o + k * 4 + 4].try_into().unwrap());
        [f(0), f(1), f(2)]
    };
    let mut out = Vec::with_capacity((size * size * 6 * 8) as usize);
    for face in 0..6 {
        for y in 0..size {
            for x in 0..size {
                let s = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let t = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let d = match face {
                    0 => Vec3::new(1.0, -t, -s),
                    1 => Vec3::new(-1.0, -t, s),
                    2 => Vec3::new(s, 1.0, t),
                    3 => Vec3::new(s, -1.0, -t),
                    4 => Vec3::new(s, -t, 1.0),
                    _ => Vec3::new(-s, -t, -1.0),
                }
                .normalize();
                let u = 0.5 + d.x.atan2(-d.z) / std::f32::consts::TAU;
                let v = d.y.clamp(-1.0, 1.0).acos() / std::f32::consts::PI;
                let fx = (u * sw as f32 - 0.5).rem_euclid(sw as f32);
                let fy = (v * sh as f32 - 0.5).clamp(0.0, (sh - 1) as f32);
                let (x0, y0) = (fx as usize % sw, fy as usize);
                let (x1, y1) = ((x0 + 1) % sw, (y0 + 1).min(sh - 1));
                let (ax, ay) = (fx.fract(), fy.fract());
                let (c00, c10, c01, c11) = (px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1));
                let mut c = [0.0f32; 3];
                for k in 0..3 {
                    let top = c00[k] + (c10[k] - c00[k]) * ax;
                    let bottom = c01[k] + (c11[k] - c01[k]) * ax;
                    c[k] = top + (bottom - top) * ay;
                }
                for ch in [c[0], c[1], c[2], 1.0] {
                    out.extend_from_slice(&half::f16::from_f32(ch).to_le_bytes());
                }
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        out,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image
}
