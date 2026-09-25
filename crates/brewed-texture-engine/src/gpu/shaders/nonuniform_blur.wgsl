// Non-uniform Blur: each pixel blurred as much as a map says, up to the radius. The input
// comes with its mips: sixteen samples spread over a disc of the pixel's radius, each read
// from between the levels where a texel is about as wide as the gap between them (see
// `trilinear`: in the precision of the input, whatever it is), weighted as a Gaussian. The
// cost is the same for any radius.

struct Params {
    radius_x: f32, radius_y: f32, tiling: u32, mapped: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;
@group(0) @binding(2) var in1: texture_2d<f32>;

const TAPS: u32 = 16u;
const GOLDEN: f32 = 2.39996323;

@fragment
fn main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    let uv = out_uv(gid.xy, size);
    var amount = 1.0;
    if (P.mapped == 1u) {
        amount = clamp(luminance(widen(read_input(in1, gid.xy, size, P.tiling), GRAY1).rgb), 0.0, 1.0);
    }
    let radius = vec2<f32>(P.radius_x, P.radius_y) * amount;
    let pixels = radius.x * f32(textureDimensions(in0, 0).x);
    if (pixels < 0.5) {
        return widen(sample_bilinear(in0, uv, P.tiling), GRAY0);
    }
    // Sixteen samples fill a disc of the radius: a gap of about half the radius between them.
    let level = log2(max(pixels * 0.5, 1.0));
    var sum = vec4<f32>(0.0);
    var weights = 0.0;
    for (var i = 0u; i < TAPS; i++) {
        let d = sqrt((f32(i) + 0.5) / f32(TAPS));
        let a = f32(i) * GOLDEN;
        let w = exp(-2.0 * d * d);
        sum = sum + widen(trilinear(in0, uv + vec2<f32>(cos(a), sin(a)) * d * radius, level, P.tiling), GRAY0) * w;
        weights = weights + w;
    }
    return sum / weights;
}
