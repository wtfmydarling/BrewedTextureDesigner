struct Params {
    red: u32, green: u32, blue: u32, alpha: u32,
    tiling: u32, pad0: u32, pad1: u32, pad2: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var a_tex: texture_2d<f32>;
@group(0) @binding(2) var b_tex: texture_2d<f32>;

// 0..3 = channel of A, 4..7 = channel of B, 8 = zero, 9 = one
fn pick(src: u32, a: vec4<f32>, b: vec4<f32>) -> f32 {
    switch src {
        case 0u: { return a.r; }
        case 1u: { return a.g; }
        case 2u: { return a.b; }
        case 3u: { return a.a; }
        case 4u: { return b.r; }
        case 5u: { return b.g; }
        case 6u: { return b.b; }
        case 7u: { return b.a; }
        case 8u: { return 0.0; }
        default: { return 1.0; }
    }
}

@fragment
fn main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    let a = widen(read_input(a_tex, gid.xy, size, P.tiling), gray_input(0u));
    let b = widen(read_input(b_tex, gid.xy, size, P.tiling), gray_input(1u));
    let o = vec4<f32>(pick(P.red, a, b), pick(P.green, a, b), pick(P.blue, a, b), pick(P.alpha, a, b));
    return o;
}
