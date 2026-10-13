//! A widget that shows a card of its own beside itself while it is pointed at (an (i) that
//! says what something is): not a tooltip of the system. The card stays while the mouse is
//! over the widget or over the card, and a moment after it left both (so that the mouse can
//! go from one to the other), then goes. One card is shown at a time: pointing at another
//! widget's (i) shows its card in place of the one shown, at once.

use std::cell::Cell;
use std::time::{Duration, Instant};

use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{self, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector, mouse, window};

/// How long the card stays after the mouse left the widget and the card.
const GRACE: Duration = Duration::from_millis(350);
const GAP: f32 = 6.0;

pub struct HoverCard<'a, Message> {
    anchor: Element<'a, Message>,
    card: Element<'a, Message>,
}

/// `card` beside `anchor` while `anchor` (or the card) is pointed at.
pub fn hover_card<'a, Message>(anchor: impl Into<Element<'a, Message>>, card: impl Into<Element<'a, Message>>) -> HoverCard<'a, Message> {
    HoverCard { anchor: anchor.into(), card: card.into() }
}

thread_local! {
    /// The card shown (of the widgets of this thread: the interface's), by its widget's number.
    static SHOWN: Cell<u64> = const { Cell::new(0) };
    /// The number the next widget gets.
    static NEXT: Cell<u64> = const { Cell::new(1) };
}

struct State {
    /// Which widget's state it is, for [`SHOWN`].
    id: u64,
    over_anchor: bool,
    over_card: bool,
    shown: bool,
    /// When the mouse left both, while the card is still shown.
    left: Option<Instant>,
}

impl Default for State {
    fn default() -> Self {
        let id = NEXT.with(|n| {
            let id = n.get();
            n.set(id + 1);
            id
        });
        State { id, over_anchor: false, over_card: false, shown: false, left: None }
    }
}

impl State {
    /// Shown while pointed at; gone a moment after it is not, and at once when another widget's
    /// card is shown in its place.
    fn settle(&mut self, now: Instant, shell_redraw: &mut dyn FnMut(Instant)) {
        let others = SHOWN.with(|s| s.get()) != self.id;
        if self.over_anchor {
            SHOWN.with(|s| s.set(self.id));
            self.shown = true;
            self.left = None;
        } else if others {
            // (Another card was pointed at: this one goes, whatever is under the mouse; what the
            // mouse was over of it, gone with it, is over no more.)
            (self.shown, self.over_card, self.left) = (false, false, None);
        } else if self.over_card {
            self.shown = true;
            self.left = None;
        } else if self.shown {
            match self.left {
                None => {
                    self.left = Some(now);
                    shell_redraw(now + GRACE);
                }
                Some(at) if now.duration_since(at) >= GRACE => {
                    self.shown = false;
                    self.left = None;
                }
                Some(at) => shell_redraw(at + GRACE),
            }
        }
    }
}

impl<'a, Message: 'a> Widget<Message, iced::Theme, iced::Renderer> for HoverCard<'a, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.anchor), Tree::new(&self.card)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.anchor, &self.card]);
    }

    fn size(&self) -> Size<Length> {
        self.anchor.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        self.anchor.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.anchor.as_widget_mut().update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
        let now = match event {
            Event::Window(window::Event::RedrawRequested(now)) => *now,
            Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft) => Instant::now(),
            _ => return,
        };
        let state = tree.state.downcast_mut::<State>();
        let over = cursor.is_over(layout.bounds());
        let was = state.shown;
        state.over_anchor = over;
        state.settle(now, &mut |at| shell.request_redraw_at(at));
        if state.shown != was {
            shell.invalidate_layout();
            shell.request_redraw();
        }
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &iced::Renderer) -> mouse::Interaction {
        self.anchor.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.anchor.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &iced::Renderer, operation: &mut dyn widget::Operation) {
        self.anchor.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        let (state, children) = (&mut tree.state, &mut tree.children);
        let mut children = children.iter_mut();
        let anchor_tree = children.next()?;
        let card_tree = children.next()?;
        let anchor_overlay = self.anchor.as_widget_mut().overlay(anchor_tree, layout, renderer, viewport, translation);
        let state = state.downcast_mut::<State>();
        if !state.shown {
            return anchor_overlay;
        }
        let card = overlay::Element::new(Box::new(Card { anchor: layout.bounds() + translation, card: &mut self.card, tree: card_tree, state }));
        Some(overlay::Group::with_children(anchor_overlay.into_iter().chain(Some(card)).collect()).overlay())
    }
}

struct Card<'a, 'b, Message> {
    anchor: Rectangle,
    card: &'b mut Element<'a, Message>,
    tree: &'b mut Tree,
    state: &'b mut State,
}

impl<Message> overlay::Overlay<Message, iced::Theme, iced::Renderer> for Card<'_, '_, Message> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let node = self.card.as_widget_mut().layout(self.tree, renderer, &layout::Limits::new(Size::ZERO, bounds));
        let size = node.size();
        // To the right of the widget, its top level with it; to the left where the right has
        // no room, and never off the window.
        let right = self.anchor.x + self.anchor.width + GAP;
        let x = if right + size.width <= bounds.width { right } else { (self.anchor.x - GAP - size.width).max(0.0) };
        let y = self.anchor.y.min(bounds.height - size.height).max(0.0);
        node.move_to(Point::new(x, y))
    }

    fn draw(&self, renderer: &mut iced::Renderer, theme: &iced::Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor) {
        self.card.as_widget().draw(self.tree, renderer, theme, style, layout, cursor, &layout.bounds());
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        self.card.as_widget_mut().update(self.tree, event, layout, cursor, renderer, clipboard, shell, &layout.bounds());
        let now = match event {
            Event::Window(window::Event::RedrawRequested(now)) => *now,
            Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft) => Instant::now(),
            _ => return,
        };
        let was = self.state.shown;
        self.state.over_card = cursor.is_over(layout.bounds());
        self.state.settle(now, &mut |at| shell.request_redraw_at(at));
        if self.state.shown != was {
            shell.invalidate_layout();
            shell.request_redraw();
        }
    }

    fn mouse_interaction(&self, layout: Layout<'_>, cursor: mouse::Cursor, renderer: &iced::Renderer) -> mouse::Interaction {
        self.card.as_widget().mouse_interaction(self.tree, layout, cursor, &layout.bounds(), renderer)
    }
}

impl<'a, Message: 'a> From<HoverCard<'a, Message>> for Element<'a, Message> {
    fn from(h: HoverCard<'a, Message>) -> Self {
        Element::new(h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The card shows while the widget or the card is pointed at, and goes a moment after
    /// neither is: not at once, so that the mouse can go from one to the other.
    #[test]
    fn the_card_waits_for_the_mouse() {
        let mut state = State::default();
        let mut redraws = Vec::new();
        let t0 = Instant::now();
        state.over_anchor = true;
        state.settle(t0, &mut |at| redraws.push(at));
        assert!(state.shown);
        // Off the (i), on its way to the card: shown still, a frame asked for when it would go.
        state.over_anchor = false;
        state.settle(t0 + Duration::from_millis(10), &mut |at| redraws.push(at));
        assert!(state.shown && redraws == [t0 + Duration::from_millis(10) + GRACE]);
        // On the card: it stays, however long.
        state.over_card = true;
        state.settle(t0 + Duration::from_secs(5), &mut |at| redraws.push(at));
        assert!(state.shown && state.left.is_none());
        // Off both: gone once the moment has passed, not before.
        state.over_card = false;
        let off = t0 + Duration::from_secs(6);
        state.settle(off, &mut |at| redraws.push(at));
        state.settle(off + GRACE / 2, &mut |at| redraws.push(at));
        assert!(state.shown);
        state.settle(off + GRACE, &mut |at| redraws.push(at));
        assert!(!state.shown);
    }

    /// One card at a time: pointing at another (i) while a card is shown (in the moment it waits,
    /// or with the mouse where the cards overlap) shows the other in its place, at once.
    #[test]
    fn one_card_is_shown_at_a_time() {
        let (mut first, mut second) = (State::default(), State::default());
        let t0 = Instant::now();
        let mut redraw = |_: Instant| {};
        first.over_anchor = true;
        first.settle(t0, &mut redraw);
        // Off the first (i), onto the second, within the moment the first waits.
        first.over_anchor = false;
        first.settle(t0 + Duration::from_millis(50), &mut redraw);
        second.over_anchor = true;
        second.settle(t0 + Duration::from_millis(100), &mut redraw);
        // The mouse where both cards would be: the first is gone, the second stays.
        second.over_anchor = false;
        (first.over_card, second.over_card) = (true, true);
        for state in [&mut first, &mut second] {
            state.settle(t0 + Duration::from_millis(150), &mut redraw);
        }
        assert!(!first.shown && second.shown);
        first.settle(t0 + Duration::from_secs(3), &mut redraw);
        assert!(!first.shown, "a card taken over stays gone over the other");
    }
}
