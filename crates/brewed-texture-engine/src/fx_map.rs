//! FX-Map: patterns drawn where and as a program says (see
//! [`brewed_texture_core::nodes::FX_MAP`]).
//!
//! The image is split in four again and again; every cell of each depth drawn draws Count
//! splats, and each splat is a quad the GPU draws, blending it with what is under it, in the
//! order of the splats: depth after depth, cell after cell (row by row), number after number.
//! The program runs in the vertex shader, for each corner of each quad: what a splat is comes
//! of its depth, cell, number and seed alone, so every corner works out the same splat, and no
//! buffer holds them (a million splats would take tens of megabytes of one). A splat reaching
//! over an edge the image tiles on is drawn again on the other side of the image: each splat is
//! drawn as three by three quads (on the axes that tile), and those missing the image are left
//! out before anything of them is drawn.

use brewed_texture_core::expr::{self, script::Script};
use brewed_texture_core::nodes::{FX_MAP_BUILTINS, FX_MAP_OUTPUTS, FX_MAP_RULES, FX_MAP_STARTS, MAX_SPLATS, TILE_PATTERN_IMAGE};

use crate::gpu::{GRAY_FORMAT, GpuImage};
use crate::kernels::{ExecCtx, MAX_PIXEL_CONSTANTS, MAX_PIXEL_IMAGES, PixelVar, connected, input, with_mips};

/// The shape that fades out beyond its size (to one and a half times it).
const GAUSSIAN: u32 = 12;
/// About how many pixels the splats of one draw cover at most, and how many splats one draws:
/// a draw that keeps the device busy for long resets it (see `Gpu::draw`).
const BATCH_PIXELS: u64 = 1 << 24;
const BATCH_SPLATS: u64 = 1 << 16;
/// How many splats of each depth the program is run for on the CPU, to know about how much
/// they cover.
const SAMPLED: u64 = 32;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FxMapU {
    consts: [[f32; 4]; 4],
    count: u32,
    depth_from: u32,
    depth_to: u32,
    seed: u32,
    pattern: u32,
    blend: u32,
    tiling: u32,
    channel: u32,
    softness: f32,
    sides: f32,
    inner: f32,
    thickness: f32,
    corner: f32,
    copies_x: u32,
    copies_y: u32,
    pad: u32,
}

/// The parameters, the patterns and the reading of the program's images, and the splats'
/// quads: the WGSL around the program (`{IMAGES}`: its images' bindings, `{READS}`: what it
/// reads of them, `{PROGRAM}`: it).
const SHADER: &str = r#"
struct Params {
    consts: array<vec4<f32>, 4>,
    count: u32, depth_from: u32, depth_to: u32, seed: u32,
    pattern: u32, blend: u32, tiling: u32, channel: u32,
    softness: f32, sides: f32, inner: f32, thickness: f32,
    corner: f32, copies_x: u32, copies_y: u32, pad0: u32,
};
@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var pattern0: texture_2d<f32>;
@group(0) @binding(2) var pattern1: texture_2d<f32>;
@group(0) @binding(3) var pattern2: texture_2d<f32>;
@group(0) @binding(4) var pattern3: texture_2d<f32>;
{IMAGES}
const IMAGE: u32 = 14u;
const GAUSSIAN: u32 = 12u;
const MIN_BLEND: u32 = 4u;
const MULTIPLY: u32 = 5u;

// What the program reads of a pixel of an image: its Read Channel.
fn chan(c: vec4<f32>) -> f32 {
    switch P.channel {
        case 1u: { return c.r; }
        case 2u: { return c.g; }
        case 3u: { return c.b; }
        case 4u: { return c.a; }
        default: { return luminance(c.rgb); }
    }
}

// Whether `x` is a number: not NaN, not infinite (told by its bits, which a compiler may not
// take away as it may `x != x`).
fn is_number(x: f32) -> bool {
    return (bitcast<u32>(x) & 0x7f800000u) != 0x7f800000u;
}

struct Splat {
    @builtin(position) at: vec4<f32>,
    // Where in the splat, in pixels from its middle, across and down it as it is turned.
    @location(0) q: vec2<f32>,
    // Half its width and height, in pixels.
    @location(1) @interpolate(flat) half: vec2<f32>,
    // Its color (its brightness in it) and its opacity.
    @location(2) @interpolate(flat) color: vec4<f32>,
    // Which pattern input it draws (0 to 3), and the level of its mips to read it at.
    @location(3) @interpolate(flat) image: vec2<f32>,
};

@vertex
fn splat_vertex(@builtin(vertex_index) corner: u32, @builtin(instance_index) instance: u32) -> Splat {
    var out: Splat;
    // Left out: a quad outside the image.
    out.at = vec4<f32>(2.0, 2.0, 0.0, 1.0);
    out.q = vec2<f32>(0.0);
    out.half = vec2<f32>(1.0);
    out.color = vec4<f32>(0.0);
    out.image = vec2<f32>(0.0);
    let copies = P.copies_x * P.copies_y;
    var s = instance / copies;
    let copy = instance % copies;
    // Which depth, cell and number the splat is: the splats of a depth come after those of the
    // depths before it, cell by cell, row by row.
    var depth = P.depth_from;
    loop {
        let at_depth = P.count << (2u * depth);
        if (s < at_depth || depth >= P.depth_to) { break; }
        s = s - at_depth;
        depth = depth + 1u;
    }
    let cells = 1u << depth;
    let cell = s / P.count;
    let i = s - cell * P.count;
    let col = cell & (cells - 1u);
    let row = cell >> depth;
    // Its random numbers: of the seed, its depth, its cell and its number.
    fx_seed = fx_pcg(fx_pcg(fx_pcg(fx_pcg(P.seed) + depth) + cell) + i);
    let size = vec2<f32>(target_size());
    // The middle of its cell.
    let cu = (f32(col) + 0.5) / f32(cells);
    let cv = (f32(row) + 0.5) / f32(cells);
{READS}
{PROGRAM}
    let wh = vec2<f32>(l2_0, l2_1);
    let opacity = clamp(l6_0 * l5_3, 0.0, 1.0);
    if (l7_0 == 0.0 || !(wh.x > 0.0) || !(wh.y > 0.0) || !(opacity > 0.0)) { return out; }
    if (!is_number(l0_0) || !is_number(l1_0) || !is_number(l3_0) || !is_number(wh.x) || !is_number(wh.y)) { return out; }
    // Its middle, in the image: on an axis that tiles, the one in the image (as far from its
    // edge as the splat was).
    var middle = (vec2<f32>(f32(col), f32(row)) + vec2<f32>(l0_0, l1_0)) / f32(cells);
    if (tile_x(P.tiling)) { middle.x = middle.x - floor(middle.x); }
    if (tile_y(P.tiling)) { middle.y = middle.y - floor(middle.y); }
    let half = wh * 0.5 / f32(cells) * size;
    // How far its quad reaches from its middle, in pixels: to the edge of the pattern (a
    // gaussian fades out at one and a half times its size), and a pixel more for its edge.
    let reach = half * select(1.0, 1.5, P.pattern == GAUSSIAN) + vec2<f32>(1.0);
    // Which copy of it this is: one image across or down, on the axes that tile.
    let shift = vec2<f32>(f32(copy % P.copies_x), f32(copy / P.copies_x)) - vec2<f32>(f32(P.copies_x / 2u), f32(P.copies_y / 2u));
    let at = (middle + shift) * size;
    // Left out where the copy misses the image: what the turned quad reaches.
    let turn = radians(l3_0);
    let cs = abs(vec2<f32>(cos(turn), sin(turn)));
    let reached = vec2<f32>(cs.x * reach.x + cs.y * reach.y, cs.y * reach.x + cs.x * reach.y);
    if (at.x + reached.x < 0.0 || at.x - reached.x > size.x || at.y + reached.y < 0.0 || at.y - reached.y > size.y) {
        return out;
    }
    let local = (vec2<f32>(f32(corner & 1u), f32(corner >> 1u)) * 2.0 - 1.0) * reach;
    let p = at + rotate_ccw(local, l3_0);
    out.at = vec4<f32>(p.x / size.x * 2.0 - 1.0, 1.0 - p.y / size.y * 2.0, 0.0, 1.0);
    out.q = local;
    out.half = half;
    out.color = vec4<f32>(vec3<f32>(l5_0, l5_1, l5_2) * l4_0, opacity);
    // The pattern input it draws, and the level of its mips as big as the splat.
    let k = u32(select(0.0, clamp(round(l8_0) - 1.0, 0.0, 3.0), is_number(l8_0)));
    var pixels = vec2<f32>(1.0);
    switch k {
        case 1u: { pixels = vec2<f32>(textureDimensions(pattern1)); }
        case 2u: { pixels = vec2<f32>(textureDimensions(pattern2)); }
        case 3u: { pixels = vec2<f32>(textureDimensions(pattern3)); }
        default: { pixels = vec2<f32>(textureDimensions(pattern0)); }
    }
    let shrunk = pixels / max(2.0 * half, vec2<f32>(1e-3));
    out.image = vec2<f32>(f32(k), log2(max(max(shrunk.x, shrunk.y), 1.0)));
    return out;
}

@fragment
fn splat_fragment(s: Splat) -> @location(0) vec4<f32> {
    var cover = 0.0;
    var rgb = vec3<f32>(1.0);
    if (P.pattern == IMAGE) {
        let n = s.q / s.half;
        if (abs(n.x) > 1.0 || abs(n.y) > 1.0) { discard; }
        let uv = n * 0.5 + 0.5;
        let k = u32(s.image.x);
        var c: vec4<f32>;
        switch k {
            case 1u: { c = trilinear(pattern1, uv, s.image.y, 0u); }
            case 2u: { c = trilinear(pattern2, uv, s.image.y, 0u); }
            case 3u: { c = trilinear(pattern3, uv, s.image.y, 0u); }
            default: { c = trilinear(pattern0, uv, s.image.y, 0u); }
        }
        // A grayscale pattern covers as much as it is bright; a color one, as its alpha says.
        if (gray_input(k)) {
            cover = c.r;
        } else {
            rgb = c.rgb;
            cover = c.a;
        }
    } else {
        var sp: ShapeParams;
        sp.kind = P.pattern;
        sp.half = s.half;
        sp.softness = P.softness;
        sp.aa = 0.75;
        sp.sides = P.sides;
        sp.inner = P.inner;
        sp.thickness = P.thickness;
        sp.corner = P.corner;
        cover = shape_value(sp, s.q);
    }
    let a = clamp(cover, 0.0, 1.0) * s.color.a;
    if (!(a > 0.0)) { discard; }
    let c = rgb * s.color.rgb;
    // Premultiplied by how much it covers (see `splat_blend`); what Min and Multiply leave of
    // what is under it where it covers less: all of it.
    if (P.blend == MIN_BLEND || P.blend == MULTIPLY) {
        return vec4<f32>(mix(vec3<f32>(1.0), c, a), a);
    }
    return vec4<f32>(c * a, a);
}
"#;

/// How many values each local of `script` has in the shader: an output as many as it takes at
/// most, a name of the program's own one.
fn widths(script: &Script) -> Vec<usize> {
    (0..script.locals.len()).map(|l| FX_MAP_OUTPUTS.get(l).map_or(1, |(_, _, most)| *most)).collect()
}

/// The splat of number `s` of an FX-Map drawing `count` splats a cell at the depths from `from`
/// on: its depth, cell and number (as the vertex shader works them out).
fn splat_at(mut s: u64, count: u64, from: u32) -> (u32, u64, u64) {
    let mut depth = from;
    loop {
        let at_depth = count << (2 * depth);
        if s < at_depth {
            break;
        }
        s -= at_depth;
        depth += 1;
    }
    (depth, s / count, s % count)
}

/// The seed of the random numbers of a splat (as the vertex shader works it out).
fn splat_seed(seed: u32, depth: u32, cell: u64, i: u64) -> u32 {
    use brewed_texture_core::expr::builtins::pcg_hash as pcg;
    pcg(pcg(pcg(pcg(seed).wrapping_add(depth)).wrapping_add(cell as u32)).wrapping_add(i as u32))
}

pub fn fx_map(cx: &mut ExecCtx) -> Result<Option<String>, String> {
    let p = cx.params;
    let compiled = expr::script::compile_script_cached(p.text("program"), &FX_MAP_RULES);
    let script = match &*compiled {
        Ok(s) => s,
        Err(e) => return Err(format!("The program is incorrect: {}", e.message)),
    };
    let count = p.i("count").max(1) as u64;
    let depth = p.i("depth").clamp(0, 12) as u32;
    let from = (p.i("from_depth").clamp(0, 12) as u32).min(depth);
    let splats: u64 = (from..=depth).map(|d| count << (2 * d)).sum();
    if splats > MAX_SPLATS {
        return Err(format!("{splats} splats are too many (at most {MAX_SPLATS}): a smaller Count or Depth"));
    }
    let pattern = p.u("pattern");

    // What the program reads: built-ins by their names, the rest as the evaluation found them.
    let mut images: Vec<GpuImage> = Vec::new();
    let mut consts: Vec<f32> = Vec::new();
    let mut exprs: Vec<String> = Vec::new();
    let mut image_of: Vec<Option<usize>> = vec![None; script.vars.len()];
    // What each is on the CPU, to know how much the splats cover (an image taken to be 1).
    let mut cpu: Vec<Option<f64>> = Vec::with_capacity(script.vars.len());
    for (k, name) in script.vars.iter().enumerate() {
        if let Some(b) = FX_MAP_BUILTINS.iter().find(|b| **b == name) {
            exprs.push(
                match *b {
                    "i" => "f32(i)",
                    "count" => "f32(P.count)",
                    "depth" => "f32(depth)",
                    "cells" => "f32(cells)",
                    "col" => "f32(col)",
                    "row" => "f32(row)",
                    "u" => "cu",
                    "v" => "cv",
                    "seed" => "f32(P.seed)",
                    "width" => "size.x",
                    _ => "size.y",
                }
                .to_string(),
            );
            cpu.push(None);
            continue;
        }
        match cx.pixel_vars.get(k).cloned().unwrap_or(PixelVar::Constant(0.0)) {
            PixelVar::Image(img) => {
                if images.len() >= MAX_PIXEL_IMAGES {
                    return Err(format!("An FX-Map reads at most {MAX_PIXEL_IMAGES} images"));
                }
                image_of[k] = Some(images.len());
                exprs.push(format!("img{}", images.len()));
                images.push(img);
                cpu.push(Some(1.0));
            }
            PixelVar::Constant(v) => {
                if consts.len() >= MAX_PIXEL_CONSTANTS {
                    return Err(format!("An FX-Map reads at most {MAX_PIXEL_CONSTANTS} values"));
                }
                const LANES: [&str; 4] = ["x", "y", "z", "w"];
                exprs.push(format!("P.consts[{}].{}", consts.len() / 4, LANES[consts.len() % 4]));
                consts.push(v);
                cpu.push(Some(f64::from(v)));
            }
            // (Only Pixel Math has built-ins of its own.)
            PixelVar::Builtin(_) => {
                exprs.push("0.0".into());
                cpu.push(Some(0.0));
            }
        }
    }

    // The program as WGSL: each local a variable a value, from what it is at the start.
    let widths = widths(script);
    let inputs = script.vars.len();
    let var = |k: usize| if k < inputs { exprs[k].clone() } else { format!("l{}_0", k - inputs) };
    let sample = |k: usize, x: &str, y: &str| match image_of.get(k).copied().flatten() {
        Some(i) => format!("chan(widen(sample_bilinear(t{i}, vec2<f32>({x}, {y}), P.tiling), gray_input({}u)))", 4 + i),
        None => var(k),
    };
    let mut program = String::new();
    for (l, width) in widths.iter().enumerate() {
        let start = FX_MAP_STARTS.get(l).copied().unwrap_or(&[]);
        for j in 0..*width {
            let v = start.get(j).copied().unwrap_or(0.0);
            program.push_str(&format!("    var l{l}_{j}: f32 = {v:?};\n"));
        }
    }
    for (l, values) in expr::wgsl::statements_wgsl(script, &var, &sample) {
        // All of a list worked out before any of it is set (it may read what it sets).
        program.push_str("    {\n");
        for (j, v) in values.iter().enumerate() {
            program.push_str(&format!("        let a{j} = {v};\n"));
        }
        for j in 0..widths[l] {
            // One value for a size is its width and its height; three for a color, an opaque one.
            let value = if j < values.len() {
                format!("a{j}")
            } else if values.len() == 1 {
                "a0".to_string()
            } else {
                "1.0".to_string()
            };
            program.push_str(&format!("        l{l}_{j} = {value};\n"));
        }
        program.push_str("    }\n");
    }
    let mut bindings = String::new();
    let mut reads = String::new();
    for i in 0..images.len() {
        bindings.push_str(&format!("@group(0) @binding({}) var t{i}: texture_2d<f32>;\n", 5 + i));
        // Read at the middle of the cell.
        reads.push_str(&format!("    let img{i} = chan(widen(sample_bilinear(t{i}, vec2<f32>(cu, cv), P.tiling), gray_input({}u)));\n", 4 + i));
    }
    let source = format!(
        "{}{}{}",
        crate::gpu::COMMON_WGSL,
        expr::wgsl::PRELUDE,
        SHADER.replace("{IMAGES}", &bindings).replace("{READS}", &reads).replace("{PROGRAM}", &program)
    );

    // Drawn where it can be blended: into a copy in half floats of an image of floats of 32
    // bits (see `Gpu::blendable`).
    let output = cx.output;
    let direct = cx.gpu.blendable(output.format());
    let target = if direct {
        output.clone()
    } else {
        let format = if output.is_gray() { GRAY_FORMAT } else { crate::gpu::COLOR_FORMAT };
        cx.gpu.take_scratch(output.width(), output.height(), format)
    };
    let blend = p.u("blend");
    let result = draw(cx, script, &source, &target, (count, from, depth), (&images, &consts, &cpu), blend);
    let result = result.and_then(|warning| {
        if !direct {
            cx.gpu.encode_show(cx.encoder, &target, output, 0)?;
        }
        Ok(warning)
    });
    if !direct {
        cx.gpu.recycle(target);
    }
    let warning = result?;
    let unread = script.unread.first().map(|name| {
        let outputs: Vec<&str> = FX_MAP_OUTPUTS.iter().map(|(o, ..)| *o).collect();
        format!("The program sets '{name}' and never reads it (what makes a splat: {})", outputs.join(", "))
    });
    let unconnected = (pattern == TILE_PATTERN_IMAGE && !(1..5).any(|i| connected(cx.inputs, i)))
        .then(|| "The pattern is 'Input Image' but no image is connected".to_string());
    Ok(warning.or(unconnected).or(unread))
}

/// Record the background and the splats into `target`: `images` and `consts` what the program
/// reads, `cpu` what it reads that is not built in, on the CPU.
fn draw(
    cx: &mut ExecCtx,
    script: &Script,
    source: &str,
    target: &GpuImage,
    (count, from, depth): (u64, u32, u32),
    (images, consts, cpu): (&[GpuImage], &[f32], &[Option<f64>]),
    blend: u32,
) -> Result<Option<String>, String> {
    let p = cx.params;
    let pattern = p.u("pattern");
    // What is under the splats: the background, or black (opaque in a color image).
    if connected(cx.inputs, 0) {
        cx.gpu.encode_show(cx.encoder, input(cx.inputs, 0, &cx.gpu.black), target, 0)?;
    } else {
        let black = if target.is_gray() { wgpu::Color::TRANSPARENT } else { wgpu::Color::BLACK };
        cx.gpu.encode_fill(cx.encoder, target, black);
    }
    let kernel = cx.gpu.splat_kernel(source, 4 + images.len(), target.format(), blend)?;

    // The patterns at every scale (see `reads_at_every_scale`).
    let mut copies: Vec<Option<GpuImage>> = Vec::with_capacity(4);
    let mut result = Ok(());
    for i in 1..5 {
        let used = pattern == TILE_PATTERN_IMAGE && connected(cx.inputs, i);
        let img = input(cx.inputs, i, &cx.gpu.black).clone();
        let copy = if used { with_mips(cx, &img) } else { Ok(None) };
        copies.push(copy.unwrap_or_else(|e| {
            result = Err(e);
            None
        }));
    }
    let tiling = p.tiling();
    let (copies_x, copies_y) = (if tiling & 1 != 0 { 3 } else { 1 }, if tiling & 2 != 0 { 3 } else { 1 });
    let mut u = FxMapU {
        consts: [[0.0; 4]; 4],
        count: count as u32,
        depth_from: from,
        depth_to: depth,
        seed: p.i("seed") as u32,
        pattern,
        blend,
        tiling,
        channel: p.u("channel"),
        softness: p.f("softness").max(0.0),
        sides: p.i("sides").clamp(3, 64) as f32,
        inner: p.f("inner_radius"),
        thickness: p.f("thickness"),
        corner: p.f("corner_radius").max(0.0),
        copies_x,
        copies_y,
        pad: 0,
    };
    for (i, v) in consts.iter().enumerate() {
        u.consts[i / 4][i % 4] = *v;
    }
    let batches = batches(script, cpu, (count, from, depth), u.seed, pattern, target.size(), copies_x * copies_y);
    let reads: Vec<&GpuImage> = (1..5)
        .map(|i| copies[i - 1].as_ref().unwrap_or_else(|| input(cx.inputs, i, &cx.gpu.black)))
        .chain(images.iter())
        .collect();
    if result.is_ok() {
        cx.gpu.draw_splats(cx.encoder, &kernel, bytemuck::bytes_of(&u), &reads, target, &batches);
    }
    for copy in copies.into_iter().flatten() {
        cx.gpu.recycle(copy);
    }
    result.map(|()| None)
}

/// The draws of the splats: the instances of each (a splat's copies after each other) and
/// about how many pixels they cover, each at most about [`BATCH_PIXELS`] and
/// [`BATCH_SPLATS`] splats. How much a splat of a depth covers is what the program makes of
/// a few of them, run on the CPU.
fn batches(
    script: &Script,
    cpu: &[Option<f64>],
    (count, from, depth): (u64, u32, u32),
    seed: u32,
    pattern: u32,
    [w, h]: [u32; 2],
    copies: u32,
) -> Vec<(std::ops::Range<u32>, u64)> {
    let image = u64::from(w) * u64::from(h);
    let starts: Vec<Vec<f64>> = FX_MAP_STARTS.iter().map(|s| s.to_vec()).collect();
    let reach = if pattern == GAUSSIAN { 1.5 } else { 1.0 };
    let mut out = Vec::new();
    let mut first = 0u64;
    for d in from..=depth {
        let at_depth = count << (2 * d);
        let cells = 1u64 << d;
        let cell_pixels = image as f64 / (cells * cells) as f64;
        let sampled = at_depth.min(SAMPLED);
        let mut covered = 0.0;
        for k in 0..sampled {
            let s = first + k * at_depth / sampled;
            let (sd, cell, i) = splat_at(s, count, from);
            let (col, row) = (cell % cells, cell / cells);
            let vars: Vec<f64> = script
                .vars
                .iter()
                .zip(cpu)
                .map(|(name, v)| match (name.as_str(), v) {
                    (_, Some(v)) => *v,
                    ("i", _) => i as f64,
                    ("count", _) => count as f64,
                    ("depth", _) => f64::from(sd),
                    ("cells", _) => cells as f64,
                    ("col", _) => col as f64,
                    ("row", _) => row as f64,
                    ("u", _) => (col as f64 + 0.5) / cells as f64,
                    ("v", _) => (row as f64 + 0.5) / cells as f64,
                    ("seed", _) => f64::from(seed),
                    ("width", _) => f64::from(w),
                    _ => f64::from(h),
                })
                .collect();
            let splat = splat_seed(seed, sd, cell, i);
            let values = script.run(&vars, &starts, &|k| expr::script::rand(splat, k));
            let (sw, sh) = (values[2][0].abs(), values[2].get(1).copied().unwrap_or(values[2][0]).abs());
            if values[7][0] == 0.0 || !sw.is_finite() || !sh.is_finite() {
                continue;
            }
            let side = |s: f64, pixels: f64| (s * reach * pixels.sqrt() + 2.0).min(pixels.sqrt() * cells as f64);
            covered += side(sw, cell_pixels) * side(sh, cell_pixels);
        }
        // Of a splat: at most the image (its copies take its place on the other side).
        let per_splat = (covered / sampled.max(1) as f64).clamp(1.0, image as f64);
        let per_batch = ((BATCH_PIXELS as f64 / per_splat) as u64).clamp(1, BATCH_SPLATS);
        let mut s = first;
        while s < first + at_depth {
            let n = per_batch.min(first + at_depth - s);
            let instances = (s * u64::from(copies)) as u32..((s + n) * u64::from(copies)) as u32;
            out.push((instances, (n as f64 * per_splat) as u64));
            s += n;
        }
        first += at_depth;
    }
    out
}
