// Normal Combine: the details of one normal map on another. Reoriented (RNM): the detail map
// turned to follow the base, as if bent over it; Whiteout: slopes added, heights multiplied;
// UDN: slopes added. A mask says where the detail goes. An input nothing arrives at is flat.

struct Params {
    intensity: f32, technique: u32, masked: u32, tiling: u32,
    base: u32, detail: u32, pad0: u32, pad1: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var in0: texture_2d<f32>;
@group(0) @binding(2) var in1: texture_2d<f32>;
@group(0) @binding(3) var in2: texture_2d<f32>;

fn unpacked(c: vec4<f32>, connected: u32) -> vec3<f32> {
    if (connected == 0u) {
        return vec3<f32>(0.0, 0.0, 1.0);
    }
    let n = c.rgb * 2.0 - 1.0;
    let l = length(n);
    return select(vec3<f32>(0.0, 0.0, 1.0), n / l, l > 1e-5);
}

@fragment
fn main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    let n1 = unpacked(widen(read_input(in0, gid.xy, size, P.tiling), GRAY0), P.base);
    var n2 = unpacked(widen(read_input(in1, gid.xy, size, P.tiling), GRAY1), P.detail);
    n2 = normalize(vec3<f32>(n2.xy * max(P.intensity, 0.0), max(n2.z, 1e-4)));
    var n: vec3<f32>;
    switch P.technique {
        case 1u: { n = vec3<f32>(n1.xy + n2.xy, n1.z * n2.z); }
        case 2u: { n = vec3<f32>(n1.xy + n2.xy, n1.z); }
        default: {
            let t = n1 + vec3<f32>(0.0, 0.0, 1.0);
            let u = n2 * vec3<f32>(-1.0, -1.0, 1.0);
            n = t * dot(t, u) / max(t.z, 1e-4) - u;
        }
    }
    n = normalize(n);
    if (P.masked == 1u) {
        let m = clamp(luminance(widen(read_input(in2, gid.xy, size, P.tiling), GRAY2).rgb), 0.0, 1.0);
        n = normalize(mix(n1, n, m));
    }
    return vec4<f32>(n * 0.5 + 0.5, 1.0);
}
