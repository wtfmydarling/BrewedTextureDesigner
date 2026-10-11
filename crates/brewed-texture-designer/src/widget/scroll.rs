//! The program's box of what scrolls: every list, panel and dialog that can be longer than its
//! room is one. Its bar is beside what it holds, never over it: while there is something to
//! scroll, the room of the bar is taken from what it holds (which is laid out narrower), and
//! given back when there is not. Every such box is this one, so that none can forget: each
//! said its own before (a thin bar over the ends of its rows, a margin where the bar would be,
//! a gap, or nothing at all), and wherever nothing was said the bar lay over what was listed.

use iced::Element;
use iced::widget::scrollable::{Direction, Scrollable, Scrollbar};

use crate::style;

/// How wide the bar is, and how far from what it is beside.
pub const BAR: f32 = 8.0;
pub const GAP: f32 = 2.0;
/// The room the bar takes from what a box holds, while it shows.
#[cfg(test)]
pub const ROOM: f32 = BAR + GAP;

/// `content`, scrolling up and down where it is higher than its room (see the module). What a
/// box of the program is besides (how high, its id, what its scrolling says) is said of what
/// this returns, as of any scrollable; how its bar is, nowhere but here.
pub fn scroll_box<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Scrollable<'a, Message> {
    iced::widget::scrollable(content)
        .direction(Direction::Vertical(Scrollbar::new().width(BAR).scroller_width(BAR).spacing(GAP)))
        .style(style::scrollable)
}
