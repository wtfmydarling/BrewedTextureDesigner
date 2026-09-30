//! The Home screen (see [`crate::app::home`]): the crate `home-screen` draws it, of what this
//! program has.

use iced::Element;

use crate::app::home::{HomeMessage, HomePage};
use crate::app::{App, Message};

pub fn view(app: &App, shown: HomePage) -> Element<'_, Message> {
    let back = (!app.blank()).then(|| app.doc.title());
    let content = home_screen::Content {
        name: brewed_texture_core::APP_NAME,
        logo: Some(crate::brand::logo_large()),
        tagline: "Textures made of nodes, every one tiling, computed on the graphics card: start a package from a template, open one, or see how the examples are made.",
        new: "New Graph…",
        start_heading: "Start from a template",
        start: &app.home.start,
        more: "More templates…",
        recent: &app.ui.home_recent,
        no_recent: "The packages you open or save come here, with their pictures.",
        drop_hint: "Drop a package on the window to open it.",
        learn: "Packages to open and take apart: each opens as a package of its own, to change and save as you like.",
        lessons: &app.home.lessons,
        resources: &app.home.resources,
        back,
        time: when,
    };
    let state = home_screen::State { page: shown, layout: app.ui.home_layout, sort: app.ui.home_sort };
    home_screen::view(content, state, home_screen::Style::default()).map(|event| Message::Home(HomeMessage::Screen(event)))
}

/// A moment as the list of packages says it: on the system's calendar and clock.
fn when(at: std::time::SystemTime) -> String {
    let ms = at.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    crate::clock::said_minutes(ms)
}
