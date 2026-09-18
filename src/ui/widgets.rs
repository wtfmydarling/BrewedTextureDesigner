//! Small reusable widgets: parameter editors, value editors, expression editor.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, FontId, Response, Sense, Stroke, TextFormat, Ui, text::LayoutJob, widgets::text_edit::TextBuffer,
};

use crate::expr::{self, TokenClass};
use crate::graph::{DataType, ParamDef, ParamKind, ParamValue, Value};

/// Display color for an RGBA value (values are shown as-is, like the 2D view).
pub fn color32(c: [f32; 4]) -> Color32 {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    Color32::from_rgba_unmultiplied(q(c[0]), q(c[1]), q(c[2]), q(c[3]))
}

fn speed_for(min: f32, max: f32) -> f64 {
    ((max - min).abs() as f64 / 300.0).max(0.001)
}

/// Editor for a node parameter. Returns true when the value changed.
pub fn param_editor(ui: &mut Ui, def: &ParamDef, value: &mut ParamValue) -> bool {
    let width = ui.available_width().max(60.0);
    match (def.kind, value) {
        (ParamKind::Float { min, max, log, suffix, .. }, ParamValue::Float(v)) => {
            ui.spacing_mut().slider_width = (width - 70.0).max(40.0);
            ui.add(
                egui::Slider::new(v, min..=max)
                    .logarithmic(log)
                    .clamping(egui::SliderClamping::Never)
                    .suffix(suffix)
                    .max_decimals(3),
            )
            .changed()
        }
        (ParamKind::Int { min, max, .. }, ParamValue::Int(v)) => {
            ui.spacing_mut().slider_width = (width - 70.0).max(40.0);
            ui.add(egui::Slider::new(v, min..=max).clamping(egui::SliderClamping::Never)).changed()
        }
        (ParamKind::Bool { .. }, ParamValue::Bool(v)) => ui.checkbox(v, "").changed(),
        (ParamKind::Vec2 { min, max, .. }, ParamValue::Vec2(v)) => {
            let speed = speed_for(min, max);
            let mut changed = false;
            ui.horizontal(|ui| {
                let w = ((width - 12.0) / 2.0).max(40.0);
                changed |= ui.add_sized([w, 20.0], egui::DragValue::new(&mut v[0]).speed(speed).prefix("x ").max_decimals(3)).changed();
                changed |= ui.add_sized([w, 20.0], egui::DragValue::new(&mut v[1]).speed(speed).prefix("y ").max_decimals(3)).changed();
            });
            changed
        }
        (ParamKind::Color { .. }, ParamValue::Color(c)) => {
            let mut changed = ui.color_edit_button_rgba_unmultiplied(c).changed();
            ui.horizontal(|ui| {
                for (i, label) in ["r", "g", "b", "a"].iter().enumerate() {
                    changed |= ui
                        .add(egui::DragValue::new(&mut c[i]).speed(0.005).range(0.0..=1.0).prefix(*label).max_decimals(2))
                        .changed();
                }
            });
            changed
        }
        (ParamKind::Enum { options, .. }, ParamValue::Enum(v)) => {
            let before = *v;
            let text = options.get(*v as usize).copied().unwrap_or("?");
            egui::ComboBox::from_id_salt(ui.id().with(("enum", def.key)))
                .selected_text(text)
                .width(width - 8.0)
                .show_ui(ui, |ui| {
                    for (i, o) in options.iter().enumerate() {
                        ui.selectable_value(v, i as u32, *o);
                    }
                });
            *v != before
        }
        (ParamKind::Text { .. }, ParamValue::Text(s)) => {
            ui.add(egui::TextEdit::singleline(s).desired_width(width)).changed()
        }
        (_, v) => {
            ui.label(format!("{v:?}"));
            false
        }
    }
}

/// Read-only display of a driven parameter value.
pub fn driven_label(ui: &mut Ui, def: &ParamDef, value: &ParamValue) {
    let text = match value {
        ParamValue::Enum(i) => def.enum_options().get(*i as usize).copied().unwrap_or("?").to_string(),
        other => other.as_value().map_or_else(|| format!("{other:?}"), |v| v.display()),
    };
    ui.label(egui::RichText::new(format!("⇐ {text}")).color(super::type_color(value.socket_type().unwrap_or(DataType::Float))))
        .on_hover_text("Driven by the connected input socket");
}

/// Editor for a graph input value within its [min, max] range.
pub fn value_editor(ui: &mut Ui, value: &mut Value, min: &Value, max: &Value) -> bool {
    let width = ui.available_width().max(60.0);
    match (value, min, max) {
        (Value::Float(v), Value::Float(a), Value::Float(b)) => {
            ui.spacing_mut().slider_width = (width - 70.0).max(40.0);
            if a < b {
                ui.add(egui::Slider::new(v, *a..=*b).max_decimals(3)).changed()
            } else {
                ui.add(egui::DragValue::new(v).speed(0.01)).changed()
            }
        }
        (Value::Int(v), Value::Int(a), Value::Int(b)) => {
            ui.spacing_mut().slider_width = (width - 70.0).max(40.0);
            if a < b {
                ui.add(egui::Slider::new(v, *a..=*b)).changed()
            } else {
                ui.add(egui::DragValue::new(v)).changed()
            }
        }
        (Value::Bool(v), _, _) => ui.checkbox(v, if *v { "true" } else { "false" }).changed(),
        (Value::Vec2(v), Value::Vec2(a), Value::Vec2(b)) => {
            let mut changed = false;
            ui.horizontal(|ui| {
                let w = ((width - 12.0) / 2.0).max(40.0);
                for i in 0..2 {
                    let mut dv = egui::DragValue::new(&mut v[i])
                        .speed(speed_for(a[i], b[i]))
                        .prefix(if i == 0 { "x " } else { "y " })
                        .max_decimals(3);
                    if a[i] < b[i] {
                        dv = dv.range(a[i]..=b[i]);
                    }
                    changed |= ui.add_sized([w, 20.0], dv).changed();
                }
            });
            changed
        }
        (Value::Color(c), _, _) => ui.color_edit_button_rgba_unmultiplied(c).changed(),
        _ => false,
    }
}

/// Plain editor without range (for min / max / default fields).
pub fn raw_value_editor(ui: &mut Ui, value: &mut Value) -> bool {
    match value {
        Value::Float(v) => ui.add(egui::DragValue::new(v).speed(0.01).max_decimals(4)).changed(),
        Value::Int(v) => ui.add(egui::DragValue::new(v).speed(0.1)).changed(),
        Value::Bool(v) => ui.checkbox(v, "").changed(),
        Value::Vec2(v) => {
            let mut changed = false;
            ui.horizontal(|ui| {
                changed |= ui.add(egui::DragValue::new(&mut v[0]).speed(0.01).prefix("x ").max_decimals(3)).changed();
                changed |= ui.add(egui::DragValue::new(&mut v[1]).speed(0.01).prefix("y ").max_decimals(3)).changed();
            });
            changed
        }
        Value::Color(c) => ui.color_edit_button_rgba_unmultiplied(c).changed(),
    }
}

/// Layout job with syntax highlighting; the error span (if any) is underlined in red.
pub fn highlight_expression(src: &str, error: Option<expr::Span>, font: FontId) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut classes = vec![None; src.len()];
    for (span, class) in expr::highlight(src) {
        for c in classes.iter_mut().take(span.end.min(src.len())).skip(span.start) {
            *c = Some(class);
        }
    }
    let color_of = |c: Option<TokenClass>| match c {
        Some(TokenClass::Number) => Color32::from_rgb(181, 206, 168),
        Some(TokenClass::Function) => Color32::from_rgb(220, 220, 170),
        Some(TokenClass::Constant) => Color32::from_rgb(86, 182, 194),
        Some(TokenClass::Variable) => Color32::from_rgb(156, 220, 254),
        Some(TokenClass::Operator) => Color32::from_rgb(212, 212, 212),
        Some(TokenClass::Paren) => Color32::from_rgb(200, 160, 220),
        Some(TokenClass::Error) => Color32::from_rgb(244, 108, 100),
        None => Color32::from_rgb(180, 180, 180),
    };
    // Group consecutive bytes with the same class / error state into sections.
    let mut start = 0;
    let mut chars = src.char_indices().peekable();
    while let Some((i, _)) = chars.next() {
        let next = chars.peek().map_or(src.len(), |(j, _)| *j);
        let key = |b: usize| {
            let in_err = error.is_some_and(|e| {
                let (s, t) = if e.start == e.end { (e.start.saturating_sub(1), e.start.max(1)) } else { (e.start, e.end) };
                b >= s && b < t
            });
            (classes.get(b).copied().flatten(), in_err)
        };
        let boundary = next == src.len() || key(next) != key(i);
        if boundary {
            let (class, in_err) = key(start);
            let mut fmt = TextFormat::simple(font.clone(), color_of(class));
            if in_err {
                fmt.underline = Stroke::new(2.0, Color32::from_rgb(244, 70, 60));
                fmt.background = Color32::from_rgba_unmultiplied(244, 70, 60, 40);
            }
            job.append(&src[start..next], 0.0, fmt);
            start = next;
        }
    }
    if src.is_empty() {
        job.append("", 0.0, TextFormat::simple(font, Color32::GRAY));
    }
    job
}

/// Multiline expression editor with highlighting. Returns (response, changed).
pub fn expression_editor(ui: &mut Ui, text: &mut String, error: Option<expr::Span>) -> Response {
    let font = FontId::monospace(14.0);
    let mut layouter = |ui: &Ui, buf: &dyn TextBuffer, wrap_width: f32| -> Arc<egui::Galley> {
        let mut job = highlight_expression(buf.as_str(), error, font.clone());
        job.wrap.max_width = wrap_width;
        ui.painter().layout_job(job)
    };
    ui.add(
        egui::TextEdit::multiline(text)
            .code_editor()
            .desired_rows(2)
            .desired_width(f32::INFINITY)
            .layouter(&mut layouter),
    )
}

/// Six-dot grab handle used for drag sources.
pub fn grab_handle(ui: &mut Ui) -> Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(10.0, 18.0), Sense::hover());
    let c = if resp.hovered() { Color32::WHITE } else { Color32::from_gray(130) };
    for row in 0..3 {
        for col in 0..2 {
            let p = rect.min + egui::vec2(2.5 + col as f32 * 4.5, 4.5 + row as f32 * 4.5);
            ui.painter().circle_filled(p, 1.2, c);
        }
    }
    resp
}

/// Small painted disclosure triangle (fonts are not guaranteed to have one).
pub fn disclosure(ui: &mut Ui, open: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(12.0, 18.0), Sense::click());
    let c = if resp.hovered() { Color32::WHITE } else { Color32::from_gray(170) };
    let m = rect.center();
    let pts = if open {
        vec![m + egui::vec2(-4.0, -2.5), m + egui::vec2(4.0, -2.5), m + egui::vec2(0.0, 3.5)]
    } else {
        vec![m + egui::vec2(-2.5, -4.0), m + egui::vec2(3.5, 0.0), m + egui::vec2(-2.5, 4.0)]
    };
    ui.painter().add(egui::Shape::convex_polygon(pts, c, Stroke::NONE));
    resp
}
