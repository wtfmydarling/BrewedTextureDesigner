//! Choices picked by how they look. What a noise or a shape is called matters less than
//! what it looks like, so the field shows the node's picture next to the option's name, and
//! its menu shows every option as a picture of what the node makes with it: drawn by the
//! engine with everything else as it is now, the seed and the scale included.

use iced::widget::{button, column, container, image, row, scrollable, space, text};
use iced::{Alignment, Element, Length};
use brewed_texture_core::graph::{Node, NodeId};
use brewed_texture_core::nodes::ParamDef;

use crate::app::{App, Message};
use crate::icons::{Icon, icon};
use crate::style;
use crate::widget::dropdown::dropdown;

/// Side of an option's picture in the menu.
const PICTURE: f32 = 56.0;
/// Side of the picture in the field.
const FIELD_PICTURE: f32 = 18.0;
/// Width of an option in the menu.
const OPTION_W: f32 = 178.0;

/// Columns of a menu of `options` options: a short list stays a list, a long one spreads
/// out so it fits under the field.
pub fn columns(options: usize) -> usize {
    match options {
        0..=6 => 1,
        7..=12 => 2,
        _ => 3,
    }
}

/// The field of a choice with pictures, and its menu while it is open.
pub fn field<'a>(app: &'a App, node: &'a Node, pd: &'static ParamDef, current: u32) -> Element<'a, Message> {
    let id = node.id;
    let open = app.ui.choice_menu.as_ref().is_some_and(|m| m.node == id && m.key == pd.key);
    let label = pd.enum_options().get(current as usize).copied().unwrap_or("?");
    // The node's own thumbnail is what the current option makes.
    let face = button(
        row![
            picture(app.res.thumbs.get(&id), FIELD_PICTURE),
            text(label).size(style::TEXT_SIZE).wrapping(text::Wrapping::None),
            space().width(Length::Fill),
            icon(Icon::ChevronDown, 12.0, style::TEXT_DIM),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([2, 6])
    .style(style::choice_field(open))
    .on_press(Message::ChoiceMenu(if open { None } else { Some((id, pd.key)) }));
    let menu: Element<'a, Message> = if open { menu(app, id, pd, current) } else { space().into() };
    dropdown(face, menu, open).on_dismiss(Message::ChoiceMenu(None)).into()
}

/// Every option: its picture, then its name.
fn menu<'a>(app: &'a App, id: NodeId, pd: &'static ParamDef, current: u32) -> Element<'a, Message> {
    let options = pd.enum_options();
    let pictures = app.res.choices.get(&(id, pd.key.to_string()));
    let lit = app.ui.choice_menu.as_ref().map_or(current as usize, |m| m.cursor);
    let columns = columns(options.len());
    let mut grid = column![].spacing(2);
    for (r, chunk) in options.chunks(columns).enumerate() {
        let mut line = row![].spacing(2);
        for (c, label) in chunk.iter().enumerate() {
            let i = r * columns + c;
            line = line.push(
                button(
                    row![picture(pictures.and_then(|p| p.pictures.get(i)), PICTURE), text(*label).size(style::TEXT_SIZE)]
                        .spacing(10)
                        .align_y(Alignment::Center),
                )
                .width(Length::Fixed(OPTION_W))
                .padding(4)
                .style(style::option_button(i == current as usize, i == lit))
                .on_press(Message::PickChoice(id, pd.key, i as u32)),
            );
        }
        grid = grid.push(line);
    }
    // Scrolls when the window is too low for all of it.
    container(scrollable(grid).style(style::scrollable)).padding(4).style(style::popup).into()
}

/// A picture in its frame, or the empty frame until the engine has drawn it.
fn picture<'a>(handle: Option<&image::Handle>, side: f32) -> Element<'a, Message> {
    let inner: Element<'a, Message> = match handle {
        Some(h) => image(h.clone()).width(Length::Fixed(side)).height(Length::Fixed(side)).into(),
        None => space().width(Length::Fixed(side)).height(Length::Fixed(side)).into(),
    };
    container(inner).padding(1).style(style::picture_frame).into()
}
