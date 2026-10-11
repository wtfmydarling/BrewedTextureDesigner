//! Node library: search, categories, drag and drop onto the graph (or double click to
//! add a node in the middle of the graph view). After the program's own nodes, the graphs of
//! the library folders (Settings → Library), under the categories they say.

use iced::widget::{button, column, container, row, space, text, text_input, tooltip};
use iced::{Alignment, Element, Length};
use brewed_texture_core::{Category, NodeKind};

use crate::app::library::LibraryGraph;
use crate::app::{App, DragPayload, Message};
use crate::icons::{Icon, icon};
use crate::style;
use crate::widget::scroll::scroll_box;
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

/// A graph of a library folder: as a node, what it says it makes, and its package.
fn graph_item(g: &LibraryGraph) -> Element<'static, Message> {
    let content = row![icon(Icon::Graph, 12.0, style::TEXT_DIM), text(g.name.clone()).size(style::TEXT_SIZE)].spacing(8).align_y(Alignment::Center);
    let payload = DragPayload::Library { package: g.package.clone(), graph: g.graph, name: g.name.clone() };
    let entry = pressable(content).padding([4, 8]).on_press(Message::PanelPress(payload.clone())).on_double_click(Message::PanelAdd(payload));
    let tip = if g.description.is_empty() { g.package.clone() } else { format!("{}\n\n{}", g.description, g.package) };
    crate::view::tip(entry, tip, tooltip::Position::Right)
}

/// The header of a category: folded or not, its name, how many it has.
fn header<'a>(folded: bool, name: String, marker: Option<Icon>, count: usize, on_press: Message) -> Element<'a, Message> {
    let mut label = row![icon(if folded { Icon::ChevronRight } else { Icon::ChevronDown }, 12.0, style::TEXT_DIM)].spacing(6).align_y(Alignment::Center);
    if let Some(marker) = marker {
        label = label.push(icon(marker, 12.0, style::TEXT_FAINT));
    }
    button(
        label
            .push(crate::view::caption(&name))
            .push(space().width(Length::Fill))
            .push(text(count.to_string()).size(style::SMALL).color(style::TEXT_FAINT)),
    )
    .width(Length::Fill)
    .padding([5, 6])
    .style(style::flat_button)
    .on_press(on_press)
    .into()
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
        list = list.push(header(collapsed, category.name().to_string(), None, kinds.len(), Message::LibraryToggle(category)));
        if !collapsed {
            for kind in kinds {
                list = list.push(item(kind));
            }
        }
    }
    // The graphs of the library folders, by the categories they say (in order already).
    let graphs: Vec<&LibraryGraph> = app.library.graphs.iter().filter(|g| g.matches(&query)).collect();
    let mut at = 0;
    while at < graphs.len() {
        let category = &graphs[at].category;
        let end = graphs[at..].iter().position(|g| g.category != *category).map_or(graphs.len(), |n| at + n);
        found += end - at;
        let folded = query.is_empty() && app.ui.library_folded.contains(category);
        list = list.push(header(folded, category.clone(), Some(Icon::Package), end - at, Message::LibraryFold(category.clone())));
        if !folded {
            for g in &graphs[at..end] {
                list = list.push(graph_item(g));
            }
        }
        at = end;
    }
    if found == 0 {
        list = list.push(container(text("No matching node").size(style::SMALL).color(style::TEXT_DIM)).padding(8));
    }

    let search = text_input("Search nodes…", &app.ui.library_search)
        .on_input(Message::LibrarySearch)
        .padding([5, 8])
        .size(style::TEXT_SIZE)
        .style(style::text_input);

    // Looked at again on a click (what was put in the library folders meanwhile shows); none
    // while there is no folder.
    let tools: Element<'_, Message> = if app.settings.library_folders.is_empty() {
        space().into()
    } else {
        let spin = if app.library.looking() { style::TEXT_FAINT } else { style::TEXT_DIM };
        crate::view::tip(
            button(icon(Icon::Reset, 13.0, spin)).padding(3).style(style::flat_button).on_press_maybe((!app.library.looking()).then_some(Message::LookAtLibrary)),
            "Look at the library folders again",
            tooltip::Position::Bottom,
        )
    };

    column![
        crate::view::pane_header("Library", tools),
        container(search).padding([2, 8]),
        scroll_box(container(list).padding([6, 6])).height(Length::Fill),
    ]
    .spacing(6)
    .into()
}
