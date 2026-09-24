// Ambient occlusion of a height field, horizon based.
//
// The input comes with its whole mip chain and a trilinear sampler, so that a sample far
// from the pixel reads the whole patch of ground it stands for in one fetch: each sample
// covers its slice of the circle — as wide as the gap to the next direction, as long as
// the step to the next sample. Nothing between two samples is missed, and nothing jumps
// as an edge passes from one sample to the next: no rays, no rings.

struct Params {
    radius_x: f32, radius_y: f32, intensity: f32, depth: f32,
    invert: u32, tiling: u32, pad0: u32, pad1: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;
// Trilinear, wrapping on the axes the node tiles on.
@group(0) @binding(2) var smp: sampler;

const DIRECTIONS: i32 = 16;
const STEPS: i32 = 12;
// How far (as a sine) the terrain has to rise above the surface itself to occlude: keeps
// gentle bumps clean.
const BIAS: f32 = 0.05;

// The height around `uv`, averaged over a patch about `width` pixels of the image across.
fn height_over(uv: vec2<f32>, width: f32) -> f32 {
    return luminance(widen(textureSampleLevel(in0, smp, uv, log2(max(width, 1.0))), GRAY0).rgb);
}

@fragment
fn main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    let uv = out_uv(gid.xy, size);
    // Distances are in image widths, the unit the height depth is given in.
    let aspect = f32(size.x) / f32(size.y);
    let to_uv = vec2<f32>(1.0, aspect);
    let pixels = f32(textureDimensions(in0, 0).x);
    let radius = P.radius_x;
    let h0 = height_over(uv, 1.0) * P.depth;
    // The slope under the pixel: the horizon starts there, so a slope does not shade itself.
    let e = 1.0 / pixels;
    let grad = vec2<f32>(
        height_over(uv + vec2<f32>(e, 0.0) * to_uv, 1.0) - height_over(uv - vec2<f32>(e, 0.0) * to_uv, 1.0),
        height_over(uv + vec2<f32>(0.0, e) * to_uv, 1.0) - height_over(uv - vec2<f32>(0.0, e) * to_uv, 1.0),
    ) * P.depth / (2.0 * e);
    // The same steps in every direction: denser near the pixel, where the detail is.
    var dist: array<f32, STEPS>;
    var length: array<f32, STEPS>;
    var falloff: array<f32, STEPS>;
    var last = 0.0;
    for (var s = 0; s < STEPS; s++) {
        let t = pow(f32(s + 1) / f32(STEPS), 1.5);
        dist[s] = radius * t;
        length[s] = radius * (t - last);
        falloff[s] = 1.0 - t * t;
        last = t;
    }
    let gap = TAU / f32(DIRECTIONS);
    var occlusion = 0.0;
    for (var d = 0; d < DIRECTIONS; d++) {
        let a = (f32(d) + 0.5) * gap;
        let dir = vec2<f32>(cos(a), sin(a));
        let tangent = dot(grad, dir);
        var horizon = tangent / sqrt(1.0 + tangent * tangent) + BIAS;
        for (var s = 0; s < STEPS; s++) {
            let width = max(dist[s] * gap, length[s]) * pixels;
            let slope = (height_over(uv + dir * dist[s] * to_uv, width) * P.depth - h0) / dist[s];
            let elevation = slope / sqrt(1.0 + slope * slope);
            // Every rise of the horizon shades the pixel by the sine it adds, less with
            // distance.
            if (elevation > horizon) {
                occlusion = occlusion + (elevation - horizon) * falloff[s];
                horizon = elevation;
            }
        }
    }
    let ao = clamp(1.0 - occlusion / f32(DIRECTIONS) * P.intensity, 0.0, 1.0);
    return gray(ao);
}
