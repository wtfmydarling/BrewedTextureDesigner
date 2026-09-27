//! Modal dialogs: unsaved changes, work kept by a program that stopped, files a package cannot
//! find, the settings, keyboard shortcuts, about.

use iced::widget::{button, center, column, container, mouse_area, opaque, pick_list, row, scrollable, space, text, toggler};
use iced::{Alignment, Element, Length};
use brewed_texture_core::Precision;
use brewed_texture_core::package::{MissingKind, file_name};

use crate::app::autosave::Recovery;
use crate::app::{App, Dialog, ExplorerMessage, Message, MissingFiles, PendingAction};
use crate::icons::{Icon, icon};
use crate::settings::{Graphics, NewFiles, Settings};
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
        #[cfg(any(test, feature = "dev-tools"))]
        PendingAction::New => "starting a new package",
        #[cfg(any(test, feature = "dev-tools"))]
        PendingAction::OpenPath(_) => "opening another package",
        #[cfg(any(test, feature = "dev-tools"))]
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
    ("Ctrl+,", "Settings"),
    ("Drop a file on the window", "Open a .brewtex package; an image, vector graphics or a font becomes the node that reads it"),
    ("Explorer", "Double-click a graph to edit it; drag a graph onto another to use it as a node"),
    ("Ctrl+G", "Create a graph of the selected nodes, and use it in their place"),
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
        Dialog::Recover => recover(app),
        Dialog::Settings => settings(app),
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
                text("A node based procedural texture editor. Every image node runs as a shader on the GPU; \
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

/// A scale of the interface, as people say it: "125%".
#[derive(Clone, Copy, Debug, PartialEq)]
struct Percent(f32);

impl std::fmt::Display for Percent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}%", (self.0 * 100.0).round())
    }
}

/// How often unsaved work is kept, as people say it: "30 seconds", "2 minutes".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Every(u32);

impl std::fmt::Display for Every {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            60 => f.write_str("a minute"),
            s if s % 60 == 0 => write!(f, "{} minutes", s / 60),
            s => write!(f, "{s} seconds"),
        }
    }
}

/// `choices`, and `current` among them in its place if it is not one of them (a value the
/// settings file says).
fn with<T: Copy + PartialEq>(choices: &[T], current: T, order: impl Fn(&T) -> f32) -> Vec<T> {
    let mut all = choices.to_vec();
    if !all.contains(&current) {
        all.push(current);
        all.sort_by(|a, b| order(a).total_cmp(&order(b)));
    }
    all
}

/// The settings (Edit → Settings…): each taken as it is chosen, and kept.
fn settings(app: &App) -> Element<'_, Message> {
    let s = app.settings;
    let label = |t: &'static str| text(t).size(style::TEXT_SIZE).width(Length::Fixed(190.0));
    let note = |t: String| text(t).size(style::SMALL).color(style::TEXT_FAINT);
    let heading = |t: &'static str| text(t).size(style::TEXT_SIZE).font(style::bold());
    let line = |name: &'static str, control: Element<'static, Message>| {
        row![label(name), control].spacing(10).align_y(Alignment::Center)
    };

    let scales = with(&[0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0].map(Percent), Percent(s.scale), |p| p.0);
    let interface = column![
        heading("Interface"),
        line("Scale", choices(scales, Percent(s.scale), move |p: Percent| Message::Settings(Settings { scale: p.0, ..s }))),
    ]
    .spacing(8);

    let file_choice = |label: &'static str, which: NewFiles| {
        button(text(label).size(style::TEXT_SIZE))
            .padding([4, 10])
            .style(style::toggle_button(s.new_files == which))
            .on_press(Message::Settings(Settings { new_files: which, ..s }))
    };
    let files = column![
        heading("Files"),
        line(
            "A file chosen or dropped",
            row![file_choice("Embed in the package", NewFiles::Embed), file_choice("Link to it", NewFiles::Link)].spacing(4).into()
        ),
        note(match s.new_files {
            NewFiles::Embed => "The package holds it: the package is one file to copy or send, and needs nothing beside it.".into(),
            NewFiles::Link => "The package names it where it is: it is read from there, and goes with the package only if copied along.".into(),
        }),
    ]
    .spacing(8);

    let sizes = with(&[128, 256, 512, 1024, 2048, 4096, 8192], s.graph_size, |n| *n as f32);
    let new_graphs = column![
        heading("New Graphs"),
        line("Size", choices(sizes, s.graph_size, move |n: u32| Message::Settings(Settings { graph_size: n, ..s }))),
        line(
            "Precision",
            choices(Precision::ALL.to_vec(), s.graph_precision, move |p: Precision| Message::Settings(Settings { graph_precision: p, ..s }))
        ),
    ]
    .spacing(8);

    let intervals = with(&[10, 30, 60, 120, 300, 600].map(Every), Every(s.keep_every), |e| e.0 as f32);
    let recovery = column![
        heading("Recovery"),
        line(
            "Keep unsaved work",
            toggler(s.keep_unsaved).size(18.0).on_toggle(move |on| Message::Settings(Settings { keep_unsaved: on, ..s })).into()
        ),
        line("Every", choices(intervals, Every(s.keep_every), move |e: Every| Message::Settings(Settings { keep_every: e.0, ..s }))),
        note(if s.keep_unsaved {
            "What packages have not saved is kept beside the settings, and offered back if the program stops without quitting.".into()
        } else {
            "Nothing is kept: what was not saved is gone if the program stops without quitting.".into()
        }),
    ]
    .spacing(8);

    let apis = with(Graphics::here(), s.graphics, |g| Graphics::here().iter().position(|h| h == g).unwrap_or(usize::MAX) as f32);
    let graphics_note = match std::env::var("WGPU_BACKEND").ok().filter(|v| !v.is_empty() && s.graphics.id() != v) {
        Some(env) if s.graphics == app.graphics_at_start() => format!("The environment says WGPU_BACKEND={env}: that comes first."),
        _ if s.graphics != app.graphics_at_start() => "Taken when the program starts again.".to_string(),
        _ => "Taken when the program starts.".to_string(),
    };
    let graphics = column![
        heading("Graphics"),
        line("API", choices(apis, s.graphics, move |g: Graphics| Message::Settings(Settings { graphics: g, ..s }))),
        note(graphics_note),
    ]
    .spacing(8);

    let (file, problems) = app.settings_file();
    let mut body = column![
        row![
            text("Settings").size(18).font(style::bold()),
            space().width(Length::Fill),
            button(text("Close").size(style::TEXT_SIZE)).padding([4, 12]).style(style::button).on_press(Message::Dialog(None)),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(18);
    for problem in problems {
        body = body.push(container(text(problem.clone()).size(style::SMALL)).padding(6).width(Length::Fill).style(style::message_box(style::WARNING)));
    }
    body = body.push(interface).push(files).push(new_graphs).push(recovery).push(graphics);
    body = body.push(note(match file {
        Some(file) => format!("Kept in {}", file.display()),
        None => "Not kept: the program runs with the settings as they are by default.".into(),
    }));
    modal(dialog_box(scrollable(body).height(Length::Shrink).style(style::scrollable), 560.0), Some(Message::Dialog(None)))
}

/// A list to choose one of `options` from, `selected` chosen.
fn choices<'a, T>(options: Vec<T>, selected: T, on_select: impl Fn(T) -> Message + 'a) -> Element<'a, Message>
where
    T: ToString + PartialEq + Clone + 'a,
{
    pick_list(options, Some(selected), on_select).text_size(style::TEXT_SIZE).padding([3, 8]).style(style::pick_list).into()
}

/// How long `seconds` is, in words: "3 minutes".
fn span(seconds: u64) -> String {
    let (n, unit) = match seconds {
        0..120 => return "a minute or less".into(),
        120..7200 => (seconds / 60, "minutes"),
        7200..172_800 => (seconds / 3600, "hours"),
        _ => (seconds / 86_400, "days"),
    };
    format!("{n} {unit}")
}

/// What programs that stopped without quitting kept: each package, where it was saved and
/// when it was kept, to be recovered, kept for later or discarded.
fn recover(app: &App) -> Element<'_, Message> {
    let now = brewed_texture_core::recovery::now();
    let mut list = column![].spacing(10);
    for snapshot in app.recoverable() {
        let unsaved = &snapshot.unsaved;
        let place = unsaved.path.clone().unwrap_or_else(|| "Never saved".into());
        let mut when = format!("Kept {} ago", span(now.saturating_sub(unsaved.time)));
        if let Some(saved) = snapshot.saved.filter(|s| *s > unsaved.time) {
            when.push_str(&format!("; its file was saved {} after that: it opens untitled", span(saved - unsaved.time)));
        }
        list = list.push(
            row![
                icon(Icon::Package, 16.0, style::TEXT_DIM),
                column![
                    text(&unsaved.title).size(style::TEXT_SIZE).font(style::bold()),
                    text(place).size(style::SMALL).color(style::TEXT_FAINT),
                    text(when).size(style::SMALL).color(style::TEXT_DIM),
                ]
                .spacing(2)
                .width(Length::Fill),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
    }
    let button_of = |label: &'static str, style: fn(&iced::Theme, button::Status) -> button::Style, choice: Recovery| {
        button(text(label).size(style::TEXT_SIZE)).padding([6, 14]).style(style).on_press(Message::Recover(choice))
    };
    let body = column![
        text("Recover unsaved work?").size(18).font(style::bold()),
        text(format!(
            "{} stopped without quitting last time. What these packages had not saved yet was kept:",
            brewed_texture_core::APP_NAME
        ))
        .size(style::TEXT_SIZE)
        .color(style::TEXT_DIM),
        scrollable(list).height(Length::Shrink).style(style::scrollable),
        text(
            "Recovered, each opens with the changes it had, not saved yet: saving saves it where it was. \
             Later, they are offered again the next time the program starts."
        )
        .size(style::SMALL)
        .color(style::TEXT_FAINT),
        row![
            button_of("Discard", style::button, Recovery::Discard),
            space().width(Length::Fill),
            button_of("Later", style::flat_button, Recovery::Later),
            button_of("Recover", style::primary_button, Recovery::Recover),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(12);
    modal(dialog_box(body, 560.0), None)
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
