//! The Home screen (see [`crate::app::home`]): on the left, its pages, New Package… and
//! Open…, and where to learn more; on the right, the packages opened last with their pictures
//! (Home) or the examples (Learn).

use std::time::SystemTime;

use iced::widget::{button, column, container, image, row, scrollable, space, text};
use iced::{Alignment, Element, Length, Padding};
use brewed_texture_core::templates::Example;

use crate::app::home::{HomeMessage, HomePage, NewGraphMessage, Recent};
use crate::app::{App, Dialog, Message, SettingsMessage};
use crate::icons::{Icon, icon};
use crate::style;

/// How big the pictures of the packages are.
const PICTURE: f32 = 150.0;
const SIDEBAR: f32 = 230.0;

fn say(message: HomeMessage) -> Message {
    Message::Home(message)
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

/// A page of the sidebar, lit when it is the one shown.
fn page_button<'a>(label: &'a str, page: HomePage, shown: HomePage, glyph: Icon) -> Element<'a, Message> {
    let lit = page == shown;
    button(row![icon(glyph, 15.0, if lit { style::TEXT } else { style::TEXT_DIM }), text(label).size(style::TEXT_SIZE)].spacing(10).align_y(Alignment::Center))
        .width(Length::Fill)
        .padding([8, 12])
        .style(style::row_button(lit))
        .on_press(say(HomeMessage::Show(page)))
        .into()
}

/// A resource: a link that does something, or one there is not yet (said so).
fn resource<'a>(label: &'a str, on_press: Option<Message>) -> Element<'a, Message> {
    let mut line = row![text(label).size(style::TEXT_SIZE).color(if on_press.is_some() { style::TEXT_DIM } else { style::TEXT_FAINT })]
        .spacing(8)
        .align_y(Alignment::Center);
    if on_press.is_none() {
        line = line.push(space().width(Length::Fill)).push(text("later").size(style::CAPTION).color(style::TEXT_FAINT));
    }
    button(line).width(Length::Fill).padding([5, 12]).style(style::flat_button).on_press_maybe(on_press).into()
}

fn sidebar(app: &App, shown: HomePage) -> Element<'_, Message> {
    let logo = image(crate::brand::logo_large()).width(Length::Fixed(40.0)).height(Length::Fixed(40.0));
    let name = column![
        text("Brewed Texture").size(16).font(style::bold()),
        text("Designer").size(16).font(style::bold()),
    ];
    // (The dark of the text of a primary button.)
    let new = button(row![icon(Icon::Plus, 14.0, style::gray(18)), text("New Package…").size(style::TEXT_SIZE)].spacing(8).align_y(Alignment::Center))
        .width(Length::Fill)
        .padding([8, 12])
        .style(style::primary_button)
        .on_press(Message::NewGraph(NewGraphMessage::Open(None)));
    let open = button(row![icon(Icon::Folder, 14.0, style::TEXT), text("Open…").size(style::TEXT_SIZE)].spacing(8).align_y(Alignment::Center))
        .width(Length::Fill)
        .padding([8, 12])
        .style(style::button)
        .on_press(Message::Open);
    let mut side = column![
        row![logo, name].spacing(12).align_y(Alignment::Center),
        space().height(12),
        page_button("Home", HomePage::Home, shown, Icon::Package),
        page_button("Learn", HomePage::Learn, shown, Icon::Info),
        space().height(12),
        new,
        open,
    ]
    .spacing(6);
    // Back to what is being edited, when there is something.
    if !app.blank() {
        side = side.push(
            button(text(format!("Back to {}", app.doc.title())).size(style::TEXT_SIZE).wrapping(text::Wrapping::None))
                .width(Length::Fill)
                .padding([8, 12])
                .style(style::flat_button)
                .on_press(say(HomeMessage::Close)),
        );
    }
    let resources = column![
        container(crate::view::caption("Resources")).padding(Padding { left: 12.0, bottom: 4.0, ..Padding::ZERO }),
        resource("Keyboard and Mouse", Some(Message::Dialog(Some(Dialog::Shortcuts)))),
        resource("Settings", Some(Message::Settings(SettingsMessage::Open))),
        resource("About Brewed Texture Designer", Some(Message::Dialog(Some(Dialog::About)))),
        resource("Website", None),
        resource("Documentation", None),
        resource("Forum", None),
        resource("Community Resources", None),
    ]
    .spacing(1);
    let side = side.push(space().height(Length::Fill)).push(resources);
    container(side).padding([20, 14]).width(Length::Fixed(SIDEBAR)).height(Length::Fill).style(style::sidebar).into()
}

/// A package opened last: its picture, its name, where it is and when it was saved.
fn card(recent: &Recent) -> Element<'_, Message> {
    let picture: Element<'_, Message> = match &recent.picture {
        Some(handle) => image(handle.clone()).width(Length::Fixed(PICTURE)).height(Length::Fixed(PICTURE)).into(),
        None => container(icon(Icon::Package, 40.0, style::TEXT_FAINT)).center(Length::Fixed(PICTURE)).into(),
    };
    let when = if !recent.there {
        "Not found where it was".to_string()
    } else {
        recent.modified.map(ago).unwrap_or_default()
    };
    let body = column![
        container(picture).style(style::picture_frame),
        text(recent.name.clone()).size(style::TEXT_SIZE).font(style::bold()).wrapping(text::Wrapping::None),
        text(recent.folder.clone()).size(style::SMALL).color(style::TEXT_FAINT).wrapping(text::Wrapping::None),
        text(when).size(style::SMALL).color(if recent.there { style::TEXT_DIM } else { style::WARNING }),
    ]
    .spacing(4)
    .width(Length::Fixed(PICTURE));
    let tile = button(container(body).clip(true)).padding(8).style(style::tile_button).on_press_maybe(recent.there.then(|| say(HomeMessage::Open(recent.path.clone()))));
    crate::view::tip(tile, recent.path.clone(), iced::widget::tooltip::Position::Bottom)
}

fn home_page(app: &App) -> Element<'_, Message> {
    let mut page = column![
        text(format!("Welcome to {}", brewed_texture_core::APP_NAME)).size(26).font(style::bold()),
        text("Textures made of nodes, every one tiling, computed on the graphics card: start a package from a template, open one, or see how the examples are made.")
            .size(style::TEXT_SIZE)
            .color(style::TEXT_DIM),
        space().height(14),
        crate::view::caption("Recent"),
    ]
    .spacing(8);
    if app.ui.home_recent.is_empty() {
        page = page.push(text("The packages you open or save come here, with their pictures.").size(style::TEXT_SIZE).color(style::TEXT_FAINT));
    } else {
        page = page.push(row(app.ui.home_recent.iter().map(card)).spacing(12).wrap());
    }
    page = page.push(space().height(10)).push(
        row![icon(Icon::Info, 13.0, style::TEXT_FAINT), text("Drop a package on the window to open it.").size(style::SMALL).color(style::TEXT_FAINT)]
            .spacing(6)
            .align_y(Alignment::Center),
    );
    page.into()
}

fn learn_page<'a>() -> Element<'a, Message> {
    let mut page = column![
        text("Learn").size(26).font(style::bold()),
        text("Packages to open and take apart: each opens as a package of its own, to change and save as you like.")
            .size(style::TEXT_SIZE)
            .color(style::TEXT_DIM),
        space().height(14),
    ]
    .spacing(8);
    for example in Example::ALL {
        let open = button(text("Open").size(style::TEXT_SIZE)).padding([6, 16]).style(style::button).on_press(say(HomeMessage::Example(example)));
        let item = row![
            column![text(example.name()).size(16).font(style::bold()), text(example.about()).size(style::TEXT_SIZE).color(style::TEXT_DIM)]
                .spacing(4)
                .width(Length::Fill),
            open,
        ]
        .spacing(16)
        .align_y(Alignment::Center);
        page = page.push(container(item).padding(14).width(Length::Fill).max_width(820).style(style::card));
    }
    let keys = button(text("Keyboard and Mouse").size(style::TEXT_SIZE)).padding([6, 16]).style(style::button).on_press(Message::Dialog(Some(Dialog::Shortcuts)));
    page = page.push(
        container(
            row![
                column![
                    text("Keyboard and mouse").size(16).font(style::bold()),
                    text("What every key does (each can be given another in the settings), and the mouse in the graph and the views.")
                        .size(style::TEXT_SIZE)
                        .color(style::TEXT_DIM),
                ]
                .spacing(4)
                .width(Length::Fill),
                keys,
            ]
            .spacing(16)
            .align_y(Alignment::Center),
        )
        .padding(14)
        .width(Length::Fill)
        .max_width(820)
        .style(style::card),
    );
    page.into()
}

pub fn view(app: &App, shown: HomePage) -> Element<'_, Message> {
    let content = match shown {
        HomePage::Home => home_page(app),
        HomePage::Learn => learn_page(),
    };
    let content = scrollable(container(content).padding(Padding { top: 34.0, right: 40.0, bottom: 34.0, left: 40.0 }).width(Length::Fill))
        .height(Length::Fill)
        .width(Length::Fill)
        .style(style::scrollable);
    row![sidebar(app, shown), container(content).width(Length::Fill).height(Length::Fill).style(style::panel_dark)].height(Length::Fill).into()
}
