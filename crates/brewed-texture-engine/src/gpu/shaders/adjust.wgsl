// Per-pixel adjustments that share one parameter block. `op` selects the operation:
//   0 levels, 1 invert, 2 grayscale, 3 hsl, 4 histogram scan, 5 posterize

struct Params {
    f0: f32, f1: f32, f2: f32, f3: f32,
    f4: f32, f5: f32, f6: f32, f7: f32,
    op: u32, mode: u32, flag: u32, tiling: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;

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

// f0 position (input level that becomes mid gray), f1 contrast, flag = invert.
// A linear ramp of width 1 - contrast centered on the position; contrast 1 thresholds.
fn histogram_scan(c: vec4<f32>) -> vec4<f32> {
    let x = luminance(c.rgb);
    let w = max(1.0 - clamp(P.f1, 0.0, 1.0), 1e-4);
    var t = clamp((x - (P.f0 - 0.5 * w)) / w, 0.0, 1.0);
    if (P.flag != 0u) { t = 1.0 - t; }
    return gray(t);
}

// f0 = steps
fn posterize(c: vec4<f32>) -> vec4<f32> {
    let n = max(P.f0, 2.0);
    let q = min(floor(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * n), vec3<f32>(n - 1.0)) / (n - 1.0);
    return vec4<f32>(q, c.a);
}

@fragment
fn main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    let c = widen(read_input(in0, gid.xy, size, P.tiling), GRAY0);
    var o = c;
    switch P.op {
        case 0u: { o = levels(c); }
        case 1u: { o = vec4<f32>(1.0 - c.rgb, select(c.a, 1.0 - c.a, P.flag != 0u)); }
        case 2u: { o = grayscale(c); }
        case 3u: { o = hsl_adjust(c); }
        case 4u: { o = histogram_scan(c); }
        default: { o = posterize(c); }
    }
    return o;
}
