// Tileable noises. Every noise is built on a lattice of `scale` cells across the image; on a
// tiling axis the lattice wraps around, so the noise repeats without a seam.

struct Params {
    roughness: f32, jitter: f32, distortion: f32, threshold: f32,
    scale: i32, octaves: i32, kind: u32, seed: u32,
    tiling: u32, invert: u32, stretch: i32, density: f32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var out_tex: texture_storage_2d<rgba16float, write>;

// The types, in the order of the node's menu (brewed_texture_core::nodes::NOISE_TYPES).
const PERLIN: u32 = 0u;
const VALUE: u32 = 1u;
const BILLOW: u32 = 2u;
const RIDGED: u32 = 3u;
const LIQUID: u32 = 4u;
const STREAKS: u32 = 5u;
const BLOTCHES: u32 = 6u;
const CELLS: u32 = 7u;
const CRYSTAL: u32 = 8u;
const MOSAIC: u32 = 9u;
const CELL_EDGES: u32 = 10u;
const STONES: u32 = 11u;
const SPOTS: u32 = 12u;
const FIBERS: u32 = 13u;
const SCRATCHES: u32 = 14u;

// Wrap lattice coordinates on tiled axes so the noise is periodic over the image.
fn lattice(i: vec2<i32>, period: vec2<i32>) -> vec2<i32> {
    var q = i;
    if (tile_x(P.tiling)) { q.x = wrap_i(i.x, period.x); }
    if (tile_y(P.tiling)) { q.y = wrap_i(i.y, period.y); }
    return q;
}

fn cell_hash(i: vec2<i32>, period: vec2<i32>, seed: u32) -> u32 {
    let q = lattice(i, period);
    return hash3(q.x, q.y, seed);
}

fn grad(h: u32) -> vec2<f32> {
    let a = rand01(h) * TAU;
    return vec2<f32>(cos(a), sin(a));
}

fn fade(t: vec2<f32>) -> vec2<f32> {
    return t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
}

// Gradient noise in roughly [-1, 1].
fn perlin(p: vec2<f32>, period: vec2<i32>, seed: u32) -> f32 {
    let fl = floor(p);
    let i = vec2<i32>(fl);
    let f = p - fl;
    let u = fade(f);
    let n00 = dot(grad(cell_hash(i, period, seed)), f);
    let n10 = dot(grad(cell_hash(i + vec2<i32>(1, 0), period, seed)), f - vec2<f32>(1.0, 0.0));
    let n01 = dot(grad(cell_hash(i + vec2<i32>(0, 1), period, seed)), f - vec2<f32>(0.0, 1.0));
    let n11 = dot(grad(cell_hash(i + vec2<i32>(1, 1), period, seed)), f - vec2<f32>(1.0, 1.0));
    return mix(mix(n00, n10, u.x), mix(n01, n11, u.x), u.y) * 1.41421356;
}

// Value noise in [0, 1].
fn value_noise(p: vec2<f32>, period: vec2<i32>, seed: u32) -> f32 {
    let fl = floor(p);
    let i = vec2<i32>(fl);
    let u = fade(p - fl);
    let a = rand01(cell_hash(i, period, seed));
    let b = rand01(cell_hash(i + vec2<i32>(1, 0), period, seed));
    let c = rand01(cell_hash(i + vec2<i32>(0, 1), period, seed));
    let d = rand01(cell_hash(i + vec2<i32>(1, 1), period, seed));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// What fBm sums up, octave after octave.
const BASIS_GRADIENT: u32 = 0u;
const BASIS_VALUE: u32 = 1u;
const BASIS_BILLOW: u32 = 2u;
const BASIS_RIDGED: u32 = 3u;

// Where octave `o` is shifted to. Doubling the frequency puts every lattice point of an
// octave on one of the next; shifted, the points (where gradient noise is always zero)
// stop lining up into a grid. A shift keeps the period, so the octaves still tile.
fn octave_shift(o: i32) -> vec2<f32> {
    return vec2<f32>(0.3719, 0.6853) * f32(o);
}

// Octaves of a basis noise, each twice as fine as the last and `roughness` times as
// strong, in [0, 1].
fn fbm(p: vec2<f32>, period: vec2<i32>, seed: u32, basis: u32) -> f32 {
    return fbm_n(p, period, seed, basis, P.octaves);
}

fn fbm_n(p: vec2<f32>, period: vec2<i32>, seed: u32, basis: u32, octaves: i32) -> f32 {
    var amp = 1.0;
    var total = 0.0;
    var sum = 0.0;
    var freq = 1;
    for (var o = 0; o < clamp(octaves, 1, 10); o++) {
        let s = seed + u32(o) * 1013u;
        let q = p * f32(freq) + octave_shift(o);
        let per = period * freq;
        var n = 0.0;
        switch basis {
            case BASIS_GRADIENT: { n = perlin(q, per, s) * 0.5 + 0.5; }
            case BASIS_VALUE: { n = value_noise(q, per, s); }
            case BASIS_BILLOW: { n = abs(perlin(q, per, s)); }
            default: {
                let r = 1.0 - abs(perlin(q, per, s));
                n = r * r;
            }
        }
        sum = sum + n * amp;
        total = total + amp;
        amp = amp * P.roughness;
        freq = freq * 2;
    }
    return sum / max(total, 1e-5);
}

// Gradient fBm clusters around 0.5: stretched to use the whole range.
fn clouds(p: vec2<f32>, period: vec2<i32>, seed: u32) -> f32 {
    return clouds_n(p, period, seed, P.octaves);
}

fn clouds_n(p: vec2<f32>, period: vec2<i32>, seed: u32, octaves: i32) -> f32 {
    return (fbm_n(p, period, seed, BASIS_GRADIENT, octaves) - 0.5) * 1.6 + 0.5;
}

struct Voronoi {
    // Distances to the nearest and the second nearest feature point.
    f1: f32,
    f2: f32,
    // The nearest point, and the hash of its cell (the same on both sides of a seam).
    near: vec2<f32>,
    id: u32,
};

fn feature_point(cell: vec2<i32>, h: u32) -> vec2<f32> {
    let r = vec2<f32>(rand01(h), rand01(pcg(h)));
    return vec2<f32>(cell) + mix(vec2<f32>(0.5), r, P.jitter);
}

fn voronoi(p: vec2<f32>, period: vec2<i32>, seed: u32) -> Voronoi {
    let i = vec2<i32>(floor(p));
    var v = Voronoi(10.0, 10.0, p, 0u);
    // 5x5 neighbourhood: with full jitter the second nearest point can be two cells away.
    for (var dy = -2; dy <= 2; dy++) {
        for (var dx = -2; dx <= 2; dx++) {
            let cell = i + vec2<i32>(dx, dy);
            let h = cell_hash(cell, period, seed);
            let pt = feature_point(cell, h);
            let d = length(pt - p);
            if (d < v.f1) {
                v.f2 = v.f1;
                v.f1 = d;
                v.near = pt;
                v.id = h;
            } else if (d < v.f2) {
                v.f2 = d;
            }
        }
    }
    return v;
}

// Distance from `p` to the nearest border of its cell (whose point is `near`): to the
// nearest of the lines half way between that point and the others.
fn cell_border(p: vec2<f32>, near: vec2<f32>, period: vec2<i32>, seed: u32) -> f32 {
    let i = vec2<i32>(floor(near));
    var d = 10.0;
    for (var dy = -2; dy <= 2; dy++) {
        for (var dx = -2; dx <= 2; dx++) {
            let cell = i + vec2<i32>(dx, dy);
            let pt = feature_point(cell, cell_hash(cell, period, seed));
            let across = pt - near;
            let l = length(across);
            if (l < 1e-4) { continue; }
            d = min(d, dot((near + pt) * 0.5 - p, across / l));
        }
    }
    return max(d, 0.0);
}

// Soft round spots of random sizes and strengths, in `density` of the cells.
fn spots(p: vec2<f32>, period: vec2<i32>, seed: u32) -> f32 {
    let i = vec2<i32>(floor(p));
    var v = 0.0;
    // A spot is at most a cell across, so it stays within half a cell of its own.
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let cell = i + vec2<i32>(dx, dy);
            var h = cell_hash(cell, period, seed);
            if (rand01(h) >= P.density) { continue; }
            h = pcg(h);
            let cx = rand01(h);
            h = pcg(h);
            let cy = rand01(h);
            h = pcg(h);
            let radius = mix(0.2, 0.5, rand01(h));
            h = pcg(h);
            let strength = mix(0.4, 1.0, rand01(h));
            let d = length(p - vec2<f32>(cell) - vec2<f32>(cx, cy)) / radius;
            // Bell shaped, and all the way down to nothing at its radius.
            let s = exp(-d * d * 2.5) * (1.0 - smoothstep(0.6, 1.0, d));
            v = v + s * strength * (1.0 - v);
        }
    }
    return v;
}

// Straight strokes in every direction: `count` places for them in every cell, each taken
// with the probability `keep`. A stroke is up to `half` cells from its middle to its ends,
// `width` cells wide in the middle and thinner towards the ends.
fn strokes(p: vec2<f32>, period: vec2<i32>, seed: u32, count: i32, keep: f32, half: f32, width: f32, reach: i32, aa: f32) -> f32 {
    let i = vec2<i32>(floor(p));
    var v = 0.0;
    for (var dy = -reach; dy <= reach; dy++) {
        for (var dx = -reach; dx <= reach; dx++) {
            let cell = i + vec2<i32>(dx, dy);
            var h = cell_hash(cell, period, seed);
            for (var k = 0; k < count; k++) {
                h = pcg(h);
                let taken = rand01(h) < keep;
                h = pcg(h);
                let cx = rand01(h);
                h = pcg(h);
                let cy = rand01(h);
                h = pcg(h);
                let a = rand01(h) * PI;
                h = pcg(h);
                let len = half * mix(0.45, 1.0, rand01(h));
                h = pcg(h);
                let strength = mix(0.35, 1.0, rand01(h));
                if (!taken) { continue; }
                let rel = p - vec2<f32>(cell) - vec2<f32>(cx, cy);
                if (dot(rel, rel) > (len + width + aa) * (len + width + aa)) { continue; }
                let dir = vec2<f32>(cos(a), sin(a));
                let t = clamp(dot(rel, dir), -len, len);
                let d = length(rel - dir * t);
                let along = t / len;
                let w = width * (1.0 - along * along);
                let s = 1.0 - smoothstep(w, w + aa, d);
                v = max(v, s * strength);
            }
        }
    }
    return v;
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }
    let uv = out_uv(gid.xy, size);
    let scale = max(P.scale, 1);
    let base = vec2<i32>(scale, scale);
    let p = uv * f32(scale);
    // One pixel, in cells.
    let aa = f32(scale) / f32(max(size.x, size.y));

    var v = 0.0;
    switch P.kind {
        case PERLIN: {
            v = clouds(p, base, P.seed);
        }
        case VALUE: {
            v = fbm(p, base, P.seed, BASIS_VALUE);
        }
        case BILLOW: {
            v = fbm(p, base, P.seed, BASIS_BILLOW) * 1.8;
        }
        case RIDGED: {
            v = fbm(p, base, P.seed, BASIS_RIDGED);
        }
        case LIQUID: {
            // Read where other noises push the point, themselves read where yet others push
            // it: the noise swirls and folds. The pushes are smoother than the noise, so it
            // flows rather than crumples, and they repeat with the image, so it still tiles.
            let k = P.distortion * 2.5;
            let calm = min(P.octaves, 3);
            let q = vec2<f32>(clouds_n(p, base, P.seed + 7919u, calm), clouds_n(p, base, P.seed + 15887u, calm)) - 0.5;
            let r = vec2<f32>(clouds_n(p + q * k, base, P.seed + 23873u, calm), clouds_n(p + q * k, base, P.seed + 31859u, calm)) - 0.5;
            v = clouds(p + r * k, base, P.seed);
        }
        case STREAKS: {
            // Cells `stretch` times longer than they are tall.
            let period = vec2<i32>(scale, scale * max(P.stretch, 1));
            v = (fbm(uv * vec2<f32>(period), period, P.seed, BASIS_VALUE) - 0.5) * 1.8 + 0.5;
        }
        case BLOTCHES: {
            let n = clouds(p, base, P.seed);
            v = smoothstep(P.threshold - 0.015, P.threshold + 0.015, n);
        }
        case CELLS: {
            v = voronoi(p, base, P.seed).f1 / 1.1;
        }
        case CRYSTAL: {
            // Every cell a flat facet, tilted its own way.
            let c = voronoi(p, base, P.seed);
            let tilt = grad(pcg(c.id ^ 0x5bd1e995u));
            v = mix(0.25, 0.75, rand01(pcg(c.id))) + dot(p - c.near, tilt) * 0.55;
        }
        case MOSAIC: {
            v = rand01(pcg(voronoi(p, base, P.seed).id));
        }
        case CELL_EDGES: {
            let c = voronoi(p, base, P.seed);
            v = (c.f2 - c.f1) * 2.0;
        }
        case STONES: {
            // Every cell a stone of its own height, flat on top and rounded towards the gaps.
            let c = voronoi(p, base, P.seed);
            let t = clamp(cell_border(p, c.near, base, P.seed) / 0.45, 0.0, 1.0);
            v = sqrt(t * (2.0 - t)) * mix(0.45, 1.0, rand01(pcg(c.id)));
        }
        case SPOTS: {
            v = spots(p, base, P.seed);
        }
        case FIBERS: {
            v = strokes(p, base, P.seed, 1 + i32(round(P.density * 11.0)), 1.0, 0.5, 0.018, 1, aa);
        }
        case SCRATCHES: {
            v = strokes(p, base, P.seed, 2, P.density, 1.5, 0.006, 2, aa);
        }
        default: {
            v = rand01(hash3(i32(gid.x), i32(gid.y), P.seed));
        }
    }
    v = clamp(v, 0.0, 1.0);
    if (P.invert != 0u) { v = 1.0 - v; }
    textureStore(out_tex, vec2<i32>(gid.xy), gray(v));
}
