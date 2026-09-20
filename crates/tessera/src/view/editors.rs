//! Editors for values, shared by the Input Values panel and the properties panel.

use iced::widget::{button, column, container, row, space, text, toggler, tooltip};
use iced::{Alignment, Border, Element, Length};
use tessera_core::{Edit, ParamValue, Value};

use crate::app::{App, ColorTarget, Message, PickMessage, VarField};
use crate::icons::{Icon, icon};
use crate::picker::PickTarget;
use crate::style;
use crate::view::tip;
use crate::widget::color::{color_picker, to_hex};
use crate::widget::number::{NumberField, number};

/// A small color swatch that toggles a color picker.
pub fn swatch<'a>(c: [f32; 4], open: bool, on_press: Message) -> Element<'a, Message> {
    let color = style::color_of(c);
    let chip = container(space().width(Length::Fill).height(Length::Fixed(16.0))).style(move |_| container::Style {
        background: Some(iced::Color { a: 1.0, ..color }.into()),
        border: Border { color: style::BORDER, width: 1.0, radius: 2.0.into() },
        ..Default::default()
    });
    button(
        row![
            container(chip).width(Length::Fixed(38.0)),
            text(to_hex(c)).size(style::SMALL).font(style::mono()).color(style::TEXT_DIM),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([3, 6])
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
                    Message::Edit(tessera_core::edit::gradient_edit(node, &set, n))
                },
                Message::Seal,
                picking,
            )
        }
    };
    Some(crate::view::popup_layer(
        content,
        app.ui.color_picker_at,
        iced::Size::new(236.0, 252.0),
        crate::view::window_size(app),
        Message::ColorPicker(None),
    ))
}
