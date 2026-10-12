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
    /// Where the height that moves it is read: its own place in the texture, or that of the
    /// vertex it is one with (see [`weld`]).
    height_uv: [f32; 2],
}

/// The vertices of a model that are at the same place and face the same way are one where the
/// height moves them: each reads the height where the first of them does. A model is laid out
/// in pieces — the faces of a cube, the ends of a surface that closes on itself, the rows that
/// meet at a pole, the islands a modeler cut it into — whose vertices at the joins have each
/// their own place in the texture: moved each by its own height, the pieces came apart (cracks
/// along the edges of the cube wherever two faces did not show the same of the texture, a star
/// at the poles of the sphere).
fn weld(vertices: &mut [Vertex]) {
    use std::collections::HashMap;
    // Where a vertex is, to a fifth of a thousandth of the model, and which way it faces, to a
    // few degrees. (On two grids, the second half a cell off: two vertices nearer than half a
    // cell are in one cell of one of them.)
    const CELL: f32 = 2e-4;
    let key = |v: &Vertex, off: f32| {
        let p = v.pos.map(|c| (c / CELL + off).floor() as i32);
        let n = v.normal.map(|c| (c * 8.0).round() as i32);
        [p[0], p[1], p[2], n[0], n[1], n[2]]
    };
    let mut firsts: [HashMap<[i32; 6], u32>; 2] = [HashMap::with_capacity(vertices.len()), HashMap::with_capacity(vertices.len())];
    for i in 0..vertices.len() {
        let mut one = None;
        for (first, off) in firsts.iter_mut().zip([0.0, 0.5]) {
            match first.entry(key(&vertices[i], off)) {
                std::collections::hash_map::Entry::Occupied(found) => one = one.or(Some(*found.get())),
                std::collections::hash_map::Entry::Vacant(free) => drop(free.insert(i as u32)),
            }
        }
        if let Some(first) = one {
            vertices[i].height_uv = vertices[first as usize].height_uv;
        }
    }
}

/// How finely a model is laid over its texture: how many of its vertices a unit of the texture
/// has across it (by the middle one of the sides of its triangles). The height that moves the
/// vertices is read as blurred as they are far apart (see the shader's `vertex_level`): read
/// sharp, each vertex took whatever pixel it fell on, and a step of the height — the edge of a
/// brick — came out as teeth along it.
pub(super) fn density(vertices: &[Vertex], indices: &[u32]) -> f32 {
    let mut sides: Vec<f32> = indices
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|t| {
            [(0, 1), (1, 2), (2, 0)].map(|(a, b)| {
                let (p, q) = (vertices[t[a] as usize].uv, vertices[t[b] as usize].uv);
                (p[0] - q[0]).hypot(p[1] - q[1])
            })
        })
        .filter(|side| *side > 1e-7)
        .collect();
    if sides.is_empty() {
        return 1.0;
    }
    let middle = sides.len() / 2;
    let (_, side, _) = sides.select_nth_unstable_by(middle, f32::total_cmp);
    1.0 / *side
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

/// A grid of `columns` x `rows` quads from `f(u, v) -> (position, normal, tangent)`: as many
/// each way as leave its quads about square in the texture (the height is read as blurred as
/// the vertices are far apart the coarser way: see [`density`]).
fn grid(columns: u32, rows: u32, uv_scale: [f32; 2], f: impl Fn(f32, f32) -> ([f32; 3], [f32; 3], [f32; 3]), out: &mut (Vec<Vertex>, Vec<u32>)) {
    let even = |n: u32| -> Vec<f32> { (0..=n).map(|i| i as f32 / n as f32).collect() };
    grid_at(&even(columns), &even(rows), uv_scale, f, out);
}

/// A grid of quads from `f(u, v)`, at the `u` of `across` and the `v` of `down` (each from 0 to
/// 1, in order): where they are close together, the surface is finer, its texture laid on it as
/// evenly as elsewhere (`u` and `v` are its coordinates, wherever they are taken).
fn grid_at(
    across: &[f32],
    down: &[f32],
    uv_scale: [f32; 2],
    f: impl Fn(f32, f32) -> ([f32; 3], [f32; 3], [f32; 3]),
    out: &mut (Vec<Vertex>, Vec<u32>),
) {
    let base = out.0.len() as u32;
    for &v in down {
        for &u in across {
            let (pos, normal, tangent) = f(u, v);
            let uv = [u * uv_scale[0], v * uv_scale[1]];
            out.0.push(Vertex { pos, normal, tangent: [tangent[0], tangent[1], tangent[2], 1.0], uv, height_uv: uv });
        }
    }
    quads(base, across.len() as u32 - 1, down.len() as u32 - 1, &mut out.1);
}

/// A box of half-size `half` whose edges are rounded by `radius`: the surface of a box smaller
/// by `radius` grown by a ball of it. Every point of a face is pushed out from the nearest point
/// of that box, which leaves the middle of a face flat and rounds the edges and corners; the
/// grid has `across` steps over the flat of a face and `round` over each rounded end, finest
/// where it rounds. Two faces meet along the middle of an edge facing the same way (halfway
/// between them), so that what a height makes of them holds together there (see [`weld`]).
fn rounded_box(half: f32, radius: f32, across: u32, round: u32, out: &mut (Vec<Vertex>, Vec<u32>)) {
    let (h, r) = (half, radius);
    let at = banded(r / h / 2.0, round, across);
    for (n, a, b) in CUBE_FACES {
        grid_at(
            &at,
            &at,
            [1.0, 1.0],
            |u, v| {
                let (s, t) = (u * 2.0 - 1.0, v * 2.0 - 1.0);
                let q = [(n[0] + a[0] * s + b[0] * t) * h, (n[1] + a[1] * s + b[1] * t) * h, (n[2] + a[2] * s + b[2] * t) * h];
                let core = q.map(|c| c.clamp(-(h - r), h - r));
                let out = [q[0] - core[0], q[1] - core[1], q[2] - core[2]];
                let normal = normalize(out);
                let p = [core[0] + normal[0] * r, core[1] + normal[1] * r, core[2] + normal[2] * r];
                // The face's own u axis, along the surface: it turns with the edges.
                let along = dot(a, normal);
                let tangent = normalize([a[0] - normal[0] * along, a[1] - normal[1] * along, a[2] - normal[2] * along]);
                (p, normal, tangent)
            },
            out,
        );
    }
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
/// surface is as fine all over. (Pieces meet facing the same way — an edge is an arc, however
/// small: where two met at an angle, each side of the edge moved its own way with the height,
/// and the surface opened along it.)
fn lathe(pieces: &[Piece], rows: u32, around: u32, repeats: f32, size: f32, out: &mut (Vec<Vertex>, Vec<u32>)) {
    use std::f32::consts::TAU;
    let total: f32 = pieces.iter().map(|p| p.length()).sum();
    let mut along = 0.0;
    for piece in pieces {
        let length = piece.length();
        // (An arc has rows enough to turn in, however short it is.)
        let least = if matches!(piece, Piece::Arc { .. }) { 4 } else { 1 };
        let n = ((length / total * rows as f32).ceil() as u32).max(least);
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
                let uv = [u * repeats, v];
                out.0.push(Vertex { pos: [sin * r, y, cos * r], normal: [sin * nr, ny, cos * nr], tangent: [cos, 0.0, -sin, 1.0], uv, height_uv: uv });
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
        384,
        128,
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
/// indices, the vertices as twelve floats each (position, normal, tangent, texture coordinates),
/// the indices.
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
    // (Twelve floats a vertex: where it is, how it faces, its tangent, its place in the texture.)
    const SIZE: usize = 12 * std::mem::size_of::<f32>();
    let (from, to) = (8, 8 + vertices * SIZE);
    if data.len() != to + indices * 4 {
        return Err("of another size".into());
    }
    let vs = data[from..to]
        .as_chunks::<SIZE>()
        .0
        .iter()
        .map(|b| {
            let f = bytemuck::pod_read_unaligned::<[f32; 12]>(b);
            let uv = [f[10], f[11]];
            Vertex { pos: [f[0], f[1], f[2]], normal: [f[3], f[4], f[5]], tangent: [f[6], f[7], f[8], f[9]], uv, height_uv: uv }
        })
        .collect();
    let is: Vec<u32> = data[to..].as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)).collect();
    if is.iter().any(|&i| i as usize >= vertices) {
        return Err("a triangle of a vertex it has not".into());
    }
    Ok((vs, is))
}

/// The model of `shape`: its vertices and its triangles, its vertices one where its pieces meet
/// (see [`weld`]).
pub(super) fn build_mesh(shape: Shape) -> (Vec<Vertex>, Vec<u32>) {
    let mut mesh = pieces_of(shape);
    weld(&mut mesh.0);
    mesh
}

fn pieces_of(shape: Shape) -> (Vec<Vertex>, Vec<u32>) {
    use std::f32::consts::{FRAC_PI_2, PI, TAU};
    let mut out = (Vec::new(), Vec::new());
    match shape {
        // Its edges rounded by a thirty-second of its side: enough to catch the light as the
        // edge of a real thing does, the faces flat to their ends for a texture to be judged
        // on (a quarter of each face was round, then a sixteenth, a thirty-second and a
        // sixty-fourth of the side).
        Shape::RoundedCube => rounded_box(0.8, 1.6 / 32.0, 192, 16, &mut out),
        Shape::Sphere => grid(
            384,
            192,
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
            256,
            [1.0, 1.0],
            |u, v| ([u * 2.4 - 1.2, 1.2 - v * 2.4, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
            &mut out,
        ),
        // Its edges as sharp as an edge is seen: rounded by a five-hundredth of its side, which
        // no eye takes for round. (Square, each face moved its own way with the height: the
        // cube opened along every edge.)
        Shape::Cube => rounded_box(0.8, 1.6 / 512.0, 192, 3, &mut out),
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
                384,
                3.0,
                TAU * R / 3.0,
                &mut out,
            );
        }
        Shape::Cylinder => {
            // (Its edges as sharp as an edge is seen: see the cube.)
            const R: f32 = 0.8;
            const E: f32 = 0.004;
            lathe(
                &[
                    Piece::Line([0.0, 1.0], [R - E, 1.0]),
                    Piece::Arc { center: [R - E, 1.0 - E], radius: E, from: 0.0, to: FRAC_PI_2 },
                    Piece::Line([R, 1.0 - E], [R, -(1.0 - E)]),
                    Piece::Arc { center: [R - E, -(1.0 - E)], radius: E, from: FRAC_PI_2, to: PI },
                    Piece::Line([R - E, -1.0], [0.0, -1.0]),
                ],
                256,
                320,
                2.0,
                TAU * R / 2.0,
                &mut out,
            );
        }
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

    /// The pieces of every model hold together whatever height moves them: the vertices that are
    /// at one place face one way and read the height at one place of the texture, so that a
    /// height (any: one that does not even tile) moves them to one place again. The faces of the
    /// cube, the ends of what closes on itself, the poles of the sphere, the edges of the
    /// cylinder (rounded as little as an edge is seen: square, each side moved its own way and
    /// the surface opened). The Matball, made elsewhere, has edges it is creased along: there
    /// its sides face their own ways, and only what faces one way holds together.
    #[test]
    fn the_pieces_of_every_model_hold_together_under_a_height() {
        use std::collections::HashMap;
        // A height that is the same nowhere twice: of the place in the texture alone.
        let height = |uv: [f32; 2]| (uv[0] * 12.9898 + uv[1] * 78.233).sin() * 0.5 + (uv[0] * 3.7 - uv[1] * 5.1).cos() * 0.5;
        for shape in Shape::ALL {
            let (v, _) = build_mesh(shape);
            // The vertices by where they are (to less than their models are fine).
            let mut places: HashMap<[i32; 3], Vec<usize>> = HashMap::new();
            for (i, x) in v.iter().enumerate() {
                places.entry(x.pos.map(|c| (c * 5000.0).round() as i32)).or_default().push(i);
            }
            let (mut joined, mut creased, mut apart) = (0, 0, Vec::new());
            for at in places.values().filter(|at| at.len() > 1) {
                let first = v[at[0]];
                for &i in &at[1..] {
                    let x = v[i];
                    if sub(x.pos, first.pos).iter().any(|d| d.abs() > 1e-4) {
                        continue;
                    }
                    if dot(x.normal, first.normal) < 0.9999 {
                        creased += 1;
                        continue;
                    }
                    joined += 1;
                    let moved = |x: &Vertex| [0, 1, 2].map(|k| x.pos[k] + x.normal[k] * height(x.height_uv) * 0.1);
                    let gap = sub(moved(&x), moved(&first)).iter().fold(0.0f32, |m, d| m.max(d.abs()));
                    // (The Matball's sides at a join face nearly one way, where a modeler's
                    // normals lean a little apart: as near as that, at ten times the height a
                    // view moves a model by.)
                    if gap > if shape == Shape::MatBall { 2e-3 } else { 1e-4 } {
                        apart.push(format!("at {:?}: {gap} apart, reading the height at {:?} and {:?}", x.pos, x.height_uv, first.height_uv));
                    }
                }
            }
            assert!(apart.is_empty(), "{shape:?} comes apart in {} places: {:?}", apart.len(), &apart[..apart.len().min(3)]);
            // What has joins has them joined, and nothing built here is creased.
            match shape {
                Shape::Plane | Shape::Cloth => assert_eq!(joined, 0, "{shape:?} is of one piece"),
                Shape::MatBall => assert!(joined > 100, "{shape:?}: {joined} joined, {creased} creased"),
                _ => assert!(joined > 100 && creased == 0, "{shape:?}: {joined} joined, {creased} creased"),
            }
        }
    }

    /// How finely each model is laid over its texture (see [`density`]): the cube as its grid
    /// says, every model fine enough for a relief, and about as fine both ways (the height is
    /// read as blurred as the coarser way asks).
    #[test]
    fn models_are_fine_enough_for_a_relief() {
        for shape in Shape::ALL {
            let (v, i) = build_mesh(shape);
            let d = density(&v, &i);
            assert!((90.0..=1200.0).contains(&d), "{shape:?}: {d} vertices to a unit of the texture");
            if shape == Shape::RoundedCube {
                // (192 steps over what its rounded ends, a thirty-second of the side each,
                // leave of the texture.)
                assert!((d - 192.0 / (1.0 - 2.0 / 32.0)).abs() < 2.0, "{d}");
            }
            assert!(v.iter().all(|x| x.height_uv.iter().all(|c| c.is_finite())), "{shape:?}");
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
