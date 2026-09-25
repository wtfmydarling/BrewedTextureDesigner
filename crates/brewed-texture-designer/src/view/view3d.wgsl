// The 3D view (view3d.rs): the model, lit by the studio or by an environment image, and the
// image behind it.

struct U {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    light: vec4<f32>,
    params: vec4<f32>,      // displacement, texture repeat, exposure (a factor), _
    flags: vec4<u32>,       // present maps (bits), sRGB target, gray maps (bits), lit by an environment image
    turn: vec4<f32>,        // cosine and sine of how far the environment image is turned
    levels: vec4<f32>,      // the level of the image behind the model, the top level of the glossy light, _, _
    right: vec4<f32>,       // the camera's axes, reaching to the edges of the view
    up: vec4<f32>,
    forward: vec4<f32>,
    sh: array<vec4<f32>, 9>, // the environment image's irradiance (environment.rs)
};

@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var base_map: texture_2d<f32>;
@group(0) @binding(2) var normal_map: texture_2d<f32>;
@group(0) @binding(3) var height_map: texture_2d<f32>;
@group(0) @binding(4) var rough_map: texture_2d<f32>;
@group(0) @binding(5) var metal_map: texture_2d<f32>;
@group(0) @binding(6) var ao_map: texture_2d<f32>;
@group(0) @binding(7) var emissive_map: texture_2d<f32>;
@group(0) @binding(8) var samp: sampler;
@group(0) @binding(9) var env_image: texture_2d<f32>;
@group(0) @binding(10) var env_glossy: texture_2d<f32>;
@group(0) @binding(11) var env_samp: sampler;

const PI: f32 = 3.14159265;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) uv: vec2<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) uv: vec2<f32>,
};

fn has(bit: u32) -> bool {
    return (u.flags.x & (1u << bit)) != 0u;
}

// A map as its output writes it (a map of one channel is gray), from 0 to 1.
fn read_map(c: vec4<f32>, bit: u32) -> vec4<f32> {
    let gray = (u.flags.z & (1u << bit)) != 0u;
    return clamp(widen(c, gray), vec4<f32>(0.0), vec4<f32>(1.0));
}

@vertex
fn vs(v: VIn) -> VOut {
    let uv = v.uv * u.params.y;
    var p = v.pos;
    if (has(2u)) {
        let h = read_map(textureSampleLevel(height_map, samp, uv, 0.0), 2u).r;
        p = p + v.normal * h * u.params.x;
    }
    var o: VOut;
    o.clip = u.view_proj * vec4<f32>(p, 1.0);
    o.world = p;
    o.normal = v.normal;
    o.tangent = v.tangent;
    o.uv = uv;
    return o;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Light as the screen shows it: tone mapped, and in sRGB where the target does not make it so.
fn on_screen(c: vec3<f32>) -> vec4<f32> {
    var color = aces(c);
    if (u.flags.y == 0u) {
        color = linear_to_srgb(color);
    }
    return vec4<f32>(color, 1.0);
}

// The studio's sky.
fn environment(dir: vec3<f32>) -> vec3<f32> {
    let sky = vec3<f32>(0.55, 0.62, 0.75);
    let ground = vec3<f32>(0.16, 0.14, 0.12);
    let t = clamp(dir.y * 0.5 + 0.5, 0.0, 1.0);
    return mix(ground, sky, t * t * (3.0 - 2.0 * t));
}

// A direction of the world as the environment image has it: turned back as far as the image
// is turned.
fn turned(d: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(u.turn.x * d.x + u.turn.y * d.z, d.y, u.turn.x * d.z - u.turn.y * d.x);
}

// Where an equirectangular image shows a direction (environment.rs: `direction`, backwards).
fn equirect(d: vec3<f32>) -> vec2<f32> {
    return vec2<f32>(atan2(d.x, -d.z) / (2.0 * PI) + 0.5, acos(clamp(d.y, -1.0, 1.0)) / PI);
}

// The light a surface facing `n` gets from the environment image (environment.rs: `irradiance`).
fn irradiance(n: vec3<f32>) -> vec3<f32> {
    let e = u.sh[0].rgb + u.sh[1].rgb * n.y + u.sh[2].rgb * n.z + u.sh[3].rgb * n.x
        + u.sh[4].rgb * (n.x * n.y) + u.sh[5].rgb * (n.y * n.z) + u.sh[6].rgb * (3.0 * n.z * n.z - 1.0)
        + u.sh[7].rgb * (n.x * n.z) + u.sh[8].rgb * (n.x * n.x - n.y * n.y);
    return max(e, vec3<f32>(0.0));
}

// How much of the light around it a glossy surface reflects toward the eye: Fresnel and
// shadowing over the GGX lobe (Karis's fit of the second part of split-sum lighting).
fn env_brdf(f0: vec3<f32>, rough: f32, ndv: f32) -> vec3<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = rough * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * ndv)) * r.x + r.y;
    let ab = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    return f0 * ab.x + ab.y;
}

// Lit by the environment image: its irradiance on the diffuse part, its glossy light, blurred
// as far as the surface is rough, on the specular one.
fn image_light(n: vec3<f32>, v: vec3<f32>, albedo: vec3<f32>, rough: f32, metal: f32) -> vec3<f32> {
    let ndv = max(dot(n, v), 1e-4);
    let f0 = mix(vec3<f32>(0.04), albedo, metal);
    let specular = env_brdf(f0, rough, ndv);
    let r = reflect(-v, n);
    let glossy = textureSampleLevel(env_glossy, env_samp, equirect(turned(r)), rough * u.levels.y).rgb;
    let diffuse = irradiance(turned(n)) * albedo / PI * (1.0 - metal);
    return diffuse * (vec3<f32>(1.0) - specular) + glossy * specular;
}

fn brdf(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, albedo: vec3<f32>, rough: f32, metal: f32) -> vec3<f32> {
    let h = normalize(v + l);
    let ndl = max(dot(n, l), 0.0);
    let ndv = max(dot(n, v), 1e-4);
    let ndh = max(dot(n, h), 0.0);
    let vdh = max(dot(v, h), 0.0);
    let a = max(rough * rough, 0.002);
    let a2 = a * a;
    let d = a2 / (PI * pow(ndh * ndh * (a2 - 1.0) + 1.0, 2.0));
    let k = (rough + 1.0) * (rough + 1.0) / 8.0;
    let g = (ndl / (ndl * (1.0 - k) + k)) * (ndv / (ndv * (1.0 - k) + k));
    let f0 = mix(vec3<f32>(0.04), albedo, metal);
    let f = f0 + (1.0 - f0) * pow(1.0 - vdh, 5.0);
    let spec = d * g * f / max(4.0 * ndl * ndv, 1e-4);
    let kd = (1.0 - f) * (1.0 - metal);
    return (kd * albedo / PI + spec) * ndl;
}

@fragment
fn fs(i: VOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    var albedo = vec3<f32>(0.72);
    if (has(0u)) {
        albedo = srgb_to_linear(read_map(textureSample(base_map, samp, i.uv), 0u).rgb);
    }
    var n = normalize(i.normal);
    if (has(1u)) {
        let t = normalize(i.tangent.xyz - n * dot(n, i.tangent.xyz));
        let b = cross(n, t) * i.tangent.w;
        let m = read_map(textureSample(normal_map, samp, i.uv), 1u).rgb * 2.0 - 1.0;
        // OpenGL normal maps: +y points up in the texture, i.e. against v.
        n = normalize(t * m.x + b * m.y + n * m.z);
    }
    // Seen from behind (the plane, the cloth), a surface faces the other way.
    if (!front) {
        n = -n;
    }
    var rough = 0.5;
    if (has(3u)) { rough = max(read_map(textureSample(rough_map, samp, i.uv), 3u).r, 0.02); }
    var metal = 0.0;
    if (has(4u)) { metal = read_map(textureSample(metal_map, samp, i.uv), 4u).r; }
    var ao = 1.0;
    if (has(5u)) { ao = read_map(textureSample(ao_map, samp, i.uv), 5u).r; }
    var emissive = vec3<f32>(0.0);
    if (has(6u)) { emissive = srgb_to_linear(read_map(textureSample(emissive_map, samp, i.uv), 6u).rgb); }

    let v = normalize(u.eye.xyz - i.world);
    var color: vec3<f32>;
    if (u.flags.w != 0u) {
        color = image_light(n, v, albedo, rough, metal) * ao;
    } else {
        let key = normalize(u.light.xyz);
        let fill = normalize(vec3<f32>(-key.x, 0.3, -key.z));
        color = brdf(n, v, key, albedo, rough, metal) * vec3<f32>(3.2, 3.05, 2.85);
        color = color + brdf(n, v, fill, albedo, rough, metal) * vec3<f32>(0.45, 0.5, 0.6);
        // Ambient: diffuse from the hemisphere, specular from the reflected direction.
        let f0 = mix(vec3<f32>(0.04), albedo, metal);
        let ndv = max(dot(n, v), 1e-4);
        let fr = f0 + (max(vec3<f32>(1.0 - rough), f0) - f0) * pow(1.0 - ndv, 5.0);
        let r = reflect(-v, n);
        color = color + (environment(n) * albedo * (1.0 - metal) * 0.35 + environment(r) * fr * (1.0 - rough * 0.6) * 0.6) * ao;
    }
    return on_screen((color + emissive) * u.params.z);
}

struct BOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) at: vec2<f32>,
};

// A triangle over the whole view, for the environment image behind the model.
@vertex
fn behind_vs(@builtin(vertex_index) i: u32) -> BOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    var o: BOut;
    o.clip = vec4<f32>(p, 0.5, 1.0);
    o.at = p;
    return o;
}

// The environment image at a mip level, through a cubic B-spline: four filtered reads, which
// smooth away the blocks the mips are made of.
fn bspline(uv: vec2<f32>, level: f32) -> vec3<f32> {
    let size = vec2<f32>(textureDimensions(env_image, u32(level)));
    let p = uv * size - 0.5;
    let i = floor(p);
    let f = p - i;
    let f2 = f * f;
    let f3 = f2 * f;
    let w0 = (1.0 - 3.0 * f + 3.0 * f2 - f3) / 6.0;
    let w1 = (4.0 - 6.0 * f2 + 3.0 * f3) / 6.0;
    let w2 = (1.0 + 3.0 * f + 3.0 * f2 - 3.0 * f3) / 6.0;
    let w3 = f3 / 6.0;
    let g0 = w0 + w1;
    let g1 = w2 + w3;
    // Each pair of pixels read at once, where filtering weighs them as the spline does.
    let h0 = (i - 0.5 + w1 / g0) / size;
    let h1 = (i + 1.5 + w3 / g1) / size;
    let a = textureSampleLevel(env_image, env_samp, h0, level).rgb * g0.x + textureSampleLevel(env_image, env_samp, vec2<f32>(h1.x, h0.y), level).rgb * g1.x;
    let b = textureSampleLevel(env_image, env_samp, vec2<f32>(h0.x, h1.y), level).rgb * g0.x + textureSampleLevel(env_image, env_samp, h1, level).rgb * g1.x;
    return a * g0.y + b * g1.y;
}

// Behind the model, the environment image as blurred as asked: a Gaussian-like blur of its mips
// (the glossy light would not do: the long tails of the GGX lobe spread a sun over the sky).
@fragment
fn behind_fs(i: BOut) -> @location(0) vec4<f32> {
    let uv = equirect(turned(normalize(u.forward.xyz + u.right.xyz * i.at.x + u.up.xyz * i.at.y)));
    let top = f32(textureNumLevels(env_image) - 1u);
    let level = clamp(u.levels.x, 0.0, top);
    let below = floor(level);
    let c = mix(bspline(uv, below), bspline(uv, min(below + 1.0, top)), level - below);
    return on_screen(c * u.params.z);
}
