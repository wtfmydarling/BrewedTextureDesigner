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
    container(rule::horizontal(1).style(|_| rule::Style {
        color: style::LINE,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }))
    .padding([3, 6])
    .into()
}

/// The colors a comment can be given, as a row of swatches.
fn colors<'a>(app: &App, id: tessera_core::CommentId) -> Element<'a, Message> {
    let current = app.graph().comment(id).map_or(0, |c| c.color);
    let swatches = (0..style::COMMENT_COLORS.len() as u8).map(|i| {
        button(space().width(18).height(18))
            .style(style::swatch_button(style::comment_color(i), i == current))
            .on_press(Message::FromMenu(Box::new(Message::Edit(Edit::SetCommentColor { id, color: i }))))
            .into()
    });
    container(row(swatches).spacing(4)).padding([4, 10]).into()
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
            colors(app, *id),
            separator(),
            item("Delete Comment", "Del", Some(Message::Edit(Edit::RemoveComments(vec![*id])))),
        ],
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
    // The row of comment colors is taller than a menu entry.
    let extra = if matches!(menu.target, ContextTarget::Comment(_)) { 14.0 } else { 0.0 };
    let height = items.len() as f32 * 27.0 + 12.0 + extra;
    let size = Size::new(230.0, height);
    crate::view::popup_layer(
        container(column(items).spacing(1)).padding(4).style(style::popup),
        menu.at,
        size,
        crate::view::window_size(app),
        Message::Context(None),
    )
}
