//! The templates of a new graph as the dialog shows them (see `app::home`): a picture of each,
//! drawn here (a ball for a material, an image in and out for a filter…), its card, and what
//! its (i) says of it.

use std::fmt::Write as _;

use iced::widget::{button, column, container, row, stack, svg, text};
use iced::{Alignment, Element, Length, Padding};
use brewed_texture_core::templates::Template;

use crate::app::Message;
use crate::app::home::NewGraphMessage;
use crate::icons::{Icon, icon};
use crate::style;
use crate::widget::hover_card::hover_card;

/// How big the picture of a card is.
pub const PICTURE: [f32; 2] = [150.0, 100.0];

/// The picture of `template`: what it makes, drawn.
fn picture(template: Template) -> svg::Handle {
    let mut art = String::from(r##"<rect width="150" height="100" fill="#1a1a1a"/>"##);
    match template {
        Template::Pbr | Template::PbrEmissive => {
            art.push_str(
                r##"<defs><radialGradient id="ball" cx="0.38" cy="0.34" r="0.78"><stop offset="0" stop-color="#ececec"/><stop offset="0.45" stop-color="#9c9c9c"/><stop offset="1" stop-color="#262626"/></radialGradient></defs>
                <ellipse cx="75" cy="89" rx="31" ry="5" fill="#000" opacity="0.45"/>
                <circle cx="75" cy="50" r="36" fill="url(#ball)"/>
                <ellipse cx="62" cy="35" rx="9" ry="6" fill="#fff" opacity="0.5"/>"##,
            );
            if template == Template::PbrEmissive {
                // Cracks that glow of themselves.
                let cracks = "M50 31l9 9-3 8 10 7-2 9 8 8M86 24l-4 11 8 6-5 10 9 6M58 70l12-3 6 5 11-4";
                let _ = write!(
                    art,
                    r##"<path d="{cracks}" fill="none" stroke="#ff7a1a" stroke-width="7" opacity="0.28" stroke-linecap="round" stroke-linejoin="round"/>
                    <path d="{cracks}" fill="none" stroke="#ffb35c" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/>"##
                );
            }
        }
        Template::GrayFilter | Template::ColorFilter => {
            let (a, b, c, d) = if template == Template::GrayFilter { ("#2e2e2e", "#b8b8b8", "#0d0d0d", "#f0f0f0") } else { ("#c2562a", "#2b6fd6", "#4fc26a", "#9a3fc9") };
            let _ = write!(
                art,
                r##"<defs><linearGradient id="in" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="{a}"/><stop offset="1" stop-color="{b}"/></linearGradient><linearGradient id="out" x1="0" y1="0" x2="1" y2="1"><stop offset="0.35" stop-color="{c}"/><stop offset="0.65" stop-color="{d}"/></linearGradient></defs>
                <rect x="16" y="26" width="46" height="46" rx="4" fill="url(#in)"/>
                <path d="M70 49h11m-5-5 5 5-5 5" fill="none" stroke="#bdbdbd" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/>
                <rect x="88" y="26" width="46" height="46" rx="4" fill="url(#out)"/>"##
            );
        }
        Template::PixelProcessor => {
            // Pixels, each the color its place says: red across, green down.
            for y in 0..5 {
                for x in 0..8 {
                    let (r, g) = ((x * 255 / 7) as u8, (y * 255 / 4) as u8);
                    let _ = write!(art, r##"<rect x="{}" y="{}" width="14" height="14" rx="1.5" fill="#{r:02x}{g:02x}80"/>"##, 15 + x * 15, 13 + y * 15);
                }
            }
        }
        Template::FxMap => {
            // Splats of every size, some over others.
            let mut seed = 7u32;
            let mut next = || {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed
            };
            for _ in 0..34 {
                let (x, y) = (8 + next() % 134, 8 + next() % 84);
                let r = 2 + next() % 11;
                let shade = 110 + next() % 146;
                let _ = write!(art, r##"<circle cx="{x}" cy="{y}" r="{r}" fill="#{shade:02x}{shade:02x}{shade:02x}" opacity="0.85"/>"##);
            }
        }
        Template::Empty => {
            // The graph's grid.
            for x in (0..=150).step_by(10) {
                let color = if x % 50 == 0 { "#3a3a3a" } else { "#262626" };
                let _ = write!(art, r#"<path d="M{x} 0v100" stroke="{color}" stroke-width="1"/>"#);
            }
            for y in (0..=100).step_by(10) {
                let color = if y % 50 == 0 { "#3a3a3a" } else { "#262626" };
                let _ = write!(art, r#"<path d="M0 {y}h150" stroke="{color}" stroke-width="1"/>"#);
            }
        }
    }
    let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 150 100">{art}</svg>"#);
    svg::Handle::from_memory(svg.into_bytes())
}

/// What the (i) of `template` says: its name, its type, what it is, its outputs.
fn info<'a>(template: Template) -> Element<'a, Message> {
    let outputs = template.outputs();
    let outputs = if outputs.is_empty() { "none".to_string() } else { outputs.join(", ") };
    container(
        column![
            text(template.name()).size(14).font(style::heavy()).color(style::TEXT_BRIGHT),
            text(format!("Type: {}", template.kind())).size(style::SMALL).color(style::TEXT_DIM),
            text(template.about()).size(style::SMALL),
            text(format!("Outputs: {outputs}")).size(style::SMALL).color(style::TEXT_DIM),
        ]
        .spacing(6),
    )
    .padding(12)
    .width(Length::Fixed(280.0))
    .style(style::tooltip)
    .into()
}

/// The (i) of `template`, which says what it is while it is pointed at.
fn info_button<'a>(template: Template) -> Element<'a, Message> {
    hover_card(container(icon(Icon::Info, 17.0, style::TEXT)).padding(3).style(style::info_badge), info(template)).into()
}

/// A card of the grid: the picture of `template`, its (i) at the top right, its name, and its
/// category in gray.
pub fn card<'a>(template: Template, chosen: bool) -> Element<'a, Message> {
    let picture = container(svg(picture(template)).width(Length::Fixed(PICTURE[0])).height(Length::Fixed(PICTURE[1]))).style(style::picture_frame);
    let top = stack![picture, container(info_button(template)).width(Length::Fixed(PICTURE[0])).align_x(Alignment::End).padding(4)];
    let body = column![
        top,
        text(template.name()).size(style::TEXT_SIZE).font(style::bold()),
        text(template.category().name()).size(style::SMALL).color(style::TEXT_FAINT),
    ]
    .spacing(4)
    .width(Length::Fixed(PICTURE[0]));
    button(body).padding(8).style(style::template_card(chosen)).on_press(Message::NewGraph(NewGraphMessage::Template(template))).into()
}

/// A row of the list: a small picture of `template`, its name, its category and its type, and
/// its (i).
pub fn list_row<'a>(template: Template, chosen: bool) -> Element<'a, Message> {
    let picture = container(svg(picture(template)).width(Length::Fixed(48.0)).height(Length::Fixed(32.0))).style(style::picture_frame);
    let said = row![
        picture,
        text(template.name()).size(style::TEXT_SIZE).font(style::bold()).width(Length::FillPortion(3)),
        text(template.category().name()).size(style::SMALL).color(style::TEXT_FAINT).width(Length::FillPortion(3)),
        text(template.kind()).size(style::SMALL).color(style::TEXT_DIM).width(Length::FillPortion(2)),
        info_button(template),
    ]
    .spacing(12)
    .align_y(Alignment::Center);
    button(said)
        .width(Length::Fill)
        .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 4.0 })
        .style(style::template_card(chosen))
        .on_press(Message::NewGraph(NewGraphMessage::Template(template)))
        .into()
}
