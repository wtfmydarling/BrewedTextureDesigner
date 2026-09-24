//! The Explorer: the packages open, their graphs and the files they use, as in the program
//! this one follows. A double click opens a graph; a graph dragged onto the canvas becomes a
//! node that uses it, a file the node that reads it; every row has its context menu.

use brewed_texture_core::Document;
use brewed_texture_core::package::{Resource, ResourceKind};
use iced::widget::{button, column, container, row, scrollable, space, text, text_input, tooltip};
use iced::{Alignment, Element, Length};

use crate::app::{App, ContextTarget, DragPayload, ExplorerMessage, Message};
use crate::icons::{Icon, icon};
use crate::style;
use crate::view::tip;
use crate::widget::pressable::pressable;

/// Id of the field a graph is renamed in (focused when renaming starts).
pub fn rename_id() -> iced::widget::Id {
    iced::widget::Id::new("brewed-rename-graph")
}

fn explorer(m: ExplorerMessage) -> Message {
    Message::Explorer(m)
}

fn tool<'a>(which: Icon, hint: &'a str, message: Message) -> Element<'a, Message> {
    tip(
        button(icon(which, 14.0, style::TEXT_DIM)).padding(4).style(style::flat_button).on_press(message),
        hint,
        tooltip::Position::Bottom,
    )
}

/// The icon of a file of a package.
fn resource_icon(kind: ResourceKind) -> Icon {
    match kind {
        ResourceKind::Bitmap => Icon::Image,
        ResourceKind::Svg => Icon::Vector,
        ResourceKind::Font => Icon::Font,
    }
}

pub fn view(app: &App) -> Element<'_, Message> {
    let tools = row![
        tool(Icon::Plus, "New package (Ctrl+N)", Message::New),
        tool(Icon::Folder, "Open a package (Ctrl+O)", Message::Open),
    ]
    .spacing(2);
    let mut list = column![].spacing(1);
    for (key, doc) in app.packages() {
        list = list.push(package_rows(app, key, doc));
    }
    column![
        crate::view::pane_header("Explorer", tools),
        scrollable(container(list).padding([4, 6]))
            .height(Length::Fill)
            .style(style::scrollable)
            .direction(scrollable::Direction::Vertical(scrollable::Scrollbar::new().width(6).scroller_width(6))),
    ]
    .into()
}

fn package_rows<'a>(app: &'a App, key: u64, doc: &'a Document) -> Element<'a, Message> {
    let collapsed = app.ui.explorer_collapsed.contains(&key);
    let mut head = row![
        icon(if collapsed { Icon::ChevronRight } else { Icon::ChevronDown }, 12.0, style::TEXT_DIM),
        icon(Icon::Package, 14.0, style::TEXT_DIM),
        container(text(doc.title()).size(style::TEXT_SIZE).font(style::bold()).wrapping(text::Wrapping::None))
            .width(Length::Fill)
            .clip(true),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    if doc.is_dirty() {
        head = head.push(text("•").size(style::TEXT_SIZE).color(style::TEXT_DIM));
    }
    let where_ = doc.path().map_or_else(|| "Not saved yet".to_string(), |p| p.display().to_string());
    let head = pressable(head)
        .padding([4, 4])
        .on_press(explorer(ExplorerMessage::Toggle(key)))
        .on_right_press(explorer(ExplorerMessage::Context(ContextTarget::Package(key))));
    let mut rows = column![tip(head, where_, tooltip::Position::Right)].spacing(1);
    if collapsed {
        return rows.into();
    }
    let editing = key == app.doc_key;
    for g in &doc.package().graphs {
        let renaming = app.ui.renaming_graph.as_ref().filter(|(k, id, _)| *k == key && *id == g.id);
        let row_content: Element<'a, Message> = match renaming {
            Some((_, _, name)) => text_input("Graph name", name)
                .id(rename_id())
                .on_input(|t| explorer(ExplorerMessage::RenameText(t)))
                .on_submit(explorer(ExplorerMessage::CommitRename))
                .size(style::TEXT_SIZE)
                .padding([2, 6])
                .style(style::text_input)
                .into(),
            None => text(&g.name).size(style::TEXT_SIZE).wrapping(text::Wrapping::None).into(),
        };
        let open = editing && doc.current() == g.id;
        let content = row![
            space().width(18),
            icon(Icon::Graph, 13.0, if open { style::TEXT } else { style::TEXT_DIM }),
            container(row_content).width(Length::Fill).clip(true),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        let payload = DragPayload::Graph { key, graph: g.id, name: g.name.clone() };
        let entry = pressable(content)
            .padding([3, 4])
            .selected(open)
            .on_press(Message::PanelPress(payload))
            .on_double_click(explorer(ExplorerMessage::OpenGraph(key, g.id)))
            .on_right_press(explorer(ExplorerMessage::Context(ContextTarget::GraphItem(key, g.id))));
        let hint = if open {
            "Being edited. Drag it onto another graph to use it there as a node."
        } else {
            "Double-click to edit it; drag it onto the graph to use it as a node."
        };
        rows = rows.push(tip(entry, hint, tooltip::Position::Right));
    }
    for r in &doc.package().resources {
        rows = rows.push(resource_row(key, r));
    }
    rows.into()
}

fn resource_row(key: u64, r: &Resource) -> Element<'_, Message> {
    let there = std::path::Path::new(&r.path).is_file();
    let color = if there { style::TEXT_DIM } else { style::TEXT_FAINT };
    let mut content = row![
        space().width(18),
        icon(resource_icon(r.kind), 13.0, color),
        container(
            text(r.name())
                .size(style::SMALL)
                .color(if there { style::TEXT } else { style::TEXT_FAINT })
                .wrapping(text::Wrapping::None)
        )
        .width(Length::Fill)
        .clip(true),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    if !there {
        content = content.push(icon(Icon::Warning, 12.0, style::WARNING));
    }
    let payload = DragPayload::Resource { path: r.path.clone(), kind: r.kind };
    let entry = pressable(content)
        .padding([2, 4])
        .on_press(Message::PanelPress(payload))
        .on_double_click(explorer(ExplorerMessage::AddResourceNode(r.path.clone(), r.kind, None)))
        .on_right_press(explorer(ExplorerMessage::Context(ContextTarget::ResourceItem(key, r.path.clone()))));
    let hint = if there {
        format!("{}\n{}. Drag it onto the graph, or double-click, for a node that reads it.", r.path, r.kind.name())
    } else {
        format!("{}\nNot there: right-click to say where it is now.", r.path)
    };
    tip(entry, hint, tooltip::Position::Right)
}
