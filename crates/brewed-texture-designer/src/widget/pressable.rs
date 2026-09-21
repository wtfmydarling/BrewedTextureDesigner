//! A list row that reacts on mouse *press* (to start drag and drop) and on double
//! click, highlighted while hovered.

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::widget::{Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell};
use iced::advanced::mouse;
use iced::{Background, Border, Element, Event, Length, Padding, Rectangle, Size};

use crate::style;

pub struct Pressable<'a, Message> {
    content: Element<'a, Message>,
    on_press: Option<Message>,
    on_double_click: Option<Message>,
    padding: Padding,
}

pub fn pressable<'a, Message>(content: impl Into<Element<'a, Message>>) -> Pressable<'a, Message> {
    Pressable { content: content.into(), on_press: None, on_double_click: None, padding: Padding::from([3, 8]) }
}

impl<Message> Pressable<'_, Message> {
    pub fn on_press(mut self, m: Message) -> Self {
        self.on_press = Some(m);
        self
    }
    pub fn on_double_click(mut self, m: Message) -> Self {
        self.on_double_click = Some(m);
        self
    }
    pub fn padding(mut self, p: impl Into<Padding>) -> Self {
        self.padding = p.into();
        self
    }
}

#[derive(Default)]
struct State {
    last_click: Option<mouse::Click>,
}

impl<Message: Clone> Widget<Message, iced::Theme, iced::Renderer> for Pressable<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        layout::padded(limits, Length::Fill, Length::Shrink, self.padding, |limits| {
            self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
        })
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
        let child = layout.children().next().expect("padded layout has a child");
        self.content.as_widget_mut().update(&mut tree.children[0], event, child, cursor, renderer, clipboard, shell, viewport);
        if shell.is_event_captured() {
            return;
        }
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && let Some(p) = cursor.position_over(layout.bounds())
        {
            let state = tree.state.downcast_mut::<State>();
            let click = mouse::Click::new(p, mouse::Button::Left, state.last_click);
            state.last_click = Some(click);
            let double = matches!(click.kind(), mouse::click::Kind::Double);
            match (&self.on_double_click, &self.on_press) {
                (Some(m), _) if double => shell.publish(m.clone()),
                (_, Some(m)) => shell.publish(m.clone()),
                _ => return,
            }
            shell.capture_event();
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let child = layout.children().next().expect("padded layout has a child");
        let inner = self.content.as_widget().mouse_interaction(&tree.children[0], child, cursor, viewport, renderer);
        if inner == mouse::Interaction::None && cursor.is_over(layout.bounds()) && self.on_press.is_some() {
            mouse::Interaction::Grab
        } else {
            inner
        }
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
        if cursor.is_over(layout.bounds()) {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: layout.bounds(),
                    border: Border { radius: 3.0.into(), ..Border::default() },
                    ..Default::default()
                },
                Background::Color(style::HOVER),
            );
        }
        let child = layout.children().next().expect("padded layout has a child");
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, child, cursor, viewport);
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &iced::Renderer, operation: &mut dyn Operation) {
        let child = layout.children().next().expect("padded layout has a child");
        self.content.as_widget_mut().operate(&mut tree.children[0], child, renderer, operation);
    }
}

impl<'a, Message: Clone + 'a> From<Pressable<'a, Message>> for Element<'a, Message> {
    fn from(p: Pressable<'a, Message>) -> Self {
        Element::new(p)
    }
}
