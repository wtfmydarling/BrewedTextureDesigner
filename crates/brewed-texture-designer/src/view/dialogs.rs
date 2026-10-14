//! Modal dialogs: unsaved changes, work kept by a program that stopped, files a package cannot
//! find, the settings (see [`crate::view::settings`]), keyboard and mouse, about.

use brewed_texture_core::package::{MissingKind, file_name};
use iced::widget::{button, center, column, container, mouse_area, opaque, row, rule, space, text};
use iced::{Alignment, Element, Length};

use crate::app::autosave::Recovery;
use crate::app::{App, Dialog, ExplorerMessage, Message, MissingFiles, PendingAction, SettingsMessage};
use crate::icons::{Icon, icon};
use crate::style;
use crate::widget::scroll::scroll_box;

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
    ("Points of HDRI lights", "In the 2D view: drag a light's point (a Line Light's two, a gradient's one) to place it; Shift keeps it to one axis"),
    ("3D view", "Left drag orbits, middle drag moves the camera, right drag turns the light"),
];

pub fn view<'a>(app: &'a App, dialog: &'a Dialog) -> Element<'a, Message> {
    match dialog {
        Dialog::Unsaved(action) => {
            let body = column![
                text("Save changes?").size(18).font(style::heavy()).color(style::TEXT_BRIGHT),
                text(format!("\"{}\" has unsaved changes. Do you want to save them before {}?", app.doc.title(), action_text(app, action)))
                    .size(style::TEXT_SIZE)
                    .color(style::TEXT_DIM),
                row![
                    space().width(Length::Fill),
                    button(text("Cancel").size(style::TEXT_SIZE)).padding([6, 14]).style(style::flat_button).on_press(Message::Dialog(None)),
                    button(text("Don't Save").size(style::TEXT_SIZE)).padding([6, 14]).style(style::button).on_press(Message::Proceed(action.clone())),
                    button(text("Save").size(style::TEXT_SIZE)).padding([6, 18]).style(style::primary_button).on_press(Message::SaveThen(action.clone())),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(14);
            modal(dialog_box(body, 440.0), None)
        }
        Dialog::Missing(missing) => missing_files(app, missing),
        Dialog::Notices(notices) => crate::view::notices::view(notices),
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
            // Headings as the settings' groups have them: white and heavy, a line between.
            let heading = |t: &'static str| {
                container(text(t).size(14).font(style::heavy()).color(style::TEXT_BRIGHT)).padding(iced::Padding {
                    top: 14.0,
                    bottom: 6.0,
                    left: 0.0,
                    right: 0.0,
                })
            };
            let line_between =
                rule::horizontal(1).style(|_| rule::Style { color: style::LINE, radius: 0.0.into(), fill_mode: rule::FillMode::Full, snap: true });
            let mut keyboard = column![heading("Keyboard")].spacing(4);
            for (_, commands) in crate::view::settings::GROUPS {
                for command in *commands {
                    let keys: Vec<String> = app.settings.shortcuts.of(*command).iter().map(|c| c.shown()).collect();
                    let keys = if keys.is_empty() { "—".to_string() } else { keys.join(" / ") };
                    // Where a key is the command's only with the pointer there (Space is one
                    // over the graph, another over the 2D view).
                    let place = match command.place() {
                        crate::shortcuts::Place::Graph => "  · over the graph",
                        crate::shortcuts::Place::View2d => "  · over the 2D view",
                        _ => "",
                    };
                    keyboard = keyboard.push(line(format!("{keys}{place}"), command.label().to_string()));
                }
            }
            keyboard = keyboard.push(line("Esc".into(), "Close menus and dialogs, cancel picking from the screen or a rename".into()));
            keyboard = keyboard.push(line("Enter".into(), "In a dialog, what its first button does: Save, Create, Export, Close".into()));
            let mut mouse = column![line_between, heading("Mouse")].spacing(4);
            for (what, does) in MOUSE {
                mouse = mouse.push(line(what.to_string(), does.to_string()));
            }
            let body = column![
                row![
                    text("Shortcuts").size(18).font(style::heavy()).color(style::TEXT_BRIGHT),
                    space().width(Length::Fill),
                    button(text("Change the Keys…").size(style::TEXT_SIZE))
                        .padding([4, 12])
                        .style(style::button)
                        .on_press(Message::Settings(SettingsMessage::OpenPage(crate::settings::Page::Shortcuts))),
                    button(text("Close").size(style::TEXT_SIZE)).padding([4, 12]).style(style::button).on_press(Message::Dialog(None)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
                scroll_box(column![keyboard, mouse].spacing(6)).height(Length::Shrink),
            ]
            .spacing(14);
            let dialog = container(body).padding(18).width(Length::Fixed(760.0)).max_height(640).style(style::popup);
            modal(dialog.into(), Some(Message::Dialog(None)))
        }
        Dialog::About => {
            let gpu = if app.adapter.is_empty() { "detecting…".to_string() } else { crate::gpu::said(&app.adapter, app.backend) };
            let title = row![
                iced::widget::image(crate::brand::logo_large()).width(Length::Fixed(48.0)).height(Length::Fixed(48.0)),
                column![
                    text(brewed_texture_core::APP_NAME).size(24).font(style::heavy()).color(style::TEXT_BRIGHT),
                    text(format!("Version {}", env!("CARGO_PKG_VERSION"))).size(style::TEXT_SIZE).color(style::TEXT_DIM),
                ]
                .spacing(2),
            ]
            .spacing(14)
            .align_y(Alignment::Center);
            let body = column![
                title,
                text(
                    "A node based procedural texture editor. Every image node runs as a shader on the GPU; \
                      CPU side pixel work (export, histograms, file decoding) uses SIMD on all cores."
                )
                .size(style::TEXT_SIZE),
                text(format!("GPU: {gpu}")).size(style::TEXT_SIZE),
                text(format!("CPU threads: {}", std::thread::available_parallelism().map_or(1, |n| n.get()))).size(style::TEXT_SIZE),
                // Whose the program is: anyone's (see LICENSE-MIT, LICENSE-APACHE and NOTICE).
                text(crate::third_party::OWN_LICENSE).size(style::TEXT_SIZE).color(style::TEXT_DIM),
                row![
                    space().width(Length::Fill),
                    button(text("Third-Party Notices…").size(style::TEXT_SIZE))
                        .padding([5, 14])
                        .style(style::button)
                        .on_press(Message::Dialog(Some(Dialog::Notices(Default::default())))),
                    button(text("Close").size(style::TEXT_SIZE)).padding([5, 14]).style(style::button).on_press(Message::Dialog(None))
                ]
                .spacing(8),
            ]
            .spacing(10);
            modal(dialog_box(body, 440.0), Some(Message::Dialog(None)))
        }
    }
}

/// The dialog of a new package, or of a new graph of one: its name, its template, the size and
/// precision of its images.
/// The package a new graph goes into, as the dialog lists it.
#[derive(Clone, Debug, PartialEq)]
struct Place {
    key: Option<u64>,
    said: String,
}

impl std::fmt::Display for Place {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.said)
    }
}

/// The dialog of a new graph, as Substance Designer's: on the left, darker, the templates by
/// category, as pictures or as a list; on the right, the package it goes in, its name, the size
/// of its images and their precision.
fn new_graph<'a>(app: &'a App, dialog: &'a crate::app::home::NewGraphDialog) -> Element<'a, Message> {
    use crate::app::home::NewGraphMessage as N;
    use brewed_texture_core::Precision;
    use brewed_texture_core::templates::{Category, Template};
    use home_screen::Layout;
    let say = Message::NewGraph;
    let layout = app.ui.template_layout;

    // The templates.
    // (Said on hover, as the same toggle of the Home screen says it.)
    let toggle = |this: Layout, glyph: Icon| {
        let chosen = layout == this;
        let said = if this == Layout::Grid { "As pictures" } else { "As a list" };
        let toggle = button(icon(glyph, 14.0, if chosen { style::TEXT_BRIGHT } else { style::TEXT_DIM }))
            .padding([4, 7])
            .style(style::segment(chosen))
            .on_press(say(N::Layout(this)));
        crate::view::tip(toggle, said, iced::widget::tooltip::Position::Bottom)
    };
    let mut templates = column![
        row![
            text("Templates").size(15).font(style::heavy()).color(style::TEXT_BRIGHT),
            space().width(Length::Fill),
            container(row![toggle(Layout::Grid, Icon::Grid), toggle(Layout::List, Icon::List)].spacing(2)).padding(2).style(style::segments),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(10);
    for category in Category::ALL {
        let of: Vec<&(Template, home_screen::Start)> = app.home.templates.iter().filter(|(t, _)| t.category() == category).collect();
        templates = templates.push(text(category.name()).size(style::TEXT_SIZE).font(style::heavy()).color(style::TEXT_BRIGHT));
        templates = templates.push(match layout {
            Layout::Grid => iced::widget::Row::with_children(
                of.into_iter().map(|(t, card)| home_screen::template_card(card, *t == dialog.template, say(N::Template(*t)), home_screen::Style::default())),
            )
            .spacing(10)
            .wrap()
            .into(),
            Layout::List => Element::from(
                column(
                    of.into_iter().map(|(t, card)| home_screen::template_row(card, *t == dialog.template, say(N::Template(*t)), home_screen::Style::default())),
                )
                .spacing(3),
            ),
        });
    }
    let templates =
        container(scroll_box(container(templates).padding(16)).height(Length::Fill)).width(Length::Fill).height(Length::Fill).style(style::template_area);

    // Where, what it is called, how big, how precise.
    let label = |t: &'static str| text(t).size(style::SMALL).color(style::TEXT_DIM);
    let mut places = vec![Place { key: None, said: "New package".into() }];
    for (key, doc) in app.packages() {
        places.push(Place { key: Some(key), said: doc.title() });
    }
    let chosen = places.iter().find(|p| p.key == dialog.into).cloned();
    let into = iced::widget::pick_list(places, chosen, |p: Place| Message::NewGraph(N::Into(p.key)))
        .width(Length::Fill)
        .text_size(style::TEXT_SIZE)
        .padding([5, 8])
        .style(style::pick_list);
    let name = iced::widget::text_input("Graph", &dialog.name)
        .id(new_graph_name_id())
        .on_input(|n| Message::NewGraph(N::Name(n)))
        .on_submit(say(N::Create))
        .size(style::TEXT_SIZE)
        .padding([5, 8])
        .style(style::text_input);
    let side = |value: u32, message: fn(u32) -> N| {
        iced::widget::pick_list(crate::view::toolbar::RESOLUTIONS, Some(value), move |s| Message::NewGraph(message(s)))
            .width(Length::Fill)
            .text_size(style::TEXT_SIZE)
            .padding([5, 8])
            .style(style::pick_list)
    };
    let link = button(icon(Icon::Link, 15.0, if dialog.linked { style::TEXT_BRIGHT } else { style::TEXT_DIM }))
        .padding(5)
        .style(style::segment(dialog.linked))
        .on_press(say(N::Link(!dialog.linked)));
    let link = crate::view::tip(
        link,
        if dialog.linked { "Width and height together: click to set them apart" } else { "Width and height apart: click to set them together" },
        iced::widget::tooltip::Position::Top,
    );
    let size = row![side(dialog.size[0], N::Width), link, side(dialog.size[1], N::Height)].spacing(6).align_y(Alignment::Center);
    let mut precisions = row![].spacing(0);
    for (precision, said) in [(Precision::Bits8, "8 bits"), (Precision::Half, "16 bits float"), (Precision::Full, "32 bits float")] {
        precisions = precisions.push(
            button(text(said).size(style::TEXT_SIZE).font(style::chosen_font(precision == dialog.precision)))
                .padding([5, 10])
                .style(style::segment(precision == dialog.precision))
                .on_press(say(N::Precision(precision))),
        );
    }
    let chosen = dialog.template;
    let settings = column![
        column![label("Create graph in"), into].spacing(5),
        column![label("Graph name"), name].spacing(5),
        column![label("Size (pixels)"), size].spacing(5),
        column![label("Precision"), container(precisions).style(style::segments)].spacing(5),
        rule::horizontal(1).style(|_| rule::Style { color: style::LINE, radius: 0.0.into(), fill_mode: rule::FillMode::Full, snap: true }),
        text(format!("{} · {}", chosen.category().name(), chosen.name())).size(style::TEXT_SIZE).font(style::heavy()).color(style::TEXT_BRIGHT),
        text(chosen.about()).size(style::SMALL).color(style::TEXT_DIM),
    ]
    .spacing(16)
    .width(Length::Fixed(300.0));

    let body = column![
        text("New Graph").size(18).font(style::heavy()).color(style::TEXT_BRIGHT),
        row![templates, settings].spacing(18).height(Length::Fixed(470.0)),
        row![
            space().width(Length::Fill),
            button(text("Cancel").size(style::TEXT_SIZE)).padding([6, 14]).style(style::flat_button).on_press(Message::Dialog(None)),
            button(text("Create").size(style::TEXT_SIZE).font(style::bold())).padding([6, 18]).style(style::primary_button).on_press(say(N::Create)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(14);
    modal(dialog_box(body, 1000.0), None)
}

/// The export window (the `export-window` crate's), over the program's as the Settings are: not
/// closed by a click beside it, the templates changed in it being written as they change.
fn export<'a>(app: &'a App, dialog: &'a crate::app::export_dialog::ExportDialog) -> Element<'a, Message> {
    use crate::app::export_dialog::ExportMessage as X;
    let folder = dialog.folder.as_ref().map_or_else(|| "No folder chosen yet".to_string(), |f| f.display().to_string());
    let planned = app.export_plan(dialog, dialog.folder.as_deref().unwrap_or(std::path::Path::new("")));
    // The files, under the graph each is of.
    let files = dialog
        .rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.checked && row.graph.is_some())
        .map(|(i, row)| {
            let listed = planned
                .iter()
                .filter(|p| p.row == i)
                .map(|p| export_window::Listed {
                    name: p.path.file_name().map_or_else(|| p.file.name.clone(), |n| n.to_string_lossy().into_owned()),
                    said: p.file.said.clone(),
                    size: format!("{} × {}", p.size[0], p.size[1]),
                    kind: p.format.file_type().name().to_string(),
                    depth: p.format.depth().to_string(),
                })
                .collect();
            (row.name.clone(), listed)
        })
        .collect();
    // The maps a template may be made of: every usage of an Output; of the Outputs of no usage of
    // the graph whose settings are shown (or the first checked, or the one edited), each by its
    // name.
    let row = dialog
        .window
        .selected
        .filter(|&i| dialog.rows.get(i).is_some_and(|r| r.graph.is_some()))
        .or_else(|| dialog.rows.iter().position(|r| r.checked && r.graph.is_some()))
        .and_then(|i| dialog.rows.get(i));
    let entry = row.and_then(|r| app.package_doc(r.key)?.package().graph(r.graph?));
    let graph = entry.map_or_else(|| app.graph(), |e| &*e.graph);
    let maps = brewed_texture_core::export_presets::maps(graph);
    let has: Vec<texture_export::MapId> = maps.iter().filter_map(|m| m.usage.clone()).collect();
    let custom = maps.iter().filter_map(|m| m.usage.clone().filter(|u| u.as_str().starts_with("custom:")).map(|u| (u, m.name.clone()))).collect();
    let inputs = brewed_texture_core::export_presets::input_maps()
        .into_iter()
        .map(|(map, name, group)| export_window::InputMap { map, name: name.into(), group: group.into() })
        .collect();
    let values = row.map_or_else(|| app.export_values_of(app.doc_key, None), |r| app.export_values_of(r.key, r.graph));
    let exported = dialog
        .rows
        .iter()
        .map(|r| export_window::Exported {
            name: r.name.clone(),
            depth: u8::from(r.graph.is_some()),
            checked: r.checked,
            own: r.own,
            size: r.size,
            normals: r.normals,
        })
        .collect();
    let content = export_window::Content {
        thing: "graph",
        presets: dialog.presets(),
        mine: &dialog.templates,
        templates_folder: crate::app::export_dialog::templates_folder().map(|f| f.display().to_string()),
        problems: &dialog.problems,
        exported,
        global: dialog.global,
        folder: crate::view::fitted(folder, style::TEXT_SIZE, style::ui_font(), None),
        folder_chosen: dialog.folder.is_some(),
        files,
        tokens: &brewed_texture_core::export_presets::TOKENS,
        values,
        inputs,
        custom,
        has,
    };
    let window = export_window::view(content, &dialog.window, &style::export_look(), |event| Message::Export(X::Window(event)));
    let window = container(window).width(Length::Fill).max_width(1220).height(Length::Fill).max_height(800).style(style::popup).clip(true);
    opaque(mouse_area(container(center(opaque(window))).padding(24).width(Length::Fill).height(Length::Fill).style(style::scrim)))
}

/// The earlier versions of a package: each, when it was kept and what it is, to be restored in
/// place of the package or opened beside it.
fn versions<'a>(app: &'a App, dialog: &'a crate::app::versions::VersionsDialog) -> Element<'a, Message> {
    use crate::app::versions::VersionsMessage as V;
    use brewed_texture_core::versions::Kind;
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
        text(format!("History of {name}")).size(18).font(style::heavy()).color(style::TEXT_BRIGHT),
        text(format!("Each save keeps the package as it was saved: the newest {kept} of it, beside the settings, never beside the package."))
            .size(style::TEXT_SIZE)
            .color(style::TEXT_DIM),
        container(scroll_box(list).height(Length::Shrink)).max_height(360.0),
        text(restoring).size(style::SMALL).color(style::TEXT_FAINT),
        row![space().width(Length::Fill), button(text("Close").size(style::TEXT_SIZE)).padding([6, 14]).style(style::button).on_press(Message::Dialog(None)),],
    ]
    .spacing(12);
    modal(dialog_box(body, 620.0), None)
}

/// Id of the name field of the New Graph dialog (it takes the keyboard as it opens).
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
                    text(place).size(style::SMALL).color(style::TEXT_FAINT).wrapping(text::Wrapping::WordOrGlyph),
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
        text("Recover unsaved work?").size(18).font(style::heavy()).color(style::TEXT_BRIGHT),
        text(format!("{} stopped without quitting last time. What these packages had not saved yet was kept:", brewed_texture_core::APP_NAME))
            .size(style::TEXT_SIZE)
            .color(style::TEXT_DIM),
        scroll_box(list).height(Length::Shrink),
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
                    text(&item.path).size(style::SMALL).color(style::TEXT_FAINT).wrapping(text::Wrapping::WordOrGlyph),
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
        text("Missing files").size(18).font(style::heavy()).color(style::TEXT_BRIGHT),
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
    body = body.push(scroll_box(list).height(Length::Shrink));
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
