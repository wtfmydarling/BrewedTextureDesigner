//! A widget that shows a floating element (a menu) below itself while `expanded`.
//! Clicking outside of both calls `on_dismiss`.

use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{self, Operation, Tree, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector, mouse};

pub struct DropDown<'a, Message> {
    underlay: Element<'a, Message>,
    menu: Element<'a, Message>,
    expanded: bool,
    on_dismiss: Option<Message>,
}

pub fn dropdown<'a, Message>(
    underlay: impl Into<Element<'a, Message>>,
    menu: impl Into<Element<'a, Message>>,
    expanded: bool,
) -> DropDown<'a, Message> {
    DropDown { underlay: underlay.into(), menu: menu.into(), expanded, on_dismiss: None }
}

impl<Message> DropDown<'_, Message> {
    pub fn on_dismiss(mut self, message: Message) -> Self {
        self.on_dismiss = Some(message);
        self
    }
}

impl<'a, Message: Clone + 'a> Widget<Message, iced::Theme, iced::Renderer> for DropDown<'a, Message> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.underlay), Tree::new(&self.menu)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.underlay, &self.menu]);
    }

    fn size(&self) -> Size<Length> {
        self.underlay.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        self.underlay.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
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
        self.underlay.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.underlay.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
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
        self.underlay.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.underlay.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        let mut children = tree.children.iter_mut();
        let underlay_tree = children.next()?;
        let menu_tree = children.next()?;
        let underlay_overlay =
            self.underlay.as_widget_mut().overlay(underlay_tree, layout, renderer, viewport, translation);
        if !self.expanded {
            return underlay_overlay;
        }
        let anchor = layout.bounds() + translation;
        let menu = overlay::Element::new(Box::new(MenuOverlay {
            anchor,
            menu: &mut self.menu,
            tree: menu_tree,
            on_dismiss: self.on_dismiss.clone(),
        }));
        Some(overlay::Group::with_children(underlay_overlay.into_iter().chain(Some(menu)).collect()).overlay())
    }
}

struct MenuOverlay<'a, 'b, Message> {
    anchor: Rectangle,
    menu: &'b mut Element<'a, Message>,
    tree: &'b mut Tree,
    on_dismiss: Option<Message>,
}

impl<Message: Clone> overlay::Overlay<Message, iced::Theme, iced::Renderer> for MenuOverlay<'_, '_, Message> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let limits = layout::Limits::new(Size::ZERO, bounds);
        let node = self.menu.as_widget_mut().layout(self.tree, renderer, &limits);
        let size = node.size();
        // Below the anchor, kept inside the window.
        let x = self.anchor.x.min(bounds.width - size.width).max(0.0);
        let below = self.anchor.y + self.anchor.height + 2.0;
        let y = if below + size.height > bounds.height { (self.anchor.y - size.height - 2.0).max(0.0) } else { below };
        node.move_to(Point::new(x, y))
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.menu.as_widget().draw(self.tree, renderer, theme, style, layout, cursor, &layout.bounds());
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
        if let Event::Mouse(mouse::Event::ButtonPressed(_)) = event
            && !cursor.is_over(layout.bounds())
            && !cursor.is_over(self.anchor)
        {
            if let Some(m) = &self.on_dismiss {
                shell.publish(m.clone());
            }
            return;
        }
        self.menu.as_widget_mut().update(self.tree, event, layout, cursor, renderer, clipboard, shell, &layout.bounds());
    }

    fn mouse_interaction(&self, layout: Layout<'_>, cursor: mouse::Cursor, renderer: &iced::Renderer) -> mouse::Interaction {
        self.menu.as_widget().mouse_interaction(self.tree, layout, cursor, &layout.bounds(), renderer)
    }

    fn operate(&mut self, layout: Layout<'_>, renderer: &iced::Renderer, operation: &mut dyn widget::Operation) {
        self.menu.as_widget_mut().operate(self.tree, layout, renderer, operation);
    }
}

impl<'a, Message: Clone + 'a> From<DropDown<'a, Message>> for Element<'a, Message> {
    fn from(d: DropDown<'a, Message>) -> Self {
        Element::new(d)
    }
}
