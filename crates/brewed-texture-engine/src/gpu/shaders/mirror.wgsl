struct Params {
    axis: f32, mode: u32, tiling: u32, pad0: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;
@group(0) @binding(2) var out_tex: texture_storage_2d<rgba16float, write>;

// Reflect pixel coordinate `x` around the axis `a` (in pixels) when it lies on the
// side that gets replaced.
fn reflect_coord(x: i32, a: i32, keep_low: bool) -> i32 {
    if (keep_low && x >= a) { return 2 * a - 1 - x; }
    if (!keep_low && x < a) { return 2 * a - 1 - x; }
    return x;
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }
    let in_size = textureDimensions(in0);
    // Work in input pixel space (inputs normally share the output resolution).
    let p = vec2<i32>((vec2<f32>(gid.xy) + 0.5) / vec2<f32>(size) * vec2<f32>(in_size));
    let ax = i32(round(P.axis * f32(in_size.x)));
    let ay = i32(round(P.axis * f32(in_size.y)));
    var q = p;
    switch P.mode {
        case 0u: { q.x = reflect_coord(p.x, ax, true); }
        case 1u: { q.x = reflect_coord(p.x, ax, false); }
        case 2u: { q.y = reflect_coord(p.y, ay, true); }
        case 3u: { q.y = reflect_coord(p.y, ay, false); }
        default: {
            q.x = reflect_coord(p.x, ax, true);
            q.y = reflect_coord(p.y, ay, true);
        }
    }
    textureStore(out_tex, vec2<i32>(gid.xy), widen(fetch(in0, q, P.tiling), GRAY0));
}
