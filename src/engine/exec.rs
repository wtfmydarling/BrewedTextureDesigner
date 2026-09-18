//! Records the GPU work of each image node kind.

use bytemuck::{Pod, Zeroable};
use eframe::wgpu;

use crate::expr;
use crate::gpu::{Gpu, GpuImage};
use crate::graph::{NodeKind, ParamValue};

/// Effective parameter values of a node (stored values with driven overrides).
#[derive(Clone, Debug, Default)]
pub struct EffParams {
    pub values: Vec<(String, ParamValue)>,
}

impl EffParams {
    pub fn get(&self, key: &str) -> Option<&ParamValue> {
        self.values.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    fn finite(v: f32) -> f32 {
        if v.is_finite() { v } else { 0.0 }
    }
    pub fn f(&self, key: &str) -> f32 {
        Self::finite(self.get(key).map_or(0.0, ParamValue::as_f32))
    }
    pub fn i(&self, key: &str) -> i32 {
        match self.get(key) {
            Some(ParamValue::Int(v)) => *v,
            Some(other) => other.as_f32() as i32,
            None => 0,
        }
    }
    pub fn u(&self, key: &str) -> u32 {
        match self.get(key) {
            Some(ParamValue::Enum(v)) => *v,
            Some(ParamValue::Bool(b)) => u32::from(*b),
            Some(other) => other.as_f32().max(0.0) as u32,
            None => 0,
        }
    }
    pub fn b(&self, key: &str) -> bool {
        matches!(self.get(key), Some(ParamValue::Bool(true)))
    }
    pub fn v2(&self, key: &str) -> [f32; 2] {
        match self.get(key) {
            Some(ParamValue::Vec2(v)) => v.map(Self::finite),
            _ => [0.0; 2],
        }
    }
    pub fn color(&self, key: &str) -> [f32; 4] {
        match self.get(key) {
            Some(ParamValue::Color(v)) => v.map(Self::finite),
            _ => [0.0, 0.0, 0.0, 1.0],
        }
    }
    pub fn text(&self, key: &str) -> &str {
        match self.get(key) {
            Some(ParamValue::Text(s)) => s,
            _ => "",
        }
    }
    pub fn tiling(&self) -> u32 {
        self.u("tiling").min(3)
    }
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
    row_offset: f32,
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
    pad0: u32,
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
    pad0: f32,
    pad1: f32,
    scale: i32,
    octaves: i32,
    kind: u32,
    seed: u32,
    tiling: u32,
    invert: u32,
    pad2: u32,
    pad3: u32,
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
    c: [[f32; 4]; 4],
    f: [f32; 8],
    op: u32,
    mode: u32,
    flag: u32,
    tiling: u32,
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

/// Everything a node needs to record its GPU work.
pub struct ExecCtx<'a> {
    pub gpu: &'a Gpu,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub params: &'a EffParams,
    /// Image inputs in definition order (`None` = unconnected).
    pub inputs: &'a [Option<GpuImage>],
    pub output: &'a GpuImage,
    /// Decoded bitmap for Bitmap nodes.
    pub bitmap: Option<&'a GpuImage>,
    /// Pixel Math: resolved variables.
    pub pixel_vars: &'a [PixelVar],
}

/// How a Pixel Math variable gets its value.
#[derive(Clone, Debug)]
pub enum PixelVar {
    Builtin(&'static str),
    Image(GpuImage),
    Constant(f32),
}

fn input_or<'b>(inputs: &'b [Option<GpuImage>], index: usize, fallback: &'b GpuImage) -> &'b GpuImage {
    inputs.get(index).and_then(|i| i.as_ref()).unwrap_or(fallback)
}

/// Record the work of one image node. Returns a warning for the UI, if any.
pub fn execute(kind: NodeKind, cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let tiling = p.tiling();
    let gpu = cx.gpu;
    let inputs = cx.inputs;
    let output = cx.output;
    let bitmap = cx.bitmap;
    let pixel_vars = cx.pixel_vars;
    let encoder: &mut wgpu::CommandEncoder = &mut *cx.encoder;
    let px_scale = output.width() as f32 / 1024.0;
    let mut warning = None;
    let connected = |i: usize| inputs.get(i).is_some_and(|x| x.is_some());
    if !inputs.is_empty() && !connected(0) && !matches!(kind, NodeKind::TileGenerator | NodeKind::Blend | NodeKind::ChannelShuffle) {
        warning = Some("Input is not connected".to_string());
    }

    match kind {
        NodeKind::UniformColor => {
            gpu.run(encoder, "uniform_color", bytes(&ColorU { color: p.color("color") }), &[], output)?;
        }
        NodeKind::Shape => {
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
                tiling,
                invert: u32::from(p.b("invert")),
                pad0: 0,
            };
            gpu.run(encoder, "shape", bytes(&u), &[], output)?;
        }
        NodeKind::TileGenerator => {
            let [sx, sy] = p.v2("size");
            let scale = p.f("scale");
            let random_offset = p.f("random_offset");
            let extent = sx.abs().max(sy.abs()) * scale.abs() * 0.5 * std::f32::consts::SQRT_2 + random_offset * 0.5;
            let reach = ((extent - 0.5).ceil() as i32).clamp(1, 3);
            let pattern = p.u("pattern");
            if pattern == crate::graph::defs::TILE_PATTERN_IMAGE && !connected(0) {
                warning = Some("Pattern is 'Input Image' but no image is connected".into());
            }
            let u = TileU {
                size_x: sx,
                size_y: sy,
                scale,
                row_offset: p.f("row_offset"),
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
                tiling,
                reach,
                pad0: 0,
            };
            let pattern_img = input_or(inputs, 0, &gpu.black);
            gpu.run(encoder, "tile_gen", bytes(&u), &[&pattern_img.view], output)?;
        }
        NodeKind::Gradient => {
            let [cx_, cy] = p.v2("center");
            let u = GradientU {
                angle: p.f("angle"),
                center_x: cx_,
                center_y: cy,
                repeat: p.f("repeat"),
                offset: p.f("offset"),
                kind: p.u("type"),
                invert: u32::from(p.b("invert")),
                tiling,
            };
            gpu.run(encoder, "gradient", bytes(&u), &[], output)?;
        }
        NodeKind::Noise => {
            let u = NoiseU {
                roughness: p.f("roughness").clamp(0.0, 1.0),
                jitter: p.f("jitter").clamp(0.0, 1.0),
                pad0: 0.0,
                pad1: 0.0,
                scale: p.i("scale").clamp(1, 4096),
                octaves: p.i("octaves").clamp(1, 10),
                kind: p.u("type"),
                seed: p.i("seed") as u32,
                tiling,
                invert: u32::from(p.b("invert")),
                pad2: 0,
                pad3: 0,
            };
            gpu.run(encoder, "noise", bytes(&u), &[], output)?;
        }
        NodeKind::Bitmap => match bitmap {
            Some(img) => {
                let u = BlurU { sigma: 0.0, radius: 0, dir_x: 0, dir_y: 0, tiling, pad: [0; 3] };
                gpu.run(encoder, "resample", bytes(&u), &[&img.view], output)?;
            }
            None => {
                gpu.run(encoder, "uniform_color", bytes(&ColorU { color: [0.0; 4] }), &[], output)?;
            }
        },
        NodeKind::Transform2D => {
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
                tiling,
                samples: (1.0 / min_scale).ceil().clamp(1.0, 4.0) as u32,
                pad1: 0,
            };
            let input = input_or(inputs, 0, &gpu.black);
            gpu.run(encoder, "transform", bytes(&u), &[&input.view], output)?;
        }
        NodeKind::Blend => {
            let u = BlendU {
                opacity: p.f("opacity"),
                mode: p.u("mode"),
                ignore_alpha: p.u("alpha"),
                clamp_result: u32::from(p.b("clamp")),
                tiling,
                pad: [0; 3],
            };
            let fg = input_or(inputs, 0, &gpu.black);
            let bg = input_or(inputs, 1, &gpu.black);
            let mask = input_or(inputs, 2, &gpu.white);
            if !connected(0) && !connected(1) {
                warning = Some("Foreground and background are not connected".into());
            }
            gpu.run(encoder, "blend", bytes(&u), &[&fg.view, &bg.view, &mask.view], output)?;
        }
        NodeKind::Blur => {
            let sigma = p.f("radius").max(0.0) * 0.5 * px_scale;
            let input = input_or(inputs, 0, &gpu.black).clone();
            gaussian_blur(gpu, encoder, &input, output, sigma, tiling)?;
        }
        NodeKind::Sharpen => {
            let sigma = (p.f("radius").max(0.05) * 0.5 * px_scale).max(0.35);
            let input = input_or(inputs, 0, &gpu.black).clone();
            let [w, h] = output.size();
            let blurred = gpu.take_scratch(w, h);
            gaussian_blur(gpu, encoder, &input, &blurred, sigma, tiling)?;
            let u = ScalarU { value: p.f("intensity"), a: tiling, b: 0, c: 0 };
            let r = gpu.run(encoder, "sharpen", bytes(&u), &[&input.view, &blurred.view], output);
            gpu.recycle(blurred);
            r?;
        }
        NodeKind::Warp => {
            let u = ScalarU { value: p.f("intensity"), a: tiling, b: 0, c: 0 };
            let input = input_or(inputs, 0, &gpu.black);
            let map = input_or(inputs, 1, &gpu.black);
            if connected(0) && !connected(1) {
                warning = Some("Warp Map is not connected".into());
            }
            gpu.run(encoder, "warp", bytes(&u), &[&input.view, &map.view], output)?;
        }
        NodeKind::Mirror => {
            let u = ScalarU { value: p.f("offset"), a: p.u("mode"), b: tiling, c: 0 };
            let input = input_or(inputs, 0, &gpu.black);
            gpu.run(encoder, "mirror", bytes(&u), &[&input.view], output)?;
        }
        NodeKind::NormalMap => {
            let u = ScalarU { value: p.f("intensity"), a: p.u("format"), b: tiling, c: 0 };
            let input = input_or(inputs, 0, &gpu.black);
            gpu.run(encoder, "normal", bytes(&u), &[&input.view], output)?;
        }
        NodeKind::ChannelShuffle => {
            let u = ShuffleU {
                channels: [p.u("red"), p.u("green"), p.u("blue"), p.u("alpha")],
                tiling,
                pad: [0; 3],
            };
            let a = input_or(inputs, 0, &gpu.black);
            let b = input_or(inputs, 1, &gpu.black);
            gpu.run(encoder, "shuffle", bytes(&u), &[&a.view, &b.view], output)?;
        }
        NodeKind::Levels
        | NodeKind::Invert
        | NodeKind::Grayscale
        | NodeKind::Hsl
        | NodeKind::GradientMap
        | NodeKind::Posterize
        | NodeKind::Output => {
            let mut u = AdjustU { c: [[0.0; 4]; 4], f: [0.0; 8], op: 0, mode: 0, flag: 0, tiling };
            match kind {
                NodeKind::Levels => {
                    u.op = 0;
                    u.f[..5].copy_from_slice(&[
                        p.f("in_low"),
                        p.f("in_high"),
                        p.f("gamma"),
                        p.f("out_low"),
                        p.f("out_high"),
                    ]);
                    u.mode = p.u("channels");
                    u.flag = u32::from(p.b("clamp"));
                }
                NodeKind::Invert => {
                    u.op = 1;
                    u.flag = u32::from(p.b("invert_alpha"));
                }
                NodeKind::Grayscale => {
                    u.op = 2;
                    u.mode = p.u("mode");
                }
                NodeKind::Hsl => {
                    u.op = 3;
                    u.f[0] = p.f("hue") / 360.0;
                    u.f[1] = p.f("saturation");
                    u.f[2] = p.f("lightness");
                }
                NodeKind::GradientMap => {
                    u.op = 4;
                    u.mode = p.u("stops");
                    u.c = [p.color("color_a"), p.color("color_b"), p.color("color_c"), p.color("color_d")];
                    u.f[0] = p.f("pos_b");
                    u.f[1] = p.f("pos_c");
                }
                NodeKind::Posterize => {
                    u.op = 5;
                    u.f[0] = p.i("steps").clamp(2, 256) as f32;
                }
                _ => {
                    u.op = 6;
                    u.mode = p.u("format");
                }
            }
            let input = input_or(inputs, 0, &gpu.black);
            gpu.run(encoder, "adjust", bytes(&u), &[&input.view], output)?;
        }
        NodeKind::PixelMath => {
            warning = pixel_math(gpu, encoder, p, output, pixel_vars)?;
        }
        NodeKind::Input | NodeKind::Math | NodeKind::ComposeVec2 | NodeKind::ComposeColor => {
            return Err("value nodes have no GPU work".into());
        }
    }
    Ok(warning)
}

/// Separable Gaussian blur. Large radii are blurred on a downsampled copy and
/// upsampled again, which keeps the cost bounded for any radius.
pub fn gaussian_blur(
    gpu: &Gpu,
    encoder: &mut wgpu::CommandEncoder,
    input: &GpuImage,
    output: &GpuImage,
    sigma: f32,
    tiling: u32,
) -> Result<(), String> {
    let [w, h] = output.size();
    let copy = BlurU { sigma: 0.0, radius: 0, dir_x: 0, dir_y: 0, tiling, pad: [0; 3] };
    if sigma < 0.2 || input.size() != output.size() {
        // Nothing to blur (or a 1x1 placeholder input): plain resample.
        return gpu.run(encoder, "resample", bytes(&copy), &[&input.view], output);
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
        let next = gpu.take_scratch(cw, ch);
        gpu.run(encoder, "downsample", bytes(&copy), &[&current.view], &next)?;
        current = next.clone();
        chain.push(next);
    }
    let [cw, ch] = current.size();
    let radius = (s * 3.0).ceil() as i32;
    let horizontal = BlurU { sigma: s, radius, dir_x: 1, dir_y: 0, tiling, pad: [0; 3] };
    let vertical = BlurU { dir_x: 0, dir_y: 1, ..horizontal };
    let a = gpu.take_scratch(cw, ch);
    gpu.run(encoder, "blur_1d", bytes(&horizontal), &[&current.view], &a)?;
    if levels == 0 {
        gpu.run(encoder, "blur_1d", bytes(&vertical), &[&a.view], output)?;
    } else {
        let b = gpu.take_scratch(cw, ch);
        gpu.run(encoder, "blur_1d", bytes(&vertical), &[&a.view], &b)?;
        gpu.run(encoder, "resample", bytes(&copy), &[&b.view], output)?;
        gpu.recycle(b);
    }
    gpu.recycle(a);
    for img in chain {
        gpu.recycle(img);
    }
    let _ = (w, h);
    Ok(())
}

pub const MAX_PIXEL_IMAGES: usize = 8;
pub const MAX_PIXEL_CONSTANTS: usize = 16;

/// Pixel Math: compile the expression to WGSL and run it for every pixel.
fn pixel_math(
    gpu: &Gpu,
    encoder: &mut wgpu::CommandEncoder,
    params: &EffParams,
    output: &GpuImage,
    pixel_vars: &[PixelVar],
) -> Result<Option<String>, String> {
    let src = params.text("expression");
    let compiled = expr::compile_cached(src);
    let program = match &*compiled {
        Ok(p) => p,
        Err(e) => return Err(format!("Invalid expression: {}", e.message)),
    };
    let mut images: Vec<GpuImage> = Vec::new();
    let mut consts: Vec<f32> = Vec::new();
    let mut exprs: Vec<String> = Vec::new();
    for (i, _name) in program.vars.iter().enumerate() {
        let var = pixel_vars.get(i).cloned().unwrap_or(PixelVar::Constant(0.0));
        let e = match var {
            PixelVar::Builtin(b) => match b {
                "u" => "uv.x".to_string(),
                "v" => "uv.y".to_string(),
                "px" => "f32(gid.x)".to_string(),
                "py" => "f32(gid.y)".to_string(),
                "width" => "f32(size.x)".to_string(),
                _ => "f32(size.y)".to_string(),
            },
            PixelVar::Image(_) => {
                if images.len() >= MAX_PIXEL_IMAGES {
                    return Err(format!("Pixel Math supports at most {MAX_PIXEL_IMAGES} image inputs"));
                }
                let slot = images.len();
                if let PixelVar::Image(img) = &pixel_vars[i] {
                    images.push(img.clone());
                }
                format!("img{slot}")
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
        "@group(0) @binding({}) var out_tex: texture_storage_2d<rgba16float, write>;\n",
        images.len() + 1
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
    for i in 0..images.len() {
        source.push_str(&format!("    let img{i} = chan(read_input(t{i}, gid.xy, size, P.tiling));\n"));
    }
    source.push_str(&format!(
        "    let r = {body};\n    textureStore(out_tex, vec2<i32>(gid.xy), vec4<f32>(r, r, r, 1.0));\n}}\n"
    ));

    let kernel = gpu.dynamic_kernel(&source, images.len())?;
    let mut u = PixelMathU { consts: [[0.0; 4]; 4], tiling: params.tiling(), channel: params.u("channel"), pad: [0; 2] };
    for (i, v) in consts.iter().enumerate() {
        u.consts[i / 4][i % 4] = *v;
    }
    let views: Vec<&wgpu::TextureView> = images.iter().map(|i| &i.view).collect();
    gpu.dispatch(encoder, &kernel, bytes(&u), &views, &output.view, output.size());
    Ok(None)
}
