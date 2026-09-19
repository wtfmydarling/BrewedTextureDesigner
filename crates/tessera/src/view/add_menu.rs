//! Node search popup (Space, double-click on the canvas, or a link dropped on empty space).

use iced::widget::{Id, column, container, row, scrollable, space, text, text_input};
use iced::{Alignment, Element, Length, Size};
use tessera_core::NodeKind;
use tessera_core::edit::accepts_link;

use crate::app::{AddMenu, AddMenuMessage, App, Message, menu_index};
use crate::style;

pub const SEARCH_ID: &str = "tessera-add-node-search";

pub fn search_id() -> Id {
    Id::new(SEARCH_ID)
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

pub fn view<'a>(app: &'a App, menu: &'a AddMenu) -> Element<'a, Message> {
    let kinds = items(app);
    let selected = menu_index(Some(menu), kinds.len());
    let title = if menu.pending.is_some() { "Add and connect" } else { "Add node" };
    let mut list = column![].spacing(1);
    for (i, kind) in kinds.iter().enumerate() {
        let d = kind.def();
        list = list.push(
            iced::widget::button(
                row![
                    container(space().width(3).height(14)).style(style::chip(style::category_color(d.category))),
                    text(d.name).size(style::TEXT_SIZE),
                    space().width(Length::Fill),
                    text(d.category.name()).size(style::SMALL).color(style::TEXT_FAINT),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding([4, 8])
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
        scrollable(list).height(Length::Fill).style(style::scrollable),
        text(hint).size(style::SMALL).color(style::TEXT_DIM),
    ]
    .spacing(6);
    let size = Size::new(320.0, 420.0);
    crate::view::popup_layer(
        container(body).padding(8).height(Length::Fixed(size.height)).style(style::popup),
        menu.at,
        size,
        crate::view::window_size(app),
        Message::AddMenu(AddMenuMessage::Close),
    )
}
