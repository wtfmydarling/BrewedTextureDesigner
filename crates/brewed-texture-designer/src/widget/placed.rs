//! A popup at a point of the window, kept inside it: laid out as large as it is (no larger than
//! the window), then placed with its corner at the point, or as near to it as the window lets
//! it be. (A size guessed beforehand put a menu near the bottom of the window higher than it had
//! to be, or let it run off the window.)

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree, Widget};
use iced::advanced::{Clipboard, Shell, mouse, overlay};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector};

/// How far a popup keeps from the edges of the window.
pub const MARGIN: f32 = 4.0;

pub struct Placed<'a, Message> {
    content: Element<'a, Message>,
    at: Point,
}

/// `content` with its top left corner at `at`, or as near as the window lets it be.
pub fn placed<'a, Message>(content: impl Into<Element<'a, Message>>, at: Point) -> Placed<'a, Message> {
    Placed { content: content.into(), at }
}

/// Where a popup of `size` goes in `bounds`, asked to be at `at`.
pub fn place(at: Point, size: Size, bounds: Size) -> Point {
    let x = at.x.min(bounds.width - size.width - MARGIN).max(MARGIN);
    let y = at.y.min(bounds.height - size.height - MARGIN).max(MARGIN);
    Point::new(x, y)
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for Placed<'_, Message> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        let bounds = limits.max();
        let room = Size::new((bounds.width - 2.0 * MARGIN).max(0.0), (bounds.height - 2.0 * MARGIN).max(0.0));
        let child = self.content.as_widget_mut().layout(&mut tree.children[0], renderer, &layout::Limits::new(Size::ZERO, room));
        let at = place(self.at, child.size(), bounds);
        layout::Node::with_children(bounds, vec![child.move_to(at)])
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
        let Some(child) = layout.children().next() else { return };
        self.content.as_widget_mut().update(&mut tree.children[0], event, child, cursor, renderer, clipboard, shell, viewport);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let Some(child) = layout.children().next() else { return mouse::Interaction::None };
        self.content.as_widget().mouse_interaction(&tree.children[0], child, cursor, viewport, renderer)
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
        let Some(child) = layout.children().next() else { return };
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, child, cursor, viewport);
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &iced::Renderer, operation: &mut dyn Operation) {
        let Some(child) = layout.children().next() else { return };
        self.content.as_widget_mut().operate(&mut tree.children[0], child, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        let child = layout.children().next()?;
        self.content.as_widget_mut().overlay(&mut tree.children[0], child, renderer, viewport, translation)
    }
}

impl<'a, Message: 'a> From<Placed<'a, Message>> for Element<'a, Message> {
    fn from(p: Placed<'a, Message>) -> Self {
        Element::new(p)
    }
}
