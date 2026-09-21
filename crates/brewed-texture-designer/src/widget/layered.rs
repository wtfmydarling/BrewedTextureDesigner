//! A canvas that draws each geometry its program returns in a layer of its own, and keeps
//! what it drew until something changes.
//!
//! Within one layer the renderer draws all shapes first, then all images, then all text,
//! so in a plain canvas an image covers every shape, whatever the drawing order. The
//! graph needs its nodes stacked properly (a node's thumbnail must not show through the
//! node above it, nor hide a link being dragged): it returns one geometry per node and
//! this widget layers them in order.
//!
//! The window draws frame after frame (real-time rendering), and in most of them the graph
//! looks the same as in the one before. The layers are kept on the GPU as they were drawn,
//! and drawn again from there, until the view is rebuilt from the application, an event
//! reaches the canvas or its size changes.

use std::cell::RefCell;
use std::marker::PhantomData;

use iced::advanced::graphics::cache::{Cached, Group};
use iced::advanced::graphics::geometry::Renderer as _;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::widget::{Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, mouse};
use iced::widget::canvas::Program;
use iced::{Element, Event, Length, Rectangle, Size, Vector, window};

type Geometry = <iced::Renderer as iced::advanced::graphics::geometry::Renderer>::Geometry;

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

/// The program's own state, and the layers it drew last.
struct State<S> {
    program: S,
    kept: RefCell<Kept>,
}

struct Kept {
    /// The layers are still what the program would draw.
    fresh: bool,
    size: Size,
    layers: Vec<<Geometry as Cached>::Cache>,
    group: Group,
}

impl<P, Message> LayeredCanvas<P, Message>
where
    P: Program<Message>,
{
    fn state(tree: &Tree) -> &State<P::State> {
        tree.state.downcast_ref::<State<P::State>>()
    }
}

impl<P, Message> Widget<Message, iced::Theme, iced::Renderer> for LayeredCanvas<P, Message>
where
    P: Program<Message>,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State<P::State>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            program: P::State::default(),
            kept: RefCell::new(Kept { fresh: false, size: Size::ZERO, layers: Vec::new(), group: Group::unique() }),
        })
    }

    fn diff(&self, tree: &mut Tree) {
        // Built again from the application: what it shows may have changed.
        tree.state.downcast_mut::<State<P::State>>().kept.get_mut().fresh = false;
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
        let state = tree.state.downcast_mut::<State<P::State>>();
        let redraw = matches!(event, Event::Window(window::Event::RedrawRequested(_)));
        if !redraw {
            // The mouse, the keyboard or the window did something: the program may draw
            // differently now.
            state.kept.get_mut().fresh = false;
        }
        if let Some(action) = self.program.update(&mut state.program, event, bounds, cursor) {
            let (message, request, status) = action.into_inner();
            shell.request_redraw_at(request);
            state.kept.get_mut().fresh = false;
            if let Some(message) = message {
                shell.publish(message);
            }
            if status == iced::event::Status::Captured {
                shell.capture_event();
            }
        }
        // Redraw when the cursor shape changes (as the stock canvas does).
        if shell.redraw_request() != window::RedrawRequest::NextFrame {
            let interaction = self.program.mouse_interaction(&state.program, bounds, cursor);
            if redraw {
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
        self.program.mouse_interaction(&Self::state(tree).program, layout.bounds(), cursor)
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
        let state = Self::state(tree);
        let mut kept = state.kept.borrow_mut();
        if !kept.fresh || kept.size != bounds.size() {
            let geometries = self.program.draw(&state.program, renderer, theme, bounds, cursor);
            let group = kept.group;
            let mut previous = std::mem::take(&mut kept.layers).into_iter();
            kept.layers = geometries.into_iter().map(|g| g.cache(group, previous.next())).collect();
            kept.size = bounds.size();
            kept.fresh = true;
        }
        renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
            let local = Rectangle::with_size(bounds.size());
            for (i, layer) in kept.layers.iter().enumerate() {
                let geometry = Geometry::load(layer);
                if i == 0 {
                    renderer.draw_geometry(geometry);
                } else {
                    renderer.with_layer(local, |renderer| renderer.draw_geometry(geometry));
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
