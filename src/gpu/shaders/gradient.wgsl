struct Params {
    angle: f32, center_x: f32, center_y: f32, repeat: f32,
    offset: f32, kind: u32, invert: u32, tiling: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var out_tex: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }
    let uv = out_uv(gid.xy, size);

    // Distance to the center wraps around on tiled axes so radial shapes tile.
    var p = uv - vec2<f32>(P.center_x, P.center_y);
    if (tile_x(P.tiling)) { p.x = p.x - round(p.x); }
    if (tile_y(P.tiling)) { p.y = p.y - round(p.y); }

    var t = 0.0;
    switch P.kind {
        case 0u, 1u: {
            let dir = rotate_ccw(vec2<f32>(1.0, 0.0), P.angle);
            t = dot(uv - 0.5, dir) + 0.5;
        }
        case 2u: { t = length(p) * 2.0; }
        case 3u: {
            let a = atan2(-p.y, p.x) - radians(P.angle);
            t = fract(a / TAU);
        }
        default: {
            let q = rotate_ccw(p, -P.angle);
            t = (abs(q.x) + abs(q.y)) * 2.0;
        }
    }
    t = t * P.repeat + P.offset;
    if (P.kind == 1u) {
        let f = select(clamp(t, 0.0, 1.0), fract(t), P.repeat != 1.0 || P.offset != 0.0);
        t = 1.0 - abs(2.0 * f - 1.0);
    } else if (P.repeat != 1.0 || P.offset != 0.0) {
        t = fract(t);
    }
    t = clamp(t, 0.0, 1.0);
    if (P.invert != 0u) { t = 1.0 - t; }
    textureStore(out_tex, vec2<i32>(gid.xy), gray(t));
}
