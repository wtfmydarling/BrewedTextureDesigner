//! Colors picked by hand: the saturation / value square with the hue bar beside it (the Color
//! Editor), and the strip a color or a level of gray is picked from at a click (the color
//! fields of Properties).

use std::f32::consts::PI;

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::widget::{Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell};
use iced::gradient::Linear;
use iced::{Background, Border, Color, Element, Event, Gradient, Length, Point, Radians, Rectangle, Size, mouse};

use crate::style;

/// How wide the hue bar is, and how far from the square.
const HUE_W: f32 = 18.0;
const GAP: f32 = 8.0;
/// How much wider than its square a color picker is: its hue bar, and the gap before it.
pub const HUE_ROOM: f32 = GAP + HUE_W;
/// How high the strip of a color field is.
pub const STRIP_H: f32 = 20.0;

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

/// How light a color looks: what a grayscale image makes of it (Rec. 709, as the engine has
/// it).
pub fn luminance(c: [f32; 4]) -> f32 {
    (0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]).clamp(0.0, 1.0)
}

/// The gray of `c`'s luminance, of its alpha.
pub fn gray_of(c: [f32; 4]) -> [f32; 4] {
    let l = luminance(c);
    [l, l, l, c[3]]
}

#[derive(Clone, Copy, PartialEq)]
enum Part {
    SatValue,
    Hue,
}

#[derive(Default)]
struct State {
    drag: Option<Part>,
    /// What is being picked, as the events of a frame change it (the widget's own copy is as
    /// the frame began: several events arrive in one), and the widget's copy it was taken from.
    work: [f32; 3],
    synced: Option<[f32; 3]>,
}

pub struct ColorPicker<'a, Message> {
    /// The hue, saturation and value shown: where the point of the square and the mark of the
    /// hue bar are. (Not a color: black has no saturation to say and a gray no hue, and the
    /// point dragged to the bottom of the square jumped to its corner.)
    hsv: [f32; 3],
    /// How large the square is.
    side: f32,
    on_change: Box<dyn Fn([f32; 3]) -> Message + 'a>,
    on_release: Option<Message>,
}

/// The square of the hue of `hsv`, saturation across and value down, with the hue bar beside
/// it: what is picked is said as a hue, a saturation and a value too.
pub fn color_picker<'a, Message>(hsv: [f32; 3], on_change: impl Fn([f32; 3]) -> Message + 'a) -> ColorPicker<'a, Message> {
    ColorPicker { hsv: hsv.map(clamp01), side: 200.0, on_change: Box::new(on_change), on_release: None }
}

impl<Message> ColorPicker<'_, Message> {
    pub fn on_release(mut self, message: Message) -> Self {
        self.on_release = Some(message);
        self
    }

    /// The square `side` pixels across.
    pub fn side(mut self, side: f32) -> Self {
        self.side = side;
        self
    }
}

struct Areas {
    sv: Rectangle,
    hue: Rectangle,
}

impl<Message> ColorPicker<'_, Message> {
    fn areas(&self, bounds: Rectangle) -> Areas {
        let sv = Rectangle { width: self.side, height: self.side, ..bounds };
        let hue = Rectangle { x: sv.x + self.side + GAP, width: HUE_W, ..sv };
        Areas { sv, hue }
    }

    /// The hue at `y` on the bar: red at its top and bottom, through magenta, blue, cyan, green
    /// and yellow on the way down, as Substance Designer's.
    fn hue_at(a: &Areas, y: f32) -> f32 {
        (1.0 - clamp01((y - a.hue.y) / a.hue.height)).min(0.9999)
    }

    /// `[h, s, v]` with what `part` says at `p`: the square its saturation and value (past its
    /// edges, what the nearest edge says: black under it, at the saturation the point is at),
    /// the bar its hue.
    fn value_at([h, s, v]: [f32; 3], part: Part, p: Point, a: &Areas) -> [f32; 3] {
        match part {
            Part::SatValue => [h, clamp01((p.x - a.sv.x) / a.sv.width), 1.0 - clamp01((p.y - a.sv.y) / a.sv.height)],
            Part::Hue => [Self::hue_at(a, p.y), s, v],
        }
    }
}

fn clamp01(v: f32) -> f32 {
    if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }
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

/// The frame of a field of colors at `bounds` (a line of the border's color, its corners rounded
/// by `radius`), drawn under it; what is inside it, where its colors go: none goes past the frame
/// (drawn over them, a frame of rounded corners left their square corners out, and a line of it
/// lay over their edges).
fn framed(renderer: &mut iced::Renderer, bounds: Rectangle, radius: f32) -> Rectangle {
    quad(renderer, bounds, style::BORDER, radius);
    Rectangle { x: bounds.x + 1.0, y: bounds.y + 1.0, width: (bounds.width - 2.0).max(0.0), height: (bounds.height - 2.0).max(0.0) }
}

/// Checkerboard behind transparent colors.
pub fn checker(renderer: &mut iced::Renderer, bounds: Rectangle, cell: f32) {
    quad(renderer, bounds, style::gray(92), 0.0);
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
            quad(renderer, r, style::gray(152), 0.0);
        }
    }
}

/// The hues across `r` (`vertical`: down it, from red at its top through magenta), each sixth
/// a gradient of two, inside `r` (each but the last a little into the next, drawn over it, so
/// that no seam shows between them).
fn hues(renderer: &mut iced::Renderer, r: Rectangle, vertical: bool) {
    let stops = [0.0, 1.0 / 6.0, 2.0 / 6.0, 3.0 / 6.0, 4.0 / 6.0, 5.0 / 6.0, 1.0];
    for i in 0..6 {
        let (h0, h1): (f32, f32) = if vertical { (1.0 - stops[i], 1.0 - stops[i + 1]) } else { (stops[i], stops[i + 1]) };
        let c0 = hsv_to_rgb([h0, 1.0, 1.0]);
        let c1 = hsv_to_rgb([h1, 1.0, 1.0]);
        let angle = if vertical { Radians(PI) } else { Radians(PI / 2.0) };
        let g = Gradient::Linear(
            Linear::new(angle).add_stop(0.0, Color::from_rgb(c0[0], c0[1], c0[2])).add_stop(1.0, Color::from_rgb(c1[0], c1[1], c1[2])),
        );
        let over = if i < 5 { 0.5 } else { 0.0 };
        let part = if vertical {
            let seg = r.height / 6.0;
            Rectangle { y: r.y + i as f32 * seg, height: seg + over, ..r }
        } else {
            let seg = r.width / 6.0;
            Rectangle { x: r.x + i as f32 * seg, width: seg + over, ..r }
        };
        quad(renderer, part, g, 0.0);
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
        Size::new(Length::Fixed(self.side + GAP + HUE_W), Length::Fixed(self.side))
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        layout::atomic(limits, Length::Fixed(self.side + GAP + HUE_W), Length::Fixed(self.side))
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
        if state.synced != Some(self.hsv) {
            state.work = self.hsv;
            state.synced = Some(self.hsv);
        }
        let a = self.areas(layout.bounds());
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(p) = cursor.position() else { return };
                let part = if a.sv.contains(p) {
                    Part::SatValue
                } else if a.hue.contains(p) {
                    Part::Hue
                } else {
                    return;
                };
                state.drag = Some(part);
                state.work = Self::value_at(state.work, part, p, &a);
                shell.publish((self.on_change)(state.work));
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let Some(part) = state.drag {
                    state.work = Self::value_at(state.work, part, *position, &a);
                    shell.publish((self.on_change)(state.work));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.drag.take().is_some() => {
                if let Some(m) = &self.on_release {
                    shell.publish(m.clone());
                }
                shell.capture_event();
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
        _tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let a = self.areas(layout.bounds());
        let [h, s, v] = self.hsv;
        let pure = hsv_to_rgb([h, 1.0, 1.0]);
        let pure = Color::from_rgb(pure[0], pure[1], pure[2]);

        // Saturation (left to right) and value (top to bottom), in its frame.
        let inside = framed(renderer, a.sv, 3.0);
        let across = Gradient::Linear(Linear::new(Radians(PI / 2.0)).add_stop(0.0, Color::WHITE).add_stop(1.0, pure));
        quad(renderer, inside, across, 2.0);
        let down = Gradient::Linear(
            Linear::new(Radians(PI)).add_stop(0.0, Color::from_rgba(0.0, 0.0, 0.0, 0.0)).add_stop(1.0, Color::BLACK),
        );
        quad(renderer, inside, down, 2.0);
        let marker = Point::new(a.sv.x + s * a.sv.width, a.sv.y + (1.0 - v) * a.sv.height);
        let m = Rectangle { x: marker.x - 5.0, y: marker.y - 5.0, width: 10.0, height: 10.0 };
        outline(renderer, m, Color::BLACK, 1.0, 5.0);
        outline(renderer, Rectangle { x: m.x + 1.0, y: m.y + 1.0, width: 8.0, height: 8.0 }, Color::WHITE, 1.5, 4.0);

        // The hue bar beside it, in its frame; the hue marked across it, as the square's color
        // is: white, a dark line around it to be seen on any hue.
        let inside = framed(renderer, a.hue, 2.0);
        hues(renderer, inside, true);
        let hy = a.hue.y + (1.0 - h) * a.hue.height;
        let m = Rectangle { x: a.hue.x - 3.0, y: hy - 4.0, width: HUE_W + 6.0, height: 8.0 };
        outline(renderer, m, Color::BLACK, 1.0, 3.0);
        outline(renderer, Rectangle { x: m.x + 1.0, y: m.y + 1.0, width: m.width - 2.0, height: m.height - 2.0 }, Color::WHITE, 1.5, 2.0);
    }
}

impl<'a, Message: Clone + 'a> From<ColorPicker<'a, Message>> for Element<'a, Message> {
    fn from(p: ColorPicker<'a, Message>) -> Self {
        Element::new(p)
    }
}

/// The strip of a color field: a color picked at a click or a drag — the hues across it,
/// darker down it — or, for a level of gray, black to white across it, the level marked.
pub struct ColorStrip<'a, Message> {
    color: [f32; 4],
    gray: bool,
    on_change: Box<dyn Fn([f32; 4]) -> Message + 'a>,
    on_release: Option<Message>,
}

/// The strip of `color` (`gray`: of its level of gray): what is picked from it keeps the alpha.
pub fn color_strip<'a, Message>(color: [f32; 4], gray: bool, on_change: impl Fn([f32; 4]) -> Message + 'a) -> ColorStrip<'a, Message> {
    ColorStrip { color, gray, on_change: Box::new(on_change), on_release: None }
}

impl<Message> ColorStrip<'_, Message> {
    pub fn on_release(mut self, message: Message) -> Self {
        self.on_release = Some(message);
        self
    }

    fn color_at(&self, bounds: Rectangle, p: Point) -> [f32; 4] {
        let across = clamp01((p.x - bounds.x) / bounds.width.max(1.0));
        let down = clamp01((p.y - bounds.y) / bounds.height.max(1.0));
        let alpha = clamp01(self.color[3]);
        if self.gray {
            [across, across, across, alpha]
        } else {
            let [r, g, b] = hsv_to_rgb([across.min(0.9999), 1.0, 1.0 - down]);
            [r, g, b, alpha]
        }
    }
}

#[derive(Default)]
struct StripState {
    dragging: bool,
}

impl<Message: Clone> Widget<Message, iced::Theme, iced::Renderer> for ColorStrip<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<StripState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(StripState::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(STRIP_H))
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        let size = self.size();
        layout::atomic(limits, size.width, size.height)
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
        let state = tree.state.downcast_mut::<StripState>();
        let bounds = layout.bounds();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(p) = cursor.position_over(bounds) else { return };
                state.dragging = true;
                shell.publish((self.on_change)(self.color_at(bounds, p)));
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if state.dragging => {
                shell.publish((self.on_change)(self.color_at(bounds, *position)));
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
                state.dragging = false;
                if let Some(m) = &self.on_release {
                    shell.publish(m.clone());
                }
                shell.capture_event();
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
        let state = tree.state.downcast_ref::<StripState>();
        if state.dragging || cursor.is_over(layout.bounds()) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let r = layout.bounds();
        if self.gray {
            let inside = framed(renderer, r, 2.0);
            quad(renderer, inside, Gradient::Linear(Linear::new(Radians(PI / 2.0)).add_stop(0.0, Color::BLACK).add_stop(1.0, Color::WHITE)), 1.0);
            // The level, marked: white on black, black on white.
            let l = luminance(self.color);
            let mark = if l > 0.5 { Color::BLACK } else { Color::WHITE };
            let x = r.x + l * r.width;
            let m = Rectangle { x: x - 2.0, y: r.y - 2.0, width: 4.0, height: r.height + 4.0 };
            outline(renderer, m, mark, 1.5, 1.0);
        } else {
            let inside = framed(renderer, r, 2.0);
            hues(renderer, inside, false);
            let down = Gradient::Linear(Linear::new(Radians(PI)).add_stop(0.0, Color::from_rgba(0.0, 0.0, 0.0, 0.0)).add_stop(1.0, Color::BLACK));
            quad(renderer, inside, down, 0.0);
        }
    }
}

impl<'a, Message: Clone + 'a> From<ColorStrip<'a, Message>> for Element<'a, Message> {
    fn from(s: ColorStrip<'a, Message>) -> Self {
        Element::new(s)
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

/// The color `text` says in hexadecimal, as other programs copy it: "#RRGGBB", "RRGGBB",
/// "#RGB", "#RRGGBBAA", around spaces (or "0x" before it); `alpha` where it says none.
pub fn parse_hex(text: &str, alpha: f32) -> Option<[f32; 4]> {
    let t = text.trim();
    let t = t.strip_prefix('#').or_else(|| t.strip_prefix("0x")).or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    if !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&t[i..i + 2], 16).ok().map(|b| f32::from(b) / 255.0);
    let nibble = |i: usize| u8::from_str_radix(&t[i..=i], 16).ok().map(|n| f32::from(n * 17) / 255.0);
    match t.len() {
        3 => Some([nibble(0)?, nibble(1)?, nibble(2)?, alpha]),
        6 => Some([byte(0)?, byte(2)?, byte(4)?, alpha]),
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
    }

    /// Hexadecimal colors as other programs copy them are read; what is not one is not.
    #[test]
    fn hex_is_read_as_other_programs_write_it() {
        let gray = 128.0 / 255.0;
        for said in ["#808080", "808080", "  #808080 ", "0x808080"] {
            assert_eq!(parse_hex(said, 0.5), Some([gray, gray, gray, 0.5]), "{said:?}");
        }
        assert_eq!(parse_hex("#F80", 1.0), Some([1.0, 136.0 / 255.0, 0.0, 1.0]));
        assert_eq!(parse_hex("#FF000080", 1.0), Some([1.0, 0.0, 0.0, 128.0 / 255.0]));
        for odd in ["", "#", "#12345", "#GGGGGG", "red", "#1234567890", "ыы"] {
            assert_eq!(parse_hex(odd, 1.0), None, "{odd:?}");
        }
        assert_eq!(parse_hex(&to_hex([0.2, 0.4, 0.6, 1.0]), 1.0).map(to_hex), Some(to_hex([0.2, 0.4, 0.6, 1.0])));
    }
}
