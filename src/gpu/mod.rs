//! GPU context: compute kernels, image allocation, dispatch and readback.
//!
//! Every node image is an `rgba16float` storage texture. Each kernel follows the same
//! binding convention: `@binding(0)` uniform parameters, `@binding(1..=N)` input
//! textures, `@binding(N+1)` the output storage texture. That lets one bind group
//! layout per input count serve all kernels.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use eframe::wgpu;
use wgpu::util::DeviceExt;

pub const IMAGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const WORKGROUP: u32 = 8;
const BYTES_PER_PIXEL: u32 = 8;

pub const COMMON_WGSL: &str = include_str!("shaders/common.wgsl");

struct KernelSource {
    name: &'static str,
    source: &'static str,
    entry: &'static str,
    inputs: usize,
}

macro_rules! kernel {
    ($name:expr, $file:expr, $entry:expr, $inputs:expr) => {
        KernelSource { name: $name, source: include_str!(concat!("shaders/", $file)), entry: $entry, inputs: $inputs }
    };
}

static KERNELS: &[KernelSource] = &[
    kernel!("uniform_color", "uniform_color.wgsl", "main", 0),
    kernel!("shape", "shape.wgsl", "main", 0),
    kernel!("tile_gen", "tile_gen.wgsl", "main", 1),
    kernel!("gradient", "gradient.wgsl", "main", 0),
    kernel!("noise", "noise.wgsl", "main", 0),
    kernel!("transform", "transform.wgsl", "main", 1),
    kernel!("blend", "blend.wgsl", "main", 3),
    kernel!("blur_1d", "blur.wgsl", "blur_1d", 1),
    kernel!("downsample", "blur.wgsl", "downsample", 1),
    kernel!("resample", "blur.wgsl", "resample", 1),
    kernel!("sharpen", "sharpen.wgsl", "main", 2),
    kernel!("warp", "warp.wgsl", "main", 2),
    kernel!("mirror", "mirror.wgsl", "main", 1),
    kernel!("normal", "normal.wgsl", "main", 1),
    kernel!("adjust", "adjust.wgsl", "main", 1),
    kernel!("shuffle", "shuffle.wgsl", "main", 2),
    kernel!("preview", "display.wgsl", "preview", 1),
    kernel!("mip", "display.wgsl", "mip", 1),
    kernel!("thumbnail", "display.wgsl", "thumbnail", 1),
];

/// Names of all built-in kernels (used by tests).
pub fn kernel_names() -> impl Iterator<Item = &'static str> {
    KERNELS.iter().map(|k| k.name)
}

/// Full WGSL source (common prelude + kernel) of a built-in kernel.
pub fn kernel_source(name: &str) -> Option<String> {
    KERNELS.iter().find(|k| k.name == name).map(|k| format!("{COMMON_WGSL}\n{}", k.source))
}

/// A GPU image with its default view.
#[derive(Clone, Debug)]
pub struct GpuImage {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

impl GpuImage {
    pub fn width(&self) -> u32 {
        self.texture.width()
    }
    pub fn height(&self) -> u32 {
        self.texture.height()
    }
    pub fn size(&self) -> [u32; 2] {
        [self.width(), self.height()]
    }
    /// View of a single mip level (for writing mip chains).
    pub fn mip_view(&self, level: u32) -> wgpu::TextureView {
        self.texture.create_view(&wgpu::TextureViewDescriptor {
            base_mip_level: level,
            mip_level_count: Some(1),
            ..Default::default()
        })
    }
}

pub struct Kernel {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    inputs: usize,
}

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_info: String,
    kernels: RefCell<HashMap<String, Result<Rc<Kernel>, String>>>,
    layouts: RefCell<HashMap<usize, wgpu::BindGroupLayout>>,
    pool: RefCell<Vec<GpuImage>>,
    /// 1x1 transparent black, bound to unconnected inputs.
    pub black: GpuImage,
    /// 1x1 opaque white (e.g. an unconnected blend mask).
    pub white: GpuImage,
    /// Errors reported by wgpu outside of error scopes; shown in the status bar.
    pub errors: Arc<Mutex<Vec<String>>>,
}

impl Gpu {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, adapter_info: String) -> Gpu {
        let errors: Arc<Mutex<Vec<String>>> = Arc::default();
        let sink = errors.clone();
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            log::error!("wgpu error: {e}");
            if let Ok(mut v) = sink.lock() {
                v.push(e.to_string());
                if v.len() > 16 {
                    v.remove(0);
                }
            }
        }));
        let black = Self::create_image_raw(&device, 1, 1, 1, "black");
        let white = Self::create_image_raw(&device, 1, 1, 1, "white");
        let gpu = Gpu {
            device,
            queue,
            adapter_info,
            kernels: RefCell::default(),
            layouts: RefCell::default(),
            pool: RefCell::default(),
            black,
            white,
            errors,
        };
        gpu.upload_rgba_f32(&gpu.black, &[0.0, 0.0, 0.0, 0.0]);
        gpu.upload_rgba_f32(&gpu.white, &[1.0, 1.0, 1.0, 1.0]);
        gpu
    }

    /// Create a device without a window (tests, command line tools).
    pub fn headless() -> Result<Gpu, String> {
        pollster::block_on(async {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::PRIMARY,
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            });
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    ..Default::default()
                })
                .await
                .map_err(|e| format!("No GPU adapter: {e}"))?;
            let info = adapter.get_info();
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("ferrotex headless"),
                    required_limits: wgpu::Limits { max_texture_dimension_2d: 8192, ..wgpu::Limits::default() },
                    ..Default::default()
                })
                .await
                .map_err(|e| format!("Cannot create GPU device: {e}"))?;
            Ok(Gpu::new(device, queue, format!("{} ({:?})", info.name, info.backend)))
        })
    }

    fn create_image_raw(device: &wgpu::Device, w: u32, h: u32, mips: u32, label: &str) -> GpuImage {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
            mip_level_count: mips.max(1),
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: IMAGE_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        GpuImage { texture, view }
    }

    pub fn create_image(&self, w: u32, h: u32, label: &str) -> GpuImage {
        Self::create_image_raw(&self.device, w, h, 1, label)
    }

    pub fn create_image_with_mips(&self, w: u32, h: u32, label: &str) -> GpuImage {
        let mips = 32 - w.max(h).max(1).leading_zeros();
        Self::create_image_raw(&self.device, w, h, mips, label)
    }

    /// Borrow a scratch image of the given size from the pool.
    pub fn take_scratch(&self, w: u32, h: u32) -> GpuImage {
        let mut pool = self.pool.borrow_mut();
        if let Some(i) = pool.iter().position(|img| img.size() == [w, h]) {
            return pool.swap_remove(i);
        }
        drop(pool);
        self.create_image(w, h, "scratch")
    }

    /// Return a scratch image. Reusing it later in the same command encoder is safe:
    /// commands execute in order.
    pub fn recycle(&self, img: GpuImage) {
        let mut pool = self.pool.borrow_mut();
        pool.push(img);
        if pool.len() > 12 {
            pool.remove(0);
        }
    }

    pub fn clear_pool(&self) {
        self.pool.borrow_mut().clear();
    }

    /// Upload tightly packed RGBA f32 pixels (converted to half floats with SIMD).
    pub fn upload_rgba_f32(&self, img: &GpuImage, rgba: &[f32]) {
        let mut half = vec![0u16; rgba.len()];
        crate::simd::f32_to_f16(rgba, &mut half);
        self.upload_rgba_f16(img, &half);
    }

    pub fn upload_rgba_f16(&self, img: &GpuImage, half: &[u16]) {
        let (w, h) = (img.width(), img.height());
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &img.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(half),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * BYTES_PER_PIXEL),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    fn layout(&self, inputs: usize) -> wgpu::BindGroupLayout {
        if let Some(l) = self.layouts.borrow().get(&inputs) {
            return l.clone();
        }
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }];
        for i in 0..inputs {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: 1 + i as u32,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
        }
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 1 + inputs as u32,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::StorageTexture {
                access: wgpu::StorageTextureAccess::WriteOnly,
                format: IMAGE_FORMAT,
                view_dimension: wgpu::TextureViewDimension::D2,
            },
            count: None,
        });
        let layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&format!("layout_{inputs}_inputs")),
            entries: &entries,
        });
        self.layouts.borrow_mut().insert(inputs, layout.clone());
        layout
    }

    /// Compile WGSL into a compute kernel, capturing validation errors instead of
    /// panicking.
    fn build_kernel(&self, label: &str, source: &str, entry: &str, inputs: usize) -> Result<Rc<Kernel>, String> {
        let layout = self.layout(inputs);
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(label),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        });
        if let Some(err) = pollster::block_on(scope.pop()) {
            log::error!("kernel '{label}' failed to compile: {err}");
            return Err(format!("Shader error in '{label}': {err}"));
        }
        Ok(Rc::new(Kernel { pipeline, layout, inputs }))
    }

    /// A built-in kernel, compiled on first use.
    pub fn kernel(&self, name: &str) -> Result<Rc<Kernel>, String> {
        if let Some(k) = self.kernels.borrow().get(name) {
            return k.clone();
        }
        let src = KERNELS.iter().find(|k| k.name == name).ok_or_else(|| format!("unknown kernel '{name}'"))?;
        let source = format!("{COMMON_WGSL}\n{}", src.source);
        let result = self.build_kernel(name, &source, src.entry, src.inputs);
        self.kernels.borrow_mut().insert(name.to_string(), result.clone());
        result
    }

    /// A kernel generated at runtime (Pixel Math), cached by its source text.
    pub fn dynamic_kernel(&self, source: &str, inputs: usize) -> Result<Rc<Kernel>, String> {
        let key = format!("dyn:{inputs}:{source}");
        if let Some(k) = self.kernels.borrow().get(&key) {
            return k.clone();
        }
        let result = self.build_kernel("pixel_math", source, "main", inputs);
        let mut cache = self.kernels.borrow_mut();
        if cache.len() > 256 {
            cache.retain(|k, _| !k.starts_with("dyn:"));
        }
        cache.insert(key, result.clone());
        result
    }

    /// Record one compute dispatch covering `output`.
    pub fn dispatch(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        kernel: &Kernel,
        params: &[u8],
        inputs: &[&wgpu::TextureView],
        output: &wgpu::TextureView,
        size: [u32; 2],
    ) {
        debug_assert_eq!(inputs.len(), kernel.inputs, "kernel input count mismatch");
        let mut contents = params.to_vec();
        let padded = contents.len().div_ceil(16).max(1) * 16;
        contents.resize(padded, 0);
        let buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &contents,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let mut entries = vec![wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }];
        for (i, view) in inputs.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry {
                binding: 1 + i as u32,
                resource: wgpu::BindingResource::TextureView(view),
            });
        }
        entries.push(wgpu::BindGroupEntry {
            binding: 1 + inputs.len() as u32,
            resource: wgpu::BindingResource::TextureView(output),
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &kernel.layout,
            entries: &entries,
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
        pass.set_pipeline(&kernel.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(size[0].div_ceil(WORKGROUP), size[1].div_ceil(WORKGROUP), 1);
    }

    /// Convenience: look up a kernel by name and dispatch it.
    pub fn run(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        kernel: &str,
        params: &[u8],
        inputs: &[&wgpu::TextureView],
        output: &GpuImage,
    ) -> Result<(), String> {
        let k = self.kernel(kernel)?;
        self.dispatch(encoder, &k, params, inputs, &output.view, output.size());
        Ok(())
    }

    pub fn encoder(&self) -> wgpu::CommandEncoder {
        self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ferrotex") })
    }

    /// Record a copy of `img` into a mappable buffer. Call [`Readback::map`] after
    /// the encoder has been submitted.
    pub fn encode_readback(&self, encoder: &mut wgpu::CommandEncoder, img: &GpuImage) -> Readback {
        let (w, h) = (img.width(), img.height());
        let row = w * BYTES_PER_PIXEL;
        let padded_row = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: padded_row as u64 * h as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &img.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        Readback { buffer, width: w, height: h, padded_row, state: Arc::new(AtomicU8::new(STATE_IDLE)) }
    }

    /// Record a copy of a single pixel (for the color probe of the 2D view).
    pub fn encode_readback_region(&self, encoder: &mut wgpu::CommandEncoder, img: &GpuImage, pixel: [u32; 2]) -> Readback {
        let padded_row = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("probe"),
            size: padded_row as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &img.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: pixel[0], y: pixel[1], z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded_row), rows_per_image: Some(1) },
            },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        Readback { buffer, width: 1, height: 1, padded_row, state: Arc::new(AtomicU8::new(STATE_IDLE)) }
    }

    /// Read an image back to the CPU as RGBA f32, blocking until the GPU is done.
    pub fn read_image_blocking(&self, img: &GpuImage) -> Result<Vec<f32>, String> {
        let mut encoder = self.encoder();
        let rb = self.encode_readback(&mut encoder, img);
        self.queue.submit(Some(encoder.finish()));
        rb.map();
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| format!("GPU poll failed: {e}"))?;
        match rb.poll() {
            Some(Ok(())) => Ok(rb.to_f32()),
            Some(Err(e)) => Err(e),
            None => Err("GPU readback did not finish".into()),
        }
    }

    /// Non-blocking device poll so pending readbacks make progress.
    pub fn poll(&self) {
        let _ = self.device.poll(wgpu::PollType::Poll);
    }

    pub fn take_errors(&self) -> Vec<String> {
        self.errors.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default()
    }
}

const STATE_IDLE: u8 = 0;
const STATE_PENDING: u8 = 1;
const STATE_READY: u8 = 2;
const STATE_FAILED: u8 = 3;

/// A texture copy on its way back to the CPU.
pub struct Readback {
    buffer: wgpu::Buffer,
    pub width: u32,
    pub height: u32,
    padded_row: u32,
    state: Arc<AtomicU8>,
}

impl Readback {
    /// Start mapping. Must be called after the copy has been submitted.
    pub fn map(&self) {
        self.state.store(STATE_PENDING, Ordering::SeqCst);
        let state = self.state.clone();
        self.buffer.map_async(wgpu::MapMode::Read, .., move |result| {
            state.store(if result.is_ok() { STATE_READY } else { STATE_FAILED }, Ordering::SeqCst);
        });
    }

    /// `None` while pending.
    pub fn poll(&self) -> Option<Result<(), String>> {
        match self.state.load(Ordering::SeqCst) {
            STATE_READY => Some(Ok(())),
            STATE_FAILED => Some(Err("Mapping the readback buffer failed".into())),
            _ => None,
        }
    }

    /// Raw half-float pixels without row padding. Requires a successful map.
    pub fn to_f16(&self) -> Vec<u16> {
        let row = (self.width * BYTES_PER_PIXEL) as usize;
        let mut out = Vec::with_capacity(row / 2 * self.height as usize);
        if let Ok(view) = self.buffer.get_mapped_range(..) {
            for y in 0..self.height as usize {
                let start = y * self.padded_row as usize;
                out.extend_from_slice(bytemuck::cast_slice::<u8, u16>(&view[start..start + row]));
            }
        }
        self.buffer.unmap();
        out
    }

    /// Pixels as RGBA f32 (half floats expanded with SIMD on all cores).
    pub fn to_f32(&self) -> Vec<f32> {
        let half = self.to_f16();
        let mut out = vec![0.0f32; half.len()];
        crate::simd::f16_to_f32(&half, &mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_builtin_kernels_compile() {
        let gpu = match Gpu::headless() {
            Ok(g) => g,
            Err(e) => {
                eprintln!("skipping GPU test: {e}");
                return;
            }
        };
        let mut failures = Vec::new();
        for name in kernel_names() {
            if let Err(e) = gpu.kernel(name) {
                failures.push(format!("{name}: {e}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    }

    #[test]
    fn broken_dynamic_kernel_is_an_error_not_a_panic() {
        let Ok(gpu) = Gpu::headless() else { return };
        assert!(gpu.dynamic_kernel("this is not wgsl", 0).is_err());
    }
}
