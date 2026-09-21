//! Levels: histogram of the input with draggable black point, gamma and white point.

use std::sync::Arc;

use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::widget::{button, column, row, text};
use iced::{Alignment, Color, Element, Length, Point, Rectangle, Size, mouse};
use tessera_core::graph::{Node, NodeId};
use tessera_core::{Edit, ParamValue};
use tessera_engine::simd::ImageStats;

use crate::app::{App, Message};
use crate::style;
use crate::view::section;

pub fn view<'a>(app: &'a App, node: &'a Node) -> Element<'a, Message> {
    let mut col = column![section("Input histogram")].spacing(6);
    let Some(source) = app.graph().link_into(node.id, "i:input").map(|l| l.from) else {
        return col.push(text("Connect an input to see its histogram.").size(style::SMALL).color(style::TEXT_DIM)).into();
    };
    let Some((version, stats)) = app.res.stats.get(&source).cloned() else {
        return col.push(text("Computing the histogram…").size(style::SMALL).color(style::TEXT_DIM)).into();
    };
    let fresh = app.res.reports.get(&source).is_some_and(|r| r.version == version);
    let params = app.eval.params(node.id);
    let get = |k: &str| params.map_or(0.0, |p| p.f(k));
    let locked = ["in_low", "gamma", "in_high"].map(|k| node.is_exposed(k));
    col = col.push(
        iced::widget::canvas(Histogram {
            node: node.id,
            stats: stats.clone(),
            lo: get("in_low"),
            hi: get("in_high"),
            gamma: get("gamma").max(1e-3),
            locked,
            fresh,
        })
        .width(Length::Fill)
        .height(Length::Fixed(110.0)),
    );
    col = col.push(
        row![
            button(text("Auto levels").size(style::SMALL))
                .padding([3, 8])
                .style(style::button)
                .on_press_maybe((!locked[0] && !locked[2]).then_some(Message::AutoLevels(node.id))),
            text(format!("{}×{}{}", stats.width, stats.height, if fresh { "" } else { " · updating…" }))
                .size(style::SMALL)
                .color(style::TEXT_FAINT),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    );
    let mut table = column![
        row![
            text("").width(Length::Fixed(24.0)),
            text("min").size(style::SMALL).color(style::TEXT_DIM).width(Length::Fill),
            text("max").size(style::SMALL).color(style::TEXT_DIM).width(Length::Fill),
            text("mean").size(style::SMALL).color(style::TEXT_DIM).width(Length::Fill),
        ]
    ]
    .spacing(2);
    let colors = [style::rgb(240, 110, 110), style::rgb(110, 220, 130), style::rgb(110, 160, 250), style::TEXT];
    for (c, name) in ["R", "G", "B", "A"].iter().enumerate() {
        let cell = |v: f32| text(format!("{v:.3}")).size(style::SMALL).font(style::mono()).width(Length::Fill);
        table = table.push(row![
            text(*name).size(style::SMALL).color(colors[c]).width(Length::Fixed(24.0)),
            cell(stats.min[c]),
            cell(stats.max[c]),
            cell(stats.mean[c]),
        ]);
    }
    col.push(table).into()
}

struct Histogram {
    node: NodeId,
    stats: Arc<ImageStats>,
    lo: f32,
    hi: f32,
    gamma: f32,
    /// Exposed (locked): black point, gamma, white point.
    locked: [bool; 3],
    fresh: bool,
}

#[derive(Default)]
struct State {
    /// The handle being dragged, and where it was grabbed from its center.
    drag: Option<(usize, f32)>,
}

impl Histogram {
    fn plot(bounds: Rectangle) -> Rectangle {
        Rectangle::new(Point::new(6.0, 4.0), Size::new(bounds.width - 12.0, bounds.height - 16.0))
    }

    /// x positions of the black point, gamma (mid gray) and white point.
    fn handles(&self, plot: Rectangle) -> [f32; 3] {
        let mid = self.lo + (self.hi - self.lo) * 0.5f32.powf(self.gamma);
        [self.lo, mid, self.hi].map(|v| plot.x + v.clamp(0.0, 1.0) * plot.width)
    }

    /// What the gamma parameter is held to: the handle reaches wherever gamma can take it,
    /// which is from one end of the histogram to the other.
    fn gamma_limits() -> (f32, f32) {
        tessera_core::NodeKind::LEVELS.def().param("gamma").and_then(|p| p.limits).unwrap_or((0.01, 100.0))
    }

    fn edit(&self, handle: usize, v: f32) -> Option<Message> {
        let set = |k: &str, v: f32| Edit::SetParam { node: self.node, key: k.into(), value: ParamValue::Float(v) };
        if self.locked[handle] {
            return None;
        }
        Some(Message::Edit(match handle {
            0 => set("in_low", v.min(self.hi - 0.01)),
            2 => set("in_high", v.max(self.lo + 0.01)),
            _ => {
                // Where the handle is, as the share of the way from black to white that
                // mid gray lands on; the gamma that puts it there is 0.5 to that power.
                let span = self.hi - self.lo;
                let t = if span.abs() > 1e-6 { (v - self.lo) / span } else { 0.5 };
                // Past the black point the handle stays at black, past the white point at
                // white: the logarithm of nothing or less is no gamma at all.
                let t = t.clamp(f32::MIN_POSITIVE, 1.0);
                let (lo, hi) = Self::gamma_limits();
                set("gamma", (t.ln() / 0.5f32.ln()).clamp(lo, hi))
            }
        }))
    }
}

impl canvas::Program<Message> for Histogram {
    type State = State;

    fn update(&self, state: &mut State, event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Message>> {
        let plot = Self::plot(bounds);
        match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = cursor.position_in(bounds)?;
                let xs = self.handles(plot);
                // The nearest handle that can move (exposed levels come from their sockets).
                let nearest = (0..3).filter(|i| !self.locked[*i]).min_by(|a, b| (xs[*a] - p.x).abs().total_cmp(&(xs[*b] - p.x).abs()))?;
                state.drag = Some((nearest, xs[nearest] - p.x));
                Some(canvas::Action::capture())
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let (h, grab) = state.drag?;
                let x = position.x - bounds.x + grab;
                let v = ((x - plot.x) / plot.width).clamp(0.0, 1.0);
                self.edit(h, v).map(|m| canvas::Action::publish(m).and_capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.drag.take()?;
                Some(canvas::Action::publish(Message::Seal))
            }
            _ => None,
        }
    }

    fn mouse_interaction(&self, state: &State, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if state.drag.is_some() || cursor.is_over(bounds) {
            mouse::Interaction::ResizingHorizontally
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(&self, _state: &State, renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), style::FIELD);
        let plot = Self::plot(bounds);
        let max = (0..5).filter(|c| *c != 3).map(|c| self.stats.channel_max_count(c)).max().unwrap_or(1).max(1) as f32;
        let bar_w = plot.width / 256.0;
        let scale = |count: u32| ((count as f32 + 1.0).ln() / (max + 1.0).ln()) * plot.height;
        // The tones of the image in gray; the channels are drawn over it, as lines, only
        // when they differ (a grayscale image is one shape, not three tinted ones).
        let bars = Path::new(|b| {
            for (i, count) in self.stats.hist[4].iter().enumerate() {
                if *count == 0 {
                    continue;
                }
                let h = scale(*count);
                b.rectangle(Point::new(plot.x + i as f32 * bar_w, plot.y + plot.height - h), Size::new(bar_w.max(1.0), h));
            }
        });
        frame.fill(&bars, style::gray(104));
        let colored = self.stats.hist[0] != self.stats.hist[1] || self.stats.hist[1] != self.stats.hist[2];
        if colored {
            let colors = [style::rgb(226, 96, 96), style::rgb(96, 206, 122), style::rgb(104, 150, 236)];
            for (c, color) in colors.iter().enumerate() {
                let curve = Path::new(|b| {
                    for (i, count) in self.stats.hist[c].iter().enumerate() {
                        let p = Point::new(plot.x + (i as f32 + 0.5) * bar_w, plot.y + plot.height - scale(*count));
                        if i == 0 { b.move_to(p) } else { b.line_to(p) }
                    }
                });
                frame.stroke(&curve, Stroke::default().with_width(1.0).with_color(*color));
            }
        }
        let lum = Path::new(|b| {
            for (i, count) in self.stats.hist[4].iter().enumerate() {
                let p = Point::new(plot.x + (i as f32 + 0.5) * bar_w, plot.y + plot.height - scale(*count));
                if i == 0 { b.move_to(p) } else { b.line_to(p) }
            }
        });
        frame.stroke(&lum, Stroke::default().with_width(1.2).with_color(style::gray(230)));

        let fill = [Color::BLACK, style::rgb(128, 128, 128), Color::WHITE];
        for (i, x) in self.handles(plot).iter().enumerate() {
            frame.stroke(
                &Path::line(Point::new(*x, plot.y), Point::new(*x, plot.y + plot.height)),
                Stroke::default().with_width(1.0).with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.16)),
            );
            let base = plot.y + plot.height;
            let tri = Path::new(|b| {
                b.move_to(Point::new(*x, base - 8.0));
                b.line_to(Point::new(*x - 6.0, base + 6.0));
                b.line_to(Point::new(*x + 6.0, base + 6.0));
                b.close();
            });
            frame.fill(&tri, fill[i]);
            let edge = if self.locked[i] { style::TEXT_FAINT } else { style::ACCENT };
            frame.stroke(&tri, Stroke::default().with_width(1.0).with_color(edge));
        }
        if !self.fresh {
            frame.fill_rectangle(Point::ORIGIN, bounds.size(), Color::from_rgba(0.0, 0.0, 0.0, 0.15));
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::mouse::Cursor;
    use iced::widget::canvas::Program;

    fn histogram(lo: f32, hi: f32, gamma: f32) -> Histogram {
        Histogram {
            node: NodeId(1),
            stats: Arc::new(ImageStats {
                width: 4,
                height: 4,
                hist: Box::new([[0; 256]; 5]),
                min: [0.0; 4],
                max: [1.0; 4],
                mean: [0.5; 4],
            }),
            lo,
            hi,
            gamma,
            locked: [false; 3],
            fresh: true,
        }
    }

    /// Drag a handle to `x` (in the canvas) and read the parameter it sets.
    fn drag_to(h: &Histogram, grab: f32, to: f32) -> Option<(String, f32)> {
        let bounds = Rectangle::new(Point::new(0.0, 0.0), Size::new(200.0, 110.0));
        let mut state = State::default();
        let at = |x: f32| Cursor::Available(Point::new(bounds.x + x, bounds.y + 60.0));
        let press = iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        h.update(&mut state, &press, bounds, at(grab))?;
        let moved = iced::Event::Mouse(mouse::Event::CursorMoved { position: Point::new(bounds.x + to, bounds.y + 60.0) });
        let (message, _, _) = h.update(&mut state, &moved, bounds, at(to))?.into_inner();
        match message? {
            Message::Edit(Edit::SetParam { key, value: ParamValue::Float(v), .. }) => Some((key, v)),
            other => panic!("{other:?}"),
        }
    }

    /// Every handle reaches both ends of the histogram: the black and white points the
    /// whole way, and the gamma handle as close to each as a gamma can put mid gray.
    #[test]
    fn the_handles_reach_the_ends() {
        let plot = Histogram::plot(Rectangle::new(Point::new(0.0, 0.0), Size::new(200.0, 110.0)));
        let (left, right) = (plot.x, plot.x + plot.width);
        let h = histogram(0.0, 1.0, 1.0);

        // The white point, dragged from its place to the far left and back out to the right.
        assert_eq!(drag_to(&h, right, left - 50.0), Some(("in_high".into(), 0.01)));
        assert_eq!(drag_to(&h, right, right + 50.0), Some(("in_high".into(), 1.0)));
        // The black point, the same way.
        assert_eq!(drag_to(&h, left, right + 50.0), Some(("in_low".into(), 0.99)));
        assert_eq!(drag_to(&h, left, left - 50.0), Some(("in_low".into(), 0.0)));

        // The gamma handle: pushed to white it is a gamma that makes everything light, and
        // pushed to black one that makes everything dark. Both ends, not 93% of the way.
        let middle = plot.x + plot.width * 0.5;
        let gamma = |to: f32| drag_to(&h, middle, to).expect("a gamma").1;
        let (dark, light) = (gamma(left), gamma(right));
        let (lo, hi) = Histogram::gamma_limits();
        assert!((lo..=hi).contains(&dark) && (lo..=hi).contains(&light), "{dark} {light}");
        // Which puts mid gray at each end, not a tenth of the way short of it.
        assert!(0.5f32.powf(dark) < 0.01, "mid gray at {}", 0.5f32.powf(dark));
        assert!(0.5f32.powf(light) > 0.99, "mid gray at {}", 0.5f32.powf(light));
        // A hair inside the ends is a strong gamma, not the end of its range.
        assert!((6.0..hi).contains(&gamma(left + 1.0)) && (lo..0.05).contains(&gamma(right - 1.0)));

        // With the black and white points moved in, the gamma handle pushed past either of
        // them stays at that end: it never jumps back to the middle.
        let narrow = histogram(0.3, 0.8, 1.0);
        let mid = plot.x + plot.width * (0.3 + 0.5 * 0.5);
        for (to, dark) in [(left - 30.0, true), (left + plot.width * 0.1, true), (right + 30.0, false), (right - plot.width * 0.1, false)] {
            let (key, g) = drag_to(&narrow, mid, to).expect("a gamma");
            assert_eq!(key, "gamma");
            let at = 0.5f32.powf(g);
            assert!(if dark { at < 0.01 } else { at > 0.99 }, "dragged to {to}: gamma {g} puts mid gray at {at}");
        }

        // A black point and a white point in the same place are not a division by zero.
        let flat = histogram(0.5, 0.5, 1.0);
        match flat.edit(1, 0.9) {
            Some(Message::Edit(Edit::SetParam { value: ParamValue::Float(v), .. })) => assert!(v.is_finite(), "{v}"),
            other => panic!("{other:?}"),
        }
    }
}
