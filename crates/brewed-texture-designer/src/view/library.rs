//! Node library: search, categories, drag and drop onto the graph (or double click to
//! add a node in the middle of the graph view).

use iced::widget::{column, container, row, scrollable, space, text, text_input, tooltip};
use iced::{Alignment, Element, Length};
use brewed_texture_core::{Category, NodeKind};

use crate::app::{App, DragPayload, Message};
use crate::icons::{Icon, icon};
use crate::style;
use crate::widget::pressable::pressable;

fn item(kind: NodeKind) -> Element<'static, Message> {
    let d = kind.def();
    let content = row![
        container(space().width(3).height(14)).style(style::chip(style::category_color(d.category))),
        text(d.name).size(style::TEXT_SIZE),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let entry = pressable(content)
        .padding([4, 8])
        .on_press(Message::PanelPress(DragPayload::Node(kind)))
        .on_double_click(Message::PanelAdd(DragPayload::Node(kind)));
    crate::view::tip(entry, d.description, tooltip::Position::Right)
}

pub fn view(app: &App) -> Element<'_, Message> {
    let query = app.ui.library_search.trim().to_lowercase();
    let mut list = column![].spacing(1);
    let mut found = 0;
    for category in Category::ALL {
        let kinds: Vec<NodeKind> = NodeKind::in_category(category).filter(|k| k.def().matches(&query)).collect();
        if kinds.is_empty() {
            continue;
        }
        found += kinds.len();
        // A search always shows its results, even in collapsed categories.
        let collapsed = query.is_empty() && app.ui.library_collapsed.contains(&category);
        let header = iced::widget::button(
            row![
                icon(if collapsed { Icon::ChevronRight } else { Icon::ChevronDown }, 12.0, style::TEXT_DIM),
                crate::view::caption(category.name()),
                space().width(Length::Fill),
                text(kinds.len().to_string()).size(style::SMALL).color(style::TEXT_FAINT),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([5, 6])
        .style(style::flat_button)
        .on_press(Message::LibraryToggle(category));
        list = list.push(header);
        if !collapsed {
            for kind in kinds {
                list = list.push(item(kind));
            }
        }
    }
    if found == 0 {
        list = list.push(container(text("No matching node").size(style::SMALL).color(style::TEXT_DIM)).padding(8));
    }

    let search = text_input("Search nodes…", &app.ui.library_search)
        .on_input(Message::LibrarySearch)
        .padding([5, 8])
        .size(style::TEXT_SIZE)
        .style(style::text_input);

    column![
        crate::view::pane_header("Library", space()),
        container(search).padding([2, 8]),
        scrollable(container(list).padding([6, 6]))
            .height(Length::Fill)
            .style(style::scrollable)
            .direction(scrollable::Direction::Vertical(scrollable::Scrollbar::new().width(6).scroller_width(6))),
    ]
    .spacing(6)
    .into()
}
