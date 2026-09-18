struct Params {
    intensity: f32, tiling: u32, pad0: u32, pad1: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var src_tex: texture_2d<f32>;
@group(0) @binding(2) var blurred_tex: texture_2d<f32>;
@group(0) @binding(3) var out_tex: texture_storage_2d<rgba16float, write>;

// Unsharp mask: add back the difference between the image and its blurred copy.
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }
    let s = read_input(src_tex, gid.xy, size, P.tiling);
    let b = read_input(blurred_tex, gid.xy, size, P.tiling);
    let rgb = clamp(s.rgb + (s.rgb - b.rgb) * P.intensity, vec3<f32>(0.0), vec3<f32>(1.0));
    textureStore(out_tex, vec2<i32>(gid.xy), vec4<f32>(rgb, s.a));
}
