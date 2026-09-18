struct Params {
    roughness: f32, jitter: f32, pad0: f32, pad1: f32,
    scale: i32, octaves: i32, kind: u32, seed: u32,
    tiling: u32, invert: u32, pad2: u32, pad3: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var out_tex: texture_storage_2d<rgba16float, write>;

// Wrap lattice coordinates on tiled axes so the noise is periodic over the image.
fn lattice(i: vec2<i32>, period: vec2<i32>) -> vec2<i32> {
    var q = i;
    if (tile_x(P.tiling)) { q.x = wrap_i(i.x, period.x); }
    if (tile_y(P.tiling)) { q.y = wrap_i(i.y, period.y); }
    return q;
}

fn grad(i: vec2<i32>, seed: u32) -> vec2<f32> {
    let a = rand01(hash3(i.x, i.y, seed)) * TAU;
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
    let n00 = dot(grad(lattice(i, period), seed), f);
    let n10 = dot(grad(lattice(i + vec2<i32>(1, 0), period), seed), f - vec2<f32>(1.0, 0.0));
    let n01 = dot(grad(lattice(i + vec2<i32>(0, 1), period), seed), f - vec2<f32>(0.0, 1.0));
    let n11 = dot(grad(lattice(i + vec2<i32>(1, 1), period), seed), f - vec2<f32>(1.0, 1.0));
    return mix(mix(n00, n10, u.x), mix(n01, n11, u.x), u.y) * 1.41421356;
}

// Value noise in [0, 1].
fn value_noise(p: vec2<f32>, period: vec2<i32>, seed: u32) -> f32 {
    let fl = floor(p);
    let i = vec2<i32>(fl);
    let u = fade(p - fl);
    let a = lattice(i, period);
    let b = lattice(i + vec2<i32>(1, 0), period);
    let c = lattice(i + vec2<i32>(0, 1), period);
    let d = lattice(i + vec2<i32>(1, 1), period);
    let va = rand01(hash3(a.x, a.y, seed));
    let vb = rand01(hash3(b.x, b.y, seed));
    let vc = rand01(hash3(c.x, c.y, seed));
    let vd = rand01(hash3(d.x, d.y, seed));
    return mix(mix(va, vb, u.x), mix(vc, vd, u.x), u.y);
}

// Distances to the nearest and second nearest feature points.
fn worley(p: vec2<f32>, period: vec2<i32>, seed: u32) -> vec2<f32> {
    let fl = floor(p);
    let i = vec2<i32>(fl);
    var f1 = 10.0;
    var f2 = 10.0;
    // 5x5 neighbourhood: with full jitter the second nearest point can be two cells away.
    for (var dy = -2; dy <= 2; dy++) {
        for (var dx = -2; dx <= 2; dx++) {
            let cell = i + vec2<i32>(dx, dy);
            let lc = lattice(cell, period);
            let h = hash3(lc.x, lc.y, seed);
            let r = vec2<f32>(rand01(h), rand01(pcg(h)));
            let pt = vec2<f32>(cell) + mix(vec2<f32>(0.5), r, P.jitter);
            let d = length(pt - p);
            if (d < f1) {
                f2 = f1;
                f1 = d;
            } else if (d < f2) {
                f2 = d;
            }
        }
    }
    return vec2<f32>(f1, f2);
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }
    let uv = out_uv(gid.xy, size);
    let scale = max(P.scale, 1);
    let base = vec2<i32>(scale, scale);
    let p = uv * f32(scale);

    var v = 0.0;
    switch P.kind {
        case 0u, 1u, 2u: {
            var amp = 1.0;
            var total = 0.0;
            var freq = 1;
            for (var o = 0; o < clamp(P.octaves, 1, 10); o++) {
                let s = P.seed + u32(o) * 1013u;
                let q = p * f32(freq);
                let period = base * freq;
                var n = 0.0;
                if (P.kind == 0u) {
                    n = perlin(q, period, s) * 0.5 + 0.5;
                } else if (P.kind == 1u) {
                    n = value_noise(q, period, s);
                } else {
                    let r = 1.0 - abs(perlin(q, period, s));
                    n = r * r;
                }
                v = v + n * amp;
                total = total + amp;
                amp = amp * P.roughness;
                freq = freq * 2;
            }
            v = v / max(total, 1e-5);
            if (P.kind == 0u) {
                // fBm clusters around 0.5: stretch it to use the whole range.
                v = (v - 0.5) * 1.6 + 0.5;
            }
        }
        case 3u: {
            v = worley(p, base, P.seed).x / 1.1;
        }
        case 4u: {
            let f = worley(p, base, P.seed);
            v = clamp((f.y - f.x) * 2.0, 0.0, 1.0);
        }
        default: {
            v = rand01(hash3(i32(gid.x), i32(gid.y), P.seed));
        }
    }
    v = clamp(v, 0.0, 1.0);
    if (P.invert != 0u) { v = 1.0 - v; }
    textureStore(out_tex, vec2<i32>(gid.xy), gray(v));
}
