// The nodes that make environments (brewed_texture_core::nodes::hdri). A pixel of a panorama
// is a direction, as the 3D view reads it (the designer's environment.rs: `direction`): across,
// once round, its middle straight ahead (-z), a quarter further to the right (+x); down, from
// straight up (+y) to straight down. A light is drawn where it is seen from the middle of the
// scene: a softbox or a panel as a flat shape there (round in the panorama, far off its
// middle), a tube along the great circle from one point to the other.

struct Params {
    // Where: the light's position (xy) and size (zw); a Line Light's two points; the
    // gradient's point (xy).
    a: vec4<f32>,
    // Rotation (degrees), hardness, brightness (a factor), the hotspot's brightness.
    b: vec4<f32>,
    // The hotspot's position (xy), size, falloff.
    c: vec4<f32>,
    // The light's color (linear); the gradient's top color.
    color: vec4<f32>,
    // A Line Light's thickness, a Plane Light's cells and gap, the gradient's contrast.
    d: vec4<f32>,
    // A Shape Light's shape or a Line Light's pattern, a Plane Light's placement and pattern.
    e: vec4<u32>,
    // The gradient's bottom color.
    f: vec4<f32>,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var background_tex: texture_2d<f32>;

fn direction(uv: vec2<f32>) -> vec3<f32> {
    let phi = (uv.x - 0.5) * TAU;
    let theta = uv.y * PI;
    return vec3<f32>(sin(theta) * sin(phi), cos(theta), -sin(theta) * cos(phi));
}

// The panorama the light is drawn over: round and round across.
fn background(gid: vec2<u32>, size: vec2<u32>) -> vec3<f32> {
    return widen(read_input(background_tex, gid, size, 1u), gray_input(0u)).rgb;
}

// Two axes square to `c` (to its right and up, as seen from the middle), turned `deg`
// counter-clockwise about it.
fn axes(c: vec3<f32>, deg: f32) -> mat2x3<f32> {
    let up0 = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), abs(c.y) > 0.999);
    let right = normalize(cross(c, up0));
    let up = cross(right, c);
    let a = radians(deg);
    return mat2x3<f32>(right * cos(a) + up * sin(a), up * cos(a) - right * sin(a));
}

// 1 inside an edge at 1 of a measure `s`, fading out as soft as `hardness` says (0: from the
// middle out, 1: at once), never sharper than a pixel (`aa`: how much `s` changes across one,
// held where it says nothing: far off, where the light's plane runs to the horizon).
fn inside(s: f32, hardness: f32, aa: f32) -> f32 {
    let pixel = min(aa, 0.5);
    let soft = max((1.0 - clamp(hardness, 0.0, 1.0)) * 0.998, pixel);
    return 1.0 - smoothstep(1.0 - soft, 1.0 + pixel, s);
}

// Whether a light whose plane a direction meets `near` (the cosine to the light's middle: 1
// over its distance) is there at all: none toward its horizon, where it would be infinitely
// far and large.
fn there(near: f32) -> f32 {
    return smoothstep(0.02, 0.05, near);
}

@fragment
fn gradient_main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let d = direction(out_uv(gid.xy, target_size()));
    let t = dot(d, direction(P.a.xy)) * 0.5 + 0.5;
    let w = max(1.0 - clamp(P.d.w, 0.0, 1.0), 0.002) * 0.5;
    let k = smoothstep(0.5 - w, 0.5 + w, t);
    return vec4<f32>(mix(P.f.rgb, P.color.rgb, k) * P.b.z, 1.0);
}

@fragment
fn shape_main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    let d = direction(out_uv(gid.xy, size));
    let c = direction(P.a.xy);
    let frame = axes(c, P.b.x);
    // Where the direction meets the light's plane, at the light's distance (1), in the light's
    // own units: its edge at 1.
    let facing = dot(d, c);
    let q = d / max(facing, 0.02) - c;
    let p = vec2<f32>(dot(q, frame[0]), dot(q, frame[1])) / max(P.a.zw, vec2<f32>(1e-4));
    var s: f32;
    if (P.e.x == 1u) {
        s = length(p);
    } else {
        // A rectangle with its corners a little round.
        s = pow(pow(abs(p.x), 8.0) + pow(abs(p.y), 8.0), 0.125);
    }
    let m = inside(s, P.b.y, fwidth(s)) * there(facing);
    // The hotspot.
    let hd = length(p - P.c.xy) / max(P.c.z, 1e-3);
    let hot = (1.0 - smoothstep(1.0 - clamp(P.c.w, 0.0, 1.0), 1.0 + 1e-3, hd)) * step(1e-3, P.c.z);
    let light = P.color.rgb * mix(P.b.z, P.b.w, hot);
    return vec4<f32>(mix(background(gid.xy, size), light, m), 1.0);
}

// How a Line Light is lit across, `x` from its middle (0) to its edge (1).
fn across(kind: u32, x: f32, hardness: f32, aa: f32) -> f32 {
    let h = clamp(hardness, 0.0, 1.0);
    switch kind {
        case 1u: { return inside(x, 1.0 - 0.02 * (1.0 - h), aa); }
        case 2u: { return pow(clamp(1.0 - x, 0.0, 1.0), mix(2.0, 0.5, h)); }
        case 3u: { return pow(sqrt(clamp(1.0 - x * x, 0.0, 1.0)), mix(2.0, 0.25, h)); }
        default: { return inside(x, h, aa); }
    }
}

// Where an end of a Line Light is in the scene, seen at `uv`: on a ceiling a step above the
// middle (seen above the horizon) or a floor a step below (`mode` 0), or a step from the middle.
fn line_end(uv: vec2<f32>, mode: u32) -> vec3<f32> {
    let d = direction(uv);
    if (mode == 1u) {
        return d;
    }
    return d / max(abs(d.y), 0.02);
}

@fragment
fn line_main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    let d = direction(out_uv(gid.xy, size));
    // The tube: round about the line from one end to the other, in the scene. How near the ray
    // from the middle along `d` passes its line: the nearest points of the two (the ray's never
    // behind the middle, the line's between its ends), found again from each other.
    let p1 = line_end(P.a.xy, P.e.y);
    let e = line_end(P.a.zw, P.e.y) - p1;
    let c = max(dot(e, e), 1e-8);
    let b = dot(d, e);
    let dp = dot(d, p1);
    let pe = dot(p1, e);
    var t = clamp((dp * b - pe) / max(c - b * b, 1e-8), 0.0, 1.0);
    var s = max(dp + t * b, 0.0);
    t = clamp((s * b - pe) / c, 0.0, 1.0);
    s = max(dp + t * b, 0.0);
    let x = length(d * s - (p1 + e * t)) / max(P.d.x, 1e-4);
    let aa = min(fwidth(x), 0.5);
    let cover = 1.0 - smoothstep(1.0 - aa, 1.0 + aa, x);
    let light = P.color.rgb * P.b.z * across(P.e.x, x, P.b.y, aa);
    return vec4<f32>(background(gid.xy, size) * (1.0 - cover) + light * cover, 1.0);
}

// A pattern of `cells` a side, `gap` of each dark, at `t` (-1 to 1 across the panel).
fn cells(t: f32, count: f32, gap: f32, aa: f32) -> f32 {
    let f = fract((t * 0.5 + 0.5) * count);
    let half = clamp(gap, 0.0, 0.5) * 0.5;
    let w = min(aa, 0.5) * count * 0.5;
    return smoothstep(half - w, half + w, f) * (1.0 - smoothstep(1.0 - half - w, 1.0 - half + w, f));
}

@fragment
fn plane_main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let gid = pixel(at);
    let size = target_size();
    let d = direction(out_uv(gid.xy, size));
    let c = direction(P.a.xy);
    // The panel: its middle, which way it faces, and its axes.
    var middle = c;
    var normal = -c;
    var frame = axes(c, P.b.x);
    var above = 1.0;
    if (P.e.y != 0u) {
        // Flat overhead (a ceiling) or underfoot (a floor), a step above or below the middle
        // of the scene, where `c` is seen.
        let up = select(-1.0, 1.0, P.e.y == 1u);
        above = step(0.02, c.y * up);
        middle = c / max(abs(c.y), 0.02);
        normal = vec3<f32>(0.0, -up, 0.0);
        let along = vec3<f32>(middle.x, 0.0, middle.z);
        let ahead = select(vec3<f32>(0.0, 0.0, -1.0), normalize(along), length(along) > 1e-4);
        let side = cross(ahead, vec3<f32>(0.0, 1.0, 0.0));
        let a = radians(P.b.x);
        frame = mat2x3<f32>(side * cos(a) + ahead * sin(a), ahead * cos(a) - side * sin(a));
    }
    // Where the direction meets the panel's plane, in the panel's units.
    let towards = dot(d, normal);
    let t = dot(middle, normal) / select(towards, -1e-4, abs(towards) < 1e-4);
    // (As far off as the plane is, from the middle: a step away.)
    let near = select(0.0, 1.0 / t, t > 0.0) * length(middle);
    let hit = d * min(t, 50.0) - middle;
    let p = vec2<f32>(dot(hit, frame[0]), dot(hit, frame[1])) / max(P.a.zw, vec2<f32>(1e-4));
    let s = max(abs(p.x), abs(p.y));
    let aa = fwidth(s);
    var m = inside(s, P.b.y, aa) * there(near) * above;
    let count = max(P.d.x, 1.0);
    if (P.e.z == 1u) {
        m = m * cells(p.x, count, P.d.y, fwidth(p.x));
    } else if (P.e.z == 2u) {
        m = m * cells(p.x, count, P.d.y, fwidth(p.x)) * cells(p.y, count, P.d.y, fwidth(p.y));
    }
    let light = P.color.rgb * P.b.z;
    return vec4<f32>(mix(background(gid.xy, size), light, m), 1.0);
}
