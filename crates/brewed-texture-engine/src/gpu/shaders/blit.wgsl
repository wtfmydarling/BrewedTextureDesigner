// Copies of an image: as its node shows it (an output writes its image this way), resampled
// to the size of the target, and the levels of a mip chain.

struct Params {
    // The format of the output the image is written as (0 for every other copy).
    format: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var src: texture_2d<f32>;

// The source as its node shows it, resampled to the target's size: what an output writes,
// an image read back as its node shows it, or the top level of a copy a kernel filters.
@fragment
fn show(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let c = widen(read_input(src, pixel(at).xy, target_size(), 0u), gray_input(0u));
    return as_output(c, P.format);
}

// Each pixel the average of the four under it on the level above: the next level of a mip
// chain.
@fragment
fn mip(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(pixel(at).xy) * 2;
    return (fetch(src, p, 0u) + fetch(src, p + vec2<i32>(1, 0), 0u) + fetch(src, p + vec2<i32>(0, 1), 0u)
        + fetch(src, p + vec2<i32>(1, 1), 0u)) * 0.25;
}
