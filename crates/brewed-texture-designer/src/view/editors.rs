//! Editors for values, shared by the Input Values panel and the properties panel.

use iced::widget::{button, column, container, image, mouse_area, row, space, text, toggler, tooltip};
use iced::{Alignment, Border, ContentFit, Element, Length};
use brewed_texture_core::{Edit, ParamValue, ResourceKind, Value, files};

use crate::app::{App, ColorTarget, DragPayload, FileTarget, Message, PickMessage, VarField};
use crate::icons::{Icon, icon};
use crate::picker::PickTarget;
use crate::style;
use crate::view::tip;
use crate::widget::color::{color_picker, to_hex};
use crate::widget::number::{NumberField, number};

/// Does a field that takes the files with these extensions (none: any) take the file at `path`?
pub fn takes_file(extensions: &[&str], path: &str) -> bool {
    extensions.is_empty()
        || std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| extensions.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// The kind of the files with these extensions, if they are the files of one.
fn kind_of(extensions: &[&str]) -> Option<ResourceKind> {
    ResourceKind::ALL.into_iter().find(|k| k.extensions() == extensions)
}

/// The file a node or a graph reads: a picture of it and its name, and buttons to choose
/// another and to read none; `none` says what reading none means, and `color` is what is seen
/// then, when it is a color. (A file edited elsewhere is read again by itself.) A file of the
/// packages dragged from the Explorer is dropped on it: while one is dragged, the field says at
/// once whether it takes it.
pub fn file_field<'a>(app: &'a App, target: FileTarget, none: String, color: Option<[f32; 4]>) -> Element<'a, Message> {
    let extensions = app.takes(&target);
    let kind = kind_of(extensions);
    if let Some(DragPayload::Resource { path: dragged, name, .. }) = &app.ui.drag {
        let takes = takes_file(extensions, dragged);
        let says = if takes {
            format!("Drop to use {name}")
        } else {
            let what = match kind {
                Some(ResourceKind::Bitmap) => "a picture",
                Some(ResourceKind::Svg) => "an SVG file",
                Some(ResourceKind::Font) => "a font",
                None => "a file this takes",
            };
            format!("✕ {name} is not {what}")
        };
        let color = if takes { style::ACCENT } else { style::ERROR };
        let zone = container(crate::view::fitted(says, style::SMALL, style::ui_font(), Some(color)))
            .padding([5, 8])
            .width(Length::Fill)
            .clip(true)
            .style(style::drop_zone(takes));
        let zone = mouse_area(zone);
        return if takes { zone.on_release(Message::SetFile(target, Some(dragged.clone()))).into() } else { zone.into() };
    }

    let path = app.file_of(&target);
    let state = (!path.is_empty()).then(|| app.res.files.get(path)).flatten();
    // Not looked at yet: taken as there.
    let there = state.is_none_or(|s| s.there);
    let preview = state.and_then(|s| s.preview.as_ref());
    let picture: Element<'a, Message> = match (preview, color.filter(|_| path.is_empty())) {
        (Some(p), _) => image(p.image.clone()).width(22).height(22).content_fit(ContentFit::Contain).into(),
        (None, Some(c)) => container(container(space().width(16).height(14)).style(style::chip(style::color_of(c))))
            .width(22)
            .center_x(22)
            .into(),
        (None, None) => {
            let glyph = match kind {
                Some(ResourceKind::Svg) => Icon::Vector,
                Some(ResourceKind::Font) => Icon::Font,
                _ => Icon::Image,
            };
            container(icon(glyph, 14.0, style::TEXT_FAINT)).width(22).center_x(22).into()
        }
    };
    let embedded = files::is_embedded(path);
    let (name, color) = match (path.is_empty(), there) {
        (true, _) => (none, style::TEXT_FAINT),
        (false, true) => (app.doc.package().file_label(path), style::TEXT),
        (false, false) => (format!("{} (not there)", app.doc.package().file_label(path)), style::ERROR),
    };
    let mut about = Vec::new();
    if !path.is_empty() {
        about.push(if embedded { "Held in the package's file (see the Explorer's Resources)".to_string() } else { path.to_string() });
    }
    if let Some(p) = preview {
        about.push(p.about.clone());
    }
    if let Some((_, why)) = state.and_then(|s| s.failed.as_ref()) {
        about.push(why.clone());
    }
    if !path.is_empty() && !embedded {
        about.push("Edited elsewhere, it is read again by itself.".into());
    }
    about.push("Drag a file onto it from the Explorer, or choose one.".into());
    let shown = container(
        row![picture, crate::view::fitted(name, style::SMALL, style::ui_font(), Some(color))].spacing(6).align_y(Alignment::Center),
    )
    .padding([1, 6])
    .width(Length::Fill)
    .clip(true)
    .style(style::field_box);
    let mut buttons = row![tip(
        button(icon(Icon::Folder, 14.0, style::TEXT_DIM)).padding(4).style(style::button).on_press(Message::BrowseFile(target.clone())),
        if path.is_empty() { "Choose a file" } else { "Choose another file" },
        tooltip::Position::Top,
    )]
    .spacing(4);
    if !path.is_empty() {
        buttons = buttons.push(tip(
            button(icon(Icon::Close, 14.0, style::TEXT_DIM)).padding(4).style(style::button).on_press(Message::SetFile(target, None)),
            "Read no file",
            tooltip::Position::Top,
        ));
    }
    row![tip(shown, about.join("\n"), tooltip::Position::Top), buttons].spacing(4).align_y(Alignment::Center).into()
}

/// A small color swatch that toggles a color picker.
pub fn swatch<'a>(c: [f32; 4], open: bool, on_press: Message) -> Element<'a, Message> {
    let color = style::color_of(c);
    let chip = container(space().width(Length::Fill).height(Length::Fixed(16.0))).style(move |_| container::Style {
        background: Some(iced::Color { a: 1.0, ..color }.into()),
        border: Border { color: style::BORDER, width: 1.0, radius: 2.0.into() },
        ..Default::default()
    });
    // (Close enough together for the color's name to stay whole in the narrowest Input Values.)
    button(
        row![
            container(chip).width(Length::Fixed(32.0)),
            text(to_hex(c)).size(style::SMALL).font(style::mono()).color(style::TEXT_DIM).wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([3, 4])
    .width(Length::Fill)
    .style(style::toggle_button(open))
    .on_press(on_press)
    .into()
}

/// The expanded color editor: picker, numeric RGBA channels and the eyedropper, which
/// takes a color from anywhere on the screen for `target`.
pub fn color_editor<'a>(
    c: [f32; 4],
    target: ColorTarget,
    on_change: impl Fn([f32; 4]) -> Message + Clone + 'a,
    on_release: Message,
    picking: Option<&PickTarget>,
) -> Element<'a, Message> {
    let mut channels = row![].spacing(4);
    for (i, label) in ["R ", "G ", "B ", "A "].iter().enumerate() {
        let f = on_change.clone();
        channels = channels.push(
            number(c[i] as f64, 0.0, 1.0)
                .prefix(*label)
                .decimals(3)
                .on_change(move |v| {
                    let mut n = c;
                    n[i] = v as f32;
                    f(n)
                })
                .on_release(on_release.clone()),
        );
    }
    let footer = row![
        eyedropper(PickTarget::Color(target), picking, "Pick color", "Click anywhere on any monitor to take its color. Esc or a right click cancels."),
        space().width(Length::Fill),
        text(to_hex(c)).size(style::SMALL).font(style::mono()).color(style::TEXT_DIM),
    ]
    .align_y(Alignment::Center);
    container(column![color_picker(c, on_change).on_release(on_release), channels, footer].spacing(6))
        .padding(8)
        .style(style::card)
        .into()
}

/// A button that starts picking from the screen. While its pick is in progress it stays
/// down, and a click on it cancels.
pub fn eyedropper<'a>(target: PickTarget, picking: Option<&PickTarget>, label: &'a str, tooltip: &'a str) -> Element<'a, Message> {
    let active = picking == Some(&target);
    let message = if active { PickMessage::Cancel } else { PickMessage::Start(target) };
    let button = button(
        row![icon(Icon::Pipette, 14.0, if active { style::TEXT } else { style::TEXT_DIM }), text(label).size(style::SMALL)]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .padding([3, 8])
    .style(move |theme, status| if active { style::toggle_button(true)(theme, status) } else { style::button(theme, status) })
    .on_press(Message::Pick(message));
    tip(button, tooltip, tooltip::Position::Top)
}

/// Editor of a [`Value`] within an optional [min, max] range.
pub fn value_editor<'a>(
    value: Value,
    // What the value would be untouched, when there is such a thing: the field says so.
    default: Option<Value>,
    range: Option<(Value, Value)>,
    on_change: impl Fn(Value) -> Message + Clone + 'a,
    on_release: Message,
    color_open: bool,
    on_color: Message,
) -> Element<'a, Message> {
    let bounds = |i: usize| -> (f64, f64, bool) {
        match range {
            Some((Value::Float(a), Value::Float(b))) if a < b => (a as f64, b as f64, true),
            Some((Value::Int(a), Value::Int(b))) if a < b => (a as f64, b as f64, true),
            Some((Value::Vec2(a), Value::Vec2(b))) if a[i] < b[i] => (a[i] as f64, b[i] as f64, true),
            _ => (0.0, 1.0, false),
        }
    };
    // The default of the same shape as the value; anything else is no default at all.
    let default_at = |i: usize| match default {
        Some(Value::Float(d)) => Some(d as f64),
        Some(Value::Int(d)) => Some(d as f64),
        Some(Value::Vec2(d)) => Some(d[i] as f64),
        _ => None,
    };
    let held = |field: NumberField<'a, Message>, i: usize| match default_at(i) {
        Some(d) => field.default_value(d),
        None => field,
    };
    match value {
        Value::Float(v) => {
            let (lo, hi, limited) = bounds(0);
            let mut n = held(number(v as f64, lo, hi), 0).on_change(move |x| on_change(Value::Float(x as f32))).on_release(on_release);
            if limited {
                n = n.limits(lo, hi);
            }
            n.into()
        }
        Value::Int(v) => {
            let (lo, hi, limited) = bounds(0);
            let (lo, hi) = if limited { (lo, hi) } else { (0.0, 16.0) };
            let mut n = held(number(v as f64, lo, hi), 0).integer(true).on_change(move |x| on_change(Value::Int(x as i32))).on_release(on_release);
            if limited {
                n = n.limits(lo, hi);
            }
            n.into()
        }
        Value::Bool(b) => toggler(b)
            .on_toggle(move |x| on_change(Value::Bool(x)))
            .label(if b { "true" } else { "false" })
            .text_size(style::SMALL)
            .size(16)
            .style(style::toggler)
            .into(),
        Value::Vec2(v) => {
            let mut r = row![].spacing(4);
            for (i, prefix) in ["x ", "y "].iter().enumerate() {
                let (lo, hi, limited) = bounds(i);
                let (lo, hi) = if limited { (lo, hi) } else { (-1.0, 1.0) };
                let f = on_change.clone();
                let mut n = held(number(v[i] as f64, lo, hi), i)
                    .prefix(*prefix)
                    .on_change(move |x| {
                        let mut n = v;
                        n[i] = x as f32;
                        f(Value::Vec2(n))
                    })
                    .on_release(on_release.clone());
                if limited {
                    n = n.limits(lo, hi);
                }
                r = r.push(n);
            }
            r.into()
        }
        Value::Color(c) => {
            let _ = on_change;
            let _ = on_release;
            swatch(c, color_open, on_color)
        }
    }
}

/// The color picker, floating where it was opened. It is a layer of its own so that
/// opening it moves nothing in the panels, and a click anywhere else closes it.
pub fn color_popup(app: &App) -> Option<Element<'_, Message>> {
    let target = app.ui.color_picker.clone()?;
    let graph = app.graph();
    let picking = app.picking();
    let content: Element<'_, Message> = match &target {
        ColorTarget::Param(node, key) => {
            let (node, key) = (*node, key.clone());
            let c = match graph.node(node).and_then(|n| n.value(&key)) {
                Some(ParamValue::Color(c)) => *c,
                _ => return None,
            };
            let set = key.clone();
            color_editor(
                c,
                target.clone(),
                move |c| Message::Edit(Edit::SetParam { node, key: set.clone(), value: ParamValue::Color(c) }),
                Message::Seal,
                picking,
            )
        }
        ColorTarget::Variable(name, field) => {
            let var = graph.variable(name)?;
            let current = if *field == VarField::Value { var.value } else { var.default };
            let Value::Color(c) = current else { return None };
            let (name, field) = (name.clone(), *field);
            color_editor(
                c,
                target.clone(),
                move |c| {
                    let value = Value::Color(c);
                    Message::Edit(match field {
                        VarField::Value => Edit::SetVariable { name: name.clone(), value },
                        VarField::Default => Edit::SetVariableDefault { name: name.clone(), value },
                    })
                },
                Message::Seal,
                picking,
            )
        }
        ColorTarget::Comment(id) => {
            let id = *id;
            let c = graph.comment(id)?.color;
            color_editor(c, target.clone(), move |c| Message::Edit(Edit::SetCommentColor { id, color: c }), Message::Seal, picking)
        }
        ColorTarget::GradientStop(node, key) => {
            let (node, key) = (*node, key.clone());
            let gradient = graph.node(node).and_then(|n| n.value(&key)).and_then(ParamValue::as_gradient)?.clone();
            let index = *app.ui.gradient_stop.get(&(node, key.clone()))?;
            let stop = gradient.stops.get(index)?;
            let (base, set) = (gradient.clone(), key.clone());
            color_editor(
                stop.color,
                target.clone(),
                move |c| {
                    let mut n = base.clone();
                    n.stops[index].color = c;
                    Message::Edit(brewed_texture_core::edit::gradient_edit(node, &set, n))
                },
                Message::Seal,
                picking,
            )
        }
    };
    Some(crate::view::popup_layer(content, app.ui.color_picker_at, 236.0, Message::ColorPicker(None)))
}
