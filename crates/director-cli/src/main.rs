use clap::{Parser, ValueEnum};
use director_core::export::render_export;
use director_core::scripting::register_rhai_api_with_default_mode;
use director_core::systems::renderer::GpuContext;
use director_core::video_wrapper::RenderMode;
use director_core::DefaultAssetLoader;
use rhai::Engine;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info, warn};
use tracing_subscriber::{fmt, EnvFilter};

#[cfg(feature = "vulkan")]
use std::ffi::CStr;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to the Rhai script
    #[arg(value_name = "SCRIPT")]
    script: PathBuf,

    /// Output video path
    #[arg(value_name = "OUTPUT")]
    output: Option<PathBuf>,

    /// Enable alpha-channel output (VP9/WebM with transparency)
    #[arg(long, default_value_t = false)]
    alpha: bool,

    /// Enable Vulkan GPU-accelerated rendering
    #[arg(long, default_value_t = false)]
    gpu: bool,

    /// Log level
    #[arg(long, value_enum, default_value_t = LogLevel::Info)]
    log_level: LogLevel,

    /// Log format
    #[arg(long, value_enum, default_value_t = LogFormat::Pretty)]
    log_format: LogFormat,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Error => write!(f, "error"),
            LogLevel::Warn => write!(f, "warn"),
            LogLevel::Info => write!(f, "info"),
            LogLevel::Debug => write!(f, "debug"),
            LogLevel::Trace => write!(f, "trace"),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum LogFormat {
    Pretty,
    Json,
}

fn main() {
    let cli = Cli::parse();

    // Initialize Logging
    let filter = EnvFilter::builder()
        .with_default_directive(cli.log_level.to_string().parse().unwrap())
        .from_env_lossy();

    let subscriber_builder = fmt::Subscriber::builder()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false); // Clean output for humans (default)

    match cli.log_format {
        LogFormat::Json => {
            // For JSON, we might want target included, but user said strict NDJSON.
            // subscriber.json() enables JSON formatting.
            // We need to finish building.
            subscriber_builder.json().init();
        }
        LogFormat::Pretty => {
            subscriber_builder.pretty().init();
        }
    }

    let script_path = cli.script;
    let ext = if cli.alpha { "webm" } else { "mp4" };
    let output_path = if let Some(out) = cli.output {
        out
    } else {
        let mut p = script_path.clone();
        p.set_extension(ext);
        p
    };

    info!("Initializing Director Engine...");
    info!("Script: {:?}", script_path);
    info!("Output: {:?}", output_path);

    let mut gpu_context: Option<GpuContext> = None;

    if cli.gpu {
        #[cfg(feature = "vulkan")]
        match create_vulkan_context() {
            Some(ctx) => {
                info!("Vulkan GPU acceleration enabled");
                gpu_context = Some(ctx);
            }
            None => {
                warn!("Failed to initialize Vulkan, falling back to software rendering");
            }
        }
        #[cfg(not(feature = "vulkan"))]
        warn!("Vulkan support not compiled. Rebuild with --features=vulkan");
    }

    let script = match fs::read_to_string(&script_path) {
        Ok(s) => s,
        Err(e) => {
            error!("Error reading script file: {}", e);
            return;
        }
    };

    let mut engine = Engine::new();
    register_rhai_api_with_default_mode(
        &mut engine,
        Arc::new(DefaultAssetLoader),
        RenderMode::Export,
    );

    match engine.eval::<director_core::scripting::MovieHandle>(&script) {
        Ok(movie) => {
            info!("Script evaluated successfully. Starting render...");
            let mut director = movie.director.lock().unwrap();
            match render_export(&mut director, output_path, gpu_context.as_mut(), None, cli.alpha) {
                Ok(_) => info!("Render complete."),
                Err(e) => {
                    error!("Render failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            error!("Script Error: {}", e);
            std::process::exit(1);
        }
    }
}

#[cfg(feature = "vulkan")]
fn create_vulkan_context() -> Option<skia_safe::gpu::DirectContext> {
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
    let physical_device = physical_devices.into_iter().next()?;

    let queue_families = unsafe {
        instance.get_physical_device_queue_family_properties(physical_device)
    };
    let queue_family_index = queue_families
        .iter()
        .position(|qf| qf.queue_flags.contains(ash::vk::QueueFlags::GRAPHICS))? as u32;

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
        enabled_extension_count: 0,
        pp_enabled_extension_names: std::ptr::null(),
        p_enabled_features: std::ptr::null(),
        ..Default::default()
    };

    let device = unsafe { instance.create_device(physical_device, &device_create_info, None) }
        .ok()?;

    let queue = unsafe { device.get_device_queue(queue_family_index, 0) };

    let instance_handle = instance.handle().as_raw();
    let get_proc = move |get_proc_of: GetProcOf| -> *const std::ffi::c_void {
        match get_proc_of {
            GetProcOf::Instance(vk_instance, name) => unsafe {
                let ash_instance =
                    ash::vk::Instance::from_raw(vk_instance as u64);
                entry.get_instance_proc_addr(ash_instance, name)
                    .map_or(std::ptr::null(), |f| f as *const std::ffi::c_void)
            },
            GetProcOf::Device(_, name) => unsafe {
                let ash_instance =
                    ash::vk::Instance::from_raw(instance_handle);
                entry.get_instance_proc_addr(ash_instance, name)
                    .map_or(std::ptr::null(), |f| f as *const std::ffi::c_void)
            },
        }
    };

    let backend_context = unsafe {
        BackendContext::new(
            instance.handle().as_raw() as sk_vk::Instance,
            physical_device.as_raw() as sk_vk::PhysicalDevice,
            device.handle().as_raw() as sk_vk::Device,
            (queue.as_raw() as sk_vk::Queue, 0),
            &get_proc,
        )
    };

    let context = direct_contexts::make_vulkan(&backend_context, None)?;

    std::mem::forget(instance);
    std::mem::forget(device);

    Some(context)
}
