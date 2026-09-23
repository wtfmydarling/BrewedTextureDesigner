//! GPU implementations of the image node types.
//!
//! [`kernel_for`] maps a node type to the function that records its GPU work. Every
//! function reads the node's effective parameters from the [`ExecCtx`], binds its inputs
//! and dispatches one or more compute kernels (see `gpu/shaders`). Adding an image node
//! type means adding its definition in `brewed-texture-core` and one function here.

use bytemuck::{Pod, Zeroable};

use brewed_texture_core::NodeKind;
use brewed_texture_core::eval::Params;
use brewed_texture_core::expr;
use brewed_texture_core::nodes::{NOISE_CRACKS, NOISE_FIBERS, NOISE_SCRATCHES, NOISE_VEINS, NOISE_WOOD, TILE_PATTERN_IMAGE};

use crate::gpu::{Gpu, GpuImage};

/// How a Pixel Math variable gets its value.
#[derive(Clone, Debug)]
pub enum PixelVar {
    Builtin(&'static str),
    Image(GpuImage),
    Constant(f32),
}

/// Everything a node needs to record its GPU work.
pub struct ExecCtx<'a> {
    pub kind: NodeKind,
    pub gpu: &'a Gpu,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub params: &'a Params,
    /// Fixed inputs in definition order (`None` = unconnected).
    pub inputs: &'a [Option<GpuImage>],
    pub output: &'a GpuImage,
    /// Decoded file of a Bitmap node.
    pub bitmap: Option<&'a GpuImage>,
    /// Baked gradient of a Gradient Map node.
    pub lut: Option<&'a GpuImage>,
    /// Pixel Math: resolved variables in the order of the expression's variables.
    pub pixel_vars: &'a [PixelVar],
}

/// Records the work of one node; returns a warning for the UI, if any.
pub type KernelFn = fn(&mut ExecCtx) -> Result<Option<String>, String>;

/// The GPU implementation of an image node type.
pub fn kernel_for(kind: NodeKind) -> Option<KernelFn> {
    let f: KernelFn = match kind.id() {
        "uniform_color" => uniform_color,
        "shape" => shape,
        "tile_generator" => tile_generator,
        "gradient" => gradient,
        "noise" => noise,
        "bitmap" => bitmap,
        "transform_2d" => transform_2d,
        "blend" => blend,
        "blur" => blur,
        "directional_blur" => directional_blur,
        "slope_blur" => slope_blur,
        "sharpen" => sharpen,
        "warp" => warp,
        "directional_warp" => directional_warp,
        "mirror" => mirror,
        "pixel_math" => pixel_math,
        "normal" => normal,
        "ambient_occlusion" => ambient_occlusion,
        "curvature" => curvature,
        "edge_detect" => edge_detect,
        "emboss" => emboss,
        "levels" | "histogram_scan" | "invert" | "grayscale" | "hsl" | "posterize" => adjust,
        "gradient_map" => gradient_map,
        "channel_shuffle" => channel_shuffle,
        _ => return None,
    };
    Some(f)
}

// ------------------------------------------------------------------ uniform blocks
// Field order and sizes mirror the `Params` structs of the WGSL kernels.

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ColorU {
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ShapeU {
    size_x: f32,
    size_y: f32,
    scale: f32,
    rotation: f32,
    offset_x: f32,
    offset_y: f32,
    softness: f32,
    sides: f32,
    inner: f32,
    thickness: f32,
    corner: f32,
    repeat: f32,
    kind: u32,
    tiling: u32,
    invert: u32,
    pad0: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TileU {
    size_x: f32,
    size_y: f32,
    scale: f32,
    offset: f32,
    rotation: f32,
    random_rotation: f32,
    random_scale: f32,
    random_offset: f32,
    luminance: f32,
    random_luminance: f32,
    mask_random: f32,
    softness: f32,
    sides: f32,
    inner: f32,
    thickness: f32,
    corner: f32,
    x_amount: i32,
    y_amount: i32,
    pattern: u32,
    blend_add: u32,
    seed: u32,
    tiling: u32,
    reach: i32,
    vertical: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GradientU {
    angle: f32,
    center_x: f32,
    center_y: f32,
    repeat: f32,
    offset: f32,
    kind: u32,
    invert: u32,
    tiling: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct NoiseU {
    roughness: f32,
    jitter: f32,
    distortion: f32,
    density: f32,
    scale: i32,
    octaves: i32,
    seed: u32,
    tiling: u32,
    invert: u32,
    stretch: i32,
    count: i32,
    length: f32,
    width: f32,
    angle: f32,
    spread: f32,
    thickness: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TransformU {
    matte: [f32; 4],
    offset_x: f32,
    offset_y: f32,
    rotation: f32,
    scale_x: f32,
    scale_y: f32,
    pivot_x: f32,
    pivot_y: f32,
    pad0: f32,
    filtering: u32,
    tiling: u32,
    samples: u32,
    pad1: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BlendU {
    opacity: f32,
    mode: u32,
    ignore_alpha: u32,
    clamp_result: u32,
    tiling: u32,
    pad: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct BlurU {
    pub sigma: f32,
    pub radius: i32,
    pub dir_x: i32,
    pub dir_y: i32,
    pub tiling: u32,
    pub pad: [u32; 3],
}

impl BlurU {
    pub fn copy(tiling: u32) -> BlurU {
        BlurU { sigma: 0.0, radius: 0, dir_x: 0, dir_y: 0, tiling, pad: [0; 3] }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ScalarU {
    value: f32,
    a: u32,
    b: u32,
    c: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct AdjustU {
    f: [f32; 8],
    op: u32,
    mode: u32,
    flag: u32,
    tiling: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GradientMapU {
    channel: u32,
    keep_alpha: u32,
    tiling: u32,
    pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ShuffleU {
    channels: [u32; 4],
    tiling: u32,
    pad: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DirectionalU {
    dir_x: f32,
    dir_y: f32,
    pad0: f32,
    pad1: f32,
    samples: i32,
    symmetric: u32,
    tiling: u32,
    pad2: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SlopeU {
    dir_x: f32,
    dir_y: f32,
    amount: f32,
    pad0: f32,
    samples: i32,
    mode: u32,
    tiling: u32,
    pad1: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct HeightU {
    radius_x: f32,
    radius_y: f32,
    intensity: f32,
    depth: f32,
    invert: u32,
    tiling: u32,
    pad: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct EmbossU {
    light: [f32; 3],
    intensity: f32,
    mode: u32,
    tiling: u32,
    pad: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct DisplayU {
    pub mode: u32,
    pub checker: f32,
    /// The format of the output an image is shown as (0 shows it as it is).
    pub format: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PixelMathU {
    consts: [[f32; 4]; 4],
    tiling: u32,
    channel: u32,
    pad: [u32; 2],
}

fn bytes<T: Pod>(v: &T) -> &[u8] {
    bytemuck::bytes_of(v)
}

fn input<'b>(inputs: &'b [Option<GpuImage>], index: usize, fallback: &'b GpuImage) -> &'b GpuImage {
    inputs.get(index).and_then(|i| i.as_ref()).unwrap_or(fallback)
}

fn connected(inputs: &[Option<GpuImage>], index: usize) -> bool {
    inputs.get(index).is_some_and(|i| i.is_some())
}

/// A length given in pixels of a 1024 px wide image, in uv units of `output` (x, y).
fn px_to_uv(px: f32, output: &GpuImage) -> [f32; 2] {
    let [w, h] = output.size();
    let u = px / 1024.0;
    [u, u * w as f32 / h.max(1) as f32]
}

/// Screen direction (y down) of a counter-clockwise angle in degrees.
fn direction(deg: f32) -> [f32; 2] {
    let (s, c) = deg.to_radians().sin_cos();
    [c, -s]
}

// ------------------------------------------------------------------ generators

fn uniform_color(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    cx.gpu.run(cx.encoder, "uniform_color", bytes(&ColorU { color: cx.params.color("color") }), &[], cx.output)?;
    Ok(None)
}

fn shape(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let [sx, sy] = p.v2("size");
    let [ox, oy] = p.v2("offset");
    let u = ShapeU {
        size_x: sx,
        size_y: sy,
        scale: p.f("scale"),
        rotation: p.f("rotation"),
        offset_x: ox,
        offset_y: oy,
        softness: p.f("softness").max(0.0),
        sides: p.i("sides").clamp(3, 64) as f32,
        inner: p.f("inner_radius"),
        thickness: p.f("thickness"),
        corner: p.f("corner_radius").max(0.0),
        repeat: p.i("repeat").clamp(1, 256) as f32,
        kind: p.u("shape"),
        tiling: p.tiling(),
        invert: u32::from(p.b("invert")),
        pad0: 0,
    };
    cx.gpu.run(cx.encoder, "shape", bytes(&u), &[], cx.output)?;
    Ok(None)
}

fn tile_generator(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let [sx, sy] = p.v2("size");
    let scale = p.f("scale");
    let random_offset = p.f("random_offset");
    // How many neighbouring cells can reach into a cell.
    let extent = sx.abs().max(sy.abs()) * scale.abs() * 0.5 * std::f32::consts::SQRT_2 + random_offset * 0.5;
    let reach = ((extent - 0.5).ceil() as i32).clamp(1, 3);
    let pattern = p.u("pattern");
    let mut warning = None;
    if pattern == TILE_PATTERN_IMAGE && !connected(cx.inputs, 0) {
        warning = Some("The pattern is 'Input Image' but no image is connected".to_string());
    }
    let u = TileU {
        size_x: sx,
        size_y: sy,
        scale,
        offset: p.f("offset"),
        rotation: p.f("rotation"),
        random_rotation: p.f("random_rotation"),
        random_scale: p.f("random_scale").clamp(0.0, 1.0),
        random_offset,
        luminance: p.f("luminance"),
        random_luminance: p.f("random_luminance").clamp(0.0, 1.0),
        mask_random: p.f("mask_random"),
        softness: p.f("softness").max(0.0),
        sides: p.i("sides").clamp(3, 64) as f32,
        inner: p.f("inner_radius"),
        thickness: p.f("thickness"),
        corner: p.f("corner_radius").max(0.0),
        x_amount: p.i("x_amount").clamp(1, 1024),
        y_amount: p.i("y_amount").clamp(1, 1024),
        pattern,
        blend_add: p.u("blend_mode"),
        seed: p.i("seed") as u32,
        tiling: p.tiling(),
        reach,
        vertical: u32::from(p.b("vertical_offset")),
    };
    let pattern_img = input(cx.inputs, 0, &cx.gpu.black);
    cx.gpu.run(cx.encoder, "tile_gen", bytes(&u), &[pattern_img], cx.output)?;
    Ok(warning)
}

fn gradient(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let [cx_, cy] = p.v2("center");
    let u = GradientU {
        angle: p.f("angle"),
        center_x: cx_,
        center_y: cy,
        repeat: p.f("repeat"),
        offset: p.f("offset"),
        kind: p.u("type"),
        invert: u32::from(p.b("invert")),
        tiling: p.tiling(),
    };
    cx.gpu.run(cx.encoder, "gradient", bytes(&u), &[], cx.output)?;
    Ok(None)
}

fn noise(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let kind = p.u("type");
    let scale = p.i("scale").clamp(1, 4096);
    // Lines are as wide as they are at any resolution: a width in pixels of a 1024 px
    // image, in cells.
    let width = match kind {
        NOISE_VEINS | NOISE_CRACKS => p.f("line_width"),
        _ => p.f("width"),
    };
    let width = width.max(0.0) / 1024.0 * scale as f32;
    let density = match kind {
        NOISE_FIBERS | NOISE_SCRATCHES => p.f("line_density"),
        _ => p.f("density"),
    };
    let count = if kind == NOISE_WOOD { p.i("rings").clamp(1, 256) } else { 0 };
    let u = NoiseU {
        roughness: p.f("roughness").clamp(0.0, 1.0),
        jitter: p.f("jitter").clamp(0.0, 1.0),
        distortion: p.f("distortion"),
        density: density.clamp(0.0, 1.0),
        scale,
        octaves: p.i("octaves").clamp(1, 10),
        seed: p.i("seed") as u32,
        tiling: p.tiling(),
        invert: u32::from(p.b("invert")),
        stretch: p.i("stretch").clamp(1, 4096),
        count,
        length: p.f("length").clamp(0.01, 6.0),
        width,
        angle: p.f("angle"),
        spread: p.f("spread").clamp(0.0, 1.0),
        thickness: p.f("thickness").clamp(0.0, 1.0),
    };
    // Each type is a pipeline of its own (see common.wgsl's KIND).
    cx.gpu.run_kind(cx.encoder, "noise", kind, bytes(&u), &[], cx.output)?;
    Ok(None)
}

fn bitmap(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    match cx.bitmap {
        Some(img) => {
            cx.gpu.run(cx.encoder, "resample", bytes(&BlurU::copy(cx.params.tiling())), &[img], cx.output)?
        }
        None => cx.gpu.run(cx.encoder, "uniform_color", bytes(&ColorU { color: [0.0; 4] }), &[], cx.output)?,
    }
    Ok(None)
}

// ------------------------------------------------------------------ filters

fn transform_2d(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let [ox, oy] = p.v2("offset");
    let [sx, sy] = p.v2("scale");
    let [px, py] = p.v2("pivot");
    let min_scale = sx.abs().min(sy.abs()).max(1e-3);
    let u = TransformU {
        matte: p.color("matte"),
        offset_x: ox,
        offset_y: oy,
        rotation: p.f("rotation"),
        scale_x: sx,
        scale_y: sy,
        pivot_x: px,
        pivot_y: py,
        pad0: 0.0,
        filtering: p.u("filtering"),
        tiling: p.tiling(),
        // Supersample when shrinking, so the result does not alias.
        samples: (1.0 / min_scale).ceil().clamp(1.0, 4.0) as u32,
        pad1: 0,
    };
    let img = input(cx.inputs, 0, &cx.gpu.black);
    cx.gpu.run(cx.encoder, "transform", bytes(&u), &[img], cx.output)?;
    Ok(None)
}

fn blend(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let u = BlendU {
        opacity: p.f("opacity"),
        mode: p.u("mode"),
        ignore_alpha: p.u("alpha"),
        clamp_result: u32::from(p.b("clamp")),
        tiling: p.tiling(),
        pad: [0; 3],
    };
    let fg = input(cx.inputs, 0, &cx.gpu.black);
    let bg = input(cx.inputs, 1, &cx.gpu.black);
    let mask = input(cx.inputs, 2, &cx.gpu.white);
    cx.gpu.run(cx.encoder, "blend", bytes(&u), &[fg, bg, mask], cx.output)?;
    Ok(None)
}

fn blur(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let sigma = cx.params.f("radius").max(0.0) * 0.5 * cx.output.width() as f32 / 1024.0;
    let img = input(cx.inputs, 0, &cx.gpu.black).clone();
    gaussian_blur(cx.gpu, cx.encoder, &img, cx.output, sigma, cx.params.tiling())?;
    Ok(None)
}

fn directional_blur(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let len = px_to_uv(p.f("length").max(0.0), cx.output);
    let d = direction(p.f("angle"));
    // About one tap per pixel of streak, capped.
    let taps = (p.f("length").abs() * cx.output.width() as f32 / 1024.0).ceil().clamp(1.0, 256.0) as i32;
    let u = DirectionalU {
        dir_x: d[0] * len[0],
        dir_y: d[1] * len[1],
        pad0: 0.0,
        pad1: 0.0,
        samples: taps,
        symmetric: u32::from(p.b("symmetric")),
        tiling: p.tiling(),
        pad2: 0,
    };
    let img = input(cx.inputs, 0, &cx.gpu.black);
    cx.gpu.run(cx.encoder, "directional_blur", bytes(&u), &[img], cx.output)?;
    Ok(None)
}

fn slope_blur(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let amount = p.f("intensity") / 1024.0;
    let u = SlopeU {
        dir_x: 0.0,
        dir_y: 0.0,
        amount,
        pad0: 0.0,
        samples: p.i("samples").clamp(1, 256),
        mode: p.u("mode"),
        tiling: p.tiling(),
        pad1: 0,
    };
    let img = input(cx.inputs, 0, &cx.gpu.black);
    let map = input(cx.inputs, 1, &cx.gpu.black);
    cx.gpu.run(cx.encoder, "slope_blur", bytes(&u), &[img, map], cx.output)?;
    Ok(None)
}

fn sharpen(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let sigma = (p.f("radius").max(0.05) * 0.5 * cx.output.width() as f32 / 1024.0).max(0.35);
    let img = input(cx.inputs, 0, &cx.gpu.black).clone();
    let [w, h] = cx.output.size();
    let blurred = cx.gpu.take_scratch(w, h, cx.output.format());
    gaussian_blur(cx.gpu, cx.encoder, &img, &blurred, sigma, p.tiling())?;
    let u = ScalarU { value: p.f("intensity"), a: p.tiling(), b: 0, c: 0 };
    let r = cx.gpu.run(cx.encoder, "sharpen", bytes(&u), &[&img, &blurred], cx.output);
    cx.gpu.recycle(blurred);
    r?;
    Ok(None)
}

fn warp(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let u = ScalarU { value: cx.params.f("intensity"), a: cx.params.tiling(), b: 0, c: 0 };
    let img = input(cx.inputs, 0, &cx.gpu.black);
    let map = input(cx.inputs, 1, &cx.gpu.black);
    cx.gpu.run(cx.encoder, "warp", bytes(&u), &[img, map], cx.output)?;
    Ok(None)
}

fn directional_warp(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let d = direction(p.f("angle"));
    let [w, h] = cx.output.size();
    let k = p.f("intensity");
    let u = SlopeU {
        dir_x: d[0] * k,
        dir_y: d[1] * k * w as f32 / h.max(1) as f32,
        amount: 0.0,
        pad0: 0.0,
        samples: 0,
        mode: 0,
        tiling: p.tiling(),
        pad1: 0,
    };
    let img = input(cx.inputs, 0, &cx.gpu.black);
    let map = input(cx.inputs, 1, &cx.gpu.black);
    cx.gpu.run(cx.encoder, "directional_warp", bytes(&u), &[img, map], cx.output)?;
    Ok(None)
}

fn mirror(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let u = ScalarU { value: cx.params.f("offset"), a: cx.params.u("mode"), b: cx.params.tiling(), c: 0 };
    let img = input(cx.inputs, 0, &cx.gpu.black);
    cx.gpu.run(cx.encoder, "mirror", bytes(&u), &[img], cx.output)?;
    Ok(None)
}

// ------------------------------------------------------------------ height & normal

fn normal(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let u = ScalarU { value: cx.params.f("intensity"), a: cx.params.u("format"), b: cx.params.tiling(), c: 0 };
    let img = input(cx.inputs, 0, &cx.gpu.black);
    cx.gpu.run(cx.encoder, "normal", bytes(&u), &[img], cx.output)?;
    Ok(None)
}

fn height_kernel(cx: &mut ExecCtx, kernel: &str, radius_px: f32, intensity: f32, depth: f32) -> Result<(), String> {
    let r = px_to_uv(radius_px, cx.output);
    let u = HeightU {
        radius_x: r[0],
        radius_y: r[1],
        intensity,
        depth,
        invert: u32::from(cx.params.b("invert")),
        tiling: cx.params.tiling(),
        pad: [0; 2],
    };
    let img = input(cx.inputs, 0, &cx.gpu.black);
    cx.gpu.run(cx.encoder, kernel, bytes(&u), &[img], cx.output)
}

fn ambient_occlusion(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let (radius, intensity, depth) = (p.f("radius").max(0.5), p.f("intensity"), p.f("height"));
    // The input at every scale: a sample far from the pixel reads the whole patch of
    // ground it stands for, from the level of that size. (A grayscale height map stays one
    // channel.)
    let img = input(cx.inputs, 0, &cx.gpu.black).clone();
    let [w, h] = cx.output.size();
    let heights = cx.gpu.take_scratch_with_mips(w, h, crate::gpu::format_of(img.kind()));
    cx.gpu.encode_copy_with_mips(cx.encoder, &img, &heights, 0)?;
    let r = px_to_uv(radius, cx.output);
    let u = HeightU { radius_x: r[0], radius_y: r[1], intensity, depth, invert: 0, tiling: p.tiling(), pad: [0; 2] };
    let result = cx.gpu.run_filtered(cx.encoder, "ambient_occlusion", bytes(&u), &[&heights], cx.output, p.tiling());
    cx.gpu.recycle(heights);
    result.map(|()| None)
}

fn curvature(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let (radius, intensity) = (p.f("radius").max(0.1), p.f("intensity"));
    height_kernel(cx, "curvature", radius, intensity, 0.0)?;
    Ok(None)
}

fn edge_detect(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let (width, intensity) = (p.f("width").max(0.1), p.f("intensity"));
    height_kernel(cx, "edge_detect", width, intensity, 0.0)?;
    Ok(None)
}

fn emboss(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let (sa, ca) = p.f("light_angle").to_radians().sin_cos();
    let (se, ce) = p.f("elevation").clamp(1.0, 90.0).to_radians().sin_cos();
    let u = EmbossU {
        light: [ce * ca, ce * sa, se],
        intensity: p.f("intensity"),
        mode: p.u("output"),
        tiling: p.tiling(),
        pad: [0; 2],
    };
    let mut warning = None;
    if u.mode == 1 && !connected(cx.inputs, 1) {
        warning = Some("'Lit color' needs an image in the Color input".to_string());
    }
    let height = input(cx.inputs, 0, &cx.gpu.black);
    let color = input(cx.inputs, 1, &cx.gpu.white);
    cx.gpu.run(cx.encoder, "emboss", bytes(&u), &[height, color], cx.output)?;
    Ok(warning)
}

// ------------------------------------------------------------------ adjustments

fn adjust(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let mut u = AdjustU { f: [0.0; 8], op: 0, mode: 0, flag: 0, tiling: p.tiling() };
    match cx.kind.id() {
        "levels" => {
            u.op = 0;
            u.f[..5].copy_from_slice(&[p.f("in_low"), p.f("in_high"), p.f("gamma"), p.f("out_low"), p.f("out_high")]);
            u.mode = p.u("channels");
            u.flag = u32::from(p.b("clamp"));
        }
        "invert" => {
            u.op = 1;
            u.flag = u32::from(p.b("invert_alpha"));
        }
        "grayscale" => {
            u.op = 2;
            u.mode = p.u("mode");
        }
        "hsl" => {
            u.op = 3;
            u.f[0] = p.f("hue") / 360.0;
            u.f[1] = p.f("saturation");
            u.f[2] = p.f("lightness");
        }
        "histogram_scan" => {
            u.op = 4;
            u.f[0] = p.f("position");
            u.f[1] = p.f("contrast");
            u.flag = u32::from(p.b("invert"));
        }
        "posterize" => {
            u.op = 5;
            u.f[0] = p.i("steps").clamp(2, 256) as f32;
        }
        other => return Err(format!("'{other}' is not an adjustment")),
    }
    let img = input(cx.inputs, 0, &cx.gpu.black);
    cx.gpu.run(cx.encoder, "adjust", bytes(&u), &[img], cx.output)?;
    Ok(None)
}

fn gradient_map(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let lut = cx.lut.ok_or("The gradient was not prepared")?;
    let u = GradientMapU {
        channel: cx.params.u("channel"),
        keep_alpha: u32::from(cx.params.b("keep_alpha")),
        tiling: cx.params.tiling(),
        pad: 0,
    };
    let img = input(cx.inputs, 0, &cx.gpu.black);
    cx.gpu.run(cx.encoder, "gradient_map", bytes(&u), &[img, lut], cx.output)?;
    Ok(None)
}

fn channel_shuffle(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let u =
        ShuffleU { channels: [p.u("red"), p.u("green"), p.u("blue"), p.u("alpha")], tiling: p.tiling(), pad: [0; 3] };
    let a = input(cx.inputs, 0, &cx.gpu.black);
    let b = input(cx.inputs, 1, &cx.gpu.black);
    cx.gpu.run(cx.encoder, "shuffle", bytes(&u), &[a, b], cx.output)?;
    Ok(None)
}

/// Pass the first input through unchanged (bypassed nodes).
pub fn passthrough(
    gpu: &Gpu,
    encoder: &mut wgpu::CommandEncoder,
    input: Option<&GpuImage>,
    output: &GpuImage,
    tiling: u32,
) -> Result<(), String> {
    match input {
        Some(img) => gpu.run(encoder, "resample", bytes(&BlurU::copy(tiling)), &[img], output),
        None => gpu.run(encoder, "uniform_color", bytes(&ColorU { color: [0.0; 4] }), &[], output),
    }
}

/// Fill with transparent black (after an error, so downstream nodes still work).
pub fn clear(gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, output: &GpuImage) -> Result<(), String> {
    gpu.run(encoder, "uniform_color", bytes(&ColorU { color: [0.0; 4] }), &[], output)
}

// ------------------------------------------------------------------ blur

/// Separable Gaussian blur. Large radii are blurred on a downsampled copy and upsampled
/// again, which keeps the cost bounded for any radius.
pub fn gaussian_blur(
    gpu: &Gpu,
    encoder: &mut wgpu::CommandEncoder,
    input: &GpuImage,
    output: &GpuImage,
    sigma: f32,
    tiling: u32,
) -> Result<(), String> {
    let copy = BlurU::copy(tiling);
    if sigma < 0.2 || input.size() != output.size() {
        // Nothing to blur (or a 1x1 placeholder input): plain resample.
        return gpu.run(encoder, "resample", bytes(&copy), &[input], output);
    }
    let mut s = sigma;
    let mut levels = 0;
    while s > 12.0 && levels < 6 {
        s *= 0.5;
        levels += 1;
    }
    let mut chain: Vec<GpuImage> = Vec::new();
    let mut current = input.clone();
    for _ in 0..levels {
        let (cw, ch) = (current.width().div_ceil(2).max(1), current.height().div_ceil(2).max(1));
        let next = gpu.take_scratch(cw, ch, output.format());
        gpu.run(encoder, "downsample", bytes(&copy), &[&current], &next)?;
        current = next.clone();
        chain.push(next);
    }
    let [cw, ch] = current.size();
    let radius = (s * 3.0).ceil() as i32;
    let horizontal = BlurU { sigma: s, radius, dir_x: 1, dir_y: 0, tiling, pad: [0; 3] };
    let vertical = BlurU { dir_x: 0, dir_y: 1, ..horizontal };
    let a = gpu.take_scratch(cw, ch, output.format());
    gpu.run(encoder, "blur_1d", bytes(&horizontal), &[&current], &a)?;
    if levels == 0 {
        gpu.run(encoder, "blur_1d", bytes(&vertical), &[&a], output)?;
    } else {
        let b = gpu.take_scratch(cw, ch, output.format());
        gpu.run(encoder, "blur_1d", bytes(&vertical), &[&a], &b)?;
        gpu.run(encoder, "resample", bytes(&copy), &[&b], output)?;
        gpu.recycle(b);
    }
    gpu.recycle(a);
    for img in chain {
        gpu.recycle(img);
    }
    Ok(())
}

// ------------------------------------------------------------------ pixel math

pub const MAX_PIXEL_IMAGES: usize = 8;
pub const MAX_PIXEL_CONSTANTS: usize = 16;

/// Pixel Math: compile the expression to WGSL and run it for every pixel.
fn pixel_math(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let params = cx.params;
    let src = params.text("expression");
    let compiled = expr::compile_cached(src);
    let program = match &*compiled {
        Ok(p) => p,
        Err(e) => return Err(format!("The expression is incorrect: {}", e.message)),
    };
    let mut images: Vec<GpuImage> = Vec::new();
    let mut consts: Vec<f32> = Vec::new();
    let mut exprs: Vec<String> = Vec::new();
    for i in 0..program.vars.len() {
        let var = cx.pixel_vars.get(i).cloned().unwrap_or(PixelVar::Constant(0.0));
        let e = match var {
            PixelVar::Builtin(b) => match b {
                "u" => "uv.x".to_string(),
                "v" => "uv.y".to_string(),
                "px" => "f32(gid.x)".to_string(),
                "py" => "f32(gid.y)".to_string(),
                "width" => "f32(size.x)".to_string(),
                _ => "f32(size.y)".to_string(),
            },
            PixelVar::Image(img) => {
                if images.len() >= MAX_PIXEL_IMAGES {
                    return Err(format!("Pixel Math supports at most {MAX_PIXEL_IMAGES} image inputs"));
                }
                images.push(img);
                format!("img{}", images.len() - 1)
            }
            PixelVar::Constant(v) => {
                if consts.len() >= MAX_PIXEL_CONSTANTS {
                    return Err(format!("Pixel Math supports at most {MAX_PIXEL_CONSTANTS} constant inputs"));
                }
                let slot = consts.len();
                consts.push(v);
                const LANES: [&str; 4] = ["x", "y", "z", "w"];
                format!("P.consts[{}].{}", slot / 4, LANES[slot % 4])
            }
        };
        exprs.push(e);
    }
    let body = expr::wgsl::to_wgsl(program, &|i| exprs[i].clone());

    let mut source = String::new();
    source.push_str(crate::gpu::COMMON_WGSL);
    source.push_str(expr::wgsl::PRELUDE);
    source.push_str(
        "struct Params { consts: array<vec4<f32>, 4>, tiling: u32, channel: u32, pad0: u32, pad1: u32 };\n\
         @group(0) @binding(0) var<uniform> P: Params;\n",
    );
    for i in 0..images.len() {
        source.push_str(&format!("@group(0) @binding({}) var t{i}: texture_2d<f32>;\n", i + 1));
    }
    source.push_str(&format!(
        "@group(0) @binding({}) var out_tex: texture_storage_2d<{}, write>;\n",
        images.len() + 1,
        crate::gpu::wgsl_format(cx.output.format())
    ));
    source.push_str(
        "fn chan(c: vec4<f32>) -> f32 {\n\
             switch P.channel {\n\
                 case 1u: { return c.r; }\n\
                 case 2u: { return c.g; }\n\
                 case 3u: { return c.b; }\n\
                 case 4u: { return c.a; }\n\
                 default: { return luminance(c.rgb); }\n\
             }\n\
         }\n\
         @compute @workgroup_size(8, 8, 1)\n\
         fn main(@builtin(global_invocation_id) gid: vec3<u32>) {\n\
             let size = textureDimensions(out_tex);\n\
             if (gid.x >= size.x || gid.y >= size.y) { return; }\n\
             let uv = out_uv(gid.xy, size);\n",
    );
    for (i, image) in images.iter().enumerate() {
        let gray = image.is_gray();
        source.push_str(&format!("    let img{i} = chan(widen(read_input(t{i}, gid.xy, size, P.tiling), {gray}));\n"));
    }
    source.push_str(&format!(
        "    let r = {body};\n    textureStore(out_tex, vec2<i32>(gid.xy), vec4<f32>(r, r, r, 1.0));\n}}\n"
    ));

    let kernel = cx.gpu.dynamic_kernel(&source, images.len(), cx.output.format())?;
    let mut u =
        PixelMathU { consts: [[0.0; 4]; 4], tiling: params.tiling(), channel: params.u("channel"), pad: [0; 2] };
    for (i, v) in consts.iter().enumerate() {
        u.consts[i / 4][i % 4] = *v;
    }
    let inputs: Vec<&GpuImage> = images.iter().collect();
    cx.gpu.run_dynamic(cx.encoder, &kernel, bytes(&u), &inputs, cx.output);
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_image_node_has_a_kernel() {
        for kind in NodeKind::ALL {
            if *kind == NodeKind::OUTPUT {
                // An output shows what arrives at it.
                assert!(kernel_for(*kind).is_none());
            } else if kind.def().has_image() {
                assert!(kernel_for(*kind).is_some(), "no kernel for {}", kind.def().name);
            } else {
                assert!(kernel_for(*kind).is_none(), "value node {} has a kernel", kind.def().name);
            }
        }
    }
}
