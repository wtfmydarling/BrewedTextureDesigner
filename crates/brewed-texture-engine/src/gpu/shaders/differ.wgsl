// Where two images of integers (labels of a flood fill) differ: 1, else 0.

struct Params {
    pad0: u32, pad1: u32, pad2: u32, pad3: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var a: texture_2d<u32>;
@group(0) @binding(2) var b: texture_2d<u32>;

@fragment
fn differ(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(pixel(at).xy);
    let d = select(0.0, 1.0, textureLoad(a, p, 0).r != textureLoad(b, p, 0).r);
    return vec4<f32>(d, d, d, 1.0);
}
