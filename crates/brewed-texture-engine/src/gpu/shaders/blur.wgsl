// Separable Gaussian blur plus the resampling kernels used to build a small
// pyramid for very large radii (blur at low resolution, upsample bilinearly).

struct Params {
    sigma: f32, radius: i32, dir_x: i32, dir_y: i32,
    tiling: u32, pad0: u32, pad1: u32, pad2: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;
@group(0) @binding(2) var out_tex: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn blur_1d(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }
    let p = vec2<i32>(gid.xy);
    let dir = vec2<i32>(P.dir_x, P.dir_y);
    let k = -0.5 / max(P.sigma * P.sigma, 1e-6);
    var acc = vec4<f32>(0.0);
    var wsum = 0.0;
    for (var i = -P.radius; i <= P.radius; i++) {
        let w = exp(f32(i * i) * k);
        acc = acc + widen(fetch(in0, p + dir * i, P.tiling), GRAY0) * w;
        wsum = wsum + w;
    }
    textureStore(out_tex, p, acc / wsum);
}

// 2x2 box reduction (output is half the input size, rounded up).
@compute @workgroup_size(8, 8, 1)
fn downsample(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }
    let p = vec2<i32>(gid.xy) * 2;
    let c = widen(fetch(in0, p, P.tiling), GRAY0) + widen(fetch(in0, p + vec2<i32>(1, 0), P.tiling), GRAY0)
          + widen(fetch(in0, p + vec2<i32>(0, 1), P.tiling), GRAY0) + widen(fetch(in0, p + vec2<i32>(1, 1), P.tiling), GRAY0);
    textureStore(out_tex, vec2<i32>(gid.xy), c * 0.25);
}

// Bilinear resample of the input to the output resolution.
@compute @workgroup_size(8, 8, 1)
fn resample(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }
    let uv = out_uv(gid.xy, size);
    textureStore(out_tex, vec2<i32>(gid.xy), widen(sample_bilinear(in0, uv, P.tiling), GRAY0));
}
