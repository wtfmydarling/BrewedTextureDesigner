// Nodes that read their input as a height field: curvature and edge detection (ambient
// occlusion has a file of its own, ao.wgsl). Radii are in uv units.

struct Params {
    radius_x: f32, radius_y: f32, intensity: f32, depth: f32,
    invert: u32, tiling: u32, pad0: u32, pad1: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;
@group(0) @binding(2) var out_tex: texture_storage_2d<rgba16float, write>;

fn h(uv: vec2<f32>) -> f32 {
    return luminance(sample_bilinear(in0, uv, P.tiling).rgb);
}

// Negative Laplacian at the given scale: ridges bright, valleys dark, flat = 0.5.
@compute @workgroup_size(8, 8, 1)
fn curvature(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }
    let uv = out_uv(gid.xy, size);
    let r = vec2<f32>(P.radius_x, P.radius_y);
    let c = h(uv);
    let sum = h(uv + vec2<f32>(r.x, 0.0)) + h(uv - vec2<f32>(r.x, 0.0))
            + h(uv + vec2<f32>(0.0, r.y)) + h(uv - vec2<f32>(0.0, r.y));
    let diagonal = h(uv + r) + h(uv - r) + h(uv + vec2<f32>(r.x, -r.y)) + h(uv + vec2<f32>(-r.x, r.y));
    let lap = (sum + 0.5 * diagonal) / 6.0 - c;
    textureStore(out_tex, vec2<i32>(gid.xy), gray(clamp(0.5 - lap * P.intensity, 0.0, 1.0)));
}

// Sobel gradient magnitude.
@compute @workgroup_size(8, 8, 1)
fn edge_detect(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }
    let uv = out_uv(gid.xy, size);
    let rx = vec2<f32>(P.radius_x, 0.0);
    let ry = vec2<f32>(0.0, P.radius_y);
    let tl = h(uv - rx - ry);
    let t = h(uv - ry);
    let tr = h(uv + rx - ry);
    let l = h(uv - rx);
    let r = h(uv + rx);
    let bl = h(uv - rx + ry);
    let b = h(uv + ry);
    let br = h(uv + rx + ry);
    let gx = (tr + 2.0 * r + br) - (tl + 2.0 * l + bl);
    let gy = (bl + 2.0 * b + br) - (tl + 2.0 * t + tr);
    var e = clamp(length(vec2<f32>(gx, gy)) * 0.25 * P.intensity, 0.0, 1.0);
    if (P.invert != 0u) { e = 1.0 - e; }
    textureStore(out_tex, vec2<i32>(gid.xy), gray(e));
}
