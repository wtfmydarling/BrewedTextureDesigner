//! Keeps the window drawing frame after frame (real-time rendering) and says, a few times a
//! second, how many frames it drew in the last one. It takes no room and draws nothing.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, mouse};
use iced::{Element, Event, Length, Rectangle, Size, window};

/// How far back frames are counted.
const WINDOW: Duration = Duration::from_secs(1);
/// How often the rate is reported: often enough to follow, seldom enough to read.
const EVERY: Duration = Duration::from_millis(250);

/// Frames drawn over the last second.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameRate {
    pub fps: f32,
    /// How long a frame took on average, from one to the next.
    pub frame_ms: f32,
    /// The longest a frame took: a hitch shows here even when the average is fine.
    pub worst_ms: f32,
}

impl FrameRate {
    /// The rate of frames drawn at `frames` (oldest first).
    pub fn of(frames: &VecDeque<Instant>) -> FrameRate {
        let (Some(first), Some(last)) = (frames.front(), frames.back()) else { return FrameRate::default() };
        let span = last.duration_since(*first).as_secs_f32();
        if frames.len() < 2 || span <= 0.0 {
            return FrameRate::default();
        }
        let intervals = frames.len() - 1;
        let worst = frames.iter().zip(frames.iter().skip(1)).map(|(a, b)| b.duration_since(*a).as_secs_f32()).fold(0.0, f32::max);
        FrameRate { fps: intervals as f32 / span, frame_ms: span * 1000.0 / intervals as f32, worst_ms: worst * 1000.0 }
    }
}

pub struct FrameClock<'a, Message> {
    running: bool,
    on_report: Box<dyn Fn(FrameRate) -> Message + 'a>,
}

/// A clock that keeps the window drawing while `running`, reporting the frame rate.
pub fn frame_clock<'a, Message>(running: bool, on_report: impl Fn(FrameRate) -> Message + 'a) -> FrameClock<'a, Message> {
    FrameClock { running, on_report: Box::new(on_report) }
}

#[derive(Default)]
struct State {
    frames: VecDeque<Instant>,
    reported: Option<Instant>,
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for FrameClock<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        // Shrink, not a fixed zero: rows and columns leave out what is fixed at zero.
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &iced::Renderer, _limits: &layout::Limits) -> layout::Node {
        layout::Node::new(Size::ZERO)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let Event::Window(window::Event::RedrawRequested(now)) = event else { return };
        let state = tree.state.downcast_mut::<State>();
        state.frames.push_back(*now);
        while state.frames.front().is_some_and(|f| now.duration_since(*f) > WINDOW) {
            state.frames.pop_front();
        }
        if self.running {
            shell.request_redraw();
        }
        if state.reported.is_none_or(|at| now.duration_since(at) >= EVERY) {
            state.reported = Some(*now);
            shell.publish((self.on_report)(FrameRate::of(&state.frames)));
        }
    }

    fn draw(
        &self,
        _tree: &Tree,
        _renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
    }
}

impl<'a, Message: 'a> From<FrameClock<'a, Message>> for Element<'a, Message> {
    fn from(clock: FrameClock<'a, Message>) -> Self {
        Element::new(clock)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rate_is_what_the_frames_say() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        // Sixty frames a second, one of them late.
        let mut frames: VecDeque<Instant> = (0..=60u64).map(|i| at(i * 1000 / 60)).collect();
        let rate = FrameRate::of(&frames);
        assert!((rate.fps - 60.0).abs() < 0.5, "{rate:?}");
        assert!((rate.frame_ms - 16.67).abs() < 0.1, "{rate:?}");
        frames.push_back(at(1100));
        assert!(FrameRate::of(&frames).worst_ms > 99.0);
        // One frame is no rate at all, and neither is none.
        assert_eq!(FrameRate::of(&VecDeque::from([start])), FrameRate::default());
        assert_eq!(FrameRate::of(&VecDeque::new()), FrameRate::default());
    }
}
