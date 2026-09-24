struct Params {
    axis: f32, mode: u32, tiling: u32, pad0: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;

// Reflect pixel coordinate `x` around the axis `a` (in pixels) when it lies on the
// side that gets replaced.
fn reflect_coord(x: i32, a: i32, keep_low: bool) -> i32 {
    if (keep_low && x >= a) { return 2 * a - 1 - x; }
    if (!keep_low && x < a) { return 2 * a - 1 - x; }
    return x;
}

@fragment
fn main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
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
    return widen(fetch(in0, q, P.tiling), GRAY0);
}
