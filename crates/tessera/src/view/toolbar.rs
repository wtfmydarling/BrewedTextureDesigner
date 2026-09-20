//! The bars around the panes: the menu bar at the top of the window, the status bar at
//! the bottom, and the header of the graph pane with the texture size.

use iced::widget::{button, column, container, image, pick_list, row, rule, space, text, tooltip};
use iced::{Alignment, Element, Length};
use tessera_core::Edit;

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
        app.graph().node(*id).is_some_and(|n| n.def().has_image() && !n.def().inputs.is_empty() && n.kind != tessera_core::NodeKind::OUTPUT)
    });
    vec![
        item(undo, "Ctrl+Z", app.doc.can_undo().then_some(Message::Undo)),
        item(redo, "Ctrl+Y", app.doc.can_redo().then_some(Message::Redo)),
        separator(),
        item("Copy", "Ctrl+C", sel(Message::Copy)),
        item("Cut", "Ctrl+X", sel(Message::Cut)),
        item("Paste", "Ctrl+V", Some(Message::Paste)),
        item("Duplicate", "Ctrl+D", sel(Message::Duplicate)),
        item("Delete", "Del", (has_selection || app.ui.selected_link.is_some()).then_some(Message::Delete)),
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
        separator(),
        item("Frame Selection / All", "F", Some(Message::Frame)),
        item("Reset Panel Layout", "", Some(Message::ResetLayout)),
    ]
}

fn help_menu(_app: &App) -> Vec<Element<'_, Message>> {
    vec![
        item("Keyboard Shortcuts", "F1", Some(Message::Dialog(Some(Dialog::Shortcuts)))),
        item(format!("About {}", tessera_core::APP_NAME), "", Some(Message::Dialog(Some(Dialog::About)))),
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

/// The status bar across the bottom of the window: the latest message, then what the
/// last evaluation did and the GPU.
pub fn status_bar(app: &App) -> Element<'_, Message> {
    let status = match &app.ui.status {
        Some(s) => text(s.text.clone()).size(style::SMALL).color(if s.error { style::ERROR } else { style::TEXT }),
        None => text("Ready").size(style::SMALL).color(style::TEXT_FAINT),
    };
    let r = &app.res.report;
    let stats = tip(
        text(format!("{} nodes  ·  {} recomputed  ·  {:.1} ms", app.graph().nodes.len(), r.recomputed, r.cpu_ms))
            .size(style::SMALL)
            .color(style::TEXT_DIM),
        "Only nodes whose inputs or parameters changed are recomputed on the GPU",
        tooltip::Position::Top,
    );
    let gpu = if app.engine.is_none() {
        "Starting the GPU…".to_string()
    } else if app.adapter.is_empty() {
        "GPU".to_string()
    } else {
        app.adapter.clone()
    };
    container(
        row![
            container(status.wrapping(text::Wrapping::None)).clip(true).width(Length::Fill),
            stats,
            container(space().width(1).height(12)).style(style::hairline(style::LINE)),
            text(gpu).size(style::SMALL).color(style::TEXT_FAINT),
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .height(Length::Fixed(STATUS_BAR_H)),
    )
    .padding([0, 10])
    .width(Length::Fill)
    .style(style::window_bar)
    .into()
}
