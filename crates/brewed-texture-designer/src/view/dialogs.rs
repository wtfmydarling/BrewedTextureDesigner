//! Modal dialogs: unsaved changes, files a package cannot find, keyboard shortcuts, about.

use iced::widget::{button, center, column, container, mouse_area, opaque, row, scrollable, space, text};
use iced::{Alignment, Element, Length};
use brewed_texture_core::package::{MissingKind, file_name};

use crate::app::{App, Dialog, ExplorerMessage, Message, MissingFiles, PendingAction};
use crate::icons::{Icon, icon};
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
        PendingAction::New => "starting a new package",
        PendingAction::OpenPath(_) => "opening another package",
        PendingAction::LoadDemo => "loading the demo",
        PendingAction::Close(_) => "closing it",
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
    ("Space over the 2D view", "Show the image repeated around itself"),
    ("C", "Comment: a frame around the selected nodes, moved with them"),
    ("Ctrl+Z / Ctrl+Y", "Undo / redo"),
    ("Ctrl+S / Ctrl+Shift+S", "Save / save as"),
    ("Ctrl+O / Ctrl+N", "Open / new"),
    ("Ctrl+E", "Export all outputs"),
    ("Drop a file on the window", "Open a .brewtex package, add an image as a Bitmap node, link vector graphics or a font"),
    ("Explorer", "Double-click a graph to edit it; drag a graph onto another to use it as a node"),
    ("Ctrl+G", "Make the selected nodes a graph of their own, used in their place"),
    ("Eyedropper", "Click it, then click anywhere on the screen; for a gradient, drag across the colors"),
    ("Esc", "Close menus, cancel picking from the screen or a rename"),
    ("Number fields", "Drag to change (Shift fine, Ctrl snaps), click to type; expressions like 2*pi work"),
    ("Transform 2D gizmo", "Center moves, ring rotates, handles scale; Shift snaps / keeps proportions"),
    ("3D view", "Left drag orbits, middle drag moves the camera, right drag turns the light"),
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
        Dialog::Missing(missing) => missing_files(app, missing),
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
            let title = row![
                iced::widget::image(crate::brand::logo_large()).width(Length::Fixed(48.0)).height(Length::Fixed(48.0)),
                column![
                    text(brewed_texture_core::APP_NAME).size(24).font(style::bold()),
                    text(format!("Version {}", env!("CARGO_PKG_VERSION"))).size(style::TEXT_SIZE).color(style::TEXT_DIM),
                ]
                .spacing(2),
            ]
            .spacing(14)
            .align_y(Alignment::Center);
            let body = column![
                title,
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

/// The files a package names that are not there, each with a button to say where it is now.
fn missing_files<'a>(app: &'a App, missing: &'a MissingFiles) -> Element<'a, Message> {
    let title = app.package_doc(missing.key).map_or_else(|| "The package".to_string(), |d| format!("\"{}\"", d.title()));
    let mut list = column![].spacing(8);
    for item in &missing.items {
        let (which, what) = match item.what {
            MissingKind::Package => (Icon::Package, format!("Package of the graph(s) {}", item.graphs.join(", "))),
            MissingKind::Resource(kind) => (
                match kind {
                    brewed_texture_core::ResourceKind::Bitmap => Icon::Image,
                    brewed_texture_core::ResourceKind::Svg => Icon::Vector,
                    brewed_texture_core::ResourceKind::Font => Icon::Font,
                },
                kind.name().to_string(),
            ),
        };
        list = list.push(
            row![
                icon(which, 16.0, style::TEXT_DIM),
                column![
                    text(file_name(&item.path)).size(style::TEXT_SIZE).font(style::bold()),
                    text(&item.path).size(style::SMALL).color(style::TEXT_FAINT),
                    text(what).size(style::SMALL).color(style::TEXT_DIM),
                ]
                .spacing(2)
                .width(Length::Fill),
                button(text("Locate…").size(style::TEXT_SIZE))
                    .padding([5, 12])
                    .style(style::button)
                    .on_press(Message::Explorer(ExplorerMessage::Locate(missing.key, item.path.clone()))),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
    }
    let mut body = column![
        text("Missing files").size(18).font(style::bold()),
        text(format!(
            "{title} uses files that are not where it says. Say where each one is now: the package says so from \
             then on, and the others that were in the same folder are looked for next to it."
        ))
        .size(style::TEXT_SIZE)
        .color(style::TEXT_DIM),
    ]
    .spacing(12);
    if let Some(note) = &missing.note {
        body = body.push(container(text(note).size(style::SMALL)).padding(6).width(Length::Fill).style(style::message_box(style::WARNING)));
    }
    body = body.push(scrollable(list).height(Length::Shrink).style(style::scrollable));
    body = body.push(
        row![
            text("Left missing, they show nothing.").size(style::SMALL).color(style::TEXT_FAINT),
            space().width(Length::Fill),
            button(text("Leave Missing").size(style::TEXT_SIZE)).padding([6, 14]).style(style::button).on_press(Message::Dialog(None)),
        ]
        .align_y(Alignment::Center),
    );
    modal(dialog_box(body, 580.0), None)
}
