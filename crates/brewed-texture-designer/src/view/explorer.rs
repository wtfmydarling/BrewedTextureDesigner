//! The Explorer: the packages open, their graphs and the files they use, as in the program
//! this one follows. A package lists its graphs, and its files in its Resources folder: in a
//! folder of each kind (Bitmaps, Vector Graphics, Fonts) when it has files of more than one.
//! A file shows a picture of it, and whether the package links to it on disk or holds it in
//! its own file. On the right of a row, an ⓘ says what uses the graph or the file (its
//! "Notes", as Substance Designer says it): what nothing uses is what a package can do
//! without. A ⚠ says what is wrong. A double click opens a graph; a graph dragged onto the
//! canvas becomes a node that uses it, a file the node that reads it; every row and folder
//! has its context menu, and so has the Explorer where it lists nothing (a new package, packages
//! opened). A package may have no graph: its files only.

use std::collections::{BTreeMap, HashMap};

use brewed_texture_core::package::{Reader, Resource, ResourceKind};
use brewed_texture_core::{Document, GraphId};
use iced::widget::{button, column, container, image, row, space, text, text_input, tooltip};
use iced::{Alignment, ContentFit, Element, Length};

use crate::app::{App, ContextTarget, DragPayload, ExplorerMessage, Fold, Message};
use crate::icons::{Icon, icon};
use crate::style;
use crate::view::{fitted, tip};
use crate::widget::outside::outside;
use crate::widget::pressable::pressable;
use crate::widget::scroll::scroll_box;

/// Id of the field a graph is renamed in (focused when renaming starts).
pub fn rename_id() -> iced::widget::Id {
    iced::widget::Id::new("brewed-rename-graph")
}

fn explorer(m: ExplorerMessage) -> Message {
    Message::Explorer(m)
}

fn tool<'a>(which: Icon, hint: &'a str, message: Message) -> Element<'a, Message> {
    tip(button(icon(which, 14.0, style::TEXT_DIM)).padding(4).style(style::flat_button).on_press(message), hint, tooltip::Position::Bottom)
}

/// The icon of a file of a package, before (or without) its picture.
fn resource_icon(kind: ResourceKind) -> Icon {
    match kind {
        ResourceKind::Bitmap => Icon::Image,
        ResourceKind::Svg => Icon::Vector,
        ResourceKind::Font => Icon::Font,
    }
}

/// The folder of the files of a kind, when a package has files of more than one kind.
pub fn folder_name(kind: ResourceKind) -> &'static str {
    match kind {
        ResourceKind::Bitmap => "Bitmaps",
        ResourceKind::Svg => "Vector Graphics",
        ResourceKind::Font => "Fonts",
    }
}

/// How far a row is in: a package's graphs and its Resources folder one step, what is in
/// the folder two, what is in a folder of a kind three.
const STEP: f32 = 16.0;

/// What uses each graph and each file of the open packages: who, and how many of their nodes.
#[derive(Default)]
pub struct Uses {
    /// By the package's key and the graph: the graphs whose nodes use it.
    pub graphs: HashMap<(u64, GraphId), Vec<(String, usize)>>,
    /// By the package's key and the file: the nodes that read it (their type, and their graph).
    pub files: HashMap<(u64, String), Vec<(String, usize)>>,
}

impl Uses {
    pub fn of(app: &App) -> Uses {
        let docs = app.packages();
        let by_path: HashMap<String, u64> = docs.iter().filter_map(|(k, d)| Some((d.key()?, *k))).collect();
        let mut graphs: BTreeMap<(u64, GraphId), BTreeMap<String, usize>> = BTreeMap::new();
        let mut files: BTreeMap<(u64, String), BTreeMap<String, usize>> = BTreeMap::new();
        for (key, doc) in &docs {
            for entry in &doc.package().graphs {
                let who = |used_in: u64| {
                    if used_in == *key { entry.name.clone() } else { format!("{} ({})", entry.name, doc.title()) }
                };
                for node in entry.graph.instances() {
                    let r = &node.instance.as_deref().expect("an instance").graph;
                    let used_in = match &r.package {
                        None => Some(*key),
                        Some(path) => by_path.get(path).copied(),
                    };
                    let Some(used_in) = used_in else { continue };
                    let Some(found) = docs.iter().find(|(k, _)| *k == used_in).and_then(|(_, d)| d.package().find(r)) else {
                        continue;
                    };
                    *graphs.entry((used_in, found.id)).or_default().entry(who(used_in)).or_default() += 1;
                }
            }
            for (graph, reader, path) in doc.package().files() {
                let name = doc.package().graph(graph).map_or_else(String::new, |g| g.name.clone());
                let who = match reader {
                    Reader::Node(node) => match &node.instance {
                        Some(i) => format!("'{}' in {name}", i.graph.name),
                        None => format!("{} in {name}", node.def().name),
                    },
                    Reader::Input(input) => format!("the image input '{input}' of {name}"),
                };
                *files.entry((*key, path.to_string())).or_default().entry(who).or_default() += 1;
            }
        }
        Uses {
            graphs: graphs.into_iter().map(|(k, v)| (k, v.into_iter().collect())).collect(),
            files: files.into_iter().map(|(k, v)| (k, v.into_iter().collect())).collect(),
        }
    }
}

/// The notes of what uses something: `verb` ("Used by") and each user, with how many of its
/// nodes when more than one.
pub fn notes(users: &[(String, usize)], verb: &str) -> String {
    let who: Vec<String> = users.iter().map(|(w, n)| if *n > 1 { format!("{w} ×{n}") } else { w.clone() }).collect();
    format!("Notes\n—\n{verb} {}", who.join(", "))
}

/// The width of a column of marks.
const MARK: f32 = 16.0;

/// A mark on the right of a row, with what it says; the marks of all rows are in columns.
fn mark<'a>(which: Icon, color: iced::Color, says: String) -> Element<'a, Message> {
    tip(container(icon(which, 12.0, color)).center_x(MARK), says, tooltip::Position::Right)
}

/// The rightmost mark of a row: ⚠ what is wrong (and, below it, what uses it), else ⓘ what
/// uses it, else nothing.
fn status<'a>(warning: Option<String>, notes: Option<String>) -> Element<'a, Message> {
    match (warning, notes) {
        (Some(w), n) => mark(Icon::Warning, style::WARNING, n.map_or(w.clone(), |n| format!("{w}\n\n{n}"))),
        (None, Some(n)) => mark(Icon::Info, style::TEXT_DIM, n),
        // Nothing to say: the name takes the room (a mark is at the right edge either way, and
        // a narrow Explorer cut names short beside room kept for one).
        (None, None) => space().width(0).into(),
    }
}

pub fn view(app: &App) -> Element<'_, Message> {
    let tools = row![
        tool(Icon::Plus, "New graph from a template, in a new package or one open (Ctrl+N)", Message::NewGraph(crate::app::home::NewGraphMessage::Open(None))),
        tool(Icon::Folder, "Open packages (Ctrl+O)", Message::Open),
    ]
    .spacing(2);
    let uses = Uses::of(app);
    let mut list = column![].spacing(1);
    for (key, doc) in app.packages() {
        list = list.push(package_rows(app, &uses, key, doc));
    }
    // A right click where it lists nothing: a new package, packages opened (a row takes its own).
    let listed = iced::widget::mouse_area(scroll_box(container(list).padding([4, 6])).height(Length::Fill))
        .on_right_press(explorer(ExplorerMessage::Context(ContextTarget::Explorer)));
    column![crate::view::pane_header("Explorer", tools), listed].into()
}

/// A row that folds what is under it.
fn folder<'a>(app: &App, fold: Fold, indent: f32, label: &'a str, count: usize, context: ContextTarget, hint: &'a str) -> Element<'a, Message> {
    let folded = app.ui.explorer_folded.contains(&fold);
    let content = row![
        space().width(indent),
        icon(if folded { Icon::ChevronRight } else { Icon::ChevronDown }, 12.0, style::TEXT_DIM),
        icon(Icon::Folder, 13.0, style::TEXT_DIM),
        container(fitted(label, style::TEXT_SIZE, style::ui_font(), None)).width(Length::Fill).clip(true),
        text(count.to_string()).size(style::SMALL).color(style::TEXT_FAINT),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let entry =
        pressable(content).padding([3, 4]).on_press(explorer(ExplorerMessage::Toggle(fold))).on_right_press(explorer(ExplorerMessage::Context(context)));
    tip(entry, hint, tooltip::Position::Right)
}

fn package_rows<'a>(app: &'a App, uses: &Uses, key: u64, doc: &'a Document) -> Element<'a, Message> {
    let folded = app.ui.explorer_folded.contains(&Fold::Package(key));
    let mut head = row![
        icon(if folded { Icon::ChevronRight } else { Icon::ChevronDown }, 12.0, style::TEXT_DIM),
        icon(Icon::Package, 14.0, style::TEXT_DIM),
        container(fitted(doc.title(), style::TEXT_SIZE, style::bold(), None)).width(Length::Fill).clip(true),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    if doc.is_dirty() {
        head = head.push(text("•").size(style::TEXT_SIZE).color(style::TEXT_DIM));
    }
    let where_ = doc.path().map_or_else(|| "Not saved yet".to_string(), |p| p.display().to_string());
    let head = pressable(head)
        .padding([4, 4])
        .on_press(explorer(ExplorerMessage::Toggle(Fold::Package(key))))
        .on_right_press(explorer(ExplorerMessage::Context(ContextTarget::Package(key))));
    let mut rows = column![tip(head, where_, tooltip::Position::Right)].spacing(1);
    if folded {
        return rows.into();
    }
    let editing = key == app.doc_key;
    for g in &doc.package().graphs {
        let renaming = app.ui.renaming_graph.as_ref().filter(|(k, id, _)| *k == key && *id == g.id);
        let row_content: Element<'a, Message> = match renaming {
            // A click anywhere else in the window names it as Enter does.
            Some((_, _, name)) => outside(
                text_input("Graph name", name)
                    .id(rename_id())
                    .on_input(|t| explorer(ExplorerMessage::RenameText(t)))
                    .on_submit(explorer(ExplorerMessage::CommitRename))
                    .size(style::TEXT_SIZE)
                    .padding([2, 6])
                    .style(style::text_input),
            )
            .on_press(Some(explorer(ExplorerMessage::CommitRename)))
            .into(),
            // (Shortened in its middle where it is too long: graphs called "Graph 2" and "Graph 3"
            // cut at the edge of the column read the same.)
            None => fitted(&g.name, style::TEXT_SIZE, style::ui_font(), None),
        };
        let open = editing && doc.current() == g.id;
        let mut content = row![
            space().width(STEP + 2.0),
            icon(Icon::Graph, 13.0, if open { style::TEXT } else { style::TEXT_DIM }),
            container(row_content).width(Length::Fill).clip(true),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        content = content.push(status(None, uses.graphs.get(&(key, g.id)).map(|users| notes(users, "Used by"))));
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
    // The files: in the Resources folder, in a folder of each kind when there are several.
    let resources = &doc.package().resources;
    if resources.is_empty() {
        return rows.into();
    }
    rows = rows.push(folder(
        app,
        Fold::Resources(key),
        STEP - 12.0,
        "Resources",
        resources.len(),
        ContextTarget::ResourcesFolder(key),
        "The files the package uses: linked where they are on disk, or held in its own file. Right-click to add some.",
    ));
    if app.ui.explorer_folded.contains(&Fold::Resources(key)) {
        return rows.into();
    }
    let kinds: Vec<ResourceKind> = ResourceKind::ALL.into_iter().filter(|k| resources.iter().any(|r| r.kind == *k)).collect();
    if kinds.len() == 1 {
        for r in resources {
            rows = rows.push(resource_row(app, uses, key, r, 2.0 * STEP + 2.0));
        }
        return rows.into();
    }
    for kind in kinds {
        let of_kind: Vec<&Resource> = resources.iter().filter(|r| r.kind == kind).collect();
        rows = rows.push(folder(app, Fold::Kind(key, kind), 2.0 * STEP - 12.0, folder_name(kind), of_kind.len(), ContextTarget::ResourcesFolder(key), ""));
        if !app.ui.explorer_folded.contains(&Fold::Kind(key, kind)) {
            for r in of_kind {
                rows = rows.push(resource_row(app, uses, key, r, 3.0 * STEP + 2.0));
            }
        }
    }
    rows.into()
}

fn resource_row<'a>(app: &'a App, uses: &Uses, key: u64, r: &'a Resource, indent: f32) -> Element<'a, Message> {
    let state = app.res.files.get(&r.path);
    // Not looked at yet: taken as there.
    let there = state.is_none_or(|s| s.there);
    let preview = state.and_then(|s| s.preview.as_ref());
    let picture: Element<'a, Message> = match preview {
        Some(p) => image(p.image.clone()).width(18).height(18).content_fit(ContentFit::Contain).into(),
        None => container(icon(resource_icon(r.kind), 14.0, if there { style::TEXT_DIM } else { style::TEXT_FAINT })).width(18).center_x(18).into(),
    };
    let renaming = app.ui.renaming_file.as_ref().filter(|(k, path, _)| *k == key && *path == r.path);
    let name: Element<'a, Message> = match renaming {
        // A click anywhere else in the window names it as Enter does.
        Some((_, _, name)) => outside(
            text_input("File name", name)
                .id(rename_id())
                .on_input(|t| explorer(ExplorerMessage::RenameFileText(t)))
                .on_submit(explorer(ExplorerMessage::CommitRenameFile))
                .size(style::SMALL)
                .padding([1, 6])
                .style(style::text_input),
        )
        .on_press(Some(explorer(ExplorerMessage::CommitRenameFile)))
        .into(),
        None => fitted(r.name(), style::SMALL, style::ui_font(), Some(if there { style::TEXT } else { style::TEXT_FAINT })),
    };
    let mut content = row![space().width(indent), picture, container(name).width(Length::Fill).clip(true),].spacing(6).align_y(Alignment::Center);
    let bytes = state.and_then(|s| s.bytes).map(crate::previews::bytes);
    let (held, where_) = if r.is_embedded() {
        let size = r.data.as_ref().map(|d| crate::previews::bytes(d.len() as u64)).unwrap_or_default();
        (mark(Icon::Package, style::TEXT_FAINT, format!("In the package: held in its file ({size})")), "Held in the package's file".to_string())
    } else {
        (mark(Icon::Link, style::TEXT_FAINT, format!("Linked: the package uses the file on disk\n{}", r.path)), r.path.clone())
    };
    let warning = (!there).then(|| format!("Warnings\n—\nThe file is not there: {}", r.path));
    let read_by = uses.files.get(&(key, r.path.clone())).map(|users| notes(users, "Read by"));
    content = content.push(held).push(status(warning, read_by));
    let payload = DragPayload::Resource { path: r.path.clone(), kind: r.kind, name: r.name.clone() };
    let entry = pressable(content)
        .padding([2, 4])
        .on_press(Message::PanelPress(payload))
        .on_double_click(explorer(ExplorerMessage::AddResourceNode(r.path.clone(), r.kind, None)))
        .on_right_press(explorer(ExplorerMessage::Context(ContextTarget::ResourceItem(key, r.path.clone()))));
    // Hovered: the file larger, and what it is.
    let mut about = vec![r.name().to_string(), where_];
    match (preview, bytes) {
        (Some(p), Some(b)) => about.push(format!("{}, {b}", p.about)),
        (Some(p), None) => about.push(p.about.clone()),
        (None, Some(b)) => about.push(format!("{}, {b}", r.kind.name())),
        (None, None) => about.push(r.kind.name().to_string()),
    }
    if let Some((_, why)) = state.and_then(|s| s.failed.as_ref()) {
        about.push(why.clone());
    }
    about.push(if there {
        "Drag it onto the graph, or double-click, for a node that reads it; or onto a file field.".to_string()
    } else {
        "Not there: right-click to say where it is now.".to_string()
    });
    let mut card = column![].spacing(6);
    if let Some(p) = preview {
        card = card.push(image(p.image.clone()).width(128).height(128).content_fit(ContentFit::Contain));
    }
    card = card.push(text(about.join("\n")).size(style::SMALL));
    tooltip(entry, container(card).padding(6).max_width(320).style(style::tooltip), tooltip::Position::Right)
        .delay(std::time::Duration::from_millis(450))
        .into()
}
