// Flood fill: every pixel of a shape (a region of the mask connected through its four
// neighbours, across the edges where the image tiles) ends up with the same label, the
// smallest place in it, and each shape gets a value of its own from it.
//
// Labels spread: a pixel takes the smallest label among the pixels around it, as far as its
// distance from the outside of the shape (a distance field of it, distance.wgsl): every pixel
// nearer than that is in the shape too, so a label crosses the wide parts of a shape in a few
// passes; and among the pixels at the ends of its row and column in the shape (runs.wgsl), so
// that a label runs along a narrow straight part in one. And they jump: a pixel takes the
// label of the pixel its label names, which has the smallest label it has seen; pixels point
// to smaller places, so a label that reaches a place goes along all that point there.

struct Params {
    tiling: u32, seed: u32, mode: u32, step: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var labels: texture_2d<u32>;
// The nearest pixel outside the shapes, each pixel's (as distance.wgsl keeps it).
@group(0) @binding(2) var outside: texture_2d<u32>;
// How far each pixel is from the outside along its row and column (as runs.wgsl keeps it).
@group(0) @binding(3) var runs: texture_2d<u32>;

// How far the pixel `p` is from the outside of its shape.
fn room(p: vec2<i32>) -> f32 {
    let s = textureLoad(outside, p, 0).r;
    if (s == 0u) {
        // Nothing is outside: the whole image is the shape.
        return 3.0e38;
    }
    let i = s - 1u;
    let size = vec2<f32>(textureDimensions(outside));
    var d = abs(vec2<f32>(p) - vec2<f32>(f32(i & 65535u), f32(i >> 16u)));
    if (tile_x(P.tiling)) { d.x = min(d.x, size.x - d.x); }
    if (tile_y(P.tiling)) { d.y = min(d.y, size.y - d.y); }
    return length(d);
}

// The label a pixel has, where it wraps on the axes that tile; none outside the image on the
// others.
fn label_at(p: vec2<i32>) -> u32 {
    let d = vec2<i32>(textureDimensions(labels));
    var q = p;
    if (tile_x(P.tiling)) { q.x = wrap_i(p.x, d.x); } else if (p.x < 0 || p.x >= d.x) { return 0u; }
    if (tile_y(P.tiling)) { q.y = wrap_i(p.y, d.y); } else if (p.y < 0 || p.y >= d.y) { return 0u; }
    return textureLoad(labels, q, 0).r;
}

const DIRECTIONS: array<vec2<f32>, 8> = array<vec2<f32>, 8>(
    vec2<f32>(1.0, 0.0), vec2<f32>(-1.0, 0.0), vec2<f32>(0.0, 1.0), vec2<f32>(0.0, -1.0),
    vec2<f32>(0.7071, 0.7071), vec2<f32>(-0.7071, 0.7071), vec2<f32>(0.7071, -0.7071), vec2<f32>(-0.7071, -0.7071),
);

@fragment
fn spread(@builtin(position) at: vec4<f32>) -> @location(0) vec4<u32> {
    let p = vec2<i32>(pixel(at).xy);
    let own = label_at(p);
    if (own == 0u) {
        return vec4<u32>(0u);
    }
    var smallest = own;
    // Its four neighbours, which are its shape's if they are the mask's.
    var around = array<vec2<i32>, 4>(vec2<i32>(1, 0), vec2<i32>(-1, 0), vec2<i32>(0, 1), vec2<i32>(0, -1));
    for (var i = 0; i < 4; i++) {
        let l = label_at(p + around[i]);
        if (l != 0u && l < smallest) {
            smallest = l;
        }
    }
    // The ends of its row and column in the shape (half the image away, along a row or a column
    // that is all shape).
    let size = vec2<i32>(textureDimensions(labels));
    let run = textureLoad(runs, p, 0);
    var ends = array<vec2<i32>, 4>(vec2<i32>(-1, 0), vec2<i32>(1, 0), vec2<i32>(0, -1), vec2<i32>(0, 1));
    for (var i = 0; i < 4; i++) {
        let far = run[i];
        let whole = select(size.y, size.x, i < 2) / 2;
        let reach = select(i32(far) - 1, whole, far == 65535u);
        if (reach >= 2) {
            let l = label_at(p + ends[i] * reach);
            if (l != 0u && l < smallest) {
                smallest = l;
            }
        }
    }
    // And, all around, the pixels as far as it has room, and half as far: all of its shape.
    var directions = DIRECTIONS;
    let reach = min(room(p) - 0.75, 4096.0);
    for (var k = 0; k < 2; k++) {
        let r = reach * select(1.0, 0.5, k == 1);
        if (r < 1.5) {
            break;
        }
        for (var i = 0; i < 8; i++) {
            let l = label_at(p + vec2<i32>(round(directions[i] * r)));
            if (l != 0u && l < smallest) {
                smallest = l;
            }
        }
    }
    return vec4<u32>(smallest, 0u, 0u, 0u);
}

@fragment
fn jump(@builtin(position) at: vec4<f32>) -> @location(0) vec4<u32> {
    let p = vec2<i32>(pixel(at).xy);
    let own = textureLoad(labels, p, 0).r;
    if (own == 0u) {
        return vec4<u32>(0u);
    }
    let w = textureDimensions(labels).x;
    let named = own - 1u;
    let there = textureLoad(labels, vec2<i32>(i32(named % w), i32(named / w)), 0).r;
    return vec4<u32>(select(own, min(own, there), there != 0u), 0u, 0u, 0u);
}

fn hashed(x: u32) -> u32 {
    let state = x * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

fn random(label: u32, channel: u32) -> f32 {
    return f32(hashed(hashed(label ^ (P.seed * 0x9E3779B9u)) + channel) >> 8u) / 16777216.0;
}

// A value for each shape: a gray, or a color; black around the shapes.
@fragment
fn fill(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let own = textureLoad(labels, vec2<i32>(pixel(at).xy), 0).r;
    if (own == 0u) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    if (P.mode == 1u) {
        return vec4<f32>(random(own, 0u), random(own, 1u), random(own, 2u), 1.0);
    }
    let v = random(own, 0u);
    return vec4<f32>(v, v, v, 1.0);
}
