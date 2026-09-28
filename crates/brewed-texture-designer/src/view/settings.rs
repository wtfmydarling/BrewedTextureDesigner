//! The settings dialog (Edit → Settings…), as the programs this one follows have it: the pages
//! listed on the left, the settings of the one chosen on the right, each a row of its own —
//! what it is on the left, its value on the right. A value changed and not applied yet is
//! marked on its row and its page; one that is not as it is by default can be set back. Apply
//! takes the changes, Cancel forgets them. And the offer to start the program again, for the
//! settings it takes only then.

use iced::widget::{button, center, column, container, mouse_area, opaque, pick_list, row, scrollable, space, text, toggler};
use iced::{Alignment, Color, Element, Length, Padding};
use brewed_texture_core::Precision;

use crate::app::{App, Message, SettingsDialog, SettingsMessage};
use crate::icons::{Icon, icon};
use crate::settings::{FileDialogs, Graphics, NewFiles, Page, Setting, Settings};
use crate::shortcuts::{Chord, Command, Place, Shortcuts};
use crate::style;
use crate::widget::number::number;

/// How wide the values are.
const CONTROL: f32 = 300.0;

/// The commands on the Shortcuts page, in groups.
pub(crate) const GROUPS: &[(&str, &[Command])] = &[
    ("Packages", &[Command::New, Command::Open, Command::Save, Command::SaveAs, Command::Export]),
    (
        "Editing",
        &[Command::Undo, Command::Redo, Command::Copy, Command::Cut, Command::Paste, Command::Duplicate, Command::Delete, Command::SelectAll],
    ),
    ("Graph", &[Command::Bypass, Command::GraphFromSelection, Command::Frame, Command::AddComment, Command::AddNode]),
    ("2D View", &[Command::ShowTiling]),
    ("Program", &[Command::Settings, Command::Help]),
];

fn set(setting: Setting) -> Message {
    Message::Settings(SettingsMessage::Set(setting))
}

fn say(message: SettingsMessage) -> Message {
    Message::Settings(message)
}

pub fn view<'a>(app: &'a App, dialog: &'a SettingsDialog) -> Element<'a, Message> {
    let applied = &app.settings;
    let draft = &dialog.draft;
    let pending = draft != applied;

    // The pages, and which have changes not applied.
    let mut pages = column![].spacing(2).padding([10, 8]);
    for page in Page::ALL {
        let mut label = row![text(page.name()).size(style::TEXT_SIZE)].align_y(Alignment::Center);
        if draft.differ(applied, page) {
            label = label.push(space().width(Length::Fill)).push(text("●").size(style::CAPTION).color(style::TEXT_DIM));
        }
        pages = pages.push(
            button(label)
                .width(Length::Fill)
                .padding([7, 12])
                .style(style::row_button(page == dialog.page))
                .on_press(say(SettingsMessage::Page(page))),
        );
    }
    let sidebar = container(pages).width(Length::Fixed(190.0)).height(Length::Fill).style(style::sidebar);

    let page = dialog.page;
    let mut content = column![
        text(page.name()).size(20).font(style::bold()),
        text(page.about()).size(style::TEXT_SIZE).color(style::TEXT_DIM),
    ]
    .spacing(4)
    .padding(Padding { top: 18.0, right: 22.0, bottom: 22.0, left: 22.0 });
    let body: Vec<Element<'a, Message>> = match page {
        Page::General => general(applied, draft),
        Page::Packages => packages(applied, draft),
        Page::Recovery => recovery(applied, draft),
        Page::Performance => performance(app, applied, draft),
        Page::Shortcuts => shortcuts(applied, dialog),
    };
    for part in body {
        content = content.push(part);
    }
    let content = scrollable(content).height(Length::Fill).width(Length::Fill).style(style::scrollable);

    // What is wrong with the settings file, over everything.
    let (file, problems) = app.settings_file();
    let mut head = column![text("Settings").size(16).font(style::bold())].spacing(10).padding([12, 18]);
    if !problems.is_empty() {
        let mut said: Vec<String> = problems.iter().take(3).cloned().collect();
        if problems.len() > 3 {
            said.push(format!("And {} more.", problems.len() - 3));
        }
        head = head.push(
            container(text(said.join("\n")).size(style::SMALL)).padding(8).width(Length::Fill).style(style::message_box(style::WARNING)),
        );
    }

    let kept = match file {
        Some(file) => format!("Kept in {}", file.display()),
        None => "Not kept: the program runs with the settings as they are by default.".to_string(),
    };
    let at_defaults = draft.reset(page) == *draft;
    let footer = row![
        container(text(kept).size(style::SMALL).color(style::TEXT_FAINT).wrapping(text::Wrapping::None)).width(Length::Fill).clip(true),
        button(text("Reset Page to Defaults").size(style::TEXT_SIZE))
            .padding([6, 12])
            .style(style::button)
            .on_press_maybe((!at_defaults).then(|| say(SettingsMessage::ResetPage))),
        button(text(if pending { "Cancel" } else { "Close" }).size(style::TEXT_SIZE))
            .padding([6, 14])
            .style(style::button)
            .on_press(say(SettingsMessage::Cancel)),
        button(text("Apply").size(style::TEXT_SIZE))
            .padding([6, 18])
            .style(style::primary_button)
            .on_press_maybe(pending.then(|| say(SettingsMessage::Apply))),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .padding([10, 18]);

    let mut window = column![
        head,
        crate::view::hairline(),
        row![sidebar, container(space()).width(1).height(Length::Fill).style(style::hairline(style::LINE)), content].height(Length::Fill),
        crate::view::hairline(),
    ];
    // Applied, what the program takes when it starts: the offer to start it again, here while
    // the dialog is open (in the corner of the window once it is closed).
    if app.ui.restart_offer {
        window = window.push(container(offer(app, true)).padding([10, 18]).width(Length::Fill).style(style::message_box(style::WARNING)));
        window = window.push(crate::view::hairline());
    }
    let window = window.push(footer);
    let dialog = container(window).width(Length::Fill).max_width(940).height(Length::Fill).max_height(660).style(style::popup);
    // Not closed by a click beside it: the changes would go with it.
    opaque(mouse_area(container(center(opaque(dialog))).padding(24).width(Length::Fill).height(Length::Fill).style(style::scrim)))
}

// ---------------------------------------------------------------------------- rows

/// A row of settings: what it is, and its value.
struct Row<'a> {
    label: &'a str,
    about: String,
    control: Element<'a, Message>,
    /// Changed, and not applied yet.
    pending: bool,
    /// What sets it as it is by default, when it is not.
    reset: Option<Message>,
    /// Taken only when the program starts.
    restart: bool,
    /// Said under it, in this color.
    note: Option<(String, Color)>,
}

impl<'a> Row<'a> {
    fn new(label: &'a str, about: impl Into<String>, control: impl Into<Element<'a, Message>>) -> Row<'a> {
        Row { label, about: about.into(), control: control.into(), pending: false, reset: None, restart: false, note: None }
    }

    /// Changed when `applied` and `draft` differ; not as by default when `draft` and `default`
    /// do, when `reset` sets it back.
    fn state<T: PartialEq>(mut self, applied: T, draft: T, default: T, reset: Message) -> Row<'a> {
        self.pending = applied != draft;
        self.reset = (draft != default).then_some(reset);
        self
    }

    fn restart(mut self) -> Row<'a> {
        self.restart = true;
        self
    }

    fn note(mut self, note: Option<(String, Color)>) -> Row<'a> {
        self.note = note;
        self
    }
}

/// A group of rows under a title: the rows striped, a line between two.
fn group<'a>(title: &'a str, rows: Vec<Row<'a>>) -> Element<'a, Message> {
    let mut list = column![].spacing(1);
    for (i, r) in rows.into_iter().enumerate() {
        list = list.push(strip(r, i % 2 == 0));
    }
    column![
        container(text(title.to_uppercase()).size(style::CAPTION).font(style::bold()).color(style::TEXT_FAINT))
            .padding(Padding { top: 20.0, bottom: 6.0, left: 2.0, right: 0.0 }),
        container(list).padding(1).width(Length::Fill).style(style::setting_list),
    ]
    .into()
}

fn strip<'a>(r: Row<'a>, even: bool) -> Element<'a, Message> {
    let pending = r.pending;
    let mut title = row![text(r.label).size(style::TEXT_SIZE).color(style::TEXT)].spacing(8).align_y(Alignment::Center);
    if r.restart {
        title = title.push(container(text("Restart").size(style::CAPTION)).padding([0, 5]).style(style::tag(style::WARNING)));
    }
    let mut said = column![title].spacing(3).width(Length::Fill);
    if !r.about.is_empty() {
        said = said.push(text(r.about).size(style::SMALL).color(style::TEXT_DIM));
    }
    if let Some((note, color)) = r.note {
        said = said.push(text(note).size(style::SMALL).color(color));
    }
    let reset: Element<'a, Message> = match r.reset {
        Some(reset) => crate::view::tip(
            button(icon(Icon::Reset, 13.0, style::TEXT_DIM)).padding(4).style(style::flat_button).on_press(reset),
            "Set it as it is by default",
            iced::widget::tooltip::Position::Left,
        ),
        None => space().width(Length::Fixed(21.0)).into(),
    };
    let line = row![said, container(r.control).width(Length::Fixed(CONTROL)), reset]
        .spacing(14)
        .align_y(Alignment::Center)
        .padding(Padding { top: 9.0, right: 8.0, bottom: 9.0, left: 12.0 });
    // A change not applied yet: a bar down the left of the row, in the color of what is chosen.
    container(container(line).width(Length::Fill).style(style::setting_row(even)))
        .padding(Padding { left: 3.0, ..Padding::ZERO })
        .width(Length::Fill)
        .style(move |theme| match pending {
            true => container::Style { background: Some(style::ACCENT.into()), ..Default::default() },
            false => style::setting_row(even)(theme),
        })
        .into()
}

// ---------------------------------------------------------------------------- controls

/// Options side by side, one chosen: for a few options, all of them seen at once.
fn segmented<'a, T: Copy + PartialEq + 'a>(options: &[(T, &'a str)], chosen: T, enabled: bool, on: impl Fn(T) -> Setting) -> Element<'a, Message> {
    let mut segments = row![].spacing(2);
    for (value, label) in options {
        let option = button(text(*label).size(style::TEXT_SIZE).width(Length::Fill).align_x(Alignment::Center).wrapping(text::Wrapping::None))
            .width(Length::Fill)
            .padding([4, 6])
            .style(style::segment(*value == chosen));
        segments = segments.push(if enabled { option.on_press(set(on(*value))) } else { option });
    }
    container(segments).padding(2).width(Length::Fill).style(style::segments).into()
}

/// A list to choose one of `options` from.
fn choices<'a, T>(options: Vec<T>, chosen: T, enabled: bool, on: impl Fn(T) -> Setting + 'a) -> Element<'a, Message>
where
    T: ToString + PartialEq + Clone + 'a,
{
    if !enabled {
        // (A pick list cannot be disabled: a field that says the value, faint, in its place.)
        let said = text(chosen.to_string()).size(style::TEXT_SIZE).color(style::TEXT_FAINT);
        return container(said).padding([4, 8]).width(Length::Fill).style(style::field_box).into();
    }
    pick_list(options, Some(chosen), move |v| set(on(v))).text_size(style::TEXT_SIZE).padding([4, 8]).width(Length::Fill).style(style::pick_list).into()
}

fn switch<'a>(on: bool, enabled: bool, setting: impl Fn(bool) -> Setting + 'a) -> Element<'a, Message> {
    let toggle = toggler(on).size(18.0).style(style::toggler);
    let toggle = if enabled { toggle.on_toggle(move |v| set(setting(v))) } else { toggle };
    row![toggle, text(if on { "On" } else { "Off" }).size(style::TEXT_SIZE).color(style::TEXT_DIM)].spacing(10).align_y(Alignment::Center).into()
}

/// `choices`, and `current` among them in its place if it is not one of them (a value the
/// settings file says).
fn with<T: Copy + PartialEq>(choices: &[T], current: T, order: impl Fn(&T) -> u32) -> Vec<T> {
    let mut all = choices.to_vec();
    if !all.contains(&current) {
        all.push(current);
        all.sort_by_key(|c| order(c));
    }
    all
}

// ---------------------------------------------------------------------------- pages

fn general<'a>(applied: &Settings, draft: &Settings) -> Vec<Element<'a, Message>> {
    let d = Settings::default();
    let undo = number(f64::from(draft.undo_steps), 10.0, 1000.0)
        .integer(true)
        .logarithmic(true)
        .limits(10.0, 1000.0)
        .default_value(f64::from(d.undo_steps))
        .on_change(|v| set(Setting::UndoSteps(v.round() as u32)));
    let recent = number(f64::from(draft.recent_packages), 0.0, 20.0)
        .integer(true)
        .limits(0.0, 20.0)
        .default_value(f64::from(d.recent_packages))
        .on_change(|v| set(Setting::RecentPackages(v.round() as u32)));
    let folders = segmented(
        &[(FileDialogs::Package, "Package's folder"), (FileDialogs::Last, "Last folder")],
        draft.file_dialogs,
        true,
        Setting::FileDialogs,
    );
    vec![
        group(
            "Editing",
            vec![
                Row::new("Undo steps", "How many steps Undo can go back in each package. Each keeps what it changed in memory.", undo)
                    .state(applied.undo_steps, draft.undo_steps, d.undo_steps, set(Setting::UndoSteps(d.undo_steps))),
            ],
        ),
        group(
            "Files",
            vec![
                Row::new("Recent packages", "How many packages File → Open Recent offers; 0 offers none.", recent).state(
                    applied.recent_packages,
                    draft.recent_packages,
                    d.recent_packages,
                    set(Setting::RecentPackages(d.recent_packages)),
                ),
                Row::new(
                    "File dialogs open in",
                    match draft.file_dialogs {
                        FileDialogs::Package => "The folder of the package being edited; for one never saved, the folder a dialog was answered from last.",
                        FileDialogs::Last => "The folder a dialog was answered from last, whatever package is being edited.",
                    },
                    folders,
                )
                .state(applied.file_dialogs, draft.file_dialogs, d.file_dialogs, set(Setting::FileDialogs(d.file_dialogs))),
            ],
        ),
        group(
            "Navigation",
            vec![
                Row::new(
                    "Invert zoom",
                    "The mouse wheel pushed away zooms out, rather than in: in the graph, and in the 2D and 3D views.",
                    switch(draft.invert_zoom, true, Setting::InvertZoom),
                )
                .state(applied.invert_zoom, draft.invert_zoom, d.invert_zoom, set(Setting::InvertZoom(d.invert_zoom))),
            ],
        ),
    ]
}

/// A texture size, as the lists say it.
#[derive(Clone, Copy, PartialEq)]
struct Side(u32);

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{0} × {0}", self.0)
    }
}

fn packages<'a>(applied: &Settings, draft: &Settings) -> Vec<Element<'a, Message>> {
    let d = Settings::default();
    let files = segmented(&[(NewFiles::Embed, "Embed in the package"), (NewFiles::Link, "Link to the file")], draft.new_files, true, Setting::NewFiles);
    let sizes = with(&[256, 512, 1024, 2048, 4096, 8192].map(Side), Side(draft.graph_size), |s| s.0);
    let precision = segmented(
        &[(Precision::Bits8, "8 bits"), (Precision::Half, "16 bits float"), (Precision::Full, "32 bits float")],
        draft.graph_precision,
        true,
        Setting::GraphPrecision,
    );
    vec![
        group(
            "Files",
            vec![
                Row::new(
                    "A file chosen or dropped",
                    match draft.new_files {
                        NewFiles::Embed => "The package holds it: it is one file to copy or send, and needs nothing beside it.",
                        NewFiles::Link => "The package names it where it is on disk: it goes with the package only if copied along.",
                    },
                    files,
                )
                .state(applied.new_files, draft.new_files, d.new_files, set(Setting::NewFiles(d.new_files))),
            ],
        ),
        group(
            "New Graphs",
            vec![
                Row::new("Size", "The size of the images of a graph made new. Each graph has its own, in its properties.", choices(sizes, Side(draft.graph_size), true, |s: Side| Setting::GraphSize(s.0)))
                    .state(applied.graph_size, draft.graph_size, d.graph_size, set(Setting::GraphSize(d.graph_size))),
                Row::new("Precision", "What its images are kept in: more bits, finer steps, and more memory.", precision).state(
                    applied.graph_precision,
                    draft.graph_precision,
                    d.graph_precision,
                    set(Setting::GraphPrecision(d.graph_precision)),
                ),
            ],
        ),
    ]
}

/// How often, as people say it.
#[derive(Clone, Copy, PartialEq)]
struct Minutes(u32);

impl std::fmt::Display for Minutes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            1 => f.write_str("Every minute"),
            n => write!(f, "Every {n} minutes"),
        }
    }
}

fn recovery<'a>(applied: &Settings, draft: &Settings) -> Vec<Element<'a, Message>> {
    let d = Settings::default();
    let every = with(&[1, 2, 5, 10, 15, 30].map(Minutes), Minutes(draft.keep_every), |m| m.0);
    vec![group(
        "Unsaved Work",
        vec![
            Row::new(
                "Keep unsaved work",
                if draft.keep_unsaved {
                    "What the packages have not saved is kept beside the settings, and offered back if the program stops without quitting."
                } else {
                    "Nothing is kept: what was not saved is gone if the program stops without quitting."
                },
                switch(draft.keep_unsaved, true, Setting::KeepUnsaved),
            )
            .state(applied.keep_unsaved, draft.keep_unsaved, d.keep_unsaved, set(Setting::KeepUnsaved(d.keep_unsaved))),
            Row::new(
                "How often",
                "Only packages that changed since are kept again. On a slow disk, less often is kinder.",
                choices(every, Minutes(draft.keep_every), draft.keep_unsaved, |m: Minutes| Setting::KeepEvery(m.0)),
            )
            .state(applied.keep_every, draft.keep_every, d.keep_every, set(Setting::KeepEvery(d.keep_every))),
        ],
    )]
}

/// An amount of memory, as people say it.
#[derive(Clone, Copy, PartialEq)]
struct Megabytes(u32);

impl std::fmt::Display for Megabytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            0 => f.write_str("None"),
            mb if mb >= 1024 && mb % 1024 == 0 => write!(f, "{} GB", mb / 1024),
            mb if mb >= 1024 => write!(f, "{:.1} GB", f64::from(mb) / 1024.0),
            mb => write!(f, "{mb} MB"),
        }
    }
}

fn performance<'a>(app: &App, applied: &Settings, draft: &Settings) -> Vec<Element<'a, Message>> {
    let d = Settings::default();
    let started = app.started_with();
    let cache = with(&[0, 256, 512, 1024, 2048, 4096, 8192].map(Megabytes), Megabytes(draft.image_cache), |m| m.0);
    let kept = app.res.memory.kept;
    let cache_note = (applied.image_cache > 0 && kept > 0)
        .then(|| (format!("Kept now: {}.", Megabytes((kept >> 20) as u32)), style::TEXT_FAINT));
    let apis: Vec<(Graphics, &str)> = Graphics::here().iter().map(|g| (*g, g.name())).collect();
    let later = |differs: bool| differs.then(|| ("Taken when the program starts again.".to_string(), style::WARNING));
    let api_note = match app.graphics_from_environment() {
        Some(env) => Some((format!("The environment says WGPU_BACKEND={env}: that comes first."), style::WARNING)),
        None => later(draft.graphics != started.graphics),
    };
    vec![
        group(
            "Video Memory",
            vec![
                Row::new(
                    "Image cache",
                    "The images of the nodes no view shows are kept while they fit: a change then computes only what follows it. Those used least lately go first.",
                    choices(cache, Megabytes(draft.image_cache), true, |m: Megabytes| Setting::ImageCache(m.0)),
                )
                .state(applied.image_cache, draft.image_cache, d.image_cache, set(Setting::ImageCache(d.image_cache)))
                .note(cache_note),
            ],
        ),
        group(
            "Drawing",
            vec![
                Row::new("Graphics API", "What the program draws and computes with. Automatic takes the one the system does best.", segmented(&apis, draft.graphics, true, Setting::Graphics))
                    .state(applied.graphics, draft.graphics, d.graphics, set(Setting::Graphics(d.graphics)))
                    .restart()
                    .note(api_note),
                Row::new(
                    "Antialiasing",
                    "Smooth edges of the links, nodes and curves the program draws. Off, it takes less video memory, on large screens most.",
                    switch(draft.antialiasing, true, Setting::Antialiasing),
                )
                .state(applied.antialiasing, draft.antialiasing, d.antialiasing, set(Setting::Antialiasing(d.antialiasing)))
                .restart()
                .note(later(draft.antialiasing != started.antialiasing)),
            ],
        ),
    ]
}

fn shortcuts<'a>(applied: &Settings, dialog: &'a SettingsDialog) -> Vec<Element<'a, Message>> {
    let draft = &dialog.draft;
    let mut parts = Vec::new();
    if let Some(note) = &dialog.note {
        parts.push(
            container(text(note.clone()).size(style::SMALL)).padding(8).width(Length::Fill).style(style::message_box(style::TEXT_DIM)).into(),
        );
    }
    for (title, commands) in GROUPS {
        let rows = commands
            .iter()
            .map(|command| {
                let keys = draft.shortcuts.of(*command);
                let about = match command.place() {
                    Place::Anywhere => "Also while typing, when its key has Ctrl or is an F key.",
                    Place::Window => "",
                    Place::Graph => "With the pointer over the graph.",
                    Place::View2d => "With the pointer over the 2D view.",
                };
                Row::new(command.label(), about, key_slots(*command, &keys, dialog.capturing)).state(
                    applied.shortcuts.of(*command),
                    keys,
                    Shortcuts::defaults(*command),
                    say(SettingsMessage::ResetKeys(*command)),
                )
            })
            .collect();
        parts.push(group(title, rows));
    }
    parts
}

/// The keys of a command: each a field that waits for another key when clicked, and a key
/// more after them.
fn key_slots<'a>(command: Command, keys: &[Chord], capturing: Option<(Command, usize)>) -> Element<'a, Message> {
    let mut slots = row![].spacing(6).align_y(Alignment::Center);
    for slot in 0..Shortcuts::SLOTS {
        let waiting = capturing == Some((command, slot));
        let key = keys.get(slot);
        if key.is_none() && slot > keys.len() {
            break;
        }
        let (label, color) = match (waiting, key) {
            (true, _) => ("Press a key…".to_string(), style::ACCENT_TEXT),
            (false, Some(chord)) => (chord.shown(), style::TEXT),
            (false, None) => ("Add a key".to_string(), style::TEXT_FAINT),
        };
        let field = button(text(label).size(style::TEXT_SIZE).color(color).wrapping(text::Wrapping::None))
            .width(Length::Fixed(118.0))
            .padding([4, 8])
            .style(move |theme, status| if waiting { style::toggle_button(true)(theme, status) } else { style::choice_field(false)(theme, status) })
            .on_press(say(if waiting { SettingsMessage::StopCapture } else { SettingsMessage::Capture(command, slot) }));
        let clear: Element<'a, Message> = match key {
            Some(_) if !waiting => button(icon(Icon::Close, 11.0, style::TEXT_DIM))
                .padding(4)
                .style(style::flat_button)
                .on_press(say(SettingsMessage::ClearKey(command, slot)))
                .into(),
            _ => space().width(Length::Fixed(19.0)).into(),
        };
        slots = slots.push(row![field, clear].spacing(2).align_y(Alignment::Center));
    }
    slots.into()
}

// ---------------------------------------------------------------------------- restart

/// The offer to start the program again, once settings it takes only when it starts are
/// applied: what changed, and Later or Restart Now; `wide`, in a line across the settings
/// dialog, else stacked, in the corner of the window.
fn offer<'a>(app: &App, wide: bool) -> Element<'a, Message> {
    let started = app.started_with();
    let mut what = Vec::new();
    if app.settings.graphics != started.graphics {
        what.push("the graphics API");
    }
    if app.settings.antialiasing != started.antialiasing {
        what.push("antialiasing");
    }
    let said = column![
        row![icon(Icon::Reset, 16.0, style::TEXT), text("Restart the program?").size(14).font(style::bold()).color(style::TEXT)]
            .spacing(8)
            .align_y(Alignment::Center),
        text(format!(
            "The settings changed {}, which the program takes when it starts. Packages with unsaved changes ask to be saved first.",
            what.join(" and ")
        ))
        .size(style::SMALL)
        .color(style::TEXT_DIM),
    ]
    .spacing(6);
    let buttons = row![
        button(text("Later").size(style::TEXT_SIZE)).padding([5, 14]).style(style::button).on_press(say(SettingsMessage::RestartLater)),
        button(text("Restart Now").size(style::TEXT_SIZE)).padding([5, 14]).style(style::primary_button).on_press(say(SettingsMessage::Restart)),
    ]
    .spacing(8);
    if wide {
        row![said.width(Length::Fill), buttons].spacing(18).align_y(Alignment::Center).into()
    } else {
        column![said, row![space().width(Length::Fill), buttons]].spacing(10).into()
    }
}

/// The offer to start the program again (see [`offer`]), in the lower right corner of the
/// window, over what is there: the settings dialog shows it itself.
pub fn restart_offer(app: &App) -> Element<'_, Message> {
    let toast = opaque(container(offer(app, false)).padding(14).width(Length::Fixed(360.0)).style(style::popup));
    container(toast)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::End)
        .align_y(Alignment::End)
        .padding(Padding { top: 0.0, right: 16.0, bottom: 40.0, left: 0.0 })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Shortcuts page lists every command, once.
    #[test]
    fn every_command_is_on_the_page() {
        let listed: Vec<Command> = GROUPS.iter().flat_map(|(_, commands)| commands.iter().copied()).collect();
        for command in Command::ALL {
            assert_eq!(listed.iter().filter(|c| *c == command).count(), 1, "{command:?}");
        }
        assert_eq!(listed.len(), Command::ALL.len());
    }
}
