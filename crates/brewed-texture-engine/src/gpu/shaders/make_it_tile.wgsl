// Make It Tile: an image that does not tile, made to. Four copies of it, shifted by half the
// image on neither axis, one, the other and both, each shown where its own seams are not: the
// copy that is not shifted in the middle, the ones shifted across the edges (where they run
// on across the seam of the tiling), and each giving way to the next at a quarter of the image,
// over the blend width.

struct Params {
    width: f32, tiling: u32, pad0: u32, pad1: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;

// How much the copies shifted on an axis show at `t` along it: all of them near the edges,
// none in the middle.
fn shifted(t: f32) -> f32 {
    let from_middle = abs(t - 0.5);
    let w = clamp(P.width, 0.001, 0.249);
    return smoothstep(0.25 - w, 0.25 + w, from_middle);
}

fn copy(uv: vec2<f32>, shift: vec2<f32>) -> vec4<f32> {
    // The image read around as if it tiled: its own seams are where its copy does not show.
    return widen(sample_bilinear(in0, fract(uv + shift), 3u), GRAY0);
}

@fragment
fn main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    let uv = out_uv(gid.xy, size);
    let wx = shifted(uv.x);
    let wy = shifted(uv.y);
    return copy(uv, vec2<f32>(0.0, 0.0)) * (1.0 - wx) * (1.0 - wy)
        + copy(uv, vec2<f32>(0.5, 0.0)) * wx * (1.0 - wy)
        + copy(uv, vec2<f32>(0.0, 0.5)) * (1.0 - wx) * wy
        + copy(uv, vec2<f32>(0.5, 0.5)) * wx * wy;
}
