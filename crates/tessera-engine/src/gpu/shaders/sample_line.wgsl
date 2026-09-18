// Samples an image along a line (for picking a gradient in the 2D view). The output is
// an N x 1 image; texel i holds the color at t = i / (N - 1).

struct Params {
    from_x: f32, from_y: f32, to_x: f32, to_y: f32,
    tiling: u32, pad0: u32, pad1: u32, pad2: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;
@group(0) @binding(2) var out_tex: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }
    let t = f32(gid.x) / f32(max(size.x, 2u) - 1u);
    let uv = mix(vec2<f32>(P.from_x, P.from_y), vec2<f32>(P.to_x, P.to_y), t);
    textureStore(out_tex, vec2<i32>(gid.xy), sample_bilinear(in0, uv, P.tiling));
}
