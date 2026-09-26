//! Right-click menus of the graph canvas.

use iced::widget::{button, column, container, row, rule, space, text};
use iced::{Alignment, Element, Length, Size};
use brewed_texture_core::{Edit, NodeKind};

use crate::app::{App, ColorTarget, ContextMenu, ContextTarget, ExplorerMessage, GraphMessage, Message};
use crate::style;

fn item<'a>(label: impl Into<String>, shortcut: &'a str, message: Option<Message>) -> Element<'a, Message> {
    let enabled = message.is_some();
    button(
        row![
            text(label.into()).size(style::TEXT_SIZE).color(if enabled { style::TEXT } else { style::TEXT_FAINT }),
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

/// What is done to the files of a package: linked, imported (held in the package), those
/// linked held in it, and those no node reads taken out.
fn file_items<'a>(app: &App, key: u64) -> Vec<Element<'a, Message>> {
    let package = app.package_doc(key).map(|d| d.package());
    let linked: Vec<String> = package
        .map(|p| {
            p.resources
                .iter()
                .filter(|r| !r.is_embedded() && app.res.files.get(&r.path).is_none_or(|f| f.there))
                .map(|r| r.path.clone())
                .collect()
        })
        .unwrap_or_default();
    let unused = package.is_some_and(|p| !p.unused_resources().is_empty());
    let explorer = |m: ExplorerMessage| Message::Explorer(m);
    vec![
        item("Link Files…", "", Some(explorer(ExplorerMessage::LinkFiles(key)))),
        item("Import Files…", "", Some(explorer(ExplorerMessage::ImportFiles(key)))),
        item("Embed the Linked Files", "", (!linked.is_empty()).then(|| explorer(ExplorerMessage::Embed(key, linked)))),
        item("Remove Unused Files", "", unused.then(|| explorer(ExplorerMessage::RemoveUnused(key)))),
    ]
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
                item(
                    format!("Create Graph from Selection ({})", app.ui.selection.len().max(1)),
                    "Ctrl+G",
                    Some(Message::GraphFromSelection),
                ),
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
            let mut items = vec![item("New Graph", "", Some(Message::Explorer(ExplorerMessage::NewGraph(key))))];
            items.extend(file_items(app, key));
            items.extend([
                item("Collect Files", "", Some(Message::Explorer(ExplorerMessage::CollectFiles(key)))),
                separator(),
                item("Save", "", Some(Message::Explorer(ExplorerMessage::Save(key)))),
                item("Save As…", "", Some(Message::Explorer(ExplorerMessage::SaveAs(key)))),
                separator(),
                item("Close", "", Some(Message::Explorer(ExplorerMessage::Close(key)))),
            ]);
            items
        }
        ContextTarget::ResourcesFolder(key) => file_items(app, *key),
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
            let key = *key;
            // As the files were looked at last (not looked at yet: taken as there).
            let there = app.res.files.get(path).is_none_or(|f| f.there);
            let kind = brewed_texture_core::ResourceKind::of_path(path);
            let held = brewed_texture_core::files::is_embedded(path);
            let explorer = |m: ExplorerMessage| Some(Message::Explorer(m));
            let mut items = vec![item(
                "Add a Node Reading It",
                "double-click",
                kind.filter(|_| there).and_then(|k| explorer(ExplorerMessage::AddResourceNode(path.clone(), k, None))),
            )];
            if held {
                items.push(item("Save as a File and Link It…", "", explorer(ExplorerMessage::SaveResourceAs(key, path.clone()))));
            } else {
                items.push(item(
                    "Embed in the Package",
                    "",
                    there.then(|| Message::Explorer(ExplorerMessage::Embed(key, vec![path.clone()]))),
                ));
                items.push(item(if there { "Relink…" } else { "Locate…" }, "", explorer(ExplorerMessage::Locate(key, path.clone()))));
                // (A file edited elsewhere is read again by itself; a disk that says nothing of
                // it, on the network say, may need asking.)
                items.push(item("Read Again", "", there.then(|| Message::ReloadFile(path.clone()))));
            }
            items.extend([separator(), item("Remove from Package", "", explorer(ExplorerMessage::RemoveResource(key, path.clone())))]);
            items
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
