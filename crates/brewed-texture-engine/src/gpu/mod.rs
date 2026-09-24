//! GPU context: kernels, image allocation, drawing and readback.
//!
//! A node image is a grayscale image, one channel ([`GRAY_FORMAT`]), or a color image
//! ([`COLOR_FORMAT`]); the views sample the node images themselves, with their mips.
//!
//! A kernel draws its image: a render pass whose one triangle covers the image, and whose
//! fragment entry computes each pixel. A render pass writes any format a device renders to,
//! where a compute shader could only store to a few: a grayscale image is written as it is
//! kept, in one channel of 16 bits, with no image of 32 bits to compute it in first. Each
//! kernel follows the same binding convention: `@binding(0)` uniform parameters,
//! `@binding(1..=N)` input textures, a sampler after them for the kernels that filter their
//! inputs, and `@binding(15)` the size of the image drawn (see common.wgsl). A kernel is
//! written once and built for the images it is run on: the format it writes, and which of
//! its inputs are grayscale (it reads those through `widen`, see show.wgsl).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use brewed_texture_core::nodes::ImageKind;
use wgpu::util::DeviceExt;

/// Color images: red, green, blue and alpha as half floats.
pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Grayscale images: one channel as a half float, a quarter of the memory of a color image.
pub const GRAY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;
/// Where the size of the image a kernel draws is bound (see common.wgsl).
const TARGET_BINDING: u32 = 15;
/// How many pixels a kernel draws at most in one draw (see [`Gpu::draw`]).
const BAND_PIXELS: u32 = 1 << 20;

/// How a node's image is read (`widen`) and an output writes it (`as_output`): the WGSL the
/// kernels read their inputs with, for the views that sample the node images to read them
/// the same way.
pub const SHOW_WGSL: &str = include_str!("shaders/show.wgsl");
/// What every kernel starts with: [`SHOW_WGSL`] and the kernels' shared helpers.
pub const COMMON_WGSL: &str = concat!(include_str!("shaders/show.wgsl"), include_str!("shaders/common.wgsl"));

/// The parameters of the copies of blit.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct CopyU {
    format: u32,
    pad: [u32; 3],
}

/// The format images of a kind are stored in.
pub fn format_of(kind: ImageKind) -> wgpu::TextureFormat {
    match kind {
        ImageKind::Gray => GRAY_FORMAT,
        ImageKind::Color => COLOR_FORMAT,
    }
}

/// The format an output keeps the image it writes in, from its format (see
/// [`brewed_texture_core::nodes::OUTPUT_FORMATS`]) and bit depth (see [`crate::export`]),
/// and what arrives at it: the channels it writes, one, two or four (a device has no
/// textures of three: RGB is kept with an opaque alpha), of 8 bits, or of half floats, which
/// hold all a node's image has for 16 bits and 32. What arrives gray is one channel in any
/// format but RG.
pub fn output_format(format: u32, bit_depth: u32, arriving: ImageKind) -> wgpu::TextureFormat {
    use wgpu::TextureFormat as F;
    let channels = match (format, arriving) {
        (2, _) => 2,
        (0 | 1, ImageKind::Color) => 4,
        _ => 1,
    };
    match (channels, bit_depth == crate::export::DEPTH_8) {
        (1, true) => F::R8Unorm,
        (1, false) => F::R16Float,
        (2, true) => F::Rg8Unorm,
        (2, false) => F::Rg16Float,
        (_, true) => F::Rgba8Unorm,
        (_, false) => F::Rgba16Float,
    }
}

fn bytes_per_pixel(format: wgpu::TextureFormat) -> u32 {
    format.block_copy_size(None).unwrap_or(8)
}

struct KernelSource {
    name: &'static str,
    source: &'static str,
    entry: &'static str,
    inputs: usize,
    /// Reads its inputs through a trilinear sampler (bound after the output), which
    /// wraps on the axes the node tiles on.
    filtered: bool,
    /// How many kinds of image it makes, each built on its own (the `KIND` of common.wgsl).
    kinds: u32,
}

macro_rules! kernel {
    ($name:expr, $file:expr, $entry:expr, $inputs:expr) => {
        kinded_kernel!($name, $file, $entry, $inputs, 1)
    };
}

macro_rules! kinded_kernel {
    ($name:expr, $file:expr, $entry:expr, $inputs:expr, $kinds:expr) => {
        KernelSource {
            name: $name,
            source: include_str!(concat!("shaders/", $file)),
            entry: $entry,
            inputs: $inputs,
            filtered: false,
            kinds: $kinds,
        }
    };
}

macro_rules! filtered_kernel {
    ($name:expr, $file:expr, $entry:expr, $inputs:expr) => {
        KernelSource {
            name: $name,
            source: include_str!(concat!("shaders/", $file)),
            entry: $entry,
            inputs: $inputs,
            filtered: true,
            kinds: 1,
        }
    };
}

static KERNELS: &[KernelSource] = &[
    kernel!("uniform_color", "uniform_color.wgsl", "main", 0),
    kernel!("shape", "shape.wgsl", "main", 0),
    kernel!("tile_gen", "tile_gen.wgsl", "main", 1),
    kernel!("gradient", "gradient.wgsl", "main", 0),
    // One pipeline per type of noise (and white noise after them).
    kinded_kernel!("noise", "noise.wgsl", "main", 0, brewed_texture_core::nodes::NOISE_TYPES.len() as u32),
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
    kernel!("gradient_map", "gradient_map.wgsl", "main", 2),
    kernel!("shuffle", "shuffle.wgsl", "main", 2),
    kernel!("directional_blur", "directional.wgsl", "main", 1),
    kernel!("directional_warp", "slope.wgsl", "directional_warp", 2),
    kernel!("slope_blur", "slope.wgsl", "slope_blur", 2),
    filtered_kernel!("ambient_occlusion", "ao.wgsl", "main", 1),
    kernel!("curvature", "height.wgsl", "curvature", 1),
    kernel!("edge_detect", "height.wgsl", "edge_detect", 1),
    kernel!("emboss", "emboss.wgsl", "main", 2),
    kernel!("thumbnail", "display.wgsl", "thumbnail", 1),
    kernel!("show", "blit.wgsl", "show", 1),
    kernel!("mip", "blit.wgsl", "mip", 1),
];

/// "GPU name (backend)" for status bars and logs.
pub fn adapter_description(info: &wgpu::AdapterInfo) -> String {
    format!("{} ({:?})", info.name, info.backend)
}

/// Names of all built-in kernels (used by tests).
pub fn kernel_names() -> impl Iterator<Item = &'static str> {
    KERNELS.iter().map(|k| k.name)
}

/// Full WGSL source (common prelude + kernel) of a built-in kernel.
fn kernel_source(src: &KernelSource) -> String {
    format!("{COMMON_WGSL}\n{}", src.source)
}

/// GPU memory an image texture takes, with all its mip levels.
pub fn texture_bytes(texture: &wgpu::Texture) -> u64 {
    let (mut w, mut h) = (u64::from(texture.width()), u64::from(texture.height()));
    let pixel = u64::from(bytes_per_pixel(texture.format()));
    let mut total = 0;
    for _ in 0..texture.mip_level_count() {
        total += w * h * pixel;
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    total
}

/// A GPU image with its default view (or the view of one of its mip levels).
#[derive(Clone, Debug)]
pub struct GpuImage {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    /// The mip level the view shows first.
    pub level: u32,
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
    /// The size of the level its view shows first.
    pub fn level_size(&self) -> [u32; 2] {
        [(self.width() >> self.level).max(1), (self.height() >> self.level).max(1)]
    }
    pub fn format(&self) -> wgpu::TextureFormat {
        self.texture.format()
    }
    /// One channel: a grayscale image, or what an output writes of a single channel.
    pub fn is_gray(&self) -> bool {
        self.format().components() == 1
    }
    pub fn kind(&self) -> ImageKind {
        if self.is_gray() { ImageKind::Gray } else { ImageKind::Color }
    }
    /// GPU memory the image takes, with all its mip levels.
    pub fn bytes(&self) -> u64 {
        texture_bytes(&self.texture)
    }
    /// One mip level of the image, as an image of its own (for writing mip chains).
    pub fn level(&self, level: u32) -> GpuImage {
        let view = self.texture.create_view(&wgpu::TextureViewDescriptor {
            base_mip_level: level,
            mip_level_count: Some(1),
            ..Default::default()
        });
        GpuImage { texture: self.texture.clone(), view, level }
    }
}

pub struct Kernel {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    inputs: usize,
    filtered: bool,
}

/// A kernel as built for the images it runs on: its name (or source), the format it writes,
/// which of its inputs are grayscale, and the kind of image it makes.
type KernelKey = (String, wgpu::TextureFormat, u8, u32);

/// Scratch images kept for reuse, each with whether it was used since the last evaluation
/// began (see [`Gpu::start_use`]).
type Pool = Vec<(GpuImage, bool)>;

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_info: String,
    kernels: RefCell<HashMap<KernelKey, Result<Rc<Kernel>, String>>>,
    layouts: RefCell<HashMap<(usize, bool), wgpu::BindGroupLayout>>,
    /// Trilinear samplers for the kernels that filter their inputs, by tiling: bit 0
    /// repeats horizontally, bit 1 vertically; the other axes clamp.
    samplers: [wgpu::Sampler; 4],
    pool: RefCell<Pool>,
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
        let sampler = |tiling: u32| {
            let mode = |repeat: bool| if repeat { wgpu::AddressMode::Repeat } else { wgpu::AddressMode::ClampToEdge };
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("filtered input"),
                address_mode_u: mode(tiling & 1 != 0),
                address_mode_v: mode(tiling & 2 != 0),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            })
        };
        let samplers = [sampler(0), sampler(1), sampler(2), sampler(3)];
        let black = Self::create_image_raw(&device, 1, 1, 1, COLOR_FORMAT, "black");
        let white = Self::create_image_raw(&device, 1, 1, 1, COLOR_FORMAT, "white");
        let gpu = Gpu {
            device,
            queue,
            adapter_info,
            kernels: RefCell::default(),
            layouts: RefCell::default(),
            samplers,
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
        Self::headless_on(wgpu::Backends::PRIMARY)
    }

    /// [`Gpu::headless`] on one of `backends`.
    pub fn headless_on(backends: wgpu::Backends) -> Result<Gpu, String> {
        pollster::block_on(async {
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor { backends, ..Default::default() });
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    force_fallback_adapter: false,
                    compatible_surface: None,
                })
                .await
                .map_err(|e| format!("No GPU adapter: {e}"))?;
            let info = adapter.get_info();
            // The same limits and features the UI's device is created with, so tests see
            // what the app sees.
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("brewed headless"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits { max_bind_groups: 2, ..wgpu::Limits::default() },
                    memory_hints: wgpu::MemoryHints::MemoryUsage,
                    trace: wgpu::Trace::Off,
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                })
                .await
                .map_err(|e| format!("Cannot create GPU device: {e}"))?;
            Ok(Gpu::new(device, queue, adapter_description(&info)))
        })
    }

    fn create_image_raw(
        device: &wgpu::Device,
        w: u32,
        h: u32,
        mips: u32,
        format: wgpu::TextureFormat,
        label: &str,
    ) -> GpuImage {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
            mip_level_count: mips.max(1),
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            // Drawn by kernels, read by kernels and views, copied and read back.
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        GpuImage { texture, view, level: 0 }
    }

    /// A color image.
    pub fn create_image(&self, w: u32, h: u32, label: &str) -> GpuImage {
        Self::create_image_raw(&self.device, w, h, 1, COLOR_FORMAT, label)
    }

    /// An image of `mips` levels, as the pool makes them (for measurements).
    #[cfg(test)]
    pub(crate) fn create_image_raw_for_tests(
        &self,
        w: u32,
        h: u32,
        mips: u32,
        format: wgpu::TextureFormat,
    ) -> GpuImage {
        Self::create_image_raw(&self.device, w, h, mips, format, "measured")
    }

    pub fn create_image_as(&self, w: u32, h: u32, format: wgpu::TextureFormat, label: &str) -> GpuImage {
        Self::create_image_raw(&self.device, w, h, 1, format, label)
    }

    /// An image with a full mip chain.
    pub fn create_image_with_mips(&self, w: u32, h: u32, format: wgpu::TextureFormat, label: &str) -> GpuImage {
        let mips = 32 - w.max(h).max(1).leading_zeros();
        Self::create_image_raw(&self.device, w, h, mips, format, label)
    }

    /// Record `src` as its node shows it (as the output of `format`, see
    /// [`brewed_texture_core::nodes::OUTPUT_FORMATS`]; 0 shows it as it is) into mip 0 of
    /// `dst`, resampled to its size and converted to its format, then rebuild the mip chain.
    pub fn encode_copy_with_mips(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        src: &GpuImage,
        dst: &GpuImage,
        format: u32,
    ) -> Result<(), String> {
        self.encode_show(encoder, src, &dst.level(0), format)?;
        self.encode_mips(encoder, dst)
    }

    /// Record the mip chain of `image` from its top level: each level the average of the four
    /// pixels under each of its pixels on the level above.
    pub fn encode_mips(&self, encoder: &mut wgpu::CommandEncoder, image: &GpuImage) -> Result<(), String> {
        for level in 1..image.texture.mip_level_count() {
            let (above, level) = (image.level(level - 1), image.level(level));
            self.run(encoder, "mip", bytemuck::bytes_of(&CopyU::default()), &[&above], &level)?;
        }
        Ok(())
    }

    /// Record `src` as its node shows it into `dst` (its top level), resampled to its size
    /// and converted to its format.
    pub fn encode_show(&self, encoder: &mut wgpu::CommandEncoder, src: &GpuImage, dst: &GpuImage, format: u32) -> Result<(), String> {
        self.run(encoder, "show", bytemuck::bytes_of(&CopyU { format, pad: [0; 3] }), &[src], dst)
    }

    /// Record clearing `image` (its top level) to transparent black, whatever its format.
    pub fn encode_clear(&self, encoder: &mut wgpu::CommandEncoder, image: &GpuImage) {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &image.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }

    /// A one-row lookup table (e.g. a baked gradient).
    pub fn create_lut(&self, colors: &[[f32; 4]]) -> GpuImage {
        let img = self.create_image(colors.len().max(1) as u32, 1, "lut");
        let flat: Vec<f32> = colors.iter().flatten().copied().collect();
        self.upload_rgba_f32(&img, &flat);
        img
    }

    /// Borrow a scratch image of the given size and format from the pool.
    pub fn take_scratch(&self, w: u32, h: u32, format: wgpu::TextureFormat) -> GpuImage {
        self.take_pooled(w, h, format, 1).unwrap_or_else(|| self.create_image_as(w, h, format, "scratch"))
    }

    /// Borrow a scratch image of the given size and format, with a full mip chain.
    pub fn take_scratch_with_mips(&self, w: u32, h: u32, format: wgpu::TextureFormat) -> GpuImage {
        let mips = 32 - w.max(h).max(1).leading_zeros();
        self.take_pooled(w, h, format, mips).unwrap_or_else(|| self.create_image_with_mips(w, h, format, "scratch mips"))
    }

    fn take_pooled(&self, w: u32, h: u32, format: wgpu::TextureFormat, mips: u32) -> Option<GpuImage> {
        let mut pool = self.pool.borrow_mut();
        let i = pool
            .iter()
            .position(|(img, _)| img.size() == [w, h] && img.format() == format && img.texture.mip_level_count() == mips)?;
        Some(pool.swap_remove(i).0)
    }

    /// Return a scratch image. Reusing it later in the same command encoder is safe:
    /// commands execute in order.
    pub fn recycle(&self, img: GpuImage) {
        let mut pool = self.pool.borrow_mut();
        pool.push((img, true));
        // However a graph is built, a few dozen are plenty.
        if pool.len() > 32 {
            pool.remove(0);
        }
    }

    /// An evaluation begins: the scratch images it does not use are let go of at the next
    /// trim.
    pub fn start_use(&self) {
        for (_, used) in self.pool.borrow_mut().iter_mut() {
            *used = false;
        }
    }

    /// Let go of the scratch images the last evaluation did not use: what one change needs,
    /// the next one likely needs too, and the rest only takes memory. Of those it used, keep
    /// at most `budget` bytes: the smallest first, which save the most time for their memory
    /// (a new image costs its allocation whatever its size, and then some for each byte).
    pub fn trim_pool(&self, budget: u64) {
        let mut pool = self.pool.borrow_mut();
        pool.retain(|(_, used)| *used);
        pool.sort_by_key(|(img, _)| img.bytes());
        let mut kept = 0;
        pool.retain(|(img, _)| {
            kept += img.bytes();
            kept <= budget
        });
    }

    pub fn clear_pool(&self) {
        self.pool.borrow_mut().clear();
    }

    /// GPU memory the scratch images kept for reuse take.
    pub fn pool_bytes(&self) -> u64 {
        self.pool.borrow().iter().map(|(img, _)| img.bytes()).sum()
    }

    /// The scratch images kept for reuse (for measurements).
    #[cfg(test)]
    pub(crate) fn pooled(&self) -> Vec<GpuImage> {
        self.pool.borrow().iter().map(|(img, _)| img.clone()).collect()
    }

    /// Upload tightly packed RGBA f32 pixels to a color image (converted to half floats
    /// with SIMD).
    pub fn upload_rgba_f32(&self, img: &GpuImage, rgba: &[f32]) {
        let mut half = vec![0u16; rgba.len()];
        crate::simd::f32_to_f16(rgba, &mut half);
        self.upload_rgba_f16(img, &half);
    }

    pub fn upload_rgba_f16(&self, img: &GpuImage, half: &[u16]) {
        self.upload(img, bytemuck::cast_slice(half));
    }

    /// Upload one value per pixel to a grayscale image (as half floats, converted with
    /// SIMD).
    pub fn upload_gray_f32(&self, img: &GpuImage, values: &[f32]) {
        let mut half = vec![0u16; values.len()];
        crate::simd::f32_to_f16(values, &mut half);
        self.upload(img, bytemuck::cast_slice(&half));
    }

    fn upload(&self, img: &GpuImage, data: &[u8]) {
        let (w, h) = (img.width(), img.height());
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &img.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * bytes_per_pixel(img.format())),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    fn layout(&self, inputs: usize, filtered: bool) -> wgpu::BindGroupLayout {
        if let Some(l) = self.layouts.borrow().get(&(inputs, filtered)) {
            return l.clone();
        }
        let uniform = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let mut entries = vec![uniform(0)];
        for i in 0..inputs {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: 1 + i as u32,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    // Read texel by texel, or through a sampler by the kernels that filter.
                    sample_type: wgpu::TextureSampleType::Float { filterable: filtered },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
        }
        if filtered {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: 1 + inputs as u32,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            });
        }
        entries.push(uniform(TARGET_BINDING));
        let layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&format!("layout_{inputs}_inputs{}", if filtered { "_filtered" } else { "" })),
            entries: &entries,
        });
        self.layouts.borrow_mut().insert((inputs, filtered), layout.clone());
        layout
    }

    /// Build WGSL into a kernel drawing `output`, capturing validation errors instead of
    /// panicking. `gray` has bit n set when input n is grayscale; `kind` is the kind of image
    /// it makes.
    #[allow(clippy::too_many_arguments)]
    fn build_kernel(
        &self,
        label: &str,
        source: &str,
        entry: &str,
        inputs: usize,
        filtered: bool,
        output: wgpu::TextureFormat,
        gray: u8,
        kind: u32,
    ) -> Result<Rc<Kernel>, String> {
        let layout = self.layout(inputs, filtered);
        self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let bit = |n: u8| if gray & (1 << n) != 0 { 1.0 } else { 0.0 };
        let constants = [("GRAY0", bit(0)), ("GRAY1", bit(1)), ("GRAY2", bit(2)), ("GRAY3", bit(3)), ("KIND", f64::from(kind))];
        let pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("cover"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(entry),
                compilation_options: wgpu::PipelineCompilationOptions { constants: &constants, ..Default::default() },
                targets: &[Some(wgpu::ColorTargetState {
                    format: output,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        if let Some(err) = pollster::block_on(self.device.pop_error_scope()) {
            log::error!("kernel '{label}' failed to compile: {err}");
            return Err(format!("Shader error in '{label}': {err}"));
        }
        Ok(Rc::new(Kernel { pipeline, layout, inputs, filtered }))
    }

    /// A built-in kernel writing color images from color images, compiled on first use.
    pub fn kernel(&self, name: &str) -> Result<Rc<Kernel>, String> {
        self.kernel_variant(name, COLOR_FORMAT, 0, 0)
    }

    /// A built-in kernel as built for writing `output`, reading grayscale images at the
    /// inputs whose bits are set in `gray`, and making the kind of image `kind`.
    pub fn kernel_variant(&self, name: &str, output: wgpu::TextureFormat, gray: u8, kind: u32) -> Result<Rc<Kernel>, String> {
        let key = (name.to_string(), output, gray, kind);
        if let Some(k) = self.kernels.borrow().get(&key) {
            return k.clone();
        }
        let src = KERNELS.iter().find(|k| k.name == name).ok_or_else(|| format!("unknown kernel '{name}'"))?;
        let result = if kind < src.kinds {
            let label = if src.kinds > 1 { format!("{name} {kind}") } else { name.to_string() };
            self.build_kernel(&label, &kernel_source(src), src.entry, src.inputs, src.filtered, output, gray, kind)
        } else {
            Err(format!("'{name}' makes {} kinds of image, not a kind {kind}", src.kinds))
        };
        self.kernels.borrow_mut().insert(key, result.clone());
        result
    }

    /// A kernel generated at runtime (Pixel Math) writing `output`, cached by its source text.
    /// Its source says itself which inputs are grayscale.
    pub fn dynamic_kernel(&self, source: &str, inputs: usize, output: wgpu::TextureFormat) -> Result<Rc<Kernel>, String> {
        let key = (format!("dyn:{inputs}:{source}"), output, 0, 0);
        if let Some(k) = self.kernels.borrow().get(&key) {
            return k.clone();
        }
        let result = self.build_kernel("pixel_math", source, "main", inputs, false, output, 0, 0);
        let mut cache = self.kernels.borrow_mut();
        if cache.len() > 256 {
            cache.retain(|(k, ..), _| !k.starts_with("dyn:"));
        }
        cache.insert(key, result.clone());
        result
    }

    /// Which of `inputs` are grayscale, as the bits a kernel is built with.
    fn gray_inputs(inputs: &[&GpuImage]) -> u8 {
        inputs.iter().enumerate().filter(|(_, img)| img.is_gray()).fold(0, |bits, (i, _)| bits | (1 << i))
    }

    /// Record drawing all of `output` (one level of it) with `kernel`; a kernel that filters
    /// its inputs gets the sampler for `tiling`.
    fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        kernel: &Kernel,
        params: &[u8],
        inputs: &[&GpuImage],
        output: &GpuImage,
        tiling: u32,
    ) {
        debug_assert_eq!(inputs.len(), kernel.inputs, "kernel input count mismatch");
        // The parameters, then the size of the image drawn, where a binding may start.
        let target = u64::from(self.device.limits().min_uniform_buffer_offset_alignment);
        let params_size = (params.len().div_ceil(16).max(1) * 16) as u64;
        let target_at = params_size.div_ceil(target) * target;
        let mut contents = params.to_vec();
        contents.resize(target_at as usize, 0);
        let [w, h] = output.level_size();
        contents.extend_from_slice(bytemuck::bytes_of(&[w, h, 0, 0]));
        let buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &contents,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let range = |offset: u64, size: u64| {
            let size = wgpu::BufferSize::new(size);
            wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &buffer, offset, size })
        };
        let mut entries = vec![wgpu::BindGroupEntry { binding: 0, resource: range(0, params_size) }];
        for (i, img) in inputs.iter().enumerate() {
            let resource = wgpu::BindingResource::TextureView(&img.view);
            entries.push(wgpu::BindGroupEntry { binding: 1 + i as u32, resource });
        }
        if kernel.filtered {
            entries.push(wgpu::BindGroupEntry {
                binding: 1 + inputs.len() as u32,
                resource: wgpu::BindingResource::Sampler(&self.samplers[(tiling & 3) as usize]),
            });
        }
        entries.push(wgpu::BindGroupEntry { binding: TARGET_BINDING, resource: range(target_at, 16) });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &kernel.layout,
            entries: &entries,
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &output.view,
                depth_slice: None,
                resolve_target: None,
                // Every pixel is drawn: what was there is not read.
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&kernel.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        // In bands of rows, a draw each: a system resets a device one piece of work keeps busy
        // for long (about 2 s on Windows), and a heavy kernel on a large image on a slow device
        // gets there. A band is short whatever the kernel, the image or the device.
        let rows = (BAND_PIXELS / w).clamp(1, h);
        for y in (0..h).step_by(rows as usize) {
            pass.set_scissor_rect(0, y, w, rows.min(h - y));
            pass.draw(0..3, 0..1);
        }
    }

    /// Run a built-in kernel over `output`, built for the images it reads and writes.
    pub fn run(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        kernel: &str,
        params: &[u8],
        inputs: &[&GpuImage],
        output: &GpuImage,
    ) -> Result<(), String> {
        let k = self.kernel_variant(kernel, output.format(), Self::gray_inputs(inputs), 0)?;
        self.draw(encoder, &k, params, inputs, output, 0);
        Ok(())
    }

    /// [`Gpu::run`] of a kernel that makes several kinds of image, making `kind`.
    pub fn run_kind(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        kernel: &str,
        kind: u32,
        params: &[u8],
        inputs: &[&GpuImage],
        output: &GpuImage,
    ) -> Result<(), String> {
        let k = self.kernel_variant(kernel, output.format(), Self::gray_inputs(inputs), kind)?;
        self.draw(encoder, &k, params, inputs, output, 0);
        Ok(())
    }

    /// Run a kernel that filters its inputs, sampling them with the node's tiling.
    pub fn run_filtered(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        kernel: &str,
        params: &[u8],
        inputs: &[&GpuImage],
        output: &GpuImage,
        tiling: u32,
    ) -> Result<(), String> {
        let k = self.kernel_variant(kernel, output.format(), Self::gray_inputs(inputs), 0)?;
        self.draw(encoder, &k, params, inputs, output, tiling);
        Ok(())
    }

    /// Run a kernel generated at runtime (see [`Gpu::dynamic_kernel`]) over `output`.
    pub fn run_dynamic(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        kernel: &Kernel,
        params: &[u8],
        inputs: &[&GpuImage],
        output: &GpuImage,
    ) {
        self.draw(encoder, kernel, params, inputs, output, 0);
    }

    pub fn encoder(&self) -> wgpu::CommandEncoder {
        self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("brewed") })
    }

    /// Record a copy of `img` into a mappable buffer. Call [`Readback::map`] after
    /// the encoder has been submitted.
    pub fn encode_readback(&self, encoder: &mut wgpu::CommandEncoder, img: &GpuImage) -> Readback {
        self.encode_readback_rect(encoder, img, [0, 0], img.size())
    }

    /// Record a copy of a single pixel (for the color probe of the 2D view).
    pub fn encode_readback_region(&self, encoder: &mut wgpu::CommandEncoder, img: &GpuImage, pixel: [u32; 2]) -> Readback {
        self.encode_readback_rect(encoder, img, pixel, [1, 1])
    }

    fn encode_readback_rect(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        img: &GpuImage,
        origin: [u32; 2],
        [w, h]: [u32; 2],
    ) -> Readback {
        let format = img.format();
        let row = w * bytes_per_pixel(format);
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
                origin: wgpu::Origin3d { x: origin[0], y: origin[1], z: 0 },
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
        Readback { buffer, width: w, height: h, padded_row, format, state: Arc::new(AtomicU8::new(STATE_IDLE)) }
    }

    /// Read an image back to the CPU as RGBA f32, blocking until the GPU is done.
    pub fn read_image_blocking(&self, img: &GpuImage) -> Result<Vec<f32>, String> {
        self.read_stored_blocking(img).map(|stored| stored.to_f32())
    }

    /// Read an image back to the CPU as its texture stores it, blocking until the GPU is done.
    pub fn read_stored_blocking(&self, img: &GpuImage) -> Result<StoredImage, String> {
        let mut encoder = self.encoder();
        let rb = self.encode_readback(&mut encoder, img);
        self.queue.submit(Some(encoder.finish()));
        rb.map();
        self.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("GPU poll failed: {e}"))?;
        match rb.poll() {
            Some(Ok(())) => Ok(rb.stored()),
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
    format: wgpu::TextureFormat,
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

    /// The pixels as they are stored, without the rows' padding. Requires a successful map.
    pub fn stored(&self) -> StoredImage {
        let row = (self.width * bytes_per_pixel(self.format)) as usize;
        let mut bytes = Vec::with_capacity(row * self.height as usize);
        {
            let view = self.buffer.get_mapped_range(..);
            for y in 0..self.height as usize {
                let start = y * self.padded_row as usize;
                bytes.extend_from_slice(&view[start..start + row]);
            }
        }
        self.buffer.unmap();
        StoredImage { width: self.width, height: self.height, format: self.format, bytes }
    }

    /// Pixels as RGBA f32 (see [`StoredImage::to_f32`]). Requires a successful map.
    pub fn to_f32(&self) -> Vec<f32> {
        self.stored().to_f32()
    }
}

/// An image read back as its texture stores it: its rows packed, in its format. As small as
/// the image is on the GPU (a quarter of it as RGBA floats, or a sixteenth for one channel of
/// 8 bits), and converted a band at a time where it is read.
pub struct StoredImage {
    pub width: u32,
    pub height: u32,
    pub format: wgpu::TextureFormat,
    pub bytes: Vec<u8>,
}

impl StoredImage {
    /// RGBA f32 pixels, stored as such.
    pub fn from_f32(width: u32, height: u32, rgba: &[f32]) -> StoredImage {
        let bytes = bytemuck::cast_slice(rgba).to_vec();
        StoredImage { width, height, format: wgpu::TextureFormat::Rgba32Float, bytes }
    }

    /// Pixels as RGBA f32 whatever they are stored as: one channel is gray in red, green and
    /// blue, and opaque; two are red and green, with no blue, opaque (as a shader reads them).
    pub fn to_f32(&self) -> Vec<f32> {
        self.rows_to_f32(0..self.height)
    }

    /// Those of some rows (see [`StoredImage::to_f32`]).
    pub fn rows_to_f32(&self, rows: std::ops::Range<u32>) -> Vec<f32> {
        use wgpu::TextureFormat as F;
        let row = (self.width * bytes_per_pixel(self.format)) as usize;
        let bytes = &self.bytes[rows.start as usize * row..rows.end as usize * row];
        let values: Vec<f32> = match self.format {
            F::R8Unorm | F::Rg8Unorm | F::Rgba8Unorm => bytes.iter().map(|b| f32::from(*b) / 255.0).collect(),
            F::R32Float | F::Rg32Float | F::Rgba32Float => {
                bytes.as_chunks::<4>().0.iter().map(|b| f32::from_ne_bytes(*b)).collect()
            }
            // Half floats (copied first when they are not where half floats may be read).
            _ => match bytemuck::try_cast_slice::<u8, u16>(bytes) {
                Ok(half) => convert_half(half),
                Err(_) => {
                    let half: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|b| u16::from_ne_bytes(*b)).collect();
                    convert_half(&half)
                }
            },
        };
        expand(self.format, values)
    }
}

/// Half floats as floats, with SIMD on all cores.
fn convert_half(half: &[u16]) -> Vec<f32> {
    let mut out = vec![0.0f32; half.len()];
    crate::simd::f16_to_f32(half, &mut out);
    out
}

/// The values of pixels of `format` as RGBA (see [`StoredImage::to_f32`]).
fn expand(format: wgpu::TextureFormat, values: Vec<f32>) -> Vec<f32> {
    match format.components() {
        1 => values.iter().flat_map(|v| [*v, *v, *v, 1.0]).collect(),
        2 => values.as_chunks::<2>().0.iter().flat_map(|[r, g]| [*r, *g, 0.0, 1.0]).collect(),
        _ => values,
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
        for src in KERNELS {
            let name = src.name;
            // Every way the engine builds them: drawing color images or grayscale ones, from
            // color or grayscale inputs, and every kind of image.
            let gray_inputs = ((1u16 << src.inputs) - 1) as u8;
            for output in [COLOR_FORMAT, GRAY_FORMAT] {
                for gray in [0, gray_inputs] {
                    for kind in 0..src.kinds {
                        if let Err(e) = gpu.kernel_variant(name, output, gray, kind) {
                            failures.push(format!("{name} ({output:?}, gray {gray:b}, kind {kind}): {e}"));
                        }
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n\n"));
        // A kind the kernel does not make is an error, not a pipeline.
        assert!(gpu.kernel_variant("noise", GRAY_FORMAT, 0, 1000).is_err());
    }

    #[test]
    fn broken_dynamic_kernel_is_an_error_not_a_panic() {
        let Ok(gpu) = Gpu::headless() else { return };
        assert!(gpu.dynamic_kernel("this is not wgsl", 0, COLOR_FORMAT).is_err());
    }

    /// Grayscale images take a quarter of the memory of color ones, and read back as gray.
    #[test]
    fn grayscale_images_are_one_channel() {
        let Ok(gpu) = Gpu::headless() else { return };
        let gray = gpu.create_image_as(4, 2, GRAY_FORMAT, "gray");
        let color = gpu.create_image(4, 2, "color");
        assert_eq!(gray.bytes() * 4, color.bytes());
        assert_eq!(gray.kind(), ImageKind::Gray);
        gpu.upload_gray_f32(&gray, &[0.0, 0.25, 0.5, 1.0, 2.0, 3.0, 4.0, 5.0]);
        let back = gpu.read_image_blocking(&gray).expect("the image");
        assert_eq!(&back[4..8], &[0.25, 0.25, 0.25, 1.0]);
        assert_eq!(&back[16..20], &[2.0, 2.0, 2.0, 1.0], "values past 1 stay what they are");
    }

    /// A grayscale image is drawn in one channel of 16 bits, with the mips the views sample it
    /// with.
    #[test]
    fn grayscale_images_are_drawn_in_one_channel_with_their_mips() {
        let Ok(gpu) = Gpu::headless() else { return };
        let values = [0.0, 0.25, 0.5, 1.0, 0.125, 0.75, 1.5, 0.3];
        let source = gpu.create_image_as(4, 2, GRAY_FORMAT, "source");
        gpu.upload_gray_f32(&source, &values);
        let drawn = gpu.create_image_with_mips(4, 2, GRAY_FORMAT, "drawn");
        assert_eq!(drawn.bytes(), (4 * 2 + 2 + 1) * 2, "two bytes a pixel, on every level");
        let second = gpu.create_image(2, 1, "the second level");
        let mut encoder = gpu.encoder();
        gpu.encode_copy_with_mips(&mut encoder, &source, &drawn, 0).expect("drawn");
        gpu.encode_show(&mut encoder, &drawn.level(1), &second, 0).expect("read");
        gpu.queue.submit(Some(encoder.finish()));
        let back = gpu.read_image_blocking(&drawn).expect("the image");
        let top: Vec<f32> = back.chunks(4).map(|p| p[0]).collect();
        for (v, e) in top.iter().zip(values) {
            assert!((v - e).abs() < 1e-3, "{top:?}");
        }
        // Each pixel of the next level is the average of the four under it.
        let next = gpu.read_image_blocking(&second).expect("the second level");
        let expected = [(0.0 + 0.25 + 0.125 + 0.75) / 4.0, (0.5 + 1.0 + 1.5 + 0.3) / 4.0];
        for (p, e) in next.chunks(4).zip(expected) {
            assert!((p[0] - e).abs() < 1e-3 && p[3] == 1.0, "{next:?}");
        }
    }
}
