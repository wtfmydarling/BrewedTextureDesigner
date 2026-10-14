//! The window an export of textures is set up in, made to be the same in every program of a
//! set, as Substance Painter's export window is: SETTINGS (where, with which output template,
//! in what), OUTPUT TEMPLATES (the presets, the program's and the user's; the output maps of
//! the one shown, each a row of a name and channels; the input maps, dragged onto the channels),
//! LIST OF EXPORTS (the files before they are written). A program gives what it has (see
//! [`Content`]: its presets, the parts of a name it fills in, its maps, its word for what it
//! exports) and its look, keeps the window's [`State`], and takes what is done as [`Event`]s:
//! it writes the files, and keeps the templates of the user's (see
//! `texture_export::write_template`). It knows nothing of the program.

mod editor;
pub mod style;

use iced::widget::{Column, button, column, container, mouse_area, pick_list, pin, row, space, stack, text, text_input};
use iced::{Alignment, Color, Element, Font, Length, Point, Theme};
use scroll_box::scroll_box;
use texture_export::naming::Token;
use texture_export::{Edit, FileType, Format, MapId, Normals, Template};

/// Which template: one of the program's presets (its place in [`Content::presets`]), or one of
/// the user's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chosen {
    Preset(usize),
    Mine(usize),
}

/// A preset of the program's: its name, and the template it is; of none, the program's maps
/// as they are, each written with the name, channels and format it has.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preset<'a> {
    pub name: &'a str,
    pub template: Option<&'a Template>,
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

/// What the window keeps between frames (the program keeps it for it).
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub tab: Tab,
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
    /// The window as it opens: SETTINGS, the template written with shown in OUTPUT TEMPLATES (the
    /// first of `presets` that is one, where it is the maps as they are).
    pub fn new(chosen: Chosen, presets: &[Preset]) -> State {
        let shown = match chosen {
            Chosen::Preset(i) if presets.get(i).is_none_or(|p| p.template.is_none()) => {
                presets.iter().position(|p| p.template.is_some()).map_or(chosen, Chosen::Preset)
            }
            chosen => chosen,
        };
        State { tab: Tab::Settings, shown, slot: None, tokens: None, deleting: None, dragging: None, pointer: Point::ORIGIN, over: None }
    }

    /// What `event` does to the window itself (what it does to the export and the templates is
    /// the program's).
    pub fn update(&mut self, event: &Event) {
        match event {
            Event::Tab(tab) => {
                self.tab = *tab;
                self.tokens = None;
            }
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
            Event::Edit(_, Edit::RemoveFile(_) | Edit::AddFile { .. } | Edit::MoveFile { .. }) => {
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
    Show(Chosen),
    Slot(Option<(usize, usize)>),
    Tokens(Option<usize>),
    Deleting(Option<usize>),
    Drag(Dragged),
    Pointer(Point),
    Over(Option<(usize, usize)>),
    DragEnd,
    // The export's.
    ExportWith(Chosen),
    /// The graph's normal maps are DirectX's.
    DirectX(bool),
    Format(Format),
    /// Choose the folder (the program shows a dialog of the system), and show it.
    Browse,
    OpenFolder,
    // The templates of the user's: new, a copy of another, deleted, changed; their folder shown.
    New,
    Duplicate(Chosen),
    Delete(usize),
    Edit(usize, Edit),
    OpenTemplates,
    Export,
    Cancel,
}

/// A file the export writes, as LIST OF EXPORTS lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    pub name: String,
    /// What it is made of, said.
    pub said: String,
    /// Its kind and depth ("png", "8 bits").
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
    /// What is exported (of a graph: its name), and its size, said.
    pub name: String,
    /// The program's word for what it exports, as the window says it: "graph" (a texture
    /// designer exports a graph's outputs), "texture set" (a texture painter).
    pub thing: &'a str,
    /// The program's presets, in its order: each the template it is, or its maps as they are.
    pub presets: Vec<Preset<'a>>,
    pub size: String,
    /// The templates of the user's, the folder they are kept in, and what could not be read.
    pub mine: &'a [Template],
    pub templates_folder: Option<String>,
    pub problems: &'a [String],
    /// What the files are written with.
    pub export_with: Chosen,
    /// The program's normal maps are DirectX's (Y−), not OpenGL's.
    pub directx: bool,
    /// The format of a file whose template says none.
    pub format: Format,
    /// The folder the files go to, as the program shows a path; whether one is chosen.
    pub folder: Element<'a, M>,
    pub folder_chosen: bool,
    /// The files written, before they are.
    pub files: Vec<Listed>,
    /// The parts a name may have (`$(graph)`…), and what each stands for here.
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
}

/// What a list of templates offers: its name, as a list shows it.
#[derive(Clone, Debug, PartialEq)]
struct Offered(Chosen, String);

impl std::fmt::Display for Offered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

/// A caption in capitals, as the program's panes have.
fn caption<'a>(look: &Look, title: &str) -> iced::widget::Text<'a> {
    text(title.to_uppercase()).size(look.caption_size).font(look.bold).color(look.colors.dim)
}

/// The window, its frame and all: `say` makes what is done a message of the program's. The
/// program puts it over its own (it fills what it is given, up to a size).
pub fn view<'a, M: Clone + 'a>(mut content: Content<'a, M>, state: &'a State, look: &Look, say: impl Fn(Event) -> M + Copy + 'a) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    // (The program's element of the folder, shown in SETTINGS.)
    let folder = std::mem::replace(&mut content.folder, text("").into());
    let mut tabs = row![].spacing(0).align_y(Alignment::Center);
    for tab in Tab::ALL {
        let chosen = tab == state.tab;
        // (The tab shown has a light line on its top, and is of the page's color.)
        let top = container(space())
            .width(Length::Fill)
            .height(2)
            .style(move |_: &Theme| container::Style { background: chosen.then_some(c.bright.into()), ..Default::default() });
        let label =
            button(text(tab.name()).size(look.small_size).font(look.bold)).padding([8, 14]).style(style::tab(&look, chosen)).on_press(say(Event::Tab(tab)));
        tabs = tabs.push(column![top, label].width(Length::Shrink));
    }
    let tools: Element<'a, M> = match state.tab {
        Tab::Files => button(text("Open output directory").size(look.small_size))
            .padding([4, 10])
            .style(look.button)
            .on_press_maybe(content.folder_chosen.then(|| say(Event::OpenFolder)))
            .into(),
        _ => text(format!("Export  ·  {}", content.name)).size(look.small_size).color(c.dim).into(),
    };
    let head = container(row![tabs, space().width(Length::Fill), tools].align_y(Alignment::Center).padding(iced::Padding {
        top: 6.0,
        right: 12.0,
        bottom: 0.0,
        left: 8.0,
    }))
    .width(Length::Fill)
    .style(style::bar(&look));

    let count = content.files.len();
    let ready = content.folder_chosen && count > 0;
    let page: Element<'a, M> = match state.tab {
        Tab::Settings => settings(&content, folder, &look, say),
        Tab::Templates => editor::view(&content, state, &look, say),
        Tab::Files => files(&content, &look),
    };
    let said = match (count, content.folder_chosen) {
        (0, _) => text(format!("Nothing to export: the {} has none of the maps the template writes", content.thing)).color(c.warning),
        (_, false) => text(format!("{count} texture(s) selected for export: choose the output directory")).color(c.warning),
        (_, true) => text(format!("{count} texture(s) selected for export")).color(c.dim),
    };
    let footer = container(
        row![
            said.size(look.small_size),
            space().width(Length::Fill),
            button(text("Cancel").size(look.text_size).center()).width(Length::Fixed(110.0)).padding([5, 12]).style(look.button).on_press(say(Event::Cancel)),
            button(text("Export").size(look.text_size).center())
                .width(Length::Fixed(110.0))
                .padding([5, 12])
                .style(look.primary_button)
                .on_press_maybe(ready.then(|| say(Event::Export))),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([8, 12])
    .width(Length::Fill)
    .style(style::bar(&look));
    let line = move || {
        container(space()).width(Length::Fill).height(1).style(move |_: &Theme| container::Style { background: Some(c.line.into()), ..Default::default() })
    };
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

/// SETTINGS: what is exported (a graph, texture sets), and the general export parameters.
fn settings<'a, M: Clone + 'a>(content: &Content<'a, M>, folder: Element<'a, M>, look: &Look, say: impl Fn(Event) -> M + Copy + 'a) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    // What is exported, as Painter lists its texture sets.
    let item = |label: Element<'a, M>, chosen: bool| button(label).width(Length::Fill).padding([4, 10]).style(style::item(&look, chosen));
    let exported = row![
        container(text("✓").size(look.small_size).color(c.bright)).width(16).height(16).center_x(16).center_y(16).style(style::well(&look)),
        text(content.name.clone()).size(look.text_size),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let list = column![
        container(caption(&look, "Export")).padding([7, 10]).width(Length::Fill).style(style::strip(&look)),
        column![item(text("Global settings").size(look.text_size).font(look.bold).into(), true), item(exported.into(), false)].spacing(1).padding(4),
    ];
    let side = container(list).width(Length::Fixed(240.0)).height(Length::Fill).style(style::sunken(&look));

    // The template, the file's kind and depth, the normals, the size.
    let mut offered: Vec<Offered> = content.presets.iter().enumerate().map(|(i, p)| Offered(Chosen::Preset(i), p.name.into())).collect();
    offered.extend(content.mine.iter().enumerate().map(|(i, t)| Offered(Chosen::Mine(i), format!("{} (yours)", t.name))));
    let chosen = offered.iter().find(|o| o.0 == content.export_with).cloned();
    let template = pick_list(offered, chosen, move |o: Offered| say(Event::ExportWith(o.0)))
        .width(Length::Fill)
        .text_size(look.text_size)
        .padding([4, 8])
        .style(look.pick_list);
    let packs = content.export_with.template(&content.presets, content.mine).is_some();
    let kind = content.format.file_type();
    let kinds = pick_list(FileType::ALL, Some(kind), move |k: FileType| say(Event::Format(k.formats()[0])))
        .width(Length::Fixed(110.0))
        .text_size(look.text_size)
        .padding([4, 8])
        .style(look.pick_list);
    let depths = pick_list(kind.formats(), Some(content.format), move |f: Format| say(Event::Format(f)))
        .width(Length::Fixed(200.0))
        .text_size(look.text_size)
        .padding([4, 8])
        .style(look.pick_list);
    let normals = ["OpenGL (Y+)", "DirectX (Y−)"];
    let its_normals = pick_list(normals, Some(normals[usize::from(content.directx)]), move |n: &str| say(Event::DirectX(n.starts_with("DirectX"))))
        .width(Length::Fixed(200.0))
        .text_size(look.text_size)
        .padding([4, 8])
        .style(look.pick_list);
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
    if packs {
        rows = rows.push(setting(&look, "File type", row![kinds, depths].spacing(6)));
        rows = rows.push(setting(&look, format!("Normals of the {}", content.thing), its_normals));
    } else {
        rows = rows.push(setting(&look, "File type", text(format!("As each map of the {} says", content.thing)).size(look.text_size).color(c.dim)));
    }
    rows = rows.push(setting(
        &look,
        "Size",
        container(text(content.size.clone()).size(look.text_size).color(c.dim)).padding([5, 8]).width(Length::Fixed(200.0)).style(style::well(&look)),
    ));
    let note = if packs {
        "An output map whose template says its own file type is written in it. A normal map's green is turned over where the template writes it the other way."
            .to_string()
    } else {
        format!("Each map of the {} is written with its own name, channels and format.", content.thing)
    };
    rows = rows.push(space().height(6)).push(text(note).size(look.small_size).color(c.faint));
    let page = scroll_box(container(rows).padding([14, 20])).height(Length::Fill);
    row![side, page].spacing(0).padding(8).into()
}

/// LIST OF EXPORTS: the files, under what they are of, before they are written.
fn files<'a, M: Clone + 'a>(content: &Content<'a, M>, look: &Look) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    let cell =
        |s: String, width: Length, color: Color, font: Font| text(s).size(look.text_size).color(color).font(font).width(width).wrapping(text::Wrapping::None);
    let mut list: Column<'a, M> =
        column![row![text("⌄").size(look.text_size).color(c.dim), text(content.name.clone()).size(look.text_size).color(c.bright)].spacing(6)].spacing(2);
    for file in &content.files {
        list = list.push(
            row![
                space().width(22),
                cell(file.name.clone(), Length::Fixed(380.0), c.text, look.font),
                cell(content.size.clone(), Length::Fixed(110.0), c.text, look.font),
                cell(file.kind.clone(), Length::Fixed(50.0), c.text, look.font),
                cell(file.depth.clone(), Length::Fixed(120.0), c.text, look.font),
                cell(file.said.clone(), Length::Fill, c.dim, look.font),
            ]
            .spacing(10)
            .height(Length::Fixed(22.0))
            .align_y(Alignment::Center),
        );
    }
    if content.files.is_empty() {
        list = list.push(row![
            space().width(22),
            text(format!("None: the {} has none of the maps the template writes.", content.thing)).size(look.text_size).color(c.warning)
        ]);
    }
    container(scroll_box(container(list).padding([10, 12])).height(Length::Fill)).padding(8).into()
}
