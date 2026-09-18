// Gradient Map: the input value picks a color from a lookup table baked from the
// gradient (any number of stops) on the CPU.

struct Params {
    channel: u32, keep_alpha: u32, tiling: u32, pad0: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;
@group(0) @binding(2) var lut: texture_2d<f32>;
@group(0) @binding(3) var out_tex: texture_storage_2d<rgba16float, write>;

// 0 luminance, 1 red, 2 green, 3 blue, 4 alpha, 5 average
fn pick(c: vec4<f32>) -> f32 {
    switch P.channel {
        case 1u: { return c.r; }
        case 2u: { return c.g; }
        case 3u: { return c.b; }
        case 4u: { return c.a; }
        case 5u: { return (c.r + c.g + c.b) / 3.0; }
        default: { return luminance(c.rgb); }
    }
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }
    let c = read_input(in0, gid.xy, size, P.tiling);
    let n = i32(textureDimensions(lut).x);
    // The table holds n samples from t = 0 to t = 1 (both included).
    let x = clamp(pick(c), 0.0, 1.0) * f32(n - 1);
    let i0 = i32(floor(x));
    let i1 = min(i0 + 1, n - 1);
    let col = mix(textureLoad(lut, vec2<i32>(i0, 0), 0), textureLoad(lut, vec2<i32>(i1, 0), 0), x - f32(i0));
    textureStore(out_tex, vec2<i32>(gid.xy), vec4<f32>(col.rgb, select(col.a, col.a * c.a, P.keep_alpha != 0u)));
}
