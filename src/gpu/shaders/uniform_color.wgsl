struct Params {
    color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var out_tex: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }
    textureStore(out_tex, vec2<i32>(gid.xy), P.color);
}
