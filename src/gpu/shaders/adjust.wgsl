// Per-pixel adjustments that share one parameter block. `op` selects the operation:
//   0 levels, 1 invert, 2 grayscale, 3 hsl, 4 gradient map, 5 posterize, 6 output format

struct Params {
    c0: vec4<f32>,       // gradient map color A
    c1: vec4<f32>,       // gradient map color B
    c2: vec4<f32>,       // gradient map color C
    c3: vec4<f32>,       // gradient map color D
    f0: f32, f1: f32, f2: f32, f3: f32,
    f4: f32, f5: f32, f6: f32, f7: f32,
    op: u32, mode: u32, flag: u32, tiling: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;
@group(0) @binding(2) var out_tex: texture_storage_2d<rgba16float, write>;

// f0 in_low, f1 in_high, f2 gamma, f3 out_low, f4 out_high, flag = clamp
fn levels_channel(x: f32) -> f32 {
    let range = P.f1 - P.f0;
    var t = select((x - P.f0) / range, select(0.0, 1.0, x >= P.f0), abs(range) < 1e-6);
    if (P.flag != 0u) { t = clamp(t, 0.0, 1.0); }
    let g = max(P.f2, 1e-3);
    t = sign(t) * pow(abs(t), 1.0 / g);
    var v = P.f3 + t * (P.f4 - P.f3);
    if (P.flag != 0u) { v = clamp(v, 0.0, 1.0); }
    return v;
}

fn levels(c: vec4<f32>) -> vec4<f32> {
    // mode: 0 RGB, 1 RGBA, 2 R, 3 G, 4 B, 5 A
    var m = vec4<bool>(true, true, true, false);
    switch P.mode {
        case 1u: { m = vec4<bool>(true, true, true, true); }
        case 2u: { m = vec4<bool>(true, false, false, false); }
        case 3u: { m = vec4<bool>(false, true, false, false); }
        case 4u: { m = vec4<bool>(false, false, true, false); }
        case 5u: { m = vec4<bool>(false, false, false, true); }
        default: {}
    }
    let l = vec4<f32>(levels_channel(c.r), levels_channel(c.g), levels_channel(c.b), levels_channel(c.a));
    return select(c, l, m);
}

fn grayscale(c: vec4<f32>) -> vec4<f32> {
    var v = 0.0;
    switch P.mode {
        case 0u: { v = luminance(c.rgb); }
        case 1u: { v = (c.r + c.g + c.b) / 3.0; }
        case 2u: { v = c.r; }
        case 3u: { v = c.g; }
        case 4u: { v = c.b; }
        case 5u: { v = c.a; }
        default: { v = max(c.r, max(c.g, c.b)); }
    }
    return vec4<f32>(v, v, v, select(c.a, 1.0, P.mode == 5u));
}

fn rgb_to_hsl(c: vec3<f32>) -> vec3<f32> {
    let mx = max(c.r, max(c.g, c.b));
    let mn = min(c.r, min(c.g, c.b));
    let l = (mx + mn) * 0.5;
    let d = mx - mn;
    if (d < 1e-6) { return vec3<f32>(0.0, 0.0, l); }
    let s = d / (1.0 - abs(2.0 * l - 1.0) + 1e-6);
    var h = 0.0;
    if (mx == c.r) {
        h = (c.g - c.b) / d + select(0.0, 6.0, c.g < c.b);
    } else if (mx == c.g) {
        h = (c.b - c.r) / d + 2.0;
    } else {
        h = (c.r - c.g) / d + 4.0;
    }
    return vec3<f32>(h / 6.0, s, l);
}

fn hsl_to_rgb(hsl: vec3<f32>) -> vec3<f32> {
    let c = (1.0 - abs(2.0 * hsl.z - 1.0)) * hsl.y;
    let h = fract(hsl.x) * 6.0;
    let x = c * (1.0 - abs(h % 2.0 - 1.0));
    var rgb = vec3<f32>(0.0);
    if (h < 1.0) { rgb = vec3<f32>(c, x, 0.0); }
    else if (h < 2.0) { rgb = vec3<f32>(x, c, 0.0); }
    else if (h < 3.0) { rgb = vec3<f32>(0.0, c, x); }
    else if (h < 4.0) { rgb = vec3<f32>(0.0, x, c); }
    else if (h < 5.0) { rgb = vec3<f32>(x, 0.0, c); }
    else { rgb = vec3<f32>(c, 0.0, x); }
    return rgb + (hsl.z - c * 0.5);
}

// f0 hue shift (turns), f1 saturation (-1..1), f2 lightness (-1..1)
fn hsl_adjust(c: vec4<f32>) -> vec4<f32> {
    var hsl = rgb_to_hsl(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0)));
    hsl.x = hsl.x + P.f0;
    hsl.y = clamp(hsl.y * (1.0 + P.f1), 0.0, 1.0);
    if (P.f2 >= 0.0) {
        hsl.z = mix(hsl.z, 1.0, P.f2);
    } else {
        hsl.z = mix(hsl.z, 0.0, -P.f2);
    }
    return vec4<f32>(hsl_to_rgb(hsl), c.a);
}

// mode: 0 = 2 stops, 1 = 3 stops, 2 = 4 stops; f0 = position B, f1 = position C
fn gradient_map(c: vec4<f32>) -> vec4<f32> {
    let t = clamp(luminance(c.rgb), 0.0, 1.0);
    var col = vec4<f32>(0.0);
    if (P.mode == 0u) {
        col = mix(P.c0, P.c3, t);
    } else if (P.mode == 1u) {
        let pb = clamp(P.f0, 1e-4, 1.0 - 1e-4);
        col = select(mix(P.c1, P.c3, (t - pb) / (1.0 - pb)), mix(P.c0, P.c1, t / pb), t < pb);
    } else {
        let pb = clamp(min(P.f0, P.f1), 1e-4, 1.0 - 2e-4);
        let pc = clamp(max(P.f0, P.f1), pb + 1e-4, 1.0 - 1e-4);
        if (t < pb) {
            col = mix(P.c0, P.c1, t / pb);
        } else if (t < pc) {
            col = mix(P.c1, P.c2, (t - pb) / (pc - pb));
        } else {
            col = mix(P.c2, P.c3, (t - pc) / (1.0 - pc));
        }
    }
    return vec4<f32>(col.rgb, c.a * col.a);
}

// f0 = steps
fn posterize(c: vec4<f32>) -> vec4<f32> {
    let n = max(P.f0, 2.0);
    let q = min(floor(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * n), vec3<f32>(n - 1.0)) / (n - 1.0);
    return vec4<f32>(q, c.a);
}

// mode: 0 RGBA, 1 RGB, 2 RG, 3 R, 4 G, 5 B, 6 A. Single channels are shown as gray.
fn output_format(c: vec4<f32>) -> vec4<f32> {
    switch P.mode {
        case 1u: { return vec4<f32>(c.rgb, 1.0); }
        case 2u: { return vec4<f32>(c.r, c.g, 0.0, 1.0); }
        case 3u: { return gray(c.r); }
        case 4u: { return gray(c.g); }
        case 5u: { return gray(c.b); }
        case 6u: { return gray(c.a); }
        default: { return c; }
    }
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }
    let c = read_input(in0, gid.xy, size, P.tiling);
    var o = c;
    switch P.op {
        case 0u: { o = levels(c); }
        case 1u: { o = vec4<f32>(1.0 - c.rgb, select(c.a, 1.0 - c.a, P.flag != 0u)); }
        case 2u: { o = grayscale(c); }
        case 3u: { o = hsl_adjust(c); }
        case 4u: { o = gradient_map(c); }
        case 5u: { o = posterize(c); }
        default: { o = output_format(c); }
    }
    textureStore(out_tex, vec2<i32>(gid.xy), o);
}
