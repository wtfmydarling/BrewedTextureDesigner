//! The windows of an eyedropper that picks on a capture of the screen ([`crate::picker`]).
//! Each shows its monitor's capture pixel for pixel, so the screen seems unchanged: no
//! frame, no text. Picking one color, the cursor is a loupe; picking a gradient, the
//! path dragged with the mouse is drawn as a black line.

use iced::advanced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::widget::image;
use iced::{Color, ContentFit, Element, Length, Point, Rectangle, Size, keyboard, window};

use crate::app::{App, Message, PickMessage};
use crate::eyedropper::loupe;
use crate::picker::Shot;

pub fn view(app: &App, window: window::Id) -> Element<'_, Message> {
    let overlay = app.ui.screen_pick.as_ref().and_then(|pick| {
        let index = pick.shot_of(window)?;
        let shot = pick.shots.get(index)?;
        Some(Overlay { shot, index, gradient: pick.target.mode() == crate::eyedropper::Mode::Gradient, done: pick.done })
    });
    match overlay {
        // The capture below, the canvas above: a stack draws its layers in order (within
        // one layer, images would cover the canvas shapes).
        Some(overlay) => iced::widget::stack![
            image(overlay.shot.handle.clone()).width(Length::Fill).height(Length::Fill).content_fit(ContentFit::Fill),
            iced::widget::canvas(overlay).width(Length::Fill).height(Length::Fill),
        ]
        .into(),
        None => iced::widget::space().into(),
    }
}

struct Overlay<'a> {
    shot: &'a Shot,
    index: usize,
    gradient: bool,
    /// Picked or cancelled: only the capture shows until the window closes.
    done: bool,
}

#[derive(Default)]
pub struct State {
    cursor: Option<Point>,
    /// The path dragged so far (window coordinates).
    path: Vec<Point>,
    dragging: bool,
}

impl Overlay<'_> {
    /// Window position → pixel of the capture.
    fn pixel(&self, p: Point, bounds: Rectangle) -> [f32; 2] {
        let [w, h] = self.shot.pixel_size;
        [p.x / bounds.width.max(1.0) * w as f32, p.y / bounds.height.max(1.0) * h as f32]
    }

    fn publish(message: PickMessage) -> Option<canvas::Action<Message>> {
        Some(canvas::Action::publish(Message::Pick(message)).and_capture())
    }
}

impl canvas::Program<Message> for Overlay<'_> {
    type State = State;

    fn update(&self, state: &mut State, event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Message>> {
        use iced::Event;
        if self.done {
            return None;
        }
        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                state.cursor = cursor.position_in(bounds);
                if state.dragging
                    && let Some(p) = cursor.position_from(bounds.position())
                    && state.path.last() != Some(&p)
                {
                    state.path.push(p);
                }
                Some(canvas::Action::request_redraw())
            }
            Event::Mouse(mouse::Event::CursorLeft) => {
                state.cursor = None;
                Some(canvas::Action::request_redraw())
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = cursor.position_in(bounds)?;
                if self.gradient {
                    state.dragging = true;
                    state.path = vec![p];
                    Some(canvas::Action::request_redraw().and_capture())
                } else {
                    Some(canvas::Action::capture())
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if !self.gradient {
                    let p = cursor.position_in(bounds).or(state.cursor)?;
                    return Self::publish(PickMessage::Color { shot: self.index, at: self.pixel(p, bounds) });
                }
                if !std::mem::take(&mut state.dragging) {
                    return None;
                }
                let path = std::mem::take(&mut state.path);
                let points: Vec<[f32; 2]> = path.iter().map(|p| [p.x, p.y]).collect();
                let length: f32 = path.windows(2).map(|w| w[0].distance(w[1])).sum();
                if length < 2.0 {
                    // A click: a gradient needs a path.
                    return Some(canvas::Action::request_redraw().and_capture());
                }
                Self::publish(PickMessage::Path { shot: self.index, points })
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right)) => Self::publish(PickMessage::Cancel),
            Event::Mouse(mouse::Event::ButtonPressed(_) | mouse::Event::WheelScrolled { .. }) => Some(canvas::Action::capture()),
            Event::Keyboard(keyboard::Event::KeyPressed { key: keyboard::Key::Named(keyboard::key::Named::Escape), .. }) => Self::publish(PickMessage::Cancel),
            _ => None,
        }
    }

    fn mouse_interaction(&self, _state: &State, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if self.done || !cursor.is_over(bounds) {
            mouse::Interaction::default()
        } else if self.gradient {
            mouse::Interaction::Crosshair
        } else {
            // The loupe is drawn where the cursor is.
            mouse::Interaction::Hidden
        }
    }

    fn draw(&self, state: &State, renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        if self.done {
            return vec![frame.into_geometry()];
        }
        if self.gradient {
            draw_path(&mut frame, &state.path);
        } else if let Some(p) = state.cursor {
            self.draw_loupe(&mut frame, p, bounds);
        }
        vec![frame.into_geometry()]
    }
}

/// The dragged path: a black line with a light edge, so it shows on dark colors too.
fn draw_path(frame: &mut Frame, points: &[Point]) {
    let Some(first) = points.first() else { return };
    let path = Path::new(|b| {
        b.move_to(*first);
        for p in &points[1..] {
            b.line_to(*p);
        }
    });
    if points.len() == 1 {
        frame.fill(&Path::circle(*first, 2.0), Color::from_rgba(1.0, 1.0, 1.0, 0.55));
        frame.fill(&Path::circle(*first, 1.0), Color::BLACK);
        return;
    }
    let round = |width: f32, color: Color| Stroke {
        width,
        line_cap: canvas::LineCap::Round,
        line_join: canvas::LineJoin::Round,
        ..Stroke::default().with_color(color)
    };
    frame.stroke(&path, round(4.0, Color::from_rgba(1.0, 1.0, 1.0, 0.55)));
    frame.stroke(&path, round(2.0, Color::BLACK));
}

impl Overlay<'_> {
    /// The same loupe as the live eyedropper's, centered on the cursor.
    fn draw_loupe(&self, frame: &mut Frame, cursor: Point, bounds: Rectangle) {
        let [px, py] = self.pixel(cursor, bounds);
        let (cx, cy) = (px.floor() as i64, py.floor() as i64);
        let half = (loupe::PIXELS / 2) as i64;
        let [w, h] = self.shot.pixel_size;
        let pixels: Vec<Option<[u8; 3]>> = (-half..=half)
            .flat_map(|dy| (-half..=half).map(move |dx| (cx + dx, cy + dy)))
            .map(|(x, y)| (x >= 0 && y >= 0 && x < w as i64 && y < h as i64).then(|| self.shot.rgb(x, y)))
            .collect();
        let scale = self.shot.scale;
        let l = loupe::render(&pixels, scale);
        let size = l.size as f32 / scale;
        let hot = Point::new(l.hotspot.0 as f32 / scale, l.hotspot.1 as f32 / scale);
        let handle = image::Handle::from_rgba(l.size, l.size, l.straight_rgba());
        let at = Rectangle::new(Point::new(cursor.x - hot.x, cursor.y - hot.y), Size::new(size, size));
        frame.draw_image(at, canvas::Image::new(handle).filter_method(image::FilterMethod::Nearest));
    }
}
