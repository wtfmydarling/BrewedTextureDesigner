//! The window an export of textures is set up in, made to be the same in every program of a
//! set, as Substance Painter's is in essence: what is exported, where and in what (Settings),
//! the templates that say what files are written of which maps, the program's own and the
//! user's, made and changed there (Output Templates), and the files, listed before they are
//! written (List of Exports). A program gives what it has (see [`Content`]) and its look, keeps
//! the window's [`State`], and takes what is done as [`Event`]s: it writes the files, and keeps
//! the templates of the user's (see `texture_export::write_template`). It knows nothing of the
//! program.

mod editor;
pub mod style;

use iced::widget::{Column, button, column, container, row, space, text};
use iced::{Alignment, Color, Element, Font, Length, Padding, Theme};
use scroll_box::scroll_box;
use texture_export::naming::Token;
use texture_export::{Edit, Format, Preset, Template, Usage};

/// Which template: one of the program's (or its maps as they are), or one of the user's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chosen {
    Preset(Preset),
    Mine(usize),
}

/// The pages of the window.
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
            Tab::Settings => "Settings",
            Tab::Templates => "Output Templates",
            Tab::Files => "List of Exports",
        }
    }
}

/// What the window keeps between frames (the program keeps it for it).
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub tab: Tab,
    /// The template shown in Output Templates.
    pub shown: Chosen,
    /// The channel of a file chosen there (the file, its part): a map pressed goes to it.
    pub slot: Option<(usize, usize)>,
    /// The file whose menu of the parts of a name is open, and the one whose menu of formats is.
    pub tokens: Option<usize>,
    pub formats: Option<usize>,
    /// A template of the user's asked to be deleted, waiting for a yes.
    pub deleting: Option<usize>,
}

impl State {
    /// The window as it opens, on what is exported with: a template of the user's shown, or the
    /// Unreal Engine's.
    pub fn new(chosen: Chosen) -> State {
        let shown = match chosen {
            Chosen::Preset(Preset::Maps) => Chosen::Preset(Preset::Unreal),
            chosen => chosen,
        };
        State { tab: Tab::Settings, shown, slot: None, tokens: None, formats: None, deleting: None }
    }

    /// What `event` does to the window itself (what it does to the export and the templates is
    /// the program's).
    pub fn update(&mut self, event: &Event) {
        match event {
            Event::Tab(tab) => {
                self.tab = *tab;
                self.close_menus();
            }
            Event::Show(chosen) => {
                self.shown = *chosen;
                self.slot = None;
                self.deleting = None;
                self.close_menus();
            }
            Event::Slot(slot) => {
                self.slot = *slot;
                self.close_menus();
            }
            Event::Tokens(file) => {
                self.tokens = *file;
                self.formats = None;
            }
            Event::Formats(file) => {
                self.formats = *file;
                self.tokens = None;
            }
            Event::Deleting(asked) => self.deleting = *asked,
            Event::Edit(_, Edit::RemoveFile(_) | Edit::AddFile(_) | Edit::MoveFile { .. }) => {
                self.slot = None;
                self.close_menus();
            }
            Event::Edit(_, Edit::FileName { .. } | Edit::Format { .. }) => self.close_menus(),
            _ => {}
        }
    }

    fn close_menus(&mut self) {
        self.tokens = None;
        self.formats = None;
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
    Formats(Option<usize>),
    Deleting(Option<usize>),
    // The export's.
    ExportWith(Chosen),
    /// The graph's normal maps are DirectX's.
    DirectX(bool),
    Format(Format),
    /// Choose the folder (the program shows a dialog of the system), and show it.
    Browse,
    OpenFolder,
    // The templates of the user's.
    /// A template of one's own: new, a copy of another, deleted, changed.
    New,
    Duplicate(Chosen),
    Delete(usize),
    Edit(usize, Edit),
    /// Show the folder they are kept in.
    OpenTemplates,
    Export,
    Cancel,
}

/// A file the export writes, as the window lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    pub name: String,
    /// What it is made of, said.
    pub said: String,
    pub format: String,
}

/// The colors of the window's surfaces and text, darkest first.
#[derive(Clone, Copy, Debug)]
pub struct Colors {
    /// The bar of the tabs and the footer.
    pub header: Color,
    /// A side pane, sunk below the page; the page a tab shows; a card of a file over it; a menu.
    pub sunken: Color,
    pub page: Color,
    pub card: Color,
    pub card_edge: Color,
    pub menu: Color,
    /// A field, and the rows of a list (every other one lighter).
    pub field: Color,
    pub row: Color,
    pub row_alt: Color,
    /// Under the mouse; what is chosen; a line between parts.
    pub hover: Color,
    pub lit: Color,
    pub line: Color,
    /// Text: as any, headings and what is chosen, what is said of it, what is only hinted.
    pub text: Color,
    pub bright: Color,
    pub dim: Color,
    pub faint: Color,
    pub warning: Color,
    pub error: Color,
}

/// How the window looks: the program's fonts, colors and widgets.
#[derive(Clone, Copy)]
pub struct Look {
    /// The font of text, and its weights: of what is chosen, and of headings.
    pub font: Font,
    pub bold: Font,
    pub heavy: Font,
    /// The font of file names.
    pub mono: Font,
    /// The size of text, and of small text.
    pub text_size: f32,
    pub small_size: f32,
    pub colors: Colors,
    /// An option of a list, the one chosen where `chosen`.
    pub option: fn(&Theme, button::Status, bool) -> button::Style,
    /// An option of a segmented control, the one chosen where `chosen`, and the control.
    pub segment: fn(&Theme, button::Status, bool) -> button::Style,
    pub segments: fn(&Theme) -> container::Style,
    /// Buttons: as any, without chrome, and the default action (Export).
    pub button: fn(&Theme, button::Status) -> button::Style,
    pub flat_button: fn(&Theme, button::Status) -> button::Style,
    pub primary_button: fn(&Theme, button::Status) -> button::Style,
    pub text_input: fn(&Theme, iced::widget::text_input::Status) -> iced::widget::text_input::Style,
}

/// What the window shows.
pub struct Content<'a, M> {
    /// What is exported (of a graph: its name), and how large it is, said.
    pub name: String,
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
    /// The maps the program has.
    pub has: Vec<Usage>,
}

impl Chosen {
    /// The template it is (none: the maps as they are).
    pub fn template(self, mine: &[Template]) -> Option<&Template> {
        match self {
            Chosen::Preset(preset) => preset.template(),
            Chosen::Mine(i) => mine.get(i),
        }
    }

    /// Its name and what it is said to be.
    pub fn said(self, mine: &[Template]) -> (String, String) {
        match self {
            Chosen::Preset(preset) => (preset.name().into(), preset.about().into()),
            Chosen::Mine(i) => mine.get(i).map_or_else(Default::default, |t| (t.name.clone(), t.about.clone())),
        }
    }
}

/// The window, its frame and all: `say` makes what is done a message of the program's. The
/// program puts it over its own (it fills what it is given, up to a size).
pub fn view<'a, M: Clone + 'a>(mut content: Content<'a, M>, state: &'a State, look: &Look, say: impl Fn(Event) -> M + Copy + 'a) -> Element<'a, M> {
    let look = *look;
    // (The program's element of the folder, shown on the settings page.)
    let folder = std::mem::replace(&mut content.folder, text("").into());
    let c = look.colors;
    // The head: what is exported, and the tabs.
    let mut tabs = row![].spacing(4).align_y(Alignment::End);
    for tab in Tab::ALL {
        let chosen = tab == state.tab;
        let font = if chosen { look.bold } else { look.font };
        tabs = tabs
            .push(button(text(tab.name()).size(look.text_size).font(font)).padding([8, 16]).style(style::tab(&look, chosen)).on_press(say(Event::Tab(tab))));
    }
    let title = text(format!("Export {}", content.name)).size(16).font(look.heavy).color(c.bright);
    let head = container(column![title, tabs].spacing(10))
        .padding(Padding { top: 14.0, right: 18.0, bottom: 0.0, left: 18.0 })
        .width(Length::Fill)
        .style(style::bar(&look));

    let count = content.files.len();
    let ready = content.folder_chosen && count > 0;
    let page: Element<'a, M> = match state.tab {
        Tab::Settings => settings(&content, folder, &look, say),
        Tab::Templates => editor::view(&content, state, &look, say),
        Tab::Files => files(&content, &look, say),
    };
    let said = match (count, content.folder_chosen) {
        (0, _) => text("Nothing to export: the graph has no Output of what this writes (give Outputs their Usage)").color(c.warning),
        (_, false) => text(format!("{count} file(s) to export: choose the folder they go to")).color(c.warning),
        (1, true) => text("1 file will be exported").color(c.dim),
        (_, true) => text(format!("{count} files will be exported")).color(c.dim),
    };
    let footer = container(
        row![
            said.size(look.small_size),
            space().width(Length::Fill),
            button(text("Cancel").size(look.text_size)).padding([6, 16]).style(look.button).on_press(say(Event::Cancel)),
            button(text("Export").size(look.text_size)).padding([6, 22]).style(look.primary_button).on_press_maybe(ready.then(|| say(Event::Export))),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding([10, 18])
    .width(Length::Fill)
    .style(style::bar(&look));
    let line = move || {
        container(space()).width(Length::Fill).height(1).style(move |_: &Theme| container::Style { background: Some(c.line.into()), ..Default::default() })
    };
    column![head, line(), container(page).width(Length::Fill).height(Length::Fill).style(style::page(&look)), line(), footer].into()
}

/// A heading of a part of a page.
fn heading<'a>(look: &Look, title: &'a str) -> iced::widget::Text<'a> {
    text(title).size(look.text_size).font(look.bold).color(look.colors.bright)
}

/// A row of settings: what it is on the left, the control on the right, every other one lighter.
fn setting<'a, M: 'a>(look: &Look, even: bool, label: &'a str, control: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(
        row![text(label).size(look.text_size).color(look.colors.dim).width(Length::Fixed(150.0)), container(control).width(Length::Fill)]
            .align_y(Alignment::Center),
    )
    .padding([10, 14])
    .width(Length::Fill)
    .style(style::row(look, even))
    .into()
}

/// A segmented control of `options` (each a name, what it says, and whether it is chosen).
fn segmented<'a, M: Clone + 'a>(look: &Look, options: Vec<(String, M, bool)>) -> Element<'a, M> {
    let look = *look;
    let mut r = row![].spacing(0);
    for (name, message, chosen) in options {
        let font = if chosen { look.bold } else { look.font };
        r = r.push(button(text(name).size(look.text_size).font(font)).padding([5, 11]).style(move |t, s| (look.segment)(t, s, chosen)).on_press(message));
    }
    container(r).style(look.segments).into()
}

/// What the export writes, where and in what.
fn settings<'a, M: Clone + 'a>(content: &Content<'a, M>, folder: Element<'a, M>, look: &Look, say: impl Fn(Event) -> M + Copy + 'a) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    // The templates: the maps as they are, the program's, the user's.
    let mut choices = column![].spacing(2).padding(4);
    let mut options: Vec<Chosen> = Preset::ALL.into_iter().map(Chosen::Preset).collect();
    options.extend((0..content.mine.len()).map(Chosen::Mine));
    for chosen in options {
        let (name, about) = chosen.said(content.mine);
        let is = chosen == content.export_with;
        let mine = if matches!(chosen, Chosen::Mine(_)) { "  · yours" } else { "" };
        let label = column![
            row![text(name).size(look.text_size).font(look.bold), text(mine).size(look.small_size).color(c.dim)].align_y(Alignment::Center),
            text(about).size(look.small_size).color(c.dim),
        ]
        .spacing(2);
        choices = choices
            .push(button(label).width(Length::Fill).padding([6, 10]).style(move |t, s| (look.option)(t, s, is)).on_press(say(Event::ExportWith(chosen))));
    }
    let templates = column![
        container(scroll_box(choices).height(Length::Shrink)).max_height(250.0).width(Length::Fill).style(style::well(&look)),
        row![
            text("Templates are made and changed in Output Templates.").size(look.small_size).color(c.faint),
            space().width(Length::Fill),
            button(text("Output Templates").size(look.small_size)).padding([3, 10]).style(look.button).on_press(say(Event::Tab(Tab::Templates))),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(6);

    let packs = content.export_with.template(content.mine).is_some();
    let mut rows: Vec<Element<'a, M>> = Vec::new();
    rows.push(setting(
        &look,
        true,
        "Output folder",
        row![
            container(container(folder).padding([5, 8]).width(Length::Fill).style(style::well(&look))).width(Length::Fill).clip(true),
            button(text("Browse…").size(look.text_size)).padding([5, 12]).style(look.button).on_press(say(Event::Browse)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ));
    rows.push(setting(&look, false, "Template", templates));
    if packs {
        let template = content.export_with.template(content.mine);
        let writes = template.is_some_and(|t| t.normals == texture_export::Normals::DirectX);
        let (name, _) = content.export_with.said(content.mine);
        let (wants, written) = if writes { ("DirectX (Y−)", "DirectX's") } else { ("OpenGL (Y+)", "OpenGL's") };
        let told = if content.directx == writes {
            format!("{name} writes {wants} normal maps: written as the graph makes them")
        } else {
            format!("{name} writes {wants} normal maps: the green channel is turned over to make them {written}")
        };
        let normals = segmented(
            &look,
            vec![("OpenGL (Y+)".into(), say(Event::DirectX(false)), !content.directx), ("DirectX (Y−)".into(), say(Event::DirectX(true)), content.directx)],
        );
        rows.push(setting(&look, true, "Graph's normals", column![normals, text(told).size(look.small_size).color(c.dim)].spacing(5)));
        let formats = Format::ALL.into_iter().map(|f| (f.name().to_string(), say(Event::Format(f)), f == content.format)).collect();
        rows.push(setting(
            &look,
            false,
            "File format",
            column![segmented(&look, formats), text("A file whose template says its own format is written in that.").size(look.small_size).color(c.dim)]
                .spacing(5),
        ));
    } else {
        rows.push(setting(&look, true, "File format", text("As each Output's node says (its channels and format).").size(look.text_size).color(c.dim)));
    }
    rows.push(setting(&look, !packs, "Size", text(format!("{} (the graph's)", content.size)).size(look.text_size).color(c.text)));
    let list = rows.into_iter().fold(Column::new().spacing(1), Column::push);
    let page = column![heading(&look, "General Export Parameters"), container(list).width(Length::Fill).style(style::well(&look))].spacing(10).padding(18);
    scroll_box(page).height(Length::Fill).into()
}

/// The files, listed before they are written.
fn files<'a, M: Clone + 'a>(content: &Content<'a, M>, look: &Look, say: impl Fn(Event) -> M + Copy + 'a) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    let cell = |s: String, width: Length, color: Color, font: Font| {
        text(s).size(look.small_size).color(color).font(font).width(width).wrapping(text::Wrapping::WordOrGlyph)
    };
    let mut table = column![
        container(
            row![
                cell("File".into(), Length::FillPortion(5), c.dim, look.bold),
                cell("Size".into(), Length::FillPortion(2), c.dim, look.bold),
                cell("Format".into(), Length::FillPortion(2), c.dim, look.bold),
                cell("Made of".into(), Length::FillPortion(6), c.dim, look.bold),
            ]
            .spacing(10)
        )
        .padding([7, 12])
        .width(Length::Fill)
        .style(style::bar(&look))
    ]
    .spacing(0);
    for (i, file) in content.files.iter().enumerate() {
        table = table.push(
            container(
                row![
                    cell(file.name.clone(), Length::FillPortion(5), c.bright, look.mono),
                    cell(content.size.clone(), Length::FillPortion(2), c.text, look.font),
                    cell(file.format.clone(), Length::FillPortion(2), c.text, look.font),
                    cell(file.said.clone(), Length::FillPortion(6), c.dim, look.font),
                ]
                .spacing(10),
            )
            .padding([6, 12])
            .width(Length::Fill)
            .style(style::row(&look, i % 2 == 0)),
        );
    }
    if content.files.is_empty() {
        table = table.push(container(text("None: the graph has no Output of what the template writes.").size(look.small_size).color(c.warning)).padding(12));
    }
    let head = row![
        heading(&look, "The files, before they are written"),
        space().width(Length::Fill),
        button(text("Open Output Folder").size(look.small_size))
            .padding([4, 10])
            .style(look.button)
            .on_press_maybe(content.folder_chosen.then(|| say(Event::OpenFolder))),
    ]
    .align_y(Alignment::Center);
    let page = column![head, container(table).width(Length::Fill).style(style::well(&look))].spacing(10).padding(18);
    scroll_box(page).height(Length::Fill).into()
}
