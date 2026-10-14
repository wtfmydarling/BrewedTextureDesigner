//! The window an export of textures is set up in, made to be the same in every program of a
//! set, as Substance Painter's export window is: SETTINGS (what is exported, checked in a list
//! of the projects open and what each exports; the global settings, and those of each where
//! they differ: the template, the format, the size), OUTPUT TEMPLATES (the presets, the
//! program's and the user's, each changed where it is shown; the output maps of the one shown,
//! each a row of a name and channels; the input maps, dragged onto the channels), LIST OF
//! EXPORTS (the files before they are written). A program gives what it has (see [`Content`]:
//! its presets, the parts of a name it fills in, its maps, what it exports and its word for
//! it) and its look, keeps the window's [`State`], and takes what is done as [`Event`]s: it
//! writes the files, keeps the templates (see `texture_export::write_template`) and the settings
//! with its projects. It knows nothing of the program.

mod editor;
pub mod style;

use drop_down::dropdown;
use iced::widget::{Column, button, column, container, mouse_area, pick_list, pin, row, space, stack, text, text_input};
use iced::{Alignment, Color, Element, Font, Length, Point, Theme};
use scroll_box::scroll_box;
use texture_export::naming::Token;
use texture_export::{Edit, FileFormat, FileType, Format, MapId, Normals, Size, Template};

/// Which template: one of the program's presets (its place in [`Content::presets`]), or one of
/// the user's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chosen {
    Preset(usize),
    Mine(usize),
}

/// A preset of the program's: its name, and the template it is (as the user changed it, where
/// `changed`: it can be reset); of none, the program's maps as they are, each written with the
/// name, channels and format it has (as in the project).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preset<'a> {
    pub name: &'a str,
    pub template: Option<&'a Template>,
    pub changed: bool,
}

/// The tabs of the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Settings,
    Templates,
    Files,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Settings, Tab::Templates, Tab::Files];

    pub fn name(self) -> &'static str {
        match self {
            Tab::Settings => "SETTINGS",
            Tab::Templates => "OUTPUT TEMPLATES",
            Tab::Files => "LIST OF EXPORTS",
        }
    }
}

/// A map being dragged onto a channel: what it is (`None`: a value), turned over, written as
/// normals of a way, and what it is called.
#[derive(Clone, Debug, PartialEq)]
pub struct Dragged {
    pub map: Option<MapId>,
    pub invert: bool,
    pub normals: Option<Normals>,
    pub label: String,
}

/// A list of options of the window, open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Menu {
    /// Of the settings shown: the template, the kind of file and its depth, the size, the normals.
    Template,
    FileType,
    Depth,
    Size,
    Normals,
    /// Of an output map of the template shown: its kind of file, its depth.
    Kind(usize),
    FileDepth(usize),
}

/// What an export is set to, of all that is exported (the global settings): the template the
/// files are written with, as what, and how large.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub template: Chosen,
    pub format: FileFormat,
    pub size: Size,
}

/// What one of what is exported says of its own; where it says nothing, it is as the global
/// settings say (of its normal maps: as it makes them).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Own {
    pub template: Option<Chosen>,
    pub format: Option<FileFormat>,
    pub size: Option<Size>,
    pub normals: Option<Normals>,
}

impl Settings {
    /// The settings of one that says `own`.
    pub fn with(self, own: &Own) -> Settings {
        Settings { template: own.template.unwrap_or(self.template), format: own.format.unwrap_or(self.format), size: own.size.unwrap_or(self.size) }
    }
}

/// A setting, as it is set.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Setting {
    Template(Chosen),
    Format(FileFormat),
    Size(Size),
    Normals(Normals),
}

/// Which setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Template,
    Format,
    Size,
    Normals,
}

/// A row of the list of what is exported: a project (a heading, checked where all of it is), or
/// what it exports (a graph; a texture set, and its UV sets under it), with what it says of
/// its own.
#[derive(Clone, Debug, PartialEq)]
pub struct Exported {
    pub name: String,
    /// How deep in the list: 0 a project.
    pub depth: u8,
    pub checked: bool,
    pub own: Own,
    /// The size it is made at, and which way its normal maps point as it makes them.
    pub size: [u32; 2],
    pub normals: Normals,
}

/// What the window keeps between frames (the program keeps it for it).
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub tab: Tab,
    /// The row of the list of what is exported whose settings are shown (none: the global ones).
    pub selected: Option<usize>,
    /// The list of options open.
    pub menu: Option<Menu>,
    /// The template shown in OUTPUT TEMPLATES.
    pub shown: Chosen,
    /// The channel of an output map chosen there (the map, its part): its options shown.
    pub slot: Option<(usize, usize)>,
    /// The output map whose menu of the parts of a name is open.
    pub tokens: Option<usize>,
    /// A template of the user's asked to be deleted, waiting for a yes.
    pub deleting: Option<usize>,
    /// A map being dragged, where the pointer is, and the channel it is over.
    pub dragging: Option<Dragged>,
    pub pointer: Point,
    pub over: Option<(usize, usize)>,
}

impl State {
    /// The window as it opens: SETTINGS, the global settings; the template written with shown in
    /// OUTPUT TEMPLATES (the first of `presets` that is one, where it is the maps as they are).
    pub fn new(chosen: Chosen, presets: &[Preset]) -> State {
        let shown = match chosen {
            Chosen::Preset(i) if presets.get(i).is_none_or(|p| p.template.is_none()) => {
                presets.iter().position(|p| p.template.is_some()).map_or(chosen, Chosen::Preset)
            }
            chosen => chosen,
        };
        State {
            tab: Tab::Settings,
            selected: None,
            menu: None,
            shown,
            slot: None,
            tokens: None,
            deleting: None,
            dragging: None,
            pointer: Point::ORIGIN,
            over: None,
        }
    }

    /// What `event` does to the window itself (what it does to the export and the templates is
    /// the program's).
    pub fn update(&mut self, event: &Event) {
        // (Whatever is done closes the list of options open.)
        if !matches!(event, Event::Pointer(_) | Event::Over(_)) {
            self.menu = None;
        }
        match event {
            Event::Tab(tab) => {
                self.tab = *tab;
                self.tokens = None;
            }
            Event::Select(row) => self.selected = *row,
            Event::Menu(menu) => self.menu = *menu,
            Event::Show(chosen) => {
                self.shown = *chosen;
                self.slot = None;
                self.tokens = None;
                self.deleting = None;
            }
            Event::Slot(slot) => {
                self.slot = *slot;
                self.tokens = None;
            }
            Event::Tokens(file) => self.tokens = *file,
            Event::Deleting(asked) => self.deleting = *asked,
            Event::Drag(dragged) => {
                self.dragging = Some(dragged.clone());
                self.over = None;
            }
            Event::Pointer(at) => self.pointer = *at,
            Event::Over(slot) => self.over = *slot,
            Event::DragEnd => {
                self.dragging = None;
                self.over = None;
            }
            Event::Edit(_, Edit::Map { file, part, .. }) => {
                // (A map dropped on a channel chooses it.)
                self.slot = Some((*file, *part));
                self.tokens = None;
            }
            Event::Edit(_, Edit::RemoveFile(_) | Edit::AddFile { .. } | Edit::MoveFile { .. }) | Event::Reset(_) => {
                self.slot = None;
                self.tokens = None;
            }
            Event::Edit(_, Edit::FileName { .. }) => self.tokens = None,
            _ => {}
        }
    }
}

/// What is done in the window.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    // The window's own (see `State::update`).
    Tab(Tab),
    Select(Option<usize>),
    Menu(Option<Menu>),
    Show(Chosen),
    Slot(Option<(usize, usize)>),
    Tokens(Option<usize>),
    Deleting(Option<usize>),
    Drag(Dragged),
    Pointer(Point),
    Over(Option<(usize, usize)>),
    DragEnd,
    // What is exported: a row checked (a project: all of it), or all.
    Check(usize, bool),
    IncludeAll,
    /// A setting, of the global settings (`None`) or of a row's own; a row's own no more.
    Set(Option<usize>, Setting),
    Unset(usize, Which),
    /// Choose the folder (the program shows a dialog of the system), and show it.
    Browse,
    OpenFolder,
    // The templates: of the user's, new, a copy of another, deleted; any changed; a preset of
    // the program's as it was made again; their folder shown.
    New,
    Duplicate(Chosen),
    Delete(usize),
    Edit(Chosen, Edit),
    Reset(usize),
    OpenTemplates,
    /// The settings kept with the projects; all as they are by default.
    SaveSettings,
    ResetAll,
    Export,
    Cancel,
}

/// A file the export writes, as LIST OF EXPORTS lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    pub name: String,
    /// What it is made of, said.
    pub said: String,
    /// Its size, kind and depth ("1024 × 1024", "png", "8 bits").
    pub size: String,
    pub kind: String,
    pub depth: String,
}

/// A map a template may be made of, as the input maps list it.
#[derive(Clone, Debug, PartialEq)]
pub struct InputMap {
    pub map: MapId,
    pub name: String,
    /// What it is listed under (OpenPBR's parts: Base, Specular…).
    pub group: String,
}

/// The colors of the window's surfaces and text, darkest first.
#[derive(Clone, Copy, Debug)]
pub struct Colors {
    /// The bar of the tabs and the footer; the strip over a list.
    pub header: Color,
    pub strip: Color,
    /// A list beside the page, sunk; the page; a row raised on it, and its edge; a menu.
    pub sunken: Color,
    pub page: Color,
    pub raised: Color,
    pub edge: Color,
    pub menu: Color,
    /// A field sunk in; a row under the mouse; the row chosen; a line between parts.
    pub field: Color,
    pub hover: Color,
    pub chosen: Color,
    pub line: Color,
    /// Text: as any, headings and what is chosen, what is said of it, what is only hinted.
    pub text: Color,
    pub bright: Color,
    pub dim: Color,
    pub faint: Color,
    pub warning: Color,
    /// The maps made of others (Painter's converted maps).
    pub converted: Color,
}

/// How the window looks: the program's fonts, sizes, colors and widgets.
#[derive(Clone, Copy)]
pub struct Look {
    pub font: Font,
    pub bold: Font,
    pub mono: Font,
    /// Text, small text, and the captions of lists (in capitals, as the program's panes).
    pub text_size: f32,
    pub small_size: f32,
    pub caption_size: f32,
    pub colors: Colors,
    /// An option of a segmented control, the one chosen where `chosen`, and the control.
    pub segment: fn(&Theme, button::Status, bool) -> button::Style,
    pub segments: fn(&Theme) -> container::Style,
    /// Buttons: as any, without chrome, and the default action (Export).
    pub button: fn(&Theme, button::Status) -> button::Style,
    pub flat_button: fn(&Theme, button::Status) -> button::Style,
    pub primary_button: fn(&Theme, button::Status) -> button::Style,
    pub text_input: fn(&Theme, text_input::Status) -> text_input::Style,
    pub pick_list: fn(&Theme, pick_list::Status) -> pick_list::Style,
}

/// What the window shows.
pub struct Content<'a, M> {
    /// The program's word for what it exports, as the window says it: "graph" (a texture
    /// designer exports a graph's outputs), "texture set" (a texture painter).
    pub thing: &'a str,
    /// The program's presets, in its order: each the template it is, or its maps as they are.
    pub presets: Vec<Preset<'a>>,
    /// The templates of the user's, the folder they are kept in, and what could not be read.
    pub mine: &'a [Template],
    pub templates_folder: Option<String>,
    pub problems: &'a [String],
    /// The projects open and what each exports, and the settings of all.
    pub exported: Vec<Exported>,
    pub global: Settings,
    /// The folder the files go to, as the program shows a path; whether one is chosen.
    pub folder: Element<'a, M>,
    pub folder_chosen: bool,
    /// The files written, under what each is of, before they are.
    pub files: Vec<(String, Vec<Listed>)>,
    /// The parts a name may have (`$(graph)`…), and what each stands for here (of the first of
    /// what is exported).
    pub tokens: &'a [Token],
    pub values: Vec<(String, String)>,
    /// The maps a template may be made of; the maps of no usage the program has, each by its
    /// name; and the maps it has.
    pub inputs: Vec<InputMap>,
    pub custom: Vec<(MapId, String)>,
    pub has: Vec<MapId>,
}

impl Chosen {
    /// The template it is, of the program's `presets` or of the user's (none: the maps as they
    /// are).
    pub fn template<'t>(self, presets: &[Preset<'t>], mine: &'t [Template]) -> Option<&'t Template> {
        match self {
            Chosen::Preset(i) => presets.get(i).and_then(|p| p.template),
            Chosen::Mine(i) => mine.get(i),
        }
    }

    /// Its name, as a list says it.
    pub fn name(self, presets: &[Preset], mine: &[Template]) -> String {
        match self {
            Chosen::Preset(i) => presets.get(i).map_or_else(String::new, |p| p.name.to_string()),
            Chosen::Mine(i) => mine.get(i).map_or_else(String::new, |t| format!("{} (yours)", t.name)),
        }
    }
}

/// What a format of the files is, said: "png 8 bits", as in the project.
pub fn format_said(format: FileFormat) -> String {
    match format {
        FileFormat::AsMade => "As in the project".into(),
        FileFormat::Of(format) => format.name(),
    }
}

/// What a size is, said, of what is made `made` (none: of several sizes).
pub fn size_said(size: Size, made: Option<[u32; 2]>) -> String {
    let of = |[w, h]: [u32; 2]| if w == h { format!("{w}") } else { format!("{w} × {h}") };
    match (size, made) {
        (Size::AsMade, Some(made)) => format!("As in the project ({})", of(made)),
        (Size::AsMade, None) => "As in the project".into(),
        (Size::Side(side), Some(made)) => of(Size::Side(side).of(made)),
        (Size::Side(side), None) => format!("{side}"),
    }
}

fn normals_said(normals: Normals) -> &'static str {
    match normals {
        Normals::OpenGL => "OpenGL (Y+)",
        Normals::DirectX => "DirectX (Y−)",
    }
}

/// A caption in capitals, as the program's panes have.
fn caption<'a>(look: &Look, title: &str) -> iced::widget::Text<'a> {
    text(title.to_uppercase()).size(look.caption_size).font(look.bold).color(look.colors.dim)
}

/// An option of a list: what it says, what choosing it does, whether it is the one chosen.
pub(crate) struct Offer<M> {
    pub label: String,
    pub message: M,
    pub chosen: bool,
}

impl<M> Offer<M> {
    pub fn new(label: impl Into<String>, message: M, chosen: bool) -> Offer<M> {
        Offer { label: label.into(), message, chosen }
    }
}

/// A list to choose from, as the program's: a field that says what is chosen (`as_else`: as
/// something else says, dim), and pressed, its options below it, in groups, a line between
/// them: the first, what the project makes, apart from the others, at the top.
#[allow(clippy::too_many_arguments)]
pub(crate) fn choice<'a, M: Clone + 'a>(
    look: &Look,
    state: &State,
    menu: Menu,
    said: String,
    as_else: bool,
    width: f32,
    groups: Vec<Vec<Offer<M>>>,
    say: impl Fn(Event) -> M + Copy + 'a,
) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    let open = state.menu == Some(menu);
    let field = button(
        row![
            text(said).size(look.small_size).color(if as_else { c.dim } else { c.text }).wrapping(text::Wrapping::None),
            space().width(Length::Fill),
            text("▾").size(look.small_size).color(c.dim),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .width(Length::Fixed(width))
    .padding([4, 8])
    .clip(true)
    .style(style::field(&look, open))
    .on_press(say(Event::Menu(if open { None } else { Some(menu) })));
    let mut items: Column<'a, M> = column![].spacing(1);
    for (g, group) in groups.into_iter().filter(|g| !g.is_empty()).enumerate() {
        if g > 0 {
            items = items.push(container(container(space()).width(Length::Fill).height(1).style(style::line(&look))).padding([3, 4]));
        }
        for offer in group {
            let mark = text(if offer.chosen { "✓" } else { "" }).size(look.small_size).color(c.bright).width(Length::Fixed(14.0));
            items = items.push(
                button(row![mark, text(offer.label).size(look.small_size).wrapping(text::Wrapping::None)].align_y(Alignment::Center))
                    .width(Length::Fill)
                    .padding([4, 8])
                    .style(style::option(&look, offer.chosen))
                    .on_press(offer.message),
            );
        }
    }
    let list = container(scroll_box(items).height(Length::Shrink)).padding(4).width(Length::Fixed(width.max(180.0))).max_height(380).style(style::menu(&look));
    dropdown(field, list, open).on_dismiss(say(Event::Menu(None))).into()
}

/// A box to check, as the program's: a mark in a field.
fn check<'a, M: Clone + 'a>(look: &Look, checked: bool, on: M) -> Element<'a, M> {
    let look = *look;
    let mark = text(if checked { "✓" } else { "" }).size(look.small_size).color(look.colors.bright);
    button(container(mark).center_x(14).center_y(14)).padding(0).style(style::check(&look, checked)).on_press(on).into()
}

/// The window, its frame and all: `say` makes what is done a message of the program's. The
/// program puts it over its own (it fills what it is given, up to a size).
pub fn view<'a, M: Clone + 'a>(mut content: Content<'a, M>, state: &'a State, look: &Look, say: impl Fn(Event) -> M + Copy + 'a) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    // (The program's element of the folder, shown in SETTINGS.)
    let folder = std::mem::replace(&mut content.folder, text("").into());
    let any = content.exported.iter().any(|e| e.checked && e.depth > 0);
    let mut tabs = row![].spacing(0).align_y(Alignment::Center);
    for tab in Tab::ALL {
        let chosen = tab == state.tab;
        // (The tab shown has a light line on its top, and is of the page's color; the list of
        // exports is of nothing where nothing is exported.)
        let top = container(space())
            .width(Length::Fill)
            .height(2)
            .style(move |_: &Theme| container::Style { background: chosen.then_some(c.bright.into()), ..Default::default() });
        let can = tab != Tab::Files || any;
        let label = button(text(tab.name()).size(look.small_size).font(look.bold))
            .padding([8, 14])
            .style(style::tab(&look, chosen))
            .on_press_maybe(can.then(|| say(Event::Tab(tab))));
        tabs = tabs.push(column![top, label].width(Length::Shrink));
    }
    let tools: Element<'a, M> = match state.tab {
        Tab::Settings => {
            let all = content.exported.iter().all(|e| e.checked);
            button(text("Include all").size(look.small_size).center())
                .width(Length::Fixed(150.0))
                .padding([4, 10])
                .style(look.button)
                .on_press_maybe((!all).then(|| say(Event::IncludeAll)))
                .into()
        }
        Tab::Files => button(text("Open output directory").size(look.small_size))
            .padding([4, 10])
            .style(look.button)
            .on_press_maybe(content.folder_chosen.then(|| say(Event::OpenFolder)))
            .into(),
        Tab::Templates => space().into(),
    };
    let head = container(row![tabs, space().width(Length::Fill), tools].align_y(Alignment::Center).padding(iced::Padding {
        top: 6.0,
        right: 12.0,
        bottom: 0.0,
        left: 8.0,
    }))
    .width(Length::Fill)
    .style(style::bar(&look));

    let count: usize = content.files.iter().map(|(_, files)| files.len()).sum();
    let ready = content.folder_chosen && count > 0;
    let page: Element<'a, M> = match state.tab {
        Tab::Settings => settings(&content, folder, state, &look, say),
        Tab::Templates => editor::view(&content, state, &look, say),
        Tab::Files => files(&content, &look),
    };
    let said = match (any, count, content.folder_chosen) {
        (false, ..) => text(format!("No {} is checked for export", content.thing)).color(c.warning),
        (_, 0, _) => text(format!("Nothing to export: the {}s checked have none of the maps the templates write", content.thing)).color(c.warning),
        (_, _, false) => text(format!("{count} texture(s) selected for export: choose the output directory")).color(c.warning),
        (_, _, true) => text(format!("{count} texture(s) selected for export")).color(c.dim),
    };
    let wide = |label: &'static str, style: fn(&Theme, button::Status) -> button::Style, on: Option<M>| {
        button(text(label).size(look.text_size).center()).width(Length::Fixed(130.0)).padding([5, 12]).style(style).on_press_maybe(on)
    };
    let footer = container(
        row![
            wide("Reset all", look.button, Some(say(Event::ResetAll))),
            space().width(Length::Fill),
            said.size(look.small_size),
            space().width(12),
            wide("Cancel", look.button, Some(say(Event::Cancel))),
            wide("Save settings", look.button, Some(say(Event::SaveSettings))),
            wide("Export", look.primary_button, ready.then(|| say(Event::Export))),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([8, 12])
    .width(Length::Fill)
    .style(style::bar(&look));
    let line = move || container(space()).width(Length::Fill).height(1).style(style::line(&look));
    let window: Element<'a, M> =
        column![head, line(), container(page).width(Length::Fill).height(Length::Fill).style(style::page(&look)), line(), footer].into();
    // A map dragged: its label follows the pointer; let go anywhere, the drag ends (on a channel,
    // the channel took it first).
    match &state.dragging {
        Some(dragged) => {
            let label = container(text(dragged.label.clone()).size(look.small_size)).padding([3, 8]).style(style::dragged(&look));
            let at = Point::new(state.pointer.x + 14.0, state.pointer.y + 10.0);
            let layer = stack![window, pin(label).position(at)];
            mouse_area(layer).on_move(move |p| say(Event::Pointer(p))).on_release(say(Event::DragEnd)).interaction(iced::mouse::Interaction::Grabbing).into()
        }
        None => window,
    }
}

/// A row of SETTINGS: what it is on the left, the control on the right.
fn setting<'a, M: 'a>(look: &Look, label: impl text::IntoFragment<'a>, control: impl Into<Element<'a, M>>) -> Element<'a, M> {
    row![text(label).size(look.text_size).color(look.colors.dim).width(Length::Fixed(220.0)), container(control).width(Length::Fill).max_width(620)]
        .align_y(Alignment::Center)
        .height(Length::Fixed(34.0))
        .into()
}

/// SETTINGS: what is exported, checked in the list of the projects open and what each exports;
/// the global settings, or those of the row chosen.
fn settings<'a, M: Clone + 'a>(
    content: &Content<'a, M>,
    folder: Element<'a, M>,
    state: &'a State,
    look: &Look,
    say: impl Fn(Event) -> M + Copy + 'a,
) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    // What is exported, as Painter lists its texture sets: the global settings first.
    let selected = state.selected.filter(|&i| content.exported.get(i).is_some_and(|e| e.depth > 0));
    let global = button(text("Global settings").size(look.text_size).font(look.bold).color(if selected.is_none() { c.bright } else { c.text }))
        .width(Length::Fill)
        .padding([5, 10])
        .style(style::item(&look, selected.is_none()))
        .on_press(say(Event::Select(None)));
    let mut list = column![global].spacing(1).padding(4);
    for (i, row) in content.exported.iter().enumerate() {
        let heading = row.depth == 0;
        let name =
            text(row.name.clone()).size(look.text_size).font(if heading { look.bold } else { look.font }).color(if row.checked { c.text } else { c.dim });
        let line = row![space().width(f32::from(row.depth) * 16.0), check(&look, row.checked, say(Event::Check(i, !row.checked))), name]
            .spacing(8)
            .align_y(Alignment::Center);
        let chosen = selected == Some(i);
        list = list.push(
            button(line).width(Length::Fill).padding([4, 8]).style(style::item(&look, chosen)).on_press_maybe((!heading).then(|| say(Event::Select(Some(i))))),
        );
    }
    let side = container(column![
        container(caption(&look, "Export")).padding([7, 10]).width(Length::Fill).style(style::strip(&look)),
        scroll_box(list).height(Length::Fill)
    ])
    .width(Length::Fixed(260.0))
    .height(Length::Fill)
    .style(style::sunken(&look));

    let page = match selected.and_then(|i| content.exported.get(i).map(|row| (i, row))) {
        None => global_settings(content, folder, state, &look, say),
        Some((i, row)) => own_settings(content, i, row, state, &look, say),
    };
    row![side, scroll_box(container(page).padding([14, 20])).height(Length::Fill)].spacing(0).padding(8).into()
}

/// The templates offered: the program's maps as they are (as in the project), apart and first;
/// then `between` (as the global settings, of a row); the presets; the user's.
fn template_offers<'a, M: Clone + 'a>(
    content: &Content<'a, M>,
    chosen: Option<Chosen>,
    between: Vec<Offer<M>>,
    to: impl Fn(Chosen) -> M,
) -> Vec<Vec<Offer<M>>> {
    let offer = |i: usize, p: &Preset| Offer::new(p.name, to(Chosen::Preset(i)), chosen == Some(Chosen::Preset(i)));
    let raw = content.presets.iter().enumerate().filter(|(_, p)| p.template.is_none()).map(|(i, p)| offer(i, p)).collect();
    let presets = content.presets.iter().enumerate().filter(|(_, p)| p.template.is_some()).map(|(i, p)| offer(i, p)).collect();
    let mine =
        content.mine.iter().enumerate().map(|(i, t)| Offer::new(format!("{} (yours)", t.name), to(Chosen::Mine(i)), chosen == Some(Chosen::Mine(i)))).collect();
    vec![raw, between, presets, mine]
}

/// The kinds of file offered: as in the project, apart and first; then `between`; the kinds.
fn kind_offers<M: Clone>(chosen: Option<FileFormat>, between: Vec<Offer<M>>, to: impl Fn(FileFormat) -> M) -> Vec<Vec<Offer<M>>> {
    let kind = chosen.and_then(|f| if let FileFormat::Of(f) = f { Some(f.file_type()) } else { None });
    let kinds = FileType::ALL.into_iter().map(|k| Offer::new(k.name(), to(FileFormat::Of(k.formats()[0])), kind == Some(k))).collect();
    vec![vec![Offer::new("As in the project", to(FileFormat::AsMade), chosen == Some(FileFormat::AsMade))], between, kinds]
}

/// The sizes offered, of what is made `made`: as in the project, apart and first; then
/// `between`; the sizes.
fn size_offers<M: Clone>(chosen: Option<Size>, made: Option<[u32; 2]>, between: Vec<Offer<M>>, to: impl Fn(Size) -> M) -> Vec<Vec<Offer<M>>> {
    let sides = Size::SIDES.into_iter().map(|s| Offer::new(size_said(Size::Side(s), made), to(Size::Side(s)), chosen == Some(Size::Side(s)))).collect();
    vec![vec![Offer::new(size_said(Size::AsMade, made), to(Size::AsMade), chosen == Some(Size::AsMade))], between, sides]
}

/// The depth of a kind of file: a list where it has several.
fn depth_choice<'a, M: Clone + 'a>(
    look: &Look,
    state: &State,
    format: Format,
    to: impl Fn(Format) -> M,
    say: impl Fn(Event) -> M + Copy + 'a,
) -> Element<'a, M> {
    let formats = format.file_type().formats();
    if formats.len() == 1 {
        return container(text(format.depth()).size(look.small_size).color(look.colors.dim)).padding([4, 8]).into();
    }
    let offers = formats.iter().map(|f| Offer::new(f.depth(), to(*f), *f == format)).collect();
    choice(look, state, Menu::Depth, format.depth().into(), false, 200.0, vec![offers], say)
}

/// The global settings: where the files go, with which template, as what, how large.
fn global_settings<'a, M: Clone + 'a>(
    content: &Content<'a, M>,
    folder: Element<'a, M>,
    state: &'a State,
    look: &Look,
    say: impl Fn(Event) -> M + Copy + 'a,
) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    let global = content.global;
    let set = move |s: Setting| say(Event::Set(None, s));
    let template = choice(
        &look,
        state,
        Menu::Template,
        global.template.name(&content.presets, content.mine),
        false,
        420.0,
        template_offers(content, Some(global.template), Vec::new(), move |t| set(Setting::Template(t))),
        say,
    );
    let kind = choice(
        &look,
        state,
        Menu::FileType,
        kind_said(global.format),
        false,
        160.0,
        kind_offers(Some(global.format), Vec::new(), move |f| set(Setting::Format(f))),
        say,
    );
    let depth: Element<'a, M> = match global.format {
        FileFormat::Of(format) => depth_choice(&look, state, format, move |f| set(Setting::Format(FileFormat::Of(f))), say),
        FileFormat::AsMade => text("Each map as its Output says").size(look.small_size).color(c.faint).into(),
    };
    let sizes = content.exported.iter().filter(|e| e.depth > 0).map(|e| e.size).collect::<Vec<_>>();
    let made = sizes.first().copied().filter(|first| sizes.iter().all(|s| s == first));
    let size = choice(
        &look,
        state,
        Menu::Size,
        size_said(global.size, made),
        false,
        260.0,
        size_offers(Some(global.size), made, Vec::new(), move |s| set(Setting::Size(s))),
        say,
    );
    let mut rows = column![text("General Export Parameters").size(look.text_size).font(look.bold).color(c.bright), space().height(6)].spacing(2);
    rows = rows.push(setting(
        &look,
        "Output directory",
        row![
            container(folder).padding([5, 8]).width(Length::Fill).style(style::well(&look)).clip(true),
            button(text("Browse…").size(look.text_size)).padding([4, 10]).style(look.button).on_press(say(Event::Browse)),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    ));
    rows = rows.push(setting(&look, "Output template", template));
    rows = rows.push(setting(&look, "File type", row![kind, depth].spacing(6).align_y(Alignment::Center)));
    rows = rows.push(setting(&look, "Size", size));
    let normals = match global.template.template(&content.presets, content.mine) {
        Some(t) => format!(
            "{} writes normal maps of {}; a {}'s that point the other way have their green turned over.",
            t.name,
            normals_said(t.normals),
            content.thing
        ),
        None => format!("Raw Outputs: each Output of a {} is written with its own name, channels and format.", content.thing),
    };
    rows = rows.push(space().height(6)).push(text(normals).size(look.small_size).color(c.faint));
    rows = rows.push(
        text(format!("What a {} says of its own (choose it on the left) is written so; the rest, as these say.", content.thing))
            .size(look.small_size)
            .color(c.faint),
    );
    rows.into()
}

/// What a kind of file is, said, in its list.
fn kind_said(format: FileFormat) -> String {
    match format {
        FileFormat::AsMade => "As in the project".into(),
        FileFormat::Of(format) => format.file_type().name().into(),
    }
}

/// The settings of one of what is exported: each as the global settings say, or its own.
fn own_settings<'a, M: Clone + 'a>(
    content: &Content<'a, M>,
    i: usize,
    row: &Exported,
    state: &'a State,
    look: &Look,
    say: impl Fn(Event) -> M + Copy + 'a,
) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    let global = content.global;
    let own = row.own;
    let mine = global.with(&own);
    let set = move |s: Setting| say(Event::Set(Some(i), s));
    let unset = move |w: Which| say(Event::Unset(i, w));
    let as_global = |said: String, which: Which, is: bool| vec![Offer::new(format!("As global settings ({said})"), unset(which), is)];
    let template = choice(
        &look,
        state,
        Menu::Template,
        match own.template {
            Some(t) => t.name(&content.presets, content.mine),
            None => format!("As global settings ({})", global.template.name(&content.presets, content.mine)),
        },
        own.template.is_none(),
        420.0,
        template_offers(
            content,
            own.template,
            as_global(global.template.name(&content.presets, content.mine), Which::Template, own.template.is_none()),
            move |t| set(Setting::Template(t)),
        ),
        say,
    );
    let kind = choice(
        &look,
        state,
        Menu::FileType,
        match own.format {
            Some(f) => kind_said(f),
            None => format!("As global settings ({})", format_said(global.format)),
        },
        own.format.is_none(),
        240.0,
        kind_offers(own.format, as_global(format_said(global.format), Which::Format, own.format.is_none()), move |f| set(Setting::Format(f))),
        say,
    );
    let depth: Element<'a, M> = match (own.format, mine.format) {
        (Some(_), FileFormat::Of(format)) => depth_choice(&look, state, format, move |f| set(Setting::Format(FileFormat::Of(f))), say),
        (Some(_), FileFormat::AsMade) => text("Each map as its Output says").size(look.small_size).color(c.faint).into(),
        (None, _) => space().into(),
    };
    let size = choice(
        &look,
        state,
        Menu::Size,
        match own.size {
            Some(s) => size_said(s, Some(row.size)),
            None => format!("As global settings ({})", size_said(global.size, Some(row.size))),
        },
        own.size.is_none(),
        300.0,
        size_offers(own.size, Some(row.size), as_global(size_said(global.size, Some(row.size)), Which::Size, own.size.is_none()), move |s| {
            set(Setting::Size(s))
        }),
        say,
    );
    let normals = choice(
        &look,
        state,
        Menu::Normals,
        match own.normals {
            Some(n) => normals_said(n).into(),
            None => format!("As in the project ({})", normals_said(row.normals)),
        },
        own.normals.is_none(),
        300.0,
        vec![
            vec![Offer::new(format!("As in the project ({})", normals_said(row.normals)), unset(Which::Normals), own.normals.is_none())],
            [Normals::OpenGL, Normals::DirectX].into_iter().map(|n| Offer::new(normals_said(n), set(Setting::Normals(n)), own.normals == Some(n))).collect(),
        ],
        say,
    );
    let [w, h] = row.size;
    let mut rows = column![
        row![
            text(row.name.clone()).size(look.text_size).font(look.bold).color(c.bright),
            text(format!("{w} × {h} in the project")).size(look.small_size).color(c.dim)
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        text(format!("What this {} says of its own; what says \"as global settings\" follows them.", content.thing)).size(look.small_size).color(c.faint),
        space().height(6),
    ]
    .spacing(2);
    rows = rows.push(setting(&look, "Output template", template));
    rows = rows.push(setting(&look, "File type", row![kind, depth].spacing(6).align_y(Alignment::Center)));
    rows = rows.push(setting(&look, "Size", size));
    rows = rows.push(setting(&look, "Its normal maps", normals));
    let written = match mine.template.template(&content.presets, content.mine) {
        Some(t) => format!(
            "Its normal maps are made as {}; {} writes them as {}{}.",
            normals_said(own.normals.unwrap_or(row.normals)),
            t.name,
            normals_said(t.normals),
            if own.normals.unwrap_or(row.normals) == t.normals { "" } else { ", their green turned over" }
        ),
        None => "Raw Outputs: each of its Outputs is written with its own name, channels and format.".into(),
    };
    rows = rows.push(space().height(6)).push(text(written).size(look.small_size).color(c.faint));
    rows.into()
}

/// LIST OF EXPORTS: the files, under what they are of, before they are written.
fn files<'a, M: Clone + 'a>(content: &Content<'a, M>, look: &Look) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    let cell =
        |s: String, width: Length, color: Color, font: Font| text(s).size(look.text_size).color(color).font(font).width(width).wrapping(text::Wrapping::None);
    let mut list: Column<'a, M> = column![].spacing(2);
    for (of, files) in &content.files {
        list = list.push(row![text("⌄").size(look.text_size).color(c.dim), text(of.clone()).size(look.text_size).color(c.bright)].spacing(6));
        for file in files {
            list = list.push(
                row![
                    space().width(22),
                    cell(file.name.clone(), Length::Fixed(380.0), c.text, look.font),
                    cell(file.size.clone(), Length::Fixed(110.0), c.text, look.font),
                    cell(file.kind.clone(), Length::Fixed(50.0), c.text, look.font),
                    cell(file.depth.clone(), Length::Fixed(120.0), c.text, look.font),
                    cell(file.said.clone(), Length::Fill, c.dim, look.font),
                ]
                .spacing(10)
                .height(Length::Fixed(22.0))
                .align_y(Alignment::Center),
            );
        }
        if files.is_empty() {
            list = list.push(row![
                space().width(22),
                text(format!("None: the {} has none of the maps its template writes.", content.thing)).size(look.text_size).color(c.warning)
            ]);
        }
        list = list.push(space().height(6));
    }
    container(scroll_box(container(list).padding([10, 12])).height(Length::Fill)).padding(8).into()
}

impl<M> Content<'_, M> {
    /// The depth the global settings write a file at that says no format of its own, said.
    fn global_depth(&self) -> String {
        match self.global.format {
            FileFormat::AsMade => "as in the project".into(),
            FileFormat::Of(format) => format.depth().into(),
        }
    }

    /// The extension of such a file, where the global settings say one.
    fn global_extension(&self) -> Option<&'static str> {
        match self.global.format {
            FileFormat::AsMade => None,
            FileFormat::Of(format) => Some(format.extension()),
        }
    }
}
