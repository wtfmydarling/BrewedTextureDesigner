struct Params {
    intensity: f32, tiling: u32, pad0: u32, pad1: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;
@group(0) @binding(2) var warp_tex: texture_2d<f32>;

fn height(uv: vec2<f32>) -> f32 {
    return luminance(widen(sample_bilinear(warp_tex, uv, P.tiling), GRAY1).rgb);
}

@fragment
fn main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    let uv = out_uv(gid.xy, size);
    // Gradient of the warp map in uv units (resolution independent).
    let d = 1.0 / vec2<f32>(max(textureDimensions(warp_tex), vec2<u32>(2u)));
    let gx = (height(uv + vec2<f32>(d.x, 0.0)) - height(uv - vec2<f32>(d.x, 0.0))) / (2.0 * d.x);
    let gy = (height(uv + vec2<f32>(0.0, d.y)) - height(uv - vec2<f32>(0.0, d.y))) / (2.0 * d.y);
    let src = uv + vec2<f32>(gx, gy) * P.intensity * 0.02;
    return widen(sample_bilinear(in0, src, P.tiling), GRAY0);
}
