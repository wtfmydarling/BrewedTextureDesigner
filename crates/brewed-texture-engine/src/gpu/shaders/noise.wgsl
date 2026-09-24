// Tileable noises. Every noise is built on a lattice of `scale` cells across the image; on a
// tiling axis the lattice wraps around, so the noise repeats without a seam.
//
// Every type is a way of making noise of its own, not a way of looking at another one, and
// draws a pattern of its own from the seed.

struct Params {
    roughness: f32, jitter: f32, distortion: f32, density: f32,
    scale: i32, octaves: i32, seed: u32, tiling: u32,
    invert: u32, stretch: i32, count: i32, length: f32,
    width: f32, angle: f32, spread: f32, thickness: f32,
};

@group(0) @binding(0) var<uniform> P: Params;

// The types, in the order of the node's menu (brewed_texture_core::nodes::NOISE_TYPES); white
// noise comes after them. Each is the KIND of a pipeline of its own.
const PERLIN: u32 = 0u;
const SIMPLEX: u32 = 1u;
const VALUE: u32 = 2u;
const BILLOW: u32 = 3u;
const RIDGED: u32 = 4u;
const LIQUID: u32 = 5u;
const MARBLE: u32 = 6u;
const WOOD: u32 = 7u;
const VEINS: u32 = 8u;
const STREAKS: u32 = 9u;
const FLOW: u32 = 10u;
const FIBERS: u32 = 11u;
const SCRATCHES: u32 = 12u;
const RIPPLES: u32 = 13u;
const TURING: u32 = 14u;
const RAINDROPS: u32 = 15u;
const CAUSTICS: u32 = 16u;
const CELLS: u32 = 17u;
const CRACKS: u32 = 18u;
const CRYSTAL: u32 = 19u;
const MOSAIC: u32 = 20u;
const STONES: u32 = 21u;
const SCALES: u32 = 22u;
const BLOCKS: u32 = 23u;
const TRUCHET: u32 = 24u;
const MAZE: u32 = 25u;
const SPOTS: u32 = 26u;
const BLOTCHES: u32 = 27u;
const CRATERS: u32 = 28u;

// ------------------------------------------------------------------ lattices

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

fn rand2(h: u32) -> vec2<f32> {
    return vec2<f32>(rand01(h), rand01(pcg(h ^ 0x68e31da4u)));
}

fn grad(h: u32) -> vec2<f32> {
    let a = rand01(h) * TAU;
    return vec2<f32>(cos(a), sin(a));
}

fn fade(t: vec2<f32>) -> vec2<f32> {
    return t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
}

// How much of a pixel `aa` cells wide a line `w` cells wide covers, `d` cells from the
// line's middle.
fn line_cover(d: f32, w: f32, aa: f32) -> f32 {
    let lo = max(d - aa * 0.5, -w * 0.5);
    let hi = min(d + aa * 0.5, w * 0.5);
    return clamp((hi - lo) / aa, 0.0, 1.0);
}

// ------------------------------------------------------------------ basis noises

// Gradient (Perlin) noise in roughly [-1, 1].
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

// Gradient noise and how fast it changes along x and along y.
fn perlin_d(p: vec2<f32>, period: vec2<i32>, seed: u32) -> vec3<f32> {
    let fl = floor(p);
    let i = vec2<i32>(fl);
    let f = p - fl;
    let u = fade(f);
    let du = 30.0 * f * f * (f * (f - 2.0) + 1.0);
    let g00 = grad(cell_hash(i, period, seed));
    let g10 = grad(cell_hash(i + vec2<i32>(1, 0), period, seed));
    let g01 = grad(cell_hash(i + vec2<i32>(0, 1), period, seed));
    let g11 = grad(cell_hash(i + vec2<i32>(1, 1), period, seed));
    let n00 = dot(g00, f);
    let n10 = dot(g10, f - vec2<f32>(1.0, 0.0));
    let n01 = dot(g01, f - vec2<f32>(0.0, 1.0));
    let n11 = dot(g11, f - vec2<f32>(1.0, 1.0));
    let k = n00 - n10 - n01 + n11;
    let n = n00 + u.x * (n10 - n00) + u.y * (n01 - n00) + u.x * u.y * k;
    let d = g00 + u.x * (g10 - g00) + u.y * (g01 - g00) + u.x * u.y * (g00 - g10 - g01 + g11)
        + du * (vec2<f32>(n10 - n00, n01 - n00) + vec2<f32>(u.y, u.x) * k);
    return vec3<f32>(n, d) * 1.41421356;
}

// The hash of a corner of simplex noise's triangles: the same for corners a period apart.
fn simplex_hash(ij: vec2<i32>, columns: i32, rows: i32, seed: u32) -> u32 {
    var q = ij;
    if (tile_y(P.tiling)) {
        let j = wrap_i(q.y, rows);
        // A row a period down is shifted by half a corner for every row in between.
        q.x = q.x + (q.y - j) / 2;
        q.y = j;
    }
    if (tile_x(P.tiling)) { q.x = wrap_i(q.x, columns); }
    return hash3(q.x, q.y, seed);
}

// Simplex noise in roughly [-1, 1]: gradients on the corners of triangles instead of
// squares, so nothing in it lines up with the axes. The triangles lie in rows, each row
// shifted by half a triangle, so to come round the rows are an even number, and as many
// triangles to a row as make them all but equilateral.
fn simplex(p: vec2<f32>, period: vec2<i32>, seed: u32) -> f32 {
    // About as many rows as cells, so its bumps are about as big as those of Perlin noise.
    let rows = max(2, 2 * i32(round(f32(period.y) * 0.5)));
    let columns = max(1, i32(round(f32(rows) * 0.8660254 * f32(period.x) / f32(period.y))));
    // In units of the side of a triangle.
    let unit = f32(columns) / f32(period.x);
    let h = f32(period.y) / f32(rows) * unit;
    let q = p * unit;
    // Corner (i, j) is at (i + j / 2, j h).
    let fj = q.y / h;
    let fi = q.x - 0.5 * fj;
    let b = floor(vec2<f32>(fi, fj));
    let f = vec2<f32>(fi, fj) - b;
    let base = vec2<i32>(b);
    // The rhombus of corners from `base` is two triangles, either side of its short diagonal.
    let far = select(vec2<i32>(0, 0), vec2<i32>(1, 1), f.x + f.y > 1.0);
    // Each corner reaches as far as it can without reaching into a triangle not its own.
    let t = clamp((h * h - 0.25) / (h * h + 0.25), 0.0, 1.0);
    let r2 = min(h * h, 0.25 * (1.0 + t) * (1.0 + t) + h * h * (1.0 - t) * (1.0 - t));
    var n = 0.0;
    for (var k = 0; k < 3; k++) {
        var c = far;
        if (k == 1) { c = vec2<i32>(1, 0); }
        if (k == 2) { c = vec2<i32>(0, 1); }
        let ij = base + c;
        let d = q - vec2<f32>(f32(ij.x) + 0.5 * f32(ij.y), f32(ij.y) * h);
        let w = max(r2 - dot(d, d), 0.0);
        n = n + w * w * w * w * dot(grad(simplex_hash(ij, columns, rows, seed)), d);
    }
    // As strong whatever the reach.
    return n * 11.3 * pow(0.75 / r2, 4.5);
}

// Value noise in [0, 1]: random values at the lattice points, smoothly in between.
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

// ------------------------------------------------------------------ fractal sums

// What fBm sums up, octave after octave.
const BASIS_GRADIENT: u32 = 0u;
const BASIS_VALUE: u32 = 1u;
const BASIS_BILLOW: u32 = 2u;
const BASIS_SIMPLEX: u32 = 3u;
// One random value per cell, with hard edges.
const BASIS_BLOCK: u32 = 4u;

// Where octave `o` is shifted to. Doubling the frequency puts every lattice point of an
// octave on one of the next; shifted, the points (where gradient noise is always zero)
// stop lining up into a grid. A shift keeps the period, so the octaves still tile.
fn octave_shift(o: i32) -> vec2<f32> {
    return vec2<f32>(0.3719, 0.6853) * f32(o);
}

// Octaves of a basis noise, each twice as fine as the last and `roughness` times as
// strong, in [0, 1].
fn fbm_r(p: vec2<f32>, period: vec2<i32>, seed: u32, basis: u32, octaves: i32, roughness: f32) -> f32 {
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
            case BASIS_SIMPLEX: { n = simplex(q, per, s) * 0.5 + 0.5; }
            default: { n = rand01(cell_hash(vec2<i32>(floor(q)), per, s)); }
        }
        sum = sum + n * amp;
        total = total + amp;
        amp = amp * roughness;
        freq = freq * 2;
    }
    return sum / max(total, 1e-5);
}

// With the node's octaves and roughness.
fn fbm(p: vec2<f32>, period: vec2<i32>, seed: u32, basis: u32) -> f32 {
    return fbm_r(p, period, seed, basis, P.octaves, P.roughness);
}

// Gradient fBm clusters around 0.5: stretched to use the whole range.
fn clouds(p: vec2<f32>, period: vec2<i32>, seed: u32) -> f32 {
    return clouds_n(p, period, seed, P.octaves);
}

fn clouds_n(p: vec2<f32>, period: vec2<i32>, seed: u32, octaves: i32) -> f32 {
    return (fbm_r(p, period, seed, BASIS_GRADIENT, octaves, P.roughness) - 0.5) * 1.6 + 0.5;
}

// Octaves of gradient noise with how fast they change: the sum in [-1, 1], and its slope
// along x and along y, per cell.
fn fbm_d(p: vec2<f32>, period: vec2<i32>, seed: u32) -> vec3<f32> {
    var amp = 1.0;
    var total = 0.0;
    var sum = vec3<f32>(0.0);
    var freq = 1;
    for (var o = 0; o < clamp(P.octaves, 1, 10); o++) {
        let n = perlin_d(p * f32(freq) + octave_shift(o), period * freq, seed + u32(o) * 1013u);
        sum = sum + vec3<f32>(n.x, n.yz * f32(freq)) * amp;
        total = total + amp;
        amp = amp * P.roughness;
        freq = freq * 2;
    }
    return sum / max(total, 1e-5);
}

// Ridged multifractal: the creases of simplex noise turned up into ridges, every octave
// strongest where the ones before it rose, so detail gathers along the ridges as it does
// on mountains.
fn ridged(p: vec2<f32>, period: vec2<i32>, seed: u32) -> f32 {
    var sum = 0.0;
    var total = 0.0;
    var amp = 1.0;
    var weight = 1.0;
    var freq = 1;
    for (var o = 0; o < clamp(P.octaves, 1, 10); o++) {
        let n = simplex(p * f32(freq) + octave_shift(o), period * freq, seed + u32(o) * 1013u);
        var r = 1.0 - abs(n);
        r = r * r * weight;
        weight = clamp(r * 2.0, 0.0, 1.0);
        sum = sum + r * amp;
        total = total + amp;
        amp = amp * P.roughness;
        freq = freq * 2;
    }
    return sum / max(total, 1e-5) * 1.25;
}

// ------------------------------------------------------------------ bent and drawn out

// Wood: the rings of a log sawn along its length, where the saw meets them at a slant:
// the contour lines of a smooth noise drawn out along the grain, with fine streaks along it.
fn wood(uv: vec2<f32>, scale: i32, seed: u32) -> f32 {
    // `scale` knots of rings down the image, each `stretch` times longer than it is tall.
    let period = vec2<i32>(max(1, i32(round(f32(scale) / f32(max(P.stretch, 1))))), scale);
    let n = fbm_r(uv * vec2<f32>(period), period, seed, BASIS_GRADIENT, 2, 0.5);
    let rings = f32(clamp(P.count, 1, 256));
    // Thin dark bands of late wood between wide light ones.
    let late = pow(0.5 - 0.5 * cos(TAU * n * rings), 6.0);
    let fine = vec2<i32>(period.x * 6, scale * 20);
    let grain = value_noise(uv * vec2<f32>(fine), fine, seed ^ 0x7f4a7c15u);
    return 0.72 - 0.45 * late + (grain - 0.5) * 0.14;
}

// Veins: thin lines, as wide all along, where fractal noise crosses its middle: the edges
// of the patches it would make, wandering like cracks in ice.
fn veins(p: vec2<f32>, period: vec2<i32>, seed: u32, aa: f32) -> f32 {
    let n = fbm_d(p, period, seed);
    // How far the crossing is: how high the noise is over how steep.
    let d = abs(n.x) / max(length(n.yz), 1e-4);
    return line_cover(d, P.width, aa);
}

// Which way a smooth current runs at `p`: `angle`, turned this way and that by a noise as
// far as `spread` lets it.
fn current(p: vec2<f32>, period: vec2<i32>, seed: u32) -> vec2<f32> {
    let n = perlin(p, period, seed) + 0.5 * perlin(p * 2.0 + octave_shift(1), period * 2, seed + 1013u);
    let a = radians(P.angle) + n * P.spread * PI;
    return vec2<f32>(cos(a), -sin(a));
}

// Flow: fine grain smeared along a current, as wind, water or a brush leave it: every point
// the average of the grain `length` / 4 cells up and down its streamline.
fn flow(p: vec2<f32>, period: vec2<i32>, seed: u32) -> f32 {
    const STEPS: i32 = 12;
    const GRAIN: f32 = 24.0;
    let step = max(P.length, 0.01) * 0.25 / f32(STEPS);
    let grain = period * i32(GRAIN);
    let gs = seed ^ 0x2545f491u;
    var sum = value_noise(p * GRAIN, grain, gs);
    var ahead = p;
    var behind = p;
    for (var k = 0; k < STEPS; k++) {
        // Half a step first, to follow the bends (midpoint integration).
        ahead = ahead + current(ahead + current(ahead, period, seed) * step * 0.5, period, seed) * step;
        behind = behind - current(behind - current(behind, period, seed) * step * 0.5, period, seed) * step;
        sum = sum + value_noise(ahead * GRAIN, grain, gs) + value_noise(behind * GRAIN, grain, gs);
    }
    let mean = sum / f32(2 * STEPS + 1);
    return 0.5 + (mean - 0.5) * 4.0;
}

// ------------------------------------------------------------------ lines

// Fibers: many thin strands lying mostly one way (`angle`, within `spread`), each gently
// bent and thinner towards its ends, over and across one another: fur, hair, cloth, grass.
fn fibers(p: vec2<f32>, period: vec2<i32>, seed: u32, aa: f32) -> f32 {
    let half = max(P.length, 0.01) * 0.25;
    let expected = P.density * 250.0;
    let count = min(i32(ceil(expected)), 256);
    let keep = expected / max(f32(count), 1.0);
    let width = max(P.width, 0.0);
    // A strand reaches this many cells from the cell it starts in.
    let reach = clamp(i32(floor(half * 1.2 + width + 1.0)), 1, 3);
    let i = vec2<i32>(floor(p));
    var v = 0.0;
    for (var dy = -reach; dy <= reach; dy++) {
        for (var dx = -reach; dx <= reach; dx++) {
            let cell = i + vec2<i32>(dx, dy);
            var h = cell_hash(cell, period, seed);
            for (var k = 0; k < count; k++) {
                h = pcg(h);
                let at = vec2<f32>(cell) + rand2(h);
                let rel = p - at;
                let far = half * 1.2 + width + aa;
                if (dot(rel, rel) > far * far) { continue; }
                let g = pcg(h ^ 0x3c6ef372u);
                if (rand01(g) >= keep) { continue; }
                let a = radians(P.angle) + (rand01(pcg(g)) - 0.5) * P.spread * PI;
                let bend = (rand01(pcg(g ^ 0xa54ff53au)) - 0.5) * 2.4;
                let len = half * mix(0.5, 1.0, rand01(pcg(g ^ 0x510e527fu)));
                let strength = mix(0.35, 1.0, rand01(pcg(g ^ 0x9b05688cu)));
                // Along the strand and across it, its middle line bending away as t².
                let along = vec2<f32>(cos(a), -sin(a));
                let t = dot(rel, along);
                let s = dot(rel, vec2<f32>(-along.y, along.x));
                let tc = clamp(t, -len, len);
                let off = s - 0.5 * bend * tc * tc;
                var d = abs(off) / sqrt(1.0 + bend * bend * tc * tc);
                if (abs(t) > len) { d = length(vec2<f32>(t - tc, off)); }
                let taper = sqrt(max(1.0 - (tc / len) * (tc / len), 0.0));
                v = v + line_cover(d, width * taper, aa) * strength * (1.0 - v);
            }
        }
    }
    return v;
}

// Straight lines in every direction, `length` cells long and `width` cells wide in the
// middle, thinner towards their ends; on average `8 × density` start in a cell.
fn scratches(p: vec2<f32>, period: vec2<i32>, seed: u32, aa: f32) -> f32 {
    let half = max(P.length, 0.01) * 0.5;
    let expected = P.density * 8.0;
    let count = i32(ceil(expected));
    let keep = expected / max(f32(count), 1.0);
    let width = max(P.width, 0.0);
    // A line reaches this many cells from the cell it starts in.
    let reach = clamp(i32(floor(half + width + 1.0)), 1, 4);
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
                let w = width * (1.0 - along * along) * 0.5;
                let s = 1.0 - smoothstep(w, w + aa, d);
                v = max(v, s * strength);
            }
        }
    }
    return v;
}

// ------------------------------------------------------------------ waves

// Ripples (Gabor noise): little bundles of stripes, a few in every cell, each turned the
// way the stripes run there. Where that way changes slowly, they curl like a fingerprint.
fn ripples(p: vec2<f32>, period: vec2<i32>, seed: u32) -> f32 {
    const IMPULSES: i32 = 6;
    // A bundle fades out within a cell, and has about two and a half stripes across.
    const FADE: f32 = 1.3;
    const STRIPES: f32 = 2.5;
    // The way the stripes run changes over a quarter of the cells.
    let slow = max(period / 4, vec2<i32>(1));
    let i = vec2<i32>(floor(p));
    var sum = 0.0;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let cell = i + vec2<i32>(dx, dy);
            var h = cell_hash(cell, period, seed);
            for (var k = 0; k < IMPULSES; k++) {
                h = pcg(h);
                let at = vec2<f32>(cell) + vec2<f32>(rand01(h), rand01(pcg(h)));
                h = pcg(pcg(h));
                let weight = rand01(h) * 2.0 - 1.0;
                let d = p - at;
                let bell = exp(-PI * FADE * FADE * dot(d, d));
                if (bell < 1e-3) { continue; }
                let turn = perlin(at / vec2<f32>(period) * vec2<f32>(slow), slow, seed ^ 0x9e3779b9u);
                let a = radians(P.angle) + turn * P.spread * PI;
                sum = sum + weight * bell * cos(TAU * STRIPES * dot(d, vec2<f32>(cos(a), -sin(a))));
            }
        }
    }
    return 0.5 + sum * 0.25;
}

// Turing patterns, as reaction and diffusion leave them on shells, skins and corals: noise
// of one wavelength running every way at once, cut at its middle into winding stripes, or
// into spots when `density` leans one way or the other.
fn turing(p: vec2<f32>, period: vec2<i32>, seed: u32) -> f32 {
    const IMPULSES: i32 = 4;
    // Wide bundles (three stripes or so across), so the wavelength is all but one.
    const FADE: f32 = 0.7;
    const STRIPES: f32 = 2.0;
    let i = vec2<i32>(floor(p));
    var sum = 0.0;
    for (var dy = -2; dy <= 2; dy++) {
        for (var dx = -2; dx <= 2; dx++) {
            let cell = i + vec2<i32>(dx, dy);
            var h = cell_hash(cell, period, seed);
            for (var k = 0; k < IMPULSES; k++) {
                h = pcg(h);
                let d = p - vec2<f32>(cell) - rand2(h);
                let bell = exp(-PI * FADE * FADE * dot(d, d));
                if (bell < 1e-3) { continue; }
                let a = rand01(pcg(h ^ 0xb5297a4du)) * PI;
                let weight = rand01(pcg(h ^ 0x1b56c4e9u)) * 2.0 - 1.0;
                sum = sum + weight * bell * cos(TAU * STRIPES * dot(d, vec2<f32>(cos(a), sin(a))));
            }
        }
    }
    let n = sum / 0.82 + (P.density - 0.5) * 2.5;
    return smoothstep(-0.15, 0.15, n);
}

// Raindrops: rings of little waves spreading from where drops fell on water, the further
// they have run the fainter, running through one another; `density` is how many fell.
fn raindrops(p: vec2<f32>, period: vec2<i32>, seed: u32) -> f32 {
    const WAVELENGTH: f32 = 0.12;
    // How far behind its front a ring still shows, and how sharp the front is.
    const TRAIN: f32 = 0.3;
    const FRONT: f32 = 0.06;
    let i = vec2<i32>(floor(p));
    var sum = 0.0;
    // A front runs at most 1.4 cells from where its drop fell.
    for (var dy = -2; dy <= 2; dy++) {
        for (var dx = -2; dx <= 2; dx++) {
            let cell = i + vec2<i32>(dx, dy);
            var h = cell_hash(cell, period, seed);
            for (var k = 0; k < 2; k++) {
                h = pcg(h);
                if (rand01(h) >= P.density) { continue; }
                let at = vec2<f32>(cell) + rand2(pcg(h ^ 0x2c1b3c6du));
                let front = mix(0.15, 1.4, rand01(pcg(h ^ 0x9e3779b9u)));
                let x = length(p - at) - front;
                if (x > 0.25 || x < -1.0) { continue; }
                let fade = select(TRAIN, FRONT, x > 0.0);
                let envelope = exp(-(x * x) / (fade * fade));
                sum = sum + envelope * cos(TAU * x / WAVELENGTH) / (1.0 + 2.0 * front);
            }
        }
    }
    return 0.5 + sum * 0.75;
}

// The water surface over the caustics.
fn surface(q: vec2<f32>, period: vec2<i32>, seed: u32) -> f32 {
    return simplex(q, period, seed) + 0.25 * simplex(q * 2.0 + octave_shift(1), period * 2, seed + 1013u);
}

// Caustics: light on the floor of a pool, gathered into bright lines by the waves above as
// by lenses. A ray through the surface at p lands at p + k ∇h, so light that fell on a
// little square spreads over det(I + k H) as much, H the second derivatives of h: where
// that comes near zero the light focuses.
fn caustics(p: vec2<f32>, period: vec2<i32>, seed: u32) -> f32 {
    const E: f32 = 0.05;
    let c = surface(p, period, seed);
    let xp = surface(p + vec2<f32>(E, 0.0), period, seed);
    let xm = surface(p - vec2<f32>(E, 0.0), period, seed);
    let yp = surface(p + vec2<f32>(0.0, E), period, seed);
    let ym = surface(p - vec2<f32>(0.0, E), period, seed);
    let pp = surface(p + vec2<f32>(E, E), period, seed);
    let pm = surface(p + vec2<f32>(E, -E), period, seed);
    let mp = surface(p + vec2<f32>(-E, E), period, seed);
    let mm = surface(p - vec2<f32>(E, E), period, seed);
    let hxx = (xp - 2.0 * c + xm) / (E * E);
    let hyy = (yp - 2.0 * c + ym) / (E * E);
    let hxy = (pp - pm - mp + mm) / (4.0 * E * E);
    let k = P.distortion * 0.08;
    let spread = (1.0 + k * hxx) * (1.0 + k * hyy) - k * k * hxy * hxy;
    let light = 1.0 / max(abs(spread), 0.05);
    return 1.0 - exp(-light * 0.15);
}

// ------------------------------------------------------------------ cells

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

// Cracks: the borders of cells, cracked as dry mud or old paint crack: wobbling, wider
// here and narrower there, with finer cracks between them here and there.
fn cracks(p: vec2<f32>, period: vec2<i32>, seed: u32, aa: f32) -> f32 {
    let push = P.distortion * 0.2;
    let q = p + push * (vec2<f32>(
        fbm_r(p * 2.0, period * 2, seed ^ 0x1f123bb5u, BASIS_GRADIENT, 3, 0.5),
        fbm_r(p * 2.0, period * 2, seed ^ 0x5f356495u, BASIS_GRADIENT, 3, 0.5)) - 0.5);
    let c = voronoi(q, period, seed);
    let wide = P.width * mix(0.4, 1.6, value_noise(p * 3.0, period * 3, seed ^ 0x2c1b3c6du));
    var crack = line_cover(cell_border(q, c.near, period, seed), wide, aa);
    let q2 = q * 3.0;
    let per2 = period * 3;
    let s2 = seed ^ 0x297a2d39u;
    let c2 = voronoi(q2, per2, s2);
    let here = smoothstep(0.45, 0.65, value_noise(p * 2.0, period * 2, seed ^ 0x2d54ab8fu));
    crack = max(crack, line_cover(cell_border(q2, c2.near, per2, s2) / 3.0, P.width * 0.5, aa) * here);
    return 1.0 - crack;
}

// Scales: rows of round scales, each row half a scale over from the one above and under
// it, as on a fish or a roof of shingles; every scale bulges and has a shade of its own.
fn scales(p: vec2<f32>, period: vec2<i32>, seed: u32) -> f32 {
    const R: f32 = 0.62;
    // Rows half a cell apart: twice as many as columns, an even number, so the rows that
    // are shifted by half a scale come round.
    let rows = period.y * 2;
    let reach = R + P.jitter * 0.1;
    let lo = i32(floor(2.0 * (p.y - 0.25 - reach)));
    let hi = i32(ceil(2.0 * (p.y - 0.25 + reach)));
    // From the top row down: a row lies over the one below it.
    for (var j = lo; j <= hi; j++) {
        let shift = 0.5 * f32(wrap_i(j, 2));
        let i0 = i32(floor(p.x - shift));
        var best = 10.0;
        var id = 0u;
        for (var di = -1; di <= 1; di++) {
            var q = vec2<i32>(i0 + di, j);
            if (tile_x(P.tiling)) { q.x = wrap_i(q.x, period.x); }
            if (tile_y(P.tiling)) { q.y = wrap_i(q.y, rows); }
            let h = hash3(q.x, q.y, seed);
            let at = vec2<f32>(f32(i0 + di) + 0.5 + shift, f32(j) * 0.5 + 0.25) + (rand2(h) - 0.5) * P.jitter * 0.2;
            let d = length(p - at);
            if (d < best) {
                best = d;
                id = h;
            }
        }
        if (best < R) {
            let t = best / R;
            return sqrt(1.0 - t * t) * mix(0.55, 1.0, rand01(pcg(id ^ 0x51ed270bu)));
        }
    }
    return 0.0;
}

// ------------------------------------------------------------------ tiles

// Truchet tiles: every cell holds two quarter circles joining the middles of its sides,
// turned one of two ways at random; from cell to cell they run on as tubes that wind and
// close into loops.
fn truchet(p: vec2<f32>, period: vec2<i32>, seed: u32, aa: f32) -> f32 {
    let i = vec2<i32>(floor(p));
    var f = p - floor(p);
    if (rand01(cell_hash(i, period, seed)) < 0.5) { f.x = 1.0 - f.x; }
    let d = min(abs(length(f) - 0.5), abs(length(f - vec2<f32>(1.0)) - 0.5));
    let w = max(P.thickness, 0.0);
    // Round across, as a tube is.
    let t = clamp(d / max(w * 0.5, 1e-4), 0.0, 1.0);
    return sqrt(1.0 - t * t) * line_cover(d, w, aa);
}

// The side a maze cell opens: 0 right, 1 down, 2 left, 3 up.
fn door(cell: vec2<i32>, period: vec2<i32>, seed: u32) -> u32 {
    return cell_hash(cell, period, seed) >> 30u;
}

// A maze: every cell opens one of its four sides at random, and a corridor runs from its
// middle through each side that it or its neighbour opened.
fn maze(p: vec2<f32>, period: vec2<i32>, seed: u32, aa: f32) -> f32 {
    let i = vec2<i32>(floor(p));
    let f = p - floor(p) - 0.5;
    let own = door(i, period, seed);
    var d = length(f);
    if (own == 0u || door(i + vec2<i32>(1, 0), period, seed) == 2u) { d = min(d, select(length(f), abs(f.y), f.x > 0.0)); }
    if (own == 2u || door(i - vec2<i32>(1, 0), period, seed) == 0u) { d = min(d, select(length(f), abs(f.y), f.x < 0.0)); }
    if (own == 1u || door(i + vec2<i32>(0, 1), period, seed) == 3u) { d = min(d, select(length(f), abs(f.x), f.y > 0.0)); }
    if (own == 3u || door(i - vec2<i32>(0, 1), period, seed) == 1u) { d = min(d, select(length(f), abs(f.x), f.y < 0.0)); }
    return line_cover(d, max(P.thickness, 0.0), aa);
}

// ------------------------------------------------------------------ scattered

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

// Craters: round pits with raised rims, of many sizes, in and over one another as on the
// moon; `density` is how many there are.
fn craters(p: vec2<f32>, period: vec2<i32>, seed: u32) -> f32 {
    var height = 0.0;
    // Big ones, and three times as many three times smaller.
    for (var layer = 0; layer < 2; layer++) {
        let k = 1 + 2 * layer;
        let q = p * f32(k);
        let per = period * k;
        let s = seed + u32(layer) * 0x632be5abu;
        let i = vec2<i32>(floor(q));
        // A crater reaches twice its radius (under a cell) from its middle.
        for (var dy = -2; dy <= 2; dy++) {
            for (var dx = -2; dx <= 2; dx++) {
                let cell = i + vec2<i32>(dx, dy);
                var h = cell_hash(cell, per, s);
                if (rand01(h) >= P.density) { continue; }
                h = pcg(h);
                let at = vec2<f32>(cell) + rand2(h);
                h = pcg(h);
                // Small ones more often than big ones.
                let r = mix(0.15, 0.9, rand01(h) * rand01(pcg(h ^ 0x6a09e667u)));
                let u = length(q - at) / r;
                if (u > 2.0) { continue; }
                let bowl = select(0.0, u * u - 1.0, u < 1.0);
                let rim = exp(-(u - 1.0) * (u - 1.0) / 0.06);
                height = height + (bowl * 0.9 + rim * 0.45) * r / f32(k);
            }
        }
    }
    return 0.55 + height;
}

// ------------------------------------------------------------------

@fragment
fn main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    let uv = out_uv(gid.xy, size);
    let scale = max(P.scale, 1);
    let base = vec2<i32>(scale, scale);
    let p = uv * f32(scale);
    // One pixel, in cells.
    let aa = f32(scale) / f32(max(size.x, size.y));
    // Every type draws a pattern of its own from the seed (Perlin noise the seed as it is).
    let seed = P.seed ^ select(pcg(KIND), 0u, KIND == PERLIN);

    var v = 0.0;
    switch KIND {
        case PERLIN: {
            v = clouds(p, base, seed);
        }
        case SIMPLEX: {
            v = (fbm(p, base, seed, BASIS_SIMPLEX) - 0.5) * 1.3 + 0.5;
        }
        case VALUE: {
            v = (fbm(p, base, seed, BASIS_VALUE) - 0.5) * 1.6 + 0.5;
        }
        case BILLOW: {
            v = fbm(p, base, seed, BASIS_BILLOW) * 1.8;
        }
        case RIDGED: {
            v = ridged(p, base, seed);
        }
        case LIQUID: {
            // Read where other noises push the point, themselves read where yet others push
            // it: the noise swirls and folds. The pushes are smoother than the noise, so it
            // flows rather than crumples, and they repeat with the image, so it still tiles.
            let k = P.distortion * 2.5;
            let calm = min(P.octaves, 3);
            let q = vec2<f32>(clouds_n(p, base, seed + 7919u, calm), clouds_n(p, base, seed + 15887u, calm)) - 0.5;
            let r = vec2<f32>(clouds_n(p + q * k, base, seed + 23873u, calm), clouds_n(p + q * k, base, seed + 31859u, calm)) - 0.5;
            v = clouds(p + r * k, base, seed);
        }
        case MARBLE: {
            // Bands across the image, `scale` of them from edge to edge, bent by turbulence.
            let turbulence = fbm(p, base, seed + 4099u, BASIS_GRADIENT) * 2.0 - 1.0;
            v = 0.5 + 0.5 * sin(TAU * uv.x * f32(scale) + turbulence * P.distortion * 6.0);
        }
        case WOOD: {
            v = wood(uv, scale, seed);
        }
        case VEINS: {
            v = veins(p, base, seed, aa);
        }
        case STREAKS: {
            // Cells `stretch` times longer than they are tall.
            let period = vec2<i32>(scale, scale * max(P.stretch, 1));
            v = (fbm(uv * vec2<f32>(period), period, seed, BASIS_VALUE) - 0.5) * 1.8 + 0.5;
        }
        case FLOW: {
            v = flow(p, base, seed);
        }
        case FIBERS: {
            v = fibers(p, base, seed, aa);
        }
        case SCRATCHES: {
            v = scratches(p, base, seed, aa);
        }
        case RIPPLES: {
            v = ripples(p, base, seed);
        }
        case TURING: {
            v = turing(p, base, seed);
        }
        case RAINDROPS: {
            v = raindrops(p, base, seed);
        }
        case CAUSTICS: {
            v = caustics(p, base, seed);
        }
        case CELLS: {
            v = voronoi(p, base, seed).f1 / 1.1;
        }
        case CRACKS: {
            v = cracks(p, base, seed, aa);
        }
        case CRYSTAL: {
            // Every cell a flat facet, tilted its own way.
            let c = voronoi(p, base, seed);
            let tilt = grad(pcg(c.id ^ 0x5bd1e995u));
            v = mix(0.25, 0.75, rand01(pcg(c.id))) + dot(p - c.near, tilt) * 0.55;
        }
        case MOSAIC: {
            v = rand01(pcg(voronoi(p, base, seed).id));
        }
        case STONES: {
            // Every cell a stone of its own height, flat on top and rounded towards the gaps.
            let c = voronoi(p, base, seed);
            let t = clamp(cell_border(p, c.near, base, seed) / 0.45, 0.0, 1.0);
            v = sqrt(t * (2.0 - t)) * mix(0.45, 1.0, rand01(pcg(c.id)));
        }
        case SCALES: {
            v = scales(p, base, seed);
        }
        case BLOCKS: {
            v = (fbm(p, base, seed, BASIS_BLOCK) - 0.5) * 1.4 + 0.5;
        }
        case TRUCHET: {
            v = truchet(p, base, seed, aa);
        }
        case MAZE: {
            v = maze(p, base, seed, aa);
        }
        case SPOTS: {
            v = spots(p, base, seed);
        }
        case BLOTCHES: {
            // Where fractal noise rises over a level: the lower the level, the more it covers.
            let level = mix(0.8, 0.2, P.density);
            v = smoothstep(level - 0.025, level + 0.025, clouds(p, base, seed));
        }
        case CRATERS: {
            v = craters(p, base, seed);
        }
        default: {
            v = rand01(hash3(i32(gid.x), i32(gid.y), seed));
        }
    }
    v = clamp(v, 0.0, 1.0);
    if (P.invert != 0u) { v = 1.0 - v; }
    return gray(v);
}
