//! Sharing the UI's GPU device with the engine.
//!
//! iced creates the wgpu device. The 2D view's render pipeline is created with that
//! device on the first frame; it hands a clone to [`publish`], the app picks it up with
//! [`take`] and starts the engine thread on it. Engine and UI therefore share textures:
//! the 2D and 3D views sample the engine's images without any copy through the CPU.

use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

use brewed_texture_engine::{EngineService, Event, wgpu};
use futures::channel::mpsc;
use iced::Subscription;

#[derive(Clone)]
pub struct GpuHandles {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

static SLOT: OnceLock<Mutex<Option<GpuHandles>>> = OnceLock::new();
static PUBLISHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// See [`largest_texture`].
static LARGEST: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(8192);

/// The longest side a texture may have on the device, and so a window: what is drawn into a
/// window, and what iced draws it with, are textures of its size. What iced asks of a device
/// (8192), until the device says.
pub fn largest_texture() -> u32 {
    LARGEST.load(std::sync::atomic::Ordering::Relaxed)
}

/// Called by render pipelines when iced creates them.
pub fn publish(device: &wgpu::Device, queue: &wgpu::Queue) {
    LARGEST.store(device.limits().max_texture_dimension_2d, std::sync::atomic::Ordering::Relaxed);
    if PUBLISHED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let slot = SLOT.get_or_init(|| Mutex::new(None));
    if let Ok(mut s) = slot.lock() {
        *s = Some(GpuHandles { device: device.clone(), queue: queue.clone() });
    }
}

/// The device, once iced created it (taken once).
pub fn take() -> Option<GpuHandles> {
    SLOT.get()?.lock().ok()?.take()
}

/// What `device` runs on, said: Vulkan, DirectX 12, Metal or OpenGL (iced picks it, of those
/// the settings let it, and does not say; a normal map's green, for one, is read as the backend
/// draws it, so the status bar says it).
pub fn backend_of(device: &wgpu::Device) -> &'static str {
    use brewed_texture_engine::wgpu::hal::api;
    // SAFETY: each guard is only asked whether it is there and let go at once; nothing of the
    // device under it is touched.
    unsafe {
        #[cfg(not(target_vendor = "apple"))]
        if device.as_hal::<api::Vulkan>().is_some() {
            return "Vulkan";
        }
        #[cfg(windows)]
        if device.as_hal::<api::Dx12>().is_some() {
            return "DirectX 12";
        }
        #[cfg(target_vendor = "apple")]
        if device.as_hal::<api::Metal>().is_some() {
            return "Metal";
        }
        #[cfg(not(target_vendor = "apple"))]
        if device.as_hal::<api::Gles>().is_some() {
            return "OpenGL";
        }
    }
    ""
}

/// The GPU's name, and the backend in brackets where it is known: `RTX 4070 SUPER (Vulkan)`.
pub fn said(adapter: &str, backend: &str) -> String {
    match (adapter.is_empty(), backend.is_empty()) {
        (_, true) => adapter.to_string(),
        (true, false) => format!("GPU ({backend})"),
        (false, false) => format!("{adapter} ({backend})"),
    }
}

/// Name of the GPU, for display only (iced does not expose the adapter it picked).
///
/// Only enumerates Vulkan / Metal adapters, without debug layers: probing D3D12 creates
/// a test device, and with the debug layer enabled that reset the GPU while the UI's
/// device was in use.
pub fn adapter_name() -> String {
    let backends = wgpu::Backends::VULKAN | wgpu::Backends::METAL;
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor { backends, flags: wgpu::InstanceFlags::empty(), ..Default::default() });
    let adapters = instance.enumerate_adapters(backends);
    let best = adapters.iter().map(|a| a.get_info()).max_by_key(|info| match info.device_type {
        wgpu::DeviceType::DiscreteGpu => 3,
        wgpu::DeviceType::IntegratedGpu => 2,
        wgpu::DeviceType::VirtualGpu => 1,
        _ => 0,
    });
    best.map_or_else(|| "GPU".to_string(), |info| info.name)
}

/// Receiving end of the engine's events, wrapped so it can identify a subscription.
#[derive(Clone)]
pub struct EventSource(Arc<Mutex<Option<mpsc::UnboundedReceiver<Event>>>>);

impl Hash for EventSource {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "brewed-texture-engine-events".hash(state);
    }
}

impl EventSource {
    pub fn subscription(&self) -> Subscription<Event> {
        Subscription::run_with(self.clone(), |source| {
            let rx = source.0.lock().ok().and_then(|mut r| r.take());
            match rx {
                Some(rx) => futures::stream::StreamExt::boxed(rx),
                None => futures::stream::StreamExt::boxed(futures::stream::pending()),
            }
        })
    }
}

/// Start the engine on the shared device.
pub fn start_engine(handles: GpuHandles, adapter: String) -> (EngineService, EventSource) {
    let (tx, rx) = mpsc::unbounded();
    let emit: brewed_texture_engine::service::Emit = Arc::new(move |event| {
        let _ = tx.unbounded_send(event);
    });
    let service = EngineService::spawn(handles.device, handles.queue, adapter, emit);
    (service, EventSource(Arc::new(Mutex::new(Some(rx)))))
}
