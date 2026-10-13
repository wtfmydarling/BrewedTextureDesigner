//! The box of what scrolls: every list, panel, menu and dialog that can be longer than its room
//! is one, in every program of the set and in what they share (the Home screen). Its bar is
//! beside what it holds, never over it: while there is something to scroll, the room of the bar
//! is taken from what the box holds (which is laid out that much narrower), and given back when
//! there is not.
//!
//! How the bar is — how wide, how far from what it is beside, what it looks like — is said here
//! and nowhere else. Each place that scrolled said its own before: a thin bar over the ends of
//! its rows, a margin kept where the bar would be, a gap, or nothing at all; and wherever
//! nothing was said, the bar lay over what was listed. **Nothing calls the framework's
//! scrollable itself**: a box made any other way is a bar over its rows waiting to be found
//! again.

use iced::widget::container;
use iced::widget::scrollable::{self, Direction, Scrollable, Scrollbar};
use iced::{Color, Element, Shadow, border};

/// How wide the bar is, and how far from what it is beside.
pub const BAR: f32 = 8.0;
pub const GAP: f32 = 2.0;
/// The room the bar takes from what a box holds, while it shows: what is laid out in a box
/// that may scroll counts it out of the box's width (see the programs' fields that shorten
/// their words to fit).
pub const ROOM: f32 = BAR + GAP;

/// `content`, scrolling up and down where it is higher than its room (see the crate). What a
/// box is besides — how high, its id, what its scrolling says, its look (see [`look`]) — is
/// said of what this returns, as of any scrollable; how its bar is, nowhere but here.
pub fn scroll_box<'a, Message, Theme, Renderer>(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Scrollable<'a, Message, Theme, Renderer>
where
    Theme: scrollable::Catalog + 'a,
    Renderer: iced::advanced::text::Renderer + 'a,
{
    iced::widget::scrollable(content).direction(Direction::Vertical(Scrollbar::new().width(BAR).scroller_width(BAR).spacing(GAP)))
}

/// What a box looks like: its bar a rounded gray, lighter while it is pointed at or dragged; the
/// mark of a scroll with the middle button on `page`, drawn in `dim`.
pub fn look<Theme>(page: Color, dim: Color) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
    let gray = |v: u8| Color::from_rgb8(v, v, v);
    move |_, status| {
        let pointed = matches!(status, scrollable::Status::Hovered { .. } | scrollable::Status::Dragged { .. });
        let rail = scrollable::Rail {
            background: None,
            border: border::rounded(3),
            scroller: scrollable::Scroller { background: if pointed { gray(92) } else { gray(64) }.into(), border: border::rounded(3) },
        };
        scrollable::Style {
            container: container::Style::default(),
            vertical_rail: rail,
            horizontal_rail: rail,
            gap: None,
            auto_scroll: scrollable::AutoScroll {
                background: Color { a: 0.9, ..page }.into(),
                border: border::rounded(u32::MAX).width(1).color(dim),
                shadow: Shadow::default(),
                icon: dim,
            },
        }
    }
}
