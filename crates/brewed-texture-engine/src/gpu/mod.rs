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

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use brewed_texture_core::Precision;
use brewed_texture_core::nodes::ImageKind;

/// Color images: red, green, blue and alpha as half floats.
pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Grayscale images: one channel as a half float, a quarter of the memory of a color image.
pub const GRAY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;
/// Grayscale images of a graph of full precision (see [`Precision`]): one channel as a float.
pub const GRAY_FULL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
/// The seeds of a distance field: the place of a pixel as an integer (see distance.wgsl); and the
/// labels of a flood fill (flood.wgsl).
pub const SEED_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;
/// How far each pixel is from the outside of its shape along its row and column, in the four
/// directions (see runs.wgsl).
pub const RUN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Uint;
/// Where the size of the image a kernel draws is bound (see common.wgsl).
const TARGET_BINDING: u32 = 15;
/// How many pixels a kernel draws at most in one draw (see [`Gpu::draw`]).
const BAND_PIXELS: u32 = 1 << 20;
/// Bytes of a buffer of the draws' parameters (see [`Uniforms`]): those of a part of the work
/// (see [`PART_DRAWS`]) and more.
const UNIFORM_CHUNK: u64 = 1 << 16;
/// How much work is recorded at most before it is submitted, a part of what is being done
/// (see [`Gpu::draw`]): draws, and pixels drawn.
const PART_DRAWS: u32 = 64;
const PART_PIXELS: u64 = 1 << 24;

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

/// The format images of a kind are kept in at a precision (see [`Precision`]).
pub fn format_of(kind: ImageKind, precision: Precision) -> wgpu::TextureFormat {
    use wgpu::TextureFormat as F;
    match (kind, precision) {
        (ImageKind::Gray, Precision::Bits8) => F::R8Unorm,
        (ImageKind::Gray, Precision::Half) => GRAY_FORMAT,
        (ImageKind::Gray, Precision::Full) => GRAY_FULL_FORMAT,
        (ImageKind::Color, Precision::Bits8) => F::Rgba8Unorm,
        (ImageKind::Color, Precision::Half) => COLOR_FORMAT,
        (ImageKind::Color, Precision::Full) => F::Rgba32Float,
    }
}

/// The precision of the images of a format: a byte, a half float or a float a channel.
pub fn precision_of(format: wgpu::TextureFormat) -> Precision {
    use wgpu::TextureFormat as F;
    match format {
        F::R32Float | F::Rg32Float | F::Rgba32Float => Precision::Full,
        F::R16Float | F::Rg16Float | F::Rgba16Float => Precision::Half,
        _ => Precision::Bits8,
    }
}

/// Whether a format can be sampled through a filter: by the views, and by the kernels that
/// filter their inputs. Floats of 32 bits cannot on every device (it takes a feature WebGPU
/// does not promise), so what is shown of them, and what such a kernel reads, is a copy in half
/// floats.
pub fn filterable(format: wgpu::TextureFormat) -> bool {
    !matches!(format, wgpu::TextureFormat::R32Float | wgpu::TextureFormat::Rg32Float | wgpu::TextureFormat::Rgba32Float)
}

/// The format an output keeps the image it writes in, from its channels (see
/// [`brewed_texture_core::nodes::OUTPUT_FORMATS`]), its format (see
/// [`brewed_texture_core::nodes::OUTPUT_DEPTHS`]) and what arrives at it (its kind and
/// format): the channels it writes, one, two or four (a device has no textures of three: RGB is
/// kept with an opaque alpha), what arrives gray one channel in any format but RG; in bytes
/// for 8 bits and DXT, in half floats and floats for 16 and 32 bits float, and for 16 bits (a
/// PNG of 16 bits) as precisely as it arrives, in half floats at least: a file of 16 bits of an
/// image of floats has all its steps.
pub fn output_format(channels: u32, depth: u32, arriving: ImageKind, arriving_format: Option<wgpu::TextureFormat>) -> wgpu::TextureFormat {
    use brewed_texture_core::nodes::{DEPTH_16, DEPTH_16F, DEPTH_32F, DEPTH_HDR32F, DEPTH_TIFF16, DEPTH_TIFF32F};
    use wgpu::TextureFormat as F;
    let channels = match (channels, arriving) {
        (2, _) => 2,
        (0 | 1, ImageKind::Color) => 4,
        _ => 1,
    };
    let precision = match depth {
        DEPTH_16 | DEPTH_TIFF16 => arriving_format.map_or(Precision::Half, precision_of).max(Precision::Half),
        DEPTH_16F => Precision::Half,
        DEPTH_32F | DEPTH_TIFF32F | DEPTH_HDR32F => Precision::Full,
        _ => Precision::Bits8,
    };
    match (channels, precision) {
        (1, Precision::Bits8) => F::R8Unorm,
        (1, Precision::Half) => F::R16Float,
        (1, Precision::Full) => F::R32Float,
        (2, Precision::Bits8) => F::Rg8Unorm,
        (2, Precision::Half) => F::Rg16Float,
        (2, Precision::Full) => F::Rg32Float,
        (_, Precision::Bits8) => F::Rgba8Unorm,
        (_, Precision::Half) => F::Rgba16Float,
        (_, Precision::Full) => F::Rgba32Float,
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
    /// Reads images of unsigned integers (the seeds of a distance field), not of floats.
    uint: bool,
    /// Writes integers in this format ([`SEED_FORMAT`], [`RUN_FORMAT`]), not an image of floats.
    writes: Option<wgpu::TextureFormat>,
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
            uint: false,
            writes: None,
            kinds: $kinds,
        }
    };
}

macro_rules! uint_kernel {
    ($name:expr, $file:expr, $entry:expr, $inputs:expr) => {
        KernelSource {
            name: $name,
            source: include_str!(concat!("shaders/", $file)),
            entry: $entry,
            inputs: $inputs,
            filtered: false,
            uint: true,
            writes: None,
            kinds: 1,
        }
    };
}

/// A kernel writing integers in `format`, from images of floats or of integers (`uint`).
macro_rules! integer_kernel {
    ($name:expr, $file:expr, $entry:expr, $inputs:expr, $uint:expr, $format:expr) => {
        KernelSource {
            name: $name,
            source: include_str!(concat!("shaders/", $file)),
            entry: $entry,
            inputs: $inputs,
            filtered: false,
            uint: $uint,
            writes: Some($format),
            kinds: 1,
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
            uint: false,
            writes: None,
            kinds: 1,
        }
    };
}

static KERNELS: &[KernelSource] = &[
    kernel!("uniform_color", "uniform_color.wgsl", "main", 0),
    kernel!("shape", "shape.wgsl", "main", 0),
    kernel!("tile_gen", "tile_gen.wgsl", "main", 1),
    kernel!("tile_sampler", "tile_sampler.wgsl", "main", 8),
    kernel!("gradient", "gradient.wgsl", "main", 0),
    kernel!("hdri_gradient", "hdri.wgsl", "gradient_main", 0),
    kernel!("hdri_shape_light", "hdri.wgsl", "shape_main", 1),
    kernel!("hdri_line_light", "hdri.wgsl", "line_main", 1),
    kernel!("hdri_plane_light", "hdri.wgsl", "plane_main", 1),
    // One pipeline per type of noise (and white noise after them).
    kinded_kernel!("noise", "noise.wgsl", "main", 0, brewed_texture_core::nodes::NOISE_TYPES.len() as u32),
    kernel!("transform", "transform.wgsl", "main", 1),
    kernel!("blend", "blend.wgsl", "main", 3),
    kernel!("blur_1d", "blur.wgsl", "blur_1d", 1),
    kernel!("downsample", "blur.wgsl", "downsample", 1),
    kernel!("resample", "blur.wgsl", "resample", 1),
    kernel!("downsample_max", "blur.wgsl", "downsample_max", 1),
    kernel!("sharpen", "sharpen.wgsl", "main", 2),
    kernel!("warp", "warp.wgsl", "main", 2),
    kernel!("mirror", "mirror.wgsl", "main", 1),
    kernel!("normal", "normal.wgsl", "main", 1),
    kernel!("adjust", "adjust.wgsl", "main", 1),
    kernel!("gradient_map", "gradient_map.wgsl", "main", 2),
    kernel!("shuffle", "shuffle.wgsl", "main", 2),
    kernel!("rgba_merge", "rgba_merge.wgsl", "main", 4),
    kernel!("curve", "curve.wgsl", "main", 2),
    kernel!("gradient_dynamic", "gradient_dynamic.wgsl", "main", 3),
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
    kernel!("height_blend", "height_blend.wgsl", "main", 3),
    kernel!("normal_combine", "normal_combine.wgsl", "main", 3),
    kernel!("nonuniform_blur", "nonuniform_blur.wgsl", "main", 2),
    kernel!("make_it_tile", "make_it_tile.wgsl", "main", 1),
    integer_kernel!("distance_seed", "distance_seed.wgsl", "seed", 1, false, SEED_FORMAT),
    integer_kernel!("flood_label", "distance_seed.wgsl", "label", 1, false, SEED_FORMAT),
    integer_kernel!("runs_seed", "distance_seed.wgsl", "runs_seed", 1, false, RUN_FORMAT),
    integer_kernel!("runs_step", "runs.wgsl", "step", 1, true, RUN_FORMAT),
    integer_kernel!("flood_spread", "flood.wgsl", "spread", 3, true, SEED_FORMAT),
    integer_kernel!("flood_jump", "flood.wgsl", "jump", 1, true, SEED_FORMAT),
    uint_kernel!("flood_fill", "flood.wgsl", "fill", 1),
    uint_kernel!("differ", "differ.wgsl", "differ", 2),
    integer_kernel!("distance_jump", "distance.wgsl", "jump", 1, true, SEED_FORMAT),
    uint_kernel!("distance_field", "distance.wgsl", "field", 1),
    uint_kernel!("distance_signed", "distance.wgsl", "signed_field", 2),
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

/// The bindings of a kernel that reads `inputs` images: its parameters, the images (read texel
/// by texel, or through a sampler by the kernels that filter; or integers), the sampler of those
/// that filter, and the size of the image drawn.
fn layout_entries(inputs: usize, filtered: bool, uint: bool) -> Vec<wgpu::BindGroupLayoutEntry> {
    let uniform = |binding: u32| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    };
    let mut entries = vec![uniform(0)];
    for i in 0..inputs {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 1 + i as u32,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: if uint { wgpu::TextureSampleType::Uint } else { wgpu::TextureSampleType::Float { filterable: filtered } },
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
    entries
}

/// How a splat of an FX-Map blends with what is under it, by its Blending (see
/// [`brewed_texture_core::nodes::FX_MAP_BLENDS`]): Max, Add, Stack, Subtract, Min or Multiply.
/// What a splat gives is premultiplied by how much it covers (see fx_map.rs); its alpha lies
/// over what is under it, but for Multiply, which keeps what is there.
fn splat_blend(mode: u32) -> wgpu::BlendState {
    use wgpu::{BlendComponent, BlendFactor as F, BlendOperation as O};
    let with = |src_factor, dst_factor, operation| BlendComponent { src_factor, dst_factor, operation };
    let over = with(F::One, F::OneMinusSrcAlpha, O::Add);
    let (color, alpha) = match mode {
        1 => (with(F::One, F::One, O::Add), over),
        2 => (over, over),
        3 => (with(F::One, F::One, O::ReverseSubtract), over),
        4 => (with(F::One, F::One, O::Min), over),
        5 => (with(F::Zero, F::Src, O::Add), with(F::Zero, F::One, O::Add)),
        _ => (with(F::One, F::One, O::Max), over),
    };
    wgpu::BlendState { color, alpha }
}

/// A pipeline that draws the splats of an FX-Map: quads the vertex entry `splat_vertex` places
/// (running the program), the fragment entry `splat_fragment` fills, blended with what is
/// under them (see fx_map.rs).
pub struct SplatKernel {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    inputs: usize,
}

/// The pipeline of a kernel: WGSL `source`, whose fragment entry `entry` draws `output` from
/// what `layout` binds, making the kind of image `kind`.
fn kernel_pipeline(
    device: &wgpu::Device,
    label: &str,
    source: &str,
    entry: &str,
    layout: &wgpu::BindGroupLayout,
    output: wgpu::TextureFormat,
    kind: u32,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(source.into()) });
    let pipeline_layout =
        device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: &[layout], push_constant_ranges: &[] });
    let constants = [("KIND", f64::from(kind))];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
            targets: &[Some(wgpu::ColorTargetState { format: output, blend: None, write_mask: wgpu::ColorWrites::ALL })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// Build each built-in kernel once (the first kind of those that make several), drawing color
/// images and grayscale ones (in half floats, as a graph makes them by default), the one that
/// takes the longest first, on the thread this is called on and while nothing waits for it:
/// for what the device keeps of what it built. DX12 keeps the shaders it compiled, by their
/// source, and a driver keeps its own: a kernel the engine builds for a node later is there in
/// a moment, where the Tile Sampler's took most of a second on DX12. Nothing is kept here, and
/// nothing asks the device what went wrong: its error scopes are the device's, not the
/// thread's, and the engine's are its own (the kernels build, the tests say so).
pub fn warm_up(device: &wgpu::Device) {
    let mut sources: Vec<&KernelSource> = KERNELS.iter().collect();
    sources.sort_by_key(|src| src.name != "tile_sampler");
    let mut layouts: HashMap<(usize, bool, bool), wgpu::BindGroupLayout> = HashMap::new();
    for src in sources {
        let layout = layouts.entry((src.inputs, src.filtered, src.uint)).or_insert_with(|| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("warm-up"),
                entries: &layout_entries(src.inputs, src.filtered, src.uint),
            })
        });
        let source = kernel_source(src);
        let outputs = match src.writes {
            Some(format) => vec![format],
            None => vec![COLOR_FORMAT, GRAY_FORMAT],
        };
        for output in outputs {
            let _ = kernel_pipeline(device, src.name, &source, src.entry, layout, output, 0);
        }
    }
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
        let view = self.texture.create_view(&wgpu::TextureViewDescriptor { base_mip_level: level, mip_level_count: Some(1), ..Default::default() });
        GpuImage { texture: self.texture.clone(), view, level }
    }
}

pub struct Kernel {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    inputs: usize,
    filtered: bool,
}

/// A kernel as built for the images it runs on: its name (or source), the format it writes, and
/// the kind of image it makes. (Whichever of its inputs are grayscale: see `gray_input` in
/// common.wgsl.)
type KernelKey = (String, wgpu::TextureFormat, u32);

/// Scratch images kept for reuse, each with whether it was used since the last evaluation
/// began (see [`Gpu::start_use`]).
type Pool = Vec<(GpuImage, bool)>;

/// The parameters of the draws recorded since work was last submitted, a slice of a buffer
/// each. The buffers stay from one submission to the next and are written once each when the
/// work is submitted ([`Gpu::submit`]): a buffer of its own for every draw, and the one wgpu
/// uploads it through, were two allocations a draw, of 64 KB each on D3D12 (where a buffer is
/// placed on 64 KB).
#[derive(Default)]
struct Uniforms {
    /// Buffers of [`UNIFORM_CHUNK`] bytes (or of what one draw needs, if more), and what the
    /// draws since the last submission put in them.
    chunks: Vec<(wgpu::Buffer, Vec<u8>)>,
    /// The first buffer that may have room.
    current: usize,
}

pub struct Gpu {
    pub device: wgpu::Device,
    /// Work is submitted with [`Gpu::submit`], which writes the parameters of its draws first.
    pub queue: wgpu::Queue,
    pub adapter_info: String,
    kernels: RefCell<HashMap<KernelKey, Result<Rc<Kernel>, String>>>,
    /// Those of FX-Maps, by their source, the format they draw and how they blend.
    splat_kernels: RefCell<HashMap<KernelKey, Result<Rc<SplatKernel>, String>>>,
    /// By the number of inputs, whether they are filtered, and whether they are integers.
    layouts: RefCell<HashMap<(usize, bool, bool), wgpu::BindGroupLayout>>,
    /// Those of the splat kernels, by the number of inputs.
    splat_layouts: RefCell<HashMap<usize, wgpu::BindGroupLayout>>,
    /// Trilinear samplers for the kernels that filter their inputs, by tiling: bit 0
    /// repeats horizontally, bit 1 vertically; the other axes clamp.
    samplers: [wgpu::Sampler; 4],
    pool: RefCell<Pool>,
    uniforms: RefCell<Uniforms>,
    /// What was drawn since work was last submitted: draws, and pixels.
    recorded: Cell<(u32, u64)>,
    /// The part of the work [`Gpu::draw`] submitted last, which the next part waits for.
    part: RefCell<Option<wgpu::SubmissionIndex>>,
    /// 1x1 transparent black, bound to unconnected inputs.
    pub black: GpuImage,
    /// 1x1 opaque white (e.g. an unconnected blend mask).
    pub white: GpuImage,
    /// Errors reported by wgpu outside of error scopes; shown in the status bar.
    pub errors: Arc<Mutex<Vec<String>>>,
}

/// Write down an error of the device, for the log and the status bar (the last 16).
fn record(errors: &Mutex<Vec<String>>, e: &wgpu::Error) {
    log::error!("wgpu error: {e}");
    if let Ok(mut v) = errors.lock() {
        v.push(e.to_string());
        if v.len() > 16 {
            v.remove(0);
        }
    }
}

/// Is `err` about an object labelled `label` (wgpu says so: "In Device::create_render_pipeline,
/// label = 'noise'")?
fn is_about(err: &wgpu::Error, label: &str) -> bool {
    err.to_string().contains(&format!("label = '{label}'"))
}

impl Gpu {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, adapter_info: String) -> Gpu {
        let errors: Arc<Mutex<Vec<String>>> = Arc::default();
        let sink = errors.clone();
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| record(&sink, &e)));
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
            splat_kernels: RefCell::default(),
            layouts: RefCell::default(),
            splat_layouts: RefCell::default(),
            samplers,
            pool: RefCell::default(),
            uniforms: RefCell::default(),
            recorded: Cell::default(),
            part: RefCell::default(),
            black,
            white,
            errors,
        };
        gpu.upload_rgba_f32(&gpu.black, &[0.0, 0.0, 0.0, 0.0]);
        gpu.upload_rgba_f32(&gpu.white, &[1.0, 1.0, 1.0, 1.0]);
        gpu
    }

    /// Create a device without a window (tests, command line tools), on the backends
    /// `WGPU_BACKEND` names (as iced does for the window), or the primary ones.
    pub fn headless() -> Result<Gpu, String> {
        Self::headless_on(wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY))
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

    fn create_image_raw(device: &wgpu::Device, w: u32, h: u32, mips: u32, format: wgpu::TextureFormat, label: &str) -> GpuImage {
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
    pub(crate) fn create_image_raw_for_tests(&self, w: u32, h: u32, mips: u32, format: wgpu::TextureFormat) -> GpuImage {
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
    pub fn encode_copy_with_mips(&self, encoder: &mut wgpu::CommandEncoder, src: &GpuImage, dst: &GpuImage, format: u32) -> Result<(), String> {
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
        self.encode_fill(encoder, image, wgpu::Color::TRANSPARENT);
    }

    /// Record filling `image` (its top level) with `color` (its channels, in its format).
    pub fn encode_fill(&self, encoder: &mut wgpu::CommandEncoder, image: &GpuImage, color: wgpu::Color) {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &image.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(color), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }

    /// Whether what is drawn into images of `format` can be blended with what they hold:
    /// floats of 32 bits cannot on every device (it takes a feature WebGPU does not promise).
    pub fn blendable(&self, format: wgpu::TextureFormat) -> bool {
        format.guaranteed_format_features(self.device.features()).flags.contains(wgpu::TextureFormatFeatureFlags::BLENDABLE)
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
        let i = pool.iter().position(|(img, _)| img.size() == [w, h] && img.format() == format && img.texture.mip_level_count() == mips)?;
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

    /// Floats as they are, one per channel, into an image of floats.
    pub fn upload_floats(&self, img: &GpuImage, values: &[f32]) {
        self.upload(img, bytemuck::cast_slice(values));
    }

    /// One half float per pixel, into a grayscale image.
    pub fn upload_gray_f16(&self, img: &GpuImage, half: &[u16]) {
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
            wgpu::TexelCopyTextureInfo { texture: &img.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * bytes_per_pixel(img.format())), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    fn layout(&self, inputs: usize, filtered: bool, uint: bool) -> wgpu::BindGroupLayout {
        if let Some(l) = self.layouts.borrow().get(&(inputs, filtered, uint)) {
            return l.clone();
        }
        let kind = match (filtered, uint) {
            (true, _) => "_filtered",
            (false, true) => "_uint",
            (false, false) => "",
        };
        let layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&format!("layout_{inputs}_inputs{kind}")),
            entries: &layout_entries(inputs, filtered, uint),
        });
        self.layouts.borrow_mut().insert((inputs, filtered, uint), layout.clone());
        layout
    }

    /// Build WGSL into a kernel drawing `output`, capturing validation errors instead of
    /// panicking. `kind` is the kind of image it makes.
    #[allow(clippy::too_many_arguments)]
    fn build_kernel(
        &self,
        label: &str,
        source: &str,
        entry: &str,
        inputs: usize,
        (filtered, uint): (bool, bool),
        output: wgpu::TextureFormat,
        kind: u32,
    ) -> Result<Rc<Kernel>, String> {
        let layout = self.layout(inputs, filtered, uint);
        self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let pipeline = kernel_pipeline(&self.device, label, source, entry, &layout, output, kind);
        if let Some(err) = pollster::block_on(self.device.pop_error_scope()) {
            // The device is the window's too, and wgpu's error scopes are the device's, not the
            // thread's: what went wrong meanwhile in the window lands here. Only what the
            // kernel's own objects say is the kernel's.
            if !is_about(&err, label) {
                record(&self.errors, &err);
            } else {
                log::error!("kernel '{label}' failed to compile: {err}");
                return Err(format!("Shader error in '{label}': {err}"));
            }
        }
        Ok(Rc::new(Kernel { pipeline, layout, inputs, filtered }))
    }

    /// A built-in kernel writing color images from color images, compiled on first use.
    pub fn kernel(&self, name: &str) -> Result<Rc<Kernel>, String> {
        self.kernel_variant(name, COLOR_FORMAT, 0)
    }

    /// A built-in kernel as built for writing `output`, and making the kind of image `kind`.
    pub fn kernel_variant(&self, name: &str, output: wgpu::TextureFormat, kind: u32) -> Result<Rc<Kernel>, String> {
        let key = (name.to_string(), output, kind);
        if let Some(k) = self.kernels.borrow().get(&key) {
            return k.clone();
        }
        let src = KERNELS.iter().find(|k| k.name == name).ok_or_else(|| format!("unknown kernel '{name}'"))?;
        let integers = [SEED_FORMAT, RUN_FORMAT];
        let result = if src.writes.map_or(integers.contains(&output), |f| f != output) {
            Err(format!("'{name}' writes {}, not {output:?}", src.writes.map_or_else(|| "images of floats".to_string(), |f| format!("{f:?}"))))
        } else if kind < src.kinds {
            let label = if src.kinds > 1 { format!("{name} {kind}") } else { name.to_string() };
            let reads = (src.filtered, src.uint);
            self.build_kernel(&label, &kernel_source(src), src.entry, src.inputs, reads, output, kind)
        } else {
            Err(format!("'{name}' makes {} kinds of image, not a kind {kind}", src.kinds))
        };
        self.kernels.borrow_mut().insert(key, result.clone());
        result
    }

    /// A kernel generated at runtime (Pixel Math) writing `output`, cached by its source text.
    /// Its source says itself which inputs are grayscale.
    pub fn dynamic_kernel(&self, source: &str, inputs: usize, output: wgpu::TextureFormat) -> Result<Rc<Kernel>, String> {
        let key = (format!("dyn:{inputs}:{source}"), output, 0);
        if let Some(k) = self.kernels.borrow().get(&key) {
            return k.clone();
        }
        let result = self.build_kernel("pixel_math", source, "main", inputs, (false, false), output, 0);
        let mut cache = self.kernels.borrow_mut();
        if cache.len() > 256 {
            cache.retain(|(k, ..), _| !k.starts_with("dyn:"));
        }
        cache.insert(key, result.clone());
        result
    }

    /// Which of `inputs` are grayscale, as the bits a kernel is built with.
    fn gray_inputs(inputs: &[&GpuImage]) -> u32 {
        inputs.iter().enumerate().filter(|(_, img)| img.is_gray()).fold(0, |bits, (i, _)| bits | (1 << i))
    }

    /// The bindings of a draw into `output`: `params`, `inputs`, the sampler for `tiling` of a
    /// kernel that filters, and the size of the image drawn with which inputs are grayscale.
    fn bind_group(&self, layout: &wgpu::BindGroupLayout, params: &[u8], inputs: &[&GpuImage], output: &GpuImage, sampler: Option<u32>) -> wgpu::BindGroup {
        // The parameters, then the size of the image drawn, where a binding may start.
        let target = u64::from(self.device.limits().min_uniform_buffer_offset_alignment);
        let params_size = (params.len().div_ceil(16).max(1) * 16) as u64;
        let target_at = params_size.div_ceil(target) * target;
        let mut contents = params.to_vec();
        contents.resize(target_at as usize, 0);
        let [w, h] = output.level_size();
        contents.extend_from_slice(bytemuck::bytes_of(&[w, h, Self::gray_inputs(inputs), 0]));
        let (buffer, at) = self.uniform_slot(&contents);
        let range = |offset: u64, size: u64| {
            let size = wgpu::BufferSize::new(size);
            wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &buffer, offset: at + offset, size })
        };
        let mut entries = vec![wgpu::BindGroupEntry { binding: 0, resource: range(0, params_size) }];
        for (i, img) in inputs.iter().enumerate() {
            let resource = wgpu::BindingResource::TextureView(&img.view);
            entries.push(wgpu::BindGroupEntry { binding: 1 + i as u32, resource });
        }
        if let Some(tiling) = sampler {
            entries.push(wgpu::BindGroupEntry {
                binding: 1 + inputs.len() as u32,
                resource: wgpu::BindingResource::Sampler(&self.samplers[(tiling & 3) as usize]),
            });
        }
        entries.push(wgpu::BindGroupEntry { binding: TARGET_BINDING, resource: range(target_at, 16) });
        self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout, entries: &entries })
    }

    /// Record drawing all of `output` (one level of it) with `kernel`; a kernel that filters
    /// its inputs gets the sampler for `tiling`.
    fn draw(&self, encoder: &mut wgpu::CommandEncoder, kernel: &Kernel, params: &[u8], inputs: &[&GpuImage], output: &GpuImage, tiling: u32) {
        debug_assert_eq!(inputs.len(), kernel.inputs, "kernel input count mismatch");
        let bind_group = self.bind_group(&kernel.layout, params, inputs, output, kernel.filtered.then_some(tiling));
        let [w, h] = output.level_size();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &output.view,
                depth_slice: None,
                resolve_target: None,
                // Every pixel is drawn: what was there is not read.
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
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
        drop(pass);
        self.drew(encoder, u64::from(w) * u64::from(h));
    }

    /// A draw of about `pixels` pixels was recorded into `encoder`.
    fn drew(&self, encoder: &mut wgpu::CommandEncoder, pixels: u64) {
        // Much work is submitted in parts, each once the part before is done. Other work (the
        // window's frames) waits for two parts at most, not for all of it; and a list of
        // commands stays short: a D3D12 command allocator keeps the memory of the longest list
        // it ever held, and wgpu keeps its allocators to use them again (all of a large graph
        // at once kept about a megabyte of system memory a node).
        let (draws, before) = self.recorded.get();
        let recorded = (draws + 1, before + pixels);
        if recorded.0 < PART_DRAWS && recorded.1 < PART_PIXELS {
            self.recorded.set(recorded);
            return;
        }
        let part = std::mem::replace(encoder, self.encoder());
        let submitted = self.submit(part);
        if let Some(before) = self.part.replace(Some(submitted)) {
            let _ = self.device.poll(wgpu::PollType::Wait { submission_index: Some(before), timeout: None });
        }
    }

    fn splat_layout(&self, inputs: usize) -> wgpu::BindGroupLayout {
        if let Some(l) = self.splat_layouts.borrow().get(&inputs) {
            return l.clone();
        }
        // A kernel's bindings, which the vertex entry reads too: it runs the program.
        let mut entries = layout_entries(inputs, false, false);
        for entry in &mut entries {
            entry.visibility = wgpu::ShaderStages::VERTEX_FRAGMENT;
        }
        let layout =
            self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(&format!("layout_{inputs}_splats")), entries: &entries });
        self.splat_layouts.borrow_mut().insert(inputs, layout.clone());
        layout
    }

    /// A kernel drawing the splats of an FX-Map with the WGSL `source` into images of `output`,
    /// blending them as its Blending `blend` says; built once for each, and kept.
    pub fn splat_kernel(&self, source: &str, inputs: usize, output: wgpu::TextureFormat, blend: u32) -> Result<Rc<SplatKernel>, String> {
        let key = (format!("splats:{inputs}:{source}"), output, blend);
        if let Some(k) = self.splat_kernels.borrow().get(&key) {
            return k.clone();
        }
        const LABEL: &str = "fx_map";
        let layout = self.splat_layout(inputs);
        self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(LABEL), source: wgpu::ShaderSource::Wgsl(source.into()) });
        let pipeline_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(LABEL),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(LABEL),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("splat_vertex"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("splat_fragment"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState { format: output, blend: Some(splat_blend(blend)), write_mask: wgpu::ColorWrites::ALL })],
            }),
            // A quad a splat: its four corners.
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let mut result = Ok(Rc::new(SplatKernel { pipeline, layout, inputs }));
        if let Some(err) = pollster::block_on(self.device.pop_error_scope()) {
            // (What went wrong meanwhile in the window is the window's: see `build_kernel`.)
            if is_about(&err, LABEL) {
                log::error!("an FX-Map failed to compile: {err}");
                result = Err(format!("Shader error in the FX-Map: {err}"));
            } else {
                record(&self.errors, &err);
            }
        }
        let mut cache = self.splat_kernels.borrow_mut();
        if cache.len() > 64 {
            cache.clear();
        }
        cache.insert(key, result.clone());
        result
    }

    /// Record drawing splats with `kernel` over what `output` holds (one level of it): the
    /// instances of each of `batches` in a draw of their own, each with about how many pixels
    /// it covers, for how much is recorded before it is submitted (see [`Gpu::draw`]).
    pub fn draw_splats(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        kernel: &SplatKernel,
        params: &[u8],
        inputs: &[&GpuImage],
        output: &GpuImage,
        batches: &[(std::ops::Range<u32>, u64)],
    ) {
        debug_assert_eq!(inputs.len(), kernel.inputs, "kernel input count mismatch");
        for (instances, pixels) in batches {
            // The parameters for each draw: once a part of the work is submitted, the next uses
            // the buffers of its parameters again.
            let bind_group = self.bind_group(&kernel.layout, params, inputs, output, None);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fx_map"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output.view,
                    depth_slice: None,
                    resolve_target: None,
                    // Over what is there.
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&kernel.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..4, instances.clone());
            drop(pass);
            self.drew(encoder, *pixels);
        }
    }

    /// Run a built-in kernel over `output`, built for the images it reads and writes.
    pub fn run(&self, encoder: &mut wgpu::CommandEncoder, kernel: &str, params: &[u8], inputs: &[&GpuImage], output: &GpuImage) -> Result<(), String> {
        let k = self.kernel_variant(kernel, output.format(), 0)?;
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
        let k = self.kernel_variant(kernel, output.format(), kind)?;
        self.draw(encoder, &k, params, inputs, output, 0);
        Ok(())
    }

    /// Run a kernel that filters its inputs, sampling them with the node's tiling. An input
    /// that cannot be filtered (32-bit floats: not every device can) is read from a copy in
    /// half floats.
    pub fn run_filtered(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        kernel: &str,
        params: &[u8],
        inputs: &[&GpuImage],
        output: &GpuImage,
        tiling: u32,
    ) -> Result<(), String> {
        let copies: Vec<Option<GpuImage>> = inputs
            .iter()
            .map(|img| {
                (!filterable(img.format())).then(|| {
                    let [w, h] = img.level_size();
                    self.take_scratch(w, h, format_of(img.kind(), Precision::Half))
                })
            })
            .collect();
        let mut result = Ok(());
        for (img, copy) in inputs.iter().zip(&copies) {
            if let Some(copy) = copy {
                result = result.and_then(|()| self.encode_show(encoder, img, copy, 0));
            }
        }
        let read: Vec<&GpuImage> = inputs.iter().zip(&copies).map(|(img, copy)| copy.as_ref().unwrap_or(img)).collect();
        let result = result.and_then(|()| {
            let k = self.kernel_variant(kernel, output.format(), 0)?;
            self.draw(encoder, &k, params, &read, output, tiling);
            Ok(())
        });
        for copy in copies.into_iter().flatten() {
            self.recycle(copy);
        }
        result
    }

    /// Run a kernel generated at runtime (see [`Gpu::dynamic_kernel`]) over `output`.
    pub fn run_dynamic(&self, encoder: &mut wgpu::CommandEncoder, kernel: &Kernel, params: &[u8], inputs: &[&GpuImage], output: &GpuImage) {
        self.draw(encoder, kernel, params, inputs, output, 0);
    }

    pub fn encoder(&self) -> wgpu::CommandEncoder {
        self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("brewed") })
    }

    /// Submit what `encoder` recorded (a new one in its place) and wait until the GPU is done:
    /// for work that decides what it does next from what the GPU found.
    pub fn finish(&self, encoder: &mut wgpu::CommandEncoder) {
        let done = std::mem::replace(encoder, self.encoder());
        self.submit(done);
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }

    /// Whether any pixel of `image` is above 0 (its red, or its one channel): the image halved
    /// by its maximum down to one pixel, read back. Waits for the GPU (see [`Gpu::finish`]).
    pub fn any(&self, encoder: &mut wgpu::CommandEncoder, image: &GpuImage) -> Result<bool, String> {
        let mut levels: Vec<GpuImage> = Vec::new();
        let mut current = image.clone();
        let mut result = Ok(());
        while current.width() > 1 || current.height() > 1 {
            let (w, h) = (current.width().div_ceil(2), current.height().div_ceil(2));
            let next = self.take_scratch(w, h, image.format());
            result = self.run(encoder, "downsample_max", &[0u8; 32], &[&current], &next);
            current = next.clone();
            levels.push(next);
            if result.is_err() {
                break;
            }
        }
        let found = result.map(|()| {
            let rb = self.encode_readback_region(encoder, &current, [0, 0]);
            self.finish(encoder);
            rb.wait(&self.device).is_ok() && rb.to_f32().first().is_some_and(|v| *v > 0.0)
        });
        for level in levels {
            self.recycle(level);
        }
        found
    }

    /// Where the parameters of a draw go: a buffer, and where in it. They are written when
    /// the work is submitted ([`Gpu::submit`]).
    fn uniform_slot(&self, contents: &[u8]) -> (wgpu::Buffer, u64) {
        let align = self.device.limits().min_uniform_buffer_offset_alignment as usize;
        let size = contents.len().div_ceil(align) * align;
        let mut uniforms = self.uniforms.borrow_mut();
        let Uniforms { chunks, current } = &mut *uniforms;
        loop {
            if *current == chunks.len() {
                let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("parameters"),
                    size: UNIFORM_CHUNK.max(size as u64),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                chunks.push((buffer, Vec::new()));
            }
            let (buffer, data) = &mut chunks[*current];
            let at = data.len();
            if (at + size) as u64 <= buffer.size() {
                data.extend_from_slice(contents);
                data.resize(at + size, 0);
                return (buffer.clone(), at as u64);
            }
            *current += 1;
        }
    }

    /// Submit recorded work, the parameters of its draws written first (a write for each
    /// buffer of them). The queue runs submissions in order, so the next writes come after
    /// this work read them.
    pub fn submit(&self, encoder: wgpu::CommandEncoder) -> wgpu::SubmissionIndex {
        self.recorded.set((0, 0));
        let mut uniforms = self.uniforms.borrow_mut();
        for (buffer, data) in &mut uniforms.chunks {
            if !data.is_empty() {
                self.queue.write_buffer(buffer, 0, data);
                data.clear();
            }
        }
        uniforms.current = 0;
        drop(uniforms);
        self.queue.submit(Some(encoder.finish()))
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

    fn encode_readback_rect(&self, encoder: &mut wgpu::CommandEncoder, img: &GpuImage, origin: [u32; 2], [w, h]: [u32; 2]) -> Readback {
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
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded_row), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        Readback { buffer, width: w, height: h, padded_row, format, state: Arc::new(AtomicU8::new(STATE_IDLE)) }
    }

    /// Read an image back to the CPU as RGBA f32, blocking until the GPU is done.
    pub fn read_image_blocking(&self, img: &GpuImage) -> Result<Vec<f32>, String> {
        self.read_stored_blocking(img).map(|stored| stored.to_f32())
    }

    /// Read an image back to the CPU as its texture stores it, blocking until the GPU is done:
    /// in bands of rows, each no larger than a buffer may be (256 MB on a device that keeps to
    /// the limits every GPU has, as the window's does: half of what 8192 × 8192 colors of half
    /// floats take).
    pub fn read_stored_blocking(&self, img: &GpuImage) -> Result<StoredImage, String> {
        let ([w, h], format) = (img.size(), img.format());
        let row = w * bytes_per_pixel(format);
        let padded = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let rows = (self.device.limits().max_buffer_size / u64::from(padded)).clamp(1, u64::from(h.max(1))) as u32;
        let mut bytes = Vec::with_capacity(row as usize * h as usize);
        for y in (0..h).step_by(rows as usize) {
            let mut encoder = self.encoder();
            let rb = self.encode_readback_rect(&mut encoder, img, [0, y], [w, rows.min(h - y)]);
            self.submit(encoder);
            rb.wait(&self.device)?;
            rb.rows_into(&mut bytes);
        }
        Ok(StoredImage { width: w, height: h, format, bytes })
    }

    /// Non-blocking device poll so pending readbacks make progress.
    pub fn poll(&self) {
        let _ = self.device.poll(wgpu::PollType::Poll);
    }

    pub fn take_errors(&self) -> Vec<String> {
        self.errors.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default()
    }
}

/// How long a copy the device is done with is waited for to be said mapped (see
/// [`Readback::wait`]): far longer than another thread takes to say so.
const READBACK_PATIENCE: std::time::Duration = std::time::Duration::from_secs(30);

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

    /// Block until the copy is on the CPU, or mapping it failed (mapping it first, if it was
    /// not: see [`Readback::map`]). The device being done with all it was given is not that
    /// yet: a mapped buffer is said to be so by whichever thread polls the device first — the
    /// window's drawing polls the same device — a moment after the device let this thread go.
    /// Looked at right then, the copy was "not there": the pictures of all the nodes an
    /// evaluation changed were dropped at once (the nodes stayed black, or as they were
    /// before), a menu's pictures were black for good, an export failed. So it is waited for.
    pub fn wait(&self, device: &wgpu::Device) -> Result<(), String> {
        if self.state.load(Ordering::SeqCst) == STATE_IDLE {
            self.map();
        }
        // (Said mapped already — one of several mapped together and waited for one by one:
        // nothing to wait for. The device is waited for once: each wait is for all that was
        // given to it by then, the window's frames too, and a frame can take its time.)
        if let Some(done) = self.poll() {
            return done;
        }
        let polled = device.poll(wgpu::PollType::wait_indefinitely());
        let started = std::time::Instant::now();
        loop {
            if let Some(done) = self.poll() {
                return done;
            }
            // (A device that cannot be polled says no more.)
            if let Err(e) = &polled {
                return Err(format!("GPU poll failed: {e}"));
            }
            if started.elapsed() > READBACK_PATIENCE {
                return Err("GPU readback did not finish".into());
            }
            std::thread::sleep(std::time::Duration::from_micros(50));
            // (Whoever polls is told: this thread too.)
            let _ = device.poll(wgpu::PollType::Poll);
        }
    }

    /// The pixels as they are stored, without the rows' padding. Requires a successful map.
    pub fn stored(&self) -> StoredImage {
        let mut bytes = Vec::with_capacity((self.width * bytes_per_pixel(self.format)) as usize * self.height as usize);
        self.rows_into(&mut bytes);
        StoredImage { width: self.width, height: self.height, format: self.format, bytes }
    }

    /// Its rows, without their padding, after `bytes`. Requires a successful map.
    fn rows_into(&self, bytes: &mut Vec<u8>) {
        let row = (self.width * bytes_per_pixel(self.format)) as usize;
        {
            let view = self.buffer.get_mapped_range(..);
            for y in 0..self.height as usize {
                let start = y * self.padded_row as usize;
                bytes.extend_from_slice(&view[start..start + row]);
            }
        }
        self.buffer.unmap();
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
            F::R32Float | F::Rg32Float | F::Rgba32Float => bytes.as_chunks::<4>().0.iter().map(|b| f32::from_ne_bytes(*b)).collect(),
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

    /// A kernel is failed only by what its own objects say: an error of the window (the device
    /// is the window's too) that lands in its error scope is only written down, and the kernel
    /// is made all the same.
    #[test]
    fn errors_of_others_do_not_fail_a_kernel() {
        let gpu = match Gpu::headless() {
            Ok(g) => g,
            Err(e) => {
                eprintln!("skipping GPU test: {e}");
                return;
            }
        };
        // What goes wrong elsewhere, as the window's texture of a size no texture may have.
        let other = |label: &str| {
            gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let _ = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: 1 << 20, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            pollster::block_on(gpu.device.pop_error_scope()).expect("a texture too large is an error")
        };
        let err = other("window frame");
        assert!(is_about(&err, "window frame") && !is_about(&err, "blur") && !is_about(&err, "window"));
        // Another thread's errors, one after the other, while kernels are made: they land in
        // the kernels' scopes, and fail none.
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let window = {
            let (device, stop) = (gpu.device.clone(), stop.clone());
            std::thread::spawn(move || {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("window frame"),
                        size: wgpu::Extent3d { width: 1 << 20, height: 1, depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    });
                }
            })
        };
        let failed: Vec<String> = KERNELS
            .iter()
            .flat_map(|src| (0..src.kinds).map(move |kind| (src.name, src.writes.unwrap_or(COLOR_FORMAT), kind)))
            .filter_map(|(name, output, kind)| gpu.kernel_variant(name, output, kind).err())
            .collect();
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        window.join().unwrap();
        assert!(failed.is_empty(), "{failed:#?}");
        assert!(!gpu.errors.lock().unwrap().is_empty(), "the window's errors are written down");
    }

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
            // Every way the engine builds them: drawing color images or grayscale ones (from
            // color or grayscale inputs alike), and every kind of image.
            let outputs = match src.writes {
                Some(format) => vec![format],
                None => vec![COLOR_FORMAT, GRAY_FORMAT],
            };
            for output in outputs {
                for kind in 0..src.kinds {
                    if let Err(e) = gpu.kernel_variant(name, output, kind) {
                        failures.push(format!("{name} ({output:?}, kind {kind}): {e}"));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n\n"));
        // A kind the kernel does not make is an error, not a pipeline.
        assert!(gpu.kernel_variant("noise", GRAY_FORMAT, 1000).is_err());
    }

    /// Building every kernel ahead (see `warm_up`) goes without a word from the device.
    #[test]
    fn every_kernel_is_built_ahead() {
        let Ok(gpu) = Gpu::headless() else { return };
        gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        warm_up(&gpu.device);
        assert!(pollster::block_on(gpu.device.pop_error_scope()).is_none());
    }

    #[test]
    fn broken_dynamic_kernel_is_an_error_not_a_panic() {
        let Ok(gpu) = Gpu::headless() else { return };
        assert!(gpu.dynamic_kernel("this is not wgsl", 0, COLOR_FORMAT).is_err());
    }

    /// Images larger than a buffer may be are read back whole, in bands of rows: an export of
    /// 8192 × 8192 colors of half floats (512 MB) or floats (1 GB) is (here one of 8192 × 2100
    /// floats, more than the 256 MB of a buffer).
    #[test]
    fn images_larger_than_a_buffer_are_read_back() {
        let Ok(gpu) = Gpu::headless() else { return };
        let (w, h) = (8192u32, 2100u32);
        let big = gpu.create_image_as(w, h, wgpu::TextureFormat::Rgba32Float, "big");
        assert!(big.bytes() > gpu.device.limits().max_buffer_size);
        // Each pixel says where it is.
        let values: Vec<f32> = (0..h).flat_map(|y| (0..w).flat_map(move |x| [x as f32, y as f32, 0.5, 1.0])).collect();
        gpu.upload_floats(&big, &values);
        let back = gpu.read_stored_blocking(&big).expect("read back");
        assert_eq!((back.width, back.height, back.bytes.len()), (w, h, values.len() * 4));
        assert!(back.bytes.as_chunks::<4>().0.iter().map(|b| f32::from_ne_bytes(*b)).eq(values.iter().copied()));
        assert!(gpu.take_errors().is_empty());
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

    /// Draws keep their own parameters however much is drawn before the work is submitted: it
    /// is submitted in parts, with the parameters of each in a buffer used again.
    #[test]
    fn draws_keep_their_own_parameters() {
        let Ok(gpu) = Gpu::headless() else { return };
        // Twenty parts of draws, each of a color of its own.
        let draws = PART_DRAWS as usize * 20;
        let images: Vec<GpuImage> = (0..draws).map(|_| gpu.create_image(1, 1, "a color")).collect();
        let color = |i: usize, round: usize| {
            let v = (i + round) as f32 / draws as f32;
            [v, 1.0 - v, 0.5, 1.0]
        };
        for round in 0..2 {
            let mut encoder = gpu.encoder();
            for (i, image) in images.iter().enumerate() {
                let params = color(i, round);
                gpu.run(&mut encoder, "uniform_color", bytemuck::bytes_of(&params), &[], image).expect("drawn");
                // What is not submitted yet is a part at most.
                assert!(gpu.recorded.get().0 < PART_DRAWS);
            }
            gpu.submit(encoder);
            let mut encoder = gpu.encoder();
            let back: Vec<Readback> = images.iter().map(|image| gpu.encode_readback(&mut encoder, image)).collect();
            gpu.submit(encoder);
            back.iter().for_each(Readback::map);
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            for (i, rb) in back.iter().enumerate() {
                let (px, want) = (rb.to_f32(), color(i, round));
                assert!(px.iter().zip(want).all(|(p, w)| (p - w).abs() < 1e-3), "draw {i} of round {round}: {px:?}");
            }
            assert_eq!(gpu.uniforms.borrow().chunks.len(), 1, "round {round}");
        }
        assert!(gpu.take_errors().is_empty());
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
        gpu.submit(encoder);
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

    /// A copy is read back whole while other threads poll the device and submit to it, as the
    /// window's drawing does while the engine works: the device done with all it was given, a
    /// copy can still be a moment from being said mapped (by the other thread, which the device
    /// told first), and what looked at it right then took it for not there — a batch of nodes'
    /// pictures dropped, an export failed. Every one arrives, with what was put in it.
    #[test]
    fn a_copy_is_waited_for_while_another_thread_polls_the_device() {
        let Ok(gpu) = Gpu::headless() else { return };
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let others: Vec<_> = (0..3)
            .map(|i| {
                let (device, queue, stop) = (gpu.device.clone(), gpu.queue.clone(), stop.clone());
                std::thread::spawn(move || {
                    while !stop.load(Ordering::SeqCst) {
                        let _ = device.poll(wgpu::PollType::Poll);
                        // (One of them submits too, as a frame does.)
                        if i == 0 {
                            let encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
                            queue.submit([encoder.finish()]);
                        }
                    }
                })
            })
            .collect();
        let image = gpu.take_scratch(4, 4, COLOR_FORMAT);
        let (rounds, mut early, mut wrong) = (3000u32, 0, Vec::new());
        for round in 0..rounds {
            let level = (round % 251) as f32 / 250.0;
            gpu.upload_rgba_f32(&image, &[level, 0.5, 1.0 - level, 1.0].repeat(16));
            let mut encoder = gpu.encoder();
            let rb = gpu.encode_readback_region(&mut encoder, &image, [1, 2]);
            gpu.submit(encoder);
            rb.map();
            // (How it was looked at before: after the device's wait alone.)
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            if rb.poll().is_none() {
                early += 1;
            }
            match rb.wait(&gpu.device) {
                Ok(()) => {
                    let px = rb.to_f32();
                    if (px[0] - level).abs() > 2e-3 || (px[2] - (1.0 - level)).abs() > 2e-3 {
                        wrong.push(format!("round {round}: {px:?} for {level}"));
                    }
                }
                Err(e) => wrong.push(format!("round {round}: {e}")),
            }
        }
        stop.store(true, Ordering::SeqCst);
        for other in others {
            other.join().expect("the other thread");
        }
        assert!(wrong.is_empty(), "{} of {rounds} ({early} were not there yet after the device's wait): {:?}", wrong.len(), &wrong[..wrong.len().min(5)]);
        if std::env::var("BREWED_READBACK_RACE").is_ok() {
            eprintln!("not there yet after the device's wait: {early} of {rounds}");
        }
    }
}
