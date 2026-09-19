//! The infinite node canvas.
//!
//! Interaction state (what is being dragged) lives in the canvas widget; everything that
//! changes the document is published as a [`GraphMessage`]. Nodes being moved are drawn
//! at their dragged position and committed as one edit on release.

pub mod layout;

use iced::alignment::Vertical;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::widget::text::{Alignment as TextAlign, LineHeight, Shaping};
use iced::advanced::mouse;
use iced::{Color, Element, Length, Pixels, Point, Rectangle, Size, Vector, keyboard};
use tessera_core::graph::{Link, Node, NodeId, SocketRef};
use tessera_core::nodes::{OUTPUT_FORMATS, Role};
use tessera_core::{DataType, NodeKind, Value, can_connect};

use crate::app::{App, ContextTarget, GraphMessage, Message, SelectMode};
use crate::style;
use layout::{LINE_H, NodeLayout, SOCKET_R, Transform};

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
    },
    PressedCanvas {
        at: Point,
    },
    Boxing {
        start: Point,
        current: Point,
        additive: bool,
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
    iced::widget::canvas(GraphCanvas { app }).width(Length::Fill).height(Length::Fill).into()
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

    fn can_link(&self, from: &SocketRef, ty: DataType, to: &Hit) -> bool {
        if from.output == to.output || from.node == to.node {
            return false;
        }
        let (out_ty, in_ty, out_node, in_node) =
            if from.output { (ty, to.ty, from.node, to.node) } else { (to.ty, ty, to.node, from.node) };
        can_connect(out_ty, in_ty) && !self.app.graph().depends_on(out_node, in_node)
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
                    Interaction::Panning { last } => {
                        let p = p?;
                        let delta = p - *last;
                        *last = p;
                        let pan = t.pan + delta;
                        return publish(GraphMessage::View { pan, zoom: t.zoom });
                    }
                    Interaction::PressedRight { at } => {
                        let p = p?;
                        if p.distance(*at) > 4.0 {
                            let delta = p - *at;
                            state.interaction = Interaction::Panning { last: p };
                            return publish(GraphMessage::View { pan: t.pan + delta, zoom: t.zoom });
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
                let dy = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y * 60.0,
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                let factor = (dy * 0.0018).exp();
                let zoom = (t.zoom * factor).clamp(0.12, 3.0);
                let g = t.to_graph(p);
                let pan = Vector::new(p.x - g[0] * zoom, p.y - g[1] * zoom);
                publish(GraphMessage::View { pan, zoom })
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
                                state.interaction = Interaction::Linking { from, ty: s.ty, anchor: *pos, current: p };
                                return publish(GraphMessage::Detach { node: hit.node, socket: hit.key });
                            }
                            let from = SocketRef { node: hit.node, key: hit.key.clone(), output: hit.output };
                            state.interaction = Interaction::Linking { from, ty: hit.ty, anchor: hit.pos, current: p };
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
                        let target = if let Some(node) = self.node_at(&layouts, at) {
                            ContextTarget::Node(node)
                        } else if let Some(link) = self.link_at(&layouts, at) {
                            ContextTarget::Link(link)
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
                    (mouse::Button::Left, Interaction::Linking { from, ty, current, .. }) => {
                        let layouts = self.layouts(None);
                        match self.socket_at(&layouts, current) {
                            Some(target) if self.can_link(&from, ty, &target) => {
                                let to = SocketRef { node: target.node, key: target.key, output: target.output };
                                publish(GraphMessage::Connect(from, to))
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
            Interaction::Panning { .. } | Interaction::Moving { .. } => mouse::Interaction::Grabbing,
            Interaction::Linking { .. } => mouse::Interaction::Crosshair,
            _ => {
                let Some(p) = cursor.position_in(bounds) else { return mouse::Interaction::default() };
                if self.app.ui.drag.is_some() {
                    return mouse::Interaction::Copy;
                }
                let layouts = self.layouts(None);
                if self.socket_at(&layouts, p).is_some() || state.hovered_link.is_some() {
                    mouse::Interaction::Pointer
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
        let mut frame = Frame::new(renderer, bounds.size());
        let t = self.transform();
        let z = t.zoom;
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), style::CANVAS);
        draw_grid(&mut frame, &t, bounds.size());

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

        // Links under the nodes.
        for link in &graph.links {
            let Some((a, b)) = layout::link_endpoints(&layouts, link) else { continue };
            let ty = graph.output_socket(link.from, &link.from_socket).map_or(DataType::Float, |s| s.ty);
            let mut color = style::type_color(ty);
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

        let linking = match &state.interaction {
            Interaction::Linking { from, ty, .. } => Some((from.clone(), *ty)),
            _ => None,
        };
        let visible = Rectangle::with_size(bounds.size());
        for l in &layouts {
            if !l.rect.intersects(&visible) {
                continue;
            }
            let Some(node) = graph.node(l.id) else { continue };
            self.draw_node(&mut frame, node, l, linking.as_ref());
        }

        // The link being dragged.
        if let Interaction::Linking { from, ty, anchor, current } = &state.interaction {
            let (a, b) = if from.output { (*anchor, *current) } else { (*current, *anchor) };
            let target_ok = self.socket_at(&layouts, *current).is_some_and(|h| self.can_link(from, *ty, &h));
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

        // Something dragged from a panel.
        if let Some(payload) = &app.ui.drag
            && cursor.is_over(bounds)
        {
            let r = Rectangle::with_size(bounds.size()).shrink(2.0);
            frame.stroke_rectangle(r.position(), r.size(), Stroke::default().with_width(2.0).with_color(style::ACCENT));
            text_at(&mut frame, format!("Drop to add {}", payload.label()), Point::new(bounds.width / 2.0, 22.0), 15.0, style::ACCENT, TextAlign::Center);
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
        vec![frame.into_geometry()]
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

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() > max_chars {
        s.chars().take(max_chars.saturating_sub(1)).collect::<String>() + "…"
    } else {
        s.to_string()
    }
}

/// Title shown on a node: its label, the variable of an Input node, the name of an
/// Output node, or the node type.
pub fn node_title(node: &Node) -> String {
    if !node.label.is_empty() {
        return node.label.clone();
    }
    match node.def().role {
        Role::Variable(_) => node.variable.clone().unwrap_or_else(|| node.def().name.to_string()),
        Role::Output => node.text("name").to_string(),
        _ => node.def().name.to_string(),
    }
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
    fn draw_node(&self, frame: &mut Frame, node: &Node, l: &NodeLayout, linking: Option<&(SocketRef, DataType)>) {
        let app = self.app;
        let graph = app.graph();
        let z = self.transform().zoom;
        let d = node.def();
        let eval = app.eval.node(node.id);
        let report = app.res.reports.get(&node.id);
        let error = eval.and_then(|e| e.error().map(str::to_string)).or_else(|| report.and_then(|r| r.error.clone()));
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
        let body = Path::rounded_rectangle(l.rect.position(), l.rect.size(), radius.into());
        frame.fill(&body, if node.bypass { style::rgb(38, 39, 43) } else { style::NODE_BODY });
        let header_color = if node.bypass { style::rgb(70, 70, 74) } else { style::category_color(d.category) };
        let header = Path::rounded_rectangle(
            l.header.position(),
            l.header.size(),
            iced::border::Radius { top_left: radius, top_right: radius, bottom_right: 0.0, bottom_left: 0.0 },
        );
        frame.fill(&header, header_color);

        let show_text = z > 0.32;
        let font_size = scaled(12.5, z);
        let small = scaled(11.0, z);
        if show_text {
            let max_chars = ((l.rect.width - 44.0 * z) / (7.0 * z)).max(3.0) as usize;
            let mut title = truncate(&node_title(node), max_chars);
            if node.bypass {
                title = truncate(&format!("{title} (bypassed)"), max_chars);
            }
            frame.fill_text(Text {
                content: title,
                position: Point::new(l.header.x + 8.0 * z, l.header.center_y()),
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
        let cy = l.header.center_y();
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
            text_at(frame, format.to_string(), Point::new(badge_x + 4.0 * z, cy), small, style::rgb(230, 230, 230), TextAlign::Right);
        }

        // Socket labels.
        if z > 0.48 {
            for (s, p) in &l.inputs {
                let linked = graph.link_into(node.id, &s.key).is_some();
                let color = if linked && s.kind == tessera_core::SocketKind::Param {
                    style::type_color(s.ty)
                } else {
                    style::rgb(200, 200, 204)
                };
                text_at(frame, s.label.clone(), *p + Vector::new(10.0 * z, 0.0), small, color, TextAlign::Left);
            }
            for (s, p) in &l.outputs {
                text_at(frame, s.label.clone(), *p - Vector::new(10.0 * z, 0.0), small, style::rgb(200, 200, 204), TextAlign::Right);
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
            frame.fill_rectangle(thumb.position(), thumb.size(), style::rgb(20, 20, 22));
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
                    text_at(frame, label, Point::new(sw.x + sw.width + 6.0 * z, line(row)), small, style::rgb(220, 220, 224), TextAlign::Left);
                }
                (None, Some(v)) => {
                    text_at(frame, format!("= {}", v.display()), Point::new(text_x, line(row)), small, style::rgb(230, 230, 234), TextAlign::Left);
                }
                (None, None) => {}
            }
        }

        // Outline.
        let (w, c) = if selected {
            (2.0, style::ACCENT)
        } else if error.is_some() {
            (1.5, style::ERROR)
        } else {
            (1.0, style::rgb(18, 18, 20))
        };
        frame.stroke(&body, Stroke::default().with_width(w).with_color(c));

        // Sockets.
        let r = (SOCKET_R * z).max(2.5);
        for (list, output) in [(&l.inputs, false), (&l.outputs, true)] {
            for (s, p) in list {
                let linked = if output {
                    graph.links.iter().any(|lk| lk.from == node.id && lk.from_socket == s.key)
                } else {
                    graph.link_into(node.id, &s.key).is_some()
                };
                let mut color = style::type_color(s.ty);
                let mut radius = r;
                if let Some((from, ty)) = linking {
                    let hit = Hit { node: node.id, key: s.key.clone(), output, ty: s.ty, pos: *p };
                    let is_origin = from.node == node.id && from.key == s.key && from.output == output;
                    if self.can_link(from, *ty, &hit) {
                        radius = r * 1.35;
                    } else if !is_origin {
                        color = style::alpha(color, 0.3);
                    }
                }
                let fill = if linked { color } else { style::rgb(30, 31, 34) };
                let path = if s.ty == DataType::Image { Path::circle(*p, radius) } else { diamond(*p, radius * 1.2) };
                frame.fill(&path, fill);
                frame.stroke(&path, Stroke::default().with_width(1.6 * z.max(0.6)).with_color(color));
            }
        }
    }
}
