//! Choices picked by how they look. What a noise or a shape is called matters less than
//! what it looks like, so the field shows the node's picture next to the option's name, and
//! its menu shows every option as a picture of what a node of its kind makes with it, as the
//! node is when it is added: the same pictures in every menu of the kind, drawn once. A long
//! menu has a search field, which finds options by their names and by what they look like.
//!
//! And choices of many options under headings (an Output's usage): the menu shows each
//! heading with its options under it, in columns, each option saying what it is where it is
//! pointed at; the same search, the same keys.

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

/// The options a search leaves, in their order (under their headings, in the order the menu
/// shows them).
pub fn shown(pd: &ParamDef, query: &str) -> Vec<usize> {
    if pd.groups.is_empty() {
        return (0..pd.enum_options().len()).filter(|&i| pd.option_matches(i, query)).collect();
    }
    pd.groups.iter().flat_map(|(_, of)| of.iter().map(|&i| i as usize)).filter(|&i| pd.option_matches(i, query)).collect()
}

/// How many options a row of the menu of `pd` has: one, in a menu under headings, which the
/// keys go down option after option.
pub fn columns_of(pd: &ParamDef) -> usize {
    if pd.groups.is_empty() { columns(pd.enum_options().len()) } else { 1 }
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

/// Width of a column of a menu under headings.
const GROUPED_W: f32 = 190.0;

/// Where `heights` (of headings and their options) split into `count` runs, the tallest as low
/// as can be: the end of each run.
fn even_runs(heights: &[usize], count: usize) -> Vec<usize> {
    let n = heights.len();
    let count = count.clamp(1, n.max(1));
    let sum = |a: usize, b: usize| heights[a..b].iter().sum::<usize>();
    // best[k][i]: the lowest tallest run of the first i headings in k runs, and where the last
    // one starts.
    let mut best = vec![vec![(usize::MAX, 0); n + 1]; count + 1];
    best[0][0] = (0, 0);
    for k in 1..=count {
        for i in 1..=n {
            for j in (k - 1)..i {
                let (before, _) = best[k - 1][j];
                if before == usize::MAX {
                    continue;
                }
                let tallest = before.max(sum(j, i));
                if tallest < best[k][i].0 {
                    best[k][i] = (tallest, j);
                }
            }
        }
    }
    let mut ends = Vec::with_capacity(count);
    let (mut k, mut i) = (count, n);
    while k > 0 && i > 0 {
        ends.push(i);
        i = best[k][i].1;
        k -= 1;
    }
    ends.reverse();
    ends
}

/// The field of a choice of options under headings, and its menu while it is open: the
/// option's name, its heading before it.
pub fn grouped_field<'a>(app: &'a App, node: &'a Node, pd: &'static ParamDef, current: u32) -> Element<'a, Message> {
    let id = node.id;
    let open = app.ui.choice_menu.as_ref().is_some_and(|m| m.node == id && m.key == pd.key);
    let label = pd.enum_options().get(current as usize).copied().unwrap_or("?");
    let heading = pd.groups.iter().find(|(_, of)| of.contains(&current)).map(|(h, _)| *h).filter(|h| *h != label);
    let mut face = row![].spacing(6).align_y(Alignment::Center);
    if let Some(h) = heading {
        face = face.push(text(h).size(style::SMALL).color(style::TEXT_DIM).wrapping(text::Wrapping::None));
    }
    let face = face.push(text(label).size(style::TEXT_SIZE).wrapping(text::Wrapping::None)).push(space().width(Length::Fill)).push(icon(
        Icon::ChevronDown,
        12.0,
        style::TEXT_DIM,
    ));
    let face = button(face).width(Length::Fill).padding([3, 6]).style(style::choice_field(open)).on_press(Message::ChoiceMenu(if open {
        None
    } else {
        Some((id, pd.key))
    }));
    let menu: Element<'a, Message> = if open { grouped_menu(app, node, pd, current) } else { space().into() };
    dropdown(face, menu, open).on_dismiss(Message::ChoiceMenu(None)).into()
}

/// The headings and the options under them that the search leaves, in columns as even as the
/// headings let them be.
fn grouped_menu<'a>(app: &'a App, node: &'a Node, pd: &'static ParamDef, current: u32) -> Element<'a, Message> {
    let id = node.id;
    let options = pd.enum_options();
    let (lit, query) = app.ui.choice_menu.as_ref().map_or((current as usize, ""), |m| (m.cursor, m.query.as_str()));
    let groups: Vec<(&str, Vec<usize>)> = pd
        .groups
        .iter()
        .map(|(h, of)| (*h, of.iter().map(|&i| i as usize).filter(|&i| pd.option_matches(i, query)).collect::<Vec<_>>()))
        .filter(|(_, of)| !of.is_empty())
        .collect();
    // Up to four columns, a dozen lines or more each, each a run of headings: as low as the
    // headings let the tallest be.
    let heights: Vec<usize> = groups.iter().map(|(_, of)| of.len() + 1).collect();
    let count = heights.iter().sum::<usize>().div_ceil(12).clamp(1, 4).min(groups.len().max(1));
    let width = Length::Fixed(count as f32 * GROUPED_W + (count - 1) as f32 * 8.0);
    let mut columns: Vec<Vec<&(&str, Vec<usize>)>> = Vec::new();
    let mut start = 0;
    for end in even_runs(&heights, count) {
        columns.push(groups[start..end].iter().collect());
        start = end;
    }
    let mut grid = row![].spacing(8);
    for column_groups in columns {
        let mut col = column![].spacing(1).width(Length::Fixed(GROUPED_W));
        for (heading, of) in column_groups {
            col = col.push(container(text(*heading).size(style::TEXT_SIZE).font(style::heavy()).color(style::TEXT_BRIGHT)).padding([5, 6]));
            for &i in of {
                let option = button(text(options[i]).size(style::TEXT_SIZE).wrapping(text::Wrapping::None))
                    .width(Length::Fill)
                    .padding([3, 10])
                    .style(style::option_button(i == current as usize, i == lit))
                    .on_press(Message::PickChoice(id, pd.key, i as u32));
                col = col.push(match pd.tips.get(i) {
                    Some(tip) if !tip.is_empty() => crate::view::tip(option, *tip, iced::widget::tooltip::Position::Right),
                    _ => option.into(),
                });
            }
        }
        grid = grid.push(col);
    }
    let mut body = column![
        text_input("Search…", query)
            .id(search_id())
            .on_input(Message::ChoiceSearch)
            .padding([5, 8])
            .size(style::TEXT_SIZE)
            .width(width)
            .style(style::text_input),
    ]
    .spacing(6);
    if groups.is_empty() {
        body = body.push(container(text("Nothing matches").size(style::SMALL).color(style::TEXT_DIM)).padding(8));
    } else {
        body = body.push(scrollable(grid).id(list_id()).style(style::scrollable));
    }
    container(body).padding(6).style(style::popup).into()
}

/// A picture in its frame, or the empty frame until the engine has drawn it.
fn picture<'a>(handle: Option<&image::Handle>, side: f32) -> Element<'a, Message> {
    let inner: Element<'a, Message> = match handle {
        Some(h) => image(h.clone()).width(Length::Fixed(side)).height(Length::Fixed(side)).into(),
        None => space().width(Length::Fixed(side)).height(Length::Fixed(side)).into(),
    };
    container(inner).padding(1).style(style::picture_frame).into()
}
