//! What places the nodes that make panoramas in the 2D view (overlay.rs draws it and takes the
//! mouse), as Substance Designer's HDRI tools are placed: the points of a gradient (where its top
//! color is) and of a tube (where it begins and ends), dragged where they should be; and the gizmo
//! of a softbox or a panel, its outline drawn on the panorama as it is seen there (curved far from
//! its middle), with grips that move it, size it and turn it. A point of a panorama is across (0
//! to 1, once round) and down (0 straight up, 1 straight down).

use brewed_texture_core::graph::NodeId;
use brewed_texture_core::{Edit, NodeKind, ParamValue};
use iced::Vector;

use crate::app::App;

/// The points of a node of panoramas, as the graph gives them.
#[derive(Clone, Debug)]
pub struct Points {
    node: NodeId,
    points: Vec<(&'static str, Vector)>,
    /// The points that take their values from sockets: they stay where they are.
    locked: Vec<&'static str>,
}

impl Points {
    /// The points of the node `id`, if it makes a panorama.
    pub fn of(app: &App, id: NodeId) -> Option<Points> {
        let node = app.graph().node(id)?;
        let keys: &[&'static str] = if node.kind == NodeKind::HDRI_GRADIENT {
            &["point"]
        } else if node.kind == NodeKind::HDRI_LINE_LIGHT {
            &["point_1", "point_2"]
        } else {
            return None;
        };
        let params = app.eval.params(id)?;
        Some(Points {
            node: id,
            points: keys.iter().map(|k| (*k, Vector::from(params.v2(k)))).collect(),
            locked: keys.iter().copied().filter(|k| node.is_exposed(k)).collect(),
        })
    }

    /// Where the points are on the panorama.
    pub fn points(&self) -> impl Iterator<Item = Vector> + '_ {
        self.points.iter().map(|(_, at)| *at)
    }

    /// Whether the point `i` takes its value from a socket.
    pub fn locked(&self, i: usize) -> bool {
        self.points.get(i).is_some_and(|(k, _)| self.locked.contains(k))
    }

    /// What dragging the point `i` from `from` to `to` (points of the panorama) does: it moves as
    /// far, round and round across; Shift keeps it to one axis.
    pub fn edits(&self, i: usize, from: Vector, to: Vector, shift: bool) -> Vec<Edit> {
        let Some(&(key, at)) = self.points.get(i).filter(|_| !self.locked(i)) else { return Vec::new() };
        let mut by = to - from;
        if shift {
            if by.x.abs() > by.y.abs() {
                by.y = 0.0;
            } else {
                by.x = 0.0;
            }
        }
        let to = at + by;
        vec![Edit::SetParam { node: self.node, key: key.into(), value: ParamValue::Vec2([to.x.rem_euclid(1.0), to.y.clamp(0.0, 1.0)]) }]
    }
}

type V3 = [f32; 3];

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normalize(a: V3) -> V3 {
    scale(a, 1.0 / dot(a, a).sqrt().max(1e-9))
}

/// The direction a point of a panorama looks in (hdri.wgsl's `direction`).
fn direction(uv: Vector) -> V3 {
    use std::f32::consts::{PI, TAU};
    let (phi, theta) = ((uv.x - 0.5) * TAU, uv.y * PI);
    [theta.sin() * phi.sin(), theta.cos(), -theta.sin() * phi.cos()]
}

/// Where a direction is on a panorama, across as near `near` as it is round (it may be past 0
/// or 1: the same point of the panorama).
fn uv_near(d: V3, near: f32) -> Vector {
    use std::f32::consts::{PI, TAU};
    let d = normalize(d);
    let u = d[0].atan2(-d[2]) / TAU + 0.5;
    Vector::new(u - (u - near).round(), d[1].clamp(-1.0, 1.0).acos() / PI)
}

/// What the mouse takes of a softbox's or a panel's gizmo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Grip {
    /// Its middle: moves it.
    Move,
    /// An edge (-1 or 1 along one of its own axes) or a corner (along both): sizes it, as much
    /// on the other side.
    Size { x: i8, y: i8 },
    /// The knob beyond its top edge: turns it.
    Turn,
}

/// A softbox (Shape Light) or a panel (Plane Light) as its gizmo sees it.
#[derive(Clone, Debug)]
pub struct Panel {
    node: NodeId,
    kind: NodeKind,
    position: Vector,
    size: [f32; 2],
    rotation: f32,
    /// A Shape Light's: a rectangle (0) or a disc (1).
    shape: u32,
    /// A Plane Light's: facing the middle (0), a ceiling (1), a floor (2).
    placement: u32,
    /// What takes its value from a socket: the gizmo leaves it.
    locked: Vec<&'static str>,
}

/// A light's flat shape in the scene: its middle, its axes (turned as it is), which way it faces.
struct Plane {
    middle: V3,
    axes: [V3; 2],
    normal: V3,
}

/// How many points a line of the outline has round the light.
const AROUND: usize = 192;

impl Panel {
    pub fn of(app: &App, id: NodeId) -> Option<Panel> {
        let node = app.graph().node(id)?;
        if node.kind != NodeKind::HDRI_SHAPE_LIGHT && node.kind != NodeKind::HDRI_PLANE_LIGHT {
            return None;
        }
        let p = app.eval.params(id)?;
        Some(Panel {
            node: id,
            kind: node.kind,
            position: Vector::from(p.v2("position")),
            size: p.v2("size"),
            rotation: p.f("rotation"),
            shape: p.u("shape"),
            placement: p.u("placement"),
            locked: ["position", "size", "rotation"].into_iter().filter(|k| node.is_exposed(k)).collect(),
        })
    }

    /// Where it is in the scene, as its kernel puts it (hdri.wgsl): a step from the middle facing
    /// it, or a step above or below (a ceiling, a floor). None where it is not drawn (a ceiling
    /// seen below the horizon).
    fn plane(&self) -> Option<Plane> {
        let c = direction(self.position);
        let a = self.rotation.to_radians();
        if self.kind == NodeKind::HDRI_PLANE_LIGHT && self.placement != 0 {
            let up = if self.placement == 1 { 1.0 } else { -1.0 };
            if c[1] * up < 0.02 {
                return None;
            }
            let middle = scale(c, 1.0 / c[1].abs().max(0.02));
            let along = [middle[0], 0.0, middle[2]];
            let ahead = if dot(along, along) > 1e-8 { normalize(along) } else { [0.0, 0.0, -1.0] };
            let side = cross(ahead, [0.0, 1.0, 0.0]);
            let axes = [add(scale(side, a.cos()), scale(ahead, a.sin())), sub(scale(ahead, a.cos()), scale(side, a.sin()))];
            return Some(Plane { middle, axes, normal: [0.0, -up, 0.0] });
        }
        let up0 = if c[1].abs() > 0.999 { [0.0, 0.0, 1.0] } else { [0.0, 1.0, 0.0] };
        let right = normalize(cross(c, up0));
        let up = cross(right, c);
        let axes = [add(scale(right, a.cos()), scale(up, a.sin())), sub(scale(up, a.cos()), scale(right, a.sin()))];
        Some(Plane { middle: c, axes, normal: scale(c, -1.0) })
    }

    /// Where on its plane a point of the panorama is seen, along its axes; None where the plane is
    /// not seen at all.
    fn on_plane(plane: &Plane, uv: Vector) -> Option<[f32; 2]> {
        let d = direction(uv);
        let towards = dot(d, plane.normal);
        if towards.abs() < 1e-4 {
            return None;
        }
        let t = dot(plane.middle, plane.normal) / towards;
        if t <= 0.0 {
            return None;
        }
        let local = sub(scale(d, t), plane.middle);
        Some([dot(local, plane.axes[0]), dot(local, plane.axes[1])])
    }

    /// The direction of a point of its plane, along its axes.
    fn toward(plane: &Plane, x: f32, y: f32) -> V3 {
        add(plane.middle, add(scale(plane.axes[0], x), scale(plane.axes[1], y)))
    }

    /// How far out it reaches at an angle `t` from its first axis, as a part of its size that
    /// way: its kernel's edge.
    fn reach(&self, t: f32) -> f32 {
        let (c, s) = (t.cos().abs(), t.sin().abs());
        if self.kind == NodeKind::HDRI_PLANE_LIGHT {
            1.0 / c.max(s)
        } else if self.shape == 1 {
            1.0
        } else {
            // A rectangle with its corners a little round.
            (c.powi(8) + s.powi(8)).powf(-0.125)
        }
    }

    /// How far out a corner grip is, as a part of the size each way.
    fn corner(&self) -> f32 {
        self.reach(std::f32::consts::FRAC_PI_4) * std::f32::consts::FRAC_1_SQRT_2
    }

    /// Where a point of its plane is on the panorama, gone to from its middle (across as near
    /// it as it is round); None where the way there goes over a pole, where it is seen across.
    fn path(&self, plane: &Plane, x: f32, y: f32) -> Option<Vector> {
        let mut at = uv_near(Self::toward(plane, 0.0, 0.0), self.position.x);
        for i in 1..=16 {
            let k = i as f32 / 16.0;
            let next = uv_near(Self::toward(plane, x * k, y * k), at.x);
            if (next.x - at.x).abs() > 0.25 {
                return None;
            }
            at = next;
        }
        Some(at)
    }

    /// The grips, where they are on the panorama (the knob, which overlay.rs puts a few pixels
    /// beyond `stem`, aside); those seen over a pole left out.
    pub fn grips(&self) -> Vec<(Vector, Grip)> {
        let Some(plane) = self.plane() else { return vec![(self.position, Grip::Move)] };
        let [sx, sy] = self.size;
        let corner = self.corner();
        let mut grips = vec![(self.position, Grip::Move)];
        for (x, y) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, 1), (1, -1), (-1, -1)] {
            let k = if x != 0 && y != 0 { corner } else { 1.0 };
            if let Some(at) = self.path(&plane, sx * f32::from(x) * k, sy * f32::from(y) * k) {
                grips.push((at, Grip::Size { x, y }));
            }
        }
        grips
    }

    /// Where the knob that turns it stands out from: the middle of its top edge (of its bottom
    /// one where the top is seen over a pole), and a point a little further out that way.
    pub fn stem(&self) -> Option<(Vector, Vector)> {
        let plane = self.plane()?;
        let sy = self.size[1];
        [sy, -sy].into_iter().find_map(|y| Some((self.path(&plane, 0.0, y)?, self.path(&plane, 0.0, y * 1.04 + 0.004 * y.signum())?)))
    }

    /// Its outline on the panorama, as a line going on past the edges of the panorama (overlay.rs
    /// draws it once round each way too), broken where it goes over a pole.
    pub fn outline(&self) -> Vec<Vec<Vector>> {
        let Some(plane) = self.plane() else { return Vec::new() };
        let [sx, sy] = self.size;
        let mut lines = vec![Vec::new()];
        let mut last: Option<Vector> = None;
        for i in 0..=AROUND {
            let t = i as f32 / AROUND as f32 * std::f32::consts::TAU;
            let r = self.reach(t);
            let d = Self::toward(&plane, sx * r * t.cos(), sy * r * t.sin());
            let at = uv_near(d, last.map_or(self.position.x, |l| l.x));
            if let Some(l) = last
                && (at.x - l.x).abs() > 0.25
            {
                lines.push(Vec::new());
            }
            lines.last_mut().expect("a line").push(at);
            last = Some(at);
        }
        lines.retain(|l| l.len() > 1);
        lines
    }

    /// What dragging `grip` from `from` to `to` (points of the panorama) does to it as it was when
    /// taken (`self`). Shift keeps a move to one axis, its proportions, and turns it by 15°.
    pub fn edits(&self, grip: Grip, from: Vector, to: Vector, shift: bool) -> Vec<Edit> {
        if self.locked(grip) {
            return Vec::new();
        }
        let node = self.node;
        let set = |key: &str, value: ParamValue| vec![Edit::SetParam { node, key: key.into(), value }];
        let on = || {
            let plane = self.plane()?;
            Some((Self::on_plane(&plane, from)?, Self::on_plane(&plane, to)?))
        };
        let [sx, sy] = self.size;
        match grip {
            Grip::Move => {
                let mut by = to - from;
                if shift {
                    if by.x.abs() > by.y.abs() {
                        by.y = 0.0;
                    } else {
                        by.x = 0.0;
                    }
                }
                let at = self.position + by;
                set("position", ParamValue::Vec2([at.x.rem_euclid(1.0), at.y.clamp(0.0, 1.0)]))
            }
            Grip::Size { x, y } => {
                let Some((a, b)) = on() else { return Vec::new() };
                let k = if x != 0 && y != 0 { self.corner() } else { 1.0 };
                // Where the grip is taken to: as far off the cursor as it was when taken.
                let held = [sx * f32::from(x) * k - a[0], sy * f32::from(y) * k - a[1]];
                let mut size = self.size;
                if x != 0 {
                    size[0] = ((b[0] + held[0]) / k).abs();
                }
                if y != 0 {
                    size[1] = ((b[1] + held[1]) / k).abs();
                }
                if shift && sx > 1e-4 && sy > 1e-4 {
                    let f = match (x != 0, y != 0) {
                        (true, true) => (size[0] / sx + size[1] / sy) / 2.0,
                        (true, false) => size[0] / sx,
                        _ => size[1] / sy,
                    };
                    size = [sx * f, sy * f];
                }
                set("size", ParamValue::Vec2(size.map(|s| s.clamp(0.001, 50.0))))
            }
            Grip::Turn => {
                let Some((a, b)) = on() else { return Vec::new() };
                // Counter-clockwise from its up.
                let angle = |p: [f32; 2]| (-p[0]).atan2(p[1]);
                let mut r = self.rotation + (angle(b) - angle(a)).to_degrees();
                r = (r + 180.0).rem_euclid(360.0) - 180.0;
                if shift {
                    r = (r / 15.0).round() * 15.0;
                }
                set("rotation", ParamValue::Float(r))
            }
        }
    }

    /// Whether what a grip changes takes its value from a socket.
    pub fn locked(&self, grip: Grip) -> bool {
        let key = match grip {
            Grip::Move => "position",
            Grip::Size { .. } => "size",
            Grip::Turn => "rotation",
        };
        self.locked.contains(&key)
    }
}
