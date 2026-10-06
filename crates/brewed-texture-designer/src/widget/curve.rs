//! Curve editor: the curve over a grid, with a handle at each of its points. Drag a handle to
//! move its point (across only as far as its neighbours leave room), click anywhere else in
//! the square to add a point there and drag it, drag a handle out of the square to remove its
//! point (a curve keeps two).

use iced::advanced::graphics::geometry::Renderer as _;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::widget::{Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell};
use iced::widget::canvas::{Frame, Path, Stroke};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector, mouse};
use brewed_texture_core::Curve;

use crate::style;

/// Height of the square the curve is drawn in.
const SIDE: f32 = 180.0;
/// Room around the square, for the handles at its edges.
const PAD: f32 = 6.0;
/// How near a handle the pointer grabs it.
const GRAB: f32 = 8.0;
/// How far out of the square a handle is dragged to remove its point.
const REMOVE_DISTANCE: f32 = 24.0;
/// Segments of the curve as it is drawn.
const SEGMENTS: usize = 96;

pub struct CurveEditor<'a, Message> {
    curve: Curve,
    on_change: Box<dyn Fn(Curve) -> Message + 'a>,
    on_release: Option<Message>,
}

pub fn curve_editor<'a, Message>(curve: &Curve, on_change: impl Fn(Curve) -> Message + 'a) -> CurveEditor<'a, Message> {
    CurveEditor { curve: curve.normalized(), on_change: Box::new(on_change), on_release: None }
}

impl<Message> CurveEditor<'_, Message> {
    /// Sent when a drag ends (the changes it made are one step to undo).
    pub fn on_release(mut self, message: Message) -> Self {
        self.on_release = Some(message);
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Grab {
    index: usize,
    /// Dragged out of the square: its point goes when the button is let go.
    removing: bool,
    /// The curve as the drag made it last, which it moves on: the editor's own is the one it
    /// was made with until the window is drawn again (a point added and dragged before that
    /// was lost, and its neighbour moved).
    curve: Curve,
}

#[derive(Default)]
struct State {
    grab: Option<Grab>,
}

/// The square the curve is drawn in, within `bounds`.
fn square(bounds: Rectangle) -> Rectangle {
    Rectangle { x: bounds.x + PAD, y: bounds.y + PAD, width: (bounds.width - 2.0 * PAD).max(1.0), height: SIDE }
}

/// Where a point of the curve is on the screen.
fn to_screen(sq: Rectangle, p: [f32; 2]) -> Point {
    Point::new(sq.x + p[0] * sq.width, sq.y + (1.0 - p[1]) * sq.height)
}

/// Which point of the curve's square is under `p` (clamped to it).
fn to_curve(sq: Rectangle, p: Point) -> [f32; 2] {
    [((p.x - sq.x) / sq.width).clamp(0.0, 1.0), (1.0 - (p.y - sq.y) / sq.height).clamp(0.0, 1.0)]
}

/// How far `p` is out of `r` (0 inside).
fn outside(r: Rectangle, p: Point) -> f32 {
    let dx = (r.x - p.x).max(p.x - (r.x + r.width)).max(0.0);
    let dy = (r.y - p.y).max(p.y - (r.y + r.height)).max(0.0);
    dx.max(dy)
}

impl<Message> CurveEditor<'_, Message> {
    /// The handle under `p`, the nearest of those in reach.
    fn handle_at(&self, sq: Rectangle, p: Point) -> Option<usize> {
        let distance = |i: usize| to_screen(sq, self.curve.points[i]).distance(p);
        (0..self.curve.points.len()).filter(|&i| distance(i) <= GRAB).min_by(|&a, &b| distance(a).total_cmp(&distance(b)))
    }
}

impl<Message: Clone> Widget<Message, iced::Theme, iced::Renderer> for CurveEditor<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(SIDE + 2.0 * PAD))
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        layout::atomic(limits, Length::Fill, Length::Fixed(SIDE + 2.0 * PAD))
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        let sq = square(bounds);
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(p) = cursor.position_over(bounds) else { return };
                if let Some(index) = self.handle_at(sq, p) {
                    state.grab = Some(Grab { index, removing: false, curve: self.curve.clone() });
                } else if outside(sq, p) == 0.0 {
                    let [x, y] = to_curve(sq, p);
                    let mut curve = self.curve.clone();
                    let Some(index) = curve.insert(x, y) else { return };
                    shell.publish((self.on_change)(curve.clone()));
                    state.grab = Some(Grab { index, removing: false, curve });
                } else {
                    return;
                }
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let Some(grab) = &mut state.grab else { return };
                if grab.index >= grab.curve.points.len() {
                    return;
                }
                let removing = grab.curve.points.len() > 2 && outside(sq, *position) > REMOVE_DISTANCE;
                if removing != grab.removing {
                    grab.removing = removing;
                    shell.request_redraw();
                }
                if !removing {
                    let [x, y] = to_curve(sq, *position);
                    let moved = grab.curve.moved(grab.index, x, y);
                    if moved != grab.curve {
                        shell.publish((self.on_change)(moved.clone()));
                        grab.curve = moved;
                    }
                }
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let Some(grab) = state.grab.take() else { return };
                if grab.removing && grab.index < grab.curve.points.len() && grab.curve.points.len() > 2 {
                    let mut curve = grab.curve;
                    curve.points.remove(grab.index);
                    shell.publish((self.on_change)(curve));
                }
                if let Some(m) = &self.on_release {
                    shell.publish(m.clone());
                }
                shell.capture_event();
                shell.request_redraw();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        let sq = square(layout.bounds());
        match &state.grab {
            Some(g) if g.removing => mouse::Interaction::NotAllowed,
            Some(_) => mouse::Interaction::Grabbing,
            None => match cursor.position_over(layout.bounds()) {
                Some(p) if self.handle_at(sq, p).is_some() => mouse::Interaction::Grab,
                Some(p) if outside(sq, p) == 0.0 => mouse::Interaction::Crosshair,
                _ => mouse::Interaction::default(),
            },
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        // Drawn from the widget's corner.
        let sq = square(Rectangle { x: 0.0, y: 0.0, ..bounds });
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill_rectangle(sq.position(), sq.size(), style::FIELD);
        // Quarters, and the straight line the curve starts as.
        let thin = |color| Stroke::default().with_width(1.0).with_color(color);
        for k in 1..4 {
            let t = k as f32 / 4.0;
            frame.stroke(&Path::line(to_screen(sq, [t, 0.0]), to_screen(sq, [t, 1.0])), thin(style::LINE));
            frame.stroke(&Path::line(to_screen(sq, [0.0, t]), to_screen(sq, [1.0, t])), thin(style::LINE));
        }
        frame.stroke(&Path::line(to_screen(sq, [0.0, 0.0]), to_screen(sq, [1.0, 1.0])), thin(style::gray(52)));
        frame.stroke(&Path::rectangle(sq.position(), sq.size()), thin(style::BORDER_LIGHT));
        let values = self.curve.bake(SEGMENTS + 1);
        let curve = Path::new(|b| {
            for (i, v) in values.iter().enumerate() {
                let p = to_screen(sq, [i as f32 / SEGMENTS as f32, *v]);
                if i == 0 { b.move_to(p) } else { b.line_to(p) }
            }
        });
        frame.stroke(&curve, Stroke::default().with_width(1.8).with_color(style::TEXT));
        // The handles: the one held (or under the pointer) lit.
        let local = cursor.position_in(bounds);
        let hot = state.grab.as_ref().map(|g| g.index).or_else(|| local.and_then(|p| self.handle_at(sq, p)));
        for (i, p) in self.curve.points.iter().enumerate() {
            let at = to_screen(sq, *p);
            let removing = state.grab.as_ref().is_some_and(|g| g.index == i && g.removing);
            let color = if removing {
                style::alpha(style::TEXT, 0.3)
            } else if hot == Some(i) {
                style::ACCENT
            } else {
                style::TEXT_DIM
            };
            let size = if hot == Some(i) { 9.0 } else { 7.0 };
            frame.fill_rectangle(at - Vector::new(size / 2.0, size / 2.0), Size::new(size, size), style::BORDER);
            frame.fill_rectangle(at - Vector::new(size / 2.0 - 1.5, size / 2.0 - 1.5), Size::new(size - 3.0, size - 3.0), color);
        }
        renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| renderer.draw_geometry(frame.into_geometry()));
    }
}

impl<'a, Message: Clone + 'a> From<CurveEditor<'a, Message>> for Element<'a, Message> {
    fn from(e: CurveEditor<'a, Message>) -> Self {
        Element::new(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_are_found_where_they_are_drawn() {
        let sq = square(Rectangle { x: 10.0, y: 20.0, width: 212.0, height: SIDE + 2.0 * PAD });
        assert_eq!((sq.width, sq.height), (200.0, SIDE));
        assert_eq!(to_screen(sq, [0.0, 0.0]), Point::new(16.0, 26.0 + SIDE));
        assert_eq!(to_screen(sq, [1.0, 1.0]), Point::new(216.0, 26.0));
        for p in [[0.25, 0.75], [0.5, 0.1], [1.0, 0.0]] {
            let back = to_curve(sq, to_screen(sq, p));
            assert!((back[0] - p[0]).abs() < 1e-5 && (back[1] - p[1]).abs() < 1e-5, "{p:?} {back:?}");
        }
        assert_eq!(outside(sq, Point::new(100.0, 100.0)), 0.0);
        assert_eq!(outside(sq, Point::new(100.0, 26.0 + SIDE + 30.0)), 30.0);
        let editor = curve_editor(&Curve::default(), |_| ());
        assert_eq!(editor.handle_at(sq, Point::new(219.0, 28.0)), Some(1));
        assert_eq!(editor.handle_at(sq, Point::new(116.0, 100.0)), None);
    }
}
