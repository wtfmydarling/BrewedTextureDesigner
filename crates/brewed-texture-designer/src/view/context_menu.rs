//! Right-click menus of the graph canvas.

use brewed_texture_core::{Edit, NodeKind};
use iced::widget::{button, container, row, space, text};
use iced::{Alignment, Element, Length};

use crate::app::{App, ColorTarget, ContextMenu, ContextTarget, ExplorerMessage, GraphMessage, Message};
use crate::shortcuts::Command;
use crate::style;
use crate::view::toolbar::{Entry, ITEM_GAP, ITEM_PAD, menu_column, menu_width, row_width, separator};

fn item<'a>(label: impl Into<String>, shortcut: impl Into<String>, message: Option<Message>) -> Entry<'a> {
    let (label, shortcut) = (label.into(), shortcut.into());
    let width = row_width(&label, &shortcut);
    let enabled = message.is_some();
    let element = button(
        row![
            text(label).size(style::TEXT_SIZE).color(if enabled { style::TEXT } else { style::TEXT_FAINT }).wrapping(text::Wrapping::None),
            space().width(Length::Fill),
            text(shortcut).size(style::SMALL).color(style::TEXT_FAINT).wrapping(text::Wrapping::None),
        ]
        .spacing(ITEM_GAP)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([4.0, ITEM_PAD])
    .style(style::row_button(false))
    .on_press_maybe(message.map(|m| Message::FromMenu(Box::new(m))));
    Entry::new(element, width)
}

/// What is done to the files of a package: imported, linked (used where they are on disk) or
/// embedded (held in the package's own file); those linked embedded; and those no node reads
/// taken out. (Both ways in are called importing, which is the word looked for.)
fn file_items<'a>(app: &App, key: u64) -> Vec<Entry<'a>> {
    let package = app.package_doc(key).map(|d| d.package());
    let linked: Vec<String> = package
        .map(|p| p.resources.iter().filter(|r| !r.is_embedded() && app.res.files.get(&r.path).is_none_or(|f| f.there)).map(|r| r.path.clone()).collect())
        .unwrap_or_default();
    let unused = package.is_some_and(|p| !p.unused_resources().is_empty());
    let explorer = |m: ExplorerMessage| Message::Explorer(m);
    vec![
        item("Import and Link Files…", "", Some(explorer(ExplorerMessage::LinkFiles(key)))),
        item("Import and Embed Files…", "", Some(explorer(ExplorerMessage::ImportFiles(key)))),
        item("Embed the Linked Files", "", (!linked.is_empty()).then(|| explorer(ExplorerMessage::Embed(key, linked)))),
        item("Remove Unused Files", "", unused.then(|| explorer(ExplorerMessage::RemoveUnused(key)))),
    ]
}

pub fn view<'a>(app: &'a App, menu: &'a ContextMenu) -> Element<'a, Message> {
    let items: Vec<Entry<'a>> = match &menu.target {
        ContextTarget::Node(id) => {
            let node = app.graph().node(*id);
            let can_bypass = node.is_some_and(|n| n.def().has_image() && !n.def().inputs.is_empty() && n.kind != NodeKind::OUTPUT);
            let bypassed = node.is_some_and(|n| n.bypass);
            let uses_graph = node.is_some_and(|n| n.instance.is_some());
            vec![
                item("Show in 2D View", "double-click", Some(Message::Graph(GraphMessage::Preview(*id)))),
                item(if bypassed { "Enable Node" } else { "Bypass Node" }, app.key_of(Command::Bypass), can_bypass.then_some(Message::ToggleBypass(*id))),
                item("Open Reference", "", uses_graph.then_some(Message::OpenReference(*id))),
                separator(),
                item(
                    format!("Create Graph from Selection ({})", app.ui.selection.len().max(1)),
                    app.key_of(Command::GraphFromSelection),
                    Some(Message::GraphFromSelection),
                ),
                item("Duplicate", app.key_of(Command::Duplicate), Some(Message::Duplicate)),
                item("Copy", app.key_of(Command::Copy), Some(Message::Copy)),
                item("Cut", app.key_of(Command::Cut), Some(Message::Cut)),
                item("Disconnect Inputs", "", Some(Message::Edit(Edit::DisconnectInputs(*id)))),
                item("Frame in a Comment", app.key_of(Command::AddComment), Some(Message::AddComment)),
                item("Export Output…", "", node.filter(|n| n.kind == NodeKind::OUTPUT).map(|n| Message::ExportNode(n.id))),
                separator(),
                item("Delete", app.key_of(Command::Delete), Some(Message::Delete)),
            ]
        }
        ContextTarget::Link(link) => vec![item("Delete Link", app.key_of(Command::Delete), Some(Message::Edit(Edit::RemoveLink(link.clone()))))],
        ContextTarget::Comment(id) => vec![
            item("Edit Text", "double-click", Some(Message::Graph(GraphMessage::EditComment(*id)))),
            item("Color…", "", Some(Message::ColorPicker(Some(ColorTarget::Comment(*id))))),
            separator(),
            item("Delete Comment", app.key_of(Command::Delete), Some(Message::Edit(Edit::RemoveComments(vec![*id])))),
        ],
        ContextTarget::Package(key) => {
            let key = *key;
            let mut items = vec![item("New Graph…", "", Some(Message::NewGraph(crate::app::home::NewGraphMessage::Open(Some(key)))))];
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
        ContextTarget::Explorer => vec![
            item("New Package", "", Some(Message::Explorer(ExplorerMessage::NewPackage))),
            item("Open Package(s)…", app.key_of(Command::Open), Some(Message::Open)),
        ],
        ContextTarget::GraphItem(key, graph) => {
            let (key, graph) = (*key, *graph);
            vec![
                item("Open", "double-click", Some(Message::Explorer(ExplorerMessage::OpenGraph(key, graph)))),
                item("Rename", "", Some(Message::Explorer(ExplorerMessage::StartRename(key, graph)))),
                item("Duplicate", "", Some(Message::Explorer(ExplorerMessage::DuplicateGraph(key, graph)))),
                separator(),
                item("New Graph…", "", Some(Message::NewGraph(crate::app::home::NewGraphMessage::Open(Some(key))))),
                separator(),
                // (Its last graph too: a package may have none.)
                item("Delete", "", Some(Message::Explorer(ExplorerMessage::DeleteGraph(key, graph)))),
            ]
        }
        ContextTarget::ResourceItem(key, path) => {
            let key = *key;
            // As the files were looked at last (not looked at yet: taken as there).
            let there = app.res.files.get(path).is_none_or(|f| f.there);
            let kind = brewed_texture_core::ResourceKind::of_path(path);
            let held = brewed_texture_core::files::is_embedded(path);
            let explorer = |m: ExplorerMessage| Some(Message::Explorer(m));
            let mut items = vec![
                item(
                    "Add a Node Reading It",
                    "double-click",
                    kind.filter(|_| there).and_then(|k| explorer(ExplorerMessage::AddResourceNode(path.clone(), k, None))),
                ),
                item("Rename", "", explorer(ExplorerMessage::StartRenameFile(key, path.clone()))),
            ];
            if held {
                items.push(item("Save as a File and Link It…", "", explorer(ExplorerMessage::SaveResourceAs(key, path.clone()))));
            } else {
                items.push(item("Embed in the Package", "", there.then(|| Message::Explorer(ExplorerMessage::Embed(key, vec![path.clone()])))));
                items.push(item(if there { "Relink…" } else { "Locate…" }, "", explorer(ExplorerMessage::Locate(key, path.clone()))));
                // (A file edited elsewhere is read again by itself; a disk that says nothing of
                // it, on the network say, may need asking.)
                items.push(item("Read Again", "", there.then(|| Message::ReloadFile(path.clone()))));
            }
            items.extend([separator(), item("Remove from Package", "", explorer(ExplorerMessage::RemoveResource(key, path.clone())))]);
            items
        }
        ContextTarget::Canvas { graph_pos } => vec![
            item("Add Node…", app.key_of(Command::AddNode), Some(Message::Graph(GraphMessage::OpenAddMenu { at: menu.at, graph_pos: *graph_pos }))),
            item("Paste", app.key_of(Command::Paste), Some(Message::Paste)),
            separator(),
            item("Select All", app.key_of(Command::SelectAll), Some(Message::SelectAll)),
            item("Frame All", "", Some(Message::FrameAll)),
            item("Add Comment", app.key_of(Command::AddComment), Some(Message::AddComment)),
        ],
    };
    // As wide as its widest item.
    let width = menu_width(&items, 222.0);
    crate::view::popup_layer(container(menu_column(items, width)).padding(4).style(style::popup), menu.at, width + 8.0, Message::Context(None))
}
