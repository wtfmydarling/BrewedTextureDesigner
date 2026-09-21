//! Numeric field: a slider bar that is scrubbed by dragging and edited as text when
//! clicked. Typed text may be an expression (`2*pi`, `1/3`, `sqrt(2)`), evaluated with
//! the same language as the Math node.

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::text::{self, Paragraph as _, Renderer as _};
use iced::advanced::widget::{Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell};
use iced::alignment::Vertical;
use iced::advanced::mouse;
use iced::{Background, Border, Element, Event, Length, Pixels, Point, Rectangle, Size, keyboard};

use crate::style;

pub struct NumberField<'a, Message> {
    value: f64,
    /// What the value would be if it had never been touched: a field showing anything else
    /// says so, in every panel.
    default: Option<f64>,
    min: f64,
    max: f64,
    limits: Option<(f64, f64)>,
    integer: bool,
    log: bool,
    prefix: String,
    suffix: String,
    decimals: usize,
    on_change: Option<Box<dyn Fn(f64) -> Message + 'a>>,
    on_release: Option<Message>,
    width: Length,
    height: f32,
    bar: bool,
}

/// A number field for `value`, whose slider covers `min ..= max` (typed values may go
/// beyond it unless [`NumberField::limits`] are set).
pub fn number<'a, Message>(value: f64, min: f64, max: f64) -> NumberField<'a, Message> {
    NumberField {
        value,
        default: None,
        min,
        max,
        limits: None,
        integer: false,
        log: false,
        prefix: String::new(),
        suffix: String::new(),
        decimals: 3,
        on_change: None,
        on_release: None,
        width: Length::Fill,
        height: 22.0,
        bar: true,
    }
}

impl<'a, Message> NumberField<'a, Message> {
    pub fn on_change(mut self, f: impl Fn(f64) -> Message + 'a) -> Self {
        self.on_change = Some(Box::new(f));
        self
    }
    pub fn on_release(mut self, message: Message) -> Self {
        self.on_release = Some(message);
        self
    }
    pub fn integer(mut self, integer: bool) -> Self {
        self.integer = integer;
        self
    }
    pub fn logarithmic(mut self, log: bool) -> Self {
        self.log = log && self.min > 0.0 && self.max > self.min;
        self
    }
    /// The value the field came with. It is then drawn heavier when it differs.
    pub fn default_value(mut self, default: f64) -> Self {
        self.default = Some(default);
        self
    }

    /// Is the value no longer the one the field came with?
    fn changed(&self) -> bool {
        self.default.is_some_and(|d| (self.value - d).abs() > 1e-9 * d.abs().max(1.0))
    }

    /// Values are clamped to this range, whatever is typed.
    pub fn limits(mut self, min: f64, max: f64) -> Self {
        self.limits = Some((min.min(max), max.max(min)));
        self
    }
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }
    pub fn suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = suffix.into();
        self
    }
    pub fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = decimals;
        self
    }
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }
    pub fn bar(mut self, bar: bool) -> Self {
        self.bar = bar;
        self
    }

    fn format(&self, v: f64) -> String {
        if self.integer {
            return format!("{}", v.round() as i64);
        }
        format_number(v, self.decimals)
    }

    fn fraction(&self, v: f64) -> f32 {
        if self.max <= self.min {
            return 0.0;
        }
        let t = if self.log {
            (v.max(1e-12).ln() - self.min.ln()) / (self.max.ln() - self.min.ln())
        } else {
            (v - self.min) / (self.max - self.min)
        };
        t.clamp(0.0, 1.0) as f32
    }

    fn finish(&self, v: f64) -> f64 {
        let mut v = if self.integer { v.round() } else { v };
        if let Some((lo, hi)) = self.limits {
            v = v.clamp(lo, hi);
        }
        if self.integer {
            v = v.clamp(i32::MIN as f64, i32::MAX as f64);
        }
        v
    }

    /// Value after dragging `dx` pixels on a field `width` pixels wide.
    fn scrub(&self, start: f64, dx: f32, width: f32, fine: bool, snap: bool) -> f64 {
        let width = width.max(20.0) as f64;
        let speed = if fine { 0.1 } else { 1.0 };
        let mut v = if self.log {
            let span = self.max.ln() - self.min.ln();
            (start.max(1e-9).ln() + dx as f64 / width * span * speed).exp()
        } else {
            let span = if self.max > self.min { self.max - self.min } else { 1.0 };
            start + dx as f64 / width * span * speed
        };
        if snap && !self.integer && self.max > self.min {
            let step = (self.max - self.min) / 20.0;
            v = (v / step).round() * step;
        }
        self.finish(v)
    }
}

/// Up to `decimals` decimals, trailing zeros removed, "-0" avoided.
pub fn format_number(v: f64, decimals: usize) -> String {
    if !v.is_finite() {
        return if v.is_nan() { "NaN".into() } else if v > 0.0 { "∞".into() } else { "-∞".into() };
    }
    let s = format!("{v:.decimals$}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    if s == "-0" { "0".into() } else { s }
}

/// Evaluate typed text: a number or a constant expression.
pub fn parse_number(text: &str) -> Option<f64> {
    let text = text.trim().replace(',', ".");
    if text.is_empty() {
        return None;
    }
    if let Ok(v) = text.parse::<f64>() {
        return v.is_finite().then_some(v);
    }
    let program = brewed_texture_core::expr::compile(&text).ok()?;
    if !program.vars.is_empty() {
        return None;
    }
    let v = program.eval(&[]);
    v.is_finite().then_some(v)
}

struct Drag {
    /// Where the drag started, in the window.
    origin: f32,
    /// Where the pointer was at the last move (only used when it is left where it lands).
    last: f32,
    /// How far it has travelled in total, in pixels: with the pointer put back after every
    /// move this keeps growing however wide the screen is.
    travelled: f32,
    start: f64,
    moved: bool,
    /// How the pointer is kept away from the edge of the screen, if it can be.
    grip: Option<crate::scrub::Grip>,
}

impl Drag {
    /// The value the drag has reached, once the field knows how wide it is.
    fn value(&self, field: &NumberField<'_, impl Sized>, width: f32, modifiers: keyboard::Modifiers) -> f64 {
        field.scrub(self.start, self.travelled, width, modifiers.shift(), modifiers.command())
    }
}

struct Editing {
    text: String,
    /// Byte index of the caret (on a char boundary).
    cursor: usize,
    select_all: bool,
    invalid: bool,
}

#[derive(Default)]
struct State {
    drag: Option<Drag>,
    edit: Option<Editing>,
    last_click: Option<mouse::Click>,
    modifiers: keyboard::Modifiers,
}

impl Editing {
    fn insert(&mut self, s: &str) {
        if self.select_all {
            self.text.clear();
            self.cursor = 0;
            self.select_all = false;
        }
        self.text.insert_str(self.cursor, s);
        self.cursor += s.len();
        self.invalid = false;
    }

    fn prev_boundary(&self) -> usize {
        self.text[..self.cursor].char_indices().next_back().map_or(0, |(i, _)| i)
    }

    fn next_boundary(&self) -> usize {
        self.text[self.cursor..].chars().next().map_or(self.cursor, |c| self.cursor + c.len_utf8())
    }
}

impl<Message: Clone> Widget<Message, iced::Theme, iced::Renderer> for NumberField<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, Length::Fixed(self.height))
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        layout::atomic(limits, self.width, Length::Fixed(self.height))
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        let Some(on_change) = &self.on_change else { return };

        if let Event::Keyboard(keyboard::Event::ModifiersChanged(m)) = event {
            state.modifiers = *m;
        }

        // ---------------------------------------------------------------- text editing
        if let Some(edit) = &mut state.edit {
            match event {
                Event::Mouse(mouse::Event::ButtonPressed(_)) if !cursor.is_over(bounds) => {
                    // Clicking elsewhere commits (or drops invalid text).
                    if let Some(v) = parse_number(&edit.text) {
                        let v = self.finish(v);
                        shell.publish(on_change(v));
                        if let Some(m) = &self.on_release {
                            shell.publish(m.clone());
                        }
                    }
                    state.edit = None;
                    shell.request_redraw();
                }
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                    edit.select_all = false;
                    edit.cursor = edit.text.len();
                    shell.capture_event();
                    shell.request_redraw();
                }
                Event::Keyboard(keyboard::Event::KeyPressed { key, physical_key, text, modifiers, .. }) => {
                    use keyboard::key::Named;
                    // `to_latin` keeps the shortcuts on the same keys on every layout.
                    let latin = key.to_latin(*physical_key).map(|c| c.to_ascii_lowercase());
                    match key {
                        keyboard::Key::Named(Named::Enter) => match parse_number(&edit.text) {
                            Some(v) => {
                                let v = self.finish(v);
                                shell.publish(on_change(v));
                                if let Some(m) = &self.on_release {
                                    shell.publish(m.clone());
                                }
                                state.edit = None;
                            }
                            None => edit.invalid = true,
                        },
                        keyboard::Key::Named(Named::Escape) => state.edit = None,
                        keyboard::Key::Named(Named::Backspace) => {
                            if edit.select_all {
                                edit.text.clear();
                                edit.cursor = 0;
                                edit.select_all = false;
                            } else if edit.cursor > 0 {
                                let prev = edit.prev_boundary();
                                edit.text.replace_range(prev..edit.cursor, "");
                                edit.cursor = prev;
                            }
                            edit.invalid = false;
                        }
                        keyboard::Key::Named(Named::Delete) => {
                            if edit.select_all {
                                edit.text.clear();
                                edit.cursor = 0;
                                edit.select_all = false;
                            } else if edit.cursor < edit.text.len() {
                                let next = edit.next_boundary();
                                edit.text.replace_range(edit.cursor..next, "");
                            }
                            edit.invalid = false;
                        }
                        keyboard::Key::Named(Named::ArrowLeft) => {
                            edit.cursor = if edit.select_all { 0 } else { edit.prev_boundary() };
                            edit.select_all = false;
                        }
                        keyboard::Key::Named(Named::ArrowRight) => {
                            edit.cursor = if edit.select_all { edit.text.len() } else { edit.next_boundary() };
                            edit.select_all = false;
                        }
                        keyboard::Key::Named(Named::Home) => {
                            edit.cursor = 0;
                            edit.select_all = false;
                        }
                        keyboard::Key::Named(Named::End) => {
                            edit.cursor = edit.text.len();
                            edit.select_all = false;
                        }
                        keyboard::Key::Character(_) if modifiers.command() => match latin {
                            Some('a') => edit.select_all = true,
                            Some('c') => clipboard.write(iced::advanced::clipboard::Kind::Standard, edit.text.clone()),
                            Some('v') => {
                                if let Some(t) = clipboard.read(iced::advanced::clipboard::Kind::Standard) {
                                    let clean: String = t.chars().filter(|c| !c.is_control()).collect();
                                    edit.insert(&clean);
                                }
                            }
                            _ => {}
                        },
                        _ => {
                            if let Some(t) = text {
                                let clean: String = t.chars().filter(|c| !c.is_control()).collect();
                                if !clean.is_empty() {
                                    edit.insert(&clean);
                                }
                            }
                        }
                    }
                    shell.capture_event();
                    shell.request_redraw();
                }
                _ => {}
            }
            return;
        }

        // ---------------------------------------------------------------- scrubbing
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if cursor.is_over(bounds) => {
                let Some(p) = cursor.position() else { return };
                let click = mouse::Click::new(p, mouse::Button::Left, state.last_click);
                state.last_click = Some(click);
                if matches!(click.kind(), mouse::click::Kind::Double | mouse::click::Kind::Triple) {
                    state.drag = None;
                    state.edit = Some(self.start_edit());
                } else {
                    // The pointer is held where the drag started, so the drag can go on past
                    // the edge of the screen.
                    let grip = crate::scrub::Grip::take();
                    state.drag = Some(Drag { origin: p.x, last: p.x, travelled: 0.0, start: self.value, moved: false, grip });
                }
                shell.capture_event();
                shell.request_redraw();
            }
            // While the pointer is locked it sends no moves of its own: what the mouse did
            // is read once a frame, for as long as the button is down.
            Event::Window(iced::window::Event::RedrawRequested(_)) => {
                let Some(drag) = &mut state.drag else { return };
                let _ = &drag;
                #[cfg(free_unix)]
                if let Some(crate::scrub::Grip::Locked(locked)) = &mut drag.grip {
                    drag.travelled += locked.travelled();
                    if drag.moved || drag.travelled.abs() > 3.0 {
                        drag.moved = true;
                        let v = drag.value(self, bounds.width, state.modifiers);
                        if v != self.value {
                            shell.publish(on_change(v));
                        }
                    }
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let Some(drag) = &mut state.drag {
                    match &mut drag.grip {
                        // The pointer is put back where the drag started after every move, so
                        // each move is a step away from there — including the move that puts
                        // it back, which is a step of nothing.
                        Some(crate::scrub::Grip::Anchored(anchor)) => {
                            let step = position.x - drag.origin;
                            if step.abs() < 0.5 {
                                shell.capture_event();
                                return;
                            }
                            drag.travelled += step;
                            if !anchor.recenter() {
                                drag.grip = None;
                                drag.last = position.x;
                            }
                        }
                        // Locked: the pointer does not move, and the frames do the counting.
                        #[cfg(free_unix)]
                        Some(crate::scrub::Grip::Locked(_)) => {
                            shell.capture_event();
                            shell.request_redraw();
                            return;
                        }
                        None => {
                            drag.travelled += position.x - drag.last;
                            drag.last = position.x;
                        }
                    }
                    if drag.moved || drag.travelled.abs() > 3.0 {
                        drag.moved = true;
                        let v = drag.value(self, bounds.width, state.modifiers);
                        if v != self.value {
                            shell.publish(on_change(v));
                        }
                    }
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(drag) = state.drag.take() {
                    if drag.moved {
                        if let Some(m) = &self.on_release {
                            shell.publish(m.clone());
                        }
                    } else {
                        // A click without dragging edits the value as text.
                        state.edit = Some(self.start_edit());
                    }
                    shell.capture_event();
                    shell.request_redraw();
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
        if self.on_change.is_none() {
            return mouse::Interaction::default();
        }
        if state.edit.is_some() {
            return if cursor.is_over(layout.bounds()) { mouse::Interaction::Text } else { mouse::Interaction::default() };
        }
        match &state.drag {
            // Held in place, the pointer points at nothing: it is out of the way until the
            // button comes up, right where the drag started.
            Some(d) if d.moved && d.grip.is_some() => mouse::Interaction::Hidden,
            Some(d) if d.moved => mouse::Interaction::ResizingHorizontally,
            _ if cursor.is_over(layout.bounds()) => mouse::Interaction::ResizingHorizontally,
            _ => mouse::Interaction::default(),
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
        let enabled = self.on_change.is_some();
        let hovered = enabled && cursor.is_over(bounds);
        let editing = state.edit.as_ref();
        let border_color = match editing {
            Some(e) if e.invalid => style::ERROR,
            Some(_) => style::ACCENT,
            None if hovered || state.drag.is_some() => style::BORDER_LIGHT,
            None => style::LINE,
        };
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: Border { color: border_color, width: 1.0, radius: 3.0.into() },
                ..renderer::Quad::default()
            },
            Background::Color(if enabled { style::FIELD } else { style::PANEL_DARK }),
        );
        let size = Pixels(style::SMALL + 0.5);
        let font = style::ui_font();
        let center = Point::new(bounds.center_x(), bounds.center_y());
        match editing {
            None => {
                if self.bar && enabled {
                    let f = self.fraction(self.value);
                    if f > 0.0 {
                        let fill = Rectangle { width: (bounds.width - 2.0) * f, height: bounds.height - 2.0, ..bounds };
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle { x: bounds.x + 1.0, y: bounds.y + 1.0, ..fill },
                                border: Border { radius: 2.0.into(), ..Border::default() },
                                ..renderer::Quad::default()
                            },
                            Background::Color(match (self.changed(), hovered || state.drag.is_some()) {
                                (true, true) => style::VALUE_FILL_CHANGED_HOT,
                                (true, false) => style::VALUE_FILL_CHANGED,
                                (false, true) => style::VALUE_FILL_HOT,
                                (false, false) => style::VALUE_FILL,
                            }),
                        );
                    }
                }
                let label = format!("{}{}{}", self.prefix, self.format(self.value), self.suffix);
                // A value that is no longer the one it came with is written in heavy type.
                let changed = self.changed();
                renderer.fill_text(
                    text::Text {
                        content: label,
                        bounds: bounds.size(),
                        size,
                        line_height: text::LineHeight::default(),
                        font: if changed { style::heavy() } else { font },
                        align_x: text::Alignment::Center,
                        align_y: Vertical::Center,
                        shaping: text::Shaping::Auto,
                        wrapping: text::Wrapping::None,
                    },
                    center,
                    match (enabled, changed) {
                        (false, _) => style::TEXT_DIM,
                        (true, true) => style::ACCENT_TEXT,
                        (true, false) => style::TEXT,
                    },
                    bounds,
                );
            }
            Some(edit) => {
                let pad = 6.0;
                let measure = |s: &str| {
                    <iced::Renderer as text::Renderer>::Paragraph::with_text(text::Text {
                        content: s,
                        bounds: Size::INFINITE,
                        size,
                        line_height: text::LineHeight::default(),
                        font,
                        align_x: text::Alignment::Left,
                        align_y: Vertical::Top,
                        shaping: text::Shaping::Auto,
                        wrapping: text::Wrapping::None,
                    })
                    .min_width()
                };
                let full = measure(&edit.text);
                let caret_x = measure(&edit.text[..edit.cursor]);
                // Keep the caret visible for long text.
                let visible = bounds.width - 2.0 * pad;
                let scroll = (caret_x - visible).max(0.0);
                let x0 = bounds.x + pad - scroll;
                if edit.select_all && !edit.text.is_empty() {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle {
                                x: x0,
                                y: bounds.y + 3.0,
                                width: full.min(visible),
                                height: bounds.height - 6.0,
                            },
                            ..renderer::Quad::default()
                        },
                        Background::Color(style::alpha(style::ACCENT, 0.45)),
                    );
                }
                renderer.with_layer(bounds, |renderer| {
                    renderer.fill_text(
                        text::Text {
                            content: edit.text.clone(),
                            bounds: Size::new(f32::INFINITY, bounds.height),
                            size,
                            line_height: text::LineHeight::default(),
                            font,
                            align_x: text::Alignment::Left,
                            align_y: Vertical::Center,
                            shaping: text::Shaping::Auto,
                            wrapping: text::Wrapping::None,
                        },
                        Point::new(x0, bounds.center_y()),
                        style::TEXT,
                        bounds,
                    );
                    if !edit.select_all {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle {
                                    x: x0 + caret_x,
                                    y: bounds.y + 4.0,
                                    width: 1.0,
                                    height: bounds.height - 8.0,
                                },
                                ..renderer::Quad::default()
                            },
                            Background::Color(style::ACCENT),
                        );
                    }
                });
            }
        }
    }
}

impl<Message> NumberField<'_, Message> {
    fn start_edit(&self) -> Editing {
        let text = if self.integer { format!("{}", self.value.round() as i64) } else { format_number(self.value, 6) };
        Editing { cursor: text.len(), text, select_all: true, invalid: false }
    }
}

impl<'a, Message: Clone + 'a> From<NumberField<'a, Message>> for Element<'a, Message> {
    fn from(field: NumberField<'a, Message>) -> Self {
        Element::new(field)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing_and_formatting() {
        assert_eq!(parse_number("0,5"), Some(0.5));
        assert_eq!(parse_number(" 2*pi "), Some(std::f64::consts::TAU));
        assert_eq!(parse_number("1/0"), None);
        assert_eq!(parse_number("x+1"), None);
        assert_eq!(parse_number(""), None);
        assert_eq!(format_number(0.25, 3), "0.25");
        assert_eq!(format_number(2.0, 3), "2");
        assert_eq!(format_number(-0.0001, 3), "0");
        assert_eq!(format_number(1234.5678, 2), "1234.57");
    }

    #[test]
    fn scrubbing() {
        let f: NumberField<'_, ()> = number(0.5, 0.0, 1.0);
        assert!((f.scrub(0.5, 100.0, 200.0, false, false) - 1.0).abs() < 1e-9);
        assert!((f.scrub(0.5, 100.0, 200.0, true, false) - 0.55).abs() < 1e-9);
        let f: NumberField<'_, ()> = number(4.0, 1.0, 16.0).integer(true).limits(1.0, 16.0);
        assert_eq!(f.scrub(4.0, -1000.0, 100.0, false, false), 1.0);
        // Half the width of a 0.1 .. 10 log slider is a factor of 10.
        let f: NumberField<'_, ()> = number(1.0, 0.1, 10.0).logarithmic(true);
        assert!((f.scrub(1.0, 50.0, 100.0, false, false) - 10.0).abs() < 1e-6);
    }
}
