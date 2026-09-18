struct Params {
    size_x: f32, size_y: f32, scale: f32, rotation: f32,
    offset_x: f32, offset_y: f32, softness: f32, sides: f32,
    inner: f32, thickness: f32, corner: f32, repeat: f32,
    kind: u32, tiling: u32, invert: u32, pad0: u32,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var out_tex: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(out_tex);
    if (gid.x >= size.x || gid.y >= size.y) { return; }

    let rep = max(P.repeat, 1.0);
    let g = out_uv(gid.xy, size) * rep;     // grid coordinates, one cell per copy
    let cell = floor(g);

    var sp: ShapeParams;
    sp.kind = P.kind;
    sp.half = vec2<f32>(P.size_x, P.size_y) * P.scale * 0.5;
    sp.softness = P.softness;
    sp.aa = max(rep / f32(size.x), rep / f32(size.y)) * 0.75;
    sp.sides = P.sides;
    sp.inner = P.inner;
    sp.thickness = P.thickness;
    sp.corner = P.corner;

    // Shapes may overflow their cell: look at the neighbours too. Neighbour cells
    // outside the image only exist on tiled axes, which is what makes shapes wrap.
    let reach = select(1, 0, P.kind == 13u);
    var v = 0.0;
    for (var dy = -reach; dy <= reach; dy++) {
        for (var dx = -reach; dx <= reach; dx++) {
            let c = cell + vec2<f32>(f32(dx), f32(dy));
            if (!tile_x(P.tiling) && (c.x < 0.0 || c.x >= rep)) { continue; }
            if (!tile_y(P.tiling) && (c.y < 0.0 || c.y >= rep)) { continue; }
            let center = c + 0.5 + vec2<f32>(P.offset_x, P.offset_y);
            let q = rotate_ccw(g - center, -P.rotation);
            v = max(v, shape_value(sp, q));
        }
    }
    if (P.invert != 0u) { v = 1.0 - v; }
    textureStore(out_tex, vec2<i32>(gid.xy), gray(clamp(v, 0.0, 1.0)));
}
