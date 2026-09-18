struct Params {
    size_x: f32, size_y: f32, scale: f32, row_offset: f32,
    rotation: f32, random_rotation: f32, random_scale: f32, random_offset: f32,
    luminance: f32, random_luminance: f32, mask_random: f32, softness: f32,
    sides: f32, inner: f32, thickness: f32, corner: f32,
    x_amount: i32, y_amount: i32, pattern: u32, blend_add: u32,
    seed: u32, tiling: u32, reach: i32, pad0: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var pattern_tex: texture_2d<f32>;
@group(0) @binding(2) var out_tex: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }

    let nx = max(P.x_amount, 1);
    let ny = max(P.y_amount, 1);
    let amount = vec2<f32>(f32(nx), f32(ny));
    let g = out_uv(gid.xy, size) * amount;
    let row = i32(floor(g.y));

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
        let j = row + dj;
        if (!tile_y(P.tiling) && (j < 0 || j >= ny)) { continue; }
        let jw = wrap_i(j, ny);
        let shift = fract(P.row_offset * f32(jw));
        let col = i32(floor(g.x - shift));
        for (var di = -P.reach; di <= P.reach; di++) {
            let i = col + di;
            if (!tile_x(P.tiling) && (i < 0 || i >= nx)) { continue; }
            let iw = wrap_i(i, nx);

            // Per-tile random numbers, stable under wrapping so the result tiles.
            let h0 = hash3(iw, jw, P.seed);
            let h1 = pcg(h0);
            let h2 = pcg(h1);
            let h3 = pcg(h2);
            let h4 = pcg(h3);
            let h5 = pcg(h4);
            if (rand01(h5) < P.mask_random) { continue; }

            let jitter = (vec2<f32>(rand01(h0), rand01(h1)) - 0.5) * P.random_offset;
            let center = vec2<f32>(f32(i) + 0.5 + shift, f32(j) + 0.5) + jitter;
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
