//! What lies under an open menu: blind to the mouse while the menu is open. iced tells what is
//! under its overlays the cursor is gone only where the deepest of them says the cursor is its
//! own, so over a menu whose submenu is open (File → Open Recent Package) the rows of the
//! Explorer under it took the cursor as theirs, and showed their tooltips. A click beside a
//! menu only closes it, so nothing under it is to be pointed at meanwhile.

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree, Widget};
use iced::advanced::{Clipboard, Shell, mouse, overlay};
use iced::{Element, Event, Length, Rectangle, Size, Vector};

pub struct Blind<'a, Message> {
    content: Element<'a, Message>,
    blind: bool,
}

/// `content`, which sees no cursor while `blind` (a menu over it is open).
pub fn blind<'a, Message>(content: impl Into<Element<'a, Message>>, blind: bool) -> Blind<'a, Message> {
    Blind { content: content.into(), blind }
}

impl<Message> Blind<'_, Message> {
    fn cursor(&self, cursor: mouse::Cursor) -> mouse::Cursor {
        if self.blind { mouse::Cursor::Unavailable } else { cursor }
    }
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for Blind<'_, Message> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
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
        let cursor = self.cursor(cursor);
        self.content.as_widget_mut().update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(&tree.children[0], layout, self.cursor(cursor), viewport, renderer)
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
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, self.cursor(cursor), viewport);
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &iced::Renderer, operation: &mut dyn Operation) {
        self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

impl<'a, Message: 'a> From<Blind<'a, Message>> for Element<'a, Message> {
    fn from(b: Blind<'a, Message>) -> Self {
        Element::new(b)
    }
}
