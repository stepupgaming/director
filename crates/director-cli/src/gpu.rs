//! Optional Vulkan GPU context for accelerated export.

#[cfg(feature = "vulkan")]
use std::ffi::CStr;
#[cfg(feature = "vulkan")]
use tracing::info;

/// Create a Skia Vulkan direct context when the `vulkan` feature is enabled.
#[cfg(feature = "vulkan")]
pub fn create_vulkan_context() -> Option<skia_safe::gpu::DirectContext> {
    use ash::vk::Handle;
    use skia_safe::gpu::direct_contexts;
    use skia_safe::gpu::vk::{self as sk_vk, BackendContext, GetProcOf};

    let entry = unsafe { ash::Entry::load() }.ok()?;

    let app_name = CStr::from_bytes_with_nul(b"Director\0").ok()?;
    let app_info = ash::vk::ApplicationInfo {
        s_type: ash::vk::StructureType::APPLICATION_INFO,
        p_next: std::ptr::null(),
        p_application_name: app_name.as_ptr(),
        application_version: 0,
        p_engine_name: app_name.as_ptr(),
        engine_version: 0,
        api_version: ash::vk::API_VERSION_1_2,
        ..Default::default()
    };

    let create_info = ash::vk::InstanceCreateInfo {
        s_type: ash::vk::StructureType::INSTANCE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: ash::vk::InstanceCreateFlags::empty(),
        p_application_info: &app_info,
        enabled_layer_count: 0,
        pp_enabled_layer_names: std::ptr::null(),
        enabled_extension_count: 0,
        pp_enabled_extension_names: std::ptr::null(),
        ..Default::default()
    };

    let instance = unsafe { entry.create_instance(&create_info, None) }.ok()?;

    let physical_devices = unsafe { instance.enumerate_physical_devices() }.ok()?;

    // Prefer discrete GPU (NVIDIA/AMD) over integrated (Intel iGPU)
    let mut selected_device = None;
    let mut selected_type = ash::vk::PhysicalDeviceType::OTHER;
    for pd in &physical_devices {
        let props = unsafe { instance.get_physical_device_properties(*pd) };
        let pd_type = props.device_type;
        let pd_name = unsafe { std::ffi::CStr::from_ptr(props.device_name.as_ptr()) }.to_string_lossy();
        match pd_type {
            ash::vk::PhysicalDeviceType::DISCRETE_GPU => {
                info!("Found discrete GPU: {}", pd_name);
                selected_device = Some(*pd);
                selected_type = pd_type;
                break;
            }
            ash::vk::PhysicalDeviceType::INTEGRATED_GPU => {
                info!("Found integrated GPU: {}", pd_name);
                if selected_device.is_none() {
                    selected_device = Some(*pd);
                    selected_type = pd_type;
                }
            }
            _ => {
                info!("Found other GPU type ({:?}): {}", pd_type, pd_name);
                if selected_device.is_none() {
                    selected_device = Some(*pd);
                    selected_type = pd_type;
                }
            }
        }
    }
    let physical_device = selected_device?;
    let gpu_type_name = match selected_type {
        ash::vk::PhysicalDeviceType::DISCRETE_GPU => "discrete GPU",
        ash::vk::PhysicalDeviceType::INTEGRATED_GPU => "integrated GPU",
        _ => "other",
    };
    info!("Using {} for rendering", gpu_type_name);

    let queue_families =
        unsafe { instance.get_physical_device_queue_family_properties(physical_device) };
    let queue_family_index = queue_families
        .iter()
        .position(|qf| qf.queue_flags.contains(ash::vk::QueueFlags::GRAPHICS))?
        as u32;

    // Enable all physical device features the GPU advertises.
    let physical_device_features =
        unsafe { instance.get_physical_device_features(physical_device) };

    // Skia expects VK_KHR_swapchain on the device even for offscreen rendering.
    let available_extensions = unsafe {
        instance
            .enumerate_device_extension_properties(physical_device)
            .ok()?
    };
    let swapchain_ext = CStr::from_bytes_with_nul(b"VK_KHR_swapchain\0").ok()?;
    let has_swapchain = available_extensions
        .iter()
        .any(|ext| unsafe { CStr::from_ptr(ext.extension_name.as_ptr()) == swapchain_ext });
    let enabled_extensions: &[*const i8] = if has_swapchain {
        &[swapchain_ext.as_ptr()]
    } else {
        &[]
    };

    let queue_priorities = [1.0f32];
    let queue_create_info = ash::vk::DeviceQueueCreateInfo {
        s_type: ash::vk::StructureType::DEVICE_QUEUE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: ash::vk::DeviceQueueCreateFlags::empty(),
        queue_family_index,
        queue_count: 1,
        p_queue_priorities: queue_priorities.as_ptr(),
        ..Default::default()
    };

    let device_create_info = ash::vk::DeviceCreateInfo {
        s_type: ash::vk::StructureType::DEVICE_CREATE_INFO,
        p_next: std::ptr::null(),
        flags: ash::vk::DeviceCreateFlags::empty(),
        queue_create_info_count: 1,
        p_queue_create_infos: &queue_create_info,
        enabled_layer_count: 0,
        pp_enabled_layer_names: std::ptr::null(),
        enabled_extension_count: enabled_extensions.len() as u32,
        pp_enabled_extension_names: enabled_extensions.as_ptr(),
        p_enabled_features: &physical_device_features,
        ..Default::default()
    };

    let device =
        unsafe { instance.create_device(physical_device, &device_create_info, None) }.ok()?;

    let queue = unsafe { device.get_device_queue(queue_family_index, 0) };

    let instance_handle = instance.handle().as_raw();
    // Leak the concrete closure to 'static — the thread-local resolver in
    // skia-safe holds a raw trait-object pointer to this closure.
    let get_proc_impl = move |get_proc_of: GetProcOf| -> *const std::ffi::c_void {
        match get_proc_of {
            GetProcOf::Instance(vk_instance, name) => unsafe {
                let ash_instance = ash::vk::Instance::from_raw(vk_instance as u64);
                entry
                    .get_instance_proc_addr(ash_instance, name)
                    .map_or(std::ptr::null(), |f| f as *const std::ffi::c_void)
            },
            GetProcOf::Device(_, name) => unsafe {
                let ash_instance = ash::vk::Instance::from_raw(instance_handle);
                entry
                    .get_instance_proc_addr(ash_instance, name)
                    .map_or(std::ptr::null(), |f| f as *const std::ffi::c_void)
            },
        }
    };
    let get_proc = Box::leak(Box::new(get_proc_impl));

    let backend_context = unsafe {
        BackendContext::new(
            instance.handle().as_raw() as sk_vk::Instance,
            physical_device.as_raw() as sk_vk::PhysicalDevice,
            device.handle().as_raw() as sk_vk::Device,
            (queue.as_raw() as sk_vk::Queue, 0),
            &*get_proc,
        )
    };

    let context = direct_contexts::make_vulkan(&backend_context, None)?;

    std::mem::forget(instance);
    std::mem::forget(device);

    Some(context)
}

#[cfg(not(feature = "vulkan"))]
pub fn create_vulkan_context() -> Option<()> {
    None
}
