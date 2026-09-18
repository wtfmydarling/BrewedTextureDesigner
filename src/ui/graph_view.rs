//! The infinite node canvas.
//!
//! Each frame runs in two passes: first every node and socket registers its
//! interaction rectangle and the resulting actions (select, move, connect ...) are
//! applied to the graph; then the updated graph is laid out again and painted, so
//! dragged nodes never lag behind the pointer.

use std::collections::BTreeSet;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, Event, FontId, Id, Key, Order, PointerButton, Pos2, Rect, Sense, Stroke,
    StrokeKind, Ui, Vec2, epaint::CubicBezierShape, pos2, vec2,
};

use super::{ACCENT, DragPayload, ERROR, WARNING, category_color, create_from_library, type_color};
use crate::engine::Engine;
use crate::graph::defs::{LibraryItem, library};
use crate::graph::{
    Clipboard, DataType, Graph, Link, Node, NodeId, NodeKind, SocketInfo, SocketKind, can_connect,
};

const NODE_W: f32 = 168.0;
const WIDE_NODE_W: f32 = 196.0;
const HEADER_H: f32 = 24.0;
const ROW_H: f32 = 20.0;
const THUMB: f32 = 136.0;
const LINE_H: f32 = 18.0;
const SOCKET_R: f32 = 5.5;

#[derive(Clone, Debug, PartialEq)]
pub struct SocketRef {
    pub node: NodeId,
    pub key: String,
    pub output: bool,
    pub ty: DataType,
}

#[derive(Clone, Debug, PartialEq)]
enum Drag {
    None,
    Pan,
    Select { start: Pos2, additive: bool },
    Move,
    Link { from: SocketRef },
}

pub struct AddMenu {
    screen_pos: Pos2,
    graph_pos: Pos2,
    search: String,
    pending: Option<SocketRef>,
    just_opened: bool,
}

#[derive(Clone, Debug)]
pub struct DebugNode {
    pub id: NodeId,
    pub rect: Rect,
    pub header: Rect,
    /// (socket key, is output, screen position)
    pub sockets: Vec<(String, bool, Pos2)>,
}

#[derive(Default)]
pub struct GraphViewOutput {
    pub preview: Option<NodeId>,
    pub message: Option<(String, bool)>,
}

struct Layout {
    rect: Rect,
    header: Rect,
    inputs: Vec<(SocketInfo, Pos2)>,
    outputs: Vec<(SocketInfo, Pos2)>,
    thumb: Option<Rect>,
    text_top: f32,
}

enum Action {
    Click { node: NodeId, toggle: bool, add: bool },
    DragStart { node: NodeId, add: bool },
    Move(Vec2),
    Preview(NodeId),
    StartLink(SocketRef),
    Detach { node: NodeId, key: String },
    Delete(Vec<NodeId>),
    Duplicate(Vec<NodeId>),
    Copy(Vec<NodeId>),
    DisconnectInputs(NodeId),
}

pub struct GraphView {
    pub pan: Vec2,
    pub zoom: f32,
    pub selected: BTreeSet<NodeId>,
    drag: Drag,
    pub selected_link: Option<Link>,
    hovered_link: Option<Link>,
    context_link: Option<Link>,
    context_pos: Pos2,
    add_menu: Option<AddMenu>,
    frame_request: bool,
    canvas: Rect,
    /// Screen geometry of the nodes drawn last frame (used by the UI self-test).
    pub debug_nodes: Vec<DebugNode>,
}

impl Default for GraphView {
    fn default() -> Self {
        GraphView {
            pan: Vec2::ZERO,
            zoom: 1.0,
            selected: BTreeSet::new(),
            drag: Drag::None,
            selected_link: None,
            hovered_link: None,
            context_link: None,
            context_pos: Pos2::ZERO,
            add_menu: None,
            frame_request: true,
            canvas: Rect::NOTHING,
            debug_nodes: Vec::new(),
        }
    }
}

fn node_width(node: &Node) -> f32 {
    if node.is_expression_node() { WIDE_NODE_W } else { NODE_W }
}

/// Size of a node in graph units.
pub fn node_size(graph: &Graph, node: &Node) -> Vec2 {
    let rows = graph.input_sockets(node).len().max(graph.output_sockets(node).len()).max(1) as f32;
    let mut body = 0.0;
    if node.kind == NodeKind::PixelMath {
        body += LINE_H;
    }
    if node.def().has_image {
        body += THUMB + 8.0;
    } else {
        body += match node.kind {
            NodeKind::Math => 2.0 * LINE_H,
            _ => LINE_H,
        };
    }
    vec2(node_width(node), HEADER_H + 4.0 + rows * ROW_H + body + 6.0)
}

pub fn node_title(graph: &Graph, node: &Node) -> String {
    if !node.label.is_empty() {
        return node.label.clone();
    }
    match node.kind {
        NodeKind::Input => match node.input_ref.and_then(|i| graph.input(i)) {
            Some(input) => input.name.clone(),
            None => "Input (missing)".into(),
        },
        NodeKind::Output => node.text("name").to_string(),
        _ => node.def().name.to_string(),
    }
}

fn bezier_points(a: Pos2, b: Pos2, zoom: f32) -> [Pos2; 4] {
    let dx = ((b.x - a.x).abs() * 0.5).max(40.0 * zoom);
    [a, a + vec2(dx, 0.0), b - vec2(dx, 0.0), b]
}

fn bezier_distance(points: &[Pos2; 4], p: Pos2) -> f32 {
    let shape = CubicBezierShape::from_points_stroke(*points, false, Color32::TRANSPARENT, Stroke::NONE);
    let mut best = f32::INFINITY;
    let mut prev = points[0];
    for i in 1..=24 {
        let q = shape.sample(i as f32 / 24.0);
        let seg = q - prev;
        let t = if seg.length_sq() > 0.0 { ((p - prev).dot(seg) / seg.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
        best = best.min((prev + seg * t).distance(p));
        prev = q;
    }
    best
}

impl GraphView {
    fn to_screen(&self, p: Pos2) -> Pos2 {
        self.canvas.min + self.pan + p.to_vec2() * self.zoom
    }

    fn to_graph(&self, p: Pos2) -> Pos2 {
        ((p - self.canvas.min - self.pan) / self.zoom).to_pos2()
    }

    /// Graph position at the center of the visible canvas.
    pub fn center_graph_pos(&self) -> [f32; 2] {
        let c = self.to_graph(self.canvas.center());
        [c.x - NODE_W * 0.5, c.y - 60.0]
    }

    pub fn canvas_rect(&self) -> Rect {
        self.canvas
    }

    pub fn request_frame(&mut self) {
        self.frame_request = true;
    }

    fn layout(&self, graph: &Graph, node: &Node) -> Layout {
        let size = node_size(graph, node);
        let min = self.to_screen(pos2(node.pos[0], node.pos[1]));
        let rect = Rect::from_min_size(min, size * self.zoom);
        let header = Rect::from_min_size(min, vec2(rect.width(), HEADER_H * self.zoom));
        let row_y = |i: usize| min.y + (HEADER_H + 2.0 + (i as f32 + 0.5) * ROW_H) * self.zoom;
        let inputs = graph
            .input_sockets(node)
            .into_iter()
            .enumerate()
            .map(|(i, s)| (s, pos2(rect.left(), row_y(i))))
            .collect::<Vec<_>>();
        let outputs = graph
            .output_sockets(node)
            .into_iter()
            .enumerate()
            .map(|(i, s)| (s, pos2(rect.right(), row_y(i))))
            .collect::<Vec<_>>();
        let rows = inputs.len().max(outputs.len()).max(1) as f32;
        let mut text_top = min.y + (HEADER_H + 4.0 + rows * ROW_H) * self.zoom;
        let mut thumb = None;
        if node.def().has_image {
            if node.kind == NodeKind::PixelMath {
                text_top += LINE_H * self.zoom;
            }
            let t = THUMB * self.zoom;
            thumb = Some(Rect::from_min_size(pos2(rect.center().x - t * 0.5, text_top + 2.0 * self.zoom), vec2(t, t)));
        }
        Layout { rect, header, inputs, outputs, thumb, text_top }
    }

    fn frame_nodes(&mut self, graph: &Graph, only_selected: bool) {
        let mut bounds = Rect::NOTHING;
        for n in &graph.nodes {
            if only_selected && !self.selected.contains(&n.id) {
                continue;
            }
            bounds = bounds.union(Rect::from_min_size(pos2(n.pos[0], n.pos[1]), node_size(graph, n)));
        }
        if !bounds.is_positive() || !self.canvas.is_positive() {
            self.zoom = 1.0;
            self.pan = self.canvas.size() * 0.5;
            return;
        }
        let margin = 50.0;
        let avail = (self.canvas.size() - Vec2::splat(2.0 * margin)).max(Vec2::splat(50.0));
        self.zoom = (avail.x / bounds.width()).min(avail.y / bounds.height()).clamp(0.2, 1.25);
        self.pan = self.canvas.size() * 0.5 - bounds.center().to_vec2() * self.zoom;
    }

    fn socket_at(&self, layouts: &[(NodeId, Layout)], p: Pos2) -> Option<SocketRef> {
        let radius = (SOCKET_R * self.zoom).max(4.0) + 6.0;
        let mut best: Option<(f32, SocketRef)> = None;
        for (id, l) in layouts {
            for (list, output) in [(&l.inputs, false), (&l.outputs, true)] {
                for (s, pos) in list.iter() {
                    let d = pos.distance(p);
                    if d <= radius && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                        best = Some((d, SocketRef { node: *id, key: s.key.clone(), output, ty: s.ty }));
                    }
                }
            }
        }
        best.map(|(_, s)| s)
    }

    fn can_link(graph: &Graph, a: &SocketRef, b: &SocketRef) -> bool {
        if a.output == b.output || a.node == b.node {
            return false;
        }
        let (out, inp) = if a.output { (a, b) } else { (b, a) };
        can_connect(out.ty, inp.ty) && !graph.depends_on(out.node, inp.node)
    }

    fn connect(graph: &mut Graph, a: &SocketRef, b: &SocketRef) -> Result<(), String> {
        let (out, inp) = if a.output { (a, b) } else { (b, a) };
        graph.connect(out.node, &out.key, inp.node, &inp.key)
    }

    pub fn delete_selection(&mut self, graph: &mut Graph) {
        if let Some(link) = self.selected_link.take() {
            graph.links.retain(|l| *l != link);
        }
        let ids: Vec<NodeId> = self.selected.iter().copied().collect();
        graph.remove_nodes(&ids);
        self.selected.clear();
    }

    pub fn copy_selection(&self, graph: &Graph, ctx: &egui::Context, clipboard: &mut Option<Clipboard>) {
        let ids: Vec<NodeId> = self.selected.iter().copied().collect();
        if ids.is_empty() {
            return;
        }
        let clip = graph.copy_nodes(&ids);
        if let Ok(json) = serde_json::to_string(&clip) {
            ctx.copy_text(format!("{}{json}", Clipboard::MAGIC));
        }
        *clipboard = Some(clip);
    }

    pub fn paste(&mut self, graph: &mut Graph, clip: &Clipboard, at: Pos2) {
        let mut min = pos2(f32::INFINITY, f32::INFINITY);
        for n in &clip.nodes {
            min = min.min(pos2(n.pos[0], n.pos[1]));
        }
        if !min.x.is_finite() {
            return;
        }
        let new = graph.paste(clip, [at.x - min.x, at.y - min.y]);
        self.selected = new.into_iter().collect();
    }

    fn duplicate(&mut self, graph: &mut Graph, ids: &[NodeId]) {
        let clip = graph.copy_nodes(ids);
        let new = graph.paste(&clip, [30.0, 30.0]);
        self.selected = new.into_iter().collect();
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        graph: &mut Graph,
        engine: &Engine,
        preview_node: Option<NodeId>,
        clipboard: &mut Option<Clipboard>,
    ) -> GraphViewOutput {
        let mut out = GraphViewOutput::default();
        let (rect, canvas) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.canvas = rect;
        let ctx = ui.ctx().clone();
        if self.frame_request && rect.is_positive() {
            self.frame_nodes(graph, false);
            self.frame_request = false;
        }
        let pointer = ctx.pointer_hover_pos();
        let hovered = pointer.is_some_and(|p| rect.contains(p));
        let base_id = ui.id().with("ferrotex_graph");

        // ---------------------------------------------------------------- zoom
        if hovered && self.add_menu.is_none() {
            let (mut scroll, pinch) = ctx.input(|i| {
                let mut s = 0.0;
                for e in &i.events {
                    if let Event::MouseWheel { unit, delta, .. } = e {
                        s += match unit {
                            egui::MouseWheelUnit::Point => delta.y,
                            egui::MouseWheelUnit::Line => delta.y * 40.0,
                            egui::MouseWheelUnit::Page => delta.y * 400.0,
                        };
                    }
                }
                (s, i.zoom_delta())
            });
            if pinch != 1.0 {
                scroll = 0.0;
            }
            let factor = (scroll * 0.0018).exp() * pinch;
            if (factor - 1.0).abs() > 1e-4 {
                if let Some(p) = pointer {
                    let g = self.to_graph(p);
                    self.zoom = (self.zoom * factor).clamp(0.12, 3.0);
                    self.pan = p - rect.min - g.to_vec2() * self.zoom;
                }
            }
        }

        // ---------------------------------------------------------------- pan
        let (middle, secondary, primary, delta, alt) = ctx.input(|i| {
            (i.pointer.middle_down(), i.pointer.secondary_down(), i.pointer.primary_down(), i.pointer.delta(), i.modifiers.alt)
        });
        let pan_button = middle || secondary || (primary && alt && self.drag == Drag::Pan);
        if self.drag == Drag::Pan {
            if pan_button || (primary && alt) {
                self.pan += delta;
            } else {
                self.drag = Drag::None;
            }
        } else if hovered && self.drag == Drag::None && (middle || secondary) && delta.length_sq() > 0.0 {
            self.drag = Drag::Pan;
            self.pan += delta;
        }

        // ---------------------------------------------------------------- pass 1: interaction
        let layouts: Vec<(NodeId, Layout)> = graph.nodes.iter().map(|n| (n.id, self.layout(graph, n))).collect();
        let mut actions: Vec<Action> = Vec::new();
        let (ctrl, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
        let mut any_node_hovered = false;
        for (node_id, l) in &layouts {
            let clipped = l.rect.intersect(rect);
            if !clipped.is_positive() {
                continue;
            }
            let resp = ui.interact(clipped, base_id.with(("node", node_id.0)), Sense::click_and_drag());
            any_node_hovered |= resp.hovered();
            if resp.clicked() {
                actions.push(Action::Click { node: *node_id, toggle: ctrl, add: shift });
            }
            if resp.double_clicked() || resp.triple_clicked() {
                actions.push(Action::Preview(*node_id));
            }
            if resp.drag_started_by(PointerButton::Primary) && !alt {
                actions.push(Action::DragStart { node: *node_id, add: shift || ctrl });
            }
            let starts_move = resp.drag_started_by(PointerButton::Primary) && !alt;
            if (self.drag == Drag::Move || starts_move) && resp.dragged_by(PointerButton::Primary) {
                actions.push(Action::Move(resp.drag_delta() / self.zoom));
            }
            if let Some(state) = engine.state(*node_id) {
                if let Some(msg) = state.error.as_ref().or(state.warning.as_ref()) {
                    let _ = resp.clone().on_hover_text(msg.clone());
                }
            }
            let node_id = *node_id;
            resp.context_menu(|ui| {
                if !self.selected.contains(&node_id) {
                    self.selected.clear();
                    self.selected.insert(node_id);
                }
                let ids: Vec<NodeId> = self.selected.iter().copied().collect();
                if ui.button("Show in 2D view").clicked() {
                    actions.push(Action::Preview(node_id));
                    ui.close();
                }
                ui.separator();
                if ui.button("Duplicate    Ctrl+D").clicked() {
                    actions.push(Action::Duplicate(ids.clone()));
                    ui.close();
                }
                if ui.button("Copy         Ctrl+C").clicked() {
                    actions.push(Action::Copy(ids.clone()));
                    ui.close();
                }
                if ui.button("Disconnect inputs").clicked() {
                    actions.push(Action::DisconnectInputs(node_id));
                    ui.close();
                }
                ui.separator();
                if ui.button("Delete       Del").clicked() {
                    actions.push(Action::Delete(ids));
                    ui.close();
                }
            });

            // Sockets are registered after the body so they win the hit test.
            let hit = vec2(SOCKET_R * self.zoom + 5.0, (ROW_H * self.zoom * 0.5).max(6.0));
            for (list, output) in [(&l.inputs, false), (&l.outputs, true)] {
                for (s, pos) in list.iter() {
                    let r = Rect::from_center_size(*pos, hit * 2.0);
                    let sid = base_id.with(("socket", node_id.0, s.key.as_str(), output));
                    let sresp = ui.interact(r.intersect(rect), sid, Sense::drag());
                    if sresp.hovered() {
                        let _ = sresp.clone().on_hover_text(format!("{} ({})", s.label, s.ty.name()));
                    }
                    if sresp.drag_started_by(PointerButton::Primary) {
                        let me = SocketRef { node: node_id, key: s.key.clone(), output, ty: s.ty };
                        if !output {
                            if let Some(link) = graph.link_into(node_id, &s.key).cloned() {
                                // Pick the link up from its input end and re-route it.
                                actions.push(Action::Detach { node: node_id, key: s.key.clone() });
                                let ty = graph.output_socket(link.from, &link.from_socket).map_or(s.ty, |o| o.ty);
                                actions.push(Action::StartLink(SocketRef {
                                    node: link.from,
                                    key: link.from_socket.clone(),
                                    output: true,
                                    ty,
                                }));
                                continue;
                            }
                        }
                        actions.push(Action::StartLink(me));
                    }
                }
            }
        }

        // Hovered link (only over empty canvas).
        self.hovered_link = None;
        if hovered && !any_node_hovered && matches!(self.drag, Drag::None) {
            if let Some(p) = pointer {
                let mut best = (8.0f32, None);
                for link in &graph.links {
                    if let Some((a, b)) = link_endpoints(&layouts, link) {
                        let d = bezier_distance(&bezier_points(a, b, self.zoom), p);
                        if d < best.0 {
                            best = (d, Some(link.clone()));
                        }
                    }
                }
                self.hovered_link = best.1;
            }
        }

        // Canvas background.
        if canvas.drag_started_by(PointerButton::Primary) && !alt {
            if let Some(origin) = ctx.input(|i| i.pointer.press_origin()) {
                self.drag = Drag::Select { start: self.to_graph(origin), additive: shift || ctrl };
            }
        }
        if canvas.drag_started_by(PointerButton::Primary) && alt {
            self.drag = Drag::Pan;
        }
        if canvas.clicked() {
            if let Some(link) = self.hovered_link.clone() {
                self.selected_link = Some(link);
                self.selected.clear();
            } else if !shift && !ctrl {
                self.selected.clear();
                self.selected_link = None;
            }
        }
        if canvas.double_clicked() || canvas.triple_clicked() {
            if let Some(p) = pointer {
                self.open_add_menu(p, None);
            }
        }
        if canvas.secondary_clicked() {
            self.context_link = self.hovered_link.clone();
            self.context_pos = pointer.unwrap_or(rect.center());
        }
        let mut open_menu_at: Option<Pos2> = None;
        let mut paste_at: Option<Pos2> = None;
        let mut frame_all = false;
        let mut select_all = false;
        canvas.context_menu(|ui| {
            if let Some(link) = self.context_link.clone() {
                if ui.button("Delete link").clicked() {
                    graph.links.retain(|l| *l != link);
                    ui.close();
                }
                ui.separator();
            }
            if ui.button("Add node…      Space").clicked() {
                open_menu_at = Some(self.context_pos);
                ui.close();
            }
            if ui.add_enabled(clipboard.is_some(), egui::Button::new("Paste          Ctrl+V")).clicked() {
                paste_at = Some(self.context_pos);
                ui.close();
            }
            ui.separator();
            if ui.button("Select all     Ctrl+A").clicked() {
                select_all = true;
                ui.close();
            }
            if ui.button("Frame all      F").clicked() {
                frame_all = true;
                ui.close();
            }
        });

        // Drag and drop from the library / input panel.
        if let Some(payload) = canvas.dnd_release_payload::<DragPayload>() {
            if let Some(p) = pointer {
                let g = self.to_graph(p);
                let pos = [g.x - NODE_W * 0.5, g.y - HEADER_H * 0.5];
                let id = match &*payload {
                    DragPayload::Library(item) => create_from_library(graph, item, pos),
                    DragPayload::GraphInput(input) => graph.add_input_node(*input, pos),
                };
                self.selected.clear();
                self.selected.insert(id);
            }
        }

        // ---------------------------------------------------------------- apply actions
        for action in actions {
            match action {
                Action::Click { node, toggle, add } => {
                    if toggle {
                        if !self.selected.remove(&node) {
                            self.selected.insert(node);
                        }
                    } else if add {
                        self.selected.insert(node);
                    } else {
                        self.selected.clear();
                        self.selected.insert(node);
                    }
                    self.selected_link = None;
                    graph.bring_to_front(&[node]);
                }
                Action::DragStart { node, add } => {
                    if !self.selected.contains(&node) {
                        if !add {
                            self.selected.clear();
                        }
                        self.selected.insert(node);
                    }
                    let ids: Vec<NodeId> = self.selected.iter().copied().collect();
                    graph.bring_to_front(&ids);
                    self.drag = Drag::Move;
                }
                Action::Move(d) => {
                    for n in graph.nodes.iter_mut().filter(|n| self.selected.contains(&n.id)) {
                        n.pos[0] += d.x;
                        n.pos[1] += d.y;
                    }
                }
                Action::Preview(node) => out.preview = Some(node),
                Action::StartLink(from) => self.drag = Drag::Link { from },
                Action::Detach { node, key } => {
                    graph.disconnect(node, &key);
                }
                Action::Delete(ids) => {
                    graph.remove_nodes(&ids);
                    self.selected.clear();
                }
                Action::Duplicate(ids) => self.duplicate(graph, &ids),
                Action::Copy(ids) => {
                    let saved = std::mem::take(&mut self.selected);
                    self.selected = ids.into_iter().collect();
                    self.copy_selection(graph, &ctx, clipboard);
                    self.selected = saved;
                }
                Action::DisconnectInputs(node) => graph.links.retain(|l| l.to != node),
            }
        }
        if self.drag == Drag::Move && !primary {
            self.drag = Drag::None;
        }
        if let Some(p) = open_menu_at {
            self.open_add_menu(p, None);
        }
        if let (Some(p), Some(clip)) = (paste_at, clipboard.clone()) {
            let g = self.to_graph(p);
            self.paste(graph, &clip, g);
        }
        if select_all {
            self.selected = graph.nodes.iter().map(|n| n.id).collect();
        }
        if frame_all {
            self.frame_nodes(graph, false);
        }

        // ---------------------------------------------------------------- keyboard
        let typing = ctx.egui_wants_keyboard_input();
        if hovered && !typing && self.add_menu.is_none() {
            let (del, dup, all, frame, space, events) = ctx.input(|i| {
                (
                    i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
                    i.modifiers.command && i.key_pressed(Key::D),
                    i.modifiers.command && i.key_pressed(Key::A),
                    !i.modifiers.command && i.key_pressed(Key::F),
                    i.key_pressed(Key::Space),
                    i.events.clone(),
                )
            });
            if del {
                self.delete_selection(graph);
            }
            if dup {
                let ids: Vec<NodeId> = self.selected.iter().copied().collect();
                self.duplicate(graph, &ids);
            }
            if all {
                self.selected = graph.nodes.iter().map(|n| n.id).collect();
            }
            if frame {
                let only = !self.selected.is_empty();
                self.frame_nodes(graph, only);
            }
            if space {
                if let Some(p) = pointer {
                    self.open_add_menu(p, None);
                }
            }
            for e in events {
                match e {
                    Event::Copy => self.copy_selection(graph, &ctx, clipboard),
                    Event::Cut => {
                        self.copy_selection(graph, &ctx, clipboard);
                        self.delete_selection(graph);
                    }
                    Event::Paste(text) => {
                        let parsed = text
                            .strip_prefix(Clipboard::MAGIC)
                            .and_then(|json| serde_json::from_str::<Clipboard>(json).ok());
                        if let Some(clip) = parsed.or_else(|| clipboard.clone()) {
                            let at = self.to_graph(pointer.unwrap_or(rect.center()));
                            self.paste(graph, &clip, at);
                        }
                    }
                    _ => {}
                }
            }
        }

        // ---------------------------------------------------------------- finish link drags
        if let Drag::Link { from } = self.drag.clone() {
            let released = ctx.input(|i| i.pointer.any_released() || !i.pointer.any_down());
            if released {
                self.drag = Drag::None;
                let layouts: Vec<(NodeId, Layout)> =
                    graph.nodes.iter().map(|n| (n.id, self.layout(graph, n))).collect();
                if let Some(p) = pointer.or_else(|| ctx.pointer_interact_pos()) {
                    match self.socket_at(&layouts, p) {
                        Some(target) if Self::can_link(graph, &from, &target) => {
                            if let Err(e) = Self::connect(graph, &from, &target) {
                                out.message = Some((e, true));
                            }
                        }
                        Some(target) if target.node != from.node && target.output != from.output => {
                            let (o, i) = if from.output { (&from, &target) } else { (&target, &from) };
                            let msg = if graph.depends_on(o.node, i.node) {
                                "This connection would create a cycle".to_string()
                            } else {
                                format!("Cannot connect {} to {}", o.ty.name(), i.ty.name())
                            };
                            out.message = Some((msg, true));
                        }
                        Some(_) => {}
                        None if rect.contains(p) => self.open_add_menu(p, Some(from)),
                        None => {}
                    }
                }
            }
        }

        // ---------------------------------------------------------------- box selection
        if let Drag::Select { start, additive } = self.drag {
            if !primary {
                if let Some(p) = pointer.or_else(|| ctx.pointer_interact_pos()) {
                    let sel = Rect::from_two_pos(start, self.to_graph(p));
                    if !additive {
                        self.selected.clear();
                    }
                    for n in &graph.nodes {
                        let r = Rect::from_min_size(pos2(n.pos[0], n.pos[1]), node_size(graph, n));
                        if sel.intersects(r) {
                            self.selected.insert(n.id);
                        }
                    }
                }
                self.drag = Drag::None;
            }
        }

        // ---------------------------------------------------------------- pass 2: paint
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(24, 25, 28));
        self.paint_grid(&painter, rect);
        let layouts: Vec<(NodeId, Layout)> = graph.nodes.iter().map(|n| (n.id, self.layout(graph, n))).collect();

        self.debug_nodes = layouts
            .iter()
            .map(|(id, l)| DebugNode {
                id: *id,
                rect: l.rect,
                header: l.header,
                sockets: l
                    .inputs
                    .iter()
                    .map(|(s, p)| (s.key.clone(), false, *p))
                    .chain(l.outputs.iter().map(|(s, p)| (s.key.clone(), true, *p)))
                    .collect(),
            })
            .collect();

        // Links under the nodes.
        for link in &graph.links {
            let Some((a, b)) = link_endpoints(&layouts, link) else { continue };
            let ty = graph.output_socket(link.from, &link.from_socket).map_or(DataType::Float, |s| s.ty);
            let mut color = type_color(ty);
            let mut width = if ty == DataType::Image { 2.6 } else { 1.9 } * self.zoom.clamp(0.5, 1.5);
            if self.selected_link.as_ref() == Some(link) {
                color = ACCENT;
                width += 1.5;
            } else if self.hovered_link.as_ref() == Some(link) {
                color = Color32::WHITE;
                width += 1.0;
            } else if !self.selected.contains(&link.from) && !self.selected.contains(&link.to) {
                color = color.gamma_multiply(0.75);
            }
            painter.add(CubicBezierShape::from_points_stroke(
                bezier_points(a, b, self.zoom),
                false,
                Color32::TRANSPARENT,
                Stroke::new(width, color),
            ));
        }

        let dragging_from = if let Drag::Link { from } = &self.drag { Some(from.clone()) } else { None };
        for (node_id, l) in &layouts {
            if !l.rect.intersects(rect) {
                continue;
            }
            let Some(node) = graph.node(*node_id) else { continue };
            self.paint_node(&painter, graph, engine, node, l, preview_node == Some(*node_id), dragging_from.as_ref());
        }

        // Link being dragged.
        if let (Some(from), Some(p)) = (&dragging_from, pointer.or_else(|| ctx.pointer_interact_pos())) {
            let start = layouts
                .iter()
                .find(|(id, _)| *id == from.node)
                .and_then(|(_, l)| {
                    let list = if from.output { &l.outputs } else { &l.inputs };
                    list.iter().find(|(s, _)| s.key == from.key).map(|(_, p)| *p)
                });
            if let Some(start) = start {
                let (a, b) = if from.output { (start, p) } else { (p, start) };
                let target_ok = self.socket_at(&layouts, p).is_some_and(|t| Self::can_link(graph, from, &t));
                let color = if target_ok { Color32::WHITE } else { type_color(from.ty) };
                painter.add(CubicBezierShape::from_points_stroke(
                    bezier_points(a, b, self.zoom),
                    false,
                    Color32::TRANSPARENT,
                    Stroke::new(2.4, color),
                ));
            }
        }

        // Selection rectangle.
        if let (Drag::Select { start, .. }, Some(p)) = (&self.drag, pointer) {
            let r = Rect::from_two_pos(self.to_screen(*start), p);
            painter.rect_filled(r, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(240, 160, 48, 18));
            painter.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, ACCENT), StrokeKind::Inside);
        }

        // Drop hint while dragging something from a panel.
        if let Some(payload) = canvas.dnd_hover_payload::<DragPayload>() {
            let label = match &*payload {
                DragPayload::Library(item) => format!("Drop to add {}", item.label),
                DragPayload::GraphInput(_) => "Drop to add an input node".into(),
            };
            painter.text(rect.center_top() + vec2(0.0, 24.0), Align2::CENTER_CENTER, label, FontId::proportional(15.0), ACCENT);
            painter.rect_stroke(rect.shrink(2.0), CornerRadius::same(4), Stroke::new(2.0, ACCENT), StrokeKind::Inside);
        }

        if graph.nodes.is_empty() {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Drag nodes from the library, double-click or press Space to add one",
                FontId::proportional(15.0),
                Color32::from_gray(120),
            );
        }

        painter.text(
            rect.right_bottom() - vec2(10.0, 8.0),
            Align2::RIGHT_BOTTOM,
            format!("{:.0}%", self.zoom * 100.0),
            FontId::proportional(12.0),
            Color32::from_gray(110),
        );

        self.show_add_menu(&ctx, graph, &mut out);
        out
    }

    fn open_add_menu(&mut self, screen_pos: Pos2, pending: Option<SocketRef>) {
        let g = self.to_graph(screen_pos);
        self.add_menu = Some(AddMenu {
            screen_pos,
            graph_pos: pos2(g.x - NODE_W * 0.5, g.y - HEADER_H * 0.5),
            search: String::new(),
            pending,
            just_opened: true,
        });
    }

    /// Node search popup (Space, double-click, or dropping a link on empty space).
    fn show_add_menu(&mut self, ctx: &egui::Context, graph: &mut Graph, out: &mut GraphViewOutput) {
        let Some(menu) = &mut self.add_menu else { return };
        let items: Vec<LibraryItem> = library()
            .into_iter()
            .filter(|item| {
                let q = menu.search.trim().to_lowercase();
                q.is_empty() || item.label.to_lowercase().contains(&q)
            })
            .filter(|item| match &menu.pending {
                None => true,
                Some(p) => item_accepts(item, p),
            })
            .collect();
        let mut chosen: Option<LibraryItem> = None;
        let mut close = false;
        let area = egui::Area::new(Id::new("ferrotex_add_menu"))
            .fixed_pos(menu.screen_pos)
            .order(Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(210.0);
                    let title = if menu.pending.is_some() { "Add and connect" } else { "Add node" };
                    ui.label(egui::RichText::new(title).strong());
                    let te = ui.add(egui::TextEdit::singleline(&mut menu.search).hint_text("Search…"));
                    if menu.just_opened {
                        te.request_focus();
                        menu.just_opened = false;
                    }
                    let enter = te.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for item in &items {
                            let r = ui.selectable_label(false, item.label);
                            if r.clicked() {
                                chosen = Some(*item);
                            }
                        }
                        if items.is_empty() {
                            ui.weak("No matching node");
                        }
                    });
                    if enter {
                        chosen = items.first().copied();
                    }
                });
            });
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            close = true;
        }
        let clicked_outside = ctx.input(|i| i.pointer.any_pressed())
            && ctx.pointer_interact_pos().is_some_and(|p| !area.response.rect.contains(p));
        if clicked_outside {
            close = true;
        }
        if let Some(item) = chosen {
            let pos = [menu.graph_pos.x, menu.graph_pos.y];
            let pending = menu.pending.clone();
            let id = create_from_library(graph, &item, pos);
            if let Some(p) = pending {
                if let Err(e) = connect_new_node(graph, &p, id) {
                    out.message = Some((e, true));
                }
            }
            self.selected.clear();
            self.selected.insert(id);
            close = true;
        }
        if close {
            self.add_menu = None;
        }
    }

    fn paint_grid(&self, painter: &egui::Painter, rect: Rect) {
        let mut step = 32.0 * self.zoom;
        while step < 14.0 {
            step *= 4.0;
        }
        let origin = rect.min + self.pan;
        let minor = Color32::from_rgb(32, 33, 37);
        let major = Color32::from_rgb(42, 43, 48);
        let first_x = ((rect.left() - origin.x) / step).floor() as i64;
        let last_x = ((rect.right() - origin.x) / step).ceil() as i64;
        for i in first_x..=last_x {
            let x = origin.x + i as f32 * step;
            let c = if i % 4 == 0 { major } else { minor };
            painter.vline(x, rect.y_range(), Stroke::new(1.0, c));
        }
        let first_y = ((rect.top() - origin.y) / step).floor() as i64;
        let last_y = ((rect.bottom() - origin.y) / step).ceil() as i64;
        for i in first_y..=last_y {
            let y = origin.y + i as f32 * step;
            let c = if i % 4 == 0 { major } else { minor };
            painter.hline(rect.x_range(), y, Stroke::new(1.0, c));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_node(
        &self,
        painter: &egui::Painter,
        graph: &Graph,
        engine: &Engine,
        node: &Node,
        l: &Layout,
        previewed: bool,
        dragging: Option<&SocketRef>,
    ) {
        let z = self.zoom;
        let d = node.def();
        let state = engine.state(node.id);
        let error = state.and_then(|s| s.error.clone());
        let warning = state.and_then(|s| s.warning.clone());
        let selected = self.selected.contains(&node.id);
        let rounding = (6.0 * z).clamp(1.0, 255.0) as u8;

        // Shadow + body.
        painter.rect_filled(l.rect.translate(vec2(3.0, 4.0) * z), CornerRadius::same(rounding), Color32::from_black_alpha(70));
        painter.rect_filled(l.rect, CornerRadius::same(rounding), Color32::from_rgb(44, 46, 51));
        let header_round = CornerRadius { nw: rounding, ne: rounding, sw: 0, se: 0 };
        painter.rect_filled(l.header, header_round, category_color(d.category));

        let show_text = z > 0.32;
        let font = FontId::proportional((12.5 * z).clamp(4.0, 30.0));
        let small = FontId::proportional((11.0 * z).clamp(4.0, 26.0));
        let mono = FontId::monospace((11.0 * z).clamp(4.0, 26.0));
        if show_text {
            let mut title = node_title(graph, node);
            let max_chars = ((l.rect.width() - 40.0 * z) / (7.0 * z)).max(3.0) as usize;
            if title.chars().count() > max_chars {
                title = title.chars().take(max_chars.saturating_sub(1)).collect::<String>() + "…";
            }
            painter.text(
                l.header.left_center() + vec2(8.0 * z, 0.0),
                Align2::LEFT_CENTER,
                title,
                font.clone(),
                Color32::WHITE,
            );
        }
        // Header badges: output format, preview marker, error / warning dot.
        let mut badge_x = l.header.right() - 10.0 * z;
        if error.is_some() || warning.is_some() {
            let c = if error.is_some() { ERROR } else { WARNING };
            painter.circle_filled(pos2(badge_x, l.header.center().y), 6.0 * z, c);
            if show_text {
                painter.text(pos2(badge_x, l.header.center().y), Align2::CENTER_CENTER, "!", small.clone(), Color32::BLACK);
            }
            badge_x -= 16.0 * z;
        }
        if previewed {
            painter.circle_stroke(pos2(badge_x, l.header.center().y), 5.0 * z, Stroke::new(1.5 * z, Color32::WHITE));
            painter.circle_filled(pos2(badge_x, l.header.center().y), 2.2 * z, Color32::WHITE);
            badge_x -= 16.0 * z;
        }
        if node.kind == NodeKind::Output && show_text {
            let fmt = crate::graph::defs::OUTPUT_FORMATS.get(node.enum_value("format") as usize).copied().unwrap_or("");
            painter.text(pos2(badge_x, l.header.center().y), Align2::RIGHT_CENTER, fmt, small.clone(), Color32::from_gray(230));
        }

        // Socket labels.
        if z > 0.48 {
            for (s, p) in &l.inputs {
                let linked = graph.link_into(node.id, &s.key).is_some();
                let driven = s.kind == SocketKind::Param && linked;
                let color = if driven { type_color(s.ty) } else { Color32::from_gray(200) };
                painter.text(*p + vec2(10.0 * z, 0.0), Align2::LEFT_CENTER, &s.label, small.clone(), color);
            }
            for (s, p) in &l.outputs {
                painter.text(*p - vec2(10.0 * z, 0.0), Align2::RIGHT_CENTER, &s.label, small.clone(), Color32::from_gray(200));
            }
        }

        // Body: thumbnail or value text.
        let text_x = l.rect.left() + 8.0 * z;
        let line = |i: f32| l.text_top + (i + 0.5) * LINE_H * z;
        if node.is_expression_node() && show_text {
            let expr = node.text("expression");
            let max_chars = ((l.rect.width() - 16.0 * z) / (6.6 * z)).max(3.0) as usize;
            let shown = if expr.chars().count() > max_chars {
                expr.chars().take(max_chars.saturating_sub(1)).collect::<String>() + "…"
            } else {
                expr.to_string()
            };
            let y = if node.kind == NodeKind::PixelMath { l.text_top - 0.5 * LINE_H * z } else { line(0.0) };
            painter.text(pos2(text_x, y), Align2::LEFT_CENTER, shown, mono.clone(), Color32::from_rgb(230, 210, 160));
        }
        if let Some(t) = l.thumb {
            painter.rect_filled(t, CornerRadius::same(2), Color32::from_rgb(20, 20, 22));
            if let Some(tex) = engine.thumbnail(node.id) {
                painter.image(tex, t, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            painter.rect_stroke(t, CornerRadius::same(2), Stroke::new(1.0, Color32::from_gray(20)), StrokeKind::Outside);
        } else if show_text {
            let value = engine.value(node.id);
            let row = if node.kind == NodeKind::Math { 1.0 } else { 0.0 };
            match (&error, value) {
                (Some(e), _) => {
                    let msg = if node.kind == NodeKind::Math { "Invalid expression" } else { e.as_str() };
                    painter.text(pos2(text_x, line(row)), Align2::LEFT_CENTER, msg, small.clone(), ERROR);
                }
                (None, Some(v)) => {
                    if let crate::graph::Value::Color(c) = v {
                        let sw = Rect::from_min_size(pos2(text_x, line(row) - 6.0 * z), vec2(28.0 * z, 12.0 * z));
                        painter.rect_filled(sw, CornerRadius::same(2), super::widgets::color32(c));
                        painter.text(sw.right_center() + vec2(6.0 * z, 0.0), Align2::LEFT_CENTER, v.display(), small.clone(), Color32::from_gray(220));
                    } else {
                        painter.text(pos2(text_x, line(row)), Align2::LEFT_CENTER, format!("= {}", v.display()), small.clone(), Color32::from_gray(230));
                    }
                }
                (None, None) => {}
            }
        }

        // Outline.
        let (stroke_w, stroke_c) = if selected {
            (2.0, ACCENT)
        } else if error.is_some() {
            (1.5, ERROR)
        } else {
            (1.0, Color32::from_gray(20))
        };
        painter.rect_stroke(l.rect, CornerRadius::same(rounding), Stroke::new(stroke_w, stroke_c), StrokeKind::Outside);

        // Sockets.
        let r = (SOCKET_R * z).max(2.5);
        for (list, output) in [(&l.inputs, false), (&l.outputs, true)] {
            for (s, p) in list.iter() {
                let color = type_color(s.ty);
                let linked = if output {
                    graph.links.iter().any(|lk| lk.from == node.id && lk.from_socket == s.key)
                } else {
                    graph.link_into(node.id, &s.key).is_some()
                };
                let me = SocketRef { node: node.id, key: s.key.clone(), output, ty: s.ty };
                let (radius, alpha) = match dragging {
                    Some(from) if Self::can_link(graph, from, &me) => (r * 1.35, 1.0),
                    Some(from) if *from != me => (r, 0.3),
                    _ => (r, 1.0),
                };
                let color = color.gamma_multiply(alpha);
                if s.ty == DataType::Image {
                    painter.circle(*p, radius, if linked { color } else { Color32::from_rgb(30, 31, 34) }, Stroke::new(1.6 * z.max(0.6), color));
                } else {
                    // Value sockets are diamonds so they read differently from images.
                    let pts = vec![*p + vec2(0.0, -radius * 1.15), *p + vec2(radius * 1.15, 0.0), *p + vec2(0.0, radius * 1.15), *p + vec2(-radius * 1.15, 0.0)];
                    painter.add(egui::Shape::convex_polygon(
                        pts,
                        if linked { color } else { Color32::from_rgb(30, 31, 34) },
                        Stroke::new(1.6 * z.max(0.6), color),
                    ));
                }
            }
        }
    }
}

fn link_endpoints(layouts: &[(NodeId, Layout)], link: &Link) -> Option<(Pos2, Pos2)> {
    let from = layouts.iter().find(|(id, _)| *id == link.from)?;
    let to = layouts.iter().find(|(id, _)| *id == link.to)?;
    let a = from.1.outputs.iter().find(|(s, _)| s.key == link.from_socket)?.1;
    let b = to.1.inputs.iter().find(|(s, _)| s.key == link.to_socket)?.1;
    Some((a, b))
}

/// Can a node created from `item` be connected to the pending socket `p`?
fn item_accepts(item: &LibraryItem, p: &SocketRef) -> bool {
    let d = crate::graph::def(item.kind);
    if p.output {
        // Needs an input compatible with the dragged output.
        let image_ok = p.ty == DataType::Image && (!d.image_inputs.is_empty() || item.kind == NodeKind::PixelMath);
        let value_ok = p.ty != DataType::Image && item.kind == NodeKind::Math && p.ty.is_scalar();
        image_ok || value_ok
    } else {
        let out_ty = match item.input_type {
            Some(t) => t,
            None => match d.outputs.first() {
                Some(o) => o.ty,
                None => return false,
            },
        };
        can_connect(out_ty, p.ty)
    }
}

/// Connect a freshly created node to the socket a link was dragged from.
fn connect_new_node(graph: &mut Graph, p: &SocketRef, id: NodeId) -> Result<(), String> {
    let Some(node) = graph.node(id).cloned() else { return Ok(()) };
    if p.output {
        let target = graph
            .input_sockets(&node)
            .into_iter()
            .find(|s| can_connect(p.ty, s.ty))
            .map(|s| s.key);
        match target {
            Some(key) => graph.connect(p.node, &p.key, id, &key),
            None => Ok(()),
        }
    } else {
        let source = graph.output_sockets(&node).into_iter().find(|s| can_connect(s.ty, p.ty)).map(|s| s.key);
        match source {
            Some(key) => graph.connect(id, &key, p.node, &p.key),
            None => Ok(()),
        }
    }
}
