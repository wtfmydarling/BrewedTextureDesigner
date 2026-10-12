//! The program's box of what scrolls: every list, panel, menu and dialog that can be longer
//! than its room is one — the box of the set of programs (the `scroll-box` crate, which says how
//! its bar is: beside what the box holds, never over it), in this program's look. **Nothing in
//! the program calls the framework's scrollable itself** (a test goes through every place that
//! scrolls, and the monkey through any state, for a bar over what it scrolls).

use iced::Element;
use iced::widget::scrollable::Scrollable;

use crate::style;

/// The room the bar takes from what a box holds, while it shows.
pub use scroll_box::ROOM;

/// `content`, scrolling up and down where it is higher than its room (see the module).
pub fn scroll_box<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Scrollable<'a, Message> {
    scroll_box::scroll_box(content).style(style::scrollable)
}
