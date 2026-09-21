//! Node search popup (Space, double-click on the canvas, or a link dropped on empty space).

use iced::widget::{Id, column, container, row, scrollable, space, text, text_input};
use iced::{Alignment, Element, Length, Size};
use brewed_texture_core::NodeKind;
use brewed_texture_core::edit::accepts_link;

use crate::app::{AddMenu, AddMenuMessage, App, Message, menu_index};
use crate::style;

pub const SEARCH_ID: &str = "brewed-add-node-search";
pub const LIST_ID: &str = "brewed-add-node-list";

const WIDTH: f32 = 320.0;
/// Rows have a fixed height so the list can follow the keyboard selection.
const ROW_H: f32 = 26.0;
const ROW_PITCH: f32 = ROW_H + 1.0;
const MIN_ROWS: usize = 3;
const MAX_ROWS: usize = 12;
/// Two lines of description under the list.
const HINT_H: f32 = 32.0;
/// Everything around the list: padding, title, search field, description, spacing.
const CHROME_H: f32 = 8.0 + 18.0 + 6.0 + 28.0 + 6.0 + 6.0 + HINT_H + 8.0;

pub fn search_id() -> Id {
    Id::new(SEARCH_ID)
}

pub fn list_id() -> Id {
    Id::new(LIST_ID)
}

/// Node types matching the query (and able to take the dropped link, if any).
pub fn items(app: &App) -> Vec<NodeKind> {
    let Some(menu) = &app.ui.add_menu else { return Vec::new() };
    let query = menu.query.trim().to_lowercase();
    let pending = menu.pending.as_ref().and_then(|s| app.graph().socket(s).map(|info| (info.ty, s.output)));
    let mut kinds: Vec<NodeKind> = NodeKind::ALL
        .iter()
        .copied()
        .filter(|k| k.def().matches(&query))
        .filter(|k| pending.is_none_or(|(ty, output)| accepts_link(*k, ty, output)))
        .collect();
    // Names that start with the query first.
    if !query.is_empty() {
        kinds.sort_by_key(|k| !k.def().name.to_lowercase().starts_with(&query));
    }
    kinds
}

/// Height of the visible part of the list for `count` items: the popup grows with the
/// results, up to [`MAX_ROWS`].
fn list_height(count: usize) -> f32 {
    count.clamp(MIN_ROWS, MAX_ROWS) as f32 * ROW_PITCH
}

/// Scroll offset that brings row `index` into view, if it is not visible at `offset`.
pub fn scroll_to_row(index: usize, offset: f32, count: usize) -> Option<f32> {
    let top = index as f32 * ROW_PITCH;
    let visible = list_height(count);
    if top < offset {
        Some(top)
    } else if top + ROW_PITCH > offset + visible {
        Some(top + ROW_PITCH - visible)
    } else {
        None
    }
}

pub fn view<'a>(app: &'a App, menu: &'a AddMenu) -> Element<'a, Message> {
    let kinds = items(app);
    let selected = menu_index(Some(menu), kinds.len());
    let title = if menu.pending.is_some() { "Add and connect" } else { "Add node" };
    let mut list = column![].spacing(ROW_PITCH - ROW_H);
    for (i, kind) in kinds.iter().enumerate() {
        let d = kind.def();
        let content = row![
            container(space().width(3).height(14)).style(style::chip(style::category_color(d.category))),
            text(d.name).size(style::TEXT_SIZE),
            space().width(Length::Fill),
            text(d.category.name()).size(style::SMALL).color(style::TEXT_FAINT),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        list = list.push(
            iced::widget::button(container(content).height(Length::Fill).align_y(Alignment::Center))
                .width(Length::Fill)
                .height(Length::Fixed(ROW_H))
                .padding([0, 8])
                .style(style::row_button(i == selected))
                .on_press(Message::AddMenu(AddMenuMessage::Choose(*kind))),
        );
    }
    if kinds.is_empty() {
        list = list.push(container(text("No matching node").size(style::SMALL).color(style::TEXT_DIM)).padding(8));
    }
    let hint = kinds.get(selected).map_or("", |k| k.def().description);
    let body = column![
        row![
            text(title).size(style::TEXT_SIZE).font(style::bold()),
            space().width(Length::Fill),
            text("↑↓  Enter  Esc").size(style::SMALL).color(style::TEXT_FAINT)
        ]
        .align_y(Alignment::Center),
        text_input("Search…", &menu.query)
            .id(search_id())
            .on_input(|q| Message::AddMenu(AddMenuMessage::Query(q)))
            .on_submit(Message::AddMenu(AddMenuMessage::Submit))
            .padding([5, 8])
            .size(style::TEXT_SIZE)
            .style(style::text_input),
        scrollable(list)
            .id(list_id())
            .height(Length::Fixed(list_height(kinds.len())))
            .on_scroll(|viewport| Message::AddMenu(AddMenuMessage::Scrolled(viewport.absolute_offset().y)))
            .style(style::scrollable),
        container(text(hint).size(style::SMALL).color(style::TEXT_DIM)).height(Length::Fixed(HINT_H)),
    ]
    .spacing(6);
    let size = Size::new(WIDTH, CHROME_H + list_height(kinds.len()));
    crate::view::popup_layer(
        container(body).padding(8).style(style::popup),
        menu.at,
        size,
        crate::view::window_size(app),
        Message::AddMenu(AddMenuMessage::Close),
    )
}
