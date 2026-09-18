//! Left panel: every node the program offers, grouped by category.
//! Items are dragged onto the canvas (or double-clicked to add at the view center).

use eframe::egui::{self, Color32, CornerRadius, RichText, Sense, Ui, vec2};

use super::{DragPayload, category_color};
use crate::graph::defs::{LibraryItem, library};
use crate::graph::{Category, def};

#[derive(Default)]
pub struct LibraryState {
    pub search: String,
    /// Screen rectangles of the entries drawn last frame (used by the UI self-test).
    pub item_rects: Vec<(&'static str, egui::Rect)>,
}

/// Returns an item that was double-clicked (to be added at the view center).
pub fn show(ui: &mut Ui, state: &mut LibraryState) -> Option<LibraryItem> {
    state.item_rects.clear();
    let mut chosen = None;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.heading("Nodes");
    });
    ui.add(egui::TextEdit::singleline(&mut state.search).hint_text("Search…").desired_width(f32::INFINITY));
    ui.add_space(4.0);
    let query = state.search.trim().to_lowercase();
    let items = library();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for cat in Category::ALL {
            let matching: Vec<&LibraryItem> = items
                .iter()
                .filter(|i| i.category == cat)
                .filter(|i| {
                    query.is_empty()
                        || i.label.to_lowercase().contains(&query)
                        || def(i.kind).description.to_lowercase().contains(&query)
                })
                .collect();
            if matching.is_empty() {
                continue;
            }
            let header = RichText::new(cat.name()).strong();
            egui::CollapsingHeader::new(header)
                .id_salt(("library_cat", cat.name()))
                .default_open(true)
                .open(if query.is_empty() { None } else { Some(true) })
                .show(ui, |ui| {
                    for item in matching {
                        if item_widget(ui, item, &mut state.item_rects) {
                            chosen = Some(*item);
                        }
                    }
                });
        }
        ui.add_space(8.0);
        ui.weak("Drag a node onto the graph, or double-click it.");
    });
    chosen
}

/// One draggable library entry. Returns true when double-clicked.
fn item_widget(ui: &mut Ui, item: &LibraryItem, rects: &mut Vec<(&'static str, egui::Rect)>) -> bool {
    let id = ui.id().with(("lib_item", item.label));
    let payload = DragPayload::Library(*item);
    let mut double = false;
    let response = ui
        .dnd_drag_source(id, payload, |ui| {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
            let visuals = ui.style().interact(&resp);
            if resp.hovered() {
                ui.painter().rect_filled(rect, CornerRadius::same(3), visuals.bg_fill);
            }
            let swatch = egui::Rect::from_min_size(rect.min + vec2(4.0, 6.0), vec2(10.0, 10.0));
            ui.painter().rect_filled(swatch, CornerRadius::same(2), category_color(item.category));
            if let Some(ty) = item.input_type {
                ui.painter().circle_filled(swatch.center(), 3.0, super::type_color(ty));
            }
            ui.painter().text(
                rect.min + vec2(22.0, 11.0),
                egui::Align2::LEFT_CENTER,
                item.label,
                egui::FontId::proportional(13.5),
                if resp.hovered() { Color32::WHITE } else { ui.visuals().text_color() },
            );
            double = resp.double_clicked() || resp.triple_clicked();
            resp
        })
        .response;
    let desc = def(item.kind).description;
    let tip = match item.input_type {
        Some(ty) => format!("Creates a new {} graph input and a node that reads it.", ty.name()),
        None => desc.to_string(),
    };
    rects.push((item.label, response.rect));
    response.on_hover_text(tip);
    double
}
