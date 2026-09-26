//! A field that is done with when the mouse is pressed anywhere else in the window: a name
//! being typed is taken as it is then, as Enter takes it. A press outside the program is not
//! seen by the window, so the name stays being typed (while a word is copied for it from
//! elsewhere, say).

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree, Widget};
use iced::advanced::{Clipboard, Shell, mouse, overlay};
use iced::{Element, Event, Length, Rectangle, Size, Vector, touch};

pub struct Outside<'a, Message> {
    content: Element<'a, Message>,
    on_press: Option<Message>,
}

/// `content`, told when the mouse is pressed elsewhere (see [`Outside::on_press`]).
pub fn outside<'a, Message>(content: impl Into<Element<'a, Message>>) -> Outside<'a, Message> {
    Outside { content: content.into(), on_press: None }
}

impl<Message> Outside<'_, Message> {
    /// What a press anywhere else in the window says (`None`: nothing; the widget stays, so
    /// that the field keeps its focus whichever it is).
    pub fn on_press(mut self, message: Option<Message>) -> Self {
        self.on_press = message;
        self
    }
}

impl<Message: Clone> Widget<Message, iced::Theme, iced::Renderer> for Outside<'_, Message> {
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
        // (Before the field sees the press: it lets go of the focus then.)
        let pressed = matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_)) | Event::Touch(touch::Event::FingerPressed { .. }));
        if pressed
            && !cursor.is_over(layout.bounds())
            && let Some(message) = &self.on_press
        {
            shell.publish(message.clone());
        }
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
        self.content.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
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
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
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

impl<'a, Message: Clone + 'a> From<Outside<'a, Message>> for Element<'a, Message> {
    fn from(o: Outside<'a, Message>) -> Self {
        Element::new(o)
    }
}
