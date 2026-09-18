// Nodes that read their input as a height field: ambient occlusion, curvature and edge
// detection. Radii are in uv units.

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

const AO_DIRECTIONS: i32 = 10;
const AO_STEPS: i32 = 8;
// Elevation (as a sine) below which terrain does not occlude: keeps gentle bumps clean.
const AO_BIAS: f32 = 0.1;

// Horizon-based occlusion: in each direction find the highest elevation angle of the
// terrain within the radius; its sine (minus a small bias, fading with distance) is the
// occluded fraction of that direction.
@compute @workgroup_size(8, 8, 1)
fn ambient_occlusion(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }
    let uv = out_uv(gid.xy, size);
    let h0 = h(uv) * P.depth;
    let aspect = f32(size.x) / f32(size.y);
    var occlusion = 0.0;
    for (var d = 0; d < AO_DIRECTIONS; d++) {
        let a = (f32(d) + 0.5) / f32(AO_DIRECTIONS) * TAU;
        let dir = vec2<f32>(cos(a), sin(a));
        var best = 0.0;
        for (var s = 1; s <= AO_STEPS; s++) {
            // Denser samples near the pixel.
            let t = f32(s * s) / f32(AO_STEPS * AO_STEPS);
            let offset = dir * vec2<f32>(P.radius_x, P.radius_y) * t;
            // Horizontal distance in the same unit as the depth (image widths).
            let dist = length(vec2<f32>(offset.x, offset.y / aspect)) + 1e-6;
            let slope = (h(uv + offset) * P.depth - h0) / dist;
            let elevation = slope / sqrt(1.0 + slope * slope);
            best = max(best, (elevation - AO_BIAS) * (1.0 - t * t));
        }
        occlusion = occlusion + best / (1.0 - AO_BIAS);
    }
    let ao = clamp(1.0 - occlusion / f32(AO_DIRECTIONS) * P.intensity, 0.0, 1.0);
    textureStore(out_tex, vec2<i32>(gid.xy), gray(ao));
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
