// Distance fields by jump flooding: every pixel ends up knowing the nearest of the seeds
// (distance_seed.wgsl), in log2 of the largest distance wanted passes: each pass looks at the
// eight pixels a step away (and itself) and keeps the nearest seed they know, the step halving
// from pass to pass, and one more pass of one (jump flooding + 1). Where the image tiles,
// distances are taken across the edges too. Seeds are kept as integers: a place of 8192 is not
// a float of 32 bits exactly.

struct Params {
    step: i32, tiling: u32, inside: u32, flip: u32,
    threshold: f32, max_distance: f32, profile: u32, pad0: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var seeds0: texture_2d<u32>;
@group(0) @binding(2) var seeds1: texture_2d<u32>;

// The seed a pixel knows, where it wraps on the axes that tile; none outside the image on the
// others.
fn known(tex: texture_2d<u32>, p: vec2<i32>) -> u32 {
    let d = vec2<i32>(textureDimensions(tex));
    var q = p;
    if (tile_x(P.tiling)) { q.x = wrap_i(p.x, d.x); } else if (p.x < 0 || p.x >= d.x) { return 0u; }
    if (tile_y(P.tiling)) { q.y = wrap_i(p.y, d.y); } else if (p.y < 0 || p.y >= d.y) { return 0u; }
    return textureLoad(tex, q, 0).r;
}

fn place(s: u32) -> vec2<f32> {
    let i = s - 1u;
    return vec2<f32>(f32(i & 65535u), f32(i >> 16u));
}

// Squared distance from `p` to the seed `s`: the shorter way round on the axes that tile.
fn distance2(p: vec2<f32>, s: u32) -> f32 {
    let size = vec2<f32>(target_size());
    var d = abs(p - place(s));
    if (tile_x(P.tiling)) { d.x = min(d.x, size.x - d.x); }
    if (tile_y(P.tiling)) { d.y = min(d.y, size.y - d.y); }
    return dot(d, d);
}

@fragment
fn jump(@builtin(position) at: vec4<f32>) -> @location(0) vec4<u32> {
    let p = vec2<i32>(pixel(at).xy);
    let fp = vec2<f32>(p);
    var best = 0u;
    var nearest = 3.0e38;
    for (var j = -1; j <= 1; j++) {
        for (var i = -1; i <= 1; i++) {
            let s = known(seeds0, p + vec2<i32>(i, j) * P.step);
            if (s != 0u) {
                let d = distance2(fp, s);
                if (d < nearest) {
                    nearest = d;
                    best = s;
                }
            }
        }
    }
    return vec4<u32>(best, 0u, 0u, 0u);
}

// The distance from the edge of the shapes, in pixels: from the pixel to its seed, less the
// half pixel between the edge and the seed's center. None: as far as it gets.
fn from_edge(seeds: texture_2d<u32>, p: vec2<i32>) -> f32 {
    let s = known(seeds, p);
    if (s == 0u) {
        return 3.0e38;
    }
    return max(sqrt(distance2(vec2<f32>(p), s)) - 0.5, 0.0);
}

// The height of a bevel `t` of the way from its edge (0) to its top (1).
fn profile(t: f32) -> f32 {
    switch P.profile {
        case 1u: { return smoothstep(0.0, 1.0, t); }
        // A quarter of a circle: steep at the edge, flat on top.
        case 2u: { return sqrt(max(1.0 - (1.0 - t) * (1.0 - t), 0.0)); }
        // Hollowed out: flat at the edge, steep at the top.
        case 3u: { return 1.0 - sqrt(max(1.0 - t * t, 0.0)); }
        default: { return t; }
    }
}

// How far each pixel is from the seeds, over the largest distance wanted (as far as that
// goes), shaped by the profile; `flip` counts from the far end.
@fragment
fn field(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(pixel(at).xy);
    var t = clamp(from_edge(seeds0, p) / max(P.max_distance, 1e-3), 0.0, 1.0);
    if (P.flip == 1u) {
        t = 1.0 - t;
    }
    let v = profile(t);
    return vec4<f32>(v, v, v, 1.0);
}

// Signed: a half at the edge of the shapes, more into them, less away from them, a half of it
// at the largest distance wanted. `seeds0` knows the nearest pixel of the shapes, `seeds1`
// the nearest one around them.
@fragment
fn signed_field(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(pixel(at).xy);
    let away = from_edge(seeds0, p);
    let into = from_edge(seeds1, p);
    // A pixel of the shapes is its own seed of the first kind.
    let d = select(-away, into, known(seeds0, p) != 0u && away == 0.0);
    let v = 0.5 + 0.5 * clamp(d / max(P.max_distance, 1e-3), -1.0, 1.0);
    return vec4<f32>(v, v, v, 1.0);
}
