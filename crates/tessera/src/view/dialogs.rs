//! Modal dialogs: unsaved changes, keyboard shortcuts, about.

use iced::widget::{button, center, column, container, mouse_area, opaque, row, space, text};
use iced::{Alignment, Element, Length};

use crate::app::{App, Dialog, Message, PendingAction};
use crate::style;

fn dialog_box<'a>(content: impl Into<Element<'a, Message>>, width: f32) -> Element<'a, Message> {
    container(content).padding(18).width(Length::Fixed(width)).style(style::popup).into()
}

fn modal<'a>(content: Element<'a, Message>, dismiss: Option<Message>) -> Element<'a, Message> {
    let scrim = mouse_area(container(center(opaque(content))).width(Length::Fill).height(Length::Fill).style(style::scrim));
    match dismiss {
        Some(m) => opaque(scrim.on_press(m)),
        None => opaque(scrim),
    }
}

fn action_text(action: &PendingAction) -> &'static str {
    match action {
        PendingAction::New => "starting a new graph",
        PendingAction::Open | PendingAction::OpenPath(_) => "opening another graph",
        PendingAction::LoadDemo => "loading the demo graph",
        PendingAction::Quit => "quitting",
    }
}

const SHORTCUTS: &[(&str, &str)] = &[
    ("Drag from the library", "Add a node (double-click adds it in the middle)"),
    ("Space / double-click the canvas", "Search and add a node"),
    ("Drag from a socket", "Connect; drop on empty space to add a connected node"),
    ("Drag a connected input", "Re-route or remove the link"),
    ("Double-click a node", "Show it in the 2D view"),
    ("Right-click", "Context menu (right-drag pans)"),
    ("Mouse wheel", "Zoom (graph, 2D and 3D view)"),
    ("Middle drag, Alt + drag", "Pan"),
    ("Drag on empty canvas", "Box select (Shift or Ctrl adds)"),
    ("Del / Backspace", "Delete the selection"),
    ("Ctrl+C / Ctrl+X / Ctrl+V", "Copy / cut / paste nodes"),
    ("Ctrl+D", "Duplicate"),
    ("Ctrl+B", "Bypass the selected node"),
    ("Ctrl+A", "Select all"),
    ("F", "Frame the selection or everything"),
    ("Ctrl+Z / Ctrl+Y", "Undo / redo"),
    ("Ctrl+S / Ctrl+Shift+S", "Save / save as"),
    ("Ctrl+O / Ctrl+N", "Open / new"),
    ("Ctrl+E", "Export all outputs"),
    ("Drop a file on the window", "Open a .tessera graph, or add an image as a Bitmap node"),
    ("Esc", "Close menus, cancel a gradient pick or a rename"),
    ("Number fields", "Drag to change (Shift fine, Ctrl snaps), click to type; expressions like 2*pi work"),
    ("Transform 2D gizmo", "Center moves, ring rotates, handles scale; Shift snaps / keeps proportions"),
    ("3D view", "Left drag orbits, right drag moves the light"),
];

pub fn view<'a>(app: &'a App, dialog: &'a Dialog) -> Element<'a, Message> {
    match dialog {
        Dialog::Unsaved(action) => {
            let body = column![
                text("Save changes?").size(18).font(style::bold()),
                text(format!(
                    "\"{}\" has unsaved changes. Do you want to save them before {}?",
                    app.doc.title(),
                    action_text(action)
                ))
                .size(style::TEXT_SIZE)
                .color(style::TEXT_DIM),
                row![
                    space().width(Length::Fill),
                    button(text("Cancel").size(style::TEXT_SIZE)).padding([6, 14]).style(style::flat_button).on_press(Message::Dialog(None)),
                    button(text("Don't Save").size(style::TEXT_SIZE))
                        .padding([6, 14])
                        .style(style::button)
                        .on_press(Message::Proceed(action.clone())),
                    button(text("Save").size(style::TEXT_SIZE))
                        .padding([6, 18])
                        .style(style::primary_button)
                        .on_press(Message::SaveThen(action.clone())),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(14);
            modal(dialog_box(body, 440.0), None)
        }
        Dialog::Shortcuts => {
            let mut table = column![].spacing(4);
            for (keys, what) in SHORTCUTS {
                table = table.push(
                    row![
                        text(*keys).size(style::TEXT_SIZE).font(style::bold()).width(Length::Fixed(230.0)),
                        text(*what).size(style::TEXT_SIZE).color(style::TEXT_DIM),
                    ]
                    .spacing(12),
                );
            }
            let body = column![
                row![
                    text("Keyboard & mouse").size(18).font(style::bold()),
                    space().width(Length::Fill),
                    button(text("Close").size(style::TEXT_SIZE)).padding([4, 12]).style(style::button).on_press(Message::Dialog(None)),
                ]
                .align_y(Alignment::Center),
                table,
            ]
            .spacing(14);
            modal(dialog_box(body, 720.0), Some(Message::Dialog(None)))
        }
        Dialog::About => {
            let gpu = if app.adapter.is_empty() { "starting…".to_string() } else { app.adapter.clone() };
            let body = column![
                text(tessera_core::APP_NAME).size(24).font(style::bold()),
                text(format!("Version {}", env!("CARGO_PKG_VERSION"))).size(style::TEXT_SIZE).color(style::TEXT_DIM),
                text("A node based procedural texture editor. Every image node runs as a compute shader on the GPU; \
                      CPU side pixel work (export, histograms, file decoding) uses SIMD on all cores.")
                .size(style::TEXT_SIZE),
                text(format!("GPU: {gpu}")).size(style::TEXT_SIZE),
                text(format!("CPU threads: {}", std::thread::available_parallelism().map_or(1, |n| n.get())))
                    .size(style::TEXT_SIZE),
                text("Rust · wgpu · iced").size(style::SMALL).color(style::TEXT_DIM),
                row![
                    space().width(Length::Fill),
                    button(text("Close").size(style::TEXT_SIZE)).padding([5, 14]).style(style::button).on_press(Message::Dialog(None))
                ],
            ]
            .spacing(10);
            modal(dialog_box(body, 440.0), Some(Message::Dialog(None)))
        }
    }
}
