// Kernels that prepare images for display. Values are shown as stored, like in Substance
// Designer: 0.5 is mid gray (base colors are authored in display space).

struct Params {
    mode: u32, checker: f32, pad0: u32, pad1: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;
@group(0) @binding(2) var out_tex: texture_storage_2d<rgba16float, write>;

fn checker(p: vec2<f32>) -> vec3<f32> {
    let c = (i32(floor(p.x)) + i32(floor(p.y))) & 1;
    return select(vec3<f32>(0.22), vec3<f32>(0.32), c == 1);
}

// mode: 0 RGBA over checker, 1 RGB, 2 R, 3 G, 4 B, 5 A
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

// Next mip level of a display copy: 2x2 box filter.
@compute @workgroup_size(8, 8, 1)
fn mip(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }
    let p = vec2<i32>(gid.xy) * 2;
    let c = fetch(in0, p, 0u) + fetch(in0, p + vec2<i32>(1, 0), 0u)
          + fetch(in0, p + vec2<i32>(0, 1), 0u) + fetch(in0, p + vec2<i32>(1, 1), 0u);
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
            acc = acc + sample_bilinear(in0, uv, 0u);
        }
    }
    let c = acc / f32(n * n);
    let cp = out_uv(gid.xy, size) * P.checker;
    textureStore(out_tex, vec2<i32>(gid.xy), display_color(c, cp));
}
