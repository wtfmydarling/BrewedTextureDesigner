struct Params {
    size_x: f32, size_y: f32, scale: f32, offset: f32,
    rotation: f32, random_rotation: f32, random_scale: f32, random_offset: f32,
    luminance: f32, random_luminance: f32, mask_random: f32, softness: f32,
    sides: f32, inner: f32, thickness: f32, corner: f32,
    x_amount: i32, y_amount: i32, pattern: u32, blend_add: u32,
    seed: u32, tiling: u32, reach: i32, vertical: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var pattern_tex: texture_2d<f32>;
@group(0) @binding(2) var out_tex: texture_storage_2d<rgba16float, write>;

// The grid is made of "lines" (rows, or columns with `vertical`). Line j is shifted along
// its own direction by `offset * j` tiles. The shift is not wrapped to [0, 1): with an
// offset of one tile every line moves by a whole tile, so the per-tile random values move
// along with the tiles instead of looking exactly like no offset at all.
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }

    let nx = max(P.x_amount, 1);
    let ny = max(P.y_amount, 1);
    let amount = vec2<f32>(f32(nx), f32(ny));
    let g = out_uv(gid.xy, size) * amount;

    let vertical = P.vertical != 0u;
    // a: along a line (the shifted direction), b: across lines.
    let ga = select(g.x, g.y, vertical);
    let gb = select(g.y, g.x, vertical);
    let na = select(nx, ny, vertical);
    let nb = select(ny, nx, vertical);
    let tile_a = select(tile_x(P.tiling), tile_y(P.tiling), vertical);
    let tile_b = select(tile_y(P.tiling), tile_x(P.tiling), vertical);
    let line = i32(floor(gb));

    var sp: ShapeParams;
    sp.kind = P.pattern;
    sp.softness = P.softness;
    sp.aa = max(amount.x / f32(size.x), amount.y / f32(size.y)) * 0.75;
    sp.sides = P.sides;
    sp.inner = P.inner;
    sp.thickness = P.thickness;
    sp.corner = P.corner;

    var acc = vec3<f32>(0.0);
    for (var dj = -P.reach; dj <= P.reach; dj++) {
        let j = line + dj;
        if (!tile_b && (j < 0 || j >= nb)) { continue; }
        let jw = wrap_i(j, nb);
        let shift = P.offset * f32(jw);
        let first = i32(floor(ga - shift));
        for (var di = -P.reach; di <= P.reach; di++) {
            let i = first + di;
            let start = f32(i) + shift;
            // Without tiling, keep every tile that overlaps the image (partial ones too).
            if (!tile_a && (start + 1.0 <= 0.0 || start >= f32(na))) { continue; }
            let iw = wrap_i(i, na);

            // Per-tile random numbers, keyed by (column, row) and stable under wrapping.
            let h0 = select(hash3(iw, jw, P.seed), hash3(jw, iw, P.seed), vertical);
            let h1 = pcg(h0);
            let h2 = pcg(h1);
            let h3 = pcg(h2);
            let h4 = pcg(h3);
            let h5 = pcg(h4);
            if (rand01(h5) < P.mask_random) { continue; }

            let jitter = (vec2<f32>(rand01(h0), rand01(h1)) - 0.5) * P.random_offset;
            let ca = start + 0.5;
            let cb = f32(j) + 0.5;
            let center = select(vec2<f32>(ca, cb), vec2<f32>(cb, ca), vertical) + jitter;
            let rot = P.rotation + (rand01(h2) * 2.0 - 1.0) * P.random_rotation;
            let sc = P.scale * (1.0 - P.random_scale * rand01(h3));
            let lum = P.luminance * (1.0 - P.random_luminance * rand01(h4));
            let q = rotate_ccw(g - center, -rot);
            let half = vec2<f32>(P.size_x, P.size_y) * sc * 0.5;

            var value = vec3<f32>(0.0);
            if (P.pattern == 14u) {
                let pu = q / max(half, vec2<f32>(1e-5)) * 0.5 + 0.5;
                if (pu.x < 0.0 || pu.y < 0.0 || pu.x > 1.0 || pu.y > 1.0) { continue; }
                let c = sample_bilinear(pattern_tex, pu, 0u);
                value = c.rgb * c.a * lum;
            } else {
                sp.half = half;
                value = vec3<f32>(shape_value(sp, q) * lum);
            }
            if (P.blend_add != 0u) {
                acc = acc + value;
            } else {
                acc = max(acc, value);
            }
        }
    }
    textureStore(out_tex, vec2<i32>(gid.xy), vec4<f32>(acc, 1.0));
}
