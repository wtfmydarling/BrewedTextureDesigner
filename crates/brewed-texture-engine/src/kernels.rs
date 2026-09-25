//! GPU implementations of the image node types.
//!
//! [`kernel_for`] maps a node type to the function that records its GPU work. Every
//! function reads the node's effective parameters from the [`ExecCtx`], binds its inputs
//! and draws with one or more kernels (see `gpu/shaders`). Adding an image node type means
//! adding its definition in `brewed-texture-core` and one function here.

use bytemuck::{Pod, Zeroable};

use brewed_texture_core::NodeKind;
use brewed_texture_core::eval::Params;
use brewed_texture_core::expr;
use brewed_texture_core::nodes::{NOISE_CRACKS, NOISE_FIBERS, NOISE_SCRATCHES, NOISE_VEINS, NOISE_WOOD, TILE_PATTERN_IMAGE};

use crate::gpu::{Gpu, GpuImage, RUN_FORMAT, SEED_FORMAT};

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
    /// The picture a node draws from: the decoded file of a Bitmap node, the drawing of an SVG
    /// or a Text node.
    pub bitmap: Option<&'a GpuImage>,
    /// The table baked for a Gradient Map (its gradient) or a Curve (its curve).
    pub lut: Option<&'a GpuImage>,
    /// Pixel Math: resolved variables in the order of the expression's variables.
    pub pixel_vars: &'a [PixelVar],
}

/// Records the work of one node; returns a warning for the UI, if any.
pub type KernelFn = fn(&mut ExecCtx) -> Result<Option<String>, String>;

/// Whether a node of `kind` reads its first input at every scale, through its mips: the
/// engine gives that input mips when it computes it.
pub fn reads_at_every_scale(kind: NodeKind) -> bool {
    kind == NodeKind::AMBIENT_OCCLUSION || kind == NodeKind::NON_UNIFORM_BLUR
}

/// The GPU implementation of an image node type.
pub fn kernel_for(kind: NodeKind) -> Option<KernelFn> {
    let f: KernelFn = match kind.id() {
        "uniform_color" => uniform_color,
        "image_input" => image_input,
        "shape" => shape,
        "tile_generator" => tile_generator,
        "tile_sampler" => tile_sampler,
        "gradient" => gradient,
        "noise" => noise,
        // The picture is drawn at the texture's size (see `crate::vector`): copied as is.
        "bitmap" | "svg" | "text" => bitmap,
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
        "distance" => distance,
        "bevel" => bevel,
        "height_blend" => height_blend,
        "normal_combine" => normal_combine,
        "non_uniform_blur" => non_uniform_blur,
        "make_it_tile" => make_it_tile,
        "flood_fill" => flood_fill,
        "levels" | "histogram_scan" | "invert" | "grayscale" | "hsl" | "posterize" => adjust,
        "gradient_map" => gradient_map,
        "channel_shuffle" => channel_shuffle,
        "rgba_merge" => rgba_merge,
        "curve" => curve,
        "output" => output,
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
struct TileSamplerU {
    size_x: f32,
    size_y: f32,
    scale: f32,
    random_scale: f32,
    scale_map: f32,
    random_position: f32,
    offset: f32,
    rotation: f32,
    random_rotation: f32,
    rotation_map: f32,
    random_mirror: f32,
    luminance: f32,
    random_luminance: f32,
    mask_random: f32,
    mask_threshold: f32,
    softness: f32,
    sides: f32,
    inner: f32,
    thickness: f32,
    corner: f32,
    x_amount: i32,
    y_amount: i32,
    pattern: u32,
    blend: u32,
    seed: u32,
    tiling: u32,
    reach: i32,
    vertical: u32,
    /// The pattern inputs connected, as bits.
    patterns: u32,
    /// The maps connected (mask, scale, rotation, color), as bits.
    maps: u32,
    color: u32,
    mask_invert: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MergeU {
    /// Red, green, blue and alpha where nothing is connected.
    values: [f32; 4],
    /// The channels connected, as bits.
    connected: u32,
    tiling: u32,
    pad: [u32; 2],
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
struct HeightBlendU {
    offset: f32,
    contrast: f32,
    output: u32,
    masked: u32,
    tiling: u32,
    pad: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct NormalCombineU {
    intensity: f32,
    technique: u32,
    masked: u32,
    tiling: u32,
    /// Whether the base and the detail are connected (else they are flat).
    base: u32,
    detail: u32,
    pad: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct NonUniformU {
    radius_x: f32,
    radius_y: f32,
    tiling: u32,
    mapped: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FloodU {
    tiling: u32,
    seed: u32,
    /// Gray (0) or color (1).
    mode: u32,
    /// How far a pass of the runs looks.
    step: u32,
}

/// How many times the labels of a flood fill spread and jump (see flood.wgsl) before it looks
/// whether they still change; then twice as many each time, up to [`FLOOD_MOST_ROUNDS`] in all.
const FLOOD_ROUNDS: usize = 16;
/// As many rounds as a flood fill takes, however winding its shapes (a maze).
const FLOOD_MOST_ROUNDS: usize = 4096;

/// The parameters of the passes of a distance field (distance.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DistanceU {
    /// How far a pass looks.
    step: i32,
    tiling: u32,
    /// The seeds are the pixels of the shapes (1), or those around them (0).
    inside: u32,
    /// Count from the largest distance wanted back.
    flip: u32,
    threshold: f32,
    /// In pixels of the image.
    max_distance: f32,
    profile: u32,
    pad: u32,
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
    pub pad: [u32; 2],
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

/// What an Image Input is while nothing arrives at it (and what the graph sees on its own):
/// its default color, made what it takes.
fn image_input(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let c = cx.params.color("default");
    let color = if cx.output.is_gray() {
        let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        [l, l, l, c[3]]
    } else {
        c
    };
    cx.gpu.run(cx.encoder, "uniform_color", bytes(&ColorU { color }), &[], cx.output)?;
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

/// Patterns on a grid, varied at random and by maps (see tile_sampler.wgsl). Each pattern is
/// read from its mips, as fine as it is drawn: one without them (or that cannot be filtered)
/// is copied into an image that has them.
fn tile_sampler(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let [sx, sy] = p.v2("size");
    let scale = p.f("scale");
    let random_position = p.f("random_position").clamp(0.0, 1.0);
    // How many neighbouring cells can reach into a cell (the maps only make tiles smaller).
    let extent = sx.abs().max(sy.abs()) * scale.abs() * 0.5 * std::f32::consts::SQRT_2 + random_position * 0.5;
    let reach = ((extent - 0.5).ceil() as i32).clamp(1, 3);
    let pattern = p.u("pattern");
    let bits = |from: usize| (0..4).filter(|&i| connected(cx.inputs, from + i)).fold(0u32, |bits, i| bits | (1 << i));
    let (patterns, maps) = (bits(0), bits(4));
    let color = p.u("color");
    let warning = if pattern == TILE_PATTERN_IMAGE && patterns == 0 {
        Some("The pattern is 'Input Image' but no image is connected".to_string())
    } else if color == 2 && maps & 8 == 0 {
        Some("The color is 'Color Map' but no Color Map is connected".to_string())
    } else {
        None
    };
    let u = TileSamplerU {
        size_x: sx,
        size_y: sy,
        scale,
        random_scale: p.f("random_scale").clamp(0.0, 1.0),
        scale_map: p.f("scale_map").clamp(0.0, 1.0),
        random_position,
        offset: p.f("offset"),
        rotation: p.f("rotation"),
        random_rotation: p.f("random_rotation"),
        rotation_map: p.f("rotation_map"),
        random_mirror: p.f("random_mirror").clamp(0.0, 1.0),
        luminance: p.f("luminance"),
        random_luminance: p.f("random_luminance").clamp(0.0, 1.0),
        mask_random: p.f("mask_random"),
        mask_threshold: p.f("mask_threshold"),
        softness: p.f("softness").max(0.0),
        sides: p.i("sides").clamp(3, 64) as f32,
        inner: p.f("inner_radius"),
        thickness: p.f("thickness"),
        corner: p.f("corner_radius").max(0.0),
        x_amount: p.i("x_amount").clamp(1, 1024),
        y_amount: p.i("y_amount").clamp(1, 1024),
        pattern,
        blend: p.u("blend_mode").min(2),
        seed: p.i("seed") as u32,
        tiling: p.tiling(),
        reach,
        vertical: u32::from(p.b("vertical_offset")),
        patterns,
        maps,
        color,
        mask_invert: u32::from(p.b("mask_invert")),
    };
    let mut copies: Vec<Option<GpuImage>> = Vec::with_capacity(4);
    let mut result = Ok(());
    for i in 0..4 {
        let img = input(cx.inputs, i, &cx.gpu.black);
        let used = pattern == TILE_PATTERN_IMAGE && connected(cx.inputs, i);
        let copy = if used && (img.texture.mip_level_count() == 1 || !crate::gpu::filterable(img.format())) {
            let [w, h] = img.size();
            let copy = cx.gpu.take_scratch_with_mips(w, h, crate::gpu::format_of(img.kind(), brewed_texture_core::Precision::Half));
            result = result.and_then(|()| cx.gpu.encode_copy_with_mips(cx.encoder, img, &copy, 0));
            Some(copy)
        } else {
            None
        };
        copies.push(copy);
    }
    let reads: Vec<&GpuImage> =
        (0..8).map(|i| copies.get(i).and_then(Option::as_ref).unwrap_or_else(|| input(cx.inputs, i, &cx.gpu.black))).collect();
    // Clamped (tiling 0): a pattern is read within itself; the maps wrap in the shader.
    let result = result.and_then(|()| cx.gpu.run_filtered(cx.encoder, "tile_sampler", bytes(&u), &reads, cx.output, 0));
    for copy in copies.into_iter().flatten() {
        cx.gpu.recycle(copy);
    }
    result.map(|()| warning)
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
    // The path is no longer than the image is wide: a longer one only goes round a tiled
    // image again, and far apart samples keep the GPU busy for long.
    let amount = (p.f("intensity") / 1024.0).clamp(-1.0, 1.0);
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
    // ground it stands for, from the level of that size. The engine computes the input with
    // its mips (see `reads_at_every_scale`); one that was there without them (or of another
    // size) is copied into an image that has them. (A grayscale height map stays one
    // channel.)
    let img = input(cx.inputs, 0, &cx.gpu.black).clone();
    let [w, h] = cx.output.size();
    let heights = (img.texture.mip_level_count() == 1 || img.size() != [w, h] || !crate::gpu::filterable(img.format()))
        .then(|| {
            let format = crate::gpu::format_of(img.kind(), brewed_texture_core::Precision::Half);
            let heights = cx.gpu.take_scratch_with_mips(w, h, format);
            cx.gpu.encode_copy_with_mips(cx.encoder, &img, &heights, 0).map(|()| heights)
        })
        .transpose()?;
    let r = px_to_uv(radius, cx.output);
    let u = HeightU { radius_x: r[0], radius_y: r[1], intensity, depth, invert: 0, tiling: p.tiling(), pad: [0; 2] };
    let read = heights.as_ref().unwrap_or(&img);
    let result = cx.gpu.run_filtered(cx.encoder, "ambient_occlusion", bytes(&u), &[read], cx.output, p.tiling());
    if let Some(heights) = heights {
        cx.gpu.recycle(heights);
    }
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

fn curve(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let table = cx.lut.ok_or("The curve was not prepared")?;
    let u = ScalarU { value: 0.0, a: cx.params.u("channels"), b: cx.params.tiling(), c: 0 };
    let img = input(cx.inputs, 0, &cx.gpu.black);
    cx.gpu.run(cx.encoder, "curve", bytes(&u), &[img, table], cx.output)?;
    Ok(None)
}

fn rgba_merge(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let connected = (0..4).filter(|&i| connected(cx.inputs, i)).fold(0u32, |bits, i| bits | (1 << i));
    let u = MergeU {
        values: [p.f("red_value"), p.f("green_value"), p.f("blue_value"), p.f("alpha_value")],
        connected,
        tiling: p.tiling(),
        pad: [0; 2],
    };
    let channels: Vec<&GpuImage> = (0..4).map(|i| input(cx.inputs, i, &cx.gpu.black)).collect();
    cx.gpu.run(cx.encoder, "rgba_merge", bytes(&u), &channels, cx.output)?;
    Ok(None)
}

/// An output: what arrives at it as it writes it, in its channels and at its bit depth (the
/// format of its image, see [`crate::gpu::output_format`]). Drawn by a render pass, which
/// writes the formats a kernel cannot.
fn output(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let img = input(cx.inputs, 0, &cx.gpu.black);
    cx.gpu.encode_show(cx.encoder, img, cx.output, cx.params.u("format"))?;
    Ok(None)
}

// ------------------------------------------------------------------ more filters

fn height_blend(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let u = HeightBlendU {
        offset: p.f("offset"),
        contrast: p.f("contrast"),
        output: p.u("output"),
        masked: u32::from(connected(cx.inputs, 2)),
        tiling: p.tiling(),
        pad: [0; 3],
    };
    let (top, bottom, mask) = (input(cx.inputs, 0, &cx.gpu.black), input(cx.inputs, 1, &cx.gpu.black), input(cx.inputs, 2, &cx.gpu.white));
    cx.gpu.run(cx.encoder, "height_blend", bytes(&u), &[top, bottom, mask], cx.output)?;
    Ok(None)
}

fn normal_combine(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let u = NormalCombineU {
        intensity: p.f("intensity"),
        technique: p.u("technique"),
        masked: u32::from(connected(cx.inputs, 2)),
        tiling: p.tiling(),
        base: u32::from(connected(cx.inputs, 0)),
        detail: u32::from(connected(cx.inputs, 1)),
        pad: [0; 2],
    };
    let (base, detail, mask) = (input(cx.inputs, 0, &cx.gpu.black), input(cx.inputs, 1, &cx.gpu.black), input(cx.inputs, 2, &cx.gpu.white));
    cx.gpu.run(cx.encoder, "normal_combine", bytes(&u), &[base, detail, mask], cx.output)?;
    Ok(None)
}

fn non_uniform_blur(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    // The input at every scale, as Ambient Occlusion reads it (see `reads_at_every_scale`): one
    // that came without its mips (or of another size) is copied into an image that has them.
    let img = input(cx.inputs, 0, &cx.gpu.black).clone();
    let [w, h] = cx.output.size();
    let mipped = (img.texture.mip_level_count() == 1 || img.size() != [w, h] || !crate::gpu::filterable(img.format()))
        .then(|| {
            let format = crate::gpu::format_of(img.kind(), brewed_texture_core::Precision::Half);
            let mipped = cx.gpu.take_scratch_with_mips(w, h, format);
            cx.gpu.encode_copy_with_mips(cx.encoder, &img, &mipped, 0).map(|()| mipped)
        })
        .transpose()?;
    let r = px_to_uv(p.f("radius"), cx.output);
    let u = NonUniformU { radius_x: r[0], radius_y: r[1], tiling: p.tiling(), mapped: u32::from(connected(cx.inputs, 1)) };
    let read = mipped.as_ref().unwrap_or(&img);
    let map = input(cx.inputs, 1, &cx.gpu.white);
    let result = cx.gpu.run_filtered(cx.encoder, "nonuniform_blur", bytes(&u), &[read, map], cx.output, p.tiling());
    if let Some(mipped) = mipped {
        cx.gpu.recycle(mipped);
    }
    result.map(|()| None)
}

fn make_it_tile(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let u = ScalarU { value: cx.params.f("blend"), a: cx.params.tiling(), b: 0, c: 0 };
    let img = input(cx.inputs, 0, &cx.gpu.black);
    cx.gpu.run(cx.encoder, "make_it_tile", bytes(&u), &[img], cx.output)?;
    Ok(None)
}

// ------------------------------------------------------------------ distance fields

/// The seeds of a distance field of `mask` (see distance.wgsl): each pixel knowing the nearest
/// pixel of the shapes (`inside` 1) or around them (0), as far as the passes reach: the first
/// looks as far as the largest distance wanted (or half across the image), the step halving
/// from pass to pass, and one more of one. An image of [`SEED_FORMAT`], to recycle.
fn distance_seeds(gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, mask: &GpuImage, size: [u32; 2], u: DistanceU) -> Result<GpuImage, String> {
    let [w, h] = size;
    let reach = (u.max_distance.ceil().max(1.0) as u32).next_power_of_two();
    let first = reach.min(w.max(h).next_power_of_two() / 2).max(1);
    let mut a = gpu.take_scratch(w, h, SEED_FORMAT);
    let mut b = gpu.take_scratch(w, h, SEED_FORMAT);
    let mut result = gpu.run(encoder, "distance_seed", bytes(&u), &[mask], &a);
    let mut step = first;
    let mut again = true;
    while result.is_ok() {
        result = gpu.run(encoder, "distance_jump", bytes(&DistanceU { step: step as i32, ..u }), &[&a], &b);
        std::mem::swap(&mut a, &mut b);
        if step > 1 {
            step /= 2;
        } else if std::mem::take(&mut again) {
            // Jump flooding + 1: a last pass of one mends the few pixels the others got wrong.
        } else {
            break;
        }
    }
    gpu.recycle(b);
    match result {
        Ok(()) => Ok(a),
        Err(e) => {
            gpu.recycle(a);
            Err(e)
        }
    }
}

/// A distance field of `mask` into `output`, as `u` says; `signed`: into the shapes and away
/// from them, both.
fn distance_field(gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, mask: &GpuImage, output: &GpuImage, u: DistanceU, signed: bool) -> Result<(), String> {
    let size = output.size();
    if signed {
        let shapes = distance_seeds(gpu, encoder, mask, size, DistanceU { inside: 1, ..u })?;
        let around = distance_seeds(gpu, encoder, mask, size, DistanceU { inside: 0, ..u });
        let result = around.and_then(|around| {
            let drawn = gpu.run(encoder, "distance_signed", bytes(&u), &[&shapes, &around], output);
            gpu.recycle(around);
            drawn
        });
        gpu.recycle(shapes);
        return result;
    }
    let seeds = distance_seeds(gpu, encoder, mask, size, u)?;
    let result = gpu.run(encoder, "distance_field", bytes(&u), &[&seeds], output);
    gpu.recycle(seeds);
    result
}

/// Each shape of the mask its own random value (see flood.wgsl): the pixels of each labelled with
/// the smallest place in it, spreading as far as the distance field of the shapes says they
/// have room and along their rows and columns, and jumping, until a round changes nothing (the
/// GPU says whether the last round of each batch did: the labels are then the same all over each
/// shape, whatever its shape).
fn flood_fill(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let size = cx.output.size();
    let [w, h] = size;
    let mask = input(cx.inputs, 0, &cx.gpu.black);
    let tiling = p.tiling();
    // How far each pixel of the shapes is from their outside, as far as it goes.
    let room = DistanceU { step: 0, tiling, inside: 0, flip: 0, threshold: p.f("threshold"), max_distance: w.max(h) as f32, profile: 0, pad: 0 };
    let outside = distance_seeds(cx.gpu, cx.encoder, mask, size, room)?;
    let u = FloodU { tiling, seed: p.i("seed").max(0) as u32, mode: p.u("output"), step: 0 };
    // The ends of each pixel's row and column in its shape.
    let mut runs = cx.gpu.take_scratch(w, h, RUN_FORMAT);
    let mut next = cx.gpu.take_scratch(w, h, RUN_FORMAT);
    let mut result = cx.gpu.run(cx.encoder, "runs_seed", bytes(&room), &[mask], &runs);
    let mut step = 1;
    while step < w.max(h) && result.is_ok() {
        result = cx.gpu.run(cx.encoder, "runs_step", bytes(&FloodU { step, ..u }), &[&runs], &next);
        std::mem::swap(&mut runs, &mut next);
        step *= 2;
    }
    cx.gpu.recycle(next);
    let mut a = cx.gpu.take_scratch(w, h, SEED_FORMAT);
    let mut b = cx.gpu.take_scratch(w, h, SEED_FORMAT);
    let before = cx.gpu.take_scratch(w, h, SEED_FORMAT);
    let changes = cx.gpu.take_scratch(w, h, crate::gpu::GRAY_FORMAT);
    result = result.and_then(|()| cx.gpu.run(cx.encoder, "flood_label", bytes(&room), &[mask], &a));
    let (mut batch, mut rounds, mut warning) = (FLOOD_ROUNDS, 0, None);
    while result.is_ok() {
        for round in 0..batch {
            if round + 1 == batch {
                // The labels before the last round, to see whether it changed any.
                cx.encoder.copy_texture_to_texture(
                    a.texture.as_image_copy(),
                    before.texture.as_image_copy(),
                    wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                );
            }
            result = result.and_then(|()| cx.gpu.run(cx.encoder, "flood_spread", bytes(&u), &[&a, &outside, &runs], &b));
            std::mem::swap(&mut a, &mut b);
            for _ in 0..2 {
                result = result.and_then(|()| cx.gpu.run(cx.encoder, "flood_jump", bytes(&u), &[&a], &b));
                std::mem::swap(&mut a, &mut b);
            }
        }
        rounds += batch;
        result = result.and_then(|()| cx.gpu.run(cx.encoder, "differ", &[0u8; 16], &[&before, &a], &changes));
        match result.as_ref().map_err(Clone::clone).and_then(|()| cx.gpu.any(cx.encoder, &changes)) {
            Ok(false) => break,
            Ok(true) if rounds >= FLOOD_MOST_ROUNDS => {
                warning = Some("The shapes wind too far to tell them all apart: some may have more than one value".to_string());
                break;
            }
            Ok(true) => batch = (batch * 2).min(FLOOD_MOST_ROUNDS - rounds),
            Err(e) => result = Err(e),
        }
    }
    let result = result.and_then(|()| cx.gpu.run(cx.encoder, "flood_fill", bytes(&u), &[&a], cx.output));
    for image in [a, b, before, changes, outside, runs] {
        cx.gpu.recycle(image);
    }
    result.map(|()| warning)
}

/// Pixels of the image, from pixels of a 1024 px texture.
fn px(value: f32, output: &GpuImage) -> f32 {
    value.max(0.0) * output.width() as f32 / 1024.0
}

fn distance(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let u = DistanceU {
        step: 0,
        tiling: p.tiling(),
        inside: 1,
        flip: 1,
        threshold: p.f("threshold"),
        max_distance: px(p.f("distance"), cx.output).max(1e-3),
        profile: 0,
        pad: 0,
    };
    let mask = input(cx.inputs, 0, &cx.gpu.black);
    match p.u("mode") {
        // From the shapes: 1 on them (their pixels are the seeds), 0 at the distance.
        0 => distance_field(cx.gpu, cx.encoder, mask, cx.output, u, false)?,
        // Into them: from the pixels around them.
        1 => distance_field(cx.gpu, cx.encoder, mask, cx.output, DistanceU { inside: 0, flip: 0, ..u }, false)?,
        _ => distance_field(cx.gpu, cx.encoder, mask, cx.output, u, true)?,
    }
    Ok(None)
}

fn bevel(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    // Inward: from the pixels around the shapes, rising into them; outward: from the shapes,
    // falling away from them.
    let outward = p.u("direction") == 1;
    let u = DistanceU {
        step: 0,
        tiling: p.tiling(),
        inside: u32::from(outward),
        flip: u32::from(outward),
        threshold: p.f("threshold"),
        max_distance: px(p.f("width"), cx.output).max(1e-3),
        profile: p.u("profile"),
        pad: 0,
    };
    let mask = input(cx.inputs, 0, &cx.gpu.black);
    distance_field(cx.gpu, cx.encoder, mask, cx.output, u, false)?;
    Ok(None)
}

// ------------------------------------------------------------------ blur

/// Separable Gaussian blur. Large radii are blurred on a downsampled copy and upsampled
/// again, which keeps the cost bounded for any radius: the copy is halved until the Gaussian
/// is narrow on it, and a Gaussian wider than four times the image (the image's average,
/// however much wider) is taken as that wide.
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
    let [w, h] = output.size();
    let mut s = sigma.min(4.0 * w.max(h) as f32);
    let (mut levels, mut cw, mut ch) = (0, w, h);
    while s > 12.0 && (cw > 1 || ch > 1) {
        s *= 0.5;
        levels += 1;
        (cw, ch) = (cw.div_ceil(2), ch.div_ceil(2));
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
         @fragment\n\
         fn main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {\n\
             let gid = pixel(at);\n\
             let size = target_size();\n\
             let uv = out_uv(gid.xy, size);\n",
    );
    for (i, image) in images.iter().enumerate() {
        let gray = image.is_gray();
        source.push_str(&format!("    let img{i} = chan(widen(read_input(t{i}, gid.xy, size, P.tiling), {gray}));\n"));
    }
    source.push_str(&format!(
        "    let r = {body};\n    return vec4<f32>(r, r, r, 1.0);\n}}\n"
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
            if kind.def().has_image() {
                assert!(kernel_for(*kind).is_some(), "no kernel for {}", kind.def().name);
            } else {
                assert!(kernel_for(*kind).is_none(), "value node {} has a kernel", kind.def().name);
            }
        }
    }
}
