//! Choices picked by how they look. What a noise or a shape is called matters less than
//! what it looks like, so the field shows the node's picture next to the option's name, and
//! its menu shows every option as a picture of what a node of its kind makes with it, as the
//! node is when it is added: the same pictures in every menu of the kind, drawn once. A long
//! menu has a search field, which finds options by their names and by what they look like.

use iced::widget::{Id, button, column, container, image, row, scrollable, space, text, text_input};
use iced::{Alignment, Element, Length};
use brewed_texture_core::graph::Node;
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
/// Space between the options.
const GAP: f32 = 2.0;

pub const SEARCH_ID: &str = "brewed-choice-search";
pub const LIST_ID: &str = "brewed-choice-list";

pub fn search_id() -> Id {
    Id::new(SEARCH_ID)
}

pub fn list_id() -> Id {
    Id::new(LIST_ID)
}

/// Whether the menu of a choice has a search field: one with more options than fit in a
/// glance has.
pub fn searched(pd: &ParamDef) -> bool {
    pd.enum_options().len() > 12
}

/// The options a search leaves, in their order.
pub fn shown(pd: &ParamDef, query: &str) -> Vec<usize> {
    (0..pd.enum_options().len()).filter(|&i| pd.option_matches(i, query)).collect()
}

/// Columns of a menu of `options` options: a short list stays a list, a long one spreads
/// out so it fits under (or over) the field.
pub fn columns(options: usize) -> usize {
    match options {
        0..=6 => 1,
        7..=12 => 2,
        13..=24 => 3,
        _ => 4,
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
    let menu: Element<'a, Message> = if open { menu(app, node, pd, current) } else { space().into() };
    dropdown(face, menu, open).on_dismiss(Message::ChoiceMenu(None)).into()
}

/// Every option the search leaves: its picture, then its name.
fn menu<'a>(app: &'a App, node: &'a Node, pd: &'static ParamDef, current: u32) -> Element<'a, Message> {
    let id = node.id;
    let options = pd.enum_options();
    let pictures = app.res.choices.get(&(node.kind.id(), pd.key));
    let (lit, query) = app.ui.choice_menu.as_ref().map_or((current as usize, ""), |m| (m.cursor, m.query.as_str()));
    // As many columns, and as wide, whatever the search leaves, so the menu keeps its place.
    let columns = columns(options.len());
    let width = Length::Fixed(columns as f32 * OPTION_W + (columns - 1) as f32 * GAP);
    let shown = shown(pd, query);
    let mut grid = column![].spacing(GAP).width(width);
    for chunk in shown.chunks(columns) {
        let mut line = row![].spacing(GAP);
        for &i in chunk {
            line = line.push(
                button(
                    row![picture(pictures.and_then(|p| p.get(i)), PICTURE), text(options[i]).size(style::TEXT_SIZE)]
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
    if shown.is_empty() {
        grid = grid.push(container(text("Nothing matches").size(style::SMALL).color(style::TEXT_DIM)).padding(8));
    }
    // Scrolls when the window is too low for all of it.
    let list = scrollable(grid).id(list_id()).style(style::scrollable);
    let body: Element<'a, Message> = if searched(pd) {
        column![
            text_input("Search…", query)
                .id(search_id())
                .on_input(Message::ChoiceSearch)
                .padding([5, 8])
                .size(style::TEXT_SIZE)
                .width(width)
                .style(style::text_input),
            list,
        ]
        .spacing(4)
        .into()
    } else {
        list.into()
    };
    container(body).padding(4).style(style::popup).into()
}

/// A picture in its frame, or the empty frame until the engine has drawn it.
fn picture<'a>(handle: Option<&image::Handle>, side: f32) -> Element<'a, Message> {
    let inner: Element<'a, Message> = match handle {
        Some(h) => image(h.clone()).width(Length::Fixed(side)).height(Length::Fixed(side)).into(),
        None => space().width(Length::Fixed(side)).height(Length::Fixed(side)).into(),
    };
    container(inner).padding(1).style(style::picture_frame).into()
}
