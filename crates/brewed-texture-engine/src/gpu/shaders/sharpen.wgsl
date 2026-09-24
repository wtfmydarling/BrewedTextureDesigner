struct Params {
    intensity: f32, tiling: u32, pad0: u32, pad1: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var src_tex: texture_2d<f32>;
@group(0) @binding(2) var blurred_tex: texture_2d<f32>;

// Unsharp mask: add back the difference between the image and its blurred copy.
@fragment
fn main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    let s = widen(read_input(src_tex, gid.xy, size, P.tiling), GRAY0);
    let b = widen(read_input(blurred_tex, gid.xy, size, P.tiling), GRAY1);
    let rgb = clamp(s.rgb + (s.rgb - b.rgb) * P.intensity, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(rgb, s.a);
}
