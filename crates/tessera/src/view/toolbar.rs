//! The bar above the graph: menus, texture size, evaluation statistics and status.

use iced::widget::{Space, button, column, container, pick_list, row, rule, space, text};
use iced::{Alignment, Element, Length};
use tessera_core::Edit;

use crate::app::{App, Dialog, MenuId, Message, PreviewMessage, ViewMode};
use crate::style;
use crate::widget::dropdown::dropdown;

const RESOLUTIONS: [u32; 7] = [128, 256, 512, 1024, 2048, 4096, 8192];

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
        color: style::BORDER_LIGHT,
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
    let title_button = button(text(title).size(style::TEXT_SIZE))
        .padding([3, 10])
        .style(style::toggle_button(open))
        .on_press(Message::Menu(Some(id)));
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
    ]
}

fn view_menu(app: &App) -> Vec<Element<'_, Message>> {
    let p = &app.ui.preview;
    let check = |on: bool, label: &str| format!("{}  {label}", if on { "●" } else { "○" });
    vec![
        item(check(p.mode == ViewMode::TwoD, "2D View"), "", Some(Message::Preview(PreviewMessage::Mode(ViewMode::TwoD)))),
        item(check(p.mode == ViewMode::ThreeD, "3D View"), "", Some(Message::Preview(PreviewMessage::Mode(ViewMode::ThreeD)))),
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

pub fn view(app: &App) -> Element<'_, Message> {
    let menus = row![
        menu(app, MenuId::File, "File", file_menu(app)),
        menu(app, MenuId::Edit, "Edit", edit_menu(app)),
        menu(app, MenuId::View, "View", view_menu(app)),
        menu(app, MenuId::Help, "Help", help_menu(app)),
    ]
    .spacing(2);

    let [w, h] = app.graph().resolution;
    let lock = app.ui.lock_square;
    let size = row![
        text("Size").size(style::TEXT_SIZE).color(style::TEXT_DIM),
        pick_list(RESOLUTIONS, Some(w), move |nw| Message::Edit(Edit::SetResolution([nw, if lock { nw } else { h }])))
            .text_size(style::TEXT_SIZE)
            .padding([2, 6])
            .style(style::pick_list),
        text("×").size(style::TEXT_SIZE).color(style::TEXT_DIM),
        pick_list(RESOLUTIONS, Some(h), move |nh| Message::Edit(Edit::SetResolution([if lock { nh } else { w }, nh])))
            .text_size(style::TEXT_SIZE)
            .padding([2, 6])
            .style(style::pick_list),
        crate::view::tip(
            button(text("Square").size(style::SMALL)).padding([3, 8]).style(style::toggle_button(lock)).on_press(Message::LockSquare),
            "Keep width and height equal",
            iced::widget::tooltip::Position::Bottom,
        ),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let r = &app.res.report;
    let gpu_state = if app.engine.is_none() { " · starting GPU…".to_string() } else { String::new() };
    let stats = text(format!(
        "{} nodes · {} recomputed · {:.1} ms{gpu_state}",
        app.graph().nodes.len(),
        r.recomputed,
        r.cpu_ms
    ))
    .size(style::SMALL)
    .color(style::TEXT_FAINT);
    let stats = crate::view::tip(
        stats,
        "Only nodes whose inputs or parameters changed are recomputed on the GPU",
        iced::widget::tooltip::Position::Bottom,
    );

    let status: Element<'_, Message> = match &app.ui.status {
        Some(s) => text(s.text.clone())
            .size(style::SMALL)
            .color(if s.error { style::ERROR } else { style::SUCCESS })
            .wrapping(text::Wrapping::None)
            .into(),
        None => Space::new().into(),
    };

    container(
        row![
            menus,
            rule::vertical(1),
            size,
            rule::vertical(1),
            stats,
            space().width(Length::Fill),
            container(status).clip(true).max_width(620),
        ]
        .spacing(10)
        .height(Length::Fixed(28.0))
        .align_y(Alignment::Center),
    )
    .padding([3, 6])
    .width(Length::Fill)
    .style(style::header_bar)
    .into()
}
