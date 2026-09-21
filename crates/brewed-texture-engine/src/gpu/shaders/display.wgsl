// Kernels that prepare images for display. Values are shown as stored, like in Substance
// Designer: 0.5 is mid gray (base colors are authored in display space).

struct Params {
    // How a thumbnail shows its image: 0 RGBA over a checkerboard, 1 RGB, 2 R, 3 G, 4 B, 5 A.
    mode: u32,
    checker: f32,
    // The format of the output the image is shown as (0, RGBA, for every other node).
    format: u32,
    pad0: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;
@group(0) @binding(2) var out_tex: texture_storage_2d<rgba16float, write>;

fn checker(p: vec2<f32>) -> vec3<f32> {
    let c = (i32(floor(p.x)) + i32(floor(p.y))) & 1;
    return select(vec3<f32>(0.22), vec3<f32>(0.32), c == 1);
}

fn display_color(c: vec4<f32>, checker_pos: vec2<f32>) -> vec4<f32> {
    var rgb = c.rgb;
    switch P.mode {
        case 1u: { rgb = c.rgb; }
        case 2u: { rgb = vec3<f32>(c.r); }
        case 3u: { rgb = vec3<f32>(c.g); }
        case 4u: { rgb = vec3<f32>(c.b); }
        case 5u: { rgb = vec3<f32>(c.a); }
        default: { rgb = mix(checker(checker_pos), c.rgb, clamp(c.a, 0.0, 1.0)); }
    }
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}

// The top level of a copy of an image for the views (or for a kernel that filters it): the
// image as its node shows it, resampled to the copy's size.
@compute @workgroup_size(8, 8, 1)
fn convert(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }
    let c = widen(read_input(in0, gid.xy, size, 0u), GRAY0);
    textureStore(out_tex, vec2<i32>(gid.xy), as_output(c, P.format));
}

// Next mip level of a copy: 2x2 box filter.
@compute @workgroup_size(8, 8, 1)
fn mip(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }
    let p = vec2<i32>(gid.xy) * 2;
    let c = widen(fetch(in0, p, 0u), GRAY0) + widen(fetch(in0, p + vec2<i32>(1, 0), 0u), GRAY0)
          + widen(fetch(in0, p + vec2<i32>(0, 1), 0u), GRAY0) + widen(fetch(in0, p + vec2<i32>(1, 1), 0u), GRAY0);
    textureStore(out_tex, vec2<i32>(gid.xy), c * 0.25);
}

// Small node thumbnail: supersampled box filter of the whole image, composited over a
// checkerboard so transparency is visible.
@compute @workgroup_size(8, 8, 1)
fn thumbnail(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }
    let n = 4;
    var acc = vec4<f32>(0.0);
    for (var sy = 0; sy < n; sy++) {
        for (var sx = 0; sx < n; sx++) {
            let sub = (vec2<f32>(f32(sx), f32(sy)) + 0.5) / f32(n);
            let uv = (vec2<f32>(gid.xy) + sub) / vec2<f32>(size);
            acc = acc + widen(sample_bilinear(in0, uv, 0u), GRAY0);
        }
    }
    let c = as_output(acc / f32(n * n), P.format);
    let cp = out_uv(gid.xy, size) * P.checker;
    textureStore(out_tex, vec2<i32>(gid.xy), display_color(c, cp));
}
