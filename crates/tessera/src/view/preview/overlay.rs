//! Interaction layer of the 2D view: zoom and pan, pixel probe and the Transform 2D gizmo.

use iced::advanced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::{Color, Point, Rectangle, Size, Vector, keyboard};
use tessera_core::graph::NodeId;
use tessera_core::{Edit, NodeKind, ParamValue};

use crate::app::{App, Message, PreviewMessage};
use crate::style;

/// Parameters of a Transform 2D node as the gizmo sees them.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Xform {
    offset: [f32; 2],
    rotation: f32,
    scale: [f32; 2],
    pivot: [f32; 2],
}

fn rotate_ccw(p: Vector, deg: f32) -> Vector {
    let (s, c) = deg.to_radians().sin_cos();
    Vector::new(c * p.x + s * p.y, -s * p.x + c * p.y)
}

impl Xform {
    /// Source uv -> destination uv (the inverse of what transform.wgsl computes).
    fn forward(&self, src: Vector, aspect: f32) -> Vector {
        let pivot = Vector::new(self.pivot[0], self.pivot[1]);
        let mut d = Vector::new((src.x - pivot.x) * self.scale[0], (src.y - pivot.y) * self.scale[1]);
        d.x *= aspect;
        d = rotate_ccw(d, self.rotation);
        d.x /= aspect;
        pivot + Vector::new(self.offset[0], self.offset[1]) + d
    }

    /// Destination uv -> offset from the pivot before rotation (scaled source space).
    fn local(&self, dest: Vector, aspect: f32) -> Vector {
        let mut d = dest - Vector::new(self.pivot[0] + self.offset[0], self.pivot[1] + self.offset[1]);
        d.x *= aspect;
        d = rotate_ccw(d, -self.rotation);
        d.x /= aspect;
        d
    }

    fn center(&self) -> Vector {
        Vector::new(self.pivot[0] + self.offset[0], self.pivot[1] + self.offset[1])
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Handle {
    Move,
    Rotate,
    /// Source-space point being dragged, and which axes it scales.
    Scale { src: [f32; 2], x: bool, y: bool },
}

#[derive(Clone, Copy, Debug)]
enum Drag {
    Pan { last: Point },
    Gizmo { handle: Handle, start_uv: Vector, start_screen: Point, start: Xform },
}

#[derive(Default)]
pub struct State {
    drag: Option<Drag>,
    last_click: Option<mouse::Click>,
    last_probe: Option<[u32; 2]>,
    bounds: Rectangle,
    modifiers: keyboard::Modifiers,
    hovered: Option<Handle>,
}

pub struct Overlay<'a> {
    pub app: &'a App,
    /// Size of the shown image in pixels.
    pub image: Option<[u32; 2]>,
}

const RING: f32 = 64.0;

/// Image rectangle in widget-local coordinates.
pub fn image_rect(bounds: Size, image: [u32; 2], zoom: f32, pan: Vector) -> Rectangle {
    let size = Size::new(image[0] as f32 * zoom, image[1] as f32 * zoom);
    Rectangle::new(
        Point::new(bounds.width / 2.0 + pan.x - size.width / 2.0, bounds.height / 2.0 + pan.y - size.height / 2.0),
        size,
    )
}

/// Zoom and pan that fit an image into the view.
pub fn fit(bounds: Size, image: [u32; 2]) -> (f32, Vector) {
    let zoom = ((bounds.width - 24.0) / image[0] as f32).min((bounds.height - 24.0) / image[1] as f32).max(0.01);
    (zoom, Vector::ZERO)
}

impl Overlay<'_> {
    /// Is the view showing the repetitions of the image around itself?
    fn tiling(&self) -> bool {
        let Some(id) = self.app.ui.preview.node else { return false };
        self.app.ui.preview.show_tiling && self.app.eval.params(id).map_or(0, |x| x.tiling()) != 0
    }

    fn gizmo_node(&self) -> Option<NodeId> {
        let id = self.app.ui.preview.node?;
        let node = self.app.graph().node(id)?;
        (node.kind == NodeKind::TRANSFORM_2D).then_some(id)
    }

    /// Effective transform (driven values included) and which parts are locked.
    fn xform(&self, id: NodeId) -> Option<(Xform, [bool; 4])> {
        let node = self.app.graph().node(id)?;
        let params = self.app.eval.params(id)?;
        let x = Xform {
            offset: params.v2("offset"),
            rotation: params.f("rotation"),
            scale: params.v2("scale"),
            pivot: params.v2("pivot"),
        };
        // Exposed parameters get their value from a socket: the gizmo must not edit them.
        let locked = ["offset", "rotation", "scale", "pivot"].map(|k| node.is_exposed(k));
        Some((x, locked))
    }

    fn handles(&self, xf: &Xform, rect: Rectangle, aspect: f32) -> (Point, Vec<(Point, Handle)>, [Point; 4]) {
        let to_screen = |uv: Vector| Point::new(rect.x + uv.x * rect.width, rect.y + uv.y * rect.height);
        let corners = [Vector::new(0.0, 0.0), Vector::new(1.0, 0.0), Vector::new(1.0, 1.0), Vector::new(0.0, 1.0)];
        let quad = corners.map(|c| to_screen(xf.forward(c, aspect)));
        let mut handles = Vec::new();
        for (i, c) in corners.iter().enumerate() {
            handles.push((quad[i], Handle::Scale { src: [c.x, c.y], x: true, y: true }));
        }
        for (m, x, y) in [
            (Vector::new(0.5, 0.0), false, true),
            (Vector::new(1.0, 0.5), true, false),
            (Vector::new(0.5, 1.0), false, true),
            (Vector::new(0.0, 0.5), true, false),
        ] {
            handles.push((to_screen(xf.forward(m, aspect)), Handle::Scale { src: [m.x, m.y], x, y }));
        }
        (to_screen(xf.center()), handles, quad)
    }

    fn hit(&self, p: Point, center: Point, handles: &[(Point, Handle)]) -> Option<Handle> {
        if p.distance(center) < 10.0 {
            return Some(Handle::Move);
        }
        if let Some((_, h)) = handles.iter().find(|(hp, _)| hp.distance(p) < 8.0) {
            return Some(*h);
        }
        ((p.distance(center) - RING).abs() < 6.0).then_some(Handle::Rotate)
    }

    #[allow(clippy::too_many_arguments)]
    fn gizmo_edits(&self, id: NodeId, drag: (Handle, Vector, Point, Xform), uv: Vector, p: Point, rect: Rectangle, aspect: f32, shift: bool) -> Vec<Edit> {
        let (handle, start_uv, start_screen, start) = drag;
        let Some((_, locked)) = self.xform(id) else { return Vec::new() };
        let set = |key: &str, value: ParamValue| Edit::SetParam { node: id, key: key.into(), value };
        match handle {
            Handle::Move if !locked[0] => {
                let d = uv - start_uv;
                let mut o = [start.offset[0] + d.x, start.offset[1] + d.y];
                if shift {
                    // Constrain to the dominant axis.
                    if d.x.abs() > d.y.abs() {
                        o[1] = start.offset[1];
                    } else {
                        o[0] = start.offset[0];
                    }
                }
                vec![set("offset", ParamValue::Vec2(o))]
            }
            Handle::Rotate if !locked[1] => {
                let c = Point::new(rect.x + start.center().x * rect.width, rect.y + start.center().y * rect.height);
                let a0 = (start_screen.y - c.y).atan2(start_screen.x - c.x);
                let a1 = (p.y - c.y).atan2(p.x - c.x);
                let mut r = start.rotation - (a1 - a0).to_degrees();
                r = (r + 180.0).rem_euclid(360.0) - 180.0;
                if shift {
                    r = (r / 15.0).round() * 15.0;
                }
                vec![set("rotation", ParamValue::Float(r))]
            }
            Handle::Scale { src, x, y } if !locked[2] => {
                let local = start.local(uv, aspect);
                let rel = Vector::new(src[0] - start.pivot[0], src[1] - start.pivot[1]);
                let mut s = start.scale;
                if x && rel.x.abs() > 1e-4 {
                    s[0] = local.x / rel.x;
                }
                if y && rel.y.abs() > 1e-4 {
                    s[1] = local.y / rel.y;
                }
                if shift {
                    let fx = if x { s[0] / start.scale[0] } else { s[1] / start.scale[1] };
                    let fy = if y { s[1] / start.scale[1] } else { fx };
                    let f = if x && y { (fx + fy) * 0.5 } else { fx.max(fy) };
                    s = [start.scale[0] * f, start.scale[1] * f];
                }
                let clamp = |v: f32| if v.abs() < 0.01 { 0.01f32.copysign(v) } else { v.clamp(-64.0, 64.0) };
                vec![set("scale", ParamValue::Vec2([clamp(s[0]), clamp(s[1])]))]
            }
            _ => Vec::new(),
        }
    }
}

impl canvas::Program<Message> for Overlay<'_> {
    type State = State;

    fn update(&self, state: &mut State, event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Message>> {
        use iced::Event;
        let p = &self.app.ui.preview;
        if state.bounds != bounds {
            state.bounds = bounds;
            return Some(canvas::Action::publish(Message::Preview(PreviewMessage::Bounds(bounds))));
        }
        if let Event::Keyboard(keyboard::Event::ModifiersChanged(m)) = event {
            state.modifiers = *m;
        }
        let image = self.image?;
        if p.fit && bounds.width > 10.0 {
            let (zoom, pan) = fit(bounds.size(), image);
            return Some(canvas::Action::publish(Message::Preview(PreviewMessage::View { zoom, pan, fit: false })));
        }
        let rect = image_rect(bounds.size(), image, p.zoom, p.pan);
        let local = cursor.position_in(bounds);
        let uv_of = |q: Point| Vector::new((q.x - rect.x) / rect.width, (q.y - rect.y) / rect.height);
        let aspect = image[0] as f32 / image[1] as f32;
        let gizmo = self.gizmo_node().and_then(|id| self.xform(id).map(|x| (id, x)));

        match event {
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let q = local?;
                let dy = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y * 60.0,
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                let zoom = (p.zoom * (dy * 0.002).exp()).clamp(0.02, 64.0);
                // Keep the image point under the cursor fixed.
                let center = Point::new(bounds.width / 2.0 + p.pan.x, bounds.height / 2.0 + p.pan.y);
                let rel = (q - center) * (1.0 / p.zoom);
                let pan = q - Point::new(bounds.width / 2.0, bounds.height / 2.0) - rel * zoom;
                Some(canvas::Action::publish(Message::Preview(PreviewMessage::View { zoom, pan, fit: false })).and_capture())
            }
            Event::Mouse(mouse::Event::ButtonPressed(button)) => {
                let q = local?;
                match button {
                    mouse::Button::Left => {
                        let click = mouse::Click::new(q, mouse::Button::Left, state.last_click);
                        state.last_click = Some(click);
                        if let Some((_, (xf, _))) = &gizmo {
                            let (center, handles, _) = self.handles(xf, rect, aspect);
                            if let Some(h) = self.hit(q, center, &handles) {
                                state.drag = Some(Drag::Gizmo { handle: h, start_uv: uv_of(q), start_screen: q, start: *xf });
                                return Some(canvas::Action::capture());
                            }
                        }
                        if matches!(click.kind(), mouse::click::Kind::Double) {
                            return Some(canvas::Action::publish(Message::Preview(PreviewMessage::Fit)).and_capture());
                        }
                        state.drag = Some(Drag::Pan { last: q });
                        Some(canvas::Action::capture())
                    }
                    mouse::Button::Middle | mouse::Button::Right => {
                        state.drag = Some(Drag::Pan { last: q });
                        Some(canvas::Action::capture())
                    }
                    _ => None,
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let q = *position - Vector::new(bounds.x, bounds.y);
                match &mut state.drag {
                    Some(Drag::Pan { last }) => {
                        let d = q - *last;
                        *last = q;
                        let pan = p.pan + d;
                        Some(canvas::Action::publish(Message::Preview(PreviewMessage::View { zoom: p.zoom, pan, fit: false })))
                    }
                    Some(Drag::Gizmo { handle, start_uv, start_screen, start }) => {
                        let (id, _) = gizmo?;
                        let edits = self.gizmo_edits(id, (*handle, *start_uv, *start_screen, *start), uv_of(q), q, rect, aspect, state.modifiers.shift());
                        if edits.is_empty() {
                            None
                        } else {
                            Some(canvas::Action::publish(Message::Edits(edits)))
                        }
                    }
                    None => {
                        // Hover: gizmo highlight and pixel probe.
                        let hovered = match (&gizmo, local) {
                            (Some((_, (xf, _))), Some(q)) => {
                                let (center, handles, _) = self.handles(xf, rect, aspect);
                                self.hit(q, center, &handles)
                            }
                            _ => None,
                        };
                        let redraw = hovered != state.hovered;
                        state.hovered = hovered;
                        // The pixel under the cursor, in whichever copy of the image it is over.
                        let px = local.and_then(|q| {
                            let tiling = self.app.eval.params(p.node?).map_or(3, |x| x.tiling());
                            let tile = [p.show_tiling && tiling & 1 != 0, p.show_tiling && tiling & 2 != 0];
                            let uv = tile_uv(uv_of(q), tiling_lattice(self.app, p.node, image), tile)?;
                            let px = |v: f32, n: u32| ((v * n as f32) as u32).min(n.saturating_sub(1));
                            Some([px(uv.x, image[0]), px(uv.y, image[1])])
                        });
                        if px != state.last_probe {
                            state.last_probe = px;
                            return Some(canvas::Action::publish(Message::Preview(PreviewMessage::Probe(px))));
                        }
                        redraw.then(canvas::Action::request_redraw)
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(_)) => match state.drag.take() {
                Some(Drag::Gizmo { .. }) => Some(canvas::Action::publish(Message::Seal)),
                Some(Drag::Pan { .. }) => None,
                None => None,
            },
            _ => None,
        }
    }

    fn mouse_interaction(&self, state: &State, _bounds: Rectangle, _cursor: mouse::Cursor) -> mouse::Interaction {
        match state.drag {
            Some(Drag::Pan { .. }) | Some(Drag::Gizmo { .. }) => mouse::Interaction::Grabbing,
            None if state.hovered.is_some() => mouse::Interaction::Grab,
            None => mouse::Interaction::default(),
        }
    }

    fn draw(&self, state: &State, renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, cursor: mouse::Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let p = &self.app.ui.preview;
        let Some(image) = self.image else { return vec![frame.into_geometry()] };
        let rect = image_rect(bounds.size(), image, p.zoom, p.pan);
        // Where the texture itself is: a quiet line around it, or — when its repetitions
        // fill the view and every one of them looks the same — a red frame around the one
        // that is the texture, while the cursor is on it.
        if self.tiling() {
            if cursor.position_in(bounds).is_some_and(|q| rect.contains(q)) {
                frame.stroke_rectangle(rect.position(), rect.size(), Stroke::default().with_width(2.0).with_color(style::ERROR));
            }
        } else {
            frame.stroke_rectangle(rect.position(), rect.size(), Stroke::default().with_width(1.0).with_color(style::gray(72)));
        }

        if let Some(id) = self.gizmo_node()
            && let Some((xf, locked)) = self.xform(id)
        {
            let aspect = image[0] as f32 / image[1] as f32;
            let (center, handles, quad) = self.handles(&xf, rect, aspect);
            let active = match state.drag {
                Some(Drag::Gizmo { handle, .. }) => Some(handle),
                _ => state.hovered,
            };
            let col = |h: Handle, locked: bool| {
                if locked {
                    style::gray(112)
                } else if active == Some(h) {
                    Color::WHITE
                } else {
                    style::ACCENT
                }
            };
            let outline = Path::new(|b| {
                b.move_to(quad[0]);
                for q in &quad[1..] {
                    b.line_to(*q);
                }
                b.close();
            });
            frame.stroke(&outline, Stroke::default().with_width(1.5).with_color(style::alpha(style::ACCENT, 0.9)));
            let ring_w = if active == Some(Handle::Rotate) { 2.5 } else { 1.2 };
            frame.stroke(&Path::circle(center, RING), Stroke::default().with_width(ring_w).with_color(col(Handle::Rotate, locked[1])));
            // Where the image's "up" points.
            let up = {
                let to_screen = |uv: Vector| Point::new(rect.x + uv.x * rect.width, rect.y + uv.y * rect.height);
                to_screen(xf.forward(Vector::new(xf.pivot[0], xf.pivot[1] - 0.5), aspect)) - center
            };
            let len = (up.x * up.x + up.y * up.y).sqrt();
            if len > 1.0 {
                let tip = center + up * (RING / len);
                frame.stroke(&Path::line(center, tip), Stroke::default().with_width(1.0).with_color(col(Handle::Rotate, locked[1])));
                frame.fill(&Path::circle(tip, 4.0), col(Handle::Rotate, locked[1]));
            }
            for (hp, h) in &handles {
                let r = Rectangle::new(Point::new(hp.x - 4.5, hp.y - 4.5), Size::new(9.0, 9.0));
                frame.fill_rectangle(r.position(), r.size(), Color::from_rgba(0.0, 0.0, 0.0, 0.6));
                frame.stroke_rectangle(r.position(), r.size(), Stroke::default().with_width(1.5).with_color(col(*h, locked[2])));
            }
            frame.fill(&Path::circle(center, 6.0), Color::from_rgba(0.0, 0.0, 0.0, 0.6));
            frame.stroke(&Path::circle(center, 6.0), Stroke::default().with_width(2.0).with_color(col(Handle::Move, locked[0])));
        }

        vec![frame.into_geometry()]
    }
}

/// The two vectors the content of a node repeats along, in image widths and heights.
///
/// For nearly every node that is the image itself. A Transform 2D with tiling on wraps its
/// *source*, so what it draws repeats along its own transformed axes — the sides of the
/// gizmo's quad. Repeating the image instead would show seams that the texture does not
/// have.
pub fn tiling_lattice(app: &App, node: Option<NodeId>, image: [u32; 2]) -> [[f32; 2]; 2] {
    let square = [[1.0, 0.0], [0.0, 1.0]];
    let Some(id) = node else { return square };
    if app.graph().node(id).map(|n| n.kind) != Some(NodeKind::TRANSFORM_2D) {
        return square;
    }
    let Some(params) = app.eval.params(id) else { return square };
    let xf = Xform {
        offset: params.v2("offset"),
        rotation: params.f("rotation"),
        scale: params.v2("scale"),
        pivot: params.v2("pivot"),
    };
    let aspect = image[0].max(1) as f32 / image[1].max(1) as f32;
    let origin = xf.forward(Vector::ZERO, aspect);
    let side = |v: Vector| {
        let d = xf.forward(v, aspect) - origin;
        [d.x, d.y]
    };
    let (a, b) = (side(Vector::new(1.0, 0.0)), side(Vector::new(0.0, 1.0)));
    // Only when one whole repetition fits in the image: otherwise the image holds less
    // than the pattern repeats by, no copy of it can fill the gaps between the copies, and
    // what the view has to show is what the texture really does — the image side by side
    // with itself, seams and all.
    let fits = a[0].abs() + b[0].abs() <= 1.0 && a[1].abs() + b[1].abs() <= 1.0;
    if fits { [a, b] } else { square }
}

/// Where a point of the view falls in the image, following the repetitions the view draws:
/// `uv` is in image widths from its top-left corner, `lattice` is what the image repeats
/// along, and `tile` says along which of the two. `None` when the point shows no copy of
/// the image.
pub fn tile_uv(uv: Vector, lattice: [[f32; 2]; 2], tile: [bool; 2]) -> Option<Vector> {
    let [a, b] = lattice;
    let det = a[0] * b[1] - a[1] * b[0];
    // A lattice with no area (a transform scaled to nothing) repeats nothing.
    let ([a, b], det) =
        if det.abs() > 1e-6 && [a[0], a[1], b[0], b[1]].iter().all(|v| v.is_finite()) { ([a, b], det) } else { ([[1.0, 0.0], [0.0, 1.0]], 1.0) };
    let steps = [
        (b[1] * uv.x - b[0] * uv.y) / det,
        (a[0] * uv.y - a[1] * uv.x) / det,
    ];
    let base = [
        if tile[0] { steps[0].floor() } else { 0.0 },
        if tile[1] { steps[1].floor() } else { 0.0 },
    ];
    // The nearest copy first: a lattice larger than the image puts it a step or two away.
    const ORDER: [f32; 5] = [0.0, -1.0, 1.0, -2.0, 2.0];
    let steps_x = if tile[0] { &ORDER[..] } else { &ORDER[..1] };
    let steps_y = if tile[1] { &ORDER[..] } else { &ORDER[..1] };
    for i in steps_x {
        for j in steps_y {
            let (m, n) = (base[0] + i, base[1] + j);
            let q = Vector::new(uv.x - m * a[0] - n * b[0], uv.y - m * a[1] - n * b[1]);
            if (0.0..=1.0).contains(&q.x) && (0.0..=1.0).contains(&q.y) {
                return Some(q);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Without a transform the image simply repeats along its own sides, and only along the
    /// axes it tiles on.
    #[test]
    fn the_image_repeats_along_its_own_sides() {
        let square = [[1.0, 0.0], [0.0, 1.0]];
        let uv = Vector::new(2.25, -1.75);
        assert_eq!(tile_uv(uv, square, [true, true]), Some(Vector::new(0.25, 0.25)));
        // Only horizontally: above and below the image there is nothing.
        assert_eq!(tile_uv(Vector::new(2.25, 0.5), square, [true, false]), Some(Vector::new(0.25, 0.5)));
        assert_eq!(tile_uv(uv, square, [true, false]), None);
        assert_eq!(tile_uv(uv, square, [false, false]), None);
        assert_eq!(tile_uv(Vector::new(0.5, 0.5), square, [false, false]), Some(Vector::new(0.5, 0.5)));
    }

    /// A rotated and scaled Transform 2D repeats along its own axes: a point one step away
    /// along them shows exactly the same pixel of the image, which is what makes the copies
    /// meet.
    #[test]
    fn a_transform_repeats_along_its_gizmo() {
        // Half size, turned by 30°, as the gizmo would show it on a square image.
        let (s, angle) = (0.5f32, 30.0f32.to_radians());
        let (sin, cos) = angle.sin_cos();
        let a = [s * cos, -s * sin];
        let b = [s * sin, s * cos];
        let lattice = [a, b];
        let inside = Vector::new(0.3, 0.4);
        for (m, n) in [(1.0, 0.0), (0.0, 1.0), (-1.0, 2.0), (2.0, -1.0)] {
            let away = Vector::new(inside.x + m * a[0] + n * b[0], inside.y + m * a[1] + n * b[1]);
            let back = tile_uv(away, lattice, [true, true]).expect("a copy of the image");
            assert!((back.x - inside.x).abs() < 1e-4 && (back.y - inside.y).abs() < 1e-4, "{back:?} from {away:?}");
        }
        // The lattice of a transform that scales to nothing is ignored, not divided by.
        assert_eq!(tile_uv(Vector::new(1.5, 0.5), [[0.0, 0.0], [0.0, 0.0]], [true, true]), Some(Vector::new(0.5, 0.5)));
        assert_eq!(tile_uv(Vector::new(1.5, 0.5), [[f32::NAN, 0.0], [0.0, 1.0]], [true, true]), Some(Vector::new(0.5, 0.5)));
    }
}
