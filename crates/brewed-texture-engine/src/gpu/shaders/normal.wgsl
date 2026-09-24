struct Params {
    intensity: f32, directx: u32, tiling: u32, pad0: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;

fn h(p: vec2<i32>) -> f32 {
    return luminance(widen(fetch(in0, p, P.tiling), GRAY0).rgb);
}

@fragment
fn main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    let in_size = vec2<f32>(textureDimensions(in0));
    let p = vec2<i32>((vec2<f32>(gid.xy) + 0.5) / vec2<f32>(size) * in_size);

    // Sobel filter; sums of 8 pixel differences -> slope per pixel -> slope per uv.
    let tl = h(p + vec2<i32>(-1, -1));
    let t = h(p + vec2<i32>(0, -1));
    let tr = h(p + vec2<i32>(1, -1));
    let l = h(p + vec2<i32>(-1, 0));
    let r = h(p + vec2<i32>(1, 0));
    let bl = h(p + vec2<i32>(-1, 1));
    let b = h(p + vec2<i32>(0, 1));
    let br = h(p + vec2<i32>(1, 1));
    let dhdu = ((tr + 2.0 * r + br) - (tl + 2.0 * l + bl)) / 8.0 * in_size.x;
    let dhdv = ((bl + 2.0 * b + br) - (tl + 2.0 * t + tr)) / 8.0 * in_size.y;

    let k = P.intensity / 64.0;
    var n = normalize(vec3<f32>(-dhdu * k, dhdv * k, 1.0));
    if (P.directx != 0u) { n.y = -n.y; }
    return vec4<f32>(n * 0.5 + 0.5, 1.0);
}
