// The 3D view (view3d.rs): the model in its OpenPBR material, lit by the studio or by an
// environment image, and the image behind it.
//
// The material is what OpenPBR makes of its layers, from the top: a fuzz, a coat, and the base,
// which is metal or not — a surface that reflects (as its IOR says) over what is under it: a
// matte color, light scattered under the surface, or light going through; a thin film colors
// the base's reflection. Each layer is lit by the studio's light and its sky, or by the
// environment image (its irradiance, and its light blurred for each roughness), each weighed
// by how much of the light it reflects (shading.rs: `lut`) and keeps from the layers under it.
// The parameters come from the material's maps, by the inputs of shading.rs (`IN_…`, put
// before this): `sampled` reads each from its texture or from the row of uniform values.

struct U {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    light: vec4<f32>,
    params: vec4<f32>,      // displacement, texture repeat, exposure (a factor), _
    flags: vec4<u32>,       // _, sRGB target, _, lit by an environment image
    turn: vec4<f32>,        // cosine and sine of how far the environment image is turned
    levels: vec4<f32>,      // the level of the image behind the model, the top level of the glossy light, _, _
    right: vec4<f32>,       // the camera's axes, reaching to the edges of the view
    up: vec4<f32>,
    forward: vec4<f32>,
    sh: array<vec4<f32>, 9>, // the environment image's irradiance (environment.rs)
    // Where each input is read (shading.rs: `place`): bits 0-4 the texture (a slot, past them a
    // layer of `extra`, 31 the row of values), bit 5 gray, bit 6 given by a map.
    inputs: array<vec4<u32>, 16>,
};

@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var m0: texture_2d<f32>;
@group(0) @binding(2) var m1: texture_2d<f32>;
@group(0) @binding(3) var m2: texture_2d<f32>;
@group(0) @binding(4) var m3: texture_2d<f32>;
@group(0) @binding(5) var m4: texture_2d<f32>;
@group(0) @binding(6) var m5: texture_2d<f32>;
@group(0) @binding(7) var m6: texture_2d<f32>;
@group(0) @binding(8) var m7: texture_2d<f32>;
@group(0) @binding(9) var m8: texture_2d<f32>;
@group(0) @binding(10) var m9: texture_2d<f32>;
@group(0) @binding(11) var m10: texture_2d<f32>;
@group(0) @binding(12) var extra: texture_2d_array<f32>;
@group(0) @binding(13) var values: texture_2d<f32>;
@group(0) @binding(14) var samp: sampler;
@group(0) @binding(15) var env_image: texture_2d<f32>;
@group(0) @binding(16) var env_glossy: texture_2d<f32>;
@group(0) @binding(17) var env_samp: sampler;
@group(0) @binding(18) var lut: texture_2d<f32>;

const PI: f32 = 3.14159265;
const SLOTS: u32 = 11u;

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
    // Where the model's surface is before the height moves it.
    @location(4) surface: vec3<f32>,
};

// ---------------------------------------------------------------------------- the material's maps

fn entry(i: u32) -> u32 {
    return u.inputs[i / 4u][i % 4u];
}

// Whether a map of the material gives input `i` (rather than the standard's value).
fn given(i: u32) -> bool {
    return (entry(i) & 64u) != 0u;
}

fn slot_grad(s: u32, uv: vec2<f32>, gx: vec2<f32>, gy: vec2<f32>) -> vec4<f32> {
    switch s {
        case 0u: { return textureSampleGrad(m0, samp, uv, gx, gy); }
        case 1u: { return textureSampleGrad(m1, samp, uv, gx, gy); }
        case 2u: { return textureSampleGrad(m2, samp, uv, gx, gy); }
        case 3u: { return textureSampleGrad(m3, samp, uv, gx, gy); }
        case 4u: { return textureSampleGrad(m4, samp, uv, gx, gy); }
        case 5u: { return textureSampleGrad(m5, samp, uv, gx, gy); }
        case 6u: { return textureSampleGrad(m6, samp, uv, gx, gy); }
        case 7u: { return textureSampleGrad(m7, samp, uv, gx, gy); }
        case 8u: { return textureSampleGrad(m8, samp, uv, gx, gy); }
        case 9u: { return textureSampleGrad(m9, samp, uv, gx, gy); }
        case 10u: { return textureSampleGrad(m10, samp, uv, gx, gy); }
        default: { return textureSampleGrad(extra, samp, uv, i32(s - SLOTS), gx, gy); }
    }
}

fn slot_level(s: u32, uv: vec2<f32>) -> vec4<f32> {
    switch s {
        case 0u: { return textureSampleLevel(m0, samp, uv, 0.0); }
        case 1u: { return textureSampleLevel(m1, samp, uv, 0.0); }
        case 2u: { return textureSampleLevel(m2, samp, uv, 0.0); }
        case 3u: { return textureSampleLevel(m3, samp, uv, 0.0); }
        case 4u: { return textureSampleLevel(m4, samp, uv, 0.0); }
        case 5u: { return textureSampleLevel(m5, samp, uv, 0.0); }
        case 6u: { return textureSampleLevel(m6, samp, uv, 0.0); }
        case 7u: { return textureSampleLevel(m7, samp, uv, 0.0); }
        case 8u: { return textureSampleLevel(m8, samp, uv, 0.0); }
        case 9u: { return textureSampleLevel(m9, samp, uv, 0.0); }
        case 10u: { return textureSampleLevel(m10, samp, uv, 0.0); }
        default: { return textureSampleLevel(extra, samp, uv, i32(s - SLOTS), 0.0); }
    }
}

// Where the fragment is in the texture, and how that changes across the screen (the maps are
// read with them wherever the shader reads them).
var<private> at_uv: vec2<f32>;
var<private> at_dx: vec2<f32>;
var<private> at_dy: vec2<f32>;

// Input `i` at `uv`: as its map has it there (a map of one channel read as gray), or its value.
fn sampled_at(i: u32, uv: vec2<f32>) -> vec4<f32> {
    let e = entry(i);
    let s = e & 31u;
    if (s == 31u) {
        return textureLoad(values, vec2<i32>(i32(i), 0), 0);
    }
    return widen(slot_grad(s, uv, at_dx, at_dy), (e & 32u) != 0u);
}

fn sampled(i: u32) -> vec4<f32> {
    return sampled_at(i, at_uv);
}

// A value of the input, its first channel.
fn value(i: u32) -> f32 {
    return sampled(i).r;
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

// A color of the input: its map holds it in sRGB.
fn color(i: u32) -> vec3<f32> {
    return srgb_to_linear(max(sampled(i).rgb, vec3<f32>(0.0)));
}

// ---------------------------------------------------------------------------- the model

@vertex
fn vs(v: VIn) -> VOut {
    let uv = v.uv * u.params.y;
    var p = v.pos;
    if (given(IN_HEIGHT)) {
        let e = entry(IN_HEIGHT);
        var h: f32;
        if ((e & 31u) == 31u) {
            h = textureLoad(values, vec2<i32>(i32(IN_HEIGHT), 0), 0).r;
        } else {
            h = widen(slot_level(e & 31u, uv), (e & 32u) != 0u).r;
        }
        // As in Substance Designer: middle gray (128) leaves the surface where it is, white
        // moves it out and black in, as far apart as the view says (it only moved out, from
        // black: the relief is as deep as it was, about the surface now).
        p = p + v.normal * (clamp(h, -4.0, 4.0) - 0.5) * u.params.x;
    }
    var o: VOut;
    o.clip = u.view_proj * vec4<f32>(p, 1.0);
    o.world = p;
    o.normal = v.normal;
    o.tangent = v.tangent;
    o.uv = uv;
    o.surface = v.pos;
    return o;
}

// Which way the surface a height map (input `i`) makes of the model faces: it rises as far as
// the view displaces it, so its normal leans away from where it rises (the surface gradient
// of the height, as Mikkelsen works it out from how the surface and the texture change across
// the screen). Flat parts show it as the rounded ones do.
fn from_height(n: vec3<f32>, surface: vec3<f32>, uv: vec2<f32>, i: u32) -> vec3<f32> {
    // How u and v change along the surface, in the world.
    let dp1 = dpdx(surface);
    let dp2 = dpdy(surface);
    let duv1 = at_dx;
    let duv2 = at_dy;
    let r1 = cross(dp2, n);
    let r2 = cross(n, dp1);
    let det = dot(dp1, r1);
    let inv = select(0.0, 1.0 / det, abs(det) > 1e-20);
    let grad_u = (r1 * duv1.x + r2 * duv2.x) * inv;
    let grad_v = (r1 * duv1.y + r2 * duv2.y) * inv;
    // How the height rises along u and along v: over a texel, or as far as a pixel reaches
    // where the texture is seen from far.
    let texel = 1.0 / 2048.0;
    let step = max(max(abs(duv1), abs(duv2)), vec2<f32>(texel));
    let du = vec2<f32>(step.x, 0.0);
    let dv = vec2<f32>(0.0, step.y);
    let rise_u = (sampled_at(i, uv + du).r - sampled_at(i, uv - du).r) / (2.0 * step.x);
    let rise_v = (sampled_at(i, uv + dv).r - sampled_at(i, uv - dv).r) / (2.0 * step.y);
    return normalize(n - u.params.x * (rise_u * grad_u + rise_v * grad_v));
}

// A normal map's normal (OpenGL: +y up in the texture, against v), in the frame of the surface.
fn tangent_space(m: vec4<f32>) -> vec3<f32> {
    return m.rgb * 2.0 - 1.0;
}

// Detail laid over a normal (the reoriented normal mapping of Barré-Brisebois and Hill).
fn reoriented(base: vec3<f32>, detail: vec3<f32>) -> vec3<f32> {
    let t = base + vec3<f32>(0.0, 0.0, 1.0);
    let d = detail * vec3<f32>(-1.0, -1.0, 1.0);
    return normalize(t * dot(t, d) / t.z - d);
}

// A tangent along `n`: the direction a tangent map (input `map`) says in the surface's frame,
// or the surface's tangent, turned by `angle` (once round), made square to `n`.
fn tangent_along(n: vec3<f32>, t0: vec3<f32>, b0: vec3<f32>, map: u32, angle: f32) -> vec3<f32> {
    var t = t0;
    if (given(map)) {
        let d = sampled(map).rg * 2.0 - 1.0;
        if (dot(d, d) > 1e-6) {
            t = normalize(t0 * d.x + b0 * d.y);
        }
    }
    let a = angle * 2.0 * PI;
    t = t * cos(a) + cross(n, t) * sin(a);
    let along = t - n * dot(n, t);
    if (dot(along, along) < 1e-8) {
        return normalize(cross(n, b0));
    }
    return normalize(along);
}

// ---------------------------------------------------------------------------- the lobes

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn schlick(f0: vec3<f32>, mu: f32) -> vec3<f32> {
    return f0 + (1.0 - f0) * pow(1.0 - clamp(mu, 0.0, 1.0), 5.0);
}

// How much a dielectric of `ior` reflects head on.
fn f0_of(ior: f32) -> f32 {
    let r = (ior - 1.0) / (ior + 1.0);
    return r * r;
}

// A metal's Fresnel: Schlick's from its color, its edges tinted (the F82 tint of OpenPBR).
fn f82(f0: vec3<f32>, tint: vec3<f32>, mu: f32) -> vec3<f32> {
    let mb = 1.0 / 7.0;
    let a = schlick(f0, mb) * (1.0 - tint) / (mb * pow(1.0 - mb, 6.0));
    let m = clamp(mu, 0.0, 1.0);
    return clamp(schlick(f0, m) - a * m * pow(1.0 - m, 6.0), vec3<f32>(0.0), vec3<f32>(1.0));
}

// What a film of index `n2`, `thick` micrometers thick, over a surface of index `n3` (each
// channel) reflects, seen at cosine `mu`: the light reflected at its top and at its bottom
// interfering, for red, green and blue (Airy's sum, the mean of both polarizations).
fn thin_film(mu: f32, n2: f32, thick: f32, n3: vec3<f32>) -> vec3<f32> {
    let cos1 = clamp(mu, 1e-4, 1.0);
    let sin1_2 = 1.0 - cos1 * cos1;
    let cos2 = sqrt(max(1.0 - sin1_2 / (n2 * n2), 0.0));
    let cos3 = sqrt(max(vec3<f32>(1.0) - sin1_2 / (n3 * n3), vec3<f32>(0.0)));
    let delta = 4.0 * PI * n2 * max(thick, 0.0) * cos2 / vec3<f32>(0.65, 0.532, 0.45);
    let r12s = (cos1 - n2 * cos2) / (cos1 + n2 * cos2);
    let r12p = (n2 * cos1 - cos2) / (n2 * cos1 + cos2);
    let r23s = (n2 * cos2 - n3 * cos3) / (n2 * cos2 + n3 * cos3);
    let r23p = (n3 * cos2 - n2 * cos3) / (n3 * cos2 + n2 * cos3);
    let c = cos(delta);
    let rs = (r12s * r12s + r23s * r23s + 2.0 * r12s * r23s * c) / (1.0 + r12s * r12s * r23s * r23s + 2.0 * r12s * r23s * c);
    let rp = (r12p * r12p + r23p * r23p + 2.0 * r12p * r23p * c) / (1.0 + r12p * r12p * r23p * r23p + 2.0 * r12p * r23p * c);
    return clamp(0.5 * (rs + rp), vec3<f32>(0.0), vec3<f32>(1.0));
}

// The index a surface that reflects `f0` head on would have.
fn ior_of(f0: vec3<f32>) -> vec3<f32> {
    let s = sqrt(clamp(f0, vec3<f32>(0.0), vec3<f32>(0.98)));
    return (1.0 + s) / (1.0 - s);
}

// GGX's roughnesses along the tangent and across it (OpenPBR's anisotropy: rougher along the
// tangent, as rough on average).
fn alphas(rough: f32, aniso: f32) -> vec2<f32> {
    let a = max(rough * rough, 1e-4);
    let k = 1.0 - clamp(aniso, 0.0, 1.0);
    let at = min(a * sqrt(2.0 / (1.0 + k * k)), 1.0);
    return vec2<f32>(max(at, 1e-4), max(k * at, 1e-4));
}

fn ggx_lambda(wt: f32, wb: f32, wn: f32, a: vec2<f32>) -> f32 {
    let n2 = max(wn * wn, 1e-8);
    return (-1.0 + sqrt(1.0 + (a.x * a.x * wt * wt + a.y * a.y * wb * wb) / n2)) * 0.5;
}

// GGX's lobe toward the eye of a light from `l`, times its cosine, its Fresnel left out:
// D G / (4 n·v), anisotropic in the frame `n`, `t`, `b`.
fn ggx(n: vec3<f32>, t: vec3<f32>, b: vec3<f32>, v: vec3<f32>, l: vec3<f32>, a: vec2<f32>) -> f32 {
    let nl = dot(n, l);
    let nv = max(dot(n, v), 1e-4);
    if (nl <= 0.0) {
        return 0.0;
    }
    let h = normalize(v + l);
    let x = dot(h, t) / a.x;
    let y = dot(h, b) / a.y;
    let z = dot(h, n);
    let k = x * x + y * y + z * z;
    let d = 1.0 / (PI * a.x * a.y * k * k);
    let g = 1.0 / (1.0 + ggx_lambda(dot(v, t), dot(v, b), nv, a) + ggx_lambda(dot(l, t), dot(l, b), nl, a));
    return d * g / (4.0 * nv);
}

// The fuzz's lobe of a light from `l`, times its cosine: Charlie's sheen, Neubelt's visibility.
fn fuzz_lobe(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, alpha: f32) -> f32 {
    let nl = dot(n, l);
    if (nl <= 0.0) {
        return 0.0;
    }
    let nv = max(dot(n, v), 1e-4);
    let h = normalize(v + l);
    let nh = dot(n, h);
    let d = (2.0 + 1.0 / alpha) * pow(max(1.0 - nh * nh, 0.0), 0.5 / alpha) / (2.0 * PI);
    // (Fading out as the light comes ever more along them, as the fibers shade one another: the
    // visibility alone would keep their sheen up to the edge, seen edge on.)
    return d / (4.0 * (nl + nv - nl * nv)) * nl * smoothstep(0.0, 0.35, nl);
}

// How much of the light around a lobe of `rough` reflects, seen at cosine `nv` (shading.rs:
// `lut`): the scale and bias of F0 of GGX, all of the fuzz's.
fn lut_at(nv: f32, rough: f32) -> vec4<f32> {
    let s = f32(textureDimensions(lut).x);
    let uv = (vec2<f32>(clamp(nv, 0.0, 1.0), clamp(rough, 0.0, 1.0)) * (s - 1.0) + 0.5) / s;
    return textureSampleLevel(lut, env_samp, uv, 0.0);
}

// The matte part of a light from `l`, times its cosine: Lambert's, made flatter as it is rough
// (Oren and Nayar, as Fujii simplifies them).
fn matte(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, sigma: f32) -> f32 {
    let nl = max(dot(n, l), 0.0);
    let nv = max(dot(n, v), 0.0);
    let s = dot(l, v) - nl * nv;
    let t = select(1.0, max(nl, nv), s > 0.0);
    let a = 1.0 / (PI + (PI / 2.0 - 2.0 / 3.0) * sigma);
    return nl * (a + sigma * a * s / max(t, 1e-4));
}

// Light scattered under the surface, of a light at cosine `nl`: it wraps round past where the
// surface turns away, each channel as far as it goes under it (`w`, 0 to 1), as much light in
// all.
fn wrapped(nl: f32, w: vec3<f32>) -> vec3<f32> {
    return max(vec3<f32>(nl) + w, vec3<f32>(0.0)) / ((1.0 + w) * (1.0 + w)) / PI;
}

// How much of the light scattered inside a material goes on toward the eye, as a part of what
// scattering it evenly would send there: Henyey and Greenstein's phase, `g` forward (above 0),
// even (0) or back (below), `cos` the cosine of the light's way on and the eye's (1: the light
// comes from behind, through the material), held under 4.
fn phase(cos: f32, g: f32) -> f32 {
    let g2 = g * g;
    return min((1.0 - g2) / pow(max(1.0 + g2 - 2.0 * g * cos, 1e-4), 1.5), 4.0);
}

// ---------------------------------------------------------------------------- the light

// How bright the studio's sky is on a matte surface: it alone lights what the studio's light
// does not reach (a second light, opposite the first, did: see `fs`).
const SKY_AROUND: f32 = 0.5;

// The studio's sky.
fn studio_sky(dir: vec3<f32>) -> vec3<f32> {
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

fn lit_by_image() -> bool {
    return u.flags.w != 0u;
}

// The light all around that a matte surface facing `n` gets.
fn around(n: vec3<f32>) -> vec3<f32> {
    if (lit_by_image()) {
        return irradiance(turned(n));
    }
    return studio_sky(n) * SKY_AROUND * PI;
}

// The same, averaged over every way a surface can face.
fn around_mean() -> vec3<f32> {
    if (lit_by_image()) {
        return max(u.sh[0].rgb, vec3<f32>(0.0));
    }
    return (studio_sky(vec3<f32>(0.0, 1.0, 0.0)) + studio_sky(vec3<f32>(0.0, -1.0, 0.0))) * 0.5 * SKY_AROUND * PI;
}

// The light from around in direction `r`, blurred as a lobe of `rough` blurs it.
fn from_around(r: vec3<f32>, rough: f32) -> vec3<f32> {
    if (lit_by_image()) {
        return textureSampleLevel(env_glossy, env_samp, equirect(turned(r)), clamp(rough, 0.0, 1.0) * u.levels.y).rgb;
    }
    return studio_sky(r) * 0.6;
}

// ---------------------------------------------------------------------------- the material

struct Surface {
    n: vec3<f32>,
    t: vec3<f32>,
    b: vec3<f32>,
    cn: vec3<f32>,
    ct: vec3<f32>,
    cb: vec3<f32>,
    base: vec3<f32>,
    metal: f32,
    sigma: f32,
    spec_weight: f32,
    spec_color: vec3<f32>,
    // What the surface that is not metal reflects: head on, how much more toward its edges
    // (Schlick's (1 - f0)(1 - μ)⁵: all of it, but less as the specular weight goes under 1,
    // none at 0), and its tint.
    f0: vec3<f32>,
    edge: f32,
    tint: vec3<f32>,
    ior: f32,
    rough: f32,
    aniso: f32,
    trans: f32,
    trans_color: vec3<f32>,
    trans_depth: f32,
    trans_scatter: vec3<f32>,
    // Which way light scattered in the glass goes on: forward (above 0), evenly, or back.
    trans_g: f32,
    dispersion: f32,
    abbe: f32,
    sss: f32,
    sss_color: vec3<f32>,
    sss_reach: vec3<f32>,
    // Which way light scattered under the surface goes on: forward (above 0), evenly, or back.
    sss_g: f32,
    coat: f32,
    coat_color: vec3<f32>,
    coat_rough: f32,
    coat_aniso: f32,
    coat_f0: f32,
    fuzz: f32,
    fuzz_color: vec3<f32>,
    fuzz_rough: f32,
    emission: vec3<f32>,
    film: f32,
    film_thick: f32,
    film_ior: f32,
    opacity: f32,
    thin: bool,
    ao: f32,
};

// The base darkened and made richer by light caught in the coat over it, as much as the coat is
// there and darkens (the internal reflection of a dielectric of index `ior`).
fn darkened(c: vec3<f32>, coat: f32, darkening: f32, ior: f32) -> vec3<f32> {
    let f = f0_of(ior);
    let k = clamp(1.0 - (1.0 - (f + (1.0 - f) / 21.0)) / (ior * ior), 0.0, 0.95);
    let dark = c * (1.0 - k) / (1.0 - c * k);
    return mix(c, dark, clamp(coat * darkening, 0.0, 1.0));
}

// The roughness of a lobe of roughness `rough` made wider: `more` added to the square of its
// width (see `surface_of`).
fn widened(rough: f32, more: f32) -> f32 {
    let a = rough * rough;
    return min(sqrt(sqrt(a * a + more)), 1.0);
}

fn surface_of(i: VOut, front: bool) -> Surface {
    var s: Surface;
    let ng0 = normalize(i.normal);
    // Where the surface turns within a pixel (a rounded edge a pixel or two wide), a lobe
    // narrower than the turn shows as a broken line of sparks: the lobes are as wide as the
    // turn at least (Tokuyoshi and Kaplanyan, 2019: the variance of the normal over the pixel,
    // added to the square of the lobe's width).
    let turn_x = dpdx(ng0);
    let turn_y = dpdy(ng0);
    let wider = min(0.5 * (dot(turn_x, turn_x) + dot(turn_y, turn_y)), 0.18);
    let tg0 = normalize(i.tangent.xyz - ng0 * dot(ng0, i.tangent.xyz));
    let bg0 = cross(ng0, tg0) * i.tangent.w;

    // The normal: the normal map's (with its detail), or the surface the height makes, and a
    // bump over either.
    var n = ng0;
    if (given(IN_NORMAL) || given(IN_DETAIL_NORMAL)) {
        var m = vec3<f32>(0.0, 0.0, 1.0);
        if (given(IN_NORMAL)) {
            m = tangent_space(sampled(IN_NORMAL));
        }
        if (given(IN_DETAIL_NORMAL)) {
            m = reoriented(normalize(m), normalize(tangent_space(sampled(IN_DETAIL_NORMAL))));
        }
        n = normalize(tg0 * m.x + bg0 * m.y + ng0 * m.z);
    } else if (given(IN_HEIGHT)) {
        n = from_height(ng0, i.surface, i.uv, IN_HEIGHT);
    }
    if (given(IN_BUMP)) {
        n = from_height(n, i.surface, i.uv, IN_BUMP);
    }
    var cn = ng0;
    if (given(IN_COAT_NORMAL)) {
        let m = tangent_space(sampled(IN_COAT_NORMAL));
        cn = normalize(tg0 * m.x + bg0 * m.y + ng0 * m.z);
    }
    // Seen from behind (the plane, the cloth), a surface faces the other way.
    var ng = ng0;
    if (!front) {
        n = -n;
        cn = -cn;
        ng = -ng0;
    }
    s.n = n;
    s.cn = cn;
    s.t = tangent_along(n, tg0, bg0, IN_TANGENT, value(IN_ANISOTROPY_ANGLE));
    s.b = cross(n, s.t);
    s.ct = tangent_along(cn, tg0, bg0, IN_COAT_TANGENT, 0.0);
    s.cb = cross(cn, s.ct);

    // The base: its color, or the diffuse color of the specular and glossiness workflow.
    var base = color(IN_BASE_COLOR);
    var metal = clamp(value(IN_METALNESS), 0.0, 1.0);
    if (!given(IN_BASE_COLOR) && given(IN_DIFFUSE)) {
        base = color(IN_DIFFUSE);
        if (!given(IN_METALNESS)) {
            metal = 0.0;
        }
    }
    s.coat = clamp(value(IN_COAT_WEIGHT), 0.0, 1.0);
    let coat_ior = max(value(IN_COAT_IOR), 1.0);
    s.coat_f0 = f0_of(coat_ior);
    let darkening = clamp(value(IN_COAT_DARKENING), 0.0, 1.0);
    s.base = darkened(clamp(base * max(value(IN_BASE_WEIGHT), 0.0), vec3<f32>(0.0), vec3<f32>(1.0)), s.coat, darkening, coat_ior);
    s.metal = metal;
    s.sigma = clamp(value(IN_DIFFUSE_ROUGHNESS), 0.0, 1.0);

    // The specular: weight, color, roughness, anisotropy, index; or a specular map's color and
    // glossiness, or a specular level.
    s.spec_weight = max(value(IN_SPECULAR_WEIGHT), 0.0);
    if (!given(IN_SPECULAR_WEIGHT) && given(IN_SPECULAR_LEVEL)) {
        s.spec_weight = max(value(IN_SPECULAR_LEVEL), 0.0) * 2.0;
    }
    s.spec_color = clamp(color(IN_SPECULAR_COLOR), vec3<f32>(0.0), vec3<f32>(1.0));
    s.ior = max(value(IN_SPECULAR_IOR), 1.0);
    s.f0 = vec3<f32>(min(f0_of(s.ior) * s.spec_weight, 1.0));
    s.edge = min(s.spec_weight, 1.0);
    s.tint = s.spec_color;
    if (given(IN_SPECULAR_F0)) {
        s.f0 = clamp(color(IN_SPECULAR_F0), vec3<f32>(0.0), vec3<f32>(1.0));
        s.edge = 1.0;
        s.tint = vec3<f32>(1.0);
    }
    var rough = value(IN_ROUGHNESS);
    if (!given(IN_ROUGHNESS) && given(IN_GLOSSINESS)) {
        rough = 1.0 - value(IN_GLOSSINESS);
    }
    s.rough = widened(clamp(rough, 0.02, 1.0), wider);
    s.aniso = clamp(value(IN_ANISOTROPY), 0.0, 1.0);

    s.trans = clamp(value(IN_TRANSMISSION_WEIGHT), 0.0, 1.0);
    s.trans_color = clamp(color(IN_TRANSMISSION_COLOR), vec3<f32>(0.0), vec3<f32>(1.0));
    s.trans_depth = max(value(IN_TRANSMISSION_DEPTH), 0.0);
    s.trans_scatter = max(color(IN_TRANSMISSION_SCATTER), vec3<f32>(0.0));
    s.trans_g = clamp(value(IN_TRANSMISSION_SCATTER_ANISOTROPY), -0.9, 0.9);
    s.dispersion = max(value(IN_DISPERSION_SCALE), 0.0);
    s.abbe = max(value(IN_ABBE_NUMBER), 1.0);

    s.sss = clamp(value(IN_SUBSURFACE_WEIGHT), 0.0, 1.0);
    s.sss_color = darkened(clamp(color(IN_SUBSURFACE_COLOR), vec3<f32>(0.0), vec3<f32>(1.0)), s.coat, darkening, coat_ior);
    // How far round the light wraps: the radius, in the model's units (about a meter
    // across), scaled for each channel; further where light scattered under the surface goes
    // on forward (deeper before it turns: its transport mean free path is 1 / (1 - g) of
    // even scattering's), less far where it turns back.
    s.sss_g = clamp(value(IN_SUBSURFACE_ANISOTROPY), -0.9, 0.9);
    let reach = max(value(IN_SUBSURFACE_RADIUS), 0.0) * max(color(IN_SUBSURFACE_RADIUS_SCALE), vec3<f32>(0.0)) * 0.6;
    s.sss_reach = clamp(reach / (1.0 - 0.5 * s.sss_g), vec3<f32>(0.0), vec3<f32>(1.0));

    s.coat_color = clamp(color(IN_COAT_COLOR), vec3<f32>(0.0), vec3<f32>(1.0));
    s.coat_rough = widened(clamp(value(IN_COAT_ROUGHNESS), 0.02, 1.0), wider);
    s.coat_aniso = clamp(value(IN_COAT_ANISOTROPY), 0.0, 1.0);

    s.fuzz = clamp(value(IN_FUZZ_WEIGHT), 0.0, 1.0);
    s.fuzz_color = clamp(color(IN_FUZZ_COLOR), vec3<f32>(0.0), vec3<f32>(1.0));
    s.fuzz_rough = clamp(value(IN_FUZZ_ROUGHNESS), 0.07, 1.0);

    // Emission: as bright as its luminance says (in nits; 1000 as bright as its color), or as
    // its color alone where no luminance is given.
    var luminance = max(value(IN_EMISSION_LUMINANCE), 0.0);
    if (!given(IN_EMISSION_LUMINANCE) && given(IN_EMISSION_COLOR)) {
        luminance = 1000.0;
    }
    s.emission = max(color(IN_EMISSION_COLOR), vec3<f32>(0.0)) * luminance / 1000.0;

    s.film = clamp(value(IN_THIN_FILM_WEIGHT), 0.0, 1.0);
    s.film_thick = max(value(IN_THIN_FILM_THICKNESS), 0.0);
    s.film_ior = max(value(IN_THIN_FILM_IOR), 1.0);
    s.opacity = clamp(value(IN_OPACITY), 0.0, 1.0);
    s.thin = value(IN_THIN_WALLED) > 0.5;
    s.ao = clamp(value(IN_AO), 0.0, 1.0);
    return s;
}

// What the metal and the surface that is not metal reflect at cosine `mu`, a thin film over
// them as much as there is one.
fn metal_fresnel(s: Surface, mu: f32) -> vec3<f32> {
    var f = f82(s.base, s.spec_color, mu);
    if (s.film > 0.0) {
        f = mix(f, thin_film(mu, s.film_ior, s.film_thick, ior_of(s.base)), s.film);
    }
    return f * min(s.spec_weight, 1.0);
}

fn dielectric_fresnel(s: Surface, mu: f32) -> vec3<f32> {
    var f = s.tint * (s.f0 + s.edge * (1.0 - s.f0) * pow(1.0 - clamp(mu, 0.0, 1.0), 5.0));
    if (s.film > 0.0) {
        f = mix(f, thin_film(mu, s.film_ior, s.film_thick, vec3<f32>(s.ior)) * s.edge, s.film);
    }
    return f;
}

// How much of a light (or of the eye's view) at cosine `mu` to the coat gets through it, its
// color taken as far as the light goes in it.
fn through_coat(s: Surface, mu: f32) -> vec3<f32> {
    let f = s.coat * (s.coat_f0 + (1.0 - s.coat_f0) * pow(1.0 - clamp(mu, 0.0, 1.0), 5.0));
    let tint = mix(vec3<f32>(1.0), pow(s.coat_color, vec3<f32>(0.5 / max(mu, 0.1))), s.coat);
    return (1.0 - f) * tint;
}

// The light `l` reflects toward the eye `v`, each layer of the material, as a part of the
// light's.
fn reflected(s: Surface, v: vec3<f32>, l: vec3<f32>) -> vec3<f32> {
    let h = normalize(v + l);
    let vh = max(dot(v, h), 0.0);
    let nl = dot(s.n, l);
    let a = alphas(s.rough, s.aniso);
    let lobe = ggx(s.n, s.t, s.b, v, l, a);
    // Metal.
    let metal = metal_fresnel(s, vh) * lobe;
    // Not metal: its reflection, over what is under it.
    let f = dielectric_fresnel(s, vh);
    let under = (1.0 - f);
    let matte_part = s.base * matte(s.n, v, l, s.sigma);
    // (Light that scatters many times under the surface goes on every way: of the phase, only
    // as much as scatters once.)
    let sss_part = s.sss_color * wrapped(nl, s.sss_reach) * mix(1.0, phase(dot(-l, v), s.sss_g), 0.35);
    let body = mix(matte_part, sss_part, s.sss) * (1.0 - s.trans);
    let dielectric = f * lobe + under * body;
    var base = mix(dielectric, metal, s.metal);
    // The coat over it: its own reflection, and what it lets through, in and out.
    if (s.coat > 0.0) {
        let cnv = max(dot(s.cn, v), 1e-4);
        let cnl = max(dot(s.cn, l), 0.0);
        let ch = max(dot(v, h), 0.0);
        let coat_f = s.coat_f0 + (1.0 - s.coat_f0) * pow(1.0 - ch, 5.0);
        let coat = s.coat * coat_f * ggx(s.cn, s.ct, s.cb, v, l, alphas(s.coat_rough, s.coat_aniso));
        base = base * through_coat(s, cnv) * through_coat(s, cnl) + vec3<f32>(coat);
    }
    // The fuzz over everything.
    if (s.fuzz > 0.0) {
        let nv = max(dot(s.n, v), 1e-4);
        let keep = 1.0 - s.fuzz * max(s.fuzz_color.r, max(s.fuzz_color.g, s.fuzz_color.b)) * lut_at(nv, s.fuzz_rough).z;
        let keep_l = 1.0 - s.fuzz * max(s.fuzz_color.r, max(s.fuzz_color.g, s.fuzz_color.b)) * lut_at(max(nl, 0.0), s.fuzz_rough).z;
        base = base * keep * keep_l + s.fuzz * s.fuzz_color * fuzz_lobe(s.n, v, l, s.fuzz_rough);
    }
    return max(base, vec3<f32>(0.0));
}

// What comes through the surface toward the eye: the light around, seen through it, bent by its
// index (each color its own, as far as it disperses) unless it is a thin sheet, its color taken
// as deep as it goes, clouded as far as it scatters.
fn transmitted(s: Surface, v: vec3<f32>) -> vec3<f32> {
    let nv = max(dot(s.n, v), 1e-4);
    var seen: vec3<f32>;
    if (s.thin) {
        seen = from_around(-v, s.rough);
    } else if (s.dispersion > 0.0) {
        let spread = s.dispersion * (s.ior - 1.0) / s.abbe;
        let r = refract(-v, s.n, 1.0 / max(s.ior - spread, 1.0));
        let g = refract(-v, s.n, 1.0 / s.ior);
        let b = refract(-v, s.n, 1.0 / (s.ior + spread));
        seen = vec3<f32>(from_around(r, s.rough).r, from_around(g, s.rough).g, from_around(b, s.rough).b);
    } else {
        seen = from_around(refract(-v, s.n, 1.0 / s.ior), s.rough);
    }
    // How far the light goes in it: across the model, longer at its edges.
    let path = 1.6 * mix(1.0, nv, 0.5);
    var tint = s.trans_color;
    if (s.trans_depth > 0.0) {
        tint = pow(max(s.trans_color, vec3<f32>(1e-4)), vec3<f32>(path / s.trans_depth));
    }
    // Scattered on forward, light clouds the glass less (its transport mean free path is longer)
    // and what clouds it is more of what is seen through it, blurred; turned back, more, and
    // of the light on the eye's side.
    let cloud = 1.0 - exp(-s.trans_scatter * path * (1.0 - 0.5 * s.trans_g) / max(s.trans_depth, 0.2));
    var scattered = around(-s.n) / PI;
    if (s.trans_g > 0.0) {
        let ahead = select(refract(-v, s.n, 1.0 / s.ior), -v, s.thin);
        scattered = mix(scattered, from_around(ahead, 0.6), s.trans_g);
    } else if (s.trans_g < 0.0) {
        scattered = mix(scattered, around(s.n) / PI, -s.trans_g);
    }
    return mix(seen, scattered, clamp(cloud, vec3<f32>(0.0), vec3<f32>(1.0))) * tint;
}

// The light all around reflected toward the eye, each layer of the material.
// The light around a lobe of roughness `rough` about `n` reflects toward the eye: as blurred as
// it is rough; anisotropic, drawn out along the tangent `t` as far as the lobe is rougher that
// way: the light read at points along it, the normal leaning as far each way as the lobe
// reaches along the tangent, less toward the ends, each as blurred as the lobe is across (and
// enough that the points run into one another).
fn glossy_around(n: vec3<f32>, t: vec3<f32>, v: vec3<f32>, rough: f32, aniso: f32) -> vec3<f32> {
    if (aniso <= 0.0) {
        return from_around(reflect(-v, n), rough);
    }
    let a = alphas(rough, aniso);
    let blur = max(sqrt(a.y), sqrt(a.x) * 0.6);
    var sum = vec3<f32>(0.0);
    var weights = 0.0;
    for (var k = 0; k < 16; k = k + 1) {
        let along = (f32(k) + 0.5) / 16.0 * 2.0 - 1.0;
        let w = 1.0 - along * along;
        sum = sum + from_around(reflect(-v, normalize(n + t * along * a.x * 1.5)), blur) * w;
        weights = weights + w;
    }
    return sum / weights;
}

fn reflected_around(s: Surface, v: vec3<f32>) -> vec3<f32> {
    let nv = max(dot(s.n, v), 1e-4);
    let dfg = lut_at(nv, s.rough).xy;
    let glossy = glossy_around(s.n, s.t, v, s.rough, s.aniso);
    // Metal: its color head on, the tint toward its edges.
    var metal_e = s.base * dfg.x + s.spec_color * dfg.y;
    if (s.film > 0.0) {
        metal_e = mix(metal_e, thin_film(nv, s.film_ior, s.film_thick, ior_of(s.base)) * (dfg.x + dfg.y), s.film);
    }
    let metal = glossy * metal_e * min(s.spec_weight, 1.0);
    // Not metal: Schlick's Fresnel over the lobe is f0 (A + B) less as much of B as the edge
    // is not there, and the edge's B.
    var spec_e = s.tint * (s.f0 * (dfg.x + (1.0 - s.edge) * dfg.y) + s.edge * dfg.y);
    if (s.film > 0.0) {
        spec_e = mix(spec_e, thin_film(nv, s.film_ior, s.film_thick, vec3<f32>(s.ior)) * (dfg.x + dfg.y) * s.edge, s.film);
    }
    let matte_e = s.base * around(s.n) / PI;
    let reach = s.sss_reach;
    // Scattered on forward, the light behind comes through toward the eye; turned back, the
    // light on the eye's side comes back to it.
    let lit = mix(around(s.n), around_mean(), reach * 0.7);
    let way = select(around(v), around(-v), s.sss_g > 0.0);
    let sss_e = s.sss_color * mix(lit, way, abs(s.sss_g) * reach * 0.5) / PI;
    let body = mix(mix(matte_e, sss_e, s.sss), transmitted(s, v), s.trans);
    let dielectric = glossy * spec_e + (1.0 - spec_e) * body;
    var out = mix(dielectric, metal, s.metal) * s.ao;
    if (s.coat > 0.0) {
        let cnv = max(dot(s.cn, v), 1e-4);
        let cdfg = lut_at(cnv, s.coat_rough).xy;
        let coat_e = s.coat * (s.coat_f0 * cdfg.x + cdfg.y);
        // (Anisotropic, drawn out along its tangent, as the base's reflection is.)
        let coat = glossy_around(s.cn, s.ct, v, s.coat_rough, s.coat_aniso) * coat_e;
        out = out * (1.0 - coat_e) * mix(vec3<f32>(1.0), pow(s.coat_color, vec3<f32>(1.0 / max(cnv, 0.1))), s.coat) + coat * s.ao;
    }
    if (s.fuzz > 0.0) {
        let e = lut_at(nv, s.fuzz_rough).z;
        // Fibers catch light that comes along the surface, not as a mirror does whatever their
        // roughness: the light around, and the light toward the surface's horizon past the
        // reflection, blurred all the way.
        let r = reflect(-v, s.n);
        let skim = normalize(r - s.n * dot(r, s.n) * 0.8);
        let fuzz = s.fuzz * s.fuzz_color * e * 0.5 * (around(s.n) / PI + from_around(skim, 1.0));
        out = out * (1.0 - s.fuzz * max(s.fuzz_color.r, max(s.fuzz_color.g, s.fuzz_color.b)) * e) + fuzz * s.ao;
    }
    return out;
}

// The glow, under the coat and the fuzz.
fn glow(s: Surface, v: vec3<f32>) -> vec3<f32> {
    var e = s.emission;
    if (s.coat > 0.0) {
        e = e * through_coat(s, max(dot(s.cn, v), 1e-4));
    }
    if (s.fuzz > 0.0) {
        e = e * (1.0 - s.fuzz * lut_at(max(dot(s.n, v), 1e-4), s.fuzz_rough).z);
    }
    return e;
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Light as the screen shows it: tone mapped, and in sRGB where the target does not make it so;
// `alpha` as much of it as there is (multiplied in: what is behind shows through the rest).
fn on_screen(c: vec3<f32>, alpha: f32) -> vec4<f32> {
    var color = aces(c);
    if (u.flags.y == 0u) {
        color = linear_to_srgb(color);
    }
    return vec4<f32>(color * alpha, alpha);
}

@fragment
fn fs(i: VOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    at_uv = i.uv;
    at_dx = dpdx(i.uv);
    at_dy = dpdy(i.uv);
    let s = surface_of(i, front);
    let v = normalize(u.eye.xyz - i.world);
    var color = reflected_around(s, v);
    // The studio's light: one, as the view says it has (a second, opposite it, filled the
    // other side: the model had two highlights for one light, and a dark line along every
    // rounded edge between a side the one lit and a side the other did; the sky fills that
    // side now); over an environment image, only where it is asked for too (u.flags.x).
    if (!lit_by_image() || u.flags.x != 0u) {
        let key = normalize(u.light.xyz);
        color = color + reflected(s, v, key) * vec3<f32>(3.2, 3.05, 2.85);
    }
    color = color + glow(s, v);
    return on_screen(color * u.params.z, s.opacity);
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
    return on_screen(c * u.params.z, 1.0);
}
