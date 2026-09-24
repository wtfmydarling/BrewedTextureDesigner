// Directional (motion) blur. Distances are in uv units, so the look does not depend
// on the resolution.

struct Params {
    dir_x: f32, dir_y: f32, pad0: f32, pad1: f32,
    samples: i32, symmetric: u32, tiling: u32, pad2: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;

// Average of `samples` taps along dir (centered on the pixel when symmetric, trailing
// behind it otherwise).
@fragment
fn main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    let uv = out_uv(gid.xy, size);
    let dir = vec2<f32>(P.dir_x, P.dir_y);
    let n = max(P.samples, 1);
    let start = select(-1.0, -0.5, P.symmetric != 0u);
    var acc = vec4<f32>(0.0);
    for (var i = 0; i < n; i++) {
        let t = start + (f32(i) + 0.5) / f32(n);
        acc = acc + widen(sample_bilinear(in0, uv + dir * t, P.tiling), GRAY0);
    }
    return acc / f32(n);
}
