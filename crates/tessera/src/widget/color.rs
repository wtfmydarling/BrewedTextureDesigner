//! HSV color picker: saturation / value square, hue strip and alpha strip.

use std::f32::consts::PI;

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::widget::{Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell};
use iced::gradient::Linear;
use iced::{Background, Border, Color, Element, Event, Gradient, Length, Point, Radians, Rectangle, Size, mouse};

use crate::style;

const SV_HEIGHT: f32 = 132.0;
const BAR: f32 = 14.0;
const GAP: f32 = 6.0;

pub fn rgb_to_hsv(c: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = c;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d < 1e-6 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    let s = if max > 1e-6 { d / max } else { 0.0 };
    [h, s, max]
}

pub fn hsv_to_rgb(hsv: [f32; 3]) -> [f32; 3] {
    let [h, s, v] = hsv;
    let h = h.rem_euclid(1.0) * 6.0;
    let c = v * s;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r + m, g + m, b + m]
}

#[derive(Clone, Copy, PartialEq)]
enum Part {
    SatValue,
    Hue,
    Alpha,
}

#[derive(Default)]
struct State {
    drag: Option<Part>,
    /// Hue kept while the color is gray (where hue is undefined).
    hue: f32,
    synced: Option<[f32; 4]>,
}

pub struct ColorPicker<'a, Message> {
    color: [f32; 4],
    on_change: Box<dyn Fn([f32; 4]) -> Message + 'a>,
    on_release: Option<Message>,
}

pub fn color_picker<'a, Message>(color: [f32; 4], on_change: impl Fn([f32; 4]) -> Message + 'a) -> ColorPicker<'a, Message> {
    ColorPicker { color, on_change: Box::new(on_change), on_release: None }
}

impl<Message> ColorPicker<'_, Message> {
    pub fn on_release(mut self, message: Message) -> Self {
        self.on_release = Some(message);
        self
    }
}

struct Areas {
    sv: Rectangle,
    hue: Rectangle,
    alpha: Rectangle,
}

fn areas(bounds: Rectangle) -> Areas {
    let sv = Rectangle { height: SV_HEIGHT, ..bounds };
    let hue = Rectangle { y: sv.y + SV_HEIGHT + GAP, height: BAR, ..bounds };
    let alpha = Rectangle { y: hue.y + BAR + GAP, height: BAR, ..bounds };
    Areas { sv, hue, alpha }
}

fn clamp01(v: f32) -> f32 {
    if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }
}

impl<Message> ColorPicker<'_, Message> {
    fn hsv(&self, state: &State) -> [f32; 3] {
        let [h, s, v] = rgb_to_hsv([clamp01(self.color[0]), clamp01(self.color[1]), clamp01(self.color[2])]);
        if s < 1e-4 || v < 1e-4 { [state.hue, s, v] } else { [h, s, v] }
    }

    fn value_at(&self, state: &State, part: Part, p: Point, a: &Areas) -> [f32; 4] {
        let [h, s, v] = self.hsv(state);
        let alpha = clamp01(self.color[3]);
        match part {
            Part::SatValue => {
                let s = clamp01((p.x - a.sv.x) / a.sv.width);
                let v = 1.0 - clamp01((p.y - a.sv.y) / a.sv.height);
                let [r, g, b] = hsv_to_rgb([h, s, v]);
                [r, g, b, alpha]
            }
            Part::Hue => {
                let h = clamp01((p.x - a.hue.x) / a.hue.width).min(0.9999);
                let [r, g, b] = hsv_to_rgb([h, s.max(1e-3), v.max(1e-3)]);
                // Keep the exact gray level when the color was gray.
                if s < 1e-4 || v < 1e-4 {
                    [self.color[0], self.color[1], self.color[2], alpha]
                } else {
                    [r, g, b, alpha]
                }
            }
            Part::Alpha => [self.color[0], self.color[1], self.color[2], clamp01((p.x - a.alpha.x) / a.alpha.width)],
        }
    }
}

fn quad(renderer: &mut iced::Renderer, bounds: Rectangle, background: impl Into<Background>, radius: f32) {
    renderer.fill_quad(
        renderer::Quad { bounds, border: Border { radius: radius.into(), ..Border::default() }, ..Default::default() },
        background,
    );
}

fn outline(renderer: &mut iced::Renderer, bounds: Rectangle, color: Color, width: f32, radius: f32) {
    renderer.fill_quad(
        renderer::Quad {
            bounds,
            border: Border { color, width, radius: radius.into() },
            ..Default::default()
        },
        Background::Color(Color::TRANSPARENT),
    );
}

/// Checkerboard behind transparent colors.
pub fn checker(renderer: &mut iced::Renderer, bounds: Rectangle, cell: f32) {
    quad(renderer, bounds, style::rgb(90, 90, 94), 0.0);
    let cols = (bounds.width / cell).ceil() as i32;
    let rows = (bounds.height / cell).ceil() as i32;
    for y in 0..rows {
        for x in 0..cols {
            if (x + y) % 2 == 0 {
                continue;
            }
            let r = Rectangle {
                x: bounds.x + x as f32 * cell,
                y: bounds.y + y as f32 * cell,
                width: cell.min(bounds.x + bounds.width - (bounds.x + x as f32 * cell)),
                height: cell.min(bounds.y + bounds.height - (bounds.y + y as f32 * cell)),
            };
            quad(renderer, r, style::rgb(150, 150, 154), 0.0);
        }
    }
}

impl<Message: Clone> Widget<Message, iced::Theme, iced::Renderer> for ColorPicker<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(SV_HEIGHT + 2.0 * (GAP + BAR)))
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        layout::atomic(limits, Length::Fill, Length::Fixed(SV_HEIGHT + 2.0 * (GAP + BAR)))
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
        if state.synced != Some(self.color) {
            let [h, s, v] = rgb_to_hsv([clamp01(self.color[0]), clamp01(self.color[1]), clamp01(self.color[2])]);
            if s > 1e-4 && v > 1e-4 && state.drag != Some(Part::Hue) {
                state.hue = h;
            }
            state.synced = Some(self.color);
        }
        let a = areas(layout.bounds());
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(p) = cursor.position() else { return };
                let part = if a.sv.contains(p) {
                    Part::SatValue
                } else if a.hue.contains(p) {
                    Part::Hue
                } else if a.alpha.contains(p) {
                    Part::Alpha
                } else {
                    return;
                };
                state.drag = Some(part);
                if part == Part::Hue {
                    state.hue = clamp01((p.x - a.hue.x) / a.hue.width).min(0.9999);
                }
                shell.publish((self.on_change)(self.value_at(state, part, p, &a)));
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let Some(part) = state.drag {
                    if part == Part::Hue {
                        state.hue = clamp01((position.x - a.hue.x) / a.hue.width).min(0.9999);
                    }
                    shell.publish((self.on_change)(self.value_at(state, part, *position, &a)));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.drag.take().is_some() {
                    if let Some(m) = &self.on_release {
                        shell.publish(m.clone());
                    }
                    shell.capture_event();
                }
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
        if state.drag.is_some() || cursor.is_over(layout.bounds()) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let a = areas(layout.bounds());
        let [h, s, v] = self.hsv(state);
        let pure = hsv_to_rgb([h, 1.0, 1.0]);
        let pure = Color::from_rgb(pure[0], pure[1], pure[2]);

        // Saturation (left to right) and value (top to bottom).
        let across = Gradient::Linear(Linear::new(Radians(PI / 2.0)).add_stop(0.0, Color::WHITE).add_stop(1.0, pure));
        quad(renderer, a.sv, across, 3.0);
        let down = Gradient::Linear(
            Linear::new(Radians(PI)).add_stop(0.0, Color::from_rgba(0.0, 0.0, 0.0, 0.0)).add_stop(1.0, Color::BLACK),
        );
        quad(renderer, a.sv, down, 3.0);
        outline(renderer, a.sv, style::BORDER, 1.0, 3.0);
        let marker = Point::new(a.sv.x + s * a.sv.width, a.sv.y + (1.0 - v) * a.sv.height);
        let m = Rectangle { x: marker.x - 5.0, y: marker.y - 5.0, width: 10.0, height: 10.0 };
        outline(renderer, m, Color::BLACK, 1.0, 5.0);
        outline(renderer, Rectangle { x: m.x + 1.0, y: m.y + 1.0, width: 8.0, height: 8.0 }, Color::WHITE, 1.5, 4.0);

        // Hue strip: six segments with two-stop gradients.
        let hues = [0.0, 1.0 / 6.0, 2.0 / 6.0, 3.0 / 6.0, 4.0 / 6.0, 5.0 / 6.0, 1.0];
        let seg = a.hue.width / 6.0;
        for i in 0..6 {
            let c0 = hsv_to_rgb([hues[i], 1.0, 1.0]);
            let c1 = hsv_to_rgb([hues[i + 1] - 1e-4, 1.0, 1.0]);
            let g = Gradient::Linear(
                Linear::new(Radians(PI / 2.0))
                    .add_stop(0.0, Color::from_rgb(c0[0], c0[1], c0[2]))
                    .add_stop(1.0, Color::from_rgb(c1[0], c1[1], c1[2])),
            );
            let r = Rectangle { x: a.hue.x + i as f32 * seg, width: seg + 0.5, ..a.hue };
            quad(renderer, r, g, 0.0);
        }
        outline(renderer, a.hue, style::BORDER, 1.0, 2.0);
        let hx = a.hue.x + h * a.hue.width;
        outline(renderer, Rectangle { x: hx - 3.0, y: a.hue.y - 2.0, width: 6.0, height: BAR + 4.0 }, Color::WHITE, 1.5, 2.0);

        // Alpha strip over a checkerboard.
        checker(renderer, a.alpha, 7.0);
        let c = style::color_of(self.color);
        let g = Gradient::Linear(
            Linear::new(Radians(PI / 2.0)).add_stop(0.0, Color { a: 0.0, ..c }).add_stop(1.0, Color { a: 1.0, ..c }),
        );
        quad(renderer, a.alpha, g, 0.0);
        outline(renderer, a.alpha, style::BORDER, 1.0, 2.0);
        let ax = a.alpha.x + clamp01(self.color[3]) * a.alpha.width;
        outline(renderer, Rectangle { x: ax - 3.0, y: a.alpha.y - 2.0, width: 6.0, height: BAR + 4.0 }, Color::WHITE, 1.5, 2.0);
    }
}

impl<'a, Message: Clone + 'a> From<ColorPicker<'a, Message>> for Element<'a, Message> {
    fn from(p: ColorPicker<'a, Message>) -> Self {
        Element::new(p)
    }
}

/// "#RRGGBB" or "#RRGGBBAA" of a color.
pub fn to_hex(c: [f32; 4]) -> String {
    let q = |v: f32| (clamp01(v) * 255.0).round() as u8;
    if q(c[3]) == 255 {
        format!("#{:02X}{:02X}{:02X}", q(c[0]), q(c[1]), q(c[2]))
    } else {
        format!("#{:02X}{:02X}{:02X}{:02X}", q(c[0]), q(c[1]), q(c[2]), q(c[3]))
    }
}

/// Parse "#RGB", "#RRGGBB" or "#RRGGBBAA" (the # is optional).
pub fn from_hex(s: &str) -> Option<[f32; 4]> {
    let s = s.trim().trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok().map(|v| v as f32 / 255.0);
    match s.len() {
        3 => {
            let d = |i: usize| u8::from_str_radix(s.get(i..i + 1)?, 16).ok().map(|v| (v * 17) as f32 / 255.0);
            Some([d(0)?, d(1)?, d(2)?, 1.0])
        }
        6 => Some([byte(0)?, byte(2)?, byte(4)?, 1.0]),
        8 => Some([byte(0)?, byte(2)?, byte(4)?, byte(6)?]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_roundtrip() {
        for c in [[1.0, 0.0, 0.0], [0.2, 0.5, 0.8], [0.3, 0.3, 0.3], [0.0, 0.0, 0.0], [1.0, 1.0, 0.0]] {
            let back = hsv_to_rgb(rgb_to_hsv(c));
            for i in 0..3 {
                assert!((back[i] - c[i]).abs() < 1e-5, "{c:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn hex() {
        assert_eq!(to_hex([1.0, 0.5, 0.0, 1.0]), "#FF8000");
        assert_eq!(from_hex("#ff8000"), Some([1.0, 128.0 / 255.0, 0.0, 1.0]));
        assert_eq!(from_hex("f00"), Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(from_hex("#00000080").map(|c| c[3]), Some(128.0 / 255.0));
        assert_eq!(from_hex("xyz"), None);
    }
}
