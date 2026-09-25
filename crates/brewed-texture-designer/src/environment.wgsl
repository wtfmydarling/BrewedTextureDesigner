// Making an environment ready (see environment.rs): its image halved a mip level at a time,
// and blurred by the GGX lobe of a roughness.

struct Filter {
    size: vec2<f32>,        // of the level written
    roughness: f32,
    source_width: f32,      // of the image's largest level
    source_levels: f32,
    samples: u32,
    pad: vec2<u32>,
};

@group(0) @binding(0) var<uniform> f: Filter;
@group(0) @binding(1) var source: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

const PI: f32 = 3.14159265;

// A triangle over the whole target.
@vertex
fn full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

// The direction a point of the image looks in (`direction` in environment.rs).
fn direction(uv: vec2<f32>) -> vec3<f32> {
    let phi = (uv.x - 0.5) * 2.0 * PI;
    let theta = uv.y * PI;
    return vec3<f32>(sin(theta) * sin(phi), cos(theta), -sin(theta) * cos(phi));
}

// Where the image shows a direction.
fn equirect(d: vec3<f32>) -> vec2<f32> {
    return vec2<f32>(atan2(d.x, -d.z) / (2.0 * PI) + 0.5, acos(clamp(d.y, -1.0, 1.0)) / PI);
}

// The level above, halved: the middle of this pixel is where 2x2 pixels of that one meet,
// and filtering averages them.
@fragment
fn halve(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    return textureSampleLevel(source, samp, at.xy / f.size, 0.0);
}

fn radical_inverse(i: u32) -> f32 {
    return f32(reverseBits(i)) * 2.3283064365386963e-10;
}

// A direction of the GGX lobe of `a` around `n`, for the point `xi` of the unit square: more
// of them where the lobe is stronger (importance sampling).
fn ggx(xi: vec2<f32>, n: vec3<f32>, a: f32) -> vec3<f32> {
    let phi = 2.0 * PI * xi.x;
    let cos_t = sqrt((1.0 - xi.y) / (1.0 + (a * a - 1.0) * xi.y));
    let sin_t = sqrt(max(1.0 - cos_t * cos_t, 0.0));
    let up = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(n.y) < 0.999);
    let tx = normalize(cross(up, n));
    let ty = cross(n, tx);
    return normalize(tx * (sin_t * cos(phi)) + ty * (sin_t * sin(phi)) + n * cos_t);
}

// The light a surface of `f.roughness` reflects toward the direction it mirrors: the
// environment through the GGX lobe, with the view along the normal (as split-sum lighting
// has it). Each sample is read from the mip level whose pixels are as large as the part of
// the sphere the sample stands for: a hundred of them give a smooth result.
@fragment
fn prefilter(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = at.xy / f.size;
    let n = direction(uv);
    if (f.roughness <= 0.0) {
        // A mirror: the image itself, from its level as detailed as this one.
        let lod = max(log2(f.source_width / f.size.x), 0.0);
        return vec4<f32>(textureSampleLevel(source, samp, uv, lod).rgb, 1.0);
    }
    let a = f.roughness * f.roughness;
    let a2 = a * a;
    // The mean solid angle of a pixel of the image (twice as wide as high: the whole sphere).
    let texel = 8.0 * PI / (f.source_width * f.source_width);
    var sum = vec3<f32>(0.0);
    var weight = 0.0;
    for (var i = 0u; i < f.samples; i = i + 1u) {
        let xi = vec2<f32>(f32(i) / f32(f.samples), radical_inverse(i));
        let h = ggx(xi, n, a);
        let ndh = dot(n, h);
        let l = 2.0 * ndh * h - n;
        let ndl = dot(n, l);
        if (ndl > 0.0) {
            let d = a2 / (PI * pow(ndh * ndh * (a2 - 1.0) + 1.0, 2.0));
            // With the view along the normal, the density of `l` is D / 4.
            let solid = 4.0 / (f32(f.samples) * d + 1e-6);
            let lod = clamp(0.5 * log2(solid / texel) + 1.0, 0.0, f.source_levels - 1.0);
            sum = sum + textureSampleLevel(source, samp, equirect(l), lod).rgb * ndl;
            weight = weight + ndl;
        }
    }
    return vec4<f32>(sum / max(weight, 1e-6), 1.0);
}
