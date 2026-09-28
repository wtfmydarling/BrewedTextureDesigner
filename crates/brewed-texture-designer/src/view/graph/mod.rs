//! The infinite node canvas.
//!
//! Interaction state (what is being dragged) lives in the canvas widget; everything that
//! changes the document is published as a [`GraphMessage`]. Nodes being moved are drawn
//! at their dragged position and committed as one edit on release.

pub mod layout;

use std::collections::HashSet;

use iced::alignment::Vertical;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::widget::text::{Alignment as TextAlign, LineHeight, Shaping};
use iced::advanced::mouse;
use iced::{Color, Element, Pixels, Point, Rectangle, Size, Vector, keyboard};
use brewed_texture_core::graph::{Comment, CommentId, Link, Node, NodeId, SocketRef};
use brewed_texture_core::nodes::{ImageKind, OUTPUT_FORMATS, Role, Slot};
use brewed_texture_core::{DataType, NodeKind, Value, can_connect};

use crate::app::{App, ContextTarget, GraphMessage, Message, SelectMode};
use crate::style;
use layout::{CommentLayout, LINE_H, NodeLayout, SOCKET_R, Transform};

#[derive(Clone, Debug, Default)]
enum Interaction {
    #[default]
    None,
    Panning {
        last: Point,
    },
    PressedRight {
        at: Point,
    },
    PressedNode {
        node: NodeId,
        at: Point,
    },
    Moving {
        ids: Vec<NodeId>,
        origin: Point,
        current: Point,
    },
    Linking {
        from: SocketRef,
        ty: DataType,
        anchor: Point,
        current: Point,
        /// The link picked up from a connected input: hidden while dragging, replaced or
        /// removed on release (one undo step), kept if the drop is not valid.
        detached: Option<Link>,
    },
    PressedCanvas {
        at: Point,
    },
    Boxing {
        start: Point,
        current: Point,
        additive: bool,
    },
    /// The title bar of a comment was pressed; it becomes a move once the cursor travels.
    PressedComment {
        id: CommentId,
        at: Point,
    },
    MovingComment {
        id: CommentId,
        /// The nodes the comment held when the move started; they travel with it.
        nodes: Vec<NodeId>,
        origin: Point,
        current: Point,
    },
    ResizingComment {
        id: CommentId,
        /// The size it had when the corner was grabbed, in graph units.
        from: [f32; 2],
        origin: Point,
        current: Point,
    },
}

#[derive(Default)]
pub struct State {
    interaction: Interaction,
    last_click: Option<mouse::Click>,
    hovered_link: Option<Link>,
    cursor: Option<Point>,
    bounds: Rectangle,
    modifiers: keyboard::Modifiers,
}

pub struct GraphCanvas<'a> {
    app: &'a App,
}

pub fn view(app: &App) -> Element<'_, Message> {
    // Layered: every node is drawn in a layer of its own (see `widget::layered`).
    crate::widget::layered::layered(GraphCanvas { app }).into()
}

fn publish(message: GraphMessage) -> Option<canvas::Action<Message>> {
    Some(canvas::Action::publish(Message::Graph(message)).and_capture())
}

struct Hit {
    node: NodeId,
    key: String,
    output: bool,
    ty: DataType,
    pos: Point,
}

impl GraphCanvas<'_> {
    fn transform(&self) -> Transform {
        Transform { pan: self.app.ui.graph_view.pan, zoom: self.app.ui.graph_view.zoom }
    }

    /// Layouts in drawing order (selected nodes on top), with an optional drag offset.
    fn layouts(&self, moving: Option<(&[NodeId], Vector)>) -> Vec<NodeLayout> {
        let graph = self.app.graph();
        let t = self.transform();
        let sel = &self.app.ui.selection;
        let mut nodes: Vec<&Node> = graph.nodes.iter().filter(|n| !sel.contains(&n.id)).collect();
        nodes.extend(graph.nodes.iter().filter(|n| sel.contains(&n.id)));
        nodes
            .into_iter()
            .map(|n| {
                let offset = match moving {
                    Some((ids, d)) if ids.contains(&n.id) => d,
                    _ => Vector::ZERO,
                };
                layout::layout(graph, n, &t, offset)
            })
            .collect()
    }

    fn socket_at(&self, layouts: &[NodeLayout], p: Point) -> Option<Hit> {
        let zoom = self.transform().zoom;
        let radius = (SOCKET_R * zoom).max(4.0) + 6.0;
        let mut best: Option<(f32, Hit)> = None;
        for l in layouts.iter().rev() {
            for (list, output) in [(&l.inputs, false), (&l.outputs, true)] {
                for (s, pos) in list {
                    let d = pos.distance(p);
                    if d <= radius && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                        best = Some((d, Hit { node: l.id, key: s.key.clone(), output, ty: s.ty, pos: *pos }));
                    }
                }
            }
        }
        best.map(|(_, h)| h)
    }

    fn node_at(&self, layouts: &[NodeLayout], p: Point) -> Option<NodeId> {
        layouts.iter().rev().find(|l| l.rect.contains(p)).map(|l| l.id)
    }

    fn link_at(&self, layouts: &[NodeLayout], p: Point) -> Option<Link> {
        let zoom = self.transform().zoom;
        let mut best = (7.0f32, None);
        for link in &self.app.graph().links {
            if let Some((a, b)) = layout::link_endpoints(layouts, link) {
                let d = layout::bezier_distance(&layout::bezier(a, b, zoom), p);
                if d < best.0 {
                    best = (d, Some(link.clone()));
                }
            }
        }
        best.1
    }

    /// Where the comments are, with the one being dragged or resized where the mouse has
    /// put it. In drawing order (bottom first).
    fn comment_layouts(&self, state: &State) -> Vec<CommentLayout> {
        let t = self.transform();
        let z = t.zoom;
        self.app
            .graph()
            .comments
            .iter()
            .map(|c| match &state.interaction {
                Interaction::MovingComment { id, origin, current, .. } if *id == c.id => {
                    layout::comment_layout(c, &t, Vector::new((current.x - origin.x) / z, (current.y - origin.y) / z), None)
                }
                Interaction::ResizingComment { id, from, origin, current } if *id == c.id => {
                    let size = resized(*from, *origin, *current, z);
                    layout::comment_layout(c, &t, Vector::ZERO, Some(size))
                }
                _ => layout::comment_layout(c, &t, Vector::ZERO, None),
            })
            .collect()
    }

    /// The comment whose corner is under `p` (the topmost one).
    fn comment_grip_at(&self, comments: &[CommentLayout], p: Point) -> Option<CommentId> {
        comments.iter().rev().find(|c| c.grip.contains(p)).map(|c| c.id)
    }

    /// The comment whose title bar is under `p` (the topmost one). The rest of a comment
    /// is not a hit: the canvas below it stays usable.
    fn comment_header_at(&self, comments: &[CommentLayout], p: Point) -> Option<CommentId> {
        comments.iter().rev().find(|c| c.header.contains(p)).map(|c| c.id)
    }

    /// The nodes a comment frames, as they are now: they move with it.
    fn nodes_in_comment(&self, id: CommentId) -> Vec<NodeId> {
        let graph = self.app.graph();
        let Some(comment) = graph.comment(id) else { return Vec::new() };
        graph
            .nodes
            .iter()
            .filter(|n| comment.holds(n.pos, layout::node_size(graph, n)))
            .map(|n| n.id)
            .collect()
    }
    /// Can a link dragged from `from` (of type `ty`) end at `to`? `blocked`: the nodes it cannot
    /// end at without a cycle (see [`Self::blocked`]), found once for all the sockets asked about.
    fn can_link(&self, from: &SocketRef, ty: DataType, to: &Hit, blocked: &HashSet<NodeId>) -> bool {
        if from.output == to.output || from.node == to.node {
            return false;
        }
        let (out_ty, in_ty) = if from.output { (ty, to.ty) } else { (to.ty, ty) };
        can_connect(out_ty, in_ty) && !blocked.contains(&to.node)
    }

    /// The nodes a link dragged from `from` cannot end at: from an output, the nodes its node
    /// takes from (they would take from it); from an input, those that take from its node.
    fn blocked(&self, from: &SocketRef) -> HashSet<NodeId> {
        let graph = self.app.graph();
        if from.output { graph.upstream(from.node) } else { graph.downstream(from.node) }
    }

    fn select_mode(&self, m: keyboard::Modifiers) -> SelectMode {
        if m.command() {
            SelectMode::Toggle
        } else if m.shift() {
            SelectMode::Add
        } else {
            SelectMode::Replace
        }
    }
}

impl canvas::Program<Message> for GraphCanvas<'_> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        use iced::Event;
        if state.bounds != bounds {
            state.bounds = bounds;
            return Some(canvas::Action::publish(Message::Graph(GraphMessage::Bounds(bounds))));
        }
        if let Event::Keyboard(keyboard::Event::ModifiersChanged(m)) = event {
            state.modifiers = *m;
            return None;
        }
        let local = cursor.position_in(bounds);
        let window = |p: Point| p + Vector::new(bounds.x, bounds.y);
        let t = self.transform();

        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let p = cursor.position().map(|p| p - Vector::new(bounds.x, bounds.y));
                state.cursor = local;
                match &mut state.interaction {
                    // Only how far the mouse went: several moves arrive in one frame, before the
                    // app has taken the previous ones, and each adds to what is there by then.
                    Interaction::Panning { last } => {
                        let p = p?;
                        let delta = p - *last;
                        *last = p;
                        return publish(GraphMessage::PanBy(delta));
                    }
                    Interaction::PressedRight { at } => {
                        let p = p?;
                        if p.distance(*at) > 4.0 {
                            let delta = p - *at;
                            state.interaction = Interaction::Panning { last: p };
                            return publish(GraphMessage::PanBy(delta));
                        }
                    }
                    Interaction::PressedNode { node, at } => {
                        let p = p?;
                        if p.distance(*at) > 3.0 {
                            let node = *node;
                            let origin = *at;
                            let sel = &self.app.ui.selection;
                            let ids: Vec<NodeId> = if sel.contains(&node) { sel.iter().copied().collect() } else { vec![node] };
                            state.interaction = Interaction::Moving { ids, origin, current: p };
                            if !sel.contains(&node) {
                                return publish(GraphMessage::Select { node, mode: SelectMode::Replace });
                            }
                            return Some(canvas::Action::request_redraw());
                        }
                    }
                    Interaction::Moving { current, .. } | Interaction::Linking { current, .. } => {
                        *current = p?;
                        return Some(canvas::Action::request_redraw());
                    }
                    Interaction::PressedComment { id, at } => {
                        let p = p?;
                        if p.distance(*at) > 3.0 {
                            let id = *id;
                            let origin = *at;
                            let nodes = self.nodes_in_comment(id);
                            state.interaction = Interaction::MovingComment { id, nodes, origin, current: p };
                            return Some(canvas::Action::request_redraw());
                        }
                    }
                    Interaction::MovingComment { current, .. } | Interaction::ResizingComment { current, .. } => {
                        *current = p?;
                        return Some(canvas::Action::request_redraw());
                    }
                    Interaction::PressedCanvas { at } => {
                        let p = p?;
                        if p.distance(*at) > 3.0 {
                            let additive = state.modifiers.shift() || state.modifiers.command();
                            state.interaction = Interaction::Boxing { start: *at, current: p, additive };
                            return Some(canvas::Action::request_redraw());
                        }
                    }
                    Interaction::Boxing { current, .. } => {
                        *current = p?;
                        return Some(canvas::Action::request_redraw());
                    }
                    Interaction::None => {
                        let hovered = match local {
                            Some(p) => {
                                let layouts = self.layouts(None);
                                if self.node_at(&layouts, p).is_some() || self.socket_at(&layouts, p).is_some() {
                                    None
                                } else {
                                    self.link_at(&layouts, p)
                                }
                            }
                            None => None,
                        };
                        if hovered != state.hovered_link {
                            state.hovered_link = hovered;
                            return Some(canvas::Action::request_redraw());
                        }
                        // Keep the drop hint of a panel drag up to date.
                        if self.app.ui.drag.is_some() {
                            return Some(canvas::Action::request_redraw());
                        }
                    }
                }
                None
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let p = local?;
                let dy = crate::view::wheel(self.app, delta);
                publish(GraphMessage::ZoomAt { at: p, factor: (dy * 0.0018).exp() })
            }
            Event::Mouse(mouse::Event::ButtonPressed(button)) => {
                let p = local?;
                match button {
                    mouse::Button::Middle => {
                        state.interaction = Interaction::Panning { last: p };
                        Some(canvas::Action::capture())
                    }
                    mouse::Button::Right => {
                        state.interaction = Interaction::PressedRight { at: p };
                        Some(canvas::Action::capture())
                    }
                    mouse::Button::Left => {
                        if state.modifiers.alt() {
                            state.interaction = Interaction::Panning { last: p };
                            return Some(canvas::Action::capture());
                        }
                        let click = mouse::Click::new(p, mouse::Button::Left, state.last_click);
                        state.last_click = Some(click);
                        let double = matches!(click.kind(), mouse::click::Kind::Double | mouse::click::Kind::Triple);
                        let layouts = self.layouts(None);
                        let graph = self.app.graph();
                        if let Some(hit) = self.socket_at(&layouts, p) {
                            // Grabbing a connected input picks its link up for re-routing.
                            if !hit.output
                                && let Some(link) = graph.link_into(hit.node, &hit.key).cloned()
                                && let Some(src) = layouts.iter().find(|l| l.id == link.from)
                                && let Some((s, pos)) = src.outputs.iter().find(|(s, _)| s.key == link.from_socket)
                            {
                                let from = SocketRef { node: link.from, key: link.from_socket.clone(), output: true };
                                state.interaction = Interaction::Linking { from, ty: s.ty, anchor: *pos, current: p, detached: Some(link) };
                                return Some(canvas::Action::request_redraw().and_capture());
                            }
                            let from = SocketRef { node: hit.node, key: hit.key.clone(), output: hit.output };
                            state.interaction = Interaction::Linking { from, ty: hit.ty, anchor: hit.pos, current: p, detached: None };
                            return Some(canvas::Action::request_redraw().and_capture());
                        }
                        if let Some(node) = self.node_at(&layouts, p) {
                            if double {
                                state.interaction = Interaction::None;
                                return publish(GraphMessage::Preview(node));
                            }
                            state.interaction = Interaction::PressedNode { node, at: p };
                            return Some(canvas::Action::capture());
                        }
                        // Comments are behind the nodes: their corner resizes, their title bar moves
                        // them, and the rest of them lets the canvas through.
                        let comments = self.comment_layouts(state);
                        if let Some(id) = self.comment_grip_at(&comments, p) {
                            let from = graph.comment(id).map_or(Comment::MIN_SIZE, |c| c.size);
                            state.interaction = Interaction::ResizingComment { id, from, origin: p, current: p };
                            return Some(canvas::Action::request_redraw().and_capture());
                        }
                        if let Some(id) = self.comment_header_at(&comments, p) {
                            if double {
                                state.interaction = Interaction::None;
                                return publish(GraphMessage::EditComment(id));
                            }
                            state.interaction = Interaction::PressedComment { id, at: p };
                            return Some(canvas::Action::capture());
                        }
                        if let Some(link) = self.link_at(&layouts, p) {
                            return publish(GraphMessage::SelectLink(link));
                        }
                        if double {
                            state.interaction = Interaction::None;
                            return publish(GraphMessage::OpenAddMenu { at: window(p), graph_pos: t.to_graph(p) });
                        }
                        state.interaction = Interaction::PressedCanvas { at: p };
                        Some(canvas::Action::capture())
                    }
                    _ => None,
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(button)) => {
                let interaction = std::mem::take(&mut state.interaction);
                match (button, interaction) {
                    (_, Interaction::Panning { .. }) => Some(canvas::Action::request_redraw()),
                    (mouse::Button::Right, Interaction::PressedRight { at }) => {
                        let layouts = self.layouts(None);
                        let comments = self.comment_layouts(state);
                        let target = if let Some(node) = self.node_at(&layouts, at) {
                            ContextTarget::Node(node)
                        } else if let Some(link) = self.link_at(&layouts, at) {
                            ContextTarget::Link(link)
                        } else if let Some(id) = self.comment_header_at(&comments, at).or_else(|| self.comment_grip_at(&comments, at)) {
                            ContextTarget::Comment(id)
                        } else {
                            ContextTarget::Canvas { graph_pos: t.to_graph(at) }
                        };
                        publish(GraphMessage::Context { at: window(at), target })
                    }
                    (mouse::Button::Left, Interaction::PressedNode { node, .. }) => {
                        publish(GraphMessage::Select { node, mode: self.select_mode(state.modifiers) })
                    }
                    (mouse::Button::Left, Interaction::Moving { ids, origin, current }) => {
                        let d = current - origin;
                        publish(GraphMessage::Move { ids, delta: [d.x / t.zoom, d.y / t.zoom] })
                    }
                    (mouse::Button::Left, Interaction::Linking { from, ty, current, detached, .. }) => {
                        let layouts = self.layouts(None);
                        let blocked = self.blocked(&from);
                        match self.socket_at(&layouts, current) {
                            Some(target) if self.can_link(&from, ty, &target, &blocked) => {
                                // Dropped back where it was: nothing changes.
                                if detached.as_ref().is_some_and(|l| l.to == target.node && l.to_socket == target.key) {
                                    return Some(canvas::Action::request_redraw());
                                }
                                let to = SocketRef { node: target.node, key: target.key, output: target.output };
                                publish(GraphMessage::Connect { from, to, replacing: detached })
                            }
                            Some(target) if target.node != from.node && target.output != from.output => {
                                let (o, i, on, inn) = if from.output {
                                    (ty, target.ty, from.node, target.node)
                                } else {
                                    (target.ty, ty, target.node, from.node)
                                };
                                let msg = if self.app.graph().depends_on(on, inn) {
                                    "This connection would create a cycle".to_string()
                                } else {
                                    format!("Cannot connect {} to {}", o.name(), i.name())
                                };
                                publish(GraphMessage::Error(msg))
                            }
                            Some(_) => Some(canvas::Action::request_redraw()),
                            None if local.is_some() => publish(GraphMessage::LinkDropped {
                                from,
                                at: window(current),
                                graph_pos: t.to_graph(current),
                                detached,
                            }),
                            None => Some(canvas::Action::request_redraw()),
                        }
                    }
                    (mouse::Button::Left, Interaction::PressedCanvas { .. }) => {
                        if state.modifiers.shift() || state.modifiers.command() {
                            None
                        } else {
                            publish(GraphMessage::ClearSelection)
                        }
                    }
                    (mouse::Button::Left, Interaction::Boxing { start, current, additive }) => {
                        let r = Rectangle::new(
                            Point::new(start.x.min(current.x), start.y.min(current.y)),
                            Size::new((start.x - current.x).abs(), (start.y - current.y).abs()),
                        );
                        let ids = self.layouts(None).iter().filter(|l| l.rect.intersects(&r)).map(|l| l.id).collect();
                        publish(GraphMessage::SelectBox { ids, additive })
                    }
                    (mouse::Button::Left, Interaction::PressedComment { id, .. }) => publish(GraphMessage::SelectComment(id)),
                    (mouse::Button::Left, Interaction::MovingComment { id, nodes, origin, current }) => {
                        let d = current - origin;
                        publish(GraphMessage::MoveComment { id, delta: [d.x / t.zoom, d.y / t.zoom], nodes })
                    }
                    (mouse::Button::Left, Interaction::ResizingComment { id, from, origin, current }) => {
                        publish(GraphMessage::ResizeComment { id, size: resized(from, origin, current, t.zoom) })
                    }
                    (mouse::Button::Left, Interaction::None) => {
                        // Dropping something dragged from a panel.
                        let payload = self.app.ui.drag.clone()?;
                        let p = local?;
                        publish(GraphMessage::Drop { payload, graph_pos: t.to_graph(p) })
                    }
                    (_, other) => {
                        state.interaction = other;
                        None
                    }
                }
            }
            _ => None,
        }
    }

    fn mouse_interaction(&self, state: &State, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        match state.interaction {
            Interaction::Panning { .. } | Interaction::Moving { .. } | Interaction::MovingComment { .. } => mouse::Interaction::Grabbing,
            Interaction::ResizingComment { .. } => mouse::Interaction::ResizingDiagonallyDown,
            Interaction::Linking { .. } => mouse::Interaction::Crosshair,
            _ => {
                let Some(p) = cursor.position_in(bounds) else { return mouse::Interaction::default() };
                if let Some(payload) = &self.app.ui.drag {
                    return if self.app.drop_refusal(payload).is_some() {
                        mouse::Interaction::NotAllowed
                    } else {
                        mouse::Interaction::Copy
                    };
                }
                let layouts = self.layouts(None);
                if self.socket_at(&layouts, p).is_some() || state.hovered_link.is_some() {
                    return mouse::Interaction::Pointer;
                }
                if self.node_at(&layouts, p).is_some() {
                    return mouse::Interaction::default();
                }
                let comments = self.comment_layouts(state);
                if self.comment_grip_at(&comments, p).is_some() {
                    mouse::Interaction::ResizingDiagonallyDown
                } else if self.comment_header_at(&comments, p).is_some() {
                    mouse::Interaction::Grab
                } else {
                    mouse::Interaction::default()
                }
            }
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        // Layers, bottom to top: grid and links, every node, then what is being dragged.
        let mut frame = Frame::new(renderer, bounds.size());
        let t = self.transform();
        let z = t.zoom;
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), style::CANVAS);
        draw_grid(&mut frame, &t, bounds.size());

        // Comments are the background of the nodes they frame.
        let visible_area = Rectangle::with_size(bounds.size());
        let comments = self.comment_layouts(state);
        let under_cursor = state.cursor.and_then(|p| comments.iter().rev().find(|c| c.rect.contains(p)).map(|c| c.id));
        for l in &comments {
            if !l.rect.intersects(&visible_area) {
                continue;
            }
            let Some(comment) = self.app.graph().comment(l.id) else { continue };
            self.draw_comment(&mut frame, comment, l, under_cursor == Some(l.id));
        }

        let moving = match &state.interaction {
            Interaction::Moving { ids, origin, current } => {
                Some((ids.as_slice(), Vector::new((current.x - origin.x) / z, (current.y - origin.y) / z)))
            }
            _ => None,
        };
        let layouts = self.layouts(moving);
        let app = self.app;
        let graph = app.graph();
        let sel = &app.ui.selection;

        // The link picked up from an input is drawn as the dragged one.
        let detached = match &state.interaction {
            Interaction::Linking { detached: Some(link), .. } => Some(link.clone()),
            _ => None,
        };
        // Links under the nodes.
        for link in &graph.links {
            if detached.as_ref() == Some(link) {
                continue;
            }
            let Some((a, b)) = layout::link_endpoints(&layouts, link) else { continue };
            let ty = graph.output_socket(link.from, &link.from_socket).map_or(DataType::Float, |s| s.ty);
            let mut color = if ty == DataType::Image { style::image_color(made_kind(app, link.from)) } else { style::type_color(ty) };
            let mut width = if ty == DataType::Image { 2.6 } else { 1.9 } * z.clamp(0.5, 1.5);
            if app.ui.selected_link.as_ref() == Some(link) {
                color = style::ACCENT;
                width += 1.5;
            } else if state.hovered_link.as_ref() == Some(link) {
                color = Color::WHITE;
                width += 1.0;
            } else if !sel.contains(&link.from) && !sel.contains(&link.to) {
                color = style::alpha(color, 0.72);
            }
            let p = layout::bezier(a, b, z);
            let path = Path::new(|builder| {
                builder.move_to(p[0]);
                builder.bezier_curve_to(p[1], p[2], p[3]);
            });
            frame.stroke(&path, Stroke::default().with_width(width).with_color(color));
        }

        // A link being dragged: which nodes it cannot end at, found once for every socket drawn.
        let linking = match &state.interaction {
            Interaction::Linking { from, ty, .. } => Some((from.clone(), *ty, self.blocked(from))),
            _ => None,
        };
        let mut geometries = vec![frame.into_geometry()];
        // Selected nodes on top.
        let order = layouts.iter().filter(|l| !sel.contains(&l.id)).chain(layouts.iter().filter(|l| sel.contains(&l.id)));
        for l in order {
            if !l.rect.intersects(&visible_area) {
                continue;
            }
            let Some(node) = graph.node(l.id) else { continue };
            let mut frame = Frame::new(renderer, bounds.size());
            self.draw_node(&mut frame, node, l, linking.as_ref(), detached.as_ref());
            geometries.push(frame.into_geometry());
        }
        let mut frame = Frame::new(renderer, bounds.size());

        // The link being dragged.
        if let (Interaction::Linking { from, ty, anchor, current, .. }, Some((.., blocked))) = (&state.interaction, &linking) {
            let (a, b) = if from.output { (*anchor, *current) } else { (*current, *anchor) };
            let target_ok = self.socket_at(&layouts, *current).is_some_and(|h| self.can_link(from, *ty, &h, blocked));
            let color = if target_ok { Color::WHITE } else { style::type_color(*ty) };
            let p = layout::bezier(a, b, z);
            let path = Path::new(|builder| {
                builder.move_to(p[0]);
                builder.bezier_curve_to(p[1], p[2], p[3]);
            });
            frame.stroke(&path, Stroke::default().with_width(2.4).with_color(color));
        }

        // Selection rectangle.
        if let Interaction::Boxing { start, current, .. } = &state.interaction {
            let r = Rectangle::new(
                Point::new(start.x.min(current.x), start.y.min(current.y)),
                Size::new((start.x - current.x).abs(), (start.y - current.y).abs()),
            );
            frame.fill_rectangle(r.position(), r.size(), style::alpha(style::ACCENT, 0.08));
            frame.stroke_rectangle(r.position(), r.size(), Stroke::default().with_width(1.0).with_color(style::ACCENT));
        }

        // Something dragged from a panel: what dropping it does, or why it cannot be dropped
        // here, said before it is.
        if let Some(payload) = &app.ui.drag
            && cursor.is_over(bounds)
        {
            let r = Rectangle::with_size(bounds.size()).shrink(2.0);
            let (color, says) = match app.drop_refusal(payload) {
                None => (style::ACCENT, format!("Drop to add {}", payload.label())),
                Some(why) => (style::ERROR, format!("Cannot be dropped here: {why}")),
            };
            frame.stroke_rectangle(r.position(), r.size(), Stroke::default().with_width(2.0).with_color(color));
            text_at(&mut frame, says, Point::new(bounds.width / 2.0, 22.0), 15.0, color, TextAlign::Center);
        }

        if graph.nodes.is_empty() {
            text_at(
                &mut frame,
                "Drag nodes from the library, double-click or press Space to add one".to_string(),
                Point::new(bounds.width / 2.0, bounds.height / 2.0),
                15.0,
                style::TEXT_FAINT,
                TextAlign::Center,
            );
        }
        text_at(
            &mut frame,
            format!("{:.0}%", z * 100.0),
            Point::new(bounds.width - 10.0, bounds.height - 12.0),
            12.0,
            style::TEXT_FAINT,
            TextAlign::Right,
        );
        geometries.push(frame.into_geometry());
        geometries
    }
}

fn text_at(frame: &mut Frame, content: String, position: Point, size: f32, color: Color, align: TextAlign) {
    frame.fill_text(Text {
        content,
        position,
        color,
        size: Pixels(size),
        font: style::ui_font(),
        align_x: align,
        align_y: Vertical::Center,
        line_height: LineHeight::default(),
        shaping: Shaping::Auto,
        max_width: f32::INFINITY,
    });
}

/// Font sizes are quantized so zooming does not re-shape text on every frame.
fn scaled(size: f32, zoom: f32) -> f32 {
    ((size * zoom) * 4.0).round() / 4.0
}

fn draw_grid(frame: &mut Frame, t: &Transform, size: Size) {
    let mut step = 32.0 * t.zoom;
    while step < 14.0 {
        step *= 4.0;
    }
    let (ox, oy) = (t.pan.x, t.pan.y);
    let first_x = ((0.0 - ox) / step).floor() as i64;
    let last_x = ((size.width - ox) / step).ceil() as i64;
    for i in first_x..=last_x {
        let x = ox + i as f32 * step;
        let c = if i % 4 == 0 { style::GRID_MAJOR } else { style::GRID_MINOR };
        frame.fill_rectangle(Point::new(x.floor(), 0.0), Size::new(1.0, size.height), c);
    }
    let first_y = ((0.0 - oy) / step).floor() as i64;
    let last_y = ((size.height - oy) / step).ceil() as i64;
    for i in first_y..=last_y {
        let y = oy + i as f32 * step;
        let c = if i % 4 == 0 { style::GRID_MAJOR } else { style::GRID_MINOR };
        frame.fill_rectangle(Point::new(0.0, y.floor()), Size::new(size.width, 1.0), c);
    }
}

pub fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() > max_chars {
        s.chars().take(max_chars.saturating_sub(1)).collect::<String>() + "…"
    } else {
        s.to_string()
    }
}

/// What the node is: the input value an Input node (an Image Input too) reads, the name of an
/// Output node, or the node type. This is what the graph shows in the header of a node, so
/// that a label never hides which node it is.
pub fn node_kind_title(node: &Node) -> String {
    if node.variable_type().is_some() {
        return node.variable.clone().unwrap_or_else(|| node.def().name.to_string());
    }
    match node.def().role {
        Role::Output => node.text("name").to_string(),
        // The graph it uses.
        Role::Instance => node.instance.as_ref().map_or_else(|| node.def().name.to_string(), |i| i.graph.name.clone()),
        _ => node.def().name.to_string(),
    }
}

/// Name of a node for the panels and the preview.
pub fn node_title(node: &Node) -> String {
    node_kind_title(node)
}

/// The top `h` pixels of a rectangle with rounded top corners (radius `r`): the band follows
/// the corner arcs and ends in a straight line.
fn top_band(rect: Rectangle, r: f32, h: f32) -> Path {
    let r = r.min(rect.width / 2.0).max(0.0);
    let h = h.min(rect.height);
    // Horizontal inset of the corner arc at depth `t` below the top.
    let inset = |t: f32| if t >= r { 0.0 } else { r - (r * r - (r - t) * (r - t)).max(0.0).sqrt() };
    const STEPS: usize = 6;
    Path::new(|b| {
        let (x0, x1, y) = (rect.x, rect.x + rect.width, rect.y);
        b.move_to(Point::new(x0 + inset(h), y + h));
        for i in (0..STEPS).rev() {
            let t = h * i as f32 / STEPS as f32;
            b.line_to(Point::new(x0 + inset(t), y + t));
        }
        for i in 0..=STEPS {
            let t = h * i as f32 / STEPS as f32;
            b.line_to(Point::new(x1 - inset(t), y + t));
        }
        b.close();
    })
}

fn diamond(center: Point, r: f32) -> Path {
    Path::new(|b| {
        b.move_to(Point::new(center.x, center.y - r));
        b.line_to(Point::new(center.x + r, center.y));
        b.line_to(Point::new(center.x, center.y + r));
        b.line_to(Point::new(center.x - r, center.y));
        b.close();
    })
}

impl GraphCanvas<'_> {
    /// A comment frame: a colored box behind the nodes, with its text in the title bar and
    /// a corner to resize it by.
    fn draw_comment(&self, frame: &mut Frame, comment: &Comment, l: &CommentLayout, hovered: bool) {
        let z = self.transform().zoom;
        let color = style::color_of(comment.color);
        let selected = self.app.ui.selected_comment == Some(comment.id);
        let radius = (6.0 * z).clamp(1.0, 10.0);
        let body = Path::rounded_rectangle(l.rect.position(), l.rect.size(), radius.into());
        frame.fill(&body, style::alpha(color, 0.13));
        // The title bar is the part that can be grabbed, so it is the solid one.
        let top = iced::border::Radius { top_left: radius, top_right: radius, bottom_right: 0.0, bottom_left: 0.0 };
        let header = Path::rounded_rectangle(l.header.position(), l.header.size(), top);
        frame.fill(&header, style::alpha(color, if hovered || selected { 0.5 } else { 0.34 }));
        let (width, outline) = if selected { (2.0, style::ACCENT) } else { (1.4, style::alpha(color, 0.8)) };
        frame.stroke(&body, Stroke::default().with_width(width).with_color(outline));

        let size = scaled(13.5, z);
        if size >= 5.0 {
            let content = if comment.text.is_empty() { "Comment".to_string() } else { comment.text.clone() };
            let max_chars = ((l.rect.width - 14.0 * z) / (7.2 * z)).max(3.0) as usize;
            let faint = comment.text.is_empty();
            let text_color = if faint { style::alpha(Color::WHITE, 0.5) } else { style::mix(color, Color::WHITE, 0.78) };
            frame.fill_text(Text {
                content: truncate(&content, max_chars),
                position: Point::new(l.header.x + 8.0 * z, l.header.center_y()),
                color: text_color,
                size: Pixels(size),
                font: style::bold(),
                align_x: TextAlign::Left,
                align_y: Vertical::Center,
                line_height: LineHeight::default(),
                shaping: Shaping::Auto,
                max_width: f32::INFINITY,
            });
        }
        // The corner: three lines, brighter when the mouse is on the comment.
        let grip = style::alpha(color, if hovered || selected { 0.95 } else { 0.6 });
        let (x1, y1) = (l.grip.x + l.grip.width, l.grip.y + l.grip.height);
        for i in 1..=3 {
            let d = l.grip.width * i as f32 / 3.5;
            let path = Path::new(|b| {
                b.move_to(Point::new(x1 - d, y1 - 2.0));
                b.line_to(Point::new(x1 - 2.0, y1 - d));
            });
            frame.stroke(&path, Stroke::default().with_width(1.6).with_color(grip));
        }
    }
    fn draw_node(
        &self,
        frame: &mut Frame,
        node: &Node,
        l: &NodeLayout,
        linking: Option<&(SocketRef, DataType, HashSet<NodeId>)>,
        detached: Option<&Link>,
    ) {
        let app = self.app;
        let graph = app.graph();
        let input_linked = |key: &str| graph.link_into(node.id, key).is_some_and(|link| Some(link) != detached);
        let z = self.transform().zoom;
        let d = node.def();
        let eval = app.eval.node(node.id);
        let report = app.res.reports.get(&node.id);
        let error = eval
            .and_then(|e| e.error().map(str::to_string))
            .or_else(|| report.and_then(|r| r.error.clone()))
            .or_else(|| app.res.inner.get(&node.id).cloned());
        let warning = eval
            .and_then(|e| e.warnings.first().cloned())
            .or_else(|| report.and_then(|r| r.warning.clone()));
        let selected = app.ui.selection.contains(&node.id);
        let radius = (6.0 * z).clamp(1.0, 12.0);

        // Shadow, body, header.
        let shadow = Path::rounded_rectangle(
            l.rect.position() + Vector::new(3.0 * z, 4.0 * z),
            l.rect.size(),
            radius.into(),
        );
        frame.fill(&shadow, Color::from_rgba(0.0, 0.0, 0.0, 0.28));
        let category = style::category_color(d.category);
        // Where the graph ends: an output node is tinted with its category all over, so the
        // result stands out among the nodes that make it.
        let is_output = matches!(d.role, Role::Output);
        let body = Path::rounded_rectangle(l.rect.position(), l.rect.size(), radius.into());
        let body_color = match (node.bypass, is_output) {
            (true, _) => style::NODE_BYPASSED,
            (false, true) => style::mix(style::NODE_BODY, category, 0.16),
            (false, false) => style::NODE_BODY,
        };
        frame.fill(&body, body_color);
        // A neutral header; the category shows as a strip along its top.
        let top = iced::border::Radius { top_left: radius, top_right: radius, bottom_right: 0.0, bottom_left: 0.0 };
        let header = Path::rounded_rectangle(l.header.position(), l.header.size(), top);
        let header_color = match (node.bypass, is_output) {
            (true, _) => style::NODE_BYPASSED,
            (false, true) => style::mix(style::NODE_HEADER, category, 0.5),
            (false, false) => style::NODE_HEADER,
        };
        frame.fill(&header, header_color);
        let strip_h = (6.0 * z).clamp(2.5, 8.0);
        let strip_color = if node.bypass { style::BORDER_LIGHT } else { category };
        frame.fill(&top_band(l.header, radius, strip_h), strip_color);
        // The title sits in what the strip leaves of the header.
        let cy = l.header.center_y() + strip_h * 0.5;

        let show_text = z > 0.32;
        let font_size = scaled(12.5, z);
        let small = scaled(11.0, z);
        if show_text {
            let max_chars = ((l.rect.width - 44.0 * z) / (7.0 * z)).max(3.0) as usize;
            let mut title = truncate(&node_kind_title(node), max_chars);
            if node.bypass {
                title = truncate(&format!("{title} (bypassed)"), max_chars);
            }
            frame.fill_text(Text {
                content: title,
                position: Point::new(l.header.x + 8.0 * z, cy),
                color: Color::WHITE,
                size: Pixels(font_size),
                font: style::bold(),
                align_x: TextAlign::Left,
                align_y: Vertical::Center,
                line_height: LineHeight::default(),
                shaping: Shaping::Auto,
                max_width: f32::INFINITY,
            });
        }

        // Header badges: error / warning, preview marker, output format.
        let mut badge_x = l.header.x + l.header.width - 10.0 * z;
        if error.is_some() || warning.is_some() {
            let c = if error.is_some() { style::ERROR } else { style::WARNING };
            frame.fill(&Path::circle(Point::new(badge_x, cy), 6.0 * z), c);
            if show_text {
                text_at(frame, "!".into(), Point::new(badge_x, cy), small, Color::BLACK, TextAlign::Center);
            }
            badge_x -= 16.0 * z;
        }
        if app.ui.preview.node == Some(node.id) {
            let center = Point::new(badge_x, cy);
            frame.stroke(&Path::circle(center, 5.0 * z), Stroke::default().with_width(1.5 * z).with_color(Color::WHITE));
            frame.fill(&Path::circle(center, 2.2 * z), Color::WHITE);
            badge_x -= 16.0 * z;
        }
        if node.kind == NodeKind::OUTPUT && show_text {
            let format = OUTPUT_FORMATS.get(node.enum_value("format") as usize).copied().unwrap_or("");
            text_at(frame, format.to_string(), Point::new(badge_x + 4.0 * z, cy), small, style::NODE_TEXT, TextAlign::Right);
        }

        // Socket labels.
        if z > 0.48 {
            for (s, p) in &l.inputs {
                let linked = input_linked(&s.key);
                let color = if linked && s.kind == brewed_texture_core::SocketKind::Param {
                    style::type_color(s.ty)
                } else {
                    style::NODE_TEXT
                };
                text_at(frame, s.label.clone(), *p + Vector::new(10.0 * z, 0.0), small, color, TextAlign::Left);
            }
            for (s, p) in &l.outputs {
                text_at(frame, s.label.clone(), *p - Vector::new(10.0 * z, 0.0), small, style::NODE_TEXT, TextAlign::Right);
            }
        }

        // Body: expression, thumbnail, value.
        let text_x = l.rect.x + 8.0 * z;
        let line = |i: f32| l.body_top + (i + 0.5) * LINE_H * z;
        if node.is_expression_node() && show_text {
            let max_chars = ((l.rect.width - 16.0 * z) / (6.6 * z)).max(3.0) as usize;
            let y = if d.has_image() { l.body_top - 0.5 * LINE_H * z } else { line(0.0) };
            frame.fill_text(Text {
                content: truncate(node.expression(), max_chars),
                position: Point::new(text_x, y),
                color: style::EXPRESSION,
                size: Pixels(small),
                font: style::mono(),
                align_x: TextAlign::Left,
                align_y: Vertical::Center,
                line_height: LineHeight::default(),
                shaping: Shaping::Auto,
                max_width: f32::INFINITY,
            });
        }
        if let Some(thumb) = l.thumb {
            frame.fill_rectangle(thumb.position(), thumb.size(), style::THUMB_BACKGROUND);
            if let Some(handle) = app.res.thumbs.get(&node.id) {
                frame.draw_image(thumb, canvas::Image::new(handle.clone()));
            }
            frame.stroke_rectangle(thumb.position(), thumb.size(), Stroke::default().with_width(1.0).with_color(style::BORDER));
        } else if show_text {
            let row = if node.is_expression_node() { 1.0 } else { 0.0 };
            let value = eval.and_then(|e| e.value);
            match (&error, value) {
                (Some(e), _) => {
                    let msg = if node.kind == NodeKind::MATH { "Invalid expression".to_string() } else { truncate(e, 26) };
                    text_at(frame, msg, Point::new(text_x, line(row)), small, style::ERROR, TextAlign::Left);
                }
                (None, Some(Value::Color(c))) => {
                    let sw = Rectangle::new(Point::new(text_x, line(row) - 6.0 * z), Size::new(28.0 * z, 12.0 * z));
                    frame.fill_rectangle(sw.position(), sw.size(), style::color_of(c));
                    let label = Value::Color(c).display();
                    text_at(frame, label, Point::new(sw.x + sw.width + 6.0 * z, line(row)), small, style::NODE_TEXT, TextAlign::Left);
                }
                (None, Some(v)) => {
                    text_at(frame, format!("= {}", v.display()), Point::new(text_x, line(row)), small, style::NODE_TEXT, TextAlign::Left);
                }
                (None, None) => {}
            }
        }

        // Outline.
        let (w, c) = if selected {
            (2.0, style::ACCENT)
        } else if error.is_some() {
            (1.5, style::ERROR)
        } else if is_output {
            (1.8, category)
        } else {
            (1.0, style::NODE_OUTLINE)
        };
        frame.stroke(&body, Stroke::default().with_width(w).with_color(c));

        // Sockets.
        let r = (SOCKET_R * z).max(2.5);
        for (list, output) in [(&l.inputs, false), (&l.outputs, true)] {
            for (s, p) in list {
                let linked = if output {
                    graph.links.iter().any(|lk| lk.from == node.id && lk.from_socket == s.key)
                } else {
                    input_linked(&s.key)
                };
                let mut color = if s.ty != DataType::Image {
                    style::type_color(s.ty)
                } else if output {
                    style::image_color(made_kind(app, node.id))
                } else {
                    // What the input takes, or what arrives at an input that takes either.
                    let arriving = graph.link_into(node.id, &s.key).and_then(|lk| made_kind(app, lk.from));
                    style::image_color(match s.slot {
                        Slot::Gray => Some(ImageKind::Gray),
                        Slot::Color => Some(ImageKind::Color),
                        Slot::Any => arriving,
                    })
                };
                let mut radius = r;
                if let Some((from, ty, blocked)) = linking {
                    let hit = Hit { node: node.id, key: s.key.clone(), output, ty: s.ty, pos: *p };
                    let is_origin = from.node == node.id && from.key == s.key && from.output == output;
                    if self.can_link(from, *ty, &hit, blocked) {
                        radius = r * 1.35;
                    } else if !is_origin {
                        color = style::alpha(color, 0.3);
                    }
                }
                let fill = if linked { color } else { style::SOCKET_EMPTY };
                let path = if s.ty == DataType::Image { Path::circle(*p, radius) } else { diamond(*p, radius * 1.2) };
                frame.fill(&path, fill);
                frame.stroke(&path, Stroke::default().with_width(1.6 * z.max(0.6)).with_color(color));
            }
        }
    }
}

/// The kind of image `node` makes as far as it is known: what the engine last made, or else
/// what the node always makes.
pub fn made_kind(app: &App, node: NodeId) -> Option<ImageKind> {
    app.res.reports.get(&node).and_then(|r| r.kind).or_else(|| {
        match app.graph().node(node)?.def().outputs.first()?.slot {
            Slot::Gray => Some(ImageKind::Gray),
            Slot::Color => Some(ImageKind::Color),
            Slot::Any => None,
        }
    })
}

/// The size a comment gets while its corner is dragged from `origin` to `current`, in
/// graph units.
fn resized(from: [f32; 2], origin: Point, current: Point, zoom: f32) -> [f32; 2] {
    let d = current - origin;
    [
        (from[0] + d.x / zoom).max(Comment::MIN_SIZE[0]),
        (from[1] + d.y / zoom).max(Comment::MIN_SIZE[1]),
    ]
}
