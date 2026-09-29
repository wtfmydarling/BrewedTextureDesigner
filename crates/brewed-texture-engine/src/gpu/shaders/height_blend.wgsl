// Height Blend: two height maps, the higher one showing where it is higher. The top one is
// raised by the offset; the contrast sharpens where one gives way to the other. It gives the
// height, or where the top one shows (its mask).

struct Params {
    offset: f32, contrast: f32, output: u32, masked: u32,
    tiling: u32, pad0: u32, pad1: u32, pad2: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;
@group(0) @binding(2) var in1: texture_2d<f32>;
@group(0) @binding(3) var in2: texture_2d<f32>;

@fragment
fn main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    let top = luminance(widen(read_input(in0, gid.xy, size, P.tiling), gray_input(0u)).rgb) + P.offset;
    let bottom = luminance(widen(read_input(in1, gid.xy, size, P.tiling), gray_input(1u)).rgb);
    // How far above the other the top one has to be to show fully.
    let width = mix(0.25, 0.0005, clamp(P.contrast, 0.0, 1.0));
    var shown = smoothstep(-width, width, top - bottom);
    if (P.masked == 1u) {
        shown = shown * clamp(luminance(widen(read_input(in2, gid.xy, size, P.tiling), gray_input(2u)).rgb), 0.0, 1.0);
    }
    let v = select(mix(bottom, top, shown), shown, P.output == 1u);
    return vec4<f32>(v, v, v, 1.0);
}
