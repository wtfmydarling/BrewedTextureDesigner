// Gradient Dynamic: a gradient map whose gradient is an image. Each value picks the color at
// its place along a row (or a column) of the Gradient image: the row at Position, or, with a
// Position Map, the one it says for the pixel. The gradient is read clamped: its ends are its
// ends, whatever the node's tiling.

struct Params {
    position: f32, vertical: u32, mapped: u32, tiling: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;
@group(0) @binding(2) var gradient: texture_2d<f32>;
@group(0) @binding(3) var position_map: texture_2d<f32>;

@fragment
fn main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    let value = clamp(luminance(widen(read_input(in0, gid.xy, size, P.tiling), gray_input(0u)).rgb), 0.0, 1.0);
    var position = P.position;
    if (P.mapped != 0u) {
        position = luminance(widen(read_input(position_map, gid.xy, size, P.tiling), gray_input(2u)).rgb);
    }
    position = clamp(position, 0.0, 1.0);
    let uv = select(vec2<f32>(value, position), vec2<f32>(position, value), P.vertical != 0u);
    return widen(sample_bilinear(gradient, uv, 0u), gray_input(1u));
}
