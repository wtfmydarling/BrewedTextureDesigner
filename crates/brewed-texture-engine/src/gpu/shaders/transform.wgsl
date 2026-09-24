struct Params {
    matte: vec4<f32>,
    offset_x: f32, offset_y: f32, rotation: f32, scale_x: f32,
    scale_y: f32, pivot_x: f32, pivot_y: f32, pad0: f32,
    filtering: u32, tiling: u32, samples: u32, pad1: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;

// Inverse transform: destination uv -> source uv. Rotation happens in a space with
// square pixels so that non-square images rotate without shearing.
fn source_uv(dest: vec2<f32>, aspect: f32) -> vec2<f32> {
    let pivot = vec2<f32>(P.pivot_x, P.pivot_y);
    var d = dest - pivot - vec2<f32>(P.offset_x, P.offset_y);
    d.x = d.x * aspect;
    d = rotate_ccw(d, -P.rotation);
    d.x = d.x / aspect;
    var s = vec2<f32>(P.scale_x, P.scale_y);
    s = select(s, vec2<f32>(1e-4), abs(s) < vec2<f32>(1e-4));
    return pivot + d / s;
}

fn transformed(dest: vec2<f32>, aspect: f32) -> vec4<f32> {
    var suv = source_uv(dest, aspect);
    if (tile_x(P.tiling)) {
        suv.x = fract(suv.x);
    } else if (suv.x < 0.0 || suv.x > 1.0) {
        return P.matte;
    }
    if (tile_y(P.tiling)) {
        suv.y = fract(suv.y);
    } else if (suv.y < 0.0 || suv.y > 1.0) {
        return P.matte;
    }
    if (P.filtering == 1u) {
        return widen(sample_nearest(in0, suv, P.tiling), GRAY0);
    }
    return widen(sample_bilinear(in0, suv, P.tiling), GRAY0);
}

@fragment
fn main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    let fsize = vec2<f32>(size);
    let aspect = fsize.x / fsize.y;
    // Supersample when the image is scaled down to avoid aliasing.
    let n = clamp(P.samples, 1u, 4u);
    var acc = vec4<f32>(0.0);
    for (var sy = 0u; sy < n; sy++) {
        for (var sx = 0u; sx < n; sx++) {
            let sub = (vec2<f32>(f32(sx), f32(sy)) + 0.5) / f32(n);
            acc = acc + transformed((vec2<f32>(gid.xy) + sub) / fsize, aspect);
        }
    }
    return acc / f32(n * n);
}
