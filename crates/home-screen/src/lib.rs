//! The Home screen of a program, as Adobe's programs have one, made to be the same in every
//! program of a set: a program gives what it has (its name and logo, what a new file starts
//! from, the files opened last, what to learn from, where to read more) and takes what is
//! chosen as [`Event`]s. It knows nothing of the program.
//!
//! On the left: New… and Open…, the pages (Home, Learn), going back to what is being edited,
//! and the resources. On the right, Home: a welcome, a row of what a new file can start from
//! (the last card shows every choice), and the files opened last, as pictures or as a list
//! sorted by any column; Learn: the lessons, each with its picture.
//!
//! What a new file starts from is shown by [`template_card`] (and [`template_row`], in a list):
//! the same cards the program's window of a new file shows, so that they are the same
//! everywhere.

mod glyph;
pub mod hover_card;
mod style;

use std::time::SystemTime;

use iced::widget::{Row, button, column, container, image, row, rule, scrollable, space, text, tooltip};
use iced::{Alignment, ContentFit, Element, Length, Padding};

pub use glyph::{Glyph, glyph};
pub use style::Style;

/// A page of the Home screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Home,
    Learn,
}

/// How the files opened last are shown: as pictures, or as a list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    #[default]
    Grid,
    List,
}

/// A column of the list of files opened last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    Name,
    Opened,
    Modified,
    Size,
    Location,
}

/// What the files opened last are sorted by: a column, the other way where `descending`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sort {
    pub by: Column,
    pub descending: bool,
}

/// The file opened last first.
impl Default for Sort {
    fn default() -> Sort {
        Sort { by: Column::Opened, descending: true }
    }
}

impl Sort {
    /// Sorted by `column`: the other way, where it is sorted by it already; else the latest and
    /// the largest first, A first.
    pub fn by(self, column: Column) -> Sort {
        if self.by == column {
            Sort { descending: !self.descending, ..self }
        } else {
            Sort { by: column, descending: matches!(column, Column::Opened | Column::Modified | Column::Size) }
        }
    }
}

/// What the Home screen keeps between frames (the program keeps it for it, across runs even).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub page: Page,
    pub layout: Layout,
    pub sort: Sort,
}

impl State {
    /// What the Home screen keeps of `event`: the page shown, the pictures or the list, how
    /// the list is sorted. The rest is the program's to do.
    pub fn update(&mut self, event: &Event) {
        match event {
            Event::Page(page) => self.page = *page,
            Event::Layout(layout) => self.layout = *layout,
            Event::Sort(column) => self.sort = self.sort.by(*column),
            _ => {}
        }
    }
}

/// What a new file can start from (a template, a preset).
#[derive(Clone, Debug, PartialEq)]
pub struct Start {
    pub id: String,
    pub title: String,
    /// Its category, said in gray under its name.
    pub detail: String,
    /// What stands for it where it has no picture.
    pub glyph: Glyph,
    /// A picture of what it makes, square.
    pub picture: Option<image::Handle>,
    /// What its (i) says: what kind of thing it makes, what it is, and its outputs.
    pub kind: String,
    pub about: String,
    pub outputs: Vec<String>,
}

/// A file opened last.
#[derive(Clone, Debug)]
pub struct Recent {
    pub path: String,
    pub name: String,
    /// The folder it is in.
    pub folder: String,
    /// When it was opened last, and written.
    pub opened: Option<SystemTime>,
    pub modified: Option<SystemTime>,
    /// How many bytes it takes.
    pub size: Option<u64>,
    /// Whether it is there still.
    pub there: bool,
    pub picture: Option<image::Handle>,
}

/// Something to learn from: a file to open and take apart, a page of the program.
#[derive(Clone, Debug)]
pub struct Lesson {
    pub id: String,
    pub title: String,
    pub about: String,
    pub picture: Option<image::Handle>,
    /// What stands for it where it has no picture.
    pub glyph: Glyph,
}

/// Where to read more, or what to open: the shortcuts, the settings, a website.
#[derive(Clone, Debug, PartialEq)]
pub struct Resource {
    pub id: String,
    pub label: String,
    pub glyph: Glyph,
    /// Whether there is anything there yet (else it says "later").
    pub available: bool,
}

/// What a program shows on its Home screen, and in its words.
pub struct Content<'a> {
    pub name: &'a str,
    pub logo: Option<image::Handle>,
    /// Under the welcome: what the program is for.
    pub tagline: &'a str,
    /// New…: what it makes.
    pub new: &'a str,
    /// What a new file can start from, its heading, and the last tile (every choice).
    pub start_heading: &'a str,
    pub start: &'a [Start],
    pub more: &'a str,
    pub recent: &'a [Recent],
    /// Said where there are no files opened last yet, and how a file is opened by dropping it.
    pub no_recent: &'a str,
    pub drop_hint: &'a str,
    /// Under the heading of Learn.
    pub learn: &'a str,
    pub lessons: &'a [Lesson],
    pub resources: &'a [Resource],
    /// What is being edited, to go back to.
    pub back: Option<String>,
    /// A moment, as the list says it (on the system's calendar and clock).
    pub time: fn(SystemTime) -> String,
}

/// What was chosen on the Home screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Page(Page),
    New,
    Open,
    /// Back to what is being edited.
    Back,
    /// Start a new file from the choice of this id.
    Start(String),
    /// See every choice of what a new file starts from.
    More,
    /// Open the file opened last at this path.
    Recent(String),
    Layout(Layout),
    Sort(Column),
    Lesson(String),
    Resource(String),
}

const SIDEBAR: f32 = 250.0;
/// The tiles of what to start from, and the pictures of the files opened last.
/// How large the picture of a card of what a new file starts from is, square.
const CARD: f32 = 132.0;
const PICTURE: f32 = 176.0;
/// The pictures of the lessons, and of the rows of the list.
const LESSON: [f32; 2] = [180.0, 112.0];
const THUMB: f32 = 64.0;

/// The Home screen.
pub fn view<'a>(content: Content<'a>, state: State, style: Style) -> Element<'a, Event> {
    let page = match state.page {
        Page::Home => home(&content, state, style),
        Page::Learn => learn(&content, style),
    };
    let page = scrollable(container(page).padding(Padding { top: 30.0, right: 40.0, bottom: 34.0, left: 40.0 }).width(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style.scrollable());
    row![
        sidebar(&content, state, style),
        rule::vertical(1).style(style.rule()),
        container(page).width(Length::Fill).height(Length::Fill).style(style.page()),
    ]
    .height(Length::Fill)
    .into()
}

fn line<'a>(style: Style) -> Element<'a, Event> {
    rule::horizontal(1).style(style.rule()).into()
}

fn heading<'a>(label: &'a str, size: f32, style: Style) -> Element<'a, Event> {
    text(label).size(size).font(style.heavy()).color(style.bright).into()
}

fn sidebar<'a>(content: &Content<'a>, state: State, style: Style) -> Element<'a, Event> {
    let mut name = row![].spacing(12).align_y(Alignment::Center);
    if let Some(logo) = &content.logo {
        name = name.push(image(logo.clone()).width(Length::Fixed(40.0)).height(Length::Fixed(40.0)));
    }
    name = name.push(text(content.name).size(16).font(style.heavy()).color(style.bright));
    // (The text of the light button is dark.)
    let dark = iced::Color::from_rgb8(18, 18, 18);
    let new = button(row![glyph(Glyph::Plus, 15.0, dark), text(content.new).size(style.text_size).font(style.medium())].spacing(9).align_y(Alignment::Center))
        .width(Length::Fill)
        .padding([9, 12])
        .style(style.primary())
        .on_press(Event::New);
    let open = button(row![glyph(Glyph::Open, 15.0, style.text), text("Open…").size(style.text_size).font(style.medium())].spacing(9).align_y(Alignment::Center))
        .width(Length::Fill)
        .padding([9, 12])
        .style(style.secondary())
        .on_press(Event::Open);
    let mut side = column![
        name,
        space().height(12),
        new,
        open,
        space().height(6),
        line(style),
        space().height(2),
        tab("Home", Glyph::Home, Page::Home, state.page, style),
        tab("Learn", Glyph::Learn, Page::Learn, state.page, style),
    ]
    .spacing(6);
    if let Some(back) = &content.back {
        side = side.push(
            button(
                row![glyph(Glyph::Back, 15.0, style.dim), text(format!("Back to {back}")).size(style.text_size).wrapping(text::Wrapping::None)]
                    .spacing(10)
                    .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding([8, 12])
            .style(style.flat())
            .on_press(Event::Back),
        );
    }
    let mut resources = column![container(heading("Resources", 14.0, style)).padding(Padding { left: 12.0, bottom: 6.0, ..Padding::ZERO })].spacing(2);
    for resource in content.resources {
        let color = if resource.available { style.dim } else { style.faint };
        let mut said = row![glyph(resource.glyph, 15.0, color), text(resource.label.as_str()).size(style.text_size).color(color)]
            .spacing(10)
            .align_y(Alignment::Center);
        if !resource.available {
            said = said.push(space().width(Length::Fill)).push(text("later").size(11).color(style.faint));
        }
        resources = resources.push(
            button(said)
                .width(Length::Fill)
                .padding([6, 12])
                .style(style.flat())
                .on_press_maybe(resource.available.then(|| Event::Resource(resource.id.clone()))),
        );
    }
    let side = side.push(space().height(Length::Fill)).push(line(style)).push(space().height(6)).push(resources);
    container(side).padding([20, 14]).width(Length::Fixed(SIDEBAR)).height(Length::Fill).style(style.sidebar()).into()
}

/// A page of the side bar: darker than the bar when it is the one shown, its text white and
/// heavier.
fn tab<'a>(label: &'a str, g: Glyph, page: Page, shown: Page, style: Style) -> Element<'a, Event> {
    let chosen = page == shown;
    button(
        row![
            glyph(g, 16.0, if chosen { style.bright } else { style.dim }),
            text(label).size(style.text_size).font(if chosen { style.semibold() } else { style.font }),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([8, 12])
    .style(style.tab(chosen))
    .on_press(Event::Page(page))
    .into()
}

fn home<'a>(content: &Content<'a>, state: State, style: Style) -> Element<'a, Event> {
    let mut page = column![
        text(format!("Welcome to {}", content.name)).size(28).font(style.heavy()).color(style.bright),
        text(content.tagline).size(style.text_size).color(style.dim),
        space().height(12),
        line(style),
        space().height(6),
        heading(content.start_heading, 16.0, style),
    ]
    .spacing(8);
    // What to start from, and every choice last.
    let mut cards: Vec<Element<'a, Event>> = content.start.iter().map(|s| template_card(s, false, Event::Start(s.id.clone()), style)).collect();
    cards.push(more_card(content.more, Event::More, style));
    page = page.push(Row::with_children(cards).spacing(12).wrap());
    page = page.push(space().height(16)).push(line(style)).push(space().height(6));
    page = page.push(row![heading("Recent", 16.0, style), space().width(Length::Fill), layout_toggle(state.layout, style)].align_y(Alignment::Center));
    if content.recent.is_empty() {
        page = page.push(text(content.no_recent).size(style.text_size).color(style.faint));
    } else {
        let recent = sorted(content.recent, state.sort);
        page = page.push(match state.layout {
            Layout::Grid => Row::with_children(recent.into_iter().map(|r| card(r, style))).spacing(12).wrap().into(),
            Layout::List => list(recent, state.sort, content.time, style),
        });
    }
    page = page.push(space().height(8)).push(
        row![glyph(Glyph::Info, 14.0, style.faint), text(content.drop_hint).size(style.small_size).color(style.faint)].spacing(6).align_y(Alignment::Center),
    );
    page.into()
}

/// A tile of what to start from: its glyph, its name, and what it is.
/// A card of what a new file starts from: its picture, its (i) at the top right of it, its name
/// and, in gray, its category. Lit when pointed at, outlined white when `chosen`; `on_press`
/// says it is chosen. (The (i) shows a card of its own while it is pointed at: see
/// [`hover_card`].)
pub fn template_card<'a, M: Clone + 'a>(start: &'a Start, chosen: bool, on_press: M, style: Style) -> Element<'a, M> {
    let picture: Element<'a, M> = match &start.picture {
        Some(handle) => image(handle.clone()).width(Length::Fixed(CARD)).height(Length::Fixed(CARD)).content_fit(ContentFit::Cover).into(),
        None => container(glyph(start.glyph, 36.0, style.faint)).center(Length::Fixed(CARD)).into(),
    };
    let top = iced::widget::stack![
        container(picture).style(style.frame()).clip(true),
        container(info_badge(start, style)).width(Length::Fixed(CARD)).align_x(Alignment::End).padding(5),
    ];
    let body = column![
        top,
        text(start.title.as_str()).size(style.text_size).font(style.semibold()).wrapping(text::Wrapping::None),
        text(start.detail.as_str()).size(style.small_size).color(style.faint).wrapping(text::Wrapping::None),
    ]
    .spacing(4)
    .width(Length::Fixed(CARD));
    button(body).padding(8).style(style.template_card(chosen)).on_press(on_press).into()
}

/// The last card of a row of what a new file starts from: every choice, `label`.
pub fn more_card<'a, M: Clone + 'a>(label: &'a str, on_press: M, style: Style) -> Element<'a, M> {
    let body = column![
        container(glyph(Glyph::More, 36.0, style.dim)).center(Length::Fixed(CARD)).style(style.frame()),
        text(label).size(style.text_size).font(style.semibold()).wrapping(text::Wrapping::None),
        text(" ").size(style.small_size),
    ]
    .spacing(4)
    .width(Length::Fixed(CARD));
    button(body).padding(8).style(style.template_card(false)).on_press(on_press).into()
}

/// What a new file starts from, as a row of a list: a small picture, its name, its category and
/// its kind, its (i); outlined white when `chosen`.
pub fn template_row<'a, M: Clone + 'a>(start: &'a Start, chosen: bool, on_press: M, style: Style) -> Element<'a, M> {
    const SMALL: f32 = 40.0;
    let picture: Element<'a, M> = match &start.picture {
        Some(handle) => image(handle.clone()).width(Length::Fixed(SMALL)).height(Length::Fixed(SMALL)).content_fit(ContentFit::Cover).into(),
        None => container(glyph(start.glyph, 20.0, style.faint)).center(Length::Fixed(SMALL)).into(),
    };
    let said = row![
        container(picture).style(style.frame()).clip(true),
        text(start.title.as_str()).size(style.text_size).font(style.semibold()).width(Length::FillPortion(3)),
        text(start.detail.as_str()).size(style.small_size).color(style.faint).width(Length::FillPortion(3)),
        text(start.kind.as_str()).size(style.small_size).color(style.dim).width(Length::FillPortion(2)),
        info_badge(start, style),
    ]
    .spacing(12)
    .align_y(Alignment::Center);
    button(said)
        .width(Length::Fill)
        .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 4.0 })
        .style(style.template_card(chosen))
        .on_press(on_press)
        .into()
}

/// The (i) of `start`, which says what it is while it is pointed at.
fn info_badge<'a, M: 'a>(start: &'a Start, style: Style) -> Element<'a, M> {
    hover_card::hover_card(container(glyph(Glyph::Info, 16.0, style.text)).padding(3).style(style.badge()), info(start, style)).into()
}

/// What the (i) of `start` says: its name, its kind, what it is, its outputs.
fn info<'a, M: 'a>(start: &'a Start, style: Style) -> Element<'a, M> {
    let outputs = if start.outputs.is_empty() { "none".to_string() } else { start.outputs.join(", ") };
    container(
        column![
            text(start.title.as_str()).size(14).font(style.heavy()).color(style.bright),
            text(format!("Type: {}", start.kind)).size(style.small_size).color(style.dim),
            text(start.about.as_str()).size(style.small_size).color(style.text),
            text(format!("Outputs: {outputs}")).size(style.small_size).color(style.dim),
        ]
        .spacing(6),
    )
    .padding(12)
    .width(Length::Fixed(280.0))
    .style(style.tooltip())
    .into()
}

/// The pictures, or the list.
fn layout_toggle<'a>(layout: Layout, style: Style) -> Element<'a, Event> {
    let option = |this: Layout, g: Glyph, said: &'static str| {
        let chosen = layout == this;
        let b = button(glyph(g, 15.0, if chosen { style.bright } else { style.dim })).padding([4, 7]).style(style.segment(chosen)).on_press(Event::Layout(this));
        tooltip(b, container(text(said).size(style.small_size)).padding(6).style(style.tooltip()), tooltip::Position::Bottom)
    };
    container(row![option(Layout::Grid, Glyph::Grid, "As pictures"), option(Layout::List, Glyph::List, "As a list")].spacing(2))
        .padding(2)
        .style(style.segments())
        .into()
}

/// The files opened last, sorted as `sort` says.
fn sorted(recent: &[Recent], sort: Sort) -> Vec<&Recent> {
    // (As the program lists them: the file opened last first.)
    let mut order: Vec<(usize, &Recent)> = recent.iter().enumerate().collect();
    order.sort_by(|(i, a), (j, b)| {
        let by = match sort.by {
            Column::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Column::Opened => a.opened.cmp(&b.opened).then(j.cmp(i)),
            Column::Modified => a.modified.cmp(&b.modified),
            Column::Size => a.size.cmp(&b.size),
            Column::Location => a.folder.to_lowercase().cmp(&b.folder.to_lowercase()),
        };
        if sort.descending { by.reverse() } else { by }
    });
    order.into_iter().map(|(_, r)| r).collect()
}

/// A file opened last as a picture: its name, where it is, when it was opened and how big.
fn card<'a>(recent: &'a Recent, style: Style) -> Element<'a, Event> {
    let picture: Element<'a, Event> = match &recent.picture {
        Some(handle) => image(handle.clone()).width(Length::Fixed(PICTURE)).height(Length::Fixed(PICTURE)).content_fit(ContentFit::Cover).into(),
        None => container(glyph(Glyph::File, 40.0, style.faint)).center(Length::Fixed(PICTURE)).into(),
    };
    let said = if recent.there {
        [recent.opened.map(ago), recent.size.map(size_said)].into_iter().flatten().collect::<Vec<_>>().join(" · ")
    } else {
        "Not found where it was".to_string()
    };
    let body = column![
        container(picture).style(style.frame()).clip(true),
        space().height(4),
        text(recent.name.as_str()).size(style.text_size).font(style.semibold()).wrapping(text::Wrapping::None),
        text(recent.folder.as_str()).size(style.small_size).color(style.faint).wrapping(text::Wrapping::None),
        text(said).size(style.small_size).color(if recent.there { style.dim } else { style.warning }),
    ]
    .spacing(3)
    .width(Length::Fixed(PICTURE));
    let tile = button(container(body).clip(true))
        .padding(10)
        .style(style.tile())
        .on_press_maybe(recent.there.then(|| Event::Recent(recent.path.clone())));
    tooltip(tile, container(text(recent.path.as_str()).size(style.small_size)).padding(6).style(style.tooltip()), tooltip::Position::Bottom).into()
}

/// The files opened last as a list, with a heading of each column to sort by.
fn list<'a>(recent: Vec<&'a Recent>, sort: Sort, time: fn(SystemTime) -> String, style: Style) -> Element<'a, Event> {
    let widths = [Length::FillPortion(4), Length::Fixed(160.0), Length::Fixed(160.0), Length::Fixed(80.0), Length::FillPortion(4)];
    let columns = [(Column::Name, "Name"), (Column::Opened, "Opened"), (Column::Modified, "Modified"), (Column::Size, "Size"), (Column::Location, "Location")];
    let mut heads = row![space().width(Length::Fixed(THUMB))].spacing(10).align_y(Alignment::Center);
    for ((column, label), width) in columns.into_iter().zip(widths) {
        let mut said = row![text(label).size(style.small_size).font(style.semibold())].spacing(2).align_y(Alignment::Center);
        if sort.by == column {
            said = said.push(glyph(if sort.descending { Glyph::SortDown } else { Glyph::SortUp }, 13.0, style.text));
        }
        heads = heads.push(button(said).width(width).padding([3, 0]).style(style.flat()).on_press(Event::Sort(column)));
    }
    let mut rows = column![container(heads).padding([2, 8]), line(style)].spacing(2);
    for r in recent {
        let thumb: Element<'a, Event> = match &r.picture {
            Some(handle) => image(handle.clone()).width(Length::Fixed(THUMB)).height(Length::Fixed(THUMB)).content_fit(ContentFit::Cover).into(),
            None => container(glyph(Glyph::File, 26.0, style.faint)).center(Length::Fixed(THUMB)).into(),
        };
        let cell = |said: String, width: Length| text(said).size(style.small_size).color(style.dim).width(width).wrapping(text::Wrapping::None);
        let name = text(r.name.as_str()).size(style.text_size).width(widths[0]).wrapping(text::Wrapping::None);
        let cells = row![
            container(thumb).style(style.frame()).clip(true),
            name,
            cell(r.opened.map(time).unwrap_or_default(), widths[1]),
            cell(if r.there { r.modified.map(time).unwrap_or_default() } else { "Not found".into() }, widths[2]),
            cell(r.size.map(size_said).unwrap_or_default(), widths[3]),
            cell(r.folder.clone(), widths[4]),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        rows = rows.push(
            button(container(cells).clip(true))
                .width(Length::Fill)
                .padding([4, 8])
                .style(style.flat())
                .on_press_maybe(r.there.then(|| Event::Recent(r.path.clone()))),
        );
    }
    rows.into()
}

fn learn<'a>(content: &Content<'a>, style: Style) -> Element<'a, Event> {
    let mut page = column![
        text("Learn").size(28).font(style.heavy()).color(style.bright),
        text(content.learn).size(style.text_size).color(style.dim),
        space().height(12),
        line(style),
        space().height(6),
    ]
    .spacing(8);
    for lesson in content.lessons {
        let picture: Element<'a, Event> = match &lesson.picture {
            Some(handle) => image(handle.clone()).width(Length::Fixed(LESSON[0])).height(Length::Fixed(LESSON[1])).content_fit(ContentFit::Cover).into(),
            None => container(glyph(lesson.glyph, 36.0, style.faint)).center_x(Length::Fixed(LESSON[0])).center_y(Length::Fixed(LESSON[1])).into(),
        };
        let item = row![
            container(picture).style(style.frame()).clip(true),
            column![text(lesson.title.as_str()).size(15).font(style.heavy()).color(style.bright), text(lesson.about.as_str()).size(style.text_size).color(style.dim)]
                .spacing(6)
                .width(Length::Fill),
            button(text("Open").size(style.text_size).font(style.medium())).padding([7, 18]).style(style.secondary()).on_press(Event::Lesson(lesson.id.clone())),
        ]
        .spacing(18)
        .align_y(Alignment::Center);
        page = page.push(container(item).padding(12).width(Length::Fill).max_width(920).style(style.card()));
    }
    page.into()
}

/// How long ago `at` was, in words.
fn ago(at: SystemTime) -> String {
    let Ok(since) = SystemTime::now().duration_since(at) else { return "just now".into() };
    let minutes = since.as_secs() / 60;
    match minutes {
        0 => "just now".into(),
        1 => "a minute ago".into(),
        2..=59 => format!("{minutes} minutes ago"),
        60..=119 => "an hour ago".into(),
        120..=1439 => format!("{} hours ago", minutes / 60),
        1440..=2879 => "yesterday".into(),
        _ => format!("{} days ago", minutes / 1440),
    }
}

/// A number of bytes, in words: "18 KB", "3.4 MB", "1.2 GB".
pub fn size_said(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB * KB {
        format!("{} KB", (b / KB).ceil().max(1.0))
    } else if b < KB * KB * KB {
        format!("{:.1} MB", b / (KB * KB))
    } else {
        format!("{:.1} GB", b / (KB * KB * KB))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recent(name: &str, opened: u64, size: u64) -> Recent {
        let at = |s: u64| Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(s));
        Recent {
            path: format!("/w/{name}"),
            name: name.into(),
            folder: "/w".into(),
            opened: at(opened),
            modified: at(1000 - opened),
            size: Some(size),
            there: true,
            picture: None,
        }
    }

    /// The files opened last are the latest first; a column sorts them, again the other way.
    #[test]
    fn files_are_sorted_by_the_column_chosen() {
        let files = [recent("b", 30, 10), recent("a", 20, 30), recent("c", 10, 20)];
        let names = |sort: Sort| sorted(&files, sort).iter().map(|r| r.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(Sort::default()), ["b", "a", "c"]);
        let by_name = Sort::default().by(Column::Name);
        assert_eq!(names(by_name), ["a", "b", "c"]);
        assert_eq!(names(by_name.by(Column::Name)), ["c", "b", "a"]);
        assert_eq!(names(Sort::default().by(Column::Size)), ["a", "c", "b"], "the largest first");
        let mut state = State::default();
        state.update(&Event::Sort(Column::Modified));
        assert_eq!(names(state.sort), ["c", "a", "b"], "the one written last first");
        state.update(&Event::Layout(Layout::List));
        state.update(&Event::Page(Page::Learn));
        assert_eq!((state.page, state.layout), (Page::Learn, Layout::List));
    }

    #[test]
    fn sizes_are_said_in_words() {
        assert_eq!(size_said(10), "1 KB");
        assert_eq!(size_said(18 * 1024), "18 KB");
        assert_eq!(size_said(3_565_158), "3.4 MB");
        assert_eq!(size_said(1_288_490_189), "1.2 GB");
    }
}
