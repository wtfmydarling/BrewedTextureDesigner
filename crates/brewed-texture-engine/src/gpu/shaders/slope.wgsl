// Filters driven by a second (grayscale) map: directional warp and slope blur.
// Distances are in uv units.

struct Params {
    dir_x: f32, dir_y: f32, amount: f32, pad0: f32,
    samples: i32, mode: u32, tiling: u32, pad1: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;
@group(0) @binding(2) var map_tex: texture_2d<f32>;

fn map_value(uv: vec2<f32>) -> f32 {
    return luminance(widen(sample_bilinear(map_tex, uv, P.tiling), GRAY1).rgb);
}

// Moves every pixel along dir by the brightness of the map.
@fragment
fn directional_warp(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    let uv = out_uv(gid.xy, size);
    let src = uv - vec2<f32>(P.dir_x, P.dir_y) * map_value(uv);
    return widen(sample_bilinear(in0, src, P.tiling), GRAY0);
}

// Follows the slope of the map downhill (uphill for a negative amount) and blurs,
// or takes the minimum / maximum of, the input along that path.
@fragment
fn slope_blur(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    let n = max(P.samples, 1);
    let step_len = P.amount / f32(n);
    let e = 1.0 / vec2<f32>(max(textureDimensions(map_tex), vec2<u32>(2u)));
    var p = out_uv(gid.xy, size);
    let first = widen(sample_bilinear(in0, p, P.tiling), GRAY0);
    var acc = first;
    var lo = first;
    var hi = first;
    for (var i = 0; i < n; i++) {
        let gx = map_value(p + vec2<f32>(e.x, 0.0)) - map_value(p - vec2<f32>(e.x, 0.0));
        let gy = map_value(p + vec2<f32>(0.0, e.y)) - map_value(p - vec2<f32>(0.0, e.y));
        let g = vec2<f32>(gx, gy);
        let len = length(g);
        // Flat areas do not move; slopes move at full speed.
        let dir = select(vec2<f32>(0.0), -g / len, len > 1e-6) * smoothstep(0.0, 0.004, len);
        p = p + dir * step_len;
        let c = widen(sample_bilinear(in0, p, P.tiling), GRAY0);
        acc = acc + c;
        lo = min(lo, c);
        hi = max(hi, c);
    }
    var o = acc / f32(n + 1);
    if (P.mode == 1u) { o = lo; }
    if (P.mode == 2u) { o = hi; }
    return o;
}
