//! Right panel, bottom half: properties of the selected node.

use std::collections::BTreeSet;

use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use super::inputs_panel::{InputsPanelState, input_details};
use super::widgets::{color32, driven_label, expression_editor, param_editor};
use super::{ACCENT, ERROR, WARNING, type_color};
use crate::engine::{Engine, VarSource};
use crate::expr::{self, builtins};
use crate::graph::{
    DataType, Graph, InputId, Node, NodeId, NodeKind, PIXEL_MATH_BUILTINS, ParamValue, param_socket_key,
};

#[derive(Default)]
pub struct PropsState {
    plot_var: usize,
    plot_range: Option<(f32, f32)>,
    levels_drag: Option<usize>,
    /// Screen rectangles of interactive widgets (used by the UI self-test).
    pub debug_rects: std::collections::HashMap<String, Rect>,
}

#[derive(Default)]
pub struct PropsOutput {
    pub export_node: Option<NodeId>,
    pub browse_bitmap: Option<NodeId>,
    pub preview: Option<NodeId>,
}

/// Socket-shaped toggle: filled when the parameter is exposed.
fn expose_toggle(ui: &mut Ui, exposed: bool, ty: DataType, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), if enabled { Sense::click() } else { Sense::hover() });
    let mut c = type_color(ty);
    if !enabled {
        c = c.gamma_multiply(0.25);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(3), Color32::from_gray(60));
    }
    let m = rect.center();
    let r = 5.0;
    let fill = if exposed { c } else { Color32::from_rgb(30, 31, 34) };
    if ty == DataType::Image {
        ui.painter().circle(m, r, fill, Stroke::new(1.5, c));
    } else {
        let pts = vec![m + vec2(0.0, -r * 1.2), m + vec2(r * 1.2, 0.0), m + vec2(0.0, r * 1.2), m + vec2(-r * 1.2, 0.0)];
        ui.painter().add(egui::Shape::convex_polygon(pts, fill, Stroke::new(1.5, c)));
    }
    if enabled {
        resp.on_hover_text(if exposed {
            "Exposed: this parameter has an input socket on the node. Click to hide it."
        } else {
            "Expose this parameter as an input socket on the node"
        })
    } else {
        resp.on_hover_text("This parameter cannot be exposed")
    }
}

fn message_box(ui: &mut Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.18))
        .stroke(Stroke::new(1.0, color))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text).color(color));
        });
}

pub fn show(
    ui: &mut Ui,
    graph: &mut Graph,
    engine: &mut Engine,
    selection: &BTreeSet<NodeId>,
    inputs_state: &mut InputsPanelState,
    state: &mut PropsState,
) -> PropsOutput {
    let mut out = PropsOutput::default();
    state.debug_rects.clear();
    ui.add_space(4.0);
    ui.heading("Properties");
    ui.separator();
    let selected: Vec<NodeId> = selection.iter().copied().filter(|id| graph.contains(*id)).collect();
    match selected.len() {
        0 => {
            ui.weak("Select a node to edit its parameters.");
            ui.add_space(6.0);
            ui.label(format!(
                "Graph: {} nodes, {} links, {} inputs, {}×{} px",
                graph.nodes.len(),
                graph.links.len(),
                graph.inputs.len(),
                graph.resolution[0],
                graph.resolution[1]
            ));
        }
        1 => {
            egui::ScrollArea::vertical().id_salt("props_scroll").auto_shrink([false, false]).show(ui, |ui| {
                node_properties(ui, graph, engine, selected[0], inputs_state, state, &mut out);
            });
        }
        n => {
            ui.label(format!("{n} nodes selected"));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Tiling for all:");
                for (i, label) in crate::graph::Tiling::LABELS.iter().enumerate() {
                    if ui.small_button(*label).clicked() {
                        for id in &selected {
                            if let Some(p) = graph.node_mut(*id).and_then(|n| n.param_mut("tiling")) {
                                p.value = ParamValue::Enum(i as u32);
                            }
                        }
                    }
                }
            });
        }
    }
    out
}

fn node_properties(
    ui: &mut Ui,
    graph: &mut Graph,
    engine: &mut Engine,
    id: NodeId,
    inputs_state: &mut InputsPanelState,
    state: &mut PropsState,
    out: &mut PropsOutput,
) {
    let Some(node) = graph.node(id).cloned() else { return };
    let d = node.def();
    let (error, warning, driven, vars) = match engine.state(id) {
        Some(s) => (s.error.clone(), s.warning.clone(), s.driven.clone(), s.vars.clone()),
        None => (None, None, Default::default(), Vec::new()),
    };

    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
        ui.painter().rect_filled(rect, CornerRadius::same(2), super::category_color(d.category));
        ui.label(RichText::new(super::graph_view::node_title(graph, &node)).heading().strong());
    });
    ui.weak(format!("{} · {}", d.name, d.category.name()));
    ui.weak(d.description);
    ui.horizontal(|ui| {
        ui.label("Label");
        let mut label = node.label.clone();
        if ui.add(egui::TextEdit::singleline(&mut label).hint_text(d.name).desired_width(f32::INFINITY)).changed() {
            if let Some(n) = graph.node_mut(id) {
                n.label = label;
            }
        }
    });
    if let Some(e) = &error {
        message_box(ui, e, ERROR);
    } else if let Some(w) = &warning {
        message_box(ui, w, WARNING);
    }
    if d.has_image && ui.button("Show in 2D view").clicked() {
        out.preview = Some(id);
    }
    ui.separator();

    // ---------------------------------------------------------------- parameters
    let value_node = d.is_value_node();
    egui::Grid::new(("params", id.0)).num_columns(3).spacing([6.0, 6.0]).striped(true).show(ui, |ui| {
        for pd in d.all_params() {
            if let Some((key, values)) = pd.visible_if {
                let current = driven
                    .get(key)
                    .map(|v| v.as_f32() as u32)
                    .unwrap_or_else(|| node.enum_value(key));
                if !values.contains(&current) {
                    continue;
                }
            }
            if pd.key == "expression" {
                continue; // dedicated editor below
            }
            let Some(slot) = node.param(pd.key).cloned() else { continue };
            let tiling_na = value_node && pd.key == "tiling";
            let label = ui.label(pd.label);
            if !pd.tooltip.is_empty() {
                label.on_hover_text(pd.tooltip);
            }
            let linked = slot.exposed && graph.link_into(id, &param_socket_key(pd.key)).is_some();
            ui.add_enabled_ui(!tiling_na, |ui| {
                ui.push_id(pd.key, |ui| {
                    if linked {
                        match driven.get(pd.key) {
                            Some(v) => driven_label(ui, pd, v),
                            None => {
                                ui.label(RichText::new("← (invalid input, using own value)").color(WARNING));
                            }
                        }
                    } else {
                        let mut v = slot.value.clone();
                        if param_editor(ui, pd, &mut v) {
                            if let Some(s) = graph.node_mut(id).and_then(|n| n.param_mut(pd.key)) {
                                s.value = v;
                            }
                        }
                    }
                })
            })
            .response
            .on_hover_text(if tiling_na { "Value nodes produce numbers, not images: tiling does not apply" } else { "" });
            let ty = slot.value.socket_type().unwrap_or(DataType::Float);
            let can = pd.can_expose() && !tiling_na;
            let toggle = expose_toggle(ui, slot.exposed, ty, can);
            state.debug_rects.insert(format!("expose:{}", pd.key), toggle.rect);
            if toggle.clicked() && can {
                graph.set_exposed(id, pd.key, !slot.exposed);
            }
            ui.end_row();
        }
    });

    // ---------------------------------------------------------------- node specific
    match node.kind {
        NodeKind::Math | NodeKind::PixelMath => expression_section(ui, graph, engine, &node, &vars, state),
        NodeKind::Input => input_section(ui, graph, &node, inputs_state),
        NodeKind::Levels => levels_section(ui, graph, engine, &node, &driven, state),
        NodeKind::Bitmap => {
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Browse…").clicked() {
                    out.browse_bitmap = Some(id);
                }
                if ui.button("Reload").clicked() {
                    engine.reload_bitmap(node.text("path"));
                }
            });
        }
        NodeKind::Output => {
            ui.separator();
            ui.label("The output image is written with the selected channels and bit depth.");
            if ui.button("Export this output…").clicked() {
                out.export_node = Some(id);
            }
        }
        NodeKind::Transform2D => {
            ui.separator();
            ui.weak("Tip: drag the gizmo in the 2D view (center moves, ring rotates, corners scale, Shift = uniform).");
            if ui.button("Reset transform").clicked() {
                if let Some(n) = graph.node_mut(id) {
                    for (k, v) in [
                        ("offset", ParamValue::Vec2([0.0, 0.0])),
                        ("rotation", ParamValue::Float(0.0)),
                        ("scale", ParamValue::Vec2([1.0, 1.0])),
                        ("pivot", ParamValue::Vec2([0.5, 0.5])),
                    ] {
                        if let Some(p) = n.param_mut(k) {
                            p.value = v;
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn expression_section(ui: &mut Ui, graph: &mut Graph, engine: &Engine, node: &Node, vars: &[crate::engine::VarInfo], state: &mut PropsState) {
    let id = node.id;
    ui.separator();
    ui.label(RichText::new("Expression").strong());
    let src = node.text("expression").to_string();
    let compiled = expr::compile_cached(&src);
    let err_span = compiled.as_ref().as_ref().err().map(|e| e.span);
    let mut text = src.clone();
    let editor = expression_editor(ui, &mut text, err_span);
    state.debug_rects.insert("expr".into(), editor.rect);
    if editor.changed() {
        if let Some(p) = graph.node_mut(id).and_then(|n| n.param_mut("expression")) {
            p.value = ParamValue::Text(text);
        }
    }
    match &*compiled {
        Err(e) => {
            message_box(ui, &format!("The expression is incorrect and needs fixing:\n{}", e.message), ERROR);
        }
        Ok(_) if node.kind == NodeKind::Math => {
            if let Some(v) = engine.value(id) {
                ui.label(RichText::new(format!("= {}", v.display())).monospace().size(16.0).color(Color32::WHITE));
            }
        }
        Ok(_) => {}
    }

    // Variables.
    if !vars.is_empty() {
        ui.add_space(4.0);
        ui.label(RichText::new("Variables").strong());
        egui::Grid::new(("vars", id.0)).num_columns(3).spacing([8.0, 4.0]).striped(true).show(ui, |ui| {
            for v in vars {
                ui.label(RichText::new(&v.name).monospace().color(Color32::from_rgb(156, 220, 254)));
                match &v.source {
                    VarSource::Local => {
                        let mut value = graph.node(id).and_then(|n| n.var_values.get(&v.name).copied()).unwrap_or(0.0);
                        ui.push_id(&v.name, |ui| {
                            if ui.add(egui::DragValue::new(&mut value).speed(0.01).max_decimals(4)).changed() {
                                if let Some(n) = graph.node_mut(id) {
                                    n.var_values.insert(v.name.clone(), value);
                                }
                            }
                        });
                        ui.weak("local value");
                    }
                    VarSource::Link(from) => {
                        ui.label(format!("{}", crate::graph::Value::Float(v.value as f32).display()));
                        let name = graph.node(*from).map(|n| super::graph_view::node_title(graph, n)).unwrap_or_default();
                        ui.weak(format!("from {name}"));
                    }
                    VarSource::GraphInput(name) => {
                        ui.label(format!("{}", crate::graph::Value::Float(v.value as f32).display()));
                        ui.weak(format!("graph input '{name}'"));
                    }
                    VarSource::Builtin => {
                        ui.label("per pixel");
                        ui.weak("built-in");
                    }
                    VarSource::Image(from) => {
                        ui.label("image");
                        let name = graph.node(*from).map(|n| super::graph_view::node_title(graph, n)).unwrap_or_default();
                        ui.weak(format!("from {name}"));
                    }
                }
                ui.end_row();
            }
        });
        ui.weak("Unconnected variables use their local value, or a graph input with the same name when auto-bind is on.");
    }

    // Plot of the Math expression over one variable.
    if node.kind == NodeKind::Math {
        if let Ok(program) = &*compiled {
            if !program.vars.is_empty() {
                egui::CollapsingHeader::new("Plot").id_salt(("plot", id.0)).default_open(true).show(ui, |ui| {
                    plot_expression(ui, program, vars, state);
                });
            }
        }
    }

    egui::CollapsingHeader::new("Functions & constants").id_salt(("funcs", id.0)).show(ui, |ui| {
        if node.kind == NodeKind::PixelMath {
            ui.label(RichText::new(format!("Per-pixel built-ins: {}", PIXEL_MATH_BUILTINS.join(", "))).strong());
            ui.weak("u, v: 0..1 image coordinates; px, py: pixel coordinates; width, height: image size.");
            ui.add_space(4.0);
        }
        ui.label("Operators: + - * / % ^ (or **), comparisons < <= > >= == !=, && || !, cond ? a : b, implicit 2x");
        egui::Grid::new(("funcs_grid", id.0)).num_columns(2).spacing([10.0, 2.0]).striped(true).show(ui, |ui| {
            for f in builtins::FUNCTIONS {
                ui.label(RichText::new(f.signature).monospace());
                ui.weak(f.doc);
                ui.end_row();
            }
            for c in builtins::CONSTANTS {
                ui.label(RichText::new(c.name).monospace());
                ui.weak(c.doc);
                ui.end_row();
            }
        });
    });
}

fn plot_expression(ui: &mut Ui, program: &expr::Program, vars: &[crate::engine::VarInfo], state: &mut PropsState) {
    if state.plot_var >= program.vars.len() {
        state.plot_var = 0;
    }
    let (mut lo, mut hi) = state.plot_range.unwrap_or((-5.0, 5.0));
    ui.horizontal(|ui| {
        ui.label("f(");
        egui::ComboBox::from_id_salt("plot_var")
            .selected_text(program.vars[state.plot_var].as_str())
            .show_ui(ui, |ui| {
                for (i, v) in program.vars.iter().enumerate() {
                    ui.selectable_value(&mut state.plot_var, i, v.as_str());
                }
            });
        ui.label(") for");
        ui.add(egui::DragValue::new(&mut lo).speed(0.1).max_decimals(2));
        ui.label("…");
        ui.add(egui::DragValue::new(&mut hi).speed(0.1).max_decimals(2));
    });
    if hi <= lo {
        hi = lo + 0.1;
    }
    state.plot_range = Some((lo, hi));

    let mut args: Vec<f64> = program
        .vars
        .iter()
        .map(|name| vars.iter().find(|v| &v.name == name).map_or(0.0, |v| v.value))
        .collect();
    let current = args[state.plot_var];
    let n = 256;
    let samples: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let x = lo as f64 + (hi - lo) as f64 * i as f64 / (n - 1) as f64;
            args[state.plot_var] = x;
            (x, program.eval(&args))
        })
        .collect();
    let finite: Vec<f64> = samples.iter().map(|s| s.1).filter(|v| v.is_finite()).collect();
    let (mut ymin, mut ymax) = finite.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
    if !ymin.is_finite() {
        ymin = -1.0;
        ymax = 1.0;
    }
    if (ymax - ymin).abs() < 1e-9 {
        ymin -= 1.0;
        ymax += 1.0;
    }
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 120.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(3), Color32::from_rgb(22, 23, 25));
    let to = |x: f64, y: f64| -> Pos2 {
        pos2(
            rect.left() + ((x - lo as f64) / (hi - lo) as f64) as f32 * rect.width(),
            rect.bottom() - 6.0 - ((y - ymin) / (ymax - ymin)) as f32 * (rect.height() - 12.0),
        )
    };
    if ymin < 0.0 && ymax > 0.0 {
        p.hline(rect.x_range(), to(0.0, 0.0).y, Stroke::new(1.0, Color32::from_gray(60)));
    }
    if (lo as f64) < 0.0 && (hi as f64) > 0.0 {
        p.vline(to(0.0, 0.0).x, rect.y_range(), Stroke::new(1.0, Color32::from_gray(60)));
    }
    // Break the polyline at non-finite values and at asymptote jumps.
    let mut segment: Vec<Pos2> = Vec::new();
    let span = ymax - ymin;
    for w in samples.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a.1.is_finite() && b.1.is_finite() && (b.1 - a.1).abs() < span * 0.9 {
            if segment.is_empty() {
                segment.push(to(a.0, a.1));
            }
            segment.push(to(b.0, b.1));
        } else if !segment.is_empty() {
            p.line(std::mem::take(&mut segment), Stroke::new(1.8, ACCENT));
        }
    }
    if !segment.is_empty() {
        p.line(segment, Stroke::new(1.8, ACCENT));
    }
    if current >= lo as f64 && current <= hi as f64 {
        p.vline(to(current, 0.0).x, rect.y_range(), Stroke::new(1.0, Color32::from_rgb(90, 180, 255)));
    }
    let font = FontId::monospace(10.0);
    p.text(rect.left_top() + vec2(4.0, 2.0), Align2::LEFT_TOP, format!("{ymax:.3}"), font.clone(), Color32::GRAY);
    p.text(rect.left_bottom() + vec2(4.0, -2.0), Align2::LEFT_BOTTOM, format!("{ymin:.3}"), font, Color32::GRAY);
}

fn input_section(ui: &mut Ui, graph: &mut Graph, node: &Node, inputs_state: &mut InputsPanelState) {
    ui.separator();
    ui.label(RichText::new("Graph input").strong());
    let current = node.input_ref;
    let label = current
        .and_then(|i| graph.input(i))
        .map_or("(missing)".to_string(), |i| format!("{} ({})", i.name, i.ty.name()));
    let mut chosen: Option<InputId> = None;
    egui::ComboBox::from_id_salt(("bind", node.id.0)).selected_text(label).show_ui(ui, |ui| {
        for input in &graph.inputs {
            if ui.selectable_label(current == Some(input.id), format!("{} ({})", input.name, input.ty.name())).clicked() {
                chosen = Some(input.id);
            }
        }
    });
    if let Some(c) = chosen {
        if let Some(n) = graph.node_mut(node.id) {
            n.input_ref = Some(c);
        }
        graph.prune_links();
    }
    let users = graph.nodes.iter().filter(|n| n.kind == NodeKind::Input && n.input_ref == current).count();
    let mut remove = None;
    match current.and_then(|i| graph.input_mut(i)) {
        Some(input) => {
            ui.horizontal(|ui| {
                ui.label("Value");
                let (min, max) = (input.min, input.max);
                if super::widgets::value_editor(ui, &mut input.value, &min, &max) {
                    input.value = input.value.clamped(&min, &max);
                }
            });
            input_details(ui, input, users, inputs_state, &mut remove);
        }
        None => {
            message_box(ui, "This node reads a graph input that no longer exists. Pick another one above.", ERROR);
        }
    }
    if let Some(id) = remove {
        graph.remove_input(id);
    }
}

/// Levels: histogram of the input with draggable black point / gamma / white point.
fn levels_section(
    ui: &mut Ui,
    graph: &mut Graph,
    engine: &mut Engine,
    node: &Node,
    driven: &std::collections::BTreeMap<String, ParamValue>,
    state: &mut PropsState,
) {
    ui.separator();
    ui.label(RichText::new("Input histogram").strong());
    let Some(source) = graph.link_into(node.id, "i:input").map(|l| l.from) else {
        ui.weak("Connect an input to see its histogram.");
        return;
    };
    let Some((stats, fresh)) = engine.stats(source) else {
        ui.weak("Computing histogram…");
        return;
    };
    let get = |k: &str| -> f32 { driven.get(k).cloned().unwrap_or_else(|| node.param(k).map_or(ParamValue::Float(0.0), |p| p.value.clone())).as_f32() };
    let (lo, hi, gamma) = (get("in_low"), get("in_high"), get("gamma").max(1e-3));

    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 110.0), Sense::click_and_drag());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(3), Color32::from_rgb(20, 21, 23));
    let plot = rect.shrink2(vec2(6.0, 4.0));
    let colors = [
        Color32::from_rgba_unmultiplied(230, 80, 80, 110),
        Color32::from_rgba_unmultiplied(80, 210, 110, 110),
        Color32::from_rgba_unmultiplied(90, 140, 240, 110),
    ];
    let max_count = (0..5).filter(|c| *c != 3).map(|c| stats.channel_max_count(c)).max().unwrap_or(1).max(1) as f32;
    let bar_w = plot.width() / 256.0;
    let scale = |count: u32| ((count as f32 + 1.0).ln() / (max_count + 1.0).ln()) * plot.height();
    for (c, color) in colors.iter().enumerate() {
        for (i, count) in stats.hist[c].iter().enumerate() {
            if *count == 0 {
                continue;
            }
            let x = plot.left() + i as f32 * bar_w;
            let h = scale(*count);
            p.rect_filled(Rect::from_min_max(pos2(x, plot.bottom() - h), pos2(x + bar_w.max(1.0), plot.bottom())), CornerRadius::ZERO, *color);
        }
    }
    let mut lum_line = Vec::with_capacity(256);
    for (i, count) in stats.hist[4].iter().enumerate() {
        lum_line.push(pos2(plot.left() + (i as f32 + 0.5) * bar_w, plot.bottom() - scale(*count)));
    }
    p.line(lum_line, Stroke::new(1.2, Color32::from_gray(230)));
    if !fresh {
        p.text(rect.right_top() + vec2(-4.0, 2.0), Align2::RIGHT_TOP, "updating…", FontId::proportional(10.0), Color32::GRAY);
    }

    // Handles: 0 = black point, 1 = gamma (mid gray), 2 = white point.
    let mid = lo + (hi - lo) * 0.5f32.powf(gamma);
    let xs = [lo, mid, hi].map(|v| plot.left() + v.clamp(0.0, 1.0) * plot.width());
    for (i, x) in xs.iter().enumerate() {
        let c = [Color32::BLACK, Color32::from_gray(128), Color32::WHITE][i];
        let tri = vec![pos2(*x, plot.bottom() - 10.0), pos2(x - 6.0, plot.bottom() + 2.0), pos2(x + 6.0, plot.bottom() + 2.0)];
        p.add(egui::Shape::convex_polygon(tri, c, Stroke::new(1.0, ACCENT)));
        p.vline(*x, plot.y_range(), Stroke::new(1.0, Color32::from_white_alpha(40)));
    }
    let locked = |k: &str| driven.contains_key(k);
    if resp.drag_started() {
        if let Some(pos) = resp.interact_pointer_pos() {
            state.levels_drag = xs
                .iter()
                .enumerate()
                .min_by(|a, b| (a.1 - pos.x).abs().total_cmp(&(b.1 - pos.x).abs()))
                .map(|(i, _)| i);
        }
    }
    if resp.dragged() {
        if let (Some(h), Some(pos)) = (state.levels_drag, resp.interact_pointer_pos()) {
            let v = ((pos.x - plot.left()) / plot.width()).clamp(0.0, 1.0);
            let set = |graph: &mut Graph, k: &str, v: f32| {
                if let Some(s) = graph.node_mut(node.id).and_then(|n| n.param_mut(k)) {
                    s.value = ParamValue::Float(v);
                }
            };
            match h {
                0 if !locked("in_low") => set(graph, "in_low", v.min(hi - 0.01)),
                2 if !locked("in_high") => set(graph, "in_high", v.max(lo + 0.01)),
                1 if !locked("gamma") => {
                    let t = ((v - lo) / (hi - lo)).clamp(0.01, 0.99);
                    set(graph, "gamma", (t.ln() / 0.5f32.ln()).clamp(0.1, 10.0));
                }
                _ => {}
            }
        }
    }
    if resp.drag_stopped() {
        state.levels_drag = None;
    }

    ui.horizontal(|ui| {
        if ui.button("Auto levels").on_hover_text("Stretch the input range to the darkest and brightest pixel").clicked() {
            let mn = stats.min[..3].iter().copied().fold(f32::INFINITY, f32::min).clamp(0.0, 1.0);
            let mx = stats.max[..3].iter().copied().fold(f32::NEG_INFINITY, f32::max).clamp(0.0, 1.0);
            if mx > mn {
                for (k, v) in [("in_low", mn), ("in_high", mx), ("gamma", 1.0)] {
                    if let Some(s) = graph.node_mut(node.id).and_then(|n| n.param_mut(k)) {
                        s.value = ParamValue::Float(v);
                    }
                }
            }
        }
        ui.weak(format!("{}×{}", stats.width, stats.height));
    });
    egui::Grid::new(("stats", node.id.0)).num_columns(4).spacing([10.0, 2.0]).show(ui, |ui| {
        ui.label("");
        ui.weak("min");
        ui.weak("max");
        ui.weak("mean");
        ui.end_row();
        for (c, name) in ["R", "G", "B", "A"].iter().enumerate() {
            ui.label(RichText::new(*name).color(color32([
                if c == 0 { 1.0 } else { 0.6 },
                if c == 1 { 1.0 } else { 0.6 },
                if c == 2 { 1.0 } else { 0.6 },
                1.0,
            ])));
            ui.monospace(format!("{:.3}", stats.min[c]));
            ui.monospace(format!("{:.3}", stats.max[c]));
            ui.monospace(format!("{:.3}", stats.mean[c]));
            ui.end_row();
        }
    });
    let _ = StrokeKind::Inside;
}
