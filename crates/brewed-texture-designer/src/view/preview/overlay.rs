//! Interaction layer of the 2D view: zoom and pan, pixel probe, the Transform 2D gizmo and the
//! points of the lights of a panorama (lights.rs).

use iced::advanced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::{Color, Point, Rectangle, Size, Vector, keyboard};
use brewed_texture_core::graph::NodeId;
use brewed_texture_core::{Edit, NodeKind, ParamValue};

use super::lights::Points;
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
    /// A point of a light of a panorama.
    Point(usize),
}

#[derive(Clone, Debug)]
enum Drag {
    Pan { last: Point },
    Gizmo { handle: Handle, start_uv: Vector, start_screen: Point, start: Xform },
    /// A point of a light: which, where it was taken, and the points as they were then.
    Point { index: usize, start_uv: Vector, start: Points },
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

    /// The points of the light of a panorama shown.
    fn points(&self) -> Option<Points> {
        Points::of(self.app, self.app.ui.preview.node?)
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
        let points = self.points();

        match event {
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let q = local?;
                let dy = crate::view::wheel(self.app, delta);
                // Relative to what the view is when the app gets it: several turns of the
                // wheel can arrive in one frame.
                Some(canvas::Action::publish(Message::Preview(PreviewMessage::ZoomAt { at: q, factor: (dy * 0.002).exp() })).and_capture())
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
                        if let Some(ps) = &points
                            && let Some(index) = point_hit(ps, q, rect)
                        {
                            state.drag = Some(Drag::Point { index, start_uv: uv_of(q), start: ps.clone() });
                            return Some(canvas::Action::capture());
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
                        Some(canvas::Action::publish(Message::Preview(PreviewMessage::PanBy(d))))
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
                    Some(Drag::Point { index, start_uv, start }) => {
                        let edits = start.edits(*index, *start_uv, uv_of(q), state.modifiers.shift());
                        if edits.is_empty() {
                            None
                        } else {
                            Some(canvas::Action::publish(Message::Edits(edits)))
                        }
                    }
                    None => {
                        // Hover: gizmo highlight and pixel probe.
                        let hovered = match (&gizmo, &points, local) {
                            (Some((_, (xf, _))), _, Some(q)) => {
                                let (center, handles, _) = self.handles(xf, rect, aspect);
                                self.hit(q, center, &handles)
                            }
                            (None, Some(ps), Some(q)) => point_hit(ps, q, rect).map(Handle::Point),
                            _ => None,
                        };
                        let redraw = hovered != state.hovered;
                        state.hovered = hovered;
                        // The pixel under the cursor, in whichever copy of the image it is over.
                        let px = local.and_then(|q| {
                            let tiling = self.app.eval.params(p.node?).map_or(3, |x| x.tiling());
                            let tile = [p.show_tiling && tiling & 1 != 0, p.show_tiling && tiling & 2 != 0];
                            let uv = tile_uv(uv_of(q), tile)?;
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
                Some(Drag::Gizmo { .. } | Drag::Point { .. }) => Some(canvas::Action::publish(Message::Seal)),
                Some(Drag::Pan { .. }) => None,
                None => None,
            },
            _ => None,
        }
    }

    fn mouse_interaction(&self, state: &State, _bounds: Rectangle, _cursor: mouse::Cursor) -> mouse::Interaction {
        match state.drag {
            Some(Drag::Pan { .. } | Drag::Gizmo { .. } | Drag::Point { .. }) => mouse::Interaction::Grabbing,
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

        if let Some(points) = self.points() {
            let active = match &state.drag {
                Some(Drag::Point { index, .. }) => Some(*index),
                _ => match state.hovered {
                    Some(Handle::Point(i)) => Some(i),
                    _ => None,
                },
            };
            draw_points(&mut frame, &points, rect, active);
        }

        vec![frame.into_geometry()]
    }
}

/// Where a point of the image (0 to 1 across and down) is in the view.
fn on_screen(rect: Rectangle, uv: Vector) -> Point {
    Point::new(rect.x + uv.x * rect.width, rect.y + uv.y * rect.height)
}

/// The point of a light under `p`: the nearest of those it is on.
fn point_hit(points: &Points, p: Point, rect: Rectangle) -> Option<usize> {
    points
        .points()
        .enumerate()
        .map(|(i, uv)| (i, on_screen(rect, uv).distance(p)))
        .filter(|(_, d)| *d < 10.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// The points of a light, drawn as the Transform 2D's center is: white while held or under the
/// mouse, gray where a socket gives them.
fn draw_points(frame: &mut Frame, points: &Points, rect: Rectangle, active: Option<usize>) {
    for (i, uv) in points.points().enumerate() {
        let at = on_screen(rect, uv);
        let color = if points.locked(i) {
            style::gray(112)
        } else if active == Some(i) {
            Color::WHITE
        } else {
            style::ACCENT
        };
        frame.fill(&Path::circle(at, 6.0), Color::from_rgba(0.0, 0.0, 0.0, 0.6));
        frame.stroke(&Path::circle(at, 6.0), Stroke::default().with_width(2.0).with_color(color));
    }
}

/// Where a point of the view falls in the image, following the copies the view draws:
/// `uv` is in image widths from its top-left corner, and `tile` says on which axes the
/// image repeats. `None` when the point shows no copy of the image.
pub fn tile_uv(uv: Vector, tile: [bool; 2]) -> Option<Vector> {
    let wrap = |v: f32, tiled: bool| if tiled { v.rem_euclid(1.0) } else { v };
    let q = Vector::new(wrap(uv.x, tile[0]), wrap(uv.y, tile[1]));
    ((0.0..=1.0).contains(&q.x) && (0.0..=1.0).contains(&q.y)).then_some(q)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Without a transform the image simply repeats along its own sides, and only along the
    /// axes it tiles on.
    #[test]
    fn the_image_repeats_along_its_own_sides() {
        let uv = Vector::new(2.25, -1.75);
        assert_eq!(tile_uv(uv, [true, true]), Some(Vector::new(0.25, 0.25)));
        // Only horizontally: above and below the image there is nothing.
        assert_eq!(tile_uv(Vector::new(2.25, 0.5), [true, false]), Some(Vector::new(0.25, 0.5)));
        assert_eq!(tile_uv(uv, [true, false]), None);
        assert_eq!(tile_uv(uv, [false, false]), None);
        assert_eq!(tile_uv(Vector::new(0.5, 0.5), [false, false]), Some(Vector::new(0.5, 0.5)));
    }

    /// The points of the nodes of panoramas are dragged in the 2D view, as Substance Designer's
    /// HDRI tools are placed: as far as the mouse goes, round and round across, in one step of
    /// the history; Shift keeps a point to one axis; one that takes its value from a socket stays.
    #[test]
    fn the_points_of_the_lights_are_dragged_in_the_2d_view() {
        use crate::app::StartupOptions;
        let (mut app, _) = App::boot(StartupOptions::default());
        let (bounds, image) = (Rectangle::new(Point::ORIGIN, Size::new(800.0, 800.0)), [1024, 1024]);
        let p = &mut app.ui.preview;
        (p.zoom, p.pan, p.fit) = (0.75, Vector::ZERO, false);
        let rect = image_rect(bounds.size(), image, 0.75, Vector::ZERO);
        let near = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3;
        // A step of its own in the history.
        let edit = |app: &mut App, e: Edit| {
            let _ = app.update(Message::Edit(e));
            let _ = app.update(Message::Seal);
        };
        // Drags the point `i` of the node shown by `by` pixels (a pixel is 1/768 of the
        // panorama), Shift held or not.
        let drag = |app: &mut App, i: usize, by: Vector, shift: bool| {
            let points = Points::of(app, app.ui.preview.node.expect("a node shown")).expect("its points");
            let at = on_screen(rect, points.points().nth(i).expect("the point"));
            let mut state = State { bounds, ..State::default() };
            if shift {
                state.modifiers = keyboard::Modifiers::SHIFT;
            }
            let moves = |q: Point| iced::Event::Mouse(mouse::Event::CursorMoved { position: q });
            for (event, q) in [
                (moves(at), at),
                (iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)), at),
                (moves(at + by * 0.5), at + by * 0.5),
                (moves(at + by), at + by),
                (iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)), at + by),
            ] {
                let overlay = Overlay { app: &*app, image: Some(image) };
                let action = canvas::Program::update(&overlay, &mut state, &event, bounds, mouse::Cursor::Available(q));
                if let Some(m) = action.and_then(|a| a.into_inner().0) {
                    let _ = app.update(m);
                }
            }
        };
        let add = |app: &mut App, kind: NodeKind| {
            edit(app, Edit::AddNode { kind, pos: [0.0, 0.0] });
            let id = app.graph().nodes_of(kind).last().expect("the node").id;
            app.ui.preview.node = Some(id);
            id
        };
        let v2 = |app: &App, id: NodeId, key: &str| app.eval.params(id).expect("its parameters").v2(key);

        // A softbox's point: as far as the mouse goes, one step back.
        let softbox = add(&mut app, NodeKind::HDRI_SHAPE_LIGHT);
        drag(&mut app, 0, Vector::new(76.8, 38.4), false);
        assert!(near(v2(&app, softbox, "position"), [0.6, 0.3]), "{:?}", v2(&app, softbox, "position"));
        let _ = app.update(Message::Undo);
        assert!(near(v2(&app, softbox, "position"), [0.5, 0.25]), "a drag is one step back");
        // With Shift, along one axis.
        drag(&mut app, 0, Vector::new(76.8, 20.0), true);
        assert!(near(v2(&app, softbox, "position"), [0.6, 0.25]), "{:?}", v2(&app, softbox, "position"));
        // Taking its value from a socket, it stays.
        edit(&mut app, Edit::SetExposed { node: softbox, key: "position".into(), exposed: true });
        let held = v2(&app, softbox, "position");
        drag(&mut app, 0, Vector::new(50.0, 0.0), false);
        assert_eq!(v2(&app, softbox, "position"), held);

        // A tube's two points, each on its own.
        let tube = add(&mut app, NodeKind::HDRI_LINE_LIGHT);
        drag(&mut app, 1, Vector::new(0.0, 76.8), false);
        assert!(near(v2(&app, tube, "point_1"), [0.4, 0.3]) && near(v2(&app, tube, "point_2"), [0.6, 0.4]));
        drag(&mut app, 0, Vector::new(-76.8, 0.0), false);
        assert!(near(v2(&app, tube, "point_1"), [0.3, 0.3]) && near(v2(&app, tube, "point_2"), [0.6, 0.4]));

        // A gradient's point: round the edge of the panorama, and no lower than its bottom.
        let gradient = add(&mut app, NodeKind::HDRI_GRADIENT);
        drag(&mut app, 0, Vector::new(-460.8, 1000.0), false);
        assert!(near(v2(&app, gradient, "point"), [0.9, 1.0]), "{:?}", v2(&app, gradient, "point"));

        // A panel's point.
        let panel = add(&mut app, NodeKind::HDRI_PLANE_LIGHT);
        drag(&mut app, 0, Vector::new(38.4, -76.8), false);
        assert!(near(v2(&app, panel, "position"), [0.55, 0.3]), "{:?}", v2(&app, panel, "position"));
    }
}
