// RGBA Merge: four grayscale images into the channels of one color image; a channel with
// nothing connected has its value.

struct Params {
    red: f32, green: f32, blue: f32, alpha: f32,
    connected: u32, tiling: u32, pad0: u32, pad1: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var red_tex: texture_2d<f32>;
@group(0) @binding(2) var green_tex: texture_2d<f32>;
@group(0) @binding(3) var blue_tex: texture_2d<f32>;
@group(0) @binding(4) var alpha_tex: texture_2d<f32>;

// Channel `bit`: its image where one is connected (a color one read as its luminance), else
// its value.
fn channel(tex: texture_2d<f32>, bit: u32, gray: bool, value: f32, gid: vec2<u32>, size: vec2<u32>) -> f32 {
    if ((P.connected & (1u << bit)) == 0u) {
        return value;
    }
    return luminance(widen(read_input(tex, gid, size, P.tiling), gray).rgb);
}

@fragment
fn main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at).xy;
    let size = target_size();
    return vec4<f32>(
        channel(red_tex, 0u, GRAY0, P.red, gid, size),
        channel(green_tex, 1u, GRAY1, P.green, gid, size),
        channel(blue_tex, 2u, GRAY2, P.blue, gid, size),
        channel(alpha_tex, 3u, GRAY3, P.alpha, gid, size),
    );
}
