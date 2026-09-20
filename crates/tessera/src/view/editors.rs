//! Editors for values, shared by the Input Values panel and the properties panel.

use iced::widget::{button, column, container, row, space, text, toggler, tooltip};
use iced::{Alignment, Border, Element, Length};
use tessera_core::Value;

use crate::app::{ColorTarget, Message, PickMessage};
use crate::icons::{Icon, icon};
use crate::picker::PickTarget;
use crate::style;
use crate::view::tip;
use crate::widget::color::{color_picker, to_hex};
use crate::widget::number::number;

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
        eyedropper(PickTarget::Color(target), picking, "Pick from screen", "Click anywhere on any monitor to take its color. Esc or a right click cancels."),
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
    match value {
        Value::Float(v) => {
            let (lo, hi, limited) = bounds(0);
            let mut n = number(v as f64, lo, hi).on_change(move |x| on_change(Value::Float(x as f32))).on_release(on_release);
            if limited {
                n = n.limits(lo, hi);
            }
            n.into()
        }
        Value::Int(v) => {
            let (lo, hi, limited) = bounds(0);
            let (lo, hi) = if limited { (lo, hi) } else { (0.0, 16.0) };
            let mut n = number(v as f64, lo, hi).integer(true).on_change(move |x| on_change(Value::Int(x as i32))).on_release(on_release);
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
                let mut n = number(v[i] as f64, lo, hi)
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
