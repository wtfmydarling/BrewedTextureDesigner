//! Custom widgets.

pub mod blind;
pub mod color;
pub mod curve;
pub mod dropdown;
pub mod gradient;
pub mod layered;
pub mod number;
pub mod outside;
pub mod placed;
pub mod pressable;

use iced::widget::{button, svg, tooltip};
use iced::{Color, Element, Length};

use crate::style;
use brewed_texture_core::DataType;

/// The socket symbol of a data type (circle for images, diamond for values), filled
/// when `filled`.
pub fn socket_icon<'a, Message: 'a>(ty: DataType, filled: bool, size: f32, color: Color) -> Element<'a, Message> {
    let hex = |c: Color| {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!("#{:02x}{:02x}{:02x}", q(c.r), q(c.g), q(c.b))
    };
    let stroke = hex(color);
    let fill = if filled { stroke.clone() } else { hex(style::FIELD) };
    let shape = if ty == DataType::Image {
        format!(r#"<circle cx="12" cy="12" r="7" fill="{fill}" stroke="{stroke}" stroke-width="2.4"/>"#)
    } else {
        format!(r#"<path d="M12 3.5 20.5 12 12 20.5 3.5 12z" fill="{fill}" stroke="{stroke}" stroke-width="2.4"/>"#)
    };
    let svg_src = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">{shape}</svg>"#);
    svg(svg::Handle::from_memory(svg_src.into_bytes())).width(Length::Fixed(size)).height(Length::Fixed(size)).into()
}

/// The expose toggle at the right end of a parameter row. `None` when the parameter
/// cannot be exposed (an empty slot of the same width keeps the rows aligned).
/// How wide the toggle that exposes a parameter is (or the room left for it, where it has none).
pub const TOGGLE_W: f32 = 22.0;

pub fn expose_toggle<'a, Message: Clone + 'a>(
    ty: Option<DataType>,
    exposed: bool,
    on_toggle: Option<Message>,
) -> Element<'a, Message> {
    const W: f32 = TOGGLE_W;
    let Some(ty) = ty else {
        return iced::widget::space().width(Length::Fixed(W)).into();
    };
    let color = if on_toggle.is_some() { style::type_color(ty) } else { style::TEXT_FAINT };
    let b = button(socket_icon(ty, exposed, 14.0, color))
        .padding(4)
        .width(Length::Fixed(W))
        .style(style::flat_button)
        .on_press_maybe(on_toggle);
    let tip = if exposed {
        "Exposed: the value comes from the node connected to this parameter's socket. Click to un-expose."
    } else {
        "Expose: give this parameter an input socket on the node"
    };
    tooltip(b, iced::widget::container(iced::widget::text(tip).size(style::SMALL)).padding(6).style(style::tooltip), tooltip::Position::Left)
        .into()
}
