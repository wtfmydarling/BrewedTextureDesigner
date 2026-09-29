// Curve: each value through a table baked on the CPU from the curve's points (in red), read
// between its entries; on a color image, the channels chosen.

struct Params {
    unused: f32, channels: u32, tiling: u32, pad: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;
@group(0) @binding(2) var table: texture_2d<f32>;

// The curve at `v` (the table holds it from 0 to 1, both in).
fn through(v: f32) -> f32 {
    let n = i32(textureDimensions(table).x);
    let x = clamp(v, 0.0, 1.0) * f32(n - 1);
    let i0 = i32(floor(x));
    let i1 = min(i0 + 1, n - 1);
    return mix(textureLoad(table, vec2<i32>(i0, 0), 0).r, textureLoad(table, vec2<i32>(i1, 0), 0).r, x - f32(i0));
}

@fragment
fn main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let c = widen(read_input(in0, gid.xy, target_size(), P.tiling), gray_input(0u));
    var o = c;
    switch P.channels {
        case 1u: { o.r = through(c.r); }
        case 2u: { o.g = through(c.g); }
        case 3u: { o.b = through(c.b); }
        case 4u: { o.a = through(c.a); }
        default: { o = vec4<f32>(through(c.r), through(c.g), through(c.b), c.a); }
    }
    return o;
}
