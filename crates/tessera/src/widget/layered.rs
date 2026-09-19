//! A canvas that draws each geometry its program returns in a layer of its own.
//!
//! Within one layer the renderer draws all shapes first, then all images, then all text,
//! so in a plain canvas an image covers every shape, whatever the drawing order. The
//! graph needs its nodes stacked properly (a node's thumbnail must not show through the
//! node above it, nor hide a link being dragged): it returns one geometry per node and
//! this widget layers them in order.

use std::marker::PhantomData;

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::widget::{Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, mouse};
use iced::widget::canvas::Program;
use iced::{Element, Event, Length, Rectangle, Size, Vector, window};

pub struct LayeredCanvas<P, Message> {
    program: P,
    width: Length,
    height: Length,
    last_interaction: Option<mouse::Interaction>,
    _message: PhantomData<Message>,
}

pub fn layered<P, Message>(program: P) -> LayeredCanvas<P, Message>
where
    P: Program<Message>,
{
    LayeredCanvas { program, width: Length::Fill, height: Length::Fill, last_interaction: None, _message: PhantomData }
}

impl<P, Message> Widget<Message, iced::Theme, iced::Renderer> for LayeredCanvas<P, Message>
where
    P: Program<Message>,
{
    fn tag(&self) -> tree::Tag {
        struct Tag<T>(T);
        tree::Tag::of::<Tag<P::State>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(P::State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, self.height)
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        layout::atomic(limits, self.width, self.height)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let state = tree.state.downcast_mut::<P::State>();
        if let Some(action) = self.program.update(state, event, bounds, cursor) {
            let (message, redraw, status) = action.into_inner();
            shell.request_redraw_at(redraw);
            if let Some(message) = message {
                shell.publish(message);
            }
            if status == iced::event::Status::Captured {
                shell.capture_event();
            }
        }
        // Redraw when the cursor shape changes (as the stock canvas does).
        if shell.redraw_request() != window::RedrawRequest::NextFrame {
            let interaction = self.program.mouse_interaction(tree.state.downcast_ref::<P::State>(), bounds, cursor);
            if matches!(event, Event::Window(window::Event::RedrawRequested(_))) {
                self.last_interaction = Some(interaction);
            } else if self.last_interaction.is_some_and(|last| last != interaction) {
                shell.request_redraw();
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.program.mouse_interaction(tree.state.downcast_ref::<P::State>(), layout.bounds(), cursor)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        if bounds.width < 1.0 || bounds.height < 1.0 {
            return;
        }
        let state = tree.state.downcast_ref::<P::State>();
        renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
            let local = Rectangle::with_size(bounds.size());
            for (i, geometry) in self.program.draw(state, renderer, theme, bounds, cursor).into_iter().enumerate() {
                if i == 0 {
                    iced::advanced::graphics::geometry::Renderer::draw_geometry(renderer, geometry);
                } else {
                    renderer.with_layer(local, |renderer| {
                        iced::advanced::graphics::geometry::Renderer::draw_geometry(renderer, geometry);
                    });
                }
            }
        });
    }
}

impl<'a, P, Message> From<LayeredCanvas<P, Message>> for Element<'a, Message>
where
    P: Program<Message> + 'a,
    Message: 'a,
{
    fn from(canvas: LayeredCanvas<P, Message>) -> Self {
        Element::new(canvas)
    }
}
