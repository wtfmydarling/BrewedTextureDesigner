struct Params {
    opacity: f32, mode: u32, ignore_alpha: u32, clamp_result: u32,
    tiling: u32, pad0: u32, pad1: u32, pad2: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var fg_tex: texture_2d<f32>;
@group(0) @binding(2) var bg_tex: texture_2d<f32>;
@group(0) @binding(3) var mask_tex: texture_2d<f32>;

fn dodge(a: f32, b: f32) -> f32 {
    if (b >= 1.0) { return 1.0; }
    return min(1.0, a / (1.0 - b));
}

fn burn(a: f32, b: f32) -> f32 {
    if (b <= 0.0) { return 0.0; }
    return 1.0 - min(1.0, (1.0 - a) / b);
}

fn soft_light(a: f32, b: f32) -> f32 {
    if (b <= 0.5) {
        return a - (1.0 - 2.0 * b) * a * (1.0 - a);
    }
    let d = select(sqrt(max(a, 0.0)), ((16.0 * a - 12.0) * a + 4.0) * a, a <= 0.25);
    return a + (2.0 * b - 1.0) * (d - a);
}

// a = background (base), b = foreground (blend layer)
fn blend_channel(mode: u32, a: f32, b: f32) -> f32 {
    switch mode {
        case 0u: { return b; }
        case 1u: { return a + b; }
        case 2u: { return a - b; }
        case 3u: { return a * b; }
        case 4u: { return 1.0 - (1.0 - a) * (1.0 - b); }
        case 5u: { return select(1.0 - 2.0 * (1.0 - a) * (1.0 - b), 2.0 * a * b, a < 0.5); }
        case 6u: { return soft_light(a, b); }
        case 7u: { return select(1.0 - 2.0 * (1.0 - a) * (1.0 - b), 2.0 * a * b, b < 0.5); }
        case 8u: { return min(a, b); }
        case 9u: { return max(a, b); }
        case 10u: { return abs(a - b); }
        case 11u: { return a + b - 2.0 * a * b; }
        case 12u: { return a / max(b, 1e-5); }
        case 13u: { return dodge(a, b); }
        case 14u: { return burn(a, b); }
        case 15u: { return a + b - 1.0; }
        case 16u: { return a + 2.0 * b - 1.0; }
        case 17u: { return select(dodge(a, 2.0 * (b - 0.5)), burn(a, 2.0 * b), b < 0.5); }
        case 18u: { return select(max(a, 2.0 * (b - 0.5)), min(a, 2.0 * b), b < 0.5); }
        default: { return b; }
    }
}

@fragment
fn main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    let fg = widen(read_input(fg_tex, gid.xy, size, P.tiling), gray_input(0u));
    let bg = widen(read_input(bg_tex, gid.xy, size, P.tiling), gray_input(1u));
    // (A color mask, as every grayscale input reads one: its luminance.)
    let mask = luminance(widen(read_input(mask_tex, gid.xy, size, P.tiling), gray_input(2u)).rgb);

    let blended = vec3<f32>(
        blend_channel(P.mode, bg.r, fg.r),
        blend_channel(P.mode, bg.g, fg.g),
        blend_channel(P.mode, bg.b, fg.b),
    );
    var t = clamp(P.opacity * mask, 0.0, 1.0);
    var alpha = bg.a;
    if (P.ignore_alpha == 0u) {
        t = t * fg.a;
        alpha = bg.a + t * (1.0 - bg.a);
    }
    var rgb = mix(bg.rgb, blended, t);
    if (P.clamp_result != 0u) {
        rgb = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));
        alpha = clamp(alpha, 0.0, 1.0);
    }
    return vec4<f32>(rgb, alpha);
}
