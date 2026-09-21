//! The bars around the panes: the menu bar at the top of the window, the status bar at
//! the bottom, and the header of the graph pane with the texture size.

use std::time::Duration;

use iced::widget::{button, column, container, image, pick_list, row, rule, space, stack, text, tooltip};
use iced::{Alignment, Element, Length};
use brewed_texture_core::Edit;

use crate::app::{App, Dialog, MenuId, Message, PreviewMessage};
use crate::icons::{Icon, icon};
use crate::style;
use crate::view::tip;
use crate::widget::dropdown::dropdown;
use crate::widget::frame_clock::frame_clock;

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

/// A reading of the status bar: as wide as it can get, so that its neighbours stay put
/// while its digits change.
fn reading<'a>(content: String, width: f32, hint: String) -> Element<'a, Message> {
    tip(
        container(text(content).size(style::SMALL).color(style::TEXT_DIM).wrapping(text::Wrapping::None))
            .width(Length::Fixed(width))
            .align_x(iced::alignment::Horizontal::Right),
        hint,
        tooltip::Position::Top,
    )
}

/// The status bar across the bottom of the window: the latest message (nothing when there
/// is none), then what is happening right now: how fast the window draws, how long the
/// graph takes to update and how much GPU memory its images take, and the GPU.
pub fn status_bar(app: &App) -> Element<'_, Message> {
    let status: Element<'_, Message> = match &app.ui.status {
        Some(s) => text(s.text.clone())
            .size(style::SMALL)
            .color(if s.error { style::ERROR } else { style::TEXT })
            .wrapping(text::Wrapping::None)
            .into(),
        None => space().into(),
    };

    let rate = app.ui.frame_rate;
    let frames = if app.ui.realtime {
        reading(
            format!("{:.0} FPS · {}", rate.fps, duration(rate.frame_ms)),
            120.0,
            format!(
                "Frames the window draws a second, and how long a frame takes on average; \
                 the longest one in the last second took {}. View → Real-time Rendering stops \
                 drawing when nothing changes.",
                duration(rate.worst_ms)
            ),
        )
    } else {
        reading(
            "On demand".into(),
            120.0,
            "The window draws only when something changes. View → Real-time Rendering draws frame \
             after frame, and counts them."
                .into(),
        )
    };

    // The engine is there once it started, or once it said anything.
    let engine = app.engine.is_some() || app.res.finished_revision > 0;
    let busy = app.res.busy_since.map(|since| since.elapsed());
    let recent: Vec<f32> =
        app.res.updates.iter().filter(|(at, _)| at.elapsed() <= READING_SPAN).map(|(_, millis)| *millis).collect();
    let graph = match busy {
        Some(for_) if app.res.slow || for_ >= COMPUTING_AFTER => {
            format!("Graph computing… {}", duration(for_.as_secs_f32() * 1000.0))
        }
        _ if !recent.is_empty() => format!("Graph {}", duration(recent.iter().sum::<f32>() / recent.len() as f32)),
        _ if !engine => String::new(),
        _ => "Graph idle".into(),
    };
    let graph = reading(
        graph,
        150.0,
        "How long the graph takes to update, averaged over the last second: from a change until \
         the GPU is done with its images, their thumbnails and the views. Only the nodes a change \
         affects are computed again. Idle while nothing changes."
            .into(),
    );
    let memory = reading(
        if engine { format!("VRAM {}", megabytes(app.res.memory)) } else { String::new() },
        100.0,
        "GPU memory the images take: every node's image at the texture size, the copies the 2D \
         and 3D views show, and the scratch images kept for reuse."
            .into(),
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
            container(status).clip(true).width(Length::Fill),
            // The clock takes no room: it only keeps the window drawing and counts frames.
            stack![frames, frame_clock(app.ui.realtime, Message::FrameRate)],
            graph,
            memory,
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
