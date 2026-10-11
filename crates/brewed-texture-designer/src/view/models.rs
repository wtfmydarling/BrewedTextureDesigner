//! The models of the 3D view: their surfaces as triangles, each corner with its normal, its
//! tangent and its place in the texture, laid out so that the texture shows the right way
//! round from outside (u to the right, v down) and normal maps are read as OpenGL makes them.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// A cube with rounded edges: flat faces to judge the texture on, and curves to
    /// catch the light. What a material is usually looked at on.
    RoundedCube,
    Sphere,
    /// A shader ball: a ball cut open into shells and rings, as materials are shown in
    /// catalogues, its surfaces turned every way, flat, hollow and rounded. The model is "Shader
    /// Ball JL 01" by Craneo, under Creative Commons Attribution 4.0 (see Help → Third-Party
    /// Notices).
    MatBall,
    Cube,
    /// A cylinder with rounded edges: round all over but for its flat ends.
    RoundedCylinder,
    Cylinder,
    Torus,
    Plane,
    /// A cloth hanging in folds: the material bent as fabric bends.
    Cloth,
}

impl Shape {
    pub const ALL: [Shape; 9] = [
        Shape::RoundedCube,
        Shape::Sphere,
        Shape::MatBall,
        Shape::Cube,
        Shape::RoundedCylinder,
        Shape::Cylinder,
        Shape::Torus,
        Shape::Plane,
        Shape::Cloth,
    ];

    /// What a file calls it (the model of a graph's 3D view, see
    /// `brewed_texture_core::project::ViewState`).
    pub fn id(self) -> &'static str {
        match self {
            Shape::RoundedCube => "rounded_cube",
            Shape::Sphere => "sphere",
            Shape::MatBall => "matball",
            Shape::Cube => "cube",
            Shape::RoundedCylinder => "rounded_cylinder",
            Shape::Cylinder => "cylinder",
            Shape::Torus => "torus",
            Shape::Plane => "plane",
            Shape::Cloth => "cloth",
        }
    }

    pub fn from_id(id: &str) -> Option<Shape> {
        Shape::ALL.into_iter().find(|s| s.id() == id)
    }

    fn label(self) -> &'static str {
        match self {
            Shape::RoundedCube => "Rounded Cube",
            Shape::Sphere => "Sphere",
            Shape::MatBall => "Matball",
            Shape::Cube => "Cube",
            Shape::RoundedCylinder => "Rounded Cylinder",
            Shape::Cylinder => "Cylinder",
            Shape::Torus => "Torus",
            Shape::Plane => "Plane",
            Shape::Cloth => "Cloth",
        }
    }
}

impl std::fmt::Display for Shape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

pub(super) fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(super) fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub(super) fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(super) fn normalize(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt().max(1e-9);
    [a[0] / l, a[1] / l, a[2] / l]
}

// ---------------------------------------------------------------------------- meshes

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct Vertex {
    pub(super) pos: [f32; 3],
    normal: [f32; 3],
    tangent: [f32; 4],
    uv: [f32; 2],
}

/// The two triangles of each quad of a grid of `columns` × `rows` quads whose vertices start at
/// `base`, row by row: counter-clockwise seen from the side the normals face, when `u` runs to
/// the right and `v` down on that side (as the texture shows).
fn quads(base: u32, columns: u32, rows: u32, out: &mut Vec<u32>) {
    for j in 0..rows {
        for i in 0..columns {
            let a = base + j * (columns + 1) + i;
            let b = a + 1;
            let c = a + columns + 1;
            let d = c + 1;
            out.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
}

/// A grid of `n` x `n` quads from `f(u, v) -> (position, normal, tangent)`.
fn grid(n: u32, uv_scale: [f32; 2], f: impl Fn(f32, f32) -> ([f32; 3], [f32; 3], [f32; 3]), out: &mut (Vec<Vertex>, Vec<u32>)) {
    let at: Vec<f32> = (0..=n).map(|i| i as f32 / n as f32).collect();
    grid_at(&at, uv_scale, f, out);
}

/// A grid of quads from `f(u, v)`, at the `u` and `v` of `at` (from 0 to 1, in order): where
/// they are close together, the surface is finer, its texture laid on it as evenly as elsewhere
/// (`u` and `v` are its coordinates, wherever they are taken).
fn grid_at(at: &[f32], uv_scale: [f32; 2], f: impl Fn(f32, f32) -> ([f32; 3], [f32; 3], [f32; 3]), out: &mut (Vec<Vertex>, Vec<u32>)) {
    let base = out.0.len() as u32;
    for &v in at {
        for &u in at {
            let (pos, normal, tangent) = f(u, v);
            out.0.push(Vertex { pos, normal, tangent: [tangent[0], tangent[1], tangent[2], 1.0], uv: [u * uv_scale[0], v * uv_scale[1]] });
        }
    }
    let n = at.len() as u32 - 1;
    quads(base, n, n, &mut out.1);
}

/// From 0 to 1, `across` steps between bands of `band` at each end, and `along` steps in each
/// band: what a face whose edges are rounded is sampled at, finer where it rounds.
fn banded(band: f32, along: u32, across: u32) -> Vec<f32> {
    let mut at: Vec<f32> = (0..along).map(|i| band * i as f32 / along as f32).collect();
    at.extend((0..across).map(|i| band + (1.0 - 2.0 * band) * i as f32 / across as f32));
    at.extend((0..=along).map(|i| 1.0 - band + band * i as f32 / along as f32));
    at
}

/// A piece of the outline of a surface of revolution, in radius and height.
#[derive(Clone, Copy)]
enum Piece {
    Line([f32; 2], [f32; 2]),
    /// The points `center + radius * (sin a, cos a)`, `a` from `from` to `to` (radians: 0 is
    /// straight up, π/2 straight out).
    Arc { center: [f32; 2], radius: f32, from: f32, to: f32 },
}

impl Piece {
    fn at(self, s: f32) -> [f32; 2] {
        match self {
            Piece::Line(a, b) => [a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s],
            Piece::Arc { center, radius, from, to } => {
                let a = from + (to - from) * s;
                [center[0] + radius * a.sin(), center[1] + radius * a.cos()]
            }
        }
    }

    /// Which way the outline goes at `s`.
    fn along(self, s: f32) -> [f32; 2] {
        match self {
            Piece::Line(a, b) => [b[0] - a[0], b[1] - a[1]],
            Piece::Arc { radius, from, to, .. } => {
                let a = from + (to - from) * s;
                let k = radius * (to - from);
                [k * a.cos(), -k * a.sin()]
            }
        }
    }

    fn length(self) -> f32 {
        match self {
            Piece::Line(a, b) => (b[0] - a[0]).hypot(b[1] - a[1]),
            Piece::Arc { radius, from, to, .. } => (radius * (to - from)).abs(),
        }
    }
}

/// A surface turned around the y axis, its outline drawn in `pieces` from the top of the axis
/// down: `u` goes round `repeats` times, `v` along the outline, once every `size` of its length
/// (square where the circumference is `repeats * size`). The rows are spread by length, so the
/// surface is as fine all over. Where two pieces meet at an angle, it has an edge.
fn lathe(pieces: &[Piece], rows: u32, around: u32, repeats: f32, size: f32, out: &mut (Vec<Vertex>, Vec<u32>)) {
    use std::f32::consts::TAU;
    let total: f32 = pieces.iter().map(|p| p.length()).sum();
    let mut along = 0.0;
    for piece in pieces {
        let length = piece.length();
        let n = ((length / total * rows as f32).ceil() as u32).max(1);
        let base = out.0.len() as u32;
        for j in 0..=n {
            let s = j as f32 / n as f32;
            let [r, y] = piece.at(s);
            // Outward: the outline's direction turned a quarter.
            let d = piece.along(s);
            let l = d[0].hypot(d[1]).max(1e-9);
            let (nr, ny) = (-d[1] / l, d[0] / l);
            let v = (along + s * length) / size;
            for i in 0..=around {
                let u = i as f32 / around as f32;
                let (sin, cos) = (u * TAU).sin_cos();
                out.0.push(Vertex {
                    pos: [sin * r, y, cos * r],
                    normal: [sin * nr, ny, cos * nr],
                    tangent: [cos, 0.0, -sin, 1.0],
                    uv: [u * repeats, v],
                });
            }
        }
        quads(base, around, n, &mut out.1);
        along += length;
    }
}

/// A torus of radii `major` and `minor` around the y axis tilted by `tilt` about the x axis,
/// centred at `center`: `u` goes round the axis, `v` round the tube (down its outside first).
fn torus(major: f32, minor: f32, center: [f32; 3], tilt: f32, uv_scale: [f32; 2], out: &mut (Vec<Vertex>, Vec<u32>)) {
    use std::f32::consts::TAU;
    let (st, ct) = tilt.sin_cos();
    let turn = |p: [f32; 3]| [p[0], p[1] * ct - p[2] * st, p[1] * st + p[2] * ct];
    grid(
        192,
        uv_scale,
        |u, v| {
            let (sa, ca) = (u * TAU).sin_cos();
            let (sb, cb) = (v * TAU).sin_cos();
            let normal = [sa * cb, -sb, ca * cb];
            let p = turn([sa * major + normal[0] * minor, normal[1] * minor, ca * major + normal[2] * minor]);
            ([p[0] + center[0], p[1] + center[1], p[2] + center[2]], turn(normal), turn([ca, 0.0, -sa]))
        },
        out,
    );
}

/// The six faces of a cube as (normal, u axis, v axis); v runs downwards in the texture.
const CUBE_FACES: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
    ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
    ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
    ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, -1.0, 0.0]),
    ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]),
    ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
];

/// The model of the Matball: made of "Shader Ball JL 01" (by Craneo, CC BY 4.0) in Blender
/// (its modifiers applied, made triangles, its tangents MikkTSpace's), turned to stand up as the
/// view has it, made the size of the other models and its texture as large on it as on the
/// sphere; kept as b"BTDM", a version (1), then deflated (zlib): the number of vertices and of
/// indices, the vertices as twelve floats each (as [`Vertex`] is), the indices.
const MATBALL: &[u8] = include_bytes!("../../assets/models/matball.bin");

/// The triangles of a model kept as [`MATBALL`] is.
fn read_model(bytes: &[u8]) -> Result<(Vec<Vertex>, Vec<u32>), String> {
    use std::io::Read;
    let packed = bytes.strip_prefix(b"BTDM").ok_or("not a model")?;
    let (version, packed) = packed.split_at_checked(4).ok_or("cut short")?;
    if version != 1u32.to_le_bytes() {
        return Err("of another version".into());
    }
    let mut data = Vec::new();
    flate2::read::ZlibDecoder::new(packed).read_to_end(&mut data).map_err(|e| e.to_string())?;
    let word = |i: usize| data.get(i * 4..i * 4 + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let (vertices, indices) = (word(0).ok_or("cut short")? as usize, word(1).ok_or("cut short")? as usize);
    const SIZE: usize = std::mem::size_of::<Vertex>();
    let (from, to) = (8, 8 + vertices * SIZE);
    if data.len() != to + indices * 4 {
        return Err("of another size".into());
    }
    let vs = data[from..to].as_chunks::<SIZE>().0.iter().map(|b| bytemuck::pod_read_unaligned::<Vertex>(b)).collect();
    let is: Vec<u32> = data[to..].as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)).collect();
    if is.iter().any(|&i| i as usize >= vertices) {
        return Err("a triangle of a vertex it has not".into());
    }
    Ok((vs, is))
}

pub(super) fn build_mesh(shape: Shape) -> (Vec<Vertex>, Vec<u32>) {
    use std::f32::consts::{FRAC_PI_2, PI, TAU};
    let mut out = (Vec::new(), Vec::new());
    match shape {
        Shape::RoundedCube => {
            // The surface of a box of half-size `H - R` grown by a ball of radius `R`:
            // every point of a face is pushed out from the nearest point of that box, which
            // leaves the middle of a face flat and rounds the edges and corners. Its edges are
            // rounded by a sixty-fourth of its side: enough to catch the light as the edge of a
            // real thing does, the faces flat to their ends for a texture to be judged on (a
            // quarter of each face was round, then a sixteenth and a thirty-second of the
            // side). The grid is finest where it rounds: sixteen steps an eighth of a turn.
            const H: f32 = 0.8;
            const R: f32 = 2.0 * H / 64.0;
            let at = banded(R / H / 2.0, 16, 112);
            for (n, a, b) in CUBE_FACES {
                grid_at(
                    &at,
                    [1.0, 1.0],
                    |u, v| {
                        let (s, t) = (u * 2.0 - 1.0, v * 2.0 - 1.0);
                        let q = [
                            (n[0] + a[0] * s + b[0] * t) * H,
                            (n[1] + a[1] * s + b[1] * t) * H,
                            (n[2] + a[2] * s + b[2] * t) * H,
                        ];
                        let core = q.map(|c| c.clamp(-(H - R), H - R));
                        let out = [q[0] - core[0], q[1] - core[1], q[2] - core[2]];
                        let normal = normalize(out);
                        let p = [core[0] + normal[0] * R, core[1] + normal[1] * R, core[2] + normal[2] * R];
                        // The face's own u axis, along the surface: it turns with the edges.
                        let along = dot(a, normal);
                        let tangent = normalize([a[0] - normal[0] * along, a[1] - normal[1] * along, a[2] - normal[2] * along]);
                        (p, normal, tangent)
                    },
                    &mut out,
                );
            }
        }
        Shape::Sphere => grid(
            160,
            [2.0, 1.0],
            |u, v| {
                let (theta, phi) = (v * PI, u * TAU);
                let n = [theta.sin() * phi.sin(), theta.cos(), theta.sin() * phi.cos()];
                let t = [phi.cos(), 0.0, -phi.sin()];
                (n, n, t)
            },
            &mut out,
        ),
        Shape::MatBall => match read_model(MATBALL) {
            Ok(model) => return model,
            // (It is built in: a test reads it.)
            Err(e) => log::error!("the matball: {e}"),
        },
        Shape::Plane => grid(
            256,
            [1.0, 1.0],
            |u, v| ([u * 2.4 - 1.2, 1.2 - v * 2.4, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
            &mut out,
        ),
        Shape::Cube => {
            for (n, a, b) in CUBE_FACES {
                grid(
                    64,
                    [1.0, 1.0],
                    |u, v| {
                        let (s, t) = ((u * 2.0 - 1.0) * 0.8, (v * 2.0 - 1.0) * 0.8);
                        let p = [n[0] * 0.8 + a[0] * s + b[0] * t, n[1] * 0.8 + a[1] * s + b[1] * t, n[2] * 0.8 + a[2] * s + b[2] * t];
                        (p, n, a)
                    },
                    &mut out,
                );
            }
        }
        Shape::RoundedCylinder => {
            // Across the top, round the edge, down the side, round the edge, across the bottom.
            const R: f32 = 0.78;
            const H: f32 = 0.9;
            const E: f32 = 0.22;
            lathe(
                &[
                    Piece::Line([0.0, H], [R - E, H]),
                    Piece::Arc { center: [R - E, H - E], radius: E, from: 0.0, to: FRAC_PI_2 },
                    Piece::Line([R, H - E], [R, -(H - E)]),
                    Piece::Arc { center: [R - E, -(H - E)], radius: E, from: FRAC_PI_2, to: PI },
                    Piece::Line([R - E, -H], [0.0, -H]),
                ],
                256,
                192,
                3.0,
                TAU * R / 3.0,
                &mut out,
            );
        }
        Shape::Cylinder => lathe(
            &[Piece::Line([0.0, 1.0], [0.8, 1.0]), Piece::Line([0.8, 1.0], [0.8, -1.0]), Piece::Line([0.8, -1.0], [0.0, -1.0])],
            256,
            160,
            2.0,
            TAU * 0.8 / 2.0,
            &mut out,
        ),
        // Tilted toward the camera, which sees through it.
        Shape::Torus => torus(0.68, 0.3, [0.0; 3], 0.5, [3.0, 1.0], &mut out),
        Shape::Cloth => {
            // Held along its top edge: folds that deepen and widen toward the hem, finer ones
            // on them, and a bow that turns its sides away.
            let depth = |u: f32, v: f32| {
                let hang = v * v;
                let folds = (u * TAU * 2.5 + (v * 3.0).sin() * 0.8).sin() * 0.11 * hang;
                let fine = (u * TAU * 6.5 + v * 2.0).sin() * 0.025 * hang;
                folds + fine - (u - 0.5) * (u - 0.5) * 0.9
            };
            grid(
                256,
                [1.0, 1.0],
                |u, v| {
                    let e = 1.0 / 1024.0;
                    // Across (+x) and down (-y) the cloth, as far as its folds go.
                    let across = normalize([1.0, 0.0, (depth(u + e, v) - depth(u - e, v)) / (4.0 * e)]);
                    let down = normalize([0.0, -1.0, (depth(u, v + e) - depth(u, v - e)) / (4.0 * e)]);
                    ([u * 2.0 - 1.0, 1.0 - v * 2.0, depth(u, v)], normalize(cross(down, across)), across)
                },
                &mut out,
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meshes_are_valid() {
        for shape in Shape::ALL {
            let (v, i) = build_mesh(shape);
            assert!(i.iter().all(|&x| (x as usize) < v.len()), "{shape:?}");
            assert_eq!(i.len() % 3, 0);
            assert!(v.iter().all(|x| x.pos.iter().chain(&x.uv).all(|c| c.is_finite())), "{shape:?}");
            assert!(v.iter().all(|x| (dot(x.normal, x.normal) - 1.0).abs() < 1e-3), "{shape:?}: a normal is not of length 1");
        }
    }

    /// Every triangle of every model faces the way its normals do (counter-clockwise seen from
    /// there: the shader lights a surface seen from behind from that side), and shows the
    /// texture as its tangent says: `u` along the tangent, `v` against the bitangent (the
    /// normal and the tangent crossed, turned over where the tangent's handedness says the
    /// texture is mirrored there), as normal maps are read. The Matball, made elsewhere, has
    /// normals the bevels of its edges lean (Blender's weighted normals): a few of its slivers
    /// face away from them.
    #[test]
    fn meshes_face_out_and_wear_the_texture_the_right_way_round() {
        let scale = |a: [f32; 3], k: f32| a.map(|c| c * k);
        for shape in Shape::ALL {
            let (v, idx) = build_mesh(shape);
            let (mut facing, mut mirrored, mut checked) = (0, 0, 0);
            for t in idx.as_chunks::<3>().0 {
                let [a, b, c] = [v[t[0] as usize], v[t[1] as usize], v[t[2] as usize]];
                let (e1, e2) = (sub(b.pos, a.pos), sub(c.pos, a.pos));
                let area = cross(e1, e2);
                // The triangles at a pole have no area.
                if dot(area, area).sqrt() < 1e-7 {
                    continue;
                }
                checked += 1;
                let n = [0, 1, 2].map(|k| a.normal[k] + b.normal[k] + c.normal[k]);
                if dot(area, n) <= 0.0 {
                    facing += 1;
                }
                let (du1, dv1, du2, dv2) = (b.uv[0] - a.uv[0], b.uv[1] - a.uv[1], c.uv[0] - a.uv[0], c.uv[1] - a.uv[1]);
                let det = du1 * dv2 - du2 * dv1;
                if det.abs() < 1e-12 {
                    continue;
                }
                // How the surface moves with u and with v.
                let pu = scale(sub(scale(e1, dv2), scale(e2, dv1)), 1.0 / det);
                let pv = scale(sub(scale(e2, du1), scale(e1, du2)), 1.0 / det);
                let tangent = [a.tangent[0], a.tangent[1], a.tangent[2]];
                if dot(pu, tangent) <= 0.0 || dot(pv, cross(a.normal, tangent)) * a.tangent[3] >= 0.0 {
                    mirrored += 1;
                }
            }
            assert!(checked > 1000, "{shape:?}");
            let slivers = if shape == Shape::MatBall { checked / 1000 } else { 0 };
            assert!(facing <= slivers, "{shape:?}: {facing} of {checked} triangles face away from their normals");
            assert!(mirrored <= slivers, "{shape:?}: {mirrored} of {checked} triangles show the texture mirrored");
        }
    }
}
