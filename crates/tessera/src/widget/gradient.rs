//! Gradient editor: the ramp with one handle per stop. Click the ramp to add a stop,
//! drag handles to move them, drag a handle down and away to remove it.

use std::f32::consts::PI;

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::widget::{Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell};
use iced::gradient::Linear;
use iced::{Background, Border, Color, Element, Event, Gradient as Fill, Length, Point, Radians, Rectangle, Size, mouse};
use tessera_core::{Gradient, GradientStop, Interpolation};

use crate::style;
use crate::widget::color::checker;

const BAR_H: f32 = 26.0;
const HANDLE_H: f32 = 18.0;
const HANDLE_W: f32 = 11.0;
/// How far below the editor a handle must be dragged to be removed.
const REMOVE_DISTANCE: f32 = 28.0;

pub struct GradientEditor<'a, Message> {
    gradient: Gradient,
    selected: Option<usize>,
    on_change: Box<dyn Fn(Gradient) -> Message + 'a>,
    on_select: Box<dyn Fn(usize) -> Message + 'a>,
    on_release: Option<Message>,
}

pub fn gradient_editor<'a, Message>(
    gradient: &Gradient,
    selected: Option<usize>,
    on_change: impl Fn(Gradient) -> Message + 'a,
    on_select: impl Fn(usize) -> Message + 'a,
) -> GradientEditor<'a, Message> {
    GradientEditor {
        gradient: gradient.normalized(),
        selected,
        on_change: Box::new(on_change),
        on_select: Box::new(on_select),
        on_release: None,
    }
}

impl<Message> GradientEditor<'_, Message> {
    pub fn on_release(mut self, message: Message) -> Self {
        self.on_release = Some(message);
        self
    }
}

struct Drag {
    index: usize,
    removing: bool,
    /// Where the handle was grabbed, from its center: the stop does not jump to the cursor.
    grab: f32,
}

#[derive(Default)]
struct State {
    drag: Option<Drag>,
}

fn bar(bounds: Rectangle) -> Rectangle {
    Rectangle { x: bounds.x + HANDLE_W / 2.0, width: bounds.width - HANDLE_W, height: BAR_H, ..bounds }
}

fn handle_rect(bounds: Rectangle, pos: f32) -> Rectangle {
    let b = bar(bounds);
    Rectangle { x: b.x + pos * b.width - HANDLE_W / 2.0, y: b.y + BAR_H + 2.0, width: HANDLE_W, height: HANDLE_H - 4.0 }
}

fn quad(renderer: &mut iced::Renderer, bounds: Rectangle, background: impl Into<Background>, border: Border) {
    renderer.fill_quad(renderer::Quad { bounds, border, ..Default::default() }, background);
}

fn linear(c0: [f32; 4], c1: [f32; 4]) -> Fill {
    Fill::Linear(Linear::new(Radians(PI / 2.0)).add_stop(0.0, style::color_of(c0)).add_stop(1.0, style::color_of(c1)))
}

impl<Message> GradientEditor<'_, Message> {
    fn position(&self, bounds: Rectangle, p: Point) -> f32 {
        let b = bar(bounds);
        ((p.x - b.x) / b.width).clamp(0.0, 1.0)
    }

    fn handle_at(&self, bounds: Rectangle, p: Point) -> Option<usize> {
        // Topmost (last drawn, selected on top) first.
        let mut order: Vec<usize> = (0..self.gradient.stops.len()).collect();
        if let Some(s) = self.selected {
            order.retain(|i| *i != s);
            order.push(s);
        }
        order.into_iter().rev().find(|&i| {
            let r = handle_rect(bounds, self.gradient.stops[i].pos);
            let hit = Rectangle { x: r.x - 3.0, y: r.y - 6.0, width: r.width + 6.0, height: r.height + 8.0 };
            hit.contains(p)
        })
    }

    /// Move stop `index` to `pos`; returns the new gradient and the stop's new index.
    fn moved(&self, index: usize, pos: f32) -> (Gradient, usize) {
        let mut g = self.gradient.clone();
        let mut stop = g.stops.remove(index);
        stop.pos = pos;
        let j = g.stops.partition_point(|s| s.pos <= pos);
        g.stops.insert(j, stop);
        (g, j)
    }
}

impl<Message: Clone> Widget<Message, iced::Theme, iced::Renderer> for GradientEditor<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(BAR_H + HANDLE_H))
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        layout::atomic(limits, Length::Fill, Length::Fixed(BAR_H + HANDLE_H))
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
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(p) = cursor.position_over(bounds) else { return };
                if let Some(i) = self.handle_at(bounds, p) {
                    shell.publish((self.on_select)(i));
                    let center = handle_rect(bounds, self.gradient.stops[i].pos).center_x();
                    state.drag = Some(Drag { index: i, removing: false, grab: center - p.x });
                } else if bar(bounds).contains(p) {
                    let mut g = self.gradient.clone();
                    let i = g.insert_at(self.position(bounds, p));
                    shell.publish((self.on_change)(g));
                    shell.publish((self.on_select)(i));
                    state.drag = Some(Drag { index: i, removing: false, grab: 0.0 });
                } else {
                    return;
                }
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let Some(drag) = &mut state.drag else { return };
                if drag.index >= self.gradient.stops.len() {
                    return;
                }
                let removing = self.gradient.stops.len() > 2 && position.y > bounds.y + bounds.height + REMOVE_DISTANCE;
                if removing != drag.removing {
                    drag.removing = removing;
                    shell.request_redraw();
                }
                if !removing {
                    let at = Point::new(position.x + drag.grab, position.y);
                    let (g, j) = self.moved(drag.index, self.position(bounds, at));
                    if g != self.gradient {
                        shell.publish((self.on_change)(g));
                    }
                    if j != drag.index {
                        shell.publish((self.on_select)(j));
                        drag.index = j;
                    }
                }
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let Some(drag) = state.drag.take() else { return };
                if drag.removing && drag.index < self.gradient.stops.len() {
                    let mut g = self.gradient.clone();
                    g.stops.remove(drag.index);
                    shell.publish((self.on_change)(g));
                    shell.publish((self.on_select)(drag.index.saturating_sub(1)));
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
        match &state.drag {
            Some(d) if d.removing => mouse::Interaction::NotAllowed,
            Some(_) => mouse::Interaction::Grabbing,
            None => match cursor.position_over(layout.bounds()) {
                Some(p) if self.handle_at(layout.bounds(), p).is_some() => mouse::Interaction::Grab,
                Some(p) if bar(layout.bounds()).contains(p) => mouse::Interaction::Crosshair,
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
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let b = bar(bounds);
        checker(renderer, b, 7.0);
        let g = &self.gradient;
        let x_at = |t: f32| b.x + t * b.width;
        let stops = &g.stops;
        match g.interpolation {
            Interpolation::Linear | Interpolation::Constant => {
                // Before the first stop, between stops, after the last stop.
                let first = stops.first().copied().unwrap_or(GradientStop { pos: 0.0, color: [0.0; 4] });
                let last = stops.last().copied().unwrap_or(first);
                if first.pos > 0.0 {
                    let r = Rectangle { width: x_at(first.pos) - b.x, ..b };
                    quad(renderer, r, style::color_of(first.color), Border::default());
                }
                for w in stops.windows(2) {
                    let (s0, s1) = (w[0], w[1]);
                    let (x0, x1) = (x_at(s0.pos), x_at(s1.pos));
                    if x1 - x0 < 0.1 {
                        continue;
                    }
                    let r = Rectangle { x: x0, width: x1 - x0 + 0.5, ..b };
                    if g.interpolation == Interpolation::Constant {
                        quad(renderer, r, style::color_of(s0.color), Border::default());
                    } else {
                        quad(renderer, r, linear(s0.color, s1.color), Border::default());
                    }
                }
                if last.pos < 1.0 {
                    let x = x_at(last.pos);
                    let r = Rectangle { x, width: b.x + b.width - x, ..b };
                    quad(renderer, r, style::color_of(last.color), Border::default());
                }
            }
            Interpolation::Smooth => {
                let n = 64;
                for i in 0..n {
                    let (t0, t1) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
                    let r = Rectangle { x: x_at(t0), width: (t1 - t0) * b.width + 0.5, ..b };
                    quad(renderer, r, linear(g.sample(t0), g.sample(t1)), Border::default());
                }
            }
        }
        quad(
            renderer,
            b,
            Color::TRANSPARENT,
            Border { color: style::BORDER, width: 1.0, radius: 2.0.into() },
        );

        // Handles, the selected one on top.
        let mut order: Vec<usize> = (0..stops.len()).collect();
        if let Some(s) = self.selected {
            order.retain(|i| *i != s);
            order.push(s);
        }
        for i in order {
            let stop = stops[i];
            let selected = self.selected == Some(i);
            let removing = state.drag.as_ref().is_some_and(|d| d.index == i && d.removing);
            let r = handle_rect(bounds, stop.pos);
            let tick = Rectangle { x: r.center_x() - 0.5, y: b.y + BAR_H - 6.0, width: 1.0, height: 8.0 };
            quad(renderer, tick, if selected { style::ACCENT } else { Color::WHITE }, Border::default());
            let mut fill = style::color_of(stop.color);
            fill.a = 1.0;
            if removing {
                fill = style::alpha(fill, 0.3);
            }
            let border = Border {
                color: if selected { style::ACCENT } else { style::rgb(20, 20, 22) },
                width: if selected { 2.0 } else { 1.0 },
                radius: 3.0.into(),
            };
            quad(renderer, r, fill, border);
        }
    }
}

impl<'a, Message: Clone + 'a> From<GradientEditor<'a, Message>> for Element<'a, Message> {
    fn from(e: GradientEditor<'a, Message>) -> Self {
        Element::new(e)
    }
}
