//! Help → Third-Party Notices: the works of others the program is made with (see
//! [`crate::third_party`]), as programs list them: each its name and its license, a link to where
//! it is; searched, under their kinds.

use iced::widget::{button, center, column, container, mouse_area, opaque, row, space, text, text_input};
use iced::{Alignment, Element, Length, Padding};

use crate::app::{Message, NoticesMessage};
use crate::icons::{Icon, icon};
use crate::style;
use crate::widget::scroll::scroll_box;
use crate::third_party::{Kind, Work, works};

/// The window's state: what is searched for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NoticesDialog {
    pub query: String,
}

/// Whether `work` is found by `query`: each of its words in the work's name or license.
pub fn matches(work: &Work, query: &str) -> bool {
    let said = format!("{} {}", work.name, work.license).to_lowercase();
    query.split_whitespace().all(|q| said.contains(&q.to_lowercase()))
}

pub fn view(dialog: &NoticesDialog) -> Element<'_, Message> {
    let works = works();
    // The works the search leaves, under their kinds: each kind a list of rows in a frame, as
    // the settings are, every other row a little lighter; each row a link to where it is.
    let mut list = column![].padding(Padding { top: 0.0, right: 14.0, bottom: 14.0, left: 14.0 });
    let mut any = false;
    for kind in Kind::ALL {
        let found: Vec<&Work> = works.iter().filter(|w| w.kind == kind && matches(w, &dialog.query)).collect();
        if found.is_empty() {
            continue;
        }
        any = true;
        list = list.push(
            container(text(format!("{} · {}", kind.heading(), found.len())).size(14).font(style::heavy()).color(style::TEXT_BRIGHT))
                .padding(Padding { top: 16.0, right: 0.0, bottom: 8.0, left: 2.0 }),
        );
        let mut rows = column![].spacing(1);
        for (i, w) in found.into_iter().enumerate() {
            let label = row![
                text(w.name.as_str()).size(style::TEXT_SIZE),
                text(format!("[{}]", w.license)).size(style::TEXT_SIZE).color(style::TEXT_DIM),
                space().width(Length::Fill),
                icon(Icon::Link, 13.0, style::TEXT_DIM),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            let link = button(label).width(Length::Fill).padding([7, 12]).style(style::link_row(i % 2 == 0)).on_press(Message::Notices(NoticesMessage::Open(w.link.clone())));
            rows = rows.push(crate::view::tip(link, w.link.as_str(), iced::widget::tooltip::Position::Bottom));
        }
        list = list.push(container(rows).padding(1).width(Length::Fill).style(style::setting_list));
    }
    if !any {
        list = list.push(container(text("Nothing matches").size(style::SMALL).color(style::TEXT_DIM)).padding(Padding { top: 16.0, ..Padding::ZERO }));
    }
    let search = text_input("Search…", &dialog.query)
        .on_input(|q| Message::Notices(NoticesMessage::Search(q)))
        .padding([5, 8])
        .size(style::TEXT_SIZE)
        .style(style::text_input);
    let head = column![
        text("Third-Party Notices").size(16).font(style::heavy()).color(style::TEXT_BRIGHT),
        text("The program is made with these works of others, each under its license. A click opens where it is.")
            .size(style::TEXT_SIZE)
            .color(style::TEXT_DIM),
    ]
    .spacing(6)
    .padding([12, 18]);
    let footer = row![
        text(format!("{} works", works.len())).size(style::SMALL).color(style::TEXT_FAINT),
        space().width(Length::Fill),
        button(text("Close").size(style::TEXT_SIZE)).padding([6, 14]).style(style::button).on_press(Message::Dialog(None)),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .padding([10, 18]);
    let window = column![
        head,
        container(search).padding(Padding { top: 0.0, right: 18.0, bottom: 8.0, left: 18.0 }),
        crate::view::hairline(),
        // (A well darker than the window, the lists lighter than it.)
        container(scroll_box(list).height(Length::Fill)).height(Length::Fill).style(style::panel_dark),
        crate::view::hairline(),
        footer,
    ];
    let dialog = container(window).width(Length::Fill).max_width(640).height(Length::Fill).max_height(680).style(style::popup);
    // Nothing is lost by a click beside it: it closes.
    opaque(
        mouse_area(container(center(opaque(dialog))).padding(24).width(Length::Fill).height(Length::Fill).style(style::scrim))
            .on_press(Message::Dialog(None)),
    )
}
