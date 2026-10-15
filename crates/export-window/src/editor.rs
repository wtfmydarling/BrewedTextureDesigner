//! OUTPUT TEMPLATES, as Substance Painter's: the presets on the left; the output maps of the one
//! shown in the middle, each a row: its handle (it is moved by it), its name (and the parts a name
//! may have, from the `$` button) and the name it makes, its format, a copy of it, and under them
//! its channels, each a box of the same size; the input maps on the right, dragged onto the
//! channels. A channel pressed shows what it is made of under its row. Every preset is changed
//! where it is shown, the program's too (each then made again as it was with Reset).

use iced::widget::{Column, Row, button, column, container, mouse_area, row, space, text, text_input};
use iced::{Alignment, Element, Length};
use scroll_box::scroll_box;
use texture_export::{Edit, FileType, Format, Layout, MapId, Missing, Normals, Part, Template};

use crate::{Chosen, Content, Dragged, Event, Look, Menu, Offer, State, caption, check, choice, still, style};

/// How wide and high the box of a channel is: of one channel or three, the same.
const CHANNEL: (f32, f32) = (136.0, 40.0);
/// How wide an output map's name is, and each of its kind of file and depth.
const NAME: f32 = 230.0;
const FORMAT: f32 = 104.0;

/// The tags of the channels of `file`'s parts: `RGB`, `A`; `Gray` of a file of one channel.
fn tags(file: &texture_export::File) -> Vec<String> {
    if file.channels() == 1 {
        return vec!["Gray".into()];
    }
    let mut letters = ["R", "G", "B", "A"].into_iter();
    file.parts.iter().map(|p| (0..p.channels()).filter_map(|_| letters.next()).collect()).collect()
}

fn normals_said(normals: Normals) -> &'static str {
    match normals {
        Normals::OpenGL => "OpenGL",
        Normals::DirectX => "DirectX",
    }
}

/// What a part is made of, said in its channel: the map's name (as the input maps list it; a
/// normal map with the way it is written, inverted where it is), or a value; empty where no map
/// is given yet.
fn label(part: &Part, template: &Template, names: &dyn Fn(&MapId) -> String) -> (String, bool) {
    match part {
        Part::Value { said, .. } if said.is_empty() => (String::new(), true),
        Part::Value { value, .. } => (format!("Constant {value}"), false),
        Part::Rgb(input) | Part::One(input) => {
            let mut said = names(&input.map);
            if input.map.is_normal() {
                said = format!("{said} {}", normals_said(input.normals.unwrap_or(template.normals)));
            }
            if input.invert {
                said = format!("{said}, inverted");
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
        container(maps(content, state, editable, &look, say)).width(Length::Fixed(240.0)).height(Length::Fill).style(style::sunken(&look)),
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

/// The preset shown: its name, its output maps.
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
                .width(Length::Fixed(NAME))
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
    let mut rows = Column::new().spacing(6);
    for (f, file) in template.files.iter().enumerate() {
        rows = rows.push(output_row(content, state, editable, template, f, file, &look, say));
    }
    if template.files.is_empty() {
        rows = rows.push(text("No output maps: create one above.").size(look.small_size).color(c.dim));
    }
    let hint = "Drag maps from the right onto the channels; drag an output map by its handle to move it.";
    let said = match template.files.len() {
        1 => "1 output map will be exported.".to_string(),
        n => format!("{n} output maps will be exported."),
    };
    let foot = column![
        text(hint).size(look.small_size).color(c.dim),
        container(text(said).size(look.small_size).color(c.text)).padding([4, 8]).width(Length::Fill).style(style::well(&look))
    ]
    .spacing(4);
    let line = container(space()).height(1).width(Length::Fill).style(style::line(&look));
    column![container(head).padding([2, 4]), line, create, scroll_box(rows).height(Length::Fill), foot].spacing(6).into()
}

/// An output map: its handle, its name and the name it makes, its format, a copy and away; under
/// them its channels; under those the parts of a name where asked, and what the channel chosen
/// is made of.
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
    // The first line: the name and its `$`; the format; a copy; away.
    let mut first: Row<'a, M> = row![].spacing(6).align_y(Alignment::Center);
    match editable {
        Some(i) => {
            let placeholder = content.tokens.first().map_or_else(|| "BaseColor".to_string(), |t| format!("{}_BaseColor", t.written()));
            first = first.push(
                text_input(&placeholder, &file.name)
                    .width(Length::Fixed(NAME))
                    .size(look.small_size)
                    .font(look.mono)
                    .padding([5, 8])
                    .style(look.text_input)
                    .on_input(move |s| say(Event::Edit(i, Edit::FileName { file: f, name: s }))),
            );
            let open = state.tokens == Some(f);
            first = first.push(
                button(text("$").size(look.text_size).font(look.bold).center())
                    .width(Length::Fixed(28.0))
                    .padding([3, 0])
                    .style(style::opener(&look, open))
                    .on_press(say(Event::Tokens(if open { None } else { Some(f) }))),
            );
        }
        None => first = first.push(container(text(file.name.clone()).size(look.small_size).font(look.mono)).width(Length::Fixed(NAME)).padding([5, 8])),
    }
    first = first.push(space().width(Length::Fill));
    first = first.push(text("Format").size(look.small_size).color(c.dim));
    match editable {
        Some(i) => {
            let to = move |format: Option<Format>| say(Event::Edit(i, Edit::Format { file: f, format }));
            let kind_of = file.format.map(Format::file_type);
            let kinds = FileType::ALL.into_iter().map(|k| Offer::new(k.name(), to(Some(k.formats()[0])), kind_of == Some(k))).collect();
            first = first.push(choice(
                &look,
                state,
                Menu::Kind(f),
                kind_of.map_or("Export settings", FileType::name).into(),
                kind_of.is_none(),
                FORMAT,
                vec![vec![Offer::new("Export settings", to(None), kind_of.is_none())], kinds],
                say,
            ));
            first = first.push(match file.format {
                Some(format) if format.file_type().formats().len() > 1 => {
                    let depths = format.file_type().formats().iter().map(|d| Offer::new(d.depth(), to(Some(*d)), *d == format)).collect();
                    choice(&look, state, Menu::FileDepth(f), format.depth().into(), false, FORMAT, vec![depths], say)
                }
                Some(format) => still(&look, format.depth().into(), FORMAT),
                None => still(&look, content.global_depth(), FORMAT),
            });
            first = first.push(small(&look, "Duplicate", Some(say(Event::Edit(i, Edit::DuplicateFile(f))))));
            first =
                first.push(button(text("✕").size(look.small_size)).padding([3, 7]).style(look.flat_button).on_press(say(Event::Edit(i, Edit::RemoveFile(f)))));
        }
        None => {
            let format = file.format.map_or_else(|| "Export settings".to_string(), |f| format!("{} {}", f.file_type().name(), f.depth()));
            first = first.push(text(format).size(look.small_size).color(c.dim));
        }
    }
    // The name it makes now (a preview); parts of a name not known here.
    let extension = file.format.map(Format::extension).or(content.global_extension());
    let values: Vec<(&str, &str)> = content.values.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let written = texture_export::naming::expand(&file.name, &values);
    let shown = extension.map_or_else(|| written.clone(), |e| format!("{written}.{e}"));
    let mut preview = row![text("Preview:").size(look.caption_size).color(c.faint), text(shown).size(look.caption_size).font(look.mono).color(c.dim)]
        .spacing(6)
        .align_y(Alignment::Center);
    let unknown = texture_export::naming::unknown_parts(&file.name, content.tokens);
    if !unknown.is_empty() {
        preview = preview.push(text(format!("{} stands for nothing here", unknown.join(", "))).size(look.caption_size).color(c.warning));
    }
    // The channels: a map dragged is dropped on them; pressed, what they are made of shows. Each
    // a box of the same size.
    let mut channels: Row<'a, M> = row![].spacing(6).align_y(Alignment::Center);
    for (p, (part, tag)) in file.parts.iter().zip(tags(file)).enumerate() {
        let (said, empty) = label(part, template, &names);
        let chosen = state.slot == Some((f, p));
        let over = state.over == Some((f, p)) && state.dragging.is_some();
        let face = column![
            text(tag.clone()).size(look.small_size).font(look.bold).color(style::channel_color(&tag)),
            text(if empty { "—".to_string() } else { said }).size(look.caption_size).wrapping(text::Wrapping::None),
        ]
        .spacing(1)
        .align_x(Alignment::Center);
        let cell = container(face).center_x(CHANNEL.0).center_y(CHANNEL.1).clip(true).style(style::channel(&look, &tag, chosen, over, empty));
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
    let mut body = column![first, preview, channels].spacing(6);
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
        body = body.push(container(menu).padding(4).width(Length::Fixed(560.0)).style(style::menu(&look)));
    }
    // What the channel chosen is made of.
    if let (Some(i), Some((at, p))) = (editable, state.slot)
        && at == f
        && let (Some(part), Some(tag)) = (file.parts.get(p), tags(file).get(p))
    {
        body = body.push(options(i, f, p, part, tag, template, content, state, &look, say));
    }
    // Its handle, on its left as high as it is: pressed, it is moved; let go over another output
    // map, it goes where that one is.
    let hovered = state.hovered.as_deref() == Some(&format!("handle:{f}"));
    let grip = container(text("⋮").size(look.text_size).color(if hovered { c.bright } else { c.dim }))
        .center_x(14)
        .center_y(Length::Fill)
        .style(style::handle(&look, hovered));
    let mut handle = mouse_area(grip).on_enter(say(Event::Hover(format!("handle:{f}")))).on_exit(say(Event::Unhover(format!("handle:{f}"))));
    if editable.is_some() {
        handle = handle.on_press(say(Event::Grab(f))).interaction(iced::mouse::Interaction::Grab);
    }
    let lifted = state.moving == Some(f);
    let target = state.moving.is_some_and(|m| m != f) && state.over_row == Some(f);
    let whole = container(row![handle, body.width(Length::Fill)].spacing(8).height(Length::Shrink))
        .padding([6, 8])
        .width(Length::Fill)
        .style(style::output_row(&look, lifted, target));
    let mut area = mouse_area(whole);
    if let (Some(i), Some(from)) = (editable, state.moving)
        && from != f
    {
        area = area.on_enter(say(Event::OverRow(Some(f)))).on_exit(say(Event::OverRow(None))).on_release(say(Event::Edit(i, Edit::MoveFile { from, to: f })));
    }
    area.into()
}

/// What a channel is made of, under its row: which map (a constant, or none yet), which channel
/// of it, inverted or not, and what is written where what is exported has no such map.
#[allow(clippy::too_many_arguments)]
fn options<'a, M: Clone + 'a>(
    i: Chosen,
    f: usize,
    p: usize,
    part: &Part,
    tag: &str,
    template: &Template,
    content: &Content<'a>,
    state: &'a State,
    look: &Look,
    say: impl Fn(Event) -> M + Copy + 'a,
) -> Element<'a, M> {
    let look = *look;
    let c = look.colors;
    let edit = move |e: Edit| say(Event::Edit(i, e));
    let still_message = say(Event::Slot(state.slot));
    let seg = move |options: Vec<(String, M, bool)>| {
        let mut r = row![].spacing(0);
        for (name, message, chosen) in options {
            r = r.push(
                button(text(name).size(look.small_size).font(if chosen { look.bold } else { look.font }))
                    .padding([2, 8])
                    .style(move |t, s| (look.segment)(t, s, chosen))
                    .on_press(message),
            );
        }
        container(r).style(look.segments)
    };
    let number = move |value: f32, make: Box<dyn Fn(f32) -> Edit + 'a>| {
        let still_message = still_message.clone();
        text_input("0 … 1", &format!("{value}"))
            .width(Length::Fixed(64.0))
            .size(look.small_size)
            .padding([2, 6])
            .style(look.text_input)
            .on_input(move |s| s.trim().parse::<f32>().ok().filter(|v| v.is_finite()).map_or_else(|| still_message.clone(), |v| edit(make(v))))
    };
    let label = |s: String| text(s).size(look.small_size).color(c.dim).width(Length::Fixed(210.0));
    let names = |map: &MapId| {
        content
            .inputs
            .iter()
            .find(|i| &i.map == map)
            .map(|i| i.name.clone())
            .or_else(|| content.custom.iter().find(|(m, _)| m == map).map(|(_, n)| n.clone()))
            .unwrap_or_else(|| map.word())
    };
    let (made_of, _) = super_label(part, template, &names);
    let mut lines = column![].spacing(6);
    // Which channel, of what; the map gone from it.
    let mut head =
        row![text(format!("Channel {tag}")).size(look.small_size).font(look.bold).color(c.bright), text(made_of).size(look.small_size).color(c.text)]
            .spacing(8)
            .align_y(Alignment::Center);
    head = head.push(space().width(Length::Fill));
    match part {
        Part::Value { said, .. } if said.is_empty() => {
            head = head.push(small(&look, "Use a constant", Some(edit(Edit::Map { file: f, part: p, map: None, invert: false, normals: None }))));
        }
        _ => head = head.push(small(&look, "Remove", Some(edit(Edit::Clear { file: f, part: p })))),
    }
    lines = lines.push(head);
    match part {
        Part::Value { said, .. } if said.is_empty() => {
            lines = lines.push(text("Drag a map onto it from the right, or use a constant.").size(look.small_size).color(c.faint));
        }
        Part::Value { value, .. } => {
            lines = lines
                .push(row![label("Value".into()), number(*value, Box::new(move |value| Edit::Value { file: f, part: p, value }))].align_y(Alignment::Center));
        }
        Part::Rgb(input) | Part::One(input) => {
            let mut taken = row![].spacing(16).align_y(Alignment::Center);
            if matches!(part, Part::One(_)) {
                let channels = ["R", "G", "B", "A"]
                    .iter()
                    .enumerate()
                    .map(|(n, s)| (s.to_string(), edit(Edit::Channel { file: f, part: p, channel: n as u8 }), usize::from(input.channel) == n))
                    .collect();
                taken = taken.push(row![label("Take from channel".into()), seg(channels)].align_y(Alignment::Center));
            }
            taken = taken.push(
                row![
                    check(&look, input.invert, edit(Edit::Invert { file: f, part: p, invert: !input.invert })),
                    text("Invert").size(look.small_size).color(c.text)
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            );
            lines = lines.push(taken);
            let value = input.missing.unwrap_or(0.0);
            let mut missing = row![
                label(format!("If the {} has no {}", content.thing, names(&input.map))),
                seg(vec![
                    ("Skip the file".into(), edit(Edit::Missing { file: f, part: p, missing: Missing::NoFile }), input.missing.is_none() && !input.optional),
                    ("Leave the channel out".into(), edit(Edit::Missing { file: f, part: p, missing: Missing::LeaveOut }), input.optional),
                    ("Fill with".into(), edit(Edit::Missing { file: f, part: p, missing: Missing::Value(value) }), input.missing.is_some()),
                ]),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            if input.missing.is_some() {
                missing = missing.push(number(value, Box::new(move |value| Edit::Missing { file: f, part: p, missing: Missing::Value(value) })));
            }
            lines = lines.push(missing);
        }
    }
    container(lines).padding([8, 10]).width(Length::Fill).style(style::menu(&look)).into()
}

/// What a part is made of, said in full (as its channel says it).
fn super_label(part: &Part, template: &Template, names: &dyn Fn(&MapId) -> String) -> (String, bool) {
    match label(part, template, names) {
        (_, true) => ("empty".into(), true),
        said => said,
    }
}

/// The input maps: those of the program's usages, those of no usage the graph has, those made of
/// others; each dragged onto a channel (pressed and moved: a click takes nothing).
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
    let entry = move |dragged: Dragged, has: bool, uses: bool, color: Option<iced::Color>| -> Element<'a, M> {
        let mark: Element<'a, M> =
            if uses { container(space()).width(9).height(9).style(style::swatch(c.bright)).into() } else { container(space()).width(9).height(9).into() };
        let tint = color.unwrap_or(if has { c.bright } else { c.dim });
        let key = dragged.key();
        let hovered = state.hovered.as_ref() == Some(&key);
        let face = container(row![mark, text(dragged.label.clone()).size(look.small_size).color(tint)].spacing(7).align_y(Alignment::Center))
            .padding([3, 8])
            .width(Length::Fill)
            .style(style::map_tile(&look, hovered && state.dragging.is_none()));
        // (A box of its own, lit under the mouse: plain which is taken.)
        let mut area = mouse_area(face).on_enter(say(Event::Hover(key.clone()))).on_exit(say(Event::Unhover(key)));
        if editable.is_some() {
            area = area.on_press(say(Event::Press(dragged))).interaction(iced::mouse::Interaction::Grab);
        }
        area.into()
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
        ("Glossiness (inverted Roughness)", "roughness", true, None),
        ("Smoothness (inverted Roughness)", "roughness", true, None),
        ("Roughness (inverted Glossiness)", "glossiness", true, None),
    ];
    for (name, map, invert, normals) in made {
        let id = MapId::from(map);
        let dragged = Dragged { map: Some(id.clone()), invert, normals, label: name.into() };
        converted = converted.push(entry(dragged, content.has.contains(&id), false, Some(c.converted)));
    }
    let strip = |title: &'static str| container(caption(&look, title)).padding([5, 8]).width(Length::Fill).style(style::strip(&look));
    let all = column![list, strip("Custom maps"), custom, strip("Converted maps"), converted];
    column![strip("Input maps"), scroll_box(all).height(Length::Fill)].into()
}
