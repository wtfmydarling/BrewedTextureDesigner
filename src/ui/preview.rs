//! The 2D view: shows the image of the double-clicked node.
//!
//! Scroll zooms around the cursor, dragging pans, the channel buttons isolate R/G/B/A,
//! "Tiling" repeats the image the way the node tiles, and a Transform 2D node gets an
//! interactive gizmo (move, rotate, scale).

use eframe::egui::{
    self, Align2, Color32, CornerRadius, Event, FontId, PointerButton, Pos2, Rect, RichText, Sense, Stroke, StrokeKind,
    Ui, Vec2, pos2, vec2,
};

use super::ACCENT;
use crate::engine::Engine;
use crate::graph::{Graph, NodeId, NodeKind, ParamValue};

const CHANNELS: [&str; 6] = ["RGBA", "RGB", "R", "G", "B", "A"];

#[derive(Clone, Copy, Debug)]
struct Xform {
    offset: [f32; 2],
    rotation: f32,
    scale: [f32; 2],
    pivot: [f32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Handle {
    Move,
    Rotate,
    /// Source-space point being dragged, and which axes it scales.
    Scale {
        src: [f32; 2],
        x: bool,
        y: bool,
    },
}

#[derive(Clone, Copy, Debug)]
struct GizmoDrag {
    handle: Handle,
    start_uv: Pos2,
    start_screen: Pos2,
    start: Xform,
}

#[derive(Clone, Copy, Debug)]
pub struct GizmoDebug {
    pub center: Pos2,
    /// Transformed image corners: top-left, top-right, bottom-right, bottom-left.
    pub corners: [Pos2; 4],
    pub ring_radius: f32,
    pub image_rect: Rect,
}

pub struct PreviewState {
    pub node: Option<NodeId>,
    pub mode: u32,
    pub show_tiling: bool,
    zoom: f32,
    pan: Vec2,
    fit: bool,
    gizmo: Option<GizmoDrag>,
    shown: Option<NodeId>,
    /// Screen geometry of the Transform 2D gizmo drawn last frame (used by the UI self-test).
    pub debug_gizmo: Option<GizmoDebug>,
}

impl Default for PreviewState {
    fn default() -> Self {
        PreviewState {
            node: None,
            mode: 0,
            show_tiling: false,
            zoom: 1.0,
            pan: Vec2::ZERO,
            fit: true,
            gizmo: None,
            shown: None,
            debug_gizmo: None,
        }
    }
}

fn rotate_ccw(p: Vec2, deg: f32) -> Vec2 {
    let (s, c) = deg.to_radians().sin_cos();
    vec2(c * p.x + s * p.y, -s * p.x + c * p.y)
}

impl Xform {
    fn from_node(graph: &Graph, engine: &Engine, id: NodeId) -> Option<(Xform, [bool; 4])> {
        let node = graph.node(id)?;
        let driven = engine.state(id).map(|s| s.driven.clone()).unwrap_or_default();
        let get = |k: &str| driven.get(k).cloned().or_else(|| node.param(k).map(|p| p.value.clone()));
        let v2 = |k: &str| match get(k) {
            Some(ParamValue::Vec2(v)) => v,
            _ => [0.0; 2],
        };
        let x = Xform {
            offset: v2("offset"),
            rotation: get("rotation").map_or(0.0, |v| v.as_f32()),
            scale: v2("scale"),
            pivot: v2("pivot"),
        };
        let locked = ["offset", "rotation", "scale", "pivot"].map(|k| driven.contains_key(k));
        Some((x, locked))
    }

    /// Source uv -> destination uv (mirrors the inverse mapping of transform.wgsl).
    fn forward(&self, src: Vec2, aspect: f32) -> Vec2 {
        let pivot = Vec2::from(self.pivot);
        let mut d = (src - pivot) * Vec2::from(self.scale);
        d.x *= aspect;
        d = rotate_ccw(d, self.rotation);
        d.x /= aspect;
        pivot + Vec2::from(self.offset) + d
    }

    /// Destination uv -> offset from the pivot in unrotated, unscaled... scaled space.
    fn local(&self, dest: Vec2, aspect: f32) -> Vec2 {
        let mut d = dest - Vec2::from(self.pivot) - Vec2::from(self.offset);
        d.x *= aspect;
        d = rotate_ccw(d, -self.rotation);
        d.x /= aspect;
        d
    }

    fn center(&self) -> Vec2 {
        Vec2::from(self.pivot) + Vec2::from(self.offset)
    }
}

fn set_param(graph: &mut Graph, id: NodeId, key: &str, value: ParamValue) {
    if let Some(p) = graph.node_mut(id).and_then(|n| n.param_mut(key)) {
        p.value = value;
    }
}

pub fn show(ui: &mut Ui, graph: &mut Graph, engine: &mut Engine, state: &mut PreviewState) {
    state.debug_gizmo = None;
    // Forget a node that was deleted.
    if state.node.is_some_and(|id| !graph.contains(id)) {
        state.node = None;
    }
    if state.node != state.shown {
        state.shown = state.node;
        state.fit = true;
        state.gizmo = None;
    }
    let node = state.node.and_then(|id| graph.node(id)).cloned();

    // ---------------------------------------------------------------- toolbar
    ui.horizontal(|ui| {
        ui.label(RichText::new("2D View").strong());
        match &node {
            Some(n) => {
                ui.label(RichText::new(super::graph_view::node_title(graph, n)).color(ACCENT));
            }
            None => {
                ui.weak("double-click a node to show it here");
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("1:1").on_hover_text("One image pixel per screen pixel").clicked() {
                state.zoom = 1.0 / ui.ctx().pixels_per_point();
                state.pan = Vec2::ZERO;
            }
            if ui.button("Fit").on_hover_text("Fit the image in the view (double-click the view)").clicked() {
                state.fit = true;
            }
            ui.toggle_value(&mut state.show_tiling, "Tiling")
                .on_hover_text("Repeat the image around itself according to the node's tiling mode");
            ui.separator();
            for (i, name) in CHANNELS.iter().enumerate().rev() {
                if ui.selectable_label(state.mode == i as u32, *name).clicked() {
                    state.mode = i as u32;
                }
            }
        });
    });

    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(18, 18, 20));

    let Some(node) = node else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "No node shown",
            FontId::proportional(15.0),
            Color32::from_gray(110),
        );
        return;
    };
    let id = node.id;

    if !node.def().has_image {
        let text = match (engine.state(id).and_then(|s| s.error.clone()), engine.value(id)) {
            (Some(e), _) => e,
            (None, Some(v)) => format!("{} = {}", super::graph_view::node_title(graph, &node), v.display()),
            (None, None) => "No value".into(),
        };
        painter.text(rect.center(), Align2::CENTER_CENTER, text, FontId::monospace(20.0), Color32::WHITE);
        return;
    }

    let Some(info) = engine.preview_info(id) else {
        painter.text(rect.center(), Align2::CENTER_CENTER, "Computing…", FontId::proportional(15.0), Color32::GRAY);
        ui.ctx().request_repaint();
        return;
    };
    let img_px = vec2(info.size[0] as f32, info.size[1] as f32);
    if state.fit && rect.is_positive() {
        state.zoom = ((rect.width() - 24.0) / img_px.x).min((rect.height() - 24.0) / img_px.y).max(0.01);
        state.pan = Vec2::ZERO;
        state.fit = false;
    }

    // ---------------------------------------------------------------- zoom & pan
    let pointer = ui.ctx().pointer_hover_pos().filter(|p| rect.contains(*p));
    if let Some(p) = pointer {
        let scroll: f32 = ui.input(|i| {
            i.events
                .iter()
                .map(|e| match e {
                    Event::MouseWheel { unit, delta, .. } => match unit {
                        egui::MouseWheelUnit::Point => delta.y,
                        egui::MouseWheelUnit::Line => delta.y * 40.0,
                        egui::MouseWheelUnit::Page => delta.y * 400.0,
                    },
                    _ => 0.0,
                })
                .sum()
        });
        let factor = (scroll * 0.002).exp() * ui.input(|i| i.zoom_delta());
        if (factor - 1.0).abs() > 1e-4 {
            let center = rect.center() + state.pan;
            let old = state.zoom;
            state.zoom = (state.zoom * factor).clamp(0.02, 64.0);
            // Keep the image point under the cursor fixed.
            let rel = (p - center) / old;
            state.pan = p - rect.center() - rel * state.zoom;
        }
    }
    if resp.double_clicked() || resp.triple_clicked() {
        state.fit = true;
    }

    let img_size = img_px * state.zoom;
    let img_rect = Rect::from_center_size(rect.center() + state.pan, img_size);
    let uv_to_screen = |uv: Vec2| img_rect.min + uv * img_rect.size();
    let screen_to_uv = |p: Pos2| (p - img_rect.min) / img_rect.size();

    // ---------------------------------------------------------------- image (with tiled copies)
    let tiling = engine
        .state(id)
        .and_then(|s| s.driven.get("tiling").map(|v| v.as_f32() as u32))
        .unwrap_or_else(|| node.enum_value("tiling"));
    let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    if let Some(tex) = info.texture {
        if state.show_tiling {
            let (rx, ry) = (if tiling & 1 != 0 { 1 } else { 0 }, if tiling & 2 != 0 { 1 } else { 0 });
            for dy in -ry..=ry {
                for dx in -rx..=rx {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let r = img_rect.translate(vec2(dx as f32 * img_size.x, dy as f32 * img_size.y));
                    if r.intersects(rect) {
                        painter.image(tex, r, full, Color32::from_gray(190));
                    }
                }
            }
        }
        painter.image(tex, img_rect, full, Color32::WHITE);
    }
    painter.rect_stroke(img_rect, CornerRadius::ZERO, Stroke::new(1.0, Color32::from_gray(70)), StrokeKind::Outside);

    // ---------------------------------------------------------------- Transform 2D gizmo

    if node.kind == NodeKind::Transform2D
        && let Some((xf, locked)) = Xform::from_node(graph, engine, id)
    {
        let aspect = img_px.x / img_px.y;
        let corners = [vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0)];
        let quad: Vec<Pos2> = corners.iter().map(|c| uv_to_screen(xf.forward(*c, aspect))).collect();
        let center = uv_to_screen(xf.center());
        let ring_r = 64.0;

        // Handles: (screen position, handle)
        let mut handles: Vec<(Pos2, Handle)> = Vec::new();
        for (i, c) in corners.iter().enumerate() {
            handles.push((quad[i], Handle::Scale { src: [c.x, c.y], x: true, y: true }));
        }
        let mids = [
            (vec2(0.5, 0.0), false, true),
            (vec2(1.0, 0.5), true, false),
            (vec2(0.5, 1.0), false, true),
            (vec2(0.0, 0.5), true, false),
        ];
        for (m, x, y) in mids {
            handles.push((uv_to_screen(xf.forward(m, aspect)), Handle::Scale { src: [m.x, m.y], x, y }));
        }

        let hit = |p: Pos2| -> Option<Handle> {
            if p.distance(center) < 10.0 {
                return Some(Handle::Move);
            }
            for (hp, h) in &handles {
                if hp.distance(p) < 8.0 {
                    return Some(*h);
                }
            }
            if (p.distance(center) - ring_r).abs() < 6.0 {
                return Some(Handle::Rotate);
            }
            None
        };
        state.debug_gizmo = Some(GizmoDebug { center, corners: [quad[0], quad[1], quad[2], quad[3]], ring_radius: ring_r, image_rect: img_rect });
        let hovered = pointer.and_then(&hit);
        let gizmo_hover = hovered.is_some() || state.gizmo.is_some();

        if resp.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            && let Some(h) = hit(origin)
        {
            state.gizmo = Some(GizmoDrag {
                handle: h,
                start_uv: screen_to_uv(origin).to_pos2(),
                start_screen: origin,
                start: xf,
            });
        }
        if let (Some(g), Some(p)) = (state.gizmo, ui.ctx().pointer_interact_pos())
            && resp.dragged_by(PointerButton::Primary)
        {
            let shift = ui.input(|i| i.modifiers.shift);
            let uv = screen_to_uv(p);
            match g.handle {
                Handle::Move if !locked[0] => {
                    let mut o = Vec2::from(g.start.offset) + (uv - g.start_uv.to_vec2());
                    if shift {
                        // Constrain to the dominant axis.
                        let d = uv - g.start_uv.to_vec2();
                        if d.x.abs() > d.y.abs() {
                            o.y = g.start.offset[1];
                        } else {
                            o.x = g.start.offset[0];
                        }
                    }
                    set_param(graph, id, "offset", ParamValue::Vec2([o.x, o.y]));
                }
                Handle::Rotate if !locked[1] => {
                    let c = uv_to_screen(g.start.center());
                    let a0 = (g.start_screen - c).angle();
                    let a1 = (p - c).angle();
                    let mut r = g.start.rotation - (a1 - a0).to_degrees();
                    r = (r + 180.0).rem_euclid(360.0) - 180.0;
                    if shift {
                        r = (r / 15.0).round() * 15.0;
                    }
                    set_param(graph, id, "rotation", ParamValue::Float(r));
                }
                Handle::Scale { src, x, y } if !locked[2] => {
                    let local = g.start.local(uv, aspect);
                    let rel = Vec2::from(src) - Vec2::from(g.start.pivot);
                    let mut s = g.start.scale;
                    if x && rel.x.abs() > 1e-4 {
                        s[0] = local.x / rel.x;
                    }
                    if y && rel.y.abs() > 1e-4 {
                        s[1] = local.y / rel.y;
                    }
                    if shift {
                        let fx = if x { s[0] / g.start.scale[0] } else { s[1] / g.start.scale[1] };
                        let fy = if y { s[1] / g.start.scale[1] } else { fx };
                        let f = if x && y { (fx + fy) * 0.5 } else { fx.max(fy) };
                        s = [g.start.scale[0] * f, g.start.scale[1] * f];
                    }
                    let clamp = |v: f32| if v.abs() < 0.01 { 0.01f32.copysign(v) } else { v.clamp(-64.0, 64.0) };
                    set_param(graph, id, "scale", ParamValue::Vec2([clamp(s[0]), clamp(s[1])]));
                }
                _ => {}
            }
        }
        if !ui.input(|i| i.pointer.primary_down()) {
            state.gizmo = None;
        }

        // Paint.
        let active = state.gizmo.map(|g| g.handle).or(hovered);
        let col = |h: Handle, locked: bool| {
            if locked {
                Color32::from_gray(110)
            } else if active == Some(h) {
                Color32::WHITE
            } else {
                ACCENT
            }
        };
        let mut closed = quad.clone();
        closed.push(quad[0]);
        painter.add(egui::Shape::line(closed, Stroke::new(1.5, ACCENT.gamma_multiply(0.9))));
        painter.circle_stroke(
            center,
            ring_r,
            Stroke::new(if active == Some(Handle::Rotate) { 2.5 } else { 1.2 }, col(Handle::Rotate, locked[1])),
        );
        // Rotation indicator: where the image's "up" axis points.
        let up = uv_to_screen(xf.forward(vec2(xf.pivot[0], xf.pivot[1] - 0.5), aspect)) - center;
        if up.length() > 1.0 {
            let tip = center + up.normalized() * ring_r;
            painter.line_segment([center, tip], Stroke::new(1.0, col(Handle::Rotate, locked[1])));
            painter.circle_filled(tip, 4.0, col(Handle::Rotate, locked[1]));
        }
        for (p, h) in &handles {
            let r = Rect::from_center_size(*p, Vec2::splat(9.0));
            painter.rect_filled(r, CornerRadius::same(1), Color32::from_black_alpha(160));
            painter.rect_stroke(r, CornerRadius::same(1), Stroke::new(1.5, col(*h, locked[2])), StrokeKind::Middle);
        }
        painter.circle_filled(center, 6.0, Color32::from_black_alpha(160));
        painter.circle_stroke(center, 6.0, Stroke::new(2.0, col(Handle::Move, locked[0])));
        if gizmo_hover {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
    }

    // ---------------------------------------------------------------- pan
    let dragging_view = resp.dragged_by(PointerButton::Middle)
        || resp.dragged_by(PointerButton::Secondary)
        || (resp.dragged_by(PointerButton::Primary) && state.gizmo.is_none());
    if dragging_view {
        state.pan += resp.drag_delta();
    }

    // ---------------------------------------------------------------- pixel probe
    let mut footer =
        format!("{}×{}  ·  {:.0}%", info.size[0], info.size[1], state.zoom * 100.0 * ui.ctx().pixels_per_point());
    if let Some(p) = pointer {
        let mut uv = screen_to_uv(p);
        let wrap_x = state.show_tiling && tiling & 1 != 0;
        let wrap_y = state.show_tiling && tiling & 2 != 0;
        if wrap_x {
            uv.x = uv.x.rem_euclid(1.0);
        }
        if wrap_y {
            uv.y = uv.y.rem_euclid(1.0);
        }
        if (0.0..1.0).contains(&uv.x) && (0.0..1.0).contains(&uv.y) {
            let px = [(uv.x * img_px.x) as u32, (uv.y * img_px.y) as u32];
            footer.push_str(&format!("  ·  x {} y {}", px[0], px[1]));
            if let Some(c) = engine.probe(id, px) {
                footer.push_str(&format!("  ·  R {:.3}  G {:.3}  B {:.3}  A {:.3}", c[0], c[1], c[2], c[3]));
            }
        }
    }
    let footer_rect = Rect::from_min_size(rect.left_bottom() - vec2(0.0, 20.0), vec2(rect.width(), 20.0));
    painter.rect_filled(footer_rect, CornerRadius::ZERO, Color32::from_black_alpha(150));
    painter.text(
        footer_rect.left_center() + vec2(8.0, 0.0),
        Align2::LEFT_CENTER,
        footer,
        FontId::monospace(11.5),
        Color32::from_gray(210),
    );
    if let Some(err) = engine.state(id).and_then(|s| s.error.clone()) {
        painter.text(rect.left_top() + vec2(8.0, 8.0), Align2::LEFT_TOP, err, FontId::proportional(13.0), super::ERROR);
    }
}
