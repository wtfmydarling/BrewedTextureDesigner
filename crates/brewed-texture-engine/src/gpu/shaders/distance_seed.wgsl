// The first pass of a distance field (see distance.wgsl): the pixels the distances are to,
// each saying where it is, the others nothing.

struct Params {
    step: i32, tiling: u32, inside: u32, flip: u32,
    threshold: f32, max_distance: f32, profile: u32, pad0: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;

// A pixel as a seed: its place, plus one (none is zero). Places fit in 16 bits each: an image
// is 8192 wide at most.
fn seed_of(p: vec2<u32>) -> u32 {
    return p.x + (p.y << 16u) + 1u;
}

@fragment
fn seed(@builtin(position) at: vec4<f32>) -> @location(0) vec4<u32> {
    let gid = pixel(at);
    let size = target_size();
    // (A color mask reads as its luminance.)
    let v = luminance(widen(read_input(in0, gid.xy, size, P.tiling), gray_input(0u)).rgb);
    // `inside`: the pixels of the shapes are the seeds (distances from them); else those
    // around them (distances into them).
    let is_in = v >= P.threshold;
    if (is_in == (P.inside == 1u)) {
        return vec4<u32>(seed_of(gid.xy), 0u, 0u, 0u);
    }
    return vec4<u32>(0u);
}

// The first pass of a flood fill (see flood.wgsl): each pixel of the shapes labelled with its
// own place (its index, plus one), the others with nothing.
@fragment
fn label(@builtin(position) at: vec4<f32>) -> @location(0) vec4<u32> {
    let gid = pixel(at);
    let size = target_size();
    let v = luminance(widen(read_input(in0, gid.xy, size, P.tiling), gray_input(0u)).rgb);
    if (v >= P.threshold) {
        return vec4<u32>(gid.x + gid.y * size.x + 1u, 0u, 0u, 0u);
    }
    return vec4<u32>(0u);
}

// The first pass of the runs of a flood fill (see runs.wgsl): a pixel outside the shapes is
// no distance from the outside; one of the shapes does not know yet.
@fragment
fn runs_seed(@builtin(position) at: vec4<f32>) -> @location(0) vec4<u32> {
    let gid = pixel(at);
    let size = target_size();
    let v = luminance(widen(read_input(in0, gid.xy, size, P.tiling), gray_input(0u)).rgb);
    if (v >= P.threshold) {
        return vec4<u32>(65535u);
    }
    return vec4<u32>(0u);
}
