//! Views: every function here turns application state into widgets and maps user
//! actions to [`Message`]s. Views only read state; all changes happen in `App::update`.

pub mod add_menu;
pub mod choices;
pub mod context_menu;
pub mod dialogs;
pub mod editors;
pub mod graph;
pub mod inputs;
pub mod library;
pub mod picker;
pub mod preview;
pub mod properties;
pub mod toolbar;
pub mod view3d;

use iced::widget::{column, container, mouse_area, opaque, pane_grid, pin, row, space, stack, text, tooltip};
use iced::{Alignment, Element, Length, Point};

use crate::app::{App, Message, Pane};
use crate::style;

pub fn root(app: &App) -> Element<'_, Message> {
    let grid = pane_grid(&app.ui.panes, |_, pane, _| {
        let body: Element<'_, Message> = match pane {
            Pane::Library => library::view(app),
            Pane::Graph => column![toolbar::graph_header(app), graph::view(app)].into(),
            Pane::View3D => preview::view_3d(app),
            Pane::View2D => preview::view_2d(app),
            Pane::Inputs => inputs::view(app),
            Pane::Properties => properties::view(app),
        };
        pane_grid::Content::new(container(body).width(Length::Fill).height(Length::Fill).style(style::panel))
    })
    .spacing(3)
    .min_size(140)
    .on_resize(8, Message::PaneResized)
    .style(|_| pane_grid::Style {
        hovered_region: pane_grid::Highlight {
            background: style::alpha(style::ACCENT, 0.1).into(),
            border: iced::Border { color: style::ACCENT, width: 1.0, radius: 0.0.into() },
        },
        picked_split: pane_grid::Line { color: style::ACCENT, width: 2.0 },
        hovered_split: pane_grid::Line { color: style::ACCENT_DIM, width: 2.0 },
    });

    let window = column![
        toolbar::menu_bar(app),
        container(grid).height(Length::Fill).style(style::window_bar),
        toolbar::status_bar(app),
    ];
    let mut layers: Vec<Element<'_, Message>> = vec![window.into()];
    if let Some(ctx) = &app.ui.context {
        layers.push(context_menu::view(app, ctx));
    }
    if let Some(menu) = &app.ui.add_menu {
        layers.push(add_menu::view(app, menu));
    }
    if let Some(drag) = &app.ui.drag {
        let ghost = container(text(format!("+ {}", drag.label())).size(style::TEXT_SIZE))
            .padding([4, 8])
            .style(style::popup);
        let at = app.ui.cursor + iced::Vector::new(14.0, 10.0);
        layers.push(pin(ghost).position(at).into());
    }
    if let Some(picker) = editors::color_popup(app) {
        layers.push(picker);
    }
    if let Some(dialog) = &app.ui.dialog {
        layers.push(dialogs::view(app, dialog));
    }
    stack(layers).into()
}

/// A popup at a window position: clicking anywhere else sends `dismiss`.
pub fn popup_layer<'a>(
    content: impl Into<Element<'a, Message>>,
    at: Point,
    size: iced::Size,
    window: iced::Size,
    dismiss: Message,
) -> Element<'a, Message> {
    let x = at.x.min(window.width - size.width - 4.0).max(4.0);
    let y = at.y.min(window.height - size.height - 4.0).max(4.0);
    stack![
        mouse_area(space().width(Length::Fill).height(Length::Fill))
            .on_press(dismiss.clone())
            .on_right_press(dismiss)
            .on_middle_press(Message::Menu(None)),
        pin(opaque(container(content).width(Length::Fixed(size.width)).max_height(size.height)))
            .position(Point::new(x, y)),
    ]
    .into()
}

/// Wrap an element in a tooltip with the app's style.
pub fn tip<'a>(content: impl Into<Element<'a, Message>>, tip: impl Into<String>, position: tooltip::Position) -> Element<'a, Message> {
    let tip: String = tip.into();
    if tip.is_empty() {
        return content.into();
    }
    tooltip(
        content,
        container(text(tip).size(style::SMALL)).padding(6).max_width(320).style(style::tooltip),
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

/// The strip at the top of a pane: its title, and tools on the right.
pub fn pane_header<'a>(title: &str, tools: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(
        row![caption(title), space().width(Length::Fill), tools.into()]
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
pub fn window_size(app: &App) -> iced::Size {
    app.ui.window_size
}
