//! The windows of the eyedropper. Each covers a monitor with what was on it when the
//! eyedropper was clicked, plus a magnifier under the cursor. A click picks a color; for
//! a gradient, a line is dragged across the colors and the result shows while dragging.

use iced::advanced::mouse;
use iced::alignment::Vertical;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::widget::text::{Alignment as TextAlign, LineHeight, Shaping};
use iced::{Color, ContentFit, Element, Length, Pixels, Point, Rectangle, Size, keyboard, window};
use tessera_core::Gradient;

use crate::app::{App, Message, PickMessage};
use crate::picker::{Shot, hex};
use crate::style;

/// Pixels across the magnifier (odd: one is in the middle).
const LOUPE_PIXELS: i64 = 15;
/// Size of a magnified pixel.
const LOUPE_ZOOM: f32 = 8.0;
/// Distance of the hint from the edge of the monitor.
const HINT_TOP: f32 = 16.0;
/// How close to the hint the cursor may come before the hint moves away.
const HINT_CLEARANCE: f32 = 110.0;

pub fn view(app: &App, window: window::Id) -> Element<'_, Message> {
    let overlay = app.ui.screen_pick.as_ref().and_then(|pick| {
        let index = pick.shot_of(window)?;
        let shot = pick.shots.get(index)?;
        Some(Overlay { shot, index, gradient: pick.target.is_gradient(), done: pick.done })
    });
    match overlay {
        // The capture below, the canvas above: a stack draws its layers in order (within
        // one layer, images would cover the canvas shapes).
        Some(overlay) => iced::widget::stack![
            iced::widget::image(overlay.shot.handle.clone()).width(Length::Fill).height(Length::Fill).content_fit(ContentFit::Fill),
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
    /// Where the gradient line starts.
    drag: Option<Point>,
}

impl Overlay<'_> {
    /// Window position → pixel of the capture.
    fn pixel(&self, p: Point, bounds: Rectangle) -> [f32; 2] {
        let [w, h] = self.shot.pixel_size;
        [p.x / bounds.width * w as f32, p.y / bounds.height * h as f32]
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
                Some(canvas::Action::request_redraw())
            }
            Event::Mouse(mouse::Event::CursorLeft) => {
                state.cursor = None;
                Some(canvas::Action::request_redraw())
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = cursor.position_in(bounds)?;
                if self.gradient {
                    state.drag = Some(p);
                    Some(canvas::Action::request_redraw().and_capture())
                } else {
                    Self::publish(PickMessage::Color { shot: self.index, at: self.pixel(p, bounds) })
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let from = state.drag.take()?;
                let to = cursor.position_in(bounds).or(state.cursor).unwrap_or(from);
                if from.distance(to) < 3.0 {
                    // A click: a gradient needs a line.
                    return Some(canvas::Action::request_redraw());
                }
                Self::publish(PickMessage::Line { shot: self.index, from: self.pixel(from, bounds), to: self.pixel(to, bounds) })
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => Self::publish(PickMessage::Cancel),
            Event::Keyboard(keyboard::Event::KeyPressed { key: keyboard::Key::Named(keyboard::key::Named::Escape), .. }) => {
                Self::publish(PickMessage::Cancel)
            }
            _ => None,
        }
    }

    fn mouse_interaction(&self, _state: &State, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if !self.done && cursor.is_over(bounds) { mouse::Interaction::Crosshair } else { mouse::Interaction::default() }
    }

    fn draw(&self, state: &State, renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        if self.done {
            return vec![frame.into_geometry()];
        }
        // A frame around every monitor: the screen is frozen while picking.
        frame.stroke_rectangle(
            Point::new(1.0, 1.0),
            Size::new(bounds.width - 2.0, bounds.height - 2.0),
            Stroke::default().with_width(2.0).with_color(style::ACCENT),
        );
        let mut preview = None;
        if let (Some(from), Some(to)) = (state.drag, state.cursor) {
            let outline = Stroke::default().with_width(4.0).with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.55));
            frame.stroke(&Path::line(from, to), outline);
            frame.stroke(&Path::line(from, to), Stroke::default().with_width(1.5).with_color(Color::WHITE));
            for p in [from, to] {
                frame.fill(&Path::circle(p, 4.5), Color::from_rgba(0.0, 0.0, 0.0, 0.6));
                frame.fill(&Path::circle(p, 3.0), Color::WHITE);
            }
            if from.distance(to) >= 3.0 {
                preview = Some(self.shot.gradient(self.pixel(from, bounds), self.pixel(to, bounds)));
            }
        }
        // The hint moves to the bottom when the cursor or the line would be under it.
        let near_top = [state.cursor, state.drag].into_iter().flatten().any(|p| p.y < HINT_TOP + HINT_CLEARANCE);
        hint(&mut frame, bounds, self.gradient, preview.as_ref(), near_top);
        if let Some(p) = state.cursor {
            loupe(&mut frame, self.shot, p, self.pixel(p, bounds), bounds);
        }
        vec![frame.into_geometry()]
    }
}

fn text(content: impl Into<String>, position: Point, color: Color, size: f32, align_x: TextAlign) -> Text {
    Text {
        content: content.into(),
        position,
        color,
        size: Pixels(size),
        font: style::ui_font(),
        align_x,
        align_y: Vertical::Center,
        line_height: LineHeight::default(),
        shaping: Shaping::Basic,
        max_width: f32::INFINITY,
    }
}

fn panel(frame: &mut Frame, rect: Rectangle) {
    frame.fill(&Path::rounded_rectangle(rect.position(), rect.size(), 4.0.into()), Color::from_rgba8(22, 23, 26, 0.94));
    frame.stroke(
        &Path::rounded_rectangle(rect.position(), rect.size(), 4.0.into()),
        Stroke::default().with_width(1.0).with_color(Color::from_rgb8(64, 68, 77)),
    );
}

/// What to do, at the top of every monitor; while dragging, the gradient being picked.
fn hint(frame: &mut Frame, bounds: Rectangle, gradient: bool, preview: Option<&Gradient>, at_bottom: bool) {
    let width = 440.0f32.min(bounds.width - 16.0);
    let height = 54.0;
    let y = if at_bottom { bounds.height - HINT_TOP - height } else { HINT_TOP };
    let rect = Rectangle::new(Point::new((bounds.width - width) / 2.0, y), Size::new(width, height));
    panel(frame, rect);
    let title = if gradient { "Drag a line across the colors to pick a gradient" } else { "Click to pick a color" };
    let center = rect.center_x();
    frame.fill_text(text(title, Point::new(center, rect.y + 17.0), style::TEXT, 13.0, TextAlign::Center));
    match preview {
        Some(g) => {
            let strip = Rectangle::new(Point::new(rect.x + 14.0, rect.y + 32.0), Size::new(width - 88.0, 12.0));
            let steps = strip.width.max(1.0) as usize;
            for i in 0..steps {
                let c = g.sample(i as f32 / (steps - 1).max(1) as f32);
                frame.fill_rectangle(Point::new(strip.x + i as f32, strip.y), Size::new(1.0, strip.height), style::color_of(c));
            }
            frame.stroke_rectangle(strip.position(), strip.size(), Stroke::default().with_width(1.0).with_color(Color::from_rgb8(64, 68, 77)));
            let stops = format!("{} stops", g.stops.len());
            frame.fill_text(text(stops, Point::new(rect.x + width - 14.0, strip.center_y()), style::TEXT_DIM, 12.0, TextAlign::Right));
        }
        None => {
            let note = "Esc or right-click cancels";
            frame.fill_text(text(note, Point::new(center, rect.y + 38.0), style::TEXT_FAINT, 11.0, TextAlign::Center));
        }
    }
}

/// The magnified pixels under the cursor and the color there.
fn loupe(frame: &mut Frame, shot: &Shot, cursor: Point, pixel: [f32; 2], bounds: Rectangle) {
    let side = LOUPE_PIXELS as f32 * LOUPE_ZOOM;
    let label = 26.0;
    let offset = 22.0;
    let mut x = cursor.x + offset;
    let mut y = cursor.y + offset;
    if x + side > bounds.width - 4.0 {
        x = cursor.x - offset - side;
    }
    if y + side + label > bounds.height - 4.0 {
        y = cursor.y - offset - side - label;
    }
    // Always entirely on the monitor.
    x = x.clamp(4.0, (bounds.width - side - 4.0).max(4.0));
    y = y.clamp(4.0, (bounds.height - side - label - 4.0).max(4.0));
    panel(frame, Rectangle::new(Point::new(x - 3.0, y - 3.0), Size::new(side + 6.0, side + label + 6.0)));
    let (cx, cy) = (pixel[0].floor() as i64, pixel[1].floor() as i64);
    let half = LOUPE_PIXELS / 2;
    for dy in -half..=half {
        for dx in -half..=half {
            let c = shot.pixel(cx + dx, cy + dy);
            let at = Point::new(x + (dx + half) as f32 * LOUPE_ZOOM, y + (dy + half) as f32 * LOUPE_ZOOM);
            frame.fill_rectangle(at, Size::new(LOUPE_ZOOM, LOUPE_ZOOM), style::color_of(c));
        }
    }
    // The picked pixel, outlined in black and white so it shows on any color.
    let center = Point::new(x + half as f32 * LOUPE_ZOOM, y + half as f32 * LOUPE_ZOOM);
    let size = Size::new(LOUPE_ZOOM, LOUPE_ZOOM);
    frame.stroke_rectangle(center, size, Stroke::default().with_width(3.0).with_color(Color::BLACK));
    frame.stroke_rectangle(center, size, Stroke::default().with_width(1.0).with_color(Color::WHITE));

    let color = shot.pixel(cx, cy);
    let row = y + side + label / 2.0 + 1.0;
    frame.fill_rectangle(Point::new(x, row - 7.0), Size::new(14.0, 14.0), style::color_of(color));
    frame.stroke_rectangle(Point::new(x, row - 7.0), Size::new(14.0, 14.0), Stroke::default().with_width(1.0).with_color(Color::from_rgb8(90, 95, 105)));
    let mut code = text(hex(color), Point::new(x + 22.0, row), style::TEXT, 12.0, TextAlign::Left);
    code.font = style::mono();
    frame.fill_text(code);
    let [w, h] = shot.pixel_size;
    let at = format!("{}, {}", cx.clamp(0, w as i64 - 1), cy.clamp(0, h as i64 - 1));
    frame.fill_text(text(at, Point::new(x + side, row), style::TEXT_FAINT, 11.0, TextAlign::Right));
}
