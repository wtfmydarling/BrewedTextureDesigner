// Render passes that write the images compute kernels cannot, in formats the device only
// renders to: grayscale images stored as half floats, and mip chains (of the images the
// views show, and of the height a kernel reads at every scale). One triangle covers the
// target, and each of its pixels reads the source.

struct Params {
    // The format of the output the image is shown as (0 for every other node).
    format: u32,
    pad0: u32,
    // The size of the target.
    size: vec2<u32>,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var src: texture_2d<f32>;

@vertex
fn cover(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

// The source pixel for pixel: a grayscale image, computed in 32 bits, stored in 16.
@fragment
fn store(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(src, vec2<i32>(pos.xy), 0);
}

// The source as its node shows it, resampled to the target's size: an image read back as
// its node shows it, or the top level of a copy a kernel filters.
@fragment
fn show(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let c = widen(read_input(src, vec2<u32>(pos.xy), P.size, 0u), GRAY0);
    return as_output(c, P.format);
}

// Each pixel the average of the four under it on the level above: the next level of a mip
// chain.
@fragment
fn mip(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(pos.xy) * 2;
    return (fetch(src, p, 0u) + fetch(src, p + vec2<i32>(1, 0), 0u) + fetch(src, p + vec2<i32>(0, 1), 0u)
        + fetch(src, p + vec2<i32>(1, 1), 0u)) * 0.25;
}
