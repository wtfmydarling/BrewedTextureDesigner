//! Right-click menus of the graph canvas.

use iced::widget::{button, column, container, row, rule, space, text};
use iced::{Alignment, Element, Length, Size};
use brewed_texture_core::{Edit, NodeKind};

use crate::app::{App, ColorTarget, ContextMenu, ContextTarget, ExplorerMessage, GraphMessage, Message};
use crate::style;

fn item<'a>(label: &'a str, shortcut: &'a str, message: Option<Message>) -> Element<'a, Message> {
    let enabled = message.is_some();
    button(
        row![
            text(label).size(style::TEXT_SIZE).color(if enabled { style::TEXT } else { style::TEXT_FAINT }),
            space().width(Length::Fill),
            text(shortcut).size(style::SMALL).color(style::TEXT_FAINT),
        ]
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([4, 10])
    .style(style::row_button(false))
    .on_press_maybe(message.map(|m| Message::FromMenu(Box::new(m))))
    .into()
}

fn separator<'a>() -> Element<'a, Message> {
    container(rule::horizontal(1).style(|_| rule::Style {
        color: style::LINE,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }))
    .padding([3, 6])
    .into()
}

pub fn view<'a>(app: &'a App, menu: &'a ContextMenu) -> Element<'a, Message> {
    let items: Vec<Element<'a, Message>> = match &menu.target {
        ContextTarget::Node(id) => {
            let node = app.graph().node(*id);
            let can_bypass = node.is_some_and(|n| n.def().has_image() && !n.def().inputs.is_empty() && n.kind != NodeKind::OUTPUT);
            let bypassed = node.is_some_and(|n| n.bypass);
            let uses_graph = node.is_some_and(|n| n.instance.is_some());
            vec![
                item("Show in 2D View", "double-click", Some(Message::Graph(GraphMessage::Preview(*id)))),
                item(if bypassed { "Enable Node" } else { "Bypass Node" }, "Ctrl+B", can_bypass.then_some(Message::ToggleBypass(*id))),
                item("Open Reference", "", uses_graph.then_some(Message::OpenReference(*id))),
                separator(),
                item("Make Subgraph", "Ctrl+G", Some(Message::MakeSubgraph)),
                item("Duplicate", "Ctrl+D", Some(Message::Duplicate)),
                item("Copy", "Ctrl+C", Some(Message::Copy)),
                item("Cut", "Ctrl+X", Some(Message::Cut)),
                item("Disconnect Inputs", "", Some(Message::Edit(Edit::DisconnectInputs(*id)))),
                item("Frame in a Comment", "C", Some(Message::AddComment)),
                item(
                    "Export Output…",
                    "",
                    node.filter(|n| n.kind == NodeKind::OUTPUT).map(|n| Message::ExportNode(n.id)),
                ),
                separator(),
                item("Delete", "Del", Some(Message::Delete)),
            ]
        }
        ContextTarget::Link(link) => vec![item("Delete Link", "Del", Some(Message::Edit(Edit::RemoveLink(link.clone()))))],
        ContextTarget::Comment(id) => vec![
            item("Edit Text", "double-click", Some(Message::Graph(GraphMessage::EditComment(*id)))),
            item("Color…", "", Some(Message::ColorPicker(Some(ColorTarget::Comment(*id))))),
            separator(),
            item("Delete Comment", "Del", Some(Message::Edit(Edit::RemoveComments(vec![*id])))),
        ],
        ContextTarget::Package(key) => {
            let key = *key;
            vec![
                item("New Graph", "", Some(Message::Explorer(ExplorerMessage::NewGraph(key)))),
                item("Link Files…", "", Some(Message::Explorer(ExplorerMessage::LinkFiles(key)))),
                separator(),
                item("Save", "", Some(Message::Explorer(ExplorerMessage::Save(key)))),
                item("Save As…", "", Some(Message::Explorer(ExplorerMessage::SaveAs(key)))),
                separator(),
                item("Close", "", Some(Message::Explorer(ExplorerMessage::Close(key)))),
            ]
        }
        ContextTarget::GraphItem(key, graph) => {
            let (key, graph) = (*key, *graph);
            let many = app.package_doc(key).is_some_and(|d| d.package().graphs.len() > 1);
            vec![
                item("Open", "double-click", Some(Message::Explorer(ExplorerMessage::OpenGraph(key, graph)))),
                item("Rename", "", Some(Message::Explorer(ExplorerMessage::StartRename(key, graph)))),
                item("Duplicate", "", Some(Message::Explorer(ExplorerMessage::DuplicateGraph(key, graph)))),
                separator(),
                item("New Graph", "", Some(Message::Explorer(ExplorerMessage::NewGraph(key)))),
                separator(),
                item("Delete", "", many.then_some(Message::Explorer(ExplorerMessage::DeleteGraph(key, graph)))),
            ]
        }
        ContextTarget::ResourceItem(key, path) => {
            let there = std::path::Path::new(path).is_file();
            let kind = brewed_texture_core::ResourceKind::of_path(path);
            vec![
                item(
                    "Add a Node Reading It",
                    "double-click",
                    kind.filter(|_| there).map(|k| Message::Explorer(ExplorerMessage::AddResourceNode(path.clone(), k, None))),
                ),
                item(if there { "Relink…" } else { "Locate…" }, "", Some(Message::Explorer(ExplorerMessage::Locate(*key, path.clone())))),
                separator(),
                item("Remove from Package", "", Some(Message::Explorer(ExplorerMessage::RemoveResource(*key, path.clone())))),
            ]
        }
        ContextTarget::Canvas { graph_pos } => vec![
            item(
                "Add Node…",
                "Space",
                Some(Message::Graph(GraphMessage::OpenAddMenu { at: menu.at, graph_pos: *graph_pos })),
            ),
            item("Paste", "Ctrl+V", Some(Message::Paste)),
            separator(),
            item("Select All", "Ctrl+A", Some(Message::SelectAll)),
            item("Frame All", "", Some(Message::FrameAll)),
            item("Add Comment", "C", Some(Message::AddComment)),
        ],
    };
    let height = items.len() as f32 * 27.0 + 12.0;
    let size = Size::new(230.0, height);
    crate::view::popup_layer(
        container(column(items).spacing(1)).padding(4).style(style::popup),
        menu.at,
        size,
        crate::view::window_size(app),
        Message::Context(None),
    )
}
