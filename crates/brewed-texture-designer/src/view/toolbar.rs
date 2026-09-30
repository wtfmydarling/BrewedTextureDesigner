//! The bars around the panes: the menu bar at the top of the window, the status bar at
//! the bottom, and the header of the graph pane with the texture size.

use std::time::Duration;

use iced::widget::{button, column, container, image, pick_list, row, rule, space, text, tooltip};
use iced::{Alignment, Element, Length};
use brewed_texture_core::{Edit, Precision};

use crate::app::{App, Dialog, ExplorerMessage, MenuId, Message, PreviewMessage, SettingsMessage};
use crate::icons::{Icon, icon};
use crate::shortcuts::Command;
use crate::style;
use crate::view::tip;
use crate::widget::dropdown::dropdown;

/// The texture sizes offered (a graph's, and a new graph's in the settings).
pub(crate) const RESOLUTIONS: [u32; 7] = [128, 256, 512, 1024, 2048, 4096, 8192];
const MENU_BAR_H: f32 = 30.0;
const STATUS_BAR_H: f32 = 24.0;

fn item<'a>(label: impl Into<String>, shortcut: impl Into<String>, message: Option<Message>) -> Element<'a, Message> {
    let enabled = message.is_some();
    let color = if enabled { style::TEXT } else { style::TEXT_FAINT };
    button(
        row![
            text(label.into()).size(style::TEXT_SIZE).color(color),
            space().width(Length::Fill),
            text(shortcut.into()).size(style::SMALL).color(if enabled { style::TEXT_DIM } else { style::TEXT_FAINT }),
        ]
        .spacing(16)
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

fn menu_body<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    container(column(items).spacing(1).width(Length::Fixed(250.0))).padding(4).style(style::popup).into()
}

fn menu<'a>(app: &'a App, id: MenuId, title: &'a str, items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let open = app.ui.menu == Some(id);
    let title_button = iced::widget::mouse_area(
        button(text(title).size(style::TEXT_SIZE)).padding([4, 10]).style(style::toggle_button(open)).on_press(Message::Menu(Some(id))),
    )
    .on_enter(Message::MenuHover(id));
    dropdown(title_button, menu_body(items), open).on_dismiss(Message::Menu(None)).into()
}

fn file_menu(app: &App) -> Vec<Element<'_, Message>> {
    let key = app.doc_key;
    let mut items = vec![
        item("Home Screen", "", Some(Message::Home(crate::app::home::HomeMessage::Show(crate::app::home::HomePage::Home)))),
        separator(),
        item("New Package…", app.key_of(Command::New), Some(Message::NewGraph(crate::app::home::NewGraphMessage::Open(None)))),
        item("New Graph…", "", Some(Message::NewGraph(crate::app::home::NewGraphMessage::Open(Some(key))))),
        item("Open Package…", app.key_of(Command::Open), Some(Message::Open)),
    ];
    // The packages opened or saved last (as many as the settings say): each by its name, and
    // the folder it is in.
    let offered = app.settings.recent_packages as usize;
    if !app.ui.recent.is_empty() && offered > 0 {
        items.push(separator());
        for path in app.ui.recent.iter().take(offered) {
            let p = std::path::Path::new(path);
            let name = p.file_name().map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
            let folder = p.parent().and_then(|f| f.file_name()).map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            items.push(recent_item(name, folder, Message::OpenRecent(path.clone())));
        }
        items.push(item("Clear Recent Packages", "", Some(Message::ClearRecent)));
    }
    items.extend([
        separator(),
        item("Link Files…", "", Some(Message::Explorer(ExplorerMessage::LinkFiles(key)))),
        item("Collect Files", "", Some(Message::Explorer(ExplorerMessage::CollectFiles(key)))),
        separator(),
        item("Save", app.key_of(Command::Save), Some(Message::Save)),
        item("Save As…", app.key_of(Command::SaveAs), Some(Message::SaveAs)),
        item("Close Package", "", Some(Message::Explorer(ExplorerMessage::Close(key)))),
        separator(),
        item("Export Outputs…", app.key_of(Command::Export), Some(Message::ExportAll)),
        separator(),
        item("Open the Demo", "", Some(Message::LoadDemo)),
        separator(),
        item("Quit", "", Some(Message::Quit)),
    ]);
    items
}

/// A package of the recent ones: its name, and on the right, the folder it is in (cut short
/// to fit the menu).
fn recent_item<'a>(name: String, folder: String, message: Message) -> Element<'a, Message> {
    let folder: String = if folder.chars().count() > 18 { folder.chars().take(17).chain(['…']).collect() } else { folder };
    button(
        row![
            text(name).size(style::TEXT_SIZE).wrapping(text::Wrapping::None),
            space().width(Length::Fill),
            text(folder).size(style::SMALL).color(style::TEXT_DIM).wrapping(text::Wrapping::None),
        ]
        .spacing(16)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([4, 10])
    .style(style::row_button(false))
    .on_press(Message::FromMenu(Box::new(message)))
    .into()
}

fn edit_menu(app: &App) -> Vec<Element<'_, Message>> {
    let has_selection = !app.ui.selection.is_empty();
    let sel = |m: Message| has_selection.then_some(m);
    let undo = app.doc.undo_label().map_or_else(|| "Undo".to_string(), |l| format!("Undo {l}"));
    let redo = app.doc.redo_label().map_or_else(|| "Redo".to_string(), |l| format!("Redo {l}"));
    let bypass = app.ui.selection.iter().next().copied().filter(|id| {
        app.graph().node(*id).is_some_and(|n| n.def().has_image() && !n.def().inputs.is_empty() && n.kind != brewed_texture_core::NodeKind::OUTPUT)
    });
    vec![
        item(undo, app.key_of(Command::Undo), app.doc.can_undo().then_some(Message::Undo)),
        item(redo, app.key_of(Command::Redo), app.doc.can_redo().then_some(Message::Redo)),
        separator(),
        item("Copy", app.key_of(Command::Copy), sel(Message::Copy)),
        item("Cut", app.key_of(Command::Cut), sel(Message::Cut)),
        item("Paste", app.key_of(Command::Paste), Some(Message::Paste)),
        item("Duplicate", app.key_of(Command::Duplicate), sel(Message::Duplicate)),
        item(
            "Delete",
            app.key_of(Command::Delete),
            (has_selection || app.ui.selected_link.is_some() || app.ui.selected_comment.is_some()).then_some(Message::Delete),
        ),
        separator(),
        item("Select All", app.key_of(Command::SelectAll), Some(Message::SelectAll)),
        item("Bypass Node", app.key_of(Command::Bypass), bypass.map(Message::ToggleBypass)),
        item(
            match app.ui.selection.len() {
                0 => "Create Graph from Selection".to_string(),
                n => format!("Create Graph from Selection ({n})"),
            },
            app.key_of(Command::GraphFromSelection),
            sel(Message::GraphFromSelection),
        ),
        item(
            if has_selection { "Frame Selection in a Comment" } else { "Add Comment" },
            app.key_of(Command::AddComment),
            Some(Message::AddComment),
        ),
        separator(),
        item("Settings…", app.key_of(Command::Settings), Some(Message::Settings(SettingsMessage::Open))),
    ]
}

fn view_menu(app: &App) -> Vec<Element<'_, Message>> {
    let p = &app.ui.preview;
    let check = |on: bool, label: &str| format!("{}  {label}", if on { "●" } else { "○" });
    vec![
        item(check(p.show_tiling, "Show Tiling in 2D View"), app.key_of(Command::ShowTiling), Some(Message::Preview(PreviewMessage::ToggleTiling))),
        item(check(app.ui.realtime, "Real-time Rendering"), "", Some(Message::ToggleRealtime)),
        separator(),
        item("Frame Selection / All", app.key_of(Command::Frame), Some(Message::Frame)),
        item("Reset Panel Layout", "", Some(Message::ResetLayout)),
    ]
}

fn help_menu(app: &App) -> Vec<Element<'_, Message>> {
    vec![
        item("Keyboard Shortcuts", app.key_of(Command::Help), Some(Message::Dialog(Some(Dialog::Shortcuts)))),
        item(format!("About {}", brewed_texture_core::APP_NAME), "", Some(Message::Dialog(Some(Dialog::About)))),
    ]
}

/// The menu bar across the top of the window: the menus and the document's name.
pub fn menu_bar(app: &App) -> Element<'_, Message> {
    let logo = image(crate::brand::logo()).width(Length::Fixed(16.0)).height(Length::Fixed(16.0));
    let dirty = if app.doc.is_dirty() { "  •" } else { "" };
    // (Not while the Home screen is shown: nothing of it is edited there.)
    let document = match app.ui.home {
        Some(_) => text(""),
        None => text(format!("{} — {}{dirty}", app.doc.title(), app.doc.entry().name)).size(style::SMALL).color(style::TEXT_DIM),
    };
    // The logo goes to the Home screen, as Adobe's does.
    let home = crate::app::home::HomeMessage::Show(crate::app::home::HomePage::Home);
    let logo = tip(button(logo).padding([4, 6]).style(style::flat_button).on_press(Message::Home(home)), "Home Screen", tooltip::Position::Bottom);
    container(
        row![
            logo,
            menu(app, MenuId::File, "File", file_menu(app)),
            menu(app, MenuId::Edit, "Edit", edit_menu(app)),
            menu(app, MenuId::View, "View", view_menu(app)),
            menu(app, MenuId::Help, "Help", help_menu(app)),
            space().width(Length::Fill),
            document,
        ]
        .spacing(2)
        .align_y(Alignment::Center)
        .height(Length::Fixed(MENU_BAR_H)),
    )
    .padding([0, 8])
    .width(Length::Fill)
    .style(style::window_bar)
    .into()
}

/// Header of the graph pane, with the size of the textures.
pub fn graph_header(app: &App) -> Element<'_, Message> {
    let [w, h] = app.graph().resolution;
    let lock = app.ui.lock_square;
    let size_list = |value: u32, on_select: Box<dyn Fn(u32) -> Message>| {
        pick_list(RESOLUTIONS, Some(value), on_select).text_size(style::SMALL).padding([2, 6]).style(style::pick_list)
    };
    let tools = row![
        text("Texture size").size(style::SMALL).color(style::TEXT_DIM),
        size_list(w, Box::new(move |nw| Message::Edit(Edit::SetResolution([nw, if lock { nw } else { h }])))),
        text("×").size(style::SMALL).color(style::TEXT_DIM),
        size_list(h, Box::new(move |nh| Message::Edit(Edit::SetResolution([if lock { nh } else { w }, nh])))),
        tip(
            button(icon(Icon::Link, 14.0, if lock { style::ACCENT_TEXT } else { style::TEXT_DIM }))
                .padding(4)
                .style(style::toggle_button(lock))
                .on_press(Message::LockSquare),
            if lock { "Width and height are kept equal (click to size them separately)" } else { "Keep width and height equal" },
            tooltip::Position::Bottom,
        ),
        tip(
            pick_list(Precision::ALL, Some(app.graph().precision), |p| Message::Edit(Edit::SetPrecision(p)))
                .text_size(style::SMALL)
                .padding([2, 6])
                .style(style::pick_list),
            "What the graph keeps its images in; a node can keep its own in another (its Precision). \
             8 bits: 256 steps from 0 to 1, a quarter of the memory of floats. 16 bits float: steps of 1/2048 \
             near 1 and finer below, values beyond 0 and 1 kept too; what a game needs. 32 bits float: smooth \
             everywhere (a gentle slope of a height stays smooth in a normal map), twice the memory.",
            tooltip::Position::Bottom,
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    crate::view::pane_header("Graph", tools)
}

/// How long a change takes before the status bar says it is being computed.
pub const COMPUTING_AFTER: Duration = Duration::from_millis(250);
/// How far back the status bar's readings go.
pub const READING_SPAN: Duration = Duration::from_secs(1);

/// A duration in milliseconds, as precise as is worth reading.
fn duration(millis: f32) -> String {
    match millis {
        m if m < 10.0 => format!("{m:.1} ms"),
        m if m < 1000.0 => format!("{m:.0} ms"),
        m => format!("{:.1} s", m / 1000.0),
    }
}

fn megabytes(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb < 1000.0 { format!("{mb:.0} MB") } else { format!("{:.1} GB", mb / 1024.0) }
}

/// `n` of `thing`, as English counts them.
fn count(n: usize, thing: &str) -> String {
    if n == 1 { format!("1 {thing}") } else { format!("{n} {thing}s") }
}

/// A reading of the status bar, as long as it has to be and no longer.
fn reading<'a>(content: String, hint: String) -> Element<'a, Message> {
    tip(
        text(content).size(style::SMALL).color(style::TEXT_DIM).wrapping(text::Wrapping::None),
        hint,
        tooltip::Position::Top,
    )
}

fn divider<'a>() -> Element<'a, Message> {
    container(space().width(1).height(12)).style(style::hairline(style::LINE)).into()
}

/// The status bar across the bottom of the window: the latest message (nothing when there is
/// none), then what is happening now, over the last second: the graph's nodes, how many of
/// them its last change computed again and how long its changes take; how much GPU memory
/// the images take; and the GPU. (How fast the 3D view draws, it says itself.)
pub fn status_bar(app: &App) -> Element<'_, Message> {
    let status: Element<'_, Message> = match &app.ui.status {
        Some(s) => text(s.text.clone())
            .size(style::SMALL)
            .color(if s.error { style::ERROR } else { style::TEXT })
            .wrapping(text::Wrapping::None)
            .into(),
        None => space().into(),
    };

    let nodes = count(app.graph().nodes.len(), "node");
    let busy = app.res.busy_since.map(|since| since.elapsed());
    let recent: Vec<(f32, usize)> =
        app.res.updates.iter().filter(|(at, ..)| at.elapsed() <= READING_SPAN).map(|(_, millis, n)| (*millis, *n)).collect();
    let graph = match busy {
        Some(for_) if app.res.slow || for_ >= COMPUTING_AFTER => {
            format!("{nodes} · computing… {}", duration(for_.as_secs_f32() * 1000.0))
        }
        _ => {
            let (recomputed, average) = match recent.last() {
                Some((_, recomputed)) => (*recomputed, recent.iter().map(|(millis, _)| millis).sum::<f32>() / recent.len() as f32),
                None => (0, 0.0),
            };
            format!("{nodes} · {recomputed} recomputed · {}", duration(average))
        }
    };
    let graph = reading(
        graph,
        "The nodes of the graph; how many of them the last change of the last second computed again \
         (only those it affects); and how long the changes of the last second took on average, from \
         the change until the GPU was done with the images, their thumbnails and the views."
            .into(),
    );

    // The engine is there once it started, or once it said anything.
    let engine = app.engine.is_some() || app.res.finished_revision > 0;
    let m = app.res.memory;
    let memory = reading(
        format!("VRAM {}", megabytes(m.total())),
        format!(
            "GPU memory the images take, at the texture size:\n\
             {} the 2D and 3D views show: {} with their mips (an output's in its channels and \
             at its bit depth)\n\
             {} kept for the next change (what the last one read)\n\
             {} scratch images, kept while changes keep coming (a slider dragged)\n\
             {} files and gradients\n\
             Half a second after the last change, only what the views show stays.",
            megabytes(m.shown),
            count(m.shown_images, "image"),
            megabytes(m.kept),
            megabytes(m.scratch),
            megabytes(m.files),
        ),
    );

    let gpu = if app.engine.is_none() {
        "Starting the GPU…".to_string()
    } else if app.adapter.is_empty() {
        "GPU".to_string()
    } else {
        app.adapter.clone()
    };
    // (It never should.)
    let gpu: Element<'_, Message> = match &app.res.stopped {
        Some(why) => tip(
            text("The engine stopped").size(style::SMALL).color(style::ERROR),
            format!("{why}\n\nNothing is computed any more: save the packages and start the program again."),
            tooltip::Position::Top,
        ),
        None => text(gpu).size(style::SMALL).color(style::TEXT_FAINT).into(),
    };
    let mut bar = row![container(status).clip(true).width(Length::Fill), graph]
        .spacing(8)
        .align_y(Alignment::Center)
        .height(Length::Fixed(STATUS_BAR_H));
    if engine {
        bar = bar.push(divider()).push(memory);
    }
    bar = bar.push(divider()).push(gpu);
    container(bar).padding([0, 10]).width(Length::Fill).style(style::window_bar).into()
}
