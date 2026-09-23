//! The bars around the panes: the menu bar at the top of the window, the status bar at
//! the bottom, and the header of the graph pane with the texture size.

use std::time::Duration;

use iced::widget::{button, column, container, image, pick_list, row, rule, space, text, tooltip};
use iced::{Alignment, Element, Length};
use brewed_texture_core::Edit;

use crate::app::{App, Dialog, MenuId, Message, PreviewMessage};
use crate::icons::{Icon, icon};
use crate::style;
use crate::view::tip;
use crate::widget::dropdown::dropdown;

const RESOLUTIONS: [u32; 7] = [128, 256, 512, 1024, 2048, 4096, 8192];
const MENU_BAR_H: f32 = 30.0;
const STATUS_BAR_H: f32 = 24.0;

fn item<'a>(label: impl Into<String>, shortcut: &'a str, message: Option<Message>) -> Element<'a, Message> {
    let enabled = message.is_some();
    let color = if enabled { style::TEXT } else { style::TEXT_FAINT };
    button(
        row![
            text(label.into()).size(style::TEXT_SIZE).color(color),
            space().width(Length::Fill),
            text(shortcut).size(style::SMALL).color(if enabled { style::TEXT_DIM } else { style::TEXT_FAINT }),
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

fn file_menu(_app: &App) -> Vec<Element<'_, Message>> {
    vec![
        item("New", "Ctrl+N", Some(Message::New)),
        item("Open…", "Ctrl+O", Some(Message::Open)),
        item("Save", "Ctrl+S", Some(Message::Save)),
        item("Save As…", "Ctrl+Shift+S", Some(Message::SaveAs)),
        separator(),
        item("Export Outputs…", "Ctrl+E", Some(Message::ExportAll)),
        separator(),
        item("Load Demo Graph", "", Some(Message::LoadDemo)),
        separator(),
        item("Quit", "", Some(Message::Quit)),
    ]
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
        item(undo, "Ctrl+Z", app.doc.can_undo().then_some(Message::Undo)),
        item(redo, "Ctrl+Y", app.doc.can_redo().then_some(Message::Redo)),
        separator(),
        item("Copy", "Ctrl+C", sel(Message::Copy)),
        item("Cut", "Ctrl+X", sel(Message::Cut)),
        item("Paste", "Ctrl+V", Some(Message::Paste)),
        item("Duplicate", "Ctrl+D", sel(Message::Duplicate)),
        item(
            "Delete",
            "Del",
            (has_selection || app.ui.selected_link.is_some() || app.ui.selected_comment.is_some()).then_some(Message::Delete),
        ),
        separator(),
        item("Select All", "Ctrl+A", Some(Message::SelectAll)),
        item("Bypass Node", "Ctrl+B", bypass.map(Message::ToggleBypass)),
        item(if has_selection { "Frame Selection in a Comment" } else { "Add Comment" }, "C", Some(Message::AddComment)),
    ]
}

fn view_menu(app: &App) -> Vec<Element<'_, Message>> {
    let p = &app.ui.preview;
    let check = |on: bool, label: &str| format!("{}  {label}", if on { "●" } else { "○" });
    vec![
        item(check(p.show_tiling, "Show Tiling in 2D View"), "", Some(Message::Preview(PreviewMessage::ToggleTiling))),
        item(check(app.ui.realtime, "Real-time Rendering"), "", Some(Message::ToggleRealtime)),
        separator(),
        item("Frame Selection / All", "F", Some(Message::Frame)),
        item("Reset Panel Layout", "", Some(Message::ResetLayout)),
    ]
}

fn help_menu(_app: &App) -> Vec<Element<'_, Message>> {
    vec![
        item("Keyboard Shortcuts", "F1", Some(Message::Dialog(Some(Dialog::Shortcuts)))),
        item(format!("About {}", brewed_texture_core::APP_NAME), "", Some(Message::Dialog(Some(Dialog::About)))),
    ]
}

/// The menu bar across the top of the window: the menus and the document's name.
pub fn menu_bar(app: &App) -> Element<'_, Message> {
    let logo = image(crate::brand::logo()).width(Length::Fixed(16.0)).height(Length::Fixed(16.0));
    let dirty = if app.doc.is_dirty() { "  •" } else { "" };
    let document = text(format!("{}{dirty}", app.doc.title())).size(style::SMALL).color(style::TEXT_DIM);
    container(
        row![
            container(logo).padding([0, 6]),
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
    let mut bar = row![container(status).clip(true).width(Length::Fill), graph]
        .spacing(8)
        .align_y(Alignment::Center)
        .height(Length::Fixed(STATUS_BAR_H));
    if engine {
        bar = bar.push(divider()).push(memory);
    }
    bar = bar.push(divider()).push(text(gpu).size(style::SMALL).color(style::TEXT_FAINT));
    container(bar).padding([0, 10]).width(Length::Fill).style(style::window_bar).into()
}
