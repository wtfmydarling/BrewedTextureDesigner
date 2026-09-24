// How far each pixel is from the outside of its shape along its row and column: to the left,
// the right, up and down (x, y, z, w), by doubling. A pass of step s looks s away in each
// direction: what the pixel there knows, plus s, if that is nearer. After passes of 1, 2, 4…
// as far as the image, each pixel knows the ends of its runs. Where the image tiles, runs
// go across its edges (a row all of the shape knows no end: 65535); where it does not, the
// edge ends them.

struct Params {
    tiling: u32, seed: u32, mode: u32, step: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var runs: texture_2d<u32>;

const NONE: u32 = 65535u;

fn runs_at(p: vec2<i32>) -> vec4<u32> {
    let d = vec2<i32>(textureDimensions(runs));
    var q = p;
    if (tile_x(P.tiling)) { q.x = wrap_i(p.x, d.x); } else if (p.x < 0 || p.x >= d.x) { return vec4<u32>(0u); }
    if (tile_y(P.tiling)) { q.y = wrap_i(p.y, d.y); } else if (p.y < 0 || p.y >= d.y) { return vec4<u32>(0u); }
    return textureLoad(runs, q, 0);
}

// `known` plus `s`, if it is known.
fn beyond(known: u32, s: u32) -> u32 {
    return select(min(known + s, NONE), NONE, known == NONE);
}

@fragment
fn step(@builtin(position) at: vec4<f32>) -> @location(0) vec4<u32> {
    let p = vec2<i32>(pixel(at).xy);
    let s = i32(P.step);
    var r = runs_at(p);
    r.x = min(r.x, beyond(runs_at(p - vec2<i32>(s, 0)).x, P.step));
    r.y = min(r.y, beyond(runs_at(p + vec2<i32>(s, 0)).y, P.step));
    r.z = min(r.z, beyond(runs_at(p - vec2<i32>(0, s)).z, P.step));
    r.w = min(r.w, beyond(runs_at(p + vec2<i32>(0, s)).w, P.step));
    return r;
}
