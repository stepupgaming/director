//! The shared Vulkan device: one wgpu device with several queues in the
//! graphics family. Bevy renders on queue 0, and each Skia context
//! `gpu_context` hands out owns one of the other queues, so a model frame is
//! a Vulkan image Skia samples where Bevy left it.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use ash::vk::{self, Handle};
use skia_safe::gpu::{direct_contexts, vk as sk_vk, DirectContext, DirectContextId};
use wgpu::hal::api::Vulkan;

/// Queues the device asks for in family 0. Queue 0 is Bevy's.
const QUEUES: usize = 8;
static PRIORITIES: [f32; QUEUES] = [1.0; QUEUES];

pub(crate) struct Shared {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    family: u32,
    queues: u32,
    next: AtomicU32,
    contexts: Mutex<Vec<DirectContextId>>,
}

static DEVICE: OnceLock<Result<Shared, String>> = OnceLock::new();

/// The process-wide device, created on first use.
pub(crate) fn device() -> Result<&'static Shared, String> {
    DEVICE.get_or_init(open).as_ref().map_err(Clone::clone)
}

fn open() -> Result<Shared, String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .map_err(|e| format!("3D needs a Vulkan adapter: {e}"))?;
    let info = adapter.get_info();
    // The features and limits Bevy asks for on its own.
    let mut features = adapter.features();
    if info.device_type == wgpu::DeviceType::DiscreteGpu {
        features.remove(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS);
    }
    let limits = adapter.limits();
    let descriptor = wgpu::DeviceDescriptor {
        label: Some("director-3d"),
        required_features: features,
        required_limits: limits.clone(),
        // SAFETY: matches Bevy's own device request.
        experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() },
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    };
    let (family, queues, open) = {
        // SAFETY: the adapter is Vulkan, and the callback only widens queue 0's family.
        let hal = unsafe { adapter.as_hal::<Vulkan>() }.ok_or("the 3D adapter is not Vulkan")?;
        let raw = hal.shared_instance().raw_instance();
        let families = unsafe { raw.get_physical_device_queue_family_properties(hal.raw_physical_device()) };
        let first = families.first().ok_or("the Vulkan adapter has no queue family")?;
        let queues = if first.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
            (first.queue_count as usize).min(QUEUES)
        } else {
            1
        };
        let open = unsafe {
            hal.open_with_callback(
                features,
                &limits,
                &descriptor.memory_hints,
                Some(Box::new(move |args| {
                    args.queue_create_infos[0] = vk::DeviceQueueCreateInfo::default()
                        .queue_family_index(0)
                        .queue_priorities(&PRIORITIES[..queues]);
                })),
            )
        }
        .map_err(|e| format!("3D device: {e}"))?;
        (0, queues as u32, open)
    };
    let (device, queue) =
        unsafe { adapter.create_device_from_hal::<Vulkan>(open, &descriptor) }.map_err(|e| format!("3D device: {e}"))?;
    Ok(Shared {
        instance,
        adapter,
        device,
        queue,
        family,
        queues,
        next: AtomicU32::new(1),
        contexts: Mutex::new(Vec::new()),
    })
}

/// A Skia Vulkan context on the 3D device, owning one of its spare queues.
/// Model frames paint into such a context without leaving the GPU. Returns
/// `None` when the device has no spare queue left.
pub fn gpu_context() -> Option<DirectContext> {
    let shared = device().ok()?;
    let index = shared.next.fetch_add(1, Ordering::Relaxed);
    if index >= shared.queues {
        return None;
    }
    let hal = unsafe { shared.device.as_hal::<Vulkan>() }?;
    let instance = hal.shared_instance();
    let entry = instance.entry().clone();
    let api = instance.instance_api_version();
    let raw_instance = instance.raw_instance().handle();
    let raw_device = hal.raw_device();
    let queue = unsafe { raw_device.get_device_queue(shared.family, index) };
    let instance_handle = raw_instance.as_raw();
    // The skia-safe thread-local resolver keeps a raw pointer to this closure,
    // so it lives for the process.
    let resolve = move |of: sk_vk::GetProcOf| -> *const std::ffi::c_void {
        let (instance, name) = match of {
            sk_vk::GetProcOf::Instance(instance, name) => (vk::Instance::from_raw(instance as u64), name),
            sk_vk::GetProcOf::Device(_, name) => (vk::Instance::from_raw(instance_handle), name),
        };
        unsafe { entry.get_instance_proc_addr(instance, name) }.map_or(std::ptr::null(), |f| f as *const std::ffi::c_void)
    };
    let resolve: &'static _ = Box::leak(Box::new(resolve));
    let backend = unsafe {
        sk_vk::BackendContext::new_builder(
            raw_instance.as_raw() as sk_vk::Instance,
            hal.raw_physical_device().as_raw() as sk_vk::PhysicalDevice,
            raw_device.handle().as_raw() as sk_vk::Device,
            (queue.as_raw() as sk_vk::Queue, shared.family as usize),
            resolve,
            Some(sk_vk::Version::from(api)),
        )
        .build()
    };
    let context = direct_contexts::make_vulkan(&backend, None)?;
    shared.contexts.lock().expect("3D contexts").push(context.id());
    Some(context)
}

/// Whether `context` came from [`gpu_context`].
pub(crate) fn shares_device(context: &DirectContext) -> bool {
    DEVICE
        .get()
        .and_then(|d| d.as_ref().ok())
        .is_some_and(|d| d.contexts.lock().expect("3D contexts").contains(&context.id()))
}

/// The queue family Skia and Bevy share.
pub(crate) fn family() -> u32 {
    device().map(|d| d.family).unwrap_or(0)
}
