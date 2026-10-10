//! Views: every function here turns application state into widgets and maps user
//! actions to [`Message`]s. Views only read state; all changes happen in `App::update`.

pub mod add_menu;
pub mod choices;
pub mod context_menu;
pub mod dialogs;
pub mod editors;
pub mod explorer;
pub mod frames;
pub mod gpu_probe;
pub mod graph;
pub mod home;
pub mod inputs;
pub mod library;
pub mod models;
pub mod notices;
pub mod picker;
pub mod preview;
pub mod properties;
pub mod settings;
pub mod toolbar;
pub mod shading;
pub mod view3d;

use iced::widget::{column, container, mouse_area, opaque, pane_grid, pin, row, space, stack, text, tooltip};
use crate::widget::blind::blind;
use iced::{Alignment, Element, Length, Point};

use crate::app::{App, Message, Pane};
use crate::style;

pub fn root(app: &App) -> Element<'_, Message> {
    let grid = pane_grid(&app.ui.panes, |_, pane, _| {
        let body: Element<'_, Message> = match pane {
            Pane::Explorer => explorer::view(app),
            Pane::Library => library::view(app),
            Pane::Graph => column![toolbar::graph_header(app), graph::view(app)].into(),
            Pane::View3D => preview::view_3d(app),
            Pane::View2D => preview::view_2d(app),
            Pane::Inputs => inputs::view(app),
            Pane::Properties => properties::view(app),
        };
        pane_grid::Content::new(container(body).width(Length::Fill).height(Length::Fill).style(style::panel))
    })
    .spacing(PANE_SPACING)
    .min_size(PANE_MIN)
    .on_resize(8, Message::PaneResized)
    .style(|_| pane_grid::Style {
        hovered_region: pane_grid::Highlight {
            background: style::alpha(style::ACCENT, 0.1).into(),
            border: iced::Border { color: style::ACCENT, width: 1.0, radius: 0.0.into() },
        },
        picked_split: pane_grid::Line { color: style::ACCENT, width: 2.0 },
        hovered_split: pane_grid::Line { color: style::ACCENT_DIM, width: 2.0 },
    });

    // The Home screen, in place of the editor while it is shown (the editor is behind it); with
    // it, the probe that has the engine start meanwhile (see `gpu_probe`). Under an open menu
    // nothing takes the cursor (see `widget::blind`).
    let menu_open = app.ui.menu.is_some();
    let window = match app.ui.home {
        Some(page) => column![
            toolbar::menu_bar(app),
            blind(container(home::view(app, page)).height(Length::Fill).style(style::window_bar), menu_open),
            gpu_probe::probe(),
        ],
        None => column![
            toolbar::menu_bar(app),
            blind(container(grid).height(Length::Fill).style(style::window_bar), menu_open),
            blind(toolbar::status_bar(app), menu_open),
        ],
    };
    let mut layers: Vec<Element<'_, Message>> = vec![window.into()];
    // The Color Editor, a window over the panels: the menus over it, a dialog over all.
    if let Some(editor) = editors::color_window(app) {
        layers.push(blind(editor, menu_open).into());
    }
    if let Some(ctx) = &app.ui.context {
        layers.push(context_menu::view(app, ctx));
    }
    if let Some(menu) = &app.ui.add_menu {
        layers.push(add_menu::view(app, menu));
    }
    if let Some(drag) = &app.ui.drag {
        // What cannot be dropped on the graph says so as soon as it is dragged, and why.
        let says = match app.drop_refusal(drag) {
            Some(why) => text(format!("✕ {why}")).size(style::TEXT_SIZE).color(style::ERROR),
            None => text(format!("+ {}", drag.label())).size(style::TEXT_SIZE),
        };
        let ghost = container(says).padding([4, 8]).max_width(420).style(style::popup);
        let at = app.ui.cursor + iced::Vector::new(14.0, 10.0);
        layers.push(pin(ghost).position(at).into());
    }
    if let Some(dialog) = &app.ui.dialog {
        // Where the program draws the window's title bar, a dialog covers what is under it, not
        // it (see `toolbar::menu_bar`).
        let dialog = dialogs::view(app, dialog);
        layers.push(if crate::app::OWN_TITLE_BAR {
            column![space().height(Length::Fixed(toolbar::MENU_BAR_H)), dialog].into()
        } else {
            dialog
        });
    }
    // Over everything, the dialogs too (the settings dialog shows it itself).
    if app.ui.restart_offer && !matches!(app.ui.dialog, Some(crate::app::Dialog::Settings(_))) {
        layers.push(settings::restart_offer(app));
    }
    // The edges that size the window, over everything (not while it fills the screen).
    if crate::app::OWN_TITLE_BAR && !app.ui.maximized {
        layers.push(toolbar::window_edges());
    }
    stack(layers).into()
}

/// A popup `width` wide at a window position, kept inside the window as large as it is:
/// clicking anywhere else sends `dismiss`.
pub fn popup_layer<'a>(content: impl Into<Element<'a, Message>>, at: Point, width: f32, dismiss: Message) -> Element<'a, Message> {
    stack![
        mouse_area(space().width(Length::Fill).height(Length::Fill))
            .on_press(dismiss.clone())
            .on_right_press(dismiss)
            .on_middle_press(Message::Menu(None)),
        crate::widget::placed::placed(opaque(container(content).width(Length::Fixed(width))), at),
    ]
    .into()
}

/// How wide a line of text is drawn at `size` in the interface's font: what a menu is as wide
/// as, to show each of its items whole. Each is measured once (anew when fonts are loaded).
pub fn text_width(content: &str, size: f32) -> f32 {
    text_width_in(content, size, style::ui_font())
}

/// How wide a line of text is drawn at `size` in `font`.
pub fn text_width_in(content: &str, size: f32, font: iced::Font) -> f32 {
    use iced::advanced::graphics::text::{Paragraph, Version, font_system};
    use iced::advanced::text::{self as core_text, Paragraph as _};
    use std::cell::RefCell;
    use std::collections::HashMap;
    type Widths = (Option<Version>, HashMap<(String, u32, iced::Font), f32>);
    thread_local! {
        static WIDTHS: RefCell<Widths> = RefCell::new((None, HashMap::new()));
    }
    let version = font_system().read().ok().map(|f| f.version());
    WIDTHS.with_borrow_mut(|(known, widths)| {
        // (Fonts loaded since, or too many kept: measured anew.)
        if *known != version || widths.len() > 8192 {
            widths.clear();
            *known = version;
        }
        *widths.entry((content.to_owned(), size.to_bits(), font)).or_insert_with(|| {
            Paragraph::with_text(core_text::Text {
                content,
                bounds: iced::Size::INFINITE,
                size: iced::Pixels(size),
                line_height: core_text::LineHeight::default(),
                font,
                align_x: core_text::Alignment::Default,
                align_y: iced::alignment::Vertical::Top,
                shaping: core_text::Shaping::default(),
                wrapping: core_text::Wrapping::None,
            })
            .min_width()
        })
    })
}

/// `content` in `room` pixels at `size` in `font`: whole when it fits, else its beginning and its
/// end with "…" between them, as much of each as fits (names told apart by their ends, "Graph 2"
/// and "Graph 3", stay told apart; cut at the edge, they read the same).
pub fn shortened(content: &str, size: f32, font: iced::Font, room: f32) -> String {
    if text_width_in(content, size, font) <= room {
        return content.to_owned();
    }
    let chars: Vec<char> = content.chars().collect();
    let cut = |kept: usize| -> String {
        let (head, tail) = (kept - kept / 2, kept / 2);
        chars[..head].iter().chain(['…'].iter()).chain(&chars[chars.len() - tail..]).collect()
    };
    // The most characters kept that fit (none: the "…" alone; nothing where even it does not).
    let (mut fits, mut over) = (0, chars.len());
    while fits + 1 < over {
        let kept = (fits + over) / 2;
        if text_width_in(&cut(kept), size, font) <= room {
            fits = kept;
        } else {
            over = kept;
        }
    }
    let shown = cut(fits);
    if fits == 0 && text_width_in(&shown, size, font) > room { String::new() } else { shown }
}

/// `content` in `room` pixels at `size` in `font`: whole when it fits, else as much of its
/// beginning as fits, with "…" after it (a sentence reads from its beginning; nothing where even
/// "…" does not fit).
pub fn cut(content: &str, size: f32, font: iced::Font, room: f32) -> String {
    if text_width_in(content, size, font) <= room {
        return content.to_owned();
    }
    let chars: Vec<char> = content.chars().collect();
    let cut = |kept: usize| -> String { chars[..kept].iter().collect::<String>().trim_end().to_owned() + "…" };
    let (mut fits, mut over) = (0, chars.len());
    while fits + 1 < over {
        let kept = (fits + over) / 2;
        if text_width_in(&cut(kept), size, font) <= room {
            fits = kept;
        } else {
            over = kept;
        }
    }
    let shown = cut(fits);
    if fits == 0 && text_width_in(&shown, size, font) > room { String::new() } else { shown }
}

/// A line of `content` at `size` in `font` (in `color`, or in the color of what it is in) as wide
/// as its room, shortened to it (see [`shortened`]).
pub fn fitted<'a>(content: impl Into<String>, size: f32, font: iced::Font, color: Option<iced::Color>) -> Element<'a, Message> {
    let content: String = content.into();
    iced::widget::responsive(move |room| {
        let line = text(shortened(&content, size, font, room.width)).size(size).font(font).wrapping(text::Wrapping::None);
        match color {
            Some(color) => line.color(color),
            None => line,
        }
        .into()
    })
    .height(Length::Shrink)
    .into()
}

/// Wrap an element in a tooltip with the app's style.
pub fn tip<'a>(content: impl Into<Element<'a, Message>>, tip: impl Into<String>, position: tooltip::Position) -> Element<'a, Message> {
    let tip: String = tip.into();
    if tip.is_empty() {
        return content.into();
    }
    // (A path in it breaks where it has to: a folder's name longer than the tip ran out of it.)
    tooltip(
        content,
        container(text(tip).size(style::SMALL).wrapping(text::Wrapping::WordOrGlyph)).padding(6).max_width(320).style(style::tooltip),
        position,
    )
    .delay(std::time::Duration::from_millis(450))
    .into()
}

/// Section header inside panels: an upper-case caption over a hairline.
pub fn section<'a>(title: &'a str) -> Element<'a, Message> {
    container(column![caption(title), hairline()].spacing(4))
        .padding(iced::Padding { top: 12.0, bottom: 4.0, left: 2.0, right: 0.0 })
        .into()
}

/// Upper-case caption (pane titles, sections).
pub fn caption<'a>(title: &str) -> iced::widget::Text<'a> {
    text(title.to_uppercase()).size(style::CAPTION).font(style::bold()).color(style::TEXT_DIM)
}

/// A horizontal 1 px line.
pub fn hairline<'a>() -> Element<'a, Message> {
    container(space().height(1)).width(Length::Fill).height(1).style(style::hairline(style::LINE)).into()
}

/// Height of pane headers.
pub const HEADER_H: f32 = 30.0;

/// How far apart the panels are, and how small one is let be.
pub const PANE_SPACING: f32 = 3.0;
pub const PANE_MIN: f32 = 140.0;

/// How wide a panel is, as the size of the window and the splits say: what its header has room
/// for (known before the panel is laid out).
pub fn pane_width(app: &App, which: Pane) -> f32 {
    let window = app.ui.window_size;
    let grid = iced::Size::new(window.width, (window.height - toolbar::MENU_BAR_H - toolbar::STATUS_BAR_H).max(0.0));
    let regions = app.ui.panes.layout().pane_regions(PANE_SPACING, PANE_MIN, grid);
    app.ui.panes.iter().find(|(_, pane)| **pane == which).and_then(|(id, _)| regions.get(id)).map_or(grid.width, |r| r.width)
}

/// The strip at the top of a pane: its title, and tools on the right.
pub fn pane_header<'a>(title: &str, tools: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(
        row![container(caption(title).wrapping(text::Wrapping::None)).width(Length::Fill).clip(true), tools.into()]
            .spacing(8)
            .align_y(Alignment::Center)
            .height(Length::Fixed(HEADER_H)),
    )
    .padding([0, 10])
    .width(Length::Fill)
    .style(style::header_bar)
    .into()
}

/// The size of the window (for keeping popups inside it).
/// How far the mouse wheel turned, in pixels (a line is 60), as the views zoom by it: the
/// wheel pushed away zooms in, unless the settings say the other way.
pub fn wheel(app: &App, delta: &iced::mouse::ScrollDelta) -> f32 {
    let dy = match delta {
        iced::mouse::ScrollDelta::Lines { y, .. } => y * 60.0,
        iced::mouse::ScrollDelta::Pixels { y, .. } => *y,
    };
    if app.settings.invert_zoom { -dy } else { dy }
}
