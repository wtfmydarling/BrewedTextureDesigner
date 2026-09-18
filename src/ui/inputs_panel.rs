//! Right panel, top half: the graph's input values.
//!
//! Every input has a type, a current value, a default and (for numeric types) a
//! min / max range. Input nodes in the graph read these values; dragging an input
//! from this panel onto the canvas creates such a node.

use std::collections::HashSet;

use eframe::egui::{self, Color32, RichText, Ui};

use super::widgets::{disclosure, grab_handle, raw_value_editor, value_editor};
use super::{DragPayload, input_base_name, type_color};
use crate::graph::{DataType, Graph, GraphInput, InputId, NodeKind, sanitize_identifier};

#[derive(Default)]
pub struct InputsPanelState {
    pub expanded: HashSet<InputId>,
    /// Name being edited (committed on focus loss / Enter).
    name_edit: Option<(InputId, String)>,
    /// Screen rectangles of interactive widgets (used by the UI self-test).
    pub debug_rects: std::collections::HashMap<String, egui::Rect>,
}

pub fn show(ui: &mut Ui, graph: &mut Graph, state: &mut InputsPanelState) {
    state.debug_rects.clear();
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.heading("Input Values");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let add = ui.menu_button("+ Add", |ui| {
                for ty in DataType::VALUE_TYPES {
                    let b = ui.button(ty.name());
                    state.debug_rects.insert(format!("add:{}", ty.name()), b.rect);
                    if b.clicked() {
                        let id = graph.add_input(ty, input_base_name(ty));
                        state.expanded.insert(id);
                        ui.close();
                    }
                }
            });
            state.debug_rects.insert("add".into(), add.response.rect);
        });
    });
    ui.separator();
    if graph.inputs.is_empty() {
        ui.weak("No inputs yet. Add one with \"+ Add\" or drag an Input node from the library.");
        return;
    }
    let mut remove: Option<InputId> = None;
    egui::ScrollArea::vertical().id_salt("inputs_scroll").auto_shrink([false, false]).show(ui, |ui| {
        let ids: Vec<InputId> = graph.inputs.iter().map(|i| i.id).collect();
        for id in ids {
            let users = graph.nodes.iter().filter(|n| n.kind == NodeKind::Input && n.input_ref == Some(id)).count();
            let Some(input) = graph.input_mut(id) else { continue };
            ui.push_id(("input", id.0), |ui| {
                input_row(ui, input, users, state, &mut remove);
            });
        }
    });
    if let Some(id) = remove {
        graph.remove_input(id);
        state.expanded.remove(&id);
    }
    // Names must stay unique identifiers (Math nodes can refer to them).
    let names: Vec<(InputId, String)> = graph.inputs.iter().map(|i| (i.id, i.name.clone())).collect();
    for (id, name) in names {
        let unique = graph.unique_input_name(&name, Some(id));
        if unique != name {
            if let Some(i) = graph.input_mut(id) {
                i.name = unique;
            }
        }
    }
}

fn input_row(ui: &mut Ui, input: &mut GraphInput, users: usize, state: &mut InputsPanelState, remove: &mut Option<InputId>) {
    let expanded = state.expanded.contains(&input.id);
    ui.horizontal(|ui| {
        // Drag handle: drop on the canvas to create an input node.
        let id = ui.id().with("drag");
        ui.dnd_drag_source(id, DragPayload::GraphInput(input.id), |ui| {
            grab_handle(ui);
        })
        .response
        .on_hover_text("Drag onto the graph to create an Input node");
        let arrow = disclosure(ui, expanded);
        let label = RichText::new(&input.name).color(type_color(input.ty)).strong();
        let r = ui.add(egui::Label::new(label).sense(egui::Sense::click()));
        if r.clicked() || arrow.clicked() {
            if expanded {
                state.expanded.remove(&input.id);
            } else {
                state.expanded.insert(input.id);
            }
        }
        let tip = if input.description.is_empty() {
            format!("{} input, used by {users} node(s). Click to edit its settings.", input.ty.name())
        } else {
            format!("{}\n{} input, used by {users} node(s).", input.description, input.ty.name())
        };
        r.on_hover_text(tip);
    });
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        let (min, max) = (input.min, input.max);
        if value_editor(ui, &mut input.value, &min, &max) {
            input.value = input.value.clamped(&min, &max);
        }
    });
    if expanded {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            input_details(ui, input, users, state, remove);
        });
    }
    ui.add_space(2.0);
}

/// Full settings of a graph input (also used by the properties of Input nodes).
pub fn input_details(
    ui: &mut Ui,
    input: &mut GraphInput,
    users: usize,
    state: &mut InputsPanelState,
    remove: &mut Option<InputId>,
) {
    egui::Grid::new(("input_details", input.id.0)).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Name");
        let editing = state.name_edit.as_ref().is_some_and(|(id, _)| *id == input.id);
        let mut text = if editing { state.name_edit.as_ref().map(|(_, s)| s.clone()).unwrap_or_default() } else { input.name.clone() };
        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(140.0));
        if r.changed() || r.has_focus() {
            state.name_edit = Some((input.id, text.clone()));
        }
        if r.lost_focus() {
            let clean = sanitize_identifier(&text);
            if !clean.is_empty() {
                input.name = clean;
            }
            state.name_edit = None;
        }
        ui.end_row();

        ui.label("Type");
        let mut ty = input.ty;
        egui::ComboBox::from_id_salt(("input_type", input.id.0)).selected_text(ty.name()).show_ui(ui, |ui| {
            for t in DataType::VALUE_TYPES {
                ui.selectable_value(&mut ty, t, t.name());
            }
        });
        if ty != input.ty {
            input.set_type(ty);
        }
        ui.end_row();

        ui.label("Default");
        let mut d = input.default;
        if raw_value_editor(ui, &mut d) {
            input.default = d.clamped(&input.min, &input.max);
        }
        ui.end_row();

        if input.has_range() {
            ui.label("Min");
            let mut m = input.min;
            if raw_value_editor(ui, &mut m) {
                input.min = m;
                input.normalize();
            }
            ui.end_row();
            ui.label("Max");
            let mut m = input.max;
            if raw_value_editor(ui, &mut m) {
                input.max = m;
                input.normalize();
            }
            ui.end_row();
            if range_inverted(input) {
                ui.label("");
                ui.colored_label(super::WARNING, "Min is greater than Max");
                ui.end_row();
            }
        }

        ui.label("Description");
        ui.add(egui::TextEdit::singleline(&mut input.description).desired_width(140.0));
        ui.end_row();
    });
    ui.horizontal(|ui| {
        if ui.button("Reset to default").clicked() {
            input.value = input.default;
        }
        if ui.button("Set as default").on_hover_text("Use the current value as the default").clicked() {
            input.default = input.value;
        }
        let del = ui.add(egui::Button::new(RichText::new("Delete").color(Color32::from_rgb(240, 120, 110))));
        let del = if users > 0 {
            del.on_hover_text(format!("{users} Input node(s) read this value; they will show an error until rebound"))
        } else {
            del
        };
        if del.clicked() {
            *remove = Some(input.id);
        }
    });
}

fn range_inverted(input: &GraphInput) -> bool {
    use crate::graph::Value;
    match (input.min, input.max) {
        (Value::Float(a), Value::Float(b)) => a > b,
        (Value::Int(a), Value::Int(b)) => a > b,
        (Value::Vec2(a), Value::Vec2(b)) => a[0] > b[0] || a[1] > b[1],
        _ => false,
    }
}
