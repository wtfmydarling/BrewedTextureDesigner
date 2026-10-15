//! OUTPUT TEMPLATES, as Substance Painter's: the presets on the left; the output maps of the one
//! shown in the middle, each a row of its name (and the parts a name may have, from the `$`
//! button), its channels, its file type and depth; the input maps on the right, dragged onto
//! the channels. A channel pressed shows its options under its row. Every preset is changed
//! where it is shown, the program's too (each then made again as it was with Reset).

use iced::widget::{Column, Row, button, column, container, mouse_area, row, space, text, text_input};
use iced::{Alignment, Element, Length};
use scroll_box::scroll_box;
use texture_export::{Edit, FileType, Format, Layout, MapId, Missing, Normals, Part, Template};

use crate::{Chosen, Content, Dragged, Event, Look, Menu, Offer, State, caption, choice, style};

/// The tags of the channels of `file`'s parts: `RGB`, `A`; `Gray` of a file of one channel.
fn tags(file: &texture_export::File) -> Vec<String> {
    if file.channels() == 1 {
        return vec!["Gray".into()];
    }
    let mut letters = ["R", "G", "B", "A"].into_iter();
    file.parts.iter().map(|p| (0..p.channels()).filter_map(|_| letters.next()).collect()).collect()
}

/// What a part is made of, said in its channel: the map's name (as the input maps list it), or
/// a value; empty where no map is given yet.
fn label(part: &Part, names: &dyn Fn(&MapId) -> String) -> (String, bool) {
    match part {
        Part::Value { said, .. } if said.is_empty() => (String::new(), true),
        Part::Value { value, .. } => (format!("{value}"), false),
        Part::Rgb(input) | Part::One(input) => {
            let mut said = names(&input.map);
            if input.invert {
                said = format!("1 − {said}");
            }
            match input.normals {
                Some(Normals::DirectX) => said.push_str(" DX"),
                Some(Normals::OpenGL) => said.push_str(" GL"),
                None => {}
            }
            (said, false)
        }
    }
}

pub(crate) fn view<'a, M: Clone + 'a>(content: &Content<'a>, state: &'a State, look: &Look, say: impl Fn(Event) -> M + Copy + 'a) -> Element<'a, M> {
    let look = *look;
    // (Whatever is shown is changed there.)
    let editable = state.shown.template(&content.presets, content.mine).is_some().then_some(state.shown);
    row![
        container(presets(content, state, &look, say)).width(Length::Fixed(230.0)).height(Length::Fill).style(style::sunken(&look)),
        container(outputs(content, state, editable, &look, say)).width(Length::Fill).height(Length::Fill),
        container(maps(content, state, editable, &look, say)).width(Length::Fixed(230.0)).height(Length::Fill).style(style::sunken(&look)),
    ]
    .spacing(8)
    .padding(8)
    .into()
}

/// A small button of a strip or a row.
fn small<'a, M: Clone + 'a>(look: &Look, label: &str, on: Option<M>) -> iced::widget::Button<'a, M> {
    button(text(label.to_string()).size(look.small_size)).padding([2, 8]).style(look.button).on_press_maybe(on)
}

/// The presets: the program's and the user's, and what is done with them.
fn presets<'a, M: Clone + 'a>(content: &Content<'a>, state: &'a State, look: &Look, say: impl Fn(Event) -> M + Copy + 'a) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    let mine = match state.shown {
        Chosen::Mine(i) => Some(i),
        Chosen::Preset(_) => None,
    };
    let changed = match state.shown {
        Chosen::Preset(i) => content.presets.get(i).is_some_and(|p| p.changed).then_some(i),
        Chosen::Mine(_) => None,
    };
    let tools = row![
        small(&look, "Reset", changed.map(|i| say(Event::Reset(i)))),
        small(&look, "Copy", Some(say(Event::Duplicate(state.shown)))),
        small(&look, "−", mine.map(|i| say(Event::Deleting(Some(i))))),
        small(&look, "+", Some(say(Event::New))),
    ]
    .spacing(3);
    let strip = container(row![caption(&look, "Presets"), space().width(Length::Fill), tools].align_y(Alignment::Center))
        .padding([5, 8])
        .width(Length::Fill)
        .style(style::strip(&look));
    let mut list = column![].spacing(1).padding(4);
    if let Some(i) = state.deleting.filter(|&i| i < content.mine.len()) {
        let asked = column![
            text(format!("Delete \"{}\" and its file?", content.mine[i].name)).size(look.small_size).color(c.warning),
            row![small(&look, "Delete", Some(say(Event::Delete(i)))), small(&look, "Keep", Some(say(Event::Deleting(None))))].spacing(4),
        ]
        .spacing(4);
        list = list.push(container(asked).padding(6).width(Length::Fill).style(style::menu(&look)));
    }
    let item = move |name: String, note: &'static str, chosen: bool, to: Chosen| {
        let color = if note == "changed" { c.converted } else { c.faint };
        let face = row![text(name).size(look.text_size), text(note).size(look.small_size).color(color)].spacing(6).align_y(Alignment::Center);
        button(face).width(Length::Fill).padding([3, 8]).style(style::item(&look, chosen)).on_press(say(Event::Show(to)))
    };
    for (i, preset) in content.presets.iter().enumerate().filter(|(_, p)| p.template.is_some()) {
        let note = if preset.changed { "changed" } else { "" };
        list = list.push(item(preset.name.into(), note, state.shown == Chosen::Preset(i), Chosen::Preset(i)));
    }
    for (i, template) in content.mine.iter().enumerate() {
        list = list.push(item(template.name.clone(), "yours", state.shown == Chosen::Mine(i), Chosen::Mine(i)));
    }
    for problem in content.problems {
        list = list.push(text(format!("Not read: {problem}")).size(look.small_size).color(c.warning));
    }
    let kept = content.templates_folder.clone().map_or_else(|| "Your presets are not kept here.".into(), |f| format!("Your presets are kept in {f}"));
    let foot = column![
        small(&look, "Open presets folder", Some(say(Event::OpenTemplates))),
        text(kept).size(look.caption_size).color(c.faint).wrapping(text::Wrapping::WordOrGlyph),
    ]
    .spacing(4)
    .padding(8);
    column![strip, scroll_box(list).height(Length::Fill), foot].into()
}

/// The preset shown: its name, which way its normal maps point, its output maps.
fn outputs<'a, M: Clone + 'a>(
    content: &Content<'a>,
    state: &'a State,
    editable: Option<Chosen>,
    look: &Look,
    say: impl Fn(Event) -> M + Copy + 'a,
) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    let Some(template) = state.shown.template(&content.presets, content.mine) else {
        return text("No preset is shown.").size(look.text_size).color(c.dim).into();
    };
    // Its name: changed where it is the user's.
    let mut head: Row<'a, M> = row![text("Current preset:").size(look.text_size).color(c.dim)].spacing(8).align_y(Alignment::Center);
    head = match state.shown {
        Chosen::Mine(i) => head.push(
            text_input("Name", &template.name)
                .width(Length::Fixed(280.0))
                .size(look.text_size)
                .padding([4, 8])
                .style(look.text_input)
                .on_input(move |s| say(Event::Edit(Chosen::Mine(i), Edit::Name(s)))),
        ),
        // (A preset of the program's keeps its name: settings find it by it.)
        Chosen::Preset(_) => head.push(text(template.name.clone()).size(look.text_size).font(look.bold).color(c.bright)),
    };
    if let Chosen::Preset(i) = state.shown
        && content.presets.get(i).is_some_and(|p| p.changed)
    {
        head = head.push(text("changed").size(look.small_size).color(c.converted));
        head = head.push(small(&look, "Reset to default", Some(say(Event::Reset(i)))));
    }
    head = head.push(space().width(Length::Fill));
    let normals = |n: Normals| match n {
        Normals::OpenGL => "OpenGL (Y+)",
        Normals::DirectX => "DirectX (Y−)",
    };
    match editable {
        Some(i) => {
            head = head.push(text("Normal maps:").size(look.small_size).color(c.dim));
            let mut seg = row![].spacing(0);
            for n in [Normals::OpenGL, Normals::DirectX] {
                let chosen = template.normals == n;
                seg = seg.push(
                    button(text(normals(n)).size(look.small_size).font(if chosen { look.bold } else { look.font }))
                        .padding([3, 8])
                        .style(move |t, s| (look.segment)(t, s, chosen))
                        .on_press(say(Event::Edit(i, Edit::Normals(n)))),
                );
            }
            head = head.push(container(seg).style(look.segments));
        }
        None => head = head.push(text(format!("Normal maps: {}", normals(template.normals))).size(look.small_size).color(c.faint)),
    }

    // The output maps: a new one laid out as Painter lays them out.
    let mut create = row![caption(&look, "Output maps"), space().width(Length::Fill)].spacing(4).align_y(Alignment::Center);
    if let Some(i) = editable {
        create = create.push(text("Create:").size(look.small_size).color(c.dim));
        // (Named by the first part of a name the program has, and its place.)
        let name = format!("{}_{}", content.tokens.first().map_or_else(String::new, |t| t.written()), template.files.len() + 1);
        for layout in Layout::ALL {
            create = create.push(small(&look, layout.name(), Some(say(Event::Edit(i, Edit::AddFile { layout, name: name.clone() })))));
        }
    }
    let mut rows = Column::new().spacing(4);
    for (f, file) in template.files.iter().enumerate() {
        rows = rows.push(output_row(content, state, editable, template, f, file, &look, say));
    }
    if template.files.is_empty() {
        rows = rows.push(text("No output maps: create one above.").size(look.small_size).color(c.dim));
    }
    let hint = "Drag and drop maps on output's channels to set up your export.";
    let said = match template.files.len() {
        1 => "1 output map will be exported.".to_string(),
        n => format!("{n} output maps will be exported."),
    };
    let foot = column![
        text(hint).size(look.small_size).color(c.dim),
        container(text(said).size(look.small_size).color(c.text)).padding([4, 8]).width(Length::Fill).style(style::well(&look))
    ]
    .spacing(4);
    let line = container(space())
        .height(1)
        .width(Length::Fill)
        .style(move |_: &iced::Theme| container::Style { background: Some(c.line.into()), ..Default::default() });
    column![container(head).padding([2, 4]), line, create, scroll_box(rows).height(Length::Fill), foot].spacing(6).into()
}

/// An output map: its name, channels, file type and depth; under it, what it is written as, the
/// parts of a name where asked, and the options of its channel chosen.
#[allow(clippy::too_many_arguments)]
fn output_row<'a, M: Clone + 'a>(
    content: &Content<'a>,
    state: &'a State,
    editable: Option<Chosen>,
    template: &Template,
    f: usize,
    file: &texture_export::File,
    look: &Look,
    say: impl Fn(Event) -> M + Copy + 'a,
) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    let names = |map: &MapId| {
        content
            .inputs
            .iter()
            .find(|i| &i.map == map)
            .map(|i| i.name.clone())
            .or_else(|| content.custom.iter().find(|(m, _)| m == map).map(|(_, n)| n.clone()))
            .unwrap_or_else(|| map.word())
    };
    // The handle, the name and its `$`.
    let handle = container(space())
        .width(4)
        .height(Length::Fixed(40.0))
        .style(move |_: &iced::Theme| container::Style { background: Some(c.edge.into()), ..Default::default() });
    let mut line: Row<'a, M> = row![handle].spacing(6).align_y(Alignment::Center);
    match editable {
        Some(i) => {
            let placeholder = content.tokens.first().map_or_else(|| "BaseColor".to_string(), |t| format!("{}_BaseColor", t.written()));
            line = line.push(
                text_input(&placeholder, &file.name)
                    .width(Length::Fixed(176.0))
                    .size(look.small_size)
                    .font(look.mono)
                    .padding([4, 6])
                    .style(look.text_input)
                    .on_input(move |s| say(Event::Edit(i, Edit::FileName { file: f, name: s }))),
            );
            let open = state.tokens == Some(f);
            line = line.push(
                button(text("$").size(look.text_size).font(look.bold).center())
                    .width(Length::Fixed(26.0))
                    .padding([3, 0])
                    .style(move |t, s| (look.segment)(t, s, open))
                    .on_press(say(Event::Tokens(if open { None } else { Some(f) }))),
            );
        }
        None => {
            line = line.push(
                container(text(file.name.clone()).size(look.small_size).font(look.mono))
                    .width(Length::Fixed(208.0))
                    .padding([4, 6])
                    .style(style::well(&look))
                    .clip(true),
            )
        }
    }
    // The channels: a map dragged is dropped on them; pressed, their options show. They share the
    // room the row has: a channel two parts of six, three channels three; what is left is left empty.
    let mut channels: Row<'a, M> = row![].spacing(4).width(Length::Fill).align_y(Alignment::Center);
    let mut used = 0;
    for (p, (part, tag)) in file.parts.iter().zip(tags(file)).enumerate() {
        let (said, empty) = label(part, &names);
        let chosen = state.slot == Some((f, p));
        let over = state.over == Some((f, p)) && state.dragging.is_some();
        let parts = if part.channels() == 3 { 3 } else { 2 };
        used += parts;
        let face = column![
            text(tag.clone()).size(look.small_size).font(look.bold).color(style::channel_color(&tag)),
            text(if empty { "—".to_string() } else { said }).size(look.caption_size).wrapping(text::Wrapping::None),
        ]
        .spacing(1)
        .align_x(Alignment::Center);
        let cell = container(face).center_x(Length::FillPortion(parts)).center_y(40).clip(true).style(style::channel(&look, &tag, chosen, over, empty));
        let mut area = mouse_area(cell);
        if let Some(i) = editable {
            match &state.dragging {
                Some(dragged) => {
                    let edit = Edit::Map { file: f, part: p, map: dragged.map.clone(), invert: dragged.invert, normals: dragged.normals };
                    area = area.on_enter(say(Event::Over(Some((f, p))))).on_exit(say(Event::Over(None))).on_release(say(Event::Edit(i, edit)));
                }
                None => area = area.on_press(say(Event::Slot(if chosen { None } else { Some((f, p)) }))).interaction(iced::mouse::Interaction::Pointer),
            }
        }
        channels = channels.push(area);
    }
    if used < 6 {
        channels = channels.push(space().width(Length::FillPortion(6 - used)));
    }
    line = line.push(channels);
    // The file type and depth: as the export sets them, or its own.
    match editable {
        Some(i) => {
            let to = move |format: Option<Format>| say(Event::Edit(i, Edit::Format { file: f, format }));
            let kind_of = file.format.map(Format::file_type);
            let kinds = FileType::ALL.into_iter().map(|k| Offer::new(k.name(), to(Some(k.formats()[0])), kind_of == Some(k))).collect();
            let kind = choice(
                &look,
                state,
                Menu::Kind(f),
                kind_of.map_or("as set", FileType::name).into(),
                kind_of.is_none(),
                96.0,
                vec![vec![Offer::new("As set", to(None), kind_of.is_none())], kinds],
                say,
            );
            let depth: Element<'a, M> = match file.format {
                Some(format) if format.file_type().formats().len() > 1 => {
                    let depths = format.file_type().formats().iter().map(|d| Offer::new(d.depth(), to(Some(*d)), *d == format)).collect();
                    choice(&look, state, Menu::FileDepth(f), format.depth().into(), false, 120.0, vec![depths], say)
                }
                Some(format) => container(text(format.depth()).size(look.small_size).color(c.dim)).width(Length::Fixed(120.0)).padding([4, 8]).into(),
                None => container(text(content.global_depth()).size(look.small_size).color(c.faint)).width(Length::Fixed(120.0)).padding([4, 8]).into(),
            };
            line = line.push(column![kind, depth].spacing(2).align_x(Alignment::End));
            line = line.push(
                column![
                    small(&look, "↑", (f > 0).then(|| say(Event::Edit(i, Edit::MoveFile { from: f, to: f - 1 })))),
                    small(&look, "↓", (f + 1 < template.files.len()).then(|| say(Event::Edit(i, Edit::MoveFile { from: f, to: f + 1 })))),
                ]
                .spacing(2),
            );
            line =
                line.push(button(text("✕").size(look.small_size)).padding([3, 7]).style(look.flat_button).on_press(say(Event::Edit(i, Edit::RemoveFile(f)))));
        }
        None => {
            let format = file.format.map_or_else(|| "as set".to_string(), |f| format!("{} {}", f.file_type().name(), f.depth()));
            line = line.push(text(format).size(look.small_size).color(c.dim));
        }
    }
    let mut body = column![line].spacing(4);
    // The name it is written by now; parts of a name not known here.
    let extension = file.format.map(Format::extension).or(content.global_extension());
    let values: Vec<(&str, &str)> = content.values.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let written = texture_export::naming::expand(&file.name, &values);
    let shown = extension.map_or_else(|| written.clone(), |e| format!("{written}.{e}"));
    let mut said = row![space().width(12), text(shown).size(look.caption_size).font(look.mono).color(c.faint)].spacing(8);
    let unknown = texture_export::naming::unknown_parts(&file.name, content.tokens);
    if !unknown.is_empty() {
        said = said.push(text(format!("{} stands for nothing here", unknown.join(", "))).size(look.caption_size).color(c.warning));
    }
    body = body.push(said);
    // The parts of a name, each said.
    if let (Some(i), true) = (editable, state.tokens == Some(f)) {
        let mut menu = column![].spacing(1);
        for token in content.tokens {
            let name = format!("{}{}", file.name, token.written());
            menu = menu.push(
                button(row![
                    text(token.written()).size(look.small_size).font(look.mono).color(c.bright).width(Length::Fixed(100.0)),
                    text(token.about).size(look.small_size).color(c.dim)
                ])
                .width(Length::Fill)
                .padding([3, 8])
                .style(style::item(&look, false))
                .on_press(say(Event::Edit(i, Edit::FileName { file: f, name }))),
            );
        }
        // What brackets do, with an example of the program's parts.
        let example = match content.tokens {
            [first, second, ..] => format!("T_{}(_{})", first.written(), second.written()),
            [first] => format!("T(_{})", first.written()),
            [] => String::new(),
        };
        if !example.is_empty() {
            menu = menu.push(
                row![
                    text("( … )").size(look.small_size).font(look.mono).color(c.bright).width(Length::Fixed(100.0)),
                    text(format!("Left out, with all in the brackets, where a part in them stands for nothing: {example}"))
                        .size(look.small_size)
                        .color(c.faint)
                ]
                .padding([3, 8]),
            );
        }
        body = body.push(row![space().width(12), container(menu).padding(4).width(Length::Fixed(520.0)).style(style::menu(&look))]);
    }
    // The options of the channel chosen.
    if let (Some(i), Some((at, p))) = (editable, state.slot)
        && at == f
        && let Some(part) = file.parts.get(p)
    {
        body = body.push(row![space().width(12), options(i, f, p, part, content.thing, state, &look, say)]);
    }
    container(body).padding([5, 6]).width(Length::Fill).style(style::raised(&look)).into()
}

/// The options of a channel: the map's channel, turned over, what is written where what is
/// exported has no such map; or its value. Emptied.
#[allow(clippy::too_many_arguments)]
fn options<'a, M: Clone + 'a>(
    i: Chosen,
    f: usize,
    p: usize,
    part: &Part,
    thing: &str,
    state: &'a State,
    look: &Look,
    say: impl Fn(Event) -> M + Copy + 'a,
) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    let edit = move |e: Edit| say(Event::Edit(i, e));
    let still = say(Event::Slot(state.slot));
    let seg = move |options: Vec<(&'static str, M, bool)>| {
        let mut r = row![].spacing(0);
        for (name, message, chosen) in options {
            r = r.push(
                button(text(name).size(look.small_size).font(if chosen { look.bold } else { look.font }))
                    .padding([2, 7])
                    .style(move |t, s| (look.segment)(t, s, chosen))
                    .on_press(message),
            );
        }
        container(r).style(look.segments)
    };
    let number = move |value: f32, make: Box<dyn Fn(f32) -> Edit + 'a>| {
        let still = still.clone();
        text_input("0…1", &format!("{value}"))
            .width(Length::Fixed(60.0))
            .size(look.small_size)
            .padding([2, 6])
            .style(look.text_input)
            .on_input(move |s| s.trim().parse::<f32>().ok().filter(|v| v.is_finite()).map_or_else(|| still.clone(), |v| edit(make(v))))
    };
    let label = |s: &'static str| text(s).size(look.small_size).color(c.dim);
    let mut strip: Row<'a, M> = row![].spacing(8).align_y(Alignment::Center);
    match part {
        Part::Value { value, .. } => {
            strip = strip.push(label("Value"));
            strip = strip.push(number(*value, Box::new(move |value| Edit::Value { file: f, part: p, value })));
        }
        Part::Rgb(input) | Part::One(input) => {
            if matches!(part, Part::One(_)) {
                strip = strip.push(label("Channel of the map"));
                let names = ["R", "G", "B", "A"];
                strip = strip.push(seg(names
                    .iter()
                    .enumerate()
                    .map(|(n, s)| (*s, edit(Edit::Channel { file: f, part: p, channel: n as u8 }), usize::from(input.channel) == n))
                    .collect()));
            }
            strip = strip.push(seg(vec![
                ("As it is", edit(Edit::Invert { file: f, part: p, invert: false }), !input.invert),
                ("1 − it", edit(Edit::Invert { file: f, part: p, invert: true }), input.invert),
            ]));
            strip = strip.push(text(format!("Where the {thing} has none")).size(look.small_size).color(c.dim));
            let value = input.missing.unwrap_or(0.0);
            strip = strip.push(seg(vec![
                ("No file", edit(Edit::Missing { file: f, part: p, missing: Missing::NoFile }), input.missing.is_none() && !input.optional),
                ("Left out", edit(Edit::Missing { file: f, part: p, missing: Missing::LeaveOut }), input.optional),
                ("A value", edit(Edit::Missing { file: f, part: p, missing: Missing::Value(value) }), input.missing.is_some()),
            ]));
            if input.missing.is_some() {
                strip = strip.push(number(value, Box::new(move |value| Edit::Missing { file: f, part: p, missing: Missing::Value(value) })));
            }
        }
    }
    strip = strip.push(small(&look, "Clear", Some(edit(Edit::Clear { file: f, part: p }))));
    container(strip).padding([4, 8]).style(style::menu(&look)).into()
}

/// The input maps: those of the program's usages, those of no usage the graph has, those made of
/// others; each dragged onto a channel.
fn maps<'a, M: Clone + 'a>(
    content: &Content<'a>,
    state: &'a State,
    editable: Option<Chosen>,
    look: &Look,
    say: impl Fn(Event) -> M + Copy + 'a,
) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    // The maps the preset shown is made of: marked.
    let used: Vec<MapId> = state
        .shown
        .template(&content.presets, content.mine)
        .map(|t| {
            t.files.iter().flat_map(|f| f.parts.iter()).filter_map(|p| if let Part::Rgb(i) | Part::One(i) = p { Some(i.map.clone()) } else { None }).collect()
        })
        .unwrap_or_default();
    let entry = move |dragged: Dragged, has: bool, uses: bool, color: Option<iced::Color>| {
        let mark: Element<'a, M> =
            if uses { container(space()).width(9).height(9).style(style::swatch(c.bright)).into() } else { container(space()).width(9).height(9).into() };
        let tint = color.unwrap_or(if has { c.bright } else { c.dim });
        let face = row![mark, text(dragged.label.clone()).size(look.small_size).color(tint)].spacing(7).align_y(Alignment::Center);
        // (A box of its own, lit under the mouse: plain which is taken.)
        let taken = button(face).width(Length::Fill).padding([3, 8]).style(style::map_box(&look)).on_press_maybe(editable.map(|_| say(Event::Drag(dragged))));
        mouse_area(taken).interaction(if editable.is_some() { iced::mouse::Interaction::Grab } else { iced::mouse::Interaction::default() })
    };
    let mut list = column![].spacing(2).padding([4, 4]);
    let mut group = String::new();
    for input in &content.inputs {
        if input.group != group {
            group = input.group.clone();
            list = list.push(container(text(group.clone()).size(look.caption_size).color(c.faint)).padding([6, 4]));
        }
        let dragged = Dragged { map: Some(input.map.clone()), invert: false, normals: None, label: input.name.clone() };
        list = list.push(entry(dragged, content.has.contains(&input.map), used.contains(&input.map), None));
    }
    let mut custom = column![].spacing(2).padding([4, 4]);
    for (map, name) in &content.custom {
        let dragged = Dragged { map: Some(map.clone()), invert: false, normals: None, label: name.clone() };
        custom = custom.push(entry(dragged, true, used.contains(map), None));
    }
    if content.custom.is_empty() {
        custom =
            custom.push(container(text(format!("The {} has no maps of usage Custom.", content.thing)).size(look.caption_size).color(c.faint)).padding([2, 8]));
    }
    let mut converted = column![].spacing(2).padding([4, 4]);
    let made = [
        ("Normal DirectX", "normal", false, Some(Normals::DirectX)),
        ("Normal OpenGL", "normal", false, Some(Normals::OpenGL)),
        ("Glossiness (1 − Roughness)", "roughness", true, None),
        ("Smoothness (1 − Roughness)", "roughness", true, None),
        ("Roughness (1 − Glossiness)", "glossiness", true, None),
    ];
    for (name, map, invert, normals) in made {
        let id = MapId::from(map);
        let dragged = Dragged { map: Some(id.clone()), invert, normals, label: name.into() };
        converted = converted.push(entry(dragged, content.has.contains(&id), false, Some(c.converted)));
    }
    converted = converted.push(entry(Dragged { map: None, invert: false, normals: None, label: "Value".into() }, true, false, Some(c.converted)));
    let strip = |title: &'static str| container(caption(&look, title)).padding([5, 8]).width(Length::Fill).style(style::strip(&look));
    let all = column![list, strip("Custom maps"), custom, strip("Converted maps"), converted];
    column![strip("Input maps"), scroll_box(all).height(Length::Fill)].into()
}
