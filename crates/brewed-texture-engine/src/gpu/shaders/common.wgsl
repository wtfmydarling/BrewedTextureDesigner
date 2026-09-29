// ---------------------------------------------------------------------------
// Shared helpers, prepended to every kernel.
//
// Conventions:
//   * a color image is rgba16float, a grayscale one r32float (one channel; r16float in a
//     graph of half precision); uv in [0, 1] with texel centers at (i + 0.5) / size and y
//     pointing down (row 0 is the top);
//   * `tiling` is a bit set: bit 0 = wrap horizontally, bit 1 = wrap vertically;
//   * angles given by the user are degrees, counter-clockwise on screen.
// ---------------------------------------------------------------------------

const PI: f32 = 3.14159265358979;
const TAU: f32 = 6.28318530717959;

// Kernels see every image as a color image: they read each input through `widen` (see
// show.wgsl, which comes before this), which spreads the one channel of a grayscale image
// over red, green and blue, with an opaque alpha. `gray_input(n)` says input n (binding n + 1)
// is grayscale: which are is said with the size of the image drawn (see `TARGET`), not built
// into the pipeline, so that one pipeline draws from grayscale images and color ones alike
// (DX12 takes a while to build one). A kernel writes the same way whatever it writes to: a
// grayscale image keeps the red channel.
fn gray_input(n: u32) -> bool {
    return (TARGET.z & (1u << n)) != 0u;
}

// The kind of image a kernel that makes several kinds (the types of noise) is built for.
// Each kind is a pipeline of its own, with only the code of its kind: the compiler drops the
// rest, and one kind builds in a fraction of the time all of them together would take.
override KIND: u32 = 0u;

// A kernel draws its image: one triangle covers it, and the kernel's fragment entry computes
// each of its pixels on its own and returns it. A render pass writes any format a device
// renders to, where a compute shader could store to only a few: a grayscale image is
// written as it is kept, one channel.

// The size of the image being drawn, and which inputs are grayscale, as bits (in the buffer of
// the parameters, after them).
@group(0) @binding(15) var<uniform> TARGET: vec4<u32>;

@vertex
fn cover(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

// The pixel a fragment is computed for, from the top left.
fn pixel(at: vec4<f32>) -> vec3<u32> {
    return vec3<u32>(vec2<u32>(at.xy), 0u);
}

fn target_size() -> vec2<u32> {
    return TARGET.xy;
}

fn tile_x(t: u32) -> bool { return (t & 1u) != 0u; }
fn tile_y(t: u32) -> bool { return (t & 2u) != 0u; }

// `i` modulo `n`, from 0 to n - 1 whatever the sign of `i`. The remainder is only ever taken
// of a number of zero or more: of a negative one, NVIDIA's Vulkan driver (616.92 still) gets
// it wrong in some kernels (the corners of simplex noise left of the image), where DX12,
// Mesa and the WGSL spec agree on it. And it is taken of unsigned numbers: DX12's shaders have
// no division of signed ones, which FXC makes of a dozen instructions or more each time, and
// every read of an image that wraps has a few of them (Tile Sampler took two seconds to
// compile on DX12).
fn wrap_i(i: i32, n: i32) -> i32 {
    let m = u32(max(n, 1));
    if (i >= 0) { return i32(u32(i) % m); }
    // -1 is n - 1, -n is 0: counted down from n - 1 by -(i + 1), which is 0 or more.
    return i32(m - 1u - u32(-(i + 1)) % m);
}

// Column or row `i` of an image `n` pixels across: wrapped where the image tiles, else clamped
// to its edge.
fn edge(i: i32, n: i32, tiles: bool) -> i32 {
    return select(clamp(i, 0, n - 1), wrap_i(i, n), tiles);
}

// Texel fetch from the mip level `level` that wraps on tiled axes and clamps to the edge
// otherwise.
fn fetch_level(tex: texture_2d<f32>, p: vec2<i32>, level: i32, tiling: u32) -> vec4<f32> {
    let d = vec2<i32>(textureDimensions(tex, level));
    return textureLoad(tex, vec2<i32>(edge(p.x, d.x, tile_x(tiling)), edge(p.y, d.y, tile_y(tiling))), level);
}

fn fetch(tex: texture_2d<f32>, p: vec2<i32>, tiling: u32) -> vec4<f32> {
    return fetch_level(tex, p, 0, tiling);
}

// The mip level `level` read between its pixels, as a linear filter reads it, but in the
// precision the image has: floats of 32 bits cannot be filtered on every device, and a filter
// weighs the pixels in steps of 1/256 on most. (Its size asked once, its two columns and two
// rows wrapped or clamped once: this is in many shaders, some many times.)
fn bilinear_level(tex: texture_2d<f32>, uv: vec2<f32>, level: i32, tiling: u32) -> vec4<f32> {
    let d = vec2<i32>(textureDimensions(tex, level));
    let st = uv * vec2<f32>(d) - 0.5;
    let fl = floor(st);
    let i = vec2<i32>(fl);
    let f = st - fl;
    let x0 = edge(i.x, d.x, tile_x(tiling));
    let x1 = edge(i.x + 1, d.x, tile_x(tiling));
    let y0 = edge(i.y, d.y, tile_y(tiling));
    let y1 = edge(i.y + 1, d.y, tile_y(tiling));
    let a = textureLoad(tex, vec2<i32>(x0, y0), level);
    let b = textureLoad(tex, vec2<i32>(x1, y0), level);
    let c = textureLoad(tex, vec2<i32>(x0, y1), level);
    let e = textureLoad(tex, vec2<i32>(x1, y1), level);
    return mix(mix(a, b, f.x), mix(c, e, f.x), f.y);
}

fn sample_bilinear(tex: texture_2d<f32>, uv: vec2<f32>, tiling: u32) -> vec4<f32> {
    return bilinear_level(tex, uv, 0, tiling);
}

// The image at `uv` from its mips, between the two levels around `lod`: what a trilinear
// sampler reads, in the precision the image has (see `bilinear_level`).
fn trilinear(tex: texture_2d<f32>, uv: vec2<f32>, lod: f32, tiling: u32) -> vec4<f32> {
    let l = clamp(lod, 0.0, f32(textureNumLevels(tex) - 1u));
    let l0 = floor(l);
    let lower = bilinear_level(tex, uv, i32(l0), tiling);
    if (l == l0) {
        return lower;
    }
    return mix(lower, bilinear_level(tex, uv, i32(l0) + 1, tiling), l - l0);
}

fn sample_nearest(tex: texture_2d<f32>, uv: vec2<f32>, tiling: u32) -> vec4<f32> {
    let d = vec2<f32>(textureDimensions(tex));
    return fetch(tex, vec2<i32>(floor(uv * d)), tiling);
}

// Read an input at the output pixel `gid`, resampling when resolutions differ
// (1x1 placeholder textures for unconnected inputs, bitmaps, ...).
fn read_input(tex: texture_2d<f32>, gid: vec2<u32>, out_size: vec2<u32>, tiling: u32) -> vec4<f32> {
    let d = textureDimensions(tex);
    if (d.x == out_size.x && d.y == out_size.y) {
        return textureLoad(tex, vec2<i32>(gid), 0);
    }
    let uv = (vec2<f32>(gid) + 0.5) / vec2<f32>(out_size);
    return sample_bilinear(tex, uv, tiling);
}

fn out_uv(gid: vec2<u32>, size: vec2<u32>) -> vec2<f32> {
    return (vec2<f32>(gid) + 0.5) / vec2<f32>(size);
}

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn gray(v: f32) -> vec4<f32> {
    return vec4<f32>(v, v, v, 1.0);
}

// Hermite step that also accepts e0 > e1.
fn sstep(e0: f32, e1: f32, x: f32) -> f32 {
    if (e0 == e1) { return select(1.0, 0.0, x < e0); }
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// Rotate a vector counter-clockwise *on screen* (y down) by `deg` degrees.
fn rotate_ccw(p: vec2<f32>, deg: f32) -> vec2<f32> {
    let a = radians(deg);
    let c = cos(a);
    let s = sin(a);
    return vec2<f32>(c * p.x + s * p.y, -s * p.x + c * p.y);
}

// ------------------------------------------------------------------ hashing
fn pcg(v: u32) -> u32 {
    let state = v * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

fn hash3(x: i32, y: i32, seed: u32) -> u32 {
    return pcg(u32(x) ^ pcg(u32(y) ^ pcg(seed)));
}

fn rand01(h: u32) -> f32 {
    return f32(h >> 8u) / 16777216.0;
}

// ------------------------------------------------------------------ shapes
// Signed distances in "cell units" (negative inside).
fn sd_box(p: vec2<f32>, h: vec2<f32>) -> f32 {
    let d = abs(p) - h;
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0);
}

fn sd_round_box(p: vec2<f32>, h: vec2<f32>, r: f32) -> f32 {
    let rr = min(r, min(h.x, h.y));
    return sd_box(p, h - vec2<f32>(rr)) - rr;
}

// Regular polygon with circumradius 1 (normalized space).
fn sd_polygon(p: vec2<f32>, n: f32) -> f32 {
    let an = PI / n;
    // Angle measured from "up"; shifted by half a sector so a vertex points up.
    let q = vec2<f32>(p.x, -p.y);
    var a = atan2(q.x, q.y) + an;
    a = a - 2.0 * an * floor((a + an) / (2.0 * an));
    let r = length(q);
    return r * cos(a) - cos(an);
}

// Star with n points, outer radius 1 and inner radius `m` (normalized space).
fn sd_star(p: vec2<f32>, n: f32, m: f32) -> f32 {
    let an = PI / n;
    let q = vec2<f32>(p.x, -p.y);
    var a = atan2(q.x, q.y);
    a = a - 2.0 * an * floor(a / (2.0 * an));
    // Fold into the first half-sector.
    a = min(a, 2.0 * an - a);
    let r = length(q);
    let pp = vec2<f32>(cos(a), sin(a)) * r;
    let a0 = vec2<f32>(1.0, 0.0);
    let b0 = vec2<f32>(cos(an), sin(an)) * m;
    let e = b0 - a0;
    let w = pp - a0;
    let t = clamp(dot(w, e) / dot(e, e), 0.0, 1.0);
    let d = length(w - e * t);
    // Inside test: which side of the edge line are we on?
    let side = e.x * w.y - e.y * w.x;
    return select(d, -d, side > 0.0);
}

struct ShapeParams {
    kind: u32,
    half: vec2<f32>,     // half extents in cell units
    softness: f32,       // fraction of the smallest half extent
    aa: f32,             // anti-aliasing width in cell units
    sides: f32,
    inner: f32,
    thickness: f32,
    corner: f32,
};

// Coverage (solid shapes) or height (profiles) in [0, 1] at local point q.
fn shape_value(sp: ShapeParams, q: vec2<f32>) -> f32 {
    let h = max(sp.half, vec2<f32>(1e-5));
    let hm = min(h.x, h.y);
    let n = q / h;               // normalized: shape spans [-1, 1]
    let r = length(n);
    var d = 0.0;
    switch sp.kind {
        case 0u: { d = (r - 1.0) * hm; }                                    // disc / ellipse
        case 1u: { d = sd_box(q, h); }                                      // square
        case 2u: { d = sd_round_box(q, h, sp.corner * hm); }                // rounded square
        case 3u: { d = sd_polygon(n, max(sp.sides, 3.0)) * hm; }            // polygon
        case 4u: { d = sd_star(n, max(sp.sides, 3.0), clamp(sp.inner, 0.01, 1.0)) * hm; }
        case 5u: {                                                          // ring
            let t = clamp(sp.thickness, 0.001, 1.0);
            d = (abs(r - (1.0 - t * 0.5)) - t * 0.5) * hm;
        }
        case 6u: {                                                          // cross
            let t = clamp(sp.thickness, 0.001, 1.0);
            d = min(sd_box(q, vec2<f32>(h.x, h.y * t)), sd_box(q, vec2<f32>(h.x * t, h.y)));
        }
        case 7u: {                                                          // capsule
            if (h.x >= h.y) {
                let c = vec2<f32>(max(abs(q.x) - (h.x - h.y), 0.0), q.y);
                d = length(c) - h.y;
            } else {
                let c = vec2<f32>(q.x, max(abs(q.y) - (h.y - h.x), 0.0));
                d = length(c) - h.x;
            }
        }
        case 8u: { return max(0.0, 1.0 - r * r); }                          // paraboloid
        case 9u: { return max(0.0, 1.0 - r); }                              // cone
        case 10u: { return max(0.0, 1.0 - max(abs(n.x), abs(n.y))); }      // pyramid
        case 11u: { return select(0.0, 0.5 + 0.5 * cos(PI * r), r < 1.0); } // bell
        case 12u: { return exp(-r * r * 4.0) * select(0.0, 1.0, r < 1.5); } // gaussian
        case 13u: { return 0.5 + 0.5 * cos(PI * n.x); }                     // waves
        default: { d = (r - 1.0) * hm; }
    }
    let soft = sp.softness * hm;
    return 1.0 - smoothstep(-soft - sp.aa, sp.aa, d);
}
