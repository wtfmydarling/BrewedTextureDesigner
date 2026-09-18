//! User interface: panels, node canvas and 2D view.

pub mod graph_view;
pub mod inputs_panel;
pub mod library;
pub mod preview;
pub mod properties;
pub mod widgets;

use eframe::egui::{self, Color32};

use crate::graph::defs::LibraryItem;
use crate::graph::{Category, DataType, Graph, InputId, NodeId};

/// What can be dragged onto the node canvas.
#[derive(Clone, Debug)]
pub enum DragPayload {
    Library(LibraryItem),
    GraphInput(InputId),
}

pub fn type_color(ty: DataType) -> Color32 {
    match ty {
        DataType::Image => Color32::from_rgb(205, 205, 205),
        DataType::Float => Color32::from_rgb(90, 180, 255),
        DataType::Int => Color32::from_rgb(80, 210, 140),
        DataType::Bool => Color32::from_rgb(255, 110, 110),
        DataType::Vec2 => Color32::from_rgb(255, 205, 100),
        DataType::Color => Color32::from_rgb(200, 146, 234),
    }
}

pub fn category_color(c: Category) -> Color32 {
    match c {
        Category::Values => Color32::from_rgb(150, 112, 34),
        Category::Generators => Color32::from_rgb(46, 118, 78),
        Category::Filters => Color32::from_rgb(44, 92, 146),
        Category::Adjustments => Color32::from_rgb(110, 74, 150),
        Category::Output => Color32::from_rgb(150, 58, 58),
    }
}

pub const ACCENT: Color32 = Color32::from_rgb(240, 160, 48);
pub const ERROR: Color32 = Color32::from_rgb(235, 80, 70);
pub const WARNING: Color32 = Color32::from_rgb(230, 190, 70);

/// Dark theme tuned for a node editor.
pub fn apply_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(30, 31, 34);
    visuals.window_fill = Color32::from_rgb(36, 37, 41);
    visuals.extreme_bg_color = Color32::from_rgb(22, 23, 25);
    visuals.faint_bg_color = Color32::from_rgb(38, 39, 43);
    visuals.selection.bg_fill = Color32::from_rgb(170, 105, 30);
    visuals.selection.stroke = egui::Stroke::new(1.0, Color32::from_rgb(255, 210, 150));
    visuals.hyperlink_color = ACCENT;
    ctx.set_visuals(visuals);
    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.slider_width = 150.0;
        style.spacing.interact_size.y = 20.0;
    });
}

/// Default name of a new graph input of type `ty`.
pub fn input_base_name(ty: DataType) -> &'static str {
    match ty {
        DataType::Float => "value",
        DataType::Int => "count",
        DataType::Bool => "toggle",
        DataType::Vec2 => "vector",
        DataType::Color => "color",
        DataType::Image => "input",
    }
}

/// Create the node described by a library entry at graph position `pos`.
pub fn create_from_library(graph: &mut Graph, item: &LibraryItem, pos: [f32; 2]) -> NodeId {
    match item.input_type {
        Some(ty) => {
            let input = graph.add_input(ty, input_base_name(ty));
            graph.add_input_node(input, pos)
        }
        None => graph.add_node(item.kind, pos),
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::{self, FontId};

    /// Every non-ASCII character shown by the UI must exist in egui's default fonts,
    /// otherwise it renders as a box.
    #[test]
    fn ui_text_has_glyphs() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |_| {});
        output.textures_delta.clear();
        let sources = [
            include_str!("graph_view.rs"),
            include_str!("inputs_panel.rs"),
            include_str!("library.rs"),
            include_str!("preview.rs"),
            include_str!("properties.rs"),
            include_str!("widgets.rs"),
            include_str!("../app.rs"),
            include_str!("../graph/defs.rs"),
            include_str!("../expr/builtins.rs"),
            include_str!("../engine/mod.rs"),
        ];
        let mut missing = std::collections::BTreeSet::new();
        for src in sources {
            for line in src.lines() {
                let code = line.trim_start();
                if code.starts_with("//") {
                    continue;
                }
                for c in code.chars().filter(|c| !c.is_ascii()) {
                    for font in [FontId::proportional(14.0)] {
                        if !ctx.fonts_mut(|f| f.has_glyph(&font, c)) {
                            missing.insert((c, font.family.to_string()));
                        }
                    }
                }
            }
        }
        assert!(missing.is_empty(), "characters without glyphs: {missing:?}");
    }
}
