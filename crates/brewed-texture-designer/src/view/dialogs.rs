//! Modal dialogs: unsaved changes, work kept by a program that stopped, files a package cannot
//! find, the settings (see [`crate::view::settings`]), keyboard and mouse, about.

use iced::widget::{button, center, column, container, mouse_area, opaque, row, scrollable, space, text};
use iced::{Alignment, Element, Length};
use brewed_texture_core::package::{MissingKind, file_name};

use crate::app::autosave::Recovery;
use crate::app::{App, Dialog, ExplorerMessage, Message, MissingFiles, PendingAction, SettingsMessage};
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

fn action_text(app: &App, action: &PendingAction) -> &'static str {
    match action {
        #[cfg(any(test, feature = "dev-tools"))]
        PendingAction::New => "starting a new package",
        #[cfg(any(test, feature = "dev-tools"))]
        PendingAction::OpenPath(_) => "opening another package",
        #[cfg(any(test, feature = "dev-tools"))]
        PendingAction::LoadDemo => "loading the demo",
        PendingAction::Close(_) => "closing it",
        PendingAction::Quit if app.ui.restarting => "restarting",
        PendingAction::Quit => "quitting",
    }
}

/// What the mouse does (the keys of the commands are the settings': see
/// [`crate::shortcuts`]).
const MOUSE: &[(&str, &str)] = &[
    ("Drag from the library", "Add a node (double-click adds it in the middle)"),
    ("Double-click the canvas", "Search and add a node"),
    ("Drag from a socket", "Connect; drop on empty space to add a connected node"),
    ("Drag a connected input", "Re-route or remove the link"),
    ("Double-click a node", "Show it in the 2D view"),
    ("Right-click", "Context menu (right-drag pans)"),
    ("Mouse wheel", "Zoom (graph, 2D and 3D view)"),
    ("Middle drag, Alt + drag", "Pan"),
    ("Drag on empty canvas", "Box select (Shift or Ctrl adds)"),
    ("Drop a file on the window", "Open a .brewtex package; an image, vector graphics or a font becomes the node that reads it"),
    ("Explorer", "Double-click a graph to edit it; drag a graph onto another to use it as a node"),
    ("Eyedropper", "Click it, then click anywhere on the screen; for a gradient, drag across the colors"),
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
                    action_text(app, action)
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
        Dialog::Settings(settings) => crate::view::settings::view(app, settings),
        Dialog::NewGraph(dialog) => new_graph(app, dialog),
        Dialog::Export(dialog) => export(app, dialog),
        Dialog::Versions(dialog) => versions(app, dialog),
        Dialog::Shortcuts => {
            let line = |keys: String, what: String| {
                row![
                    text(keys).size(style::TEXT_SIZE).font(style::bold()).width(Length::Fixed(230.0)),
                    text(what).size(style::TEXT_SIZE).color(style::TEXT_DIM),
                ]
                .spacing(12)
            };
            let mut keyboard = column![crate::view::section("Keyboard")].spacing(4);
            for (_, commands) in crate::view::settings::GROUPS {
                for command in *commands {
                    let keys: Vec<String> = app.settings.shortcuts.of(*command).iter().map(|c| c.shown()).collect();
                    let keys = if keys.is_empty() { "—".to_string() } else { keys.join(" / ") };
                    keyboard = keyboard.push(line(keys, command.label().to_string()));
                }
            }
            keyboard = keyboard.push(line("Esc".into(), "Close menus and dialogs, cancel picking from the screen or a rename".into()));
            let mut mouse = column![crate::view::section("Mouse")].spacing(4);
            for (what, does) in MOUSE {
                mouse = mouse.push(line(what.to_string(), does.to_string()));
            }
            let body = column![
                row![
                    text("Keyboard & mouse").size(18).font(style::bold()),
                    space().width(Length::Fill),
                    button(text("Change the Keys…").size(style::TEXT_SIZE))
                        .padding([4, 12])
                        .style(style::button)
                        .on_press(Message::Settings(SettingsMessage::OpenPage(crate::settings::Page::Shortcuts))),
                    button(text("Close").size(style::TEXT_SIZE)).padding([4, 12]).style(style::button).on_press(Message::Dialog(None)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
                scrollable(column![keyboard, mouse].spacing(6).padding(iced::Padding { right: 12.0, ..iced::Padding::ZERO }))
                    .height(Length::Shrink)
                    .style(style::scrollable),
            ]
            .spacing(14);
            let dialog = container(body).padding(18).width(Length::Fixed(760.0)).max_height(640).style(style::popup);
            modal(dialog.into(), Some(Message::Dialog(None)))
        }
        Dialog::About => {
            let gpu = if app.adapter.is_empty() { "detecting…".to_string() } else { app.adapter.clone() };
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

/// The dialog of a new package, or of a new graph of one: its name, its template, the size and
/// precision of its images.
fn new_graph<'a>(app: &'a App, dialog: &'a crate::app::home::NewGraphDialog) -> Element<'a, Message> {
    use brewed_texture_core::Precision;
    use brewed_texture_core::templates::Template;
    use crate::app::home::NewGraphMessage as N;
    let say = Message::NewGraph;
    let into = dialog.into.and_then(|key| app.package_doc(key)).map(|d| d.title());
    let title = match &into {
        Some(package) => format!("New Graph in {package}"),
        None => "New Package".to_string(),
    };
    let label = |t: &'static str| text(t).size(style::TEXT_SIZE).color(style::TEXT_DIM).width(Length::Fixed(110.0));
    let name = iced::widget::text_input("Graph", &dialog.name)
        .id(new_graph_name_id())
        .on_input(|n| Message::NewGraph(N::Name(n)))
        .on_submit(say(N::Create))
        .size(style::TEXT_SIZE)
        .padding([5, 8])
        .style(style::text_input);
    let mut templates = column![].spacing(4);
    for template in Template::ALL {
        let chosen = template == dialog.template;
        let outputs = template.outputs();
        let mut about = column![text(template.name()).size(style::TEXT_SIZE).font(style::bold()), text(template.about()).size(style::SMALL).color(style::TEXT_DIM)].spacing(3);
        if !outputs.is_empty() {
            about = about.push(text(format!("Outputs: {}", outputs.join(", "))).size(style::SMALL).color(style::TEXT_FAINT));
        }
        templates = templates.push(
            button(about).width(Length::Fill).padding([8, 10]).style(style::option_button(chosen, false)).on_press(say(N::Template(template))),
        );
    }
    let sizes = iced::widget::pick_list(crate::view::toolbar::RESOLUTIONS, Some(dialog.size), |s| Message::NewGraph(N::Size(s)))
        .text_size(style::TEXT_SIZE)
        .padding([4, 8])
        .style(style::pick_list);
    let mut precisions = row![].spacing(0);
    for (precision, name) in [(Precision::Bits8, "8 bits"), (Precision::Half, "16 bits float"), (Precision::Full, "32 bits float")] {
        precisions = precisions.push(
            button(text(name).size(style::TEXT_SIZE)).padding([5, 10]).style(style::segment(precision == dialog.precision)).on_press(say(N::Precision(precision))),
        );
    }
    let body = column![
        text(title).size(18).font(style::bold()),
        row![label("Name"), name].align_y(Alignment::Center),
        row![label("Template"), templates].align_y(Alignment::Start),
        row![label("Size"), sizes, text("px").size(style::SMALL).color(style::TEXT_FAINT)].spacing(6).align_y(Alignment::Center),
        row![label("Precision"), container(precisions).style(style::segments)].align_y(Alignment::Center),
        row![
            space().width(Length::Fill),
            button(text("Cancel").size(style::TEXT_SIZE)).padding([6, 14]).style(style::flat_button).on_press(Message::Dialog(None)),
            button(text("Create").size(style::TEXT_SIZE)).padding([6, 18]).style(style::primary_button).on_press(say(N::Create)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(14);
    modal(dialog_box(body, 560.0), None)
}

/// The export dialog: what is written, in what, and where; and the files, listed.
fn export<'a>(app: &'a App, dialog: &'a crate::app::export_dialog::ExportDialog) -> Element<'a, Message> {
    use brewed_texture_core::export_presets::Preset;
    use brewed_texture_core::nodes::{DEPTH_8, DEPTH_16};
    use crate::app::export_dialog::ExportMessage as X;
    let say = Message::Export;
    let label = |t: &'static str| text(t).size(style::TEXT_SIZE).color(style::TEXT_DIM).width(Length::Fixed(110.0));
    let mut presets = column![].spacing(4);
    for preset in Preset::ALL {
        let chosen = preset == dialog.preset;
        presets = presets.push(
            button(column![text(preset.name()).size(style::TEXT_SIZE).font(style::bold()), text(preset.about()).size(style::SMALL).color(style::TEXT_DIM)].spacing(3))
                .width(Length::Fill)
                .padding([7, 10])
                .style(style::option_button(chosen, false))
                .on_press(say(X::Preset(preset))),
        );
    }
    let segmented = |options: Vec<(&'static str, Message, bool)>| {
        let mut r = row![].spacing(0);
        for (name, message, chosen) in options {
            r = r.push(button(text(name).size(style::TEXT_SIZE)).padding([5, 10]).style(style::segment(chosen)).on_press(message));
        }
        container(r).style(style::segments)
    };
    // (A first base: the window is to be made again, as its user said, and says so.)
    let mut body = column![
        text("ПЕРЕДЕЛАТЬ!!!").size(30).font(style::heavy()).color(style::ERROR),
        text(format!("Export {}", app.doc.entry().name)).size(18).font(style::bold()),
        row![label("Preset"), presets].align_y(Alignment::Start),
    ]
    .spacing(12);
    if dialog.preset != Preset::Outputs {
        // What the graph makes, and what the preset writes of it.
        let (wants, writes) = if dialog.preset.directx() { ("DirectX (Y−)", "DirectX") } else { ("OpenGL (Y+)", "OpenGL") };
        let written = if dialog.directx == dialog.preset.directx() {
            format!("{} reads {wants} normal maps: written as the graph makes them", dialog.preset.name())
        } else {
            format!("{} reads {wants} normal maps: the green channel is turned over to make them {writes}'s", dialog.preset.name())
        };
        body = body.push(
            row![
                label("Graph's normals"),
                column![
                    segmented(vec![
                        ("OpenGL (Y+)", say(X::DirectX(false)), !dialog.directx),
                        ("DirectX (Y−)", say(X::DirectX(true)), dialog.directx),
                    ]),
                    text(written).size(style::SMALL).color(style::TEXT_DIM),
                ]
                .spacing(4),
            ]
            .spacing(8)
            .align_y(Alignment::Start),
        );
        body = body.push(
            row![
                label("Format"),
                segmented(vec![("PNG 8 bits", say(X::Depth(DEPTH_8)), dialog.depth == DEPTH_8), ("PNG 16 bits", say(X::Depth(DEPTH_16)), dialog.depth == DEPTH_16)]),
            ]
            .align_y(Alignment::Center),
        );
    }
    let folder = dialog.folder.as_ref().map_or_else(|| "No folder chosen yet".to_string(), |f| f.display().to_string());
    body = body.push(
        row![
            label("Folder"),
            container(text(folder).size(style::TEXT_SIZE).wrapping(text::Wrapping::None)).width(Length::Fill).clip(true),
            button(text("Browse…").size(style::TEXT_SIZE)).padding([5, 12]).style(style::button).on_press(say(X::Browse)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    // The files, before they are written.
    let planned = app.export_plan(dialog, dialog.folder.as_deref().unwrap_or(std::path::Path::new("")));
    let mut files = column![].spacing(3);
    for (path, file) in &planned {
        let name = path.file_name().map_or_else(|| file.name.clone(), |n| n.to_string_lossy().into_owned());
        let mut line = row![text(name).size(style::SMALL).font(style::mono()).width(Length::FillPortion(3))].spacing(10);
        if !file.said.is_empty() {
            line = line.push(text(format!("← {}", file.said)).size(style::SMALL).color(style::TEXT_DIM).width(Length::FillPortion(4)));
        }
        files = files.push(line);
    }
    if planned.is_empty() {
        files = files.push(text("None: the graph has no Output of what this writes (give Outputs their Usage).").size(style::SMALL).color(style::WARNING));
    }
    body = body.push(row![label("Files"), container(scrollable(files).height(Length::Shrink).spacing(8)).max_height(160.0).width(Length::Fill)].align_y(Alignment::Start));
    let ready = dialog.folder.is_some() && !planned.is_empty();
    body = body.push(
        row![
            space().width(Length::Fill),
            button(text("Cancel").size(style::TEXT_SIZE)).padding([6, 14]).style(style::flat_button).on_press(Message::Dialog(None)),
            button(text("Export").size(style::TEXT_SIZE)).padding([6, 18]).style(style::primary_button).on_press_maybe(ready.then(|| say(X::Export))),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    modal(dialog_box(body, 680.0), None)
}

/// The earlier versions of a package: each, when it was kept and what it is, to be restored in
/// place of the package or opened beside it.
fn versions<'a>(app: &'a App, dialog: &'a crate::app::versions::VersionsDialog) -> Element<'a, Message> {
    use brewed_texture_core::versions::Kind;
    use crate::app::versions::VersionsMessage as V;
    let name = dialog.path.file_name().map_or_else(|| dialog.path.display().to_string(), |n| n.to_string_lossy().into_owned());
    let mut list = column![].spacing(2);
    for version in &dialog.list {
        let what = match version.kind {
            Kind::Saved => "Saved",
            Kind::Found => "The file as it was before a save wrote over it",
            Kind::Replaced => "Unsaved changes, as they were when an earlier version was restored",
        };
        list = list.push(
            container(
                row![
                    icon(Icon::Package, 16.0, style::TEXT_DIM),
                    column![
                        text(crate::clock::said(version.time)).size(style::TEXT_SIZE).font(style::bold()),
                        text(what).size(style::SMALL).color(style::TEXT_DIM),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    button(text("Open a Copy").size(style::TEXT_SIZE))
                        .padding([5, 12])
                        .style(style::flat_button)
                        .on_press(Message::Versions(V::Copy(version.file.clone()))),
                    button(text("Restore").size(style::TEXT_SIZE))
                        .padding([5, 12])
                        .style(style::button)
                        .on_press(Message::Versions(V::Restore(version.file.clone()))),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding([6, 8]),
        );
    }
    let kept = app.settings.keep_versions;
    if dialog.list.is_empty() {
        let none = if kept == 0 { "None: earlier versions are not kept (Settings → Recovery)." } else { "None yet: each save keeps one." };
        list = list.push(text(none).size(style::TEXT_SIZE).color(style::TEXT_DIM));
    }
    let restoring = if app.package_doc(dialog.key).is_some_and(|d| d.is_dirty()) {
        "Restored, a version takes the package's place, not saved yet: saving saves it. Its unsaved changes are kept as a version first."
    } else {
        "Restored, a version takes the package's place, not saved yet: saving saves it. The file stays as it is until then."
    };
    let body = column![
        text(format!("History of {name}")).size(18).font(style::bold()),
        text(format!("Each save keeps the package as it was saved: the newest {kept} of it, beside the settings, never beside the package."))
            .size(style::TEXT_SIZE)
            .color(style::TEXT_DIM),
        container(scrollable(list).height(Length::Shrink).spacing(8).style(style::scrollable)).max_height(360.0),
        text(restoring).size(style::SMALL).color(style::TEXT_FAINT),
        row![
            space().width(Length::Fill),
            button(text("Close").size(style::TEXT_SIZE)).padding([6, 14]).style(style::button).on_press(Message::Dialog(None)),
        ],
    ]
    .spacing(12);
    modal(dialog_box(body, 620.0), None)
}

/// Id of the name field of the New Package dialog (it takes the keyboard as it opens).
pub fn new_graph_name_id() -> iced::widget::Id {
    iced::widget::Id::new("brewed-new-graph-name")
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
        scrollable(list).height(Length::Shrink).spacing(8).style(style::scrollable),
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
    body = body.push(scrollable(list).height(Length::Shrink).spacing(8).style(style::scrollable));
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
