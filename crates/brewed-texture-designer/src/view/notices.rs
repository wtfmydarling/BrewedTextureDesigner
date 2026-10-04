//! Help → Third-Party Notices: the works of others the program is made with, each under its
//! license (see [`crate::third_party`]), laid out as the settings are: on the left, the works,
//! searched, under their kinds; on the right, the one chosen: who made it, its license, where it
//! is, what it is for (and what was changed of it), and the texts of its notices and licenses.

use iced::widget::{button, center, column, container, mouse_area, opaque, row, scrollable, space, text, text_input};
use iced::{Alignment, Element, Length, Padding};

use crate::app::{Message, NoticesMessage};
use crate::style;
use crate::third_party::{Kind, Work, notices};

/// The window's state: the work chosen (of [`notices`]'s works) and what is searched for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NoticesDialog {
    pub chosen: usize,
    pub query: String,
}

/// Whether `work` is found by `query`: each of its words in the work's name, license, makers or
/// what it says of itself (the crates an entry is of).
pub fn matches(work: &Work, query: &str) -> bool {
    let said = format!("{} {} {} {}", work.name, work.license, work.by, work.note).to_lowercase();
    query.split_whitespace().all(|q| said.contains(&q.to_lowercase()))
}

pub fn view(dialog: &NoticesDialog) -> Element<'_, Message> {
    let n = notices();
    let say = Message::Notices;

    // The works the search leaves, under their kinds.
    let mut list = column![].spacing(1).padding([4, 8]);
    let mut any = false;
    for kind in Kind::ALL {
        let found: Vec<usize> = (0..n.works.len()).filter(|&i| n.works[i].kind == kind && matches(&n.works[i], &dialog.query)).collect();
        if found.is_empty() {
            continue;
        }
        any = true;
        list = list.push(
            container(text(format!("{} · {}", kind.heading(), found.len())).size(style::TEXT_SIZE).font(style::heavy()).color(style::TEXT_BRIGHT))
                .padding(Padding { top: 10.0, right: 6.0, bottom: 4.0, left: 6.0 }),
        );
        for i in found {
            let w = &n.works[i];
            let chosen = i == dialog.chosen;
            let label = row![
                text(w.name.as_str()).size(style::TEXT_SIZE).font(style::chosen_font(chosen)).wrapping(text::Wrapping::None),
                space().width(Length::Fill),
                text(w.version.as_str()).size(style::SMALL).color(style::TEXT_DIM).wrapping(text::Wrapping::None),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            list = list.push(button(label).width(Length::Fill).padding([5, 10]).style(style::nav_tab(chosen)).on_press(say(NoticesMessage::Choose(i))));
        }
    }
    if !any {
        list = list.push(container(text("Nothing matches").size(style::SMALL).color(style::TEXT_DIM)).padding(8));
    }
    let search = text_input("Search…", &dialog.query)
        .on_input(|q| Message::Notices(NoticesMessage::Search(q)))
        .padding([5, 8])
        .size(style::TEXT_SIZE)
        .style(style::text_input);
    let sidebar = container(column![container(search).padding(Padding { top: 10.0, right: 8.0, bottom: 4.0, left: 8.0 }), scrollable(list).height(Length::Fill).style(style::scrollable)])
        .width(Length::Fixed(300.0))
        .height(Length::Fill)
        .style(style::sidebar);

    // The one chosen.
    let w = n.works.get(dialog.chosen).unwrap_or(&n.works[0]);
    let mut said = Vec::new();
    if !w.version.is_empty() {
        said.push(format!("Version {}", w.version));
    }
    if !w.license.is_empty() {
        said.push(w.license.clone());
    }
    let mut content = column![
        text(w.name.as_str()).size(20).font(style::heavy()).color(style::TEXT_BRIGHT),
        text(said.join("  ·  ")).size(style::TEXT_SIZE).color(style::TEXT_DIM),
    ]
    .spacing(6)
    .padding(Padding { top: 18.0, right: 22.0, bottom: 22.0, left: 22.0 });
    let field = |label: &'static str, value: &'static str| -> Element<'static, Message> {
        row![text(label).size(style::TEXT_SIZE).color(style::TEXT_DIM).width(Length::Fixed(64.0)), text(value).size(style::TEXT_SIZE)].spacing(8).into()
    };
    if !w.by.is_empty() {
        content = content.push(field("By", w.by.as_str()));
    }
    if !w.link.is_empty() {
        content = content.push(field("Where", w.link.as_str()));
    }
    if !w.note.is_empty() {
        content = content.push(container(text(w.note.as_str()).size(style::TEXT_SIZE)).padding(Padding { top: 6.0, ..Padding::ZERO }));
    }
    for t in &w.texts {
        content = content.push(
            container(text(n.texts[*t].as_str()).size(style::SMALL).font(style::mono()))
                .padding(12)
                .width(Length::Fill)
                .style(style::panel_dark),
        );
    }
    let content = scrollable(content).height(Length::Fill).width(Length::Fill).style(style::scrollable);

    let head = column![
        text("Third-Party Notices").size(16).font(style::heavy()).color(style::TEXT_BRIGHT),
        text("The program is made with these works of others, each used under its license: what each license asks to be said of it is said here.")
            .size(style::TEXT_SIZE)
            .color(style::TEXT_DIM),
    ]
    .spacing(6)
    .padding([12, 18]);
    let footer = row![
        text(format!("{} works", n.works.len())).size(style::SMALL).color(style::TEXT_FAINT),
        space().width(Length::Fill),
        button(text("Close").size(style::TEXT_SIZE)).padding([6, 14]).style(style::button).on_press(Message::Dialog(None)),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .padding([10, 18]);
    let window = column![
        head,
        crate::view::hairline(),
        row![sidebar, container(space()).width(1).height(Length::Fill).style(style::hairline(style::LINE)), content].height(Length::Fill),
        crate::view::hairline(),
        footer,
    ];
    let dialog = container(window).width(Length::Fill).max_width(980).height(Length::Fill).max_height(680).style(style::popup);
    // Nothing is lost by a click beside it: it closes.
    opaque(
        mouse_area(container(center(opaque(dialog))).padding(24).width(Length::Fill).height(Length::Fill).style(style::scrim))
            .on_press(Message::Dialog(None)),
    )
}
