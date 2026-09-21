// Emboss: lights the height input with a directional light. Mode 0 outputs the shading
// (0.5 = flat), mode 1 multiplies the color input (unchanged where flat).

struct Params {
    light_x: f32, light_y: f32, light_z: f32, intensity: f32,
    mode: u32, tiling: u32, pad0: u32, pad1: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var height_tex: texture_2d<f32>;
@group(0) @binding(2) var color_tex: texture_2d<f32>;
@group(0) @binding(3) var out_tex: texture_storage_2d<rgba16float, write>;

fn h(p: vec2<i32>) -> f32 {
    return luminance(widen(fetch(height_tex, p, P.tiling), GRAY0).rgb);
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }
    let in_size = vec2<f32>(textureDimensions(height_tex));
    let p = vec2<i32>((vec2<f32>(gid.xy) + 0.5) / vec2<f32>(size) * in_size);
    let dx = (h(p + vec2<i32>(1, 0)) - h(p - vec2<i32>(1, 0))) * 0.5 * in_size.x / 1024.0;
    let dy = (h(p + vec2<i32>(0, 1)) - h(p - vec2<i32>(0, 1))) * 0.5 * in_size.y / 1024.0;
    // Screen y points down; the normal's y points up like in the Normal node (OpenGL).
    let n = normalize(vec3<f32>(-dx * P.intensity, dy * P.intensity, 1.0));
    let light = normalize(vec3<f32>(P.light_x, P.light_y, P.light_z));
    let shade = max(dot(n, light), 0.0) / max(light.z, 1e-3);
    if (P.mode == 1u) {
        let c = widen(read_input(color_tex, gid.xy, size, P.tiling), GRAY1);
        textureStore(out_tex, vec2<i32>(gid.xy), vec4<f32>(c.rgb * shade, c.a));
    } else {
        textureStore(out_tex, vec2<i32>(gid.xy), gray(clamp(0.5 * shade, 0.0, 1.0)));
    }
}
