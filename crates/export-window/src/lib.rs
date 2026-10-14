//! The window an export of textures is chosen in, made to be the same in every program of a
//! set: a program gives what it has (what is exported, the preset chosen, the formats it writes,
//! the folder, the files the preset writes of it, see `texture-export`) and its look, and takes
//! what is chosen as [`Event`]s. It knows nothing of the program; the program puts it in a
//! dialog of its own and writes the files.

use iced::widget::{Column, button, column, container, row, space, text};
use iced::{Alignment, Color, Element, Font, Length, Theme};
use scroll_box::scroll_box;
use texture_export::Preset;

/// How the window looks: the program's fonts, colors and widgets.
#[derive(Clone, Copy)]
pub struct Look {
    /// The font of text, and its weights: of an option chosen, and of headings.
    pub font: Font,
    pub bold: Font,
    pub heavy: Font,
    /// The font of file names.
    pub mono: Font,
    /// The size of text, and of small text.
    pub text_size: f32,
    pub small_size: f32,
    /// Text: headings, what is said of an option, a warning, and what is to be made again.
    pub bright: Color,
    pub dim: Color,
    pub warning: Color,
    pub error: Color,
    /// An option of a list (the presets), the one chosen where `chosen`.
    pub option: fn(&Theme, button::Status, bool) -> button::Style,
    /// An option of a segmented control, the one chosen where `chosen`, and the control.
    pub segment: fn(&Theme, button::Status, bool) -> button::Style,
    pub segments: fn(&Theme) -> container::Style,
    /// Buttons: as any, without chrome (Cancel), and the default action (Export).
    pub button: fn(&Theme, button::Status) -> button::Style,
    pub flat_button: fn(&Theme, button::Status) -> button::Style,
    pub primary_button: fn(&Theme, button::Status) -> button::Style,
}

/// A file the export writes, as the window lists it: its name, and what it is made of, said.
#[derive(Clone, Debug, PartialEq)]
pub struct File {
    pub name: String,
    pub said: String,
}

/// What the window shows.
pub struct Content<'a, M> {
    /// What is exported (of a graph: its name).
    pub name: String,
    pub preset: Preset,
    /// The templates of the user's (each its name and what it is said to be), the one chosen in
    /// place of the preset, the folder they are kept in, and what could not be read there.
    pub templates: Vec<(String, String)>,
    pub template: Option<usize>,
    pub templates_folder: Option<String>,
    pub problems: Vec<String>,
    /// What is chosen packs maps into files (it is not the maps as they are), the normal maps it
    /// writes are DirectX's, and its name.
    pub packs: bool,
    pub writes_directx: bool,
    pub chosen: String,
    /// The program's normal maps are DirectX's (Y−), not OpenGL's.
    pub directx: bool,
    /// The formats the files of a preset may be written in, each with what the program calls
    /// it, and the one chosen.
    pub formats: &'a [(&'a str, u32)],
    pub format: u32,
    /// The folder the files go to, as the program shows a path; whether one is chosen.
    pub folder: Element<'a, M>,
    pub folder_chosen: bool,
    /// The files the preset writes, before they are written.
    pub files: Vec<File>,
}

/// What is chosen in the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    Preset(Preset),
    /// A template of the user's, of those given.
    Template(usize),
    /// Show the folder the templates of the user's are kept in.
    OpenTemplates,
    DirectX(bool),
    Format(u32),
    /// Choose the folder (the program shows a dialog of the system).
    Browse,
    Export,
    Cancel,
}

/// The window's body, the program's dialog around it: `say` makes what is chosen a message of
/// the program's.
pub fn view<'a, M: Clone + 'a>(content: Content<'a, M>, look: &Look, say: impl Fn(Event) -> M + Copy + 'a) -> Element<'a, M> {
    let look = *look;
    let label = move |t: &'static str| text(t).size(look.text_size).color(look.dim).width(Length::Fixed(110.0));
    let option = move |name: String, about: String, chosen: bool, event: Event| {
        button(column![text(name).size(look.text_size).font(look.bold), text(about).size(look.small_size).color(look.dim)].spacing(3))
            .width(Length::Fill)
            .padding([7, 10])
            .style(move |theme, status| (look.option)(theme, status, chosen))
            .on_press(say(event))
    };
    let mut presets = column![].spacing(4);
    for preset in Preset::ALL {
        let chosen = content.template.is_none() && preset == content.preset;
        presets = presets.push(option(preset.name().into(), preset.about().into(), chosen, Event::Preset(preset)));
    }
    // The templates of the user's, after the presets: what each says, and where they are kept.
    let mine = text("Your templates").size(look.text_size).font(look.bold).color(look.bright);
    let open = button(text("Open Templates Folder").size(look.small_size)).padding([4, 10]).style(look.button).on_press(say(Event::OpenTemplates));
    presets = presets.push(row![mine, space().width(Length::Fill), open].align_y(Alignment::Center).padding([6, 0]));
    for (index, (name, about)) in content.templates.iter().enumerate() {
        presets = presets.push(option(name.clone(), about.clone(), content.template == Some(index), Event::Template(index)));
    }
    if content.templates.is_empty() {
        let kept = content.templates_folder.as_deref().map_or_else(String::new, |f| format!(" in {f}"));
        let none = format!("None yet. A template is a file of JSON{kept}: open the folder for an example to start from.");
        presets = presets.push(text(none).size(look.small_size).color(look.dim));
    }
    for problem in &content.problems {
        presets = presets.push(text(format!("Not read: {problem}")).size(look.small_size).color(look.warning));
    }
    let segmented = move |options: Vec<(&'a str, M, bool)>| {
        let mut r = row![].spacing(0);
        for (name, message, chosen) in options {
            let font = if chosen { look.bold } else { look.font };
            r = r.push(
                button(text(name).size(look.text_size).font(font))
                    .padding([5, 10])
                    .style(move |theme, status| (look.segment)(theme, status, chosen))
                    .on_press(message),
            );
        }
        container(r).style(look.segments)
    };
    // (A first base: the window is to be made again, as its user said, and says so.)
    let mut body: Column<'a, M> = column![
        text("ПЕРЕДЕЛАТЬ!!!").size(30).font(look.heavy).color(look.error),
        text(format!("Export {}", content.name)).size(18).font(look.heavy).color(look.bright),
        row![label("Preset"), presets].align_y(Alignment::Start),
    ]
    .spacing(12);
    if content.packs {
        // What the program makes, and what the preset or the template writes of it.
        let (wants, writes) = if content.writes_directx { ("DirectX (Y−)", "DirectX") } else { ("OpenGL (Y+)", "OpenGL") };
        let written = if content.directx == content.writes_directx {
            format!("{} reads {wants} normal maps: written as the graph makes them", content.chosen)
        } else {
            format!("{} reads {wants} normal maps: the green channel is turned over to make them {writes}'s", content.chosen)
        };
        body = body.push(
            row![
                label("Graph's normals"),
                column![
                    segmented(vec![
                        ("OpenGL (Y+)", say(Event::DirectX(false)), !content.directx),
                        ("DirectX (Y−)", say(Event::DirectX(true)), content.directx),
                    ]),
                    text(written).size(look.small_size).color(look.dim),
                ]
                .spacing(4),
            ]
            .spacing(8)
            .align_y(Alignment::Start),
        );
        let formats = content.formats.iter().map(|&(name, format)| (name, say(Event::Format(format)), format == content.format)).collect();
        body = body.push(row![label("Format"), segmented(formats)].align_y(Alignment::Center));
    }
    body = body.push(
        row![
            label("Folder"),
            container(content.folder).width(Length::Fill).clip(true),
            button(text("Browse…").size(look.text_size)).padding([5, 12]).style(look.button).on_press(say(Event::Browse)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    // The files, before they are written.
    let mut files = column![].spacing(3);
    for file in &content.files {
        // (A name of a graph without spaces, made a file's, breaks where it has to: it ran into the
        // column beside it.)
        let wraps = text::Wrapping::WordOrGlyph;
        let mut line = row![text(file.name.clone()).size(look.small_size).font(look.mono).wrapping(wraps).width(Length::FillPortion(3))].spacing(10);
        if !file.said.is_empty() {
            line = line.push(text(format!("← {}", file.said)).size(look.small_size).color(look.dim).wrapping(wraps).width(Length::FillPortion(4)));
        }
        files = files.push(line);
    }
    if content.files.is_empty() {
        files = files.push(text("None: the graph has no Output of what this writes (give Outputs their Usage).").size(look.small_size).color(look.warning));
    }
    body = body.push(row![label("Files"), container(scroll_box(files).height(Length::Shrink)).max_height(160.0).width(Length::Fill)].align_y(Alignment::Start));
    let ready = content.folder_chosen && !content.files.is_empty();
    body.push(
        row![
            space().width(Length::Fill),
            button(text("Cancel").size(look.text_size)).padding([6, 14]).style(look.flat_button).on_press(say(Event::Cancel)),
            button(text("Export").size(look.text_size)).padding([6, 18]).style(look.primary_button).on_press_maybe(ready.then(|| say(Event::Export))),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .into()
}
