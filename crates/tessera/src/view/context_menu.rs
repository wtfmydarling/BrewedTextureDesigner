//! Right-click menus of the graph canvas.

use iced::widget::{button, column, container, row, rule, space, text};
use iced::{Alignment, Element, Length, Size};
use tessera_core::{Edit, NodeKind};

use crate::app::{App, ContextMenu, ContextTarget, GraphMessage, Message};
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
    container(rule::horizontal(1)).padding([3, 6]).into()
}

pub fn view<'a>(app: &'a App, menu: &'a ContextMenu) -> Element<'a, Message> {
    let items: Vec<Element<'a, Message>> = match &menu.target {
        ContextTarget::Node(id) => {
            let node = app.graph().node(*id);
            let can_bypass = node.is_some_and(|n| n.def().has_image() && !n.def().inputs.is_empty() && n.kind != NodeKind::OUTPUT);
            let bypassed = node.is_some_and(|n| n.bypass);
            vec![
                item("Show in 2D View", "double-click", Some(Message::Graph(GraphMessage::Preview(*id)))),
                item(if bypassed { "Enable Node" } else { "Bypass Node" }, "Ctrl+B", can_bypass.then_some(Message::ToggleBypass(*id))),
                separator(),
                item("Duplicate", "Ctrl+D", Some(Message::Duplicate)),
                item("Copy", "Ctrl+C", Some(Message::Copy)),
                item("Cut", "Ctrl+X", Some(Message::Cut)),
                item("Disconnect Inputs", "", Some(Message::Edit(Edit::DisconnectInputs(*id)))),
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
        ContextTarget::Canvas { graph_pos } => vec![
            item(
                "Add Node…",
                "Space",
                Some(Message::Graph(GraphMessage::OpenAddMenu { at: menu.at, graph_pos: *graph_pos })),
            ),
            item("Paste", "Ctrl+V", Some(Message::Paste)),
            separator(),
            item("Select All", "Ctrl+A", Some(Message::SelectAll)),
            item("Frame All", "F", Some(Message::Frame)),
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
