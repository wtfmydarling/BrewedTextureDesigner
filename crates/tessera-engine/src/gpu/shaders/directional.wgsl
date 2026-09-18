// Directional (motion) blur. Distances are in uv units, so the look does not depend
// on the resolution.

struct Params {
    dir_x: f32, dir_y: f32, pad0: f32, pad1: f32,
    samples: i32, symmetric: u32, tiling: u32, pad2: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;
@group(0) @binding(2) var out_tex: texture_storage_2d<rgba16float, write>;

// Average of `samples` taps along dir (centered on the pixel when symmetric, trailing
// behind it otherwise).
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }
    let uv = out_uv(gid.xy, size);
    let dir = vec2<f32>(P.dir_x, P.dir_y);
    let n = max(P.samples, 1);
    let start = select(-1.0, -0.5, P.symmetric != 0u);
    var acc = vec4<f32>(0.0);
    for (var i = 0; i < n; i++) {
        let t = start + (f32(i) + 0.5) / f32(n);
        acc = acc + sample_bilinear(in0, uv + dir * t, P.tiling);
    }
    textureStore(out_tex, vec2<i32>(gid.xy), acc / f32(n));
}
