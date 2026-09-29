// Thumbnails of images. Values are shown as stored, like in Substance Designer: 0.5 is mid
// gray (base colors are authored in display space). The views sample the images themselves
// (see show.wgsl).

struct Params {
    // How a thumbnail shows its image: 0 RGBA over a checkerboard, 1 RGB, 2 R, 3 G, 4 B, 5 A.
    mode: u32,
    checker: f32,
    pad0: u32,
    pad1: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;

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

// Small node thumbnail: supersampled box filter of the whole image, composited over a
// checkerboard so transparency is visible.
@fragment
fn thumbnail(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    let n = 4;
    var acc = vec4<f32>(0.0);
    for (var sy = 0; sy < n; sy++) {
        for (var sx = 0; sx < n; sx++) {
            let sub = (vec2<f32>(f32(sx), f32(sy)) + 0.5) / f32(n);
            let uv = (vec2<f32>(gid.xy) + sub) / vec2<f32>(size);
            acc = acc + widen(sample_bilinear(in0, uv, 0u), gray_input(0u));
        }
    }
    let c = acc / f32(n * n);
    let cp = out_uv(gid.xy, size) * P.checker;
    return display_color(c, cp);
}
