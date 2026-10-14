//! How the window looks: its surfaces one above another — the dialog, the bar of its tabs, the
//! sunken side panes, the cards of files raised over the page with a shadow, the fields sunk
//! into it — of the colors the program gives in its [`crate::Look`].

use iced::border::Border;
use iced::widget::{button, container};
use iced::{Background, Color, Shadow, Theme, Vector};

use crate::Look;

fn alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

/// The bar of the tabs and the strip of the footer: the dialog's darker edge.
pub fn bar(look: &Look) -> impl Fn(&Theme) -> container::Style + use<> {
    let c = look.colors;
    move |_| container::Style { background: Some(c.header.into()), text_color: Some(c.text), ..Default::default() }
}

/// A side pane (the templates, the maps): sunk below the page.
pub fn sunken_pane(look: &Look) -> impl Fn(&Theme) -> container::Style + use<> {
    let c = look.colors;
    move |_| container::Style {
        background: Some(c.sunken.into()),
        border: Border { color: c.line, width: 1.0, radius: 4.0.into() },
        text_color: Some(c.text),
        ..Default::default()
    }
}

/// The page a tab shows.
pub fn page(look: &Look) -> impl Fn(&Theme) -> container::Style + use<> {
    let c = look.colors;
    move |_| container::Style { background: Some(c.page.into()), text_color: Some(c.text), ..Default::default() }
}

/// A file of a template: a card raised over the page, its shadow under it; lit at its edge
/// where a channel of it is chosen.
pub fn card(look: &Look, chosen: bool) -> impl Fn(&Theme) -> container::Style + use<> {
    let c = look.colors;
    move |_| container::Style {
        background: Some(c.card.into()),
        border: Border { color: if chosen { c.lit } else { c.card_edge }, width: 1.0, radius: 5.0.into() },
        shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.40), offset: Vector::new(0.0, 2.0), blur_radius: 7.0 },
        text_color: Some(c.text),
        ..Default::default()
    }
}

/// A menu that opens over what is under it: higher than a card, its shadow deeper.
pub fn menu(look: &Look) -> impl Fn(&Theme) -> container::Style + use<> {
    let c = look.colors;
    move |_| container::Style {
        background: Some(c.menu.into()),
        border: Border { color: c.card_edge, width: 1.0, radius: 4.0.into() },
        shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.55), offset: Vector::new(0.0, 5.0), blur_radius: 14.0 },
        text_color: Some(c.text),
        ..Default::default()
    }
}

/// A box sunk into the page: the folder, a list of options, a table.
pub fn well(look: &Look) -> impl Fn(&Theme) -> container::Style + use<> {
    let c = look.colors;
    move |_| container::Style {
        background: Some(c.field.into()),
        border: Border { color: c.line, width: 1.0, radius: 3.0.into() },
        text_color: Some(c.text),
        ..Default::default()
    }
}

/// A row of a list or a table: every other one a little lighter.
pub fn row(look: &Look, even: bool) -> impl Fn(&Theme) -> container::Style + use<> {
    let c = look.colors;
    move |_| container::Style { background: Some(if even { c.row } else { c.row_alt }.into()), text_color: Some(c.text), ..Default::default() }
}

/// A tab: the one shown of the page's color, joined to it, its text bright; the others sunk in
/// the bar, lit under the mouse.
pub fn tab(look: &Look, chosen: bool) -> impl Fn(&Theme, button::Status) -> button::Style + use<> {
    let c = look.colors;
    move |_, status| {
        let (background, text) = match (chosen, status) {
            (true, _) => (Some(c.page), c.bright),
            (false, button::Status::Hovered) => (Some(c.hover), c.text),
            (false, button::Status::Pressed) => (Some(c.card), c.bright),
            (false, _) => (None, c.dim),
        };
        let border = if chosen { Border { color: c.card_edge, width: 1.0, radius: 4.0.into() } } else { Border { radius: 4.0.into(), ..Border::default() } };
        button::Style { background: background.map(Background::Color), text_color: text, border, shadow: Shadow::default(), snap: true }
    }
}

/// The color a channel is shown in: red, green, blue, alpha white; three of a map neutral.
pub fn channel_color(tag: &str) -> Color {
    match tag {
        "R" => Color::from_rgb8(226, 86, 74),
        "G" => Color::from_rgb8(96, 190, 108),
        "B" => Color::from_rgb8(86, 146, 232),
        "A" => Color::from_rgb8(205, 205, 205),
        _ => Color::from_rgb8(176, 150, 222),
    }
}

/// A channel of a file, a button: tinted in its color, raised and lit where it is chosen; faint
/// where no map is given it yet.
pub fn chip(look: &Look, tag: &str, chosen: bool, empty: bool) -> impl Fn(&Theme, button::Status) -> button::Style + use<> {
    let c = look.colors;
    let color = channel_color(tag);
    move |_, status| {
        let tint = match (chosen, status, empty) {
            (true, _, _) => 0.34,
            (false, button::Status::Hovered, _) => 0.26,
            (false, _, true) => 0.06,
            (false, _, false) => 0.18,
        };
        let edge = if chosen { c.lit } else { alpha(color, if empty { 0.35 } else { 0.7 }) };
        let shadow =
            if chosen { Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.45), offset: Vector::new(0.0, 2.0), blur_radius: 5.0 } } else { Shadow::default() };
        button::Style {
            background: Some(alpha(color, tint).into()),
            text_color: if empty { c.faint } else { c.bright },
            border: Border { color: edge, width: if chosen { 2.0 } else { 1.0 }, radius: 4.0.into() },
            shadow,
            snap: true,
        }
    }
}

/// A row of a menu or a list of maps: lit under the mouse; dim where it cannot be pressed.
pub fn item(look: &Look) -> impl Fn(&Theme, button::Status) -> button::Style + use<> {
    let c = look.colors;
    move |_, status| {
        let (background, text) = match status {
            button::Status::Hovered => (Some(c.hover), c.bright),
            button::Status::Pressed => (Some(c.card), c.bright),
            button::Status::Disabled => (None, c.faint),
            button::Status::Active => (None, c.text),
        };
        button::Style {
            background: background.map(Background::Color),
            text_color: text,
            border: Border { radius: 3.0.into(), ..Border::default() },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// A small mark of a map the graph has.
pub fn dot(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { background: Some(color.into()), border: Border { radius: 4.0.into(), ..Border::default() }, ..Default::default() }
}
