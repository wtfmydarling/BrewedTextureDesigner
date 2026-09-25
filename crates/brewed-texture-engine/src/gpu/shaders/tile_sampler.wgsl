// Tile Sampler: patterns placed on a grid, each tile varied at random and by maps.
//
// The grid and its offsets are the Tile Generator's (tile_gen.wgsl). A tile takes one of the
// connected patterns at random; the maps, read where the tile's center is, keep it or leave it
// out (Mask Map), scale it (Scale Map), turn it (Rotation Map) and color it (Color Map). A
// pattern is read from its mips, from the level as fine as the tile is drawn: a large pattern
// on small tiles does not shimmer. Tiles that overlap combine by the brighter, by their sum,
// or one over another in an order of their own (each tile a random depth).

struct Params {
    size_x: f32, size_y: f32, scale: f32, random_scale: f32,
    scale_map: f32, random_position: f32, offset: f32, rotation: f32,
    random_rotation: f32, rotation_map: f32, random_mirror: f32, luminance: f32,
    random_luminance: f32, mask_random: f32, mask_threshold: f32, softness: f32,
    sides: f32, inner: f32, thickness: f32, corner: f32,
    x_amount: i32, y_amount: i32, pattern: u32, blend: u32,
    seed: u32, tiling: u32, reach: i32, vertical: u32,
    patterns: u32, maps: u32, color: u32, mask_invert: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var pattern1: texture_2d<f32>;
@group(0) @binding(2) var pattern2: texture_2d<f32>;
@group(0) @binding(3) var pattern3: texture_2d<f32>;
@group(0) @binding(4) var pattern4: texture_2d<f32>;
@group(0) @binding(5) var mask_map: texture_2d<f32>;
@group(0) @binding(6) var scale_map: texture_2d<f32>;
@group(0) @binding(7) var rotation_map: texture_2d<f32>;
@group(0) @binding(8) var color_map: texture_2d<f32>;
// Trilinear and clamped: a pattern is read within itself.
@group(0) @binding(9) var smp: sampler;

// The pattern that is the connected images (the last of the shapes' list).
const IMAGES: u32 = 14u;
// The maps connected, as bits of `P.maps`.
const MAP_MASK: u32 = 1u;
const MAP_SCALE: u32 = 2u;
const MAP_ROTATION: u32 = 4u;
const MAP_COLOR: u32 = 8u;
// Overlap: the brighter, the sum, one over another.
const ADD: u32 = 1u;
const STACK: u32 = 2u;
// Stacked, the tiles over a pixel that count: the highest four (a fifth is all but hidden).
const LAYERS: i32 = 4;

// Pattern `which` at `uv`, from the mip level `lod`.
fn pattern(which: u32, uv: vec2<f32>, lod: f32) -> vec4<f32> {
    switch which {
        case 0u: { return widen(textureSampleLevel(pattern1, smp, uv, lod), GRAY0); }
        case 1u: { return widen(textureSampleLevel(pattern2, smp, uv, lod), GRAY1); }
        case 2u: { return widen(textureSampleLevel(pattern3, smp, uv, lod), GRAY2); }
        default: { return widen(textureSampleLevel(pattern4, smp, uv, lod), GRAY3); }
    }
}

fn pattern_size(which: u32) -> vec2<f32> {
    switch which {
        case 0u: { return vec2<f32>(textureDimensions(pattern1)); }
        case 1u: { return vec2<f32>(textureDimensions(pattern2)); }
        case 2u: { return vec2<f32>(textureDimensions(pattern3)); }
        default: { return vec2<f32>(textureDimensions(pattern4)); }
    }
}

// The `k`-th of the connected patterns (the bits of `P.patterns`).
fn nth_pattern(k: u32) -> u32 {
    var seen = 0u;
    for (var i = 0u; i < 4u; i++) {
        if ((P.patterns & (1u << i)) != 0u) {
            if (seen == k) {
                return i;
            }
            seen++;
        }
    }
    return 0u;
}

// A map, as one value, where a tile's center is (`uv`); it wraps where the node tiles.
fn map_value(tex: texture_2d<f32>, uv: vec2<f32>, gray: bool) -> f32 {
    return luminance(widen(sample_bilinear(tex, uv, P.tiling), gray).rgb);
}

// A color of a tile's own: a random hue, more or less saturated.
fn own_color(h: u32) -> vec3<f32> {
    let hue = rand01(h);
    let pure = clamp(abs(fract(vec3<f32>(hue) + vec3<f32>(0.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0) - 1.0, vec3<f32>(0.0), vec3<f32>(1.0));
    return mix(vec3<f32>(1.0), pure, 0.55 + 0.4 * rand01(pcg(h)));
}

// The grid is made of "lines" (rows, or columns with `vertical`), each shifted along itself by
// `offset` tiles more than the one before, as the Tile Generator's.
@fragment
fn main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();

    let nx = max(P.x_amount, 1);
    let ny = max(P.y_amount, 1);
    let amount = vec2<f32>(f32(nx), f32(ny));
    let g = out_uv(gid.xy, size) * amount;
    // A pixel of the image, in cells.
    let texel = amount / vec2<f32>(size);

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
    sp.aa = max(texel.x, texel.y) * 0.75;
    sp.sides = P.sides;
    sp.inner = P.inner;
    sp.thickness = P.thickness;
    sp.corner = P.corner;

    let count = countOneBits(P.patterns);
    var acc = vec3<f32>(0.0);
    // Stacked: the tiles over the pixel, highest first (color, coverage), and their depths.
    var layer: array<vec4<f32>, 4>;
    var depth: array<f32, 4>;
    var layers = 0;
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
            let h6 = pcg(h5);
            let h7 = pcg(h6);
            let h8 = pcg(h7);
            let h9 = pcg(h8);
            let h10 = pcg(h9);
            if (rand01(h5) < P.mask_random) { continue; }

            let jitter = (vec2<f32>(rand01(h0), rand01(h1)) - 0.5) * P.random_position;
            let ca = start + 0.5;
            let cb = f32(j) + 0.5;
            let center = select(vec2<f32>(ca, cb), vec2<f32>(cb, ca), vertical) + jitter;
            // Where the maps are read for the tile.
            let at_map = center / amount;
            if ((P.maps & MAP_MASK) != 0u) {
                var m = map_value(mask_map, at_map, GRAY4);
                if (P.mask_invert != 0u) { m = 1.0 - m; }
                if (m < P.mask_threshold) { continue; }
            }
            var sc = P.scale * (1.0 - P.random_scale * rand01(h3));
            if ((P.maps & MAP_SCALE) != 0u) {
                sc = sc * mix(1.0, map_value(scale_map, at_map, GRAY5), P.scale_map);
            }
            if (sc <= 0.0) { continue; }
            var rot = P.rotation + (rand01(h2) * 2.0 - 1.0) * P.random_rotation;
            if ((P.maps & MAP_ROTATION) != 0u) {
                rot = rot + map_value(rotation_map, at_map, GRAY6) * P.rotation_map;
            }
            var q = rotate_ccw(g - center, -rot);
            // Mirrored across, and up and down, each by its own chance.
            if (rand01(h7) < P.random_mirror) { q.x = -q.x; }
            if (rand01(h8) < P.random_mirror) { q.y = -q.y; }
            let half = vec2<f32>(P.size_x, P.size_y) * sc * 0.5;

            // The tile's color and how much of the pixel it covers.
            var c: vec4<f32>;
            if (P.pattern == IMAGES) {
                if (count == 0u) { continue; }
                let pu = q / max(half, vec2<f32>(1e-5)) * 0.5 + 0.5;
                if (any(pu < vec2<f32>(0.0)) || any(pu > vec2<f32>(1.0))) { continue; }
                let which = nth_pattern(min(u32(rand01(h6) * f32(count)), count - 1u));
                // The level whose pixels are as large as the image's: as many of the
                // pattern's pixels as a pixel of the image covers.
                let per_pixel = pattern_size(which) * texel / max(2.0 * half, vec2<f32>(1e-5));
                c = pattern(which, pu, max(log2(max(per_pixel.x, per_pixel.y)), 0.0));
            } else {
                sp.half = half;
                c = vec4<f32>(vec3<f32>(1.0), shape_value(sp, q));
            }
            if (c.a <= 0.0) { continue; }
            var tint = vec3<f32>(P.luminance * (1.0 - P.random_luminance * rand01(h4)));
            if (P.color == 1u) {
                tint = tint * own_color(h9);
            } else if (P.color == 2u && (P.maps & MAP_COLOR) != 0u) {
                tint = tint * widen(sample_bilinear(color_map, at_map, P.tiling), GRAY7).rgb;
            }
            let rgb = c.rgb * tint;
            if (P.blend == ADD) {
                acc = acc + rgb * c.a;
            } else if (P.blend == STACK) {
                // Into the list, below the tiles higher than it; the lowest falls off a full one.
                let d = rand01(h10);
                if (layers < LAYERS || d > depth[LAYERS - 1]) {
                    var k = min(layers, LAYERS - 1);
                    loop {
                        if (k == 0 || depth[k - 1] >= d) { break; }
                        depth[k] = depth[k - 1];
                        layer[k] = layer[k - 1];
                        k = k - 1;
                    }
                    depth[k] = d;
                    layer[k] = vec4<f32>(rgb, c.a);
                    layers = min(layers + 1, LAYERS);
                }
            } else {
                acc = max(acc, rgb * c.a);
            }
        }
    }
    // Stacked: each tile over those below it, from the lowest up.
    for (var k = layers - 1; k >= 0; k--) {
        acc = mix(acc, layer[k].rgb, layer[k].a);
    }
    return vec4<f32>(acc, 1.0);
}
