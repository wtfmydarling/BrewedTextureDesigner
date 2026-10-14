//! How the window looks, from the darkest surface up, in the colors the program gives in its
//! [`crate::Look`]: the bar of the tabs and the footer, the sunken side lists, the page, the
//! rows of output maps raised on it, the channels in them, the label of a map being dragged
//! above everything.

use iced::border::Border;
use iced::widget::{button, container};
use iced::{Background, Color, Shadow, Theme, Vector};

use crate::Look;

fn alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

/// The bar of the tabs, and the footer.
pub fn bar(look: &Look) -> impl Fn(&Theme) -> container::Style + use<> {
    let c = look.colors;
    move |_| container::Style { background: Some(c.header.into()), text_color: Some(c.text), ..Default::default() }
}

/// A list beside the page (the presets, the maps): sunk below it, a line around it.
pub fn sunken(look: &Look) -> impl Fn(&Theme) -> container::Style + use<> {
    let c = look.colors;
    move |_| container::Style {
        background: Some(c.sunken.into()),
        border: Border { color: c.line, width: 1.0, radius: 2.0.into() },
        text_color: Some(c.text),
        ..Default::default()
    }
}

/// The strip at the top of a list: its caption, as the program's panes have.
pub fn strip(look: &Look) -> impl Fn(&Theme) -> container::Style + use<> {
    let c = look.colors;
    move |_| container::Style { background: Some(c.strip.into()), text_color: Some(c.text), ..Default::default() }
}

/// The page a tab shows.
pub fn page(look: &Look) -> impl Fn(&Theme) -> container::Style + use<> {
    let c = look.colors;
    move |_| container::Style { background: Some(c.page.into()), text_color: Some(c.text), ..Default::default() }
}

/// A row of an output map: raised on the page, a shadow under it.
pub fn raised(look: &Look) -> impl Fn(&Theme) -> container::Style + use<> {
    let c = look.colors;
    move |_| container::Style {
        background: Some(c.raised.into()),
        border: Border { color: c.edge, width: 1.0, radius: 3.0.into() },
        shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.35), offset: Vector::new(0.0, 1.0), blur_radius: 3.0 },
        text_color: Some(c.text),
        ..Default::default()
    }
}

/// A box sunk in a row or the page: a value shown, the folder.
pub fn well(look: &Look) -> impl Fn(&Theme) -> container::Style + use<> {
    let c = look.colors;
    move |_| container::Style {
        background: Some(c.field.into()),
        border: Border { color: c.line, width: 1.0, radius: 2.0.into() },
        text_color: Some(c.text),
        ..Default::default()
    }
}

/// A tab: the one shown of the page's color, its text bright, a light line on its top; the
/// others in the bar, lit under the mouse.
pub fn tab(look: &Look, chosen: bool) -> impl Fn(&Theme, button::Status) -> button::Style + use<> {
    let c = look.colors;
    move |_, status| {
        let (background, text) = match (chosen, status) {
            (true, _) => (Some(c.page), c.bright),
            (false, button::Status::Hovered) => (Some(c.strip), c.text),
            (false, _) => (None, c.dim),
        };
        button::Style { background: background.map(Background::Color), text_color: text, border: Border::default(), shadow: Shadow::default(), snap: true }
    }
}

/// The color a channel is shown in, as Painter shows them: red, green, blue, alpha white,
/// three of a map and gray neutral.
pub fn channel_color(tag: &str) -> Color {
    match tag {
        "R" => Color::from_rgb8(212, 70, 62),
        "G" => Color::from_rgb8(78, 176, 92),
        "B" => Color::from_rgb8(70, 132, 220),
        "A" => Color::from_rgb8(200, 200, 200),
        _ => Color::from_rgb8(150, 150, 150),
    }
}

/// A channel of an output map: a box tinted in its color, a line of it at its foot; lit where
/// a map dragged is over it, edged where it is chosen.
pub fn channel(look: &Look, tag: &str, chosen: bool, over: bool, empty: bool) -> impl Fn(&Theme) -> container::Style + use<> {
    let c = look.colors;
    let color = channel_color(tag);
    move |_| container::Style {
        background: Some(
            alpha(
                color,
                if over {
                    0.45
                } else if empty {
                    0.10
                } else {
                    0.24
                },
            )
            .into(),
        ),
        border: Border { color: if over || chosen { c.bright } else { alpha(color, 0.65) }, width: if over || chosen { 2.0 } else { 1.0 }, radius: 2.0.into() },
        text_color: Some(if empty { c.faint } else { c.bright }),
        ..Default::default()
    }
}

/// A row of a list (a preset, a map): lit under the mouse, the chosen one of the field's color.
pub fn item(look: &Look, chosen: bool) -> impl Fn(&Theme, button::Status) -> button::Style + use<> {
    let c = look.colors;
    move |_, status| {
        let (background, text) = match (chosen, status) {
            (true, _) => (Some(c.chosen), c.bright),
            (false, button::Status::Hovered) => (Some(c.hover), c.bright),
            (false, button::Status::Disabled) => (None, c.faint),
            (false, _) => (None, c.text),
        };
        button::Style {
            background: background.map(Background::Color),
            text_color: text,
            border: Border { radius: 2.0.into(), ..Border::default() },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// A map in the list of maps: a row of the list; dragged, it is.
pub fn map_row(look: &Look, hovered: bool) -> impl Fn(&Theme) -> container::Style + use<> {
    let c = look.colors;
    move |_| container::Style {
        background: hovered.then_some(Background::Color(c.hover)),
        border: Border { radius: 2.0.into(), ..Border::default() },
        ..Default::default()
    }
}

/// The label of a map being dragged: over everything, its shadow deep.
pub fn dragged(look: &Look) -> impl Fn(&Theme) -> container::Style + use<> {
    let c = look.colors;
    move |_| container::Style {
        background: Some(c.raised.into()),
        border: Border { color: c.bright, width: 1.0, radius: 3.0.into() },
        shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.6), offset: Vector::new(0.0, 4.0), blur_radius: 10.0 },
        text_color: Some(c.bright),
        ..Default::default()
    }
}

/// A menu that opens in a row (the parts of a name): above the row, its shadow deeper.
pub fn menu(look: &Look) -> impl Fn(&Theme) -> container::Style + use<> {
    let c = look.colors;
    move |_| container::Style {
        background: Some(c.menu.into()),
        border: Border { color: c.edge, width: 1.0, radius: 3.0.into() },
        shadow: Shadow { color: Color::from_rgba(0.0, 0.0, 0.0, 0.5), offset: Vector::new(0.0, 4.0), blur_radius: 10.0 },
        text_color: Some(c.text),
        ..Default::default()
    }
}

/// A small square of color: a map the template uses, in the list of maps.
pub fn swatch(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(color.into()),
        border: Border { color: alpha(Color::BLACK, 0.5), width: 1.0, radius: 1.0.into() },
        ..Default::default()
    }
}
