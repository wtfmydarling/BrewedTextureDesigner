//! Output Templates: the templates, the program's and the user's, on the left; the one shown, its
//! files as cards with their channels, in the middle; the maps a channel is made of on the right.
//! A channel is chosen, then a map pressed; a template of the program's is shown, and copied to
//! be changed.

use iced::widget::{Column, Row, button, column, container, row, space, text, text_input};
use iced::{Alignment, Element, Length};
use scroll_box::scroll_box;
use texture_export::{Edit, Format, Input, Layout, Missing, Part, Preset, Template, Usage};

use crate::{Chosen, Content, Event, Look, State, heading, segmented, style};

/// The tags of the channels of `file`'s parts: `RGB`, `A`; `Gray` of a file of one channel.
fn tags(template: &texture_export::File) -> Vec<String> {
    if template.channels() == 1 {
        return vec!["Gray".into()];
    }
    let mut letters = ["R", "G", "B", "A"].into_iter();
    template.parts.iter().map(|p| (0..p.channels()).filter_map(|_| letters.next()).collect()).collect()
}

/// What a part is made of, said on its chip.
fn label(part: &Part) -> (String, bool) {
    match part {
        Part::Value { said, .. } if said.is_empty() => ("Choose a map".into(), true),
        Part::Value { value, .. } => (format!("{value}"), false),
        Part::Rgb(input) | Part::One(input) => {
            let channel = match (part, input.channel) {
                (Part::One(_), 1) => " · G",
                (Part::One(_), 2) => " · B",
                (Part::One(_), 3) => " · A",
                _ => "",
            };
            (format!("{}{}{channel}", if input.invert { "1 − " } else { "" }, input.map.name()), false)
        }
    }
}

pub(crate) fn view<'a, M: Clone + 'a>(content: &Content<'a, M>, state: &'a State, look: &Look, say: impl Fn(Event) -> M + Copy + 'a) -> Element<'a, M> {
    let look = *look;
    let editable = match state.shown {
        Chosen::Mine(i) => (i < content.mine.len()).then_some(i),
        Chosen::Preset(_) => None,
    };
    let templates = templates(content, state, &look, say);
    let shown = shown(content, state, editable, &look, say);
    let maps = maps(content, state, editable, &look, say);
    row![
        container(templates).width(Length::Fixed(240.0)).height(Length::Fill).style(style::sunken_pane(&look)),
        container(shown).width(Length::Fill).height(Length::Fill),
        container(maps).width(Length::Fixed(230.0)).height(Length::Fill).style(style::sunken_pane(&look)),
    ]
    .spacing(12)
    .padding(14)
    .into()
}

/// The templates: the program's and the user's, and what is done with them.
fn templates<'a, M: Clone + 'a>(content: &Content<'a, M>, state: &'a State, look: &Look, say: impl Fn(Event) -> M + Copy + 'a) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    let small = |s: &'static str, on: Option<Event>| button(text(s).size(look.small_size)).padding([4, 9]).style(look.button).on_press_maybe(on.map(say));
    let mine = match state.shown {
        Chosen::Mine(i) => Some(i),
        Chosen::Preset(_) => None,
    };
    let tools =
        row![small("New", Some(Event::New)), small("Duplicate", Some(Event::Duplicate(state.shown))), small("Delete", mine.map(|i| Event::Deleting(Some(i))))]
            .spacing(5);
    let mut list = column![heading(&look, "Templates"), tools].spacing(8).padding(10);
    if let Some(i) = state.deleting.filter(|&i| i < content.mine.len()) {
        let asked = column![
            text(format!("Delete \"{}\"? Its file goes with it.", content.mine[i].name)).size(look.small_size).color(c.warning),
            row![small("Delete", Some(Event::Delete(i))), small("Keep", Some(Event::Deleting(None)))].spacing(5),
        ]
        .spacing(6);
        list = list.push(container(asked).padding(8).width(Length::Fill).style(style::card(&look, false)));
    }
    let option = move |name: String, chosen: bool, to: Chosen| {
        let font = if chosen { look.bold } else { look.font };
        button(text(name).size(look.text_size).font(font))
            .width(Length::Fill)
            .padding([5, 9])
            .style(move |t, s| (look.option)(t, s, chosen))
            .on_press(say(Event::Show(to)))
    };
    let mut entries = column![text("Built in").size(look.small_size).color(c.faint)].spacing(2);
    for preset in Preset::ALL.into_iter().filter(|p| p.template().is_some()) {
        entries = entries.push(option(preset.name().into(), state.shown == Chosen::Preset(preset), Chosen::Preset(preset)));
    }
    entries = entries.push(space().height(8)).push(text("Yours").size(look.small_size).color(c.faint));
    for (i, template) in content.mine.iter().enumerate() {
        entries = entries.push(option(template.name.clone(), state.shown == Chosen::Mine(i), Chosen::Mine(i)));
    }
    if content.mine.is_empty() {
        entries = entries.push(text("None yet: duplicate a template to make one of your own, or press New.").size(look.small_size).color(c.dim));
    }
    for problem in content.problems {
        entries = entries.push(text(format!("Not read: {problem}")).size(look.small_size).color(c.warning));
    }
    list = list.push(scroll_box(entries).height(Length::Fill));
    let kept = content.templates_folder.clone().map_or_else(|| "Templates of your own are not kept here.".into(), |f| format!("Kept in {f}"));
    list = list.push(
        column![
            button(text("Open Templates Folder").size(look.small_size))
                .width(Length::Fill)
                .padding([5, 9])
                .style(look.button)
                .on_press(say(Event::OpenTemplates)),
            text(kept).size(look.small_size).color(c.faint).wrapping(text::Wrapping::WordOrGlyph),
        ]
        .spacing(4),
    );
    list.into()
}

/// The template shown: what it is, and its files.
fn shown<'a, M: Clone + 'a>(
    content: &Content<'a, M>,
    state: &'a State,
    editable: Option<usize>,
    look: &Look,
    say: impl Fn(Event) -> M + Copy + 'a,
) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    let Some(template) = state.shown.template(content.mine) else {
        return text("No template is shown.").size(look.text_size).color(c.dim).into();
    };
    let edit = move |e: Edit| editable.map(|i| say(Event::Edit(i, e)));
    // What it is: changed where it is the user's; else said, and copied to be changed.
    let about: Element<'a, M> = match editable {
        Some(i) => {
            let field = |placeholder: &'static str, value: &str, make: fn(String) -> Edit| {
                text_input(placeholder, value).size(look.text_size).padding([5, 8]).style(look.text_input).on_input(move |s| say(Event::Edit(i, make(s))))
            };
            let normals = segmented(
                &look,
                vec![
                    (
                        "OpenGL (Y+)".into(),
                        say(Event::Edit(i, Edit::Normals(texture_export::Normals::OpenGL))),
                        template.normals == texture_export::Normals::OpenGL,
                    ),
                    (
                        "DirectX (Y−)".into(),
                        say(Event::Edit(i, Edit::Normals(texture_export::Normals::DirectX))),
                        template.normals == texture_export::Normals::DirectX,
                    ),
                ],
            );
            column![
                row![text("Name").size(look.small_size).color(c.dim).width(Length::Fixed(90.0)), field("The template's name", &template.name, Edit::Name)]
                    .align_y(Alignment::Center),
                row![
                    text("About").size(look.small_size).color(c.dim).width(Length::Fixed(90.0)),
                    field("What it writes, for whom", &template.about, Edit::About)
                ]
                .align_y(Alignment::Center),
                row![text("Normal maps").size(look.small_size).color(c.dim).width(Length::Fixed(90.0)), normals].align_y(Alignment::Center),
            ]
            .spacing(6)
            .into()
        }
        None => column![
            text(template.name.clone()).size(15).font(look.heavy).color(c.bright),
            text(template.about.clone()).size(look.small_size).color(c.dim),
            row![
                text("Built in: it is kept as it is. Duplicate it to make one of your own you can change.").size(look.small_size).color(c.faint),
                space().width(Length::Fill),
                button(text("Duplicate to Change").size(look.small_size))
                    .padding([4, 10])
                    .style(look.primary_button)
                    .on_press(say(Event::Duplicate(state.shown))),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(5)
        .into(),
    };
    let head = container(about).padding(12).width(Length::Fill).style(style::card(&look, false));

    // Its files: a new one laid out as it is asked.
    let mut add = row![heading(&look, "Files"), space().width(Length::Fill)].spacing(5).align_y(Alignment::Center);
    if editable.is_some() {
        add = add.push(text("Add a file:").size(look.small_size).color(c.dim));
        for layout in Layout::ALL {
            add = add.push(button(text(layout.name()).size(look.small_size)).padding([4, 9]).style(look.button).on_press_maybe(edit(Edit::AddFile(layout))));
        }
    }
    let mut cards = Column::new().spacing(10);
    for (f, file) in template.files.iter().enumerate() {
        cards = cards.push(card(content, state, editable, template, f, file, &look, say));
    }
    if template.files.is_empty() {
        cards = cards.push(text("No files yet: add one above, then give its channels their maps.").size(look.small_size).color(c.dim));
    }
    let mut page = column![head, add, scroll_box(cards).height(Length::Fill)].spacing(10);
    if let (Some(i), Some((f, p))) = (editable, state.slot)
        && let Some(part) = template.files.get(f).and_then(|file| file.parts.get(p))
    {
        page = page.push(inspector(template, i, f, p, part, state, &look, say));
    }
    page.into()
}

/// A file of the template shown: its name (and the parts a name may have), its format, its
/// channels.
#[allow(clippy::too_many_arguments)]
fn card<'a, M: Clone + 'a>(
    content: &Content<'a, M>,
    state: &'a State,
    editable: Option<usize>,
    template: &Template,
    f: usize,
    file: &texture_export::File,
    look: &Look,
    say: impl Fn(Event) -> M + Copy + 'a,
) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    let edit = move |e: Edit| editable.map(|i| say(Event::Edit(i, e)));
    let small = |s: String, on: Option<M>| button(text(s).size(look.small_size)).padding([4, 9]).style(look.button).on_press_maybe(on);
    let format = file.format.map_or_else(|| "As the export".to_string(), |f| f.name().to_string());
    let mut top: Row<'a, M> = row![].spacing(6).align_y(Alignment::Center);
    match editable {
        Some(i) => {
            top = top.push(
                text_input("A name: T_$(graph)_BaseColor", &file.name)
                    .size(look.text_size)
                    .font(look.mono)
                    .padding([5, 8])
                    .style(look.text_input)
                    .on_input(move |s| say(Event::Edit(i, Edit::FileName { file: f, name: s }))),
            );
            let open = state.tokens == Some(f);
            top = top.push(small("Insert Name Part".into(), Some(say(Event::Tokens(if open { None } else { Some(f) })))));
            let open = state.formats == Some(f);
            top = top.push(small(format!("Format: {format}"), Some(say(Event::Formats(if open { None } else { Some(f) })))));
            top = top.push(small("↑".into(), (f > 0).then(|| say(Event::Edit(i, Edit::MoveFile { from: f, to: f - 1 })))));
            top = top.push(small("↓".into(), (f + 1 < template.files.len()).then(|| say(Event::Edit(i, Edit::MoveFile { from: f, to: f + 1 })))));
            top = top
                .push(button(text("Remove").size(look.small_size)).padding([4, 9]).style(look.flat_button).on_press(say(Event::Edit(i, Edit::RemoveFile(f)))));
        }
        None => {
            top = top.push(text(file.name.clone()).size(look.text_size).font(look.mono).color(c.bright).width(Length::Fill));
            top = top.push(text(format!("Format: {format}")).size(look.small_size).color(c.dim));
        }
    }
    let mut body = column![top].spacing(8);
    // The parts a name may have, each said.
    if state.tokens == Some(f) && editable.is_some() {
        let mut menu = column![text("Parts of a name, filled in as the files are written:").size(look.small_size).color(c.dim)].spacing(2);
        for token in content.tokens {
            let name = format!("{}{}", file.name, token.written());
            menu = menu.push(
                button(
                    row![
                        text(token.written()).size(look.text_size).font(look.mono).color(c.bright).width(Length::Fixed(110.0)),
                        text(token.about).size(look.small_size).color(c.dim)
                    ]
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding([5, 8])
                .style(style::item(&look))
                .on_press_maybe(edit(Edit::FileName { file: f, name })),
            );
        }
        body = body.push(container(menu).padding(8).width(Length::Fill).style(style::menu(&look)));
    }
    if state.formats == Some(f) && editable.is_some() {
        let mut menu = column![].spacing(2);
        let options = std::iter::once((None, "As the export says".to_string())).chain(Format::ALL.into_iter().map(|f| (Some(f), f.name().to_string())));
        for (format, name) in options {
            let chosen = format == file.format;
            let font = if chosen { look.bold } else { look.font };
            menu = menu.push(
                button(text(name).size(look.text_size).font(font))
                    .width(Length::Fill)
                    .padding([5, 8])
                    .style(style::item(&look))
                    .on_press_maybe(edit(Edit::Format { file: f, format })),
            );
        }
        body = body.push(container(menu).padding(6).width(Length::Fixed(240.0)).style(style::menu(&look)));
    }
    // The name it is written by, of what is exported now; parts of a name not known here.
    let extension = file.format.map_or("png", Format::extension);
    let values: Vec<(&str, &str)> = content.values.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let written = texture_export::naming::expand(&file.name, &values);
    let mut said = row![text(format!("→ {written}.{extension}")).size(look.small_size).font(look.mono).color(c.dim)].spacing(10);
    let unknown = texture_export::naming::unknown_parts(&file.name, content.tokens);
    if !unknown.is_empty() {
        said = said.push(text(format!("{} stands for nothing here", unknown.join(", "))).size(look.small_size).color(c.warning));
    }
    body = body.push(said);
    // Its channels.
    let mut chips = row![].spacing(6).align_y(Alignment::Center);
    for (p, (part, tag)) in file.parts.iter().zip(tags(file)).enumerate() {
        let chosen = state.slot == Some((f, p));
        let (said, empty) = label(part);
        let color = style::channel_color(&tag);
        let face = column![text(tag.clone()).size(look.small_size).font(look.bold).color(color), text(said).size(look.small_size)]
            .spacing(1)
            .align_x(Alignment::Center);
        let width = if part.channels() == 3 { 150.0 } else { 104.0 };
        let on = if editable.is_some() { Some(say(Event::Slot(if chosen { None } else { Some((f, p)) }))) } else { None };
        chips = chips.push(button(face).width(Length::Fixed(width)).padding([6, 8]).style(style::chip(&look, &tag, chosen, empty)).on_press_maybe(on));
    }
    body = body.push(chips);
    let chosen = state.slot.is_some_and(|(at, _)| at == f);
    container(body).padding(12).width(Length::Fill).style(style::card(&look, chosen)).into()
}

/// The channel chosen: its map, the map's channel, whether it is turned over, and what it is
/// where the graph has no such map; or its value.
#[allow(clippy::too_many_arguments)]
fn inspector<'a, M: Clone + 'a>(
    template: &Template,
    i: usize,
    f: usize,
    p: usize,
    part: &Part,
    state: &'a State,
    look: &Look,
    say: impl Fn(Event) -> M + Copy + 'a,
) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    let tag = template.files.get(f).map(tags).and_then(|t| t.get(p).cloned()).unwrap_or_default();
    let file = template.files.get(f).map_or_else(String::new, |f| f.name.clone());
    let edit = move |e: Edit| say(Event::Edit(i, e));
    // (What a field says that is not a number changes nothing.)
    let still = say(Event::Slot(state.slot));
    let label = |s: &'static str| text(s).size(look.small_size).color(c.dim).width(Length::Fixed(170.0));
    let number = move |value: f32, make: Box<dyn Fn(f32) -> Edit + 'a>| {
        let still = still.clone();
        text_input("0 … 1", &format!("{value}"))
            .size(look.text_size)
            .width(Length::Fixed(90.0))
            .padding([4, 8])
            .style(look.text_input)
            .on_input(move |s| s.trim().parse::<f32>().ok().filter(|v| v.is_finite()).map_or_else(|| still.clone(), |v| edit(make(v))))
    };
    let mut rows = column![text(format!("{tag} of {file}")).size(look.text_size).font(look.bold).color(style::channel_color(&tag))].spacing(8);
    match part {
        Part::Value { value, .. } => {
            let field = number(*value, Box::new(move |value| Edit::Value { file: f, part: p, value }));
            rows = rows.push(row![label("A value everywhere"), field].align_y(Alignment::Center));
            rows = rows.push(text("Or press a map on the right: the channel is made of it.").size(look.small_size).color(c.faint));
        }
        Part::Rgb(input) | Part::One(input) => {
            let Input { map, channel, invert, missing, optional } = input.clone();
            rows = rows.push(row![label("Made of"), text(map.name()).size(look.text_size).color(c.bright)].align_y(Alignment::Center));
            if matches!(part, Part::One(_)) {
                let channels = ["R", "G", "B", "A"]
                    .into_iter()
                    .enumerate()
                    .map(|(n, s)| (s.to_string(), edit(Edit::Channel { file: f, part: p, channel: n as u8 }), channel as usize == n))
                    .collect();
                rows = rows.push(row![label("Its channel"), segmented(&look, channels)].align_y(Alignment::Center));
            }
            let turned = vec![
                ("As it is".to_string(), edit(Edit::Invert { file: f, part: p, invert: false }), !invert),
                ("Turned over (1 − it)".to_string(), edit(Edit::Invert { file: f, part: p, invert: true }), invert),
            ];
            rows = rows.push(row![label("Its values"), segmented(&look, turned)].align_y(Alignment::Center));
            let value = missing.unwrap_or(0.0);
            let without = vec![
                ("A value".to_string(), edit(Edit::Missing { file: f, part: p, missing: Missing::Value(value) }), missing.is_some()),
                ("Left out".to_string(), edit(Edit::Missing { file: f, part: p, missing: Missing::LeaveOut }), optional),
                ("No file".to_string(), edit(Edit::Missing { file: f, part: p, missing: Missing::NoFile }), missing.is_none() && !optional),
            ];
            let mut line = row![label("Where the graph has none"), segmented(&look, without)].spacing(8).align_y(Alignment::Center);
            if missing.is_some() {
                line = line.push(number(value, Box::new(move |value| Edit::Missing { file: f, part: p, missing: Missing::Value(value) })));
            }
            rows = rows.push(line);
        }
    }
    container(rows).padding(12).width(Length::Fill).style(style::card(&look, true)).into()
}

/// The maps a channel is made of: those a graph may have, those made of them, a value.
fn maps<'a, M: Clone + 'a>(
    content: &Content<'a, M>,
    state: &'a State,
    editable: Option<usize>,
    look: &Look,
    say: impl Fn(Event) -> M + Copy + 'a,
) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    let to = match (editable, state.slot) {
        (Some(i), Some((f, p))) => Some((i, f, p)),
        _ => None,
    };
    let assign = move |map: Option<Usage>, invert: bool| to.map(|(i, file, part)| say(Event::Edit(i, Edit::Map { file, part, map, invert })));
    let hint = match (editable, state.slot) {
        (None, _) => "Duplicate the template to give its channels other maps.",
        (Some(_), None) => "Choose a channel of a file, then the map it is made of.",
        (Some(_), Some(_)) => "Press the map the channel chosen is made of.",
    };
    let mut list = column![heading(&look, "Input Maps"), text(hint).size(look.small_size).color(c.dim)].spacing(3).padding(10);
    let item = move |name: String, has: bool, on: Option<M>| {
        let mark: Element<'a, M> =
            if has { container(space()).width(7).height(7).style(style::dot(c.lit)).into() } else { container(space()).width(7).height(7).into() };
        button(row![mark, text(name).size(look.text_size)].spacing(8).align_y(Alignment::Center))
            .width(Length::Fill)
            .padding([5, 8])
            .style(style::item(&look))
            .on_press_maybe(on)
    };
    let mut entries = column![].spacing(1);
    for usage in Usage::ALL {
        entries = entries.push(item(usage.name().into(), content.has.contains(&usage), assign(Some(usage), false)));
    }
    entries = entries.push(space().height(8)).push(text("Converted").size(look.small_size).color(c.faint));
    entries = entries.push(item("Smoothness (1 − Roughness)".into(), content.has.contains(&Usage::Roughness), assign(Some(Usage::Roughness), true)));
    entries = entries.push(item("Glossiness (1 − Roughness)".into(), content.has.contains(&Usage::Roughness), assign(Some(Usage::Roughness), true)));
    entries = entries.push(space().height(8)).push(text("Other").size(look.small_size).color(c.faint));
    entries = entries.push(item("A value".into(), false, assign(None, false)));
    list = list.push(scroll_box(entries).height(Length::Fill));
    list = list.push(
        row![container(space()).width(7).height(7).style(style::dot(c.lit)), text("the graph has it").size(look.small_size).color(c.faint)]
            .spacing(6)
            .align_y(Alignment::Center),
    );
    list.into()
}
