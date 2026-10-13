//! The two views of the result: the model on the left, the image on the right.

pub mod lights;
pub mod overlay;
pub mod render;

use brewed_texture_core::nodes::Role;
use iced::widget::{button, center, column, container, row, shader, stack, text};
use iced::{Alignment, Element, Length};

use crate::app::{App, Message, PreviewMessage};
use crate::icons::{Icon, icon};
use crate::style;
use crate::view::tip;
use iced::widget::tooltip::Position;

const CHANNELS: [&str; 6] = ["RGBA", "RGB", "R", "G", "B", "A"];

/// The header of the 2D view: which node is shown, the channels, tiling and zoom.
fn toolbar_2d(app: &App) -> Element<'_, Message> {
    let p = &app.ui.preview;
    let node_name = p.node.and_then(|id| app.graph().node(id)).map(crate::view::graph::node_title);
    // In a narrow view the name is shortened, and the buttons stay.
    let name = match node_name {
        Some(n) => crate::view::fitted(n, style::TEXT_SIZE, style::ui_font(), Some(style::TEXT)),
        None => crate::view::fitted("double-click a node to show it here", style::SMALL, style::ui_font(), Some(style::TEXT_DIM)),
    };
    let mut bar = row![icon(Icon::Image, 14.0, style::TEXT_DIM), container(name).width(Length::Fill).clip(true)].spacing(6).align_y(Alignment::Center);
    // In a narrow view, the buttons closer together; narrower, the channels on a line of their
    // own under the others.
    let room = crate::view::pane_width(app, crate::app::Pane::View2D);
    let (narrow, under) = (room < NARROW_2D, room < TWO_LINES_2D);
    let pad = if narrow { 4.0 } else { 6.0 };
    // (On their own line, as close as a strip of segments: it fits the narrowest view.)
    let (channel_pad, channel_gap) = if under {
        (3.0, 0)
    } else if narrow {
        (pad, 2)
    } else {
        (pad, 6)
    };
    let mut channels = row![].spacing(channel_gap).align_y(Alignment::Center);
    for (i, name) in CHANNELS.iter().enumerate() {
        let active = p.channel == i as u32;
        channels = channels.push(
            button(text(*name).size(style::SMALL))
                .padding([3.0, channel_pad])
                .style(style::toggle_button(active))
                .on_press(Message::Preview(PreviewMessage::Channel(i as u32))),
        );
    }
    if narrow {
        bar = bar.spacing(2);
    }
    let channels = if under {
        Some(channels)
    } else {
        bar = bar.push(channels);
        None
    };
    bar = bar.push(tip(
        button(icon(Icon::Grid, 15.0, if p.show_tiling { style::ACCENT_TEXT } else { style::TEXT_DIM }))
            .padding(4)
            .style(style::toggle_button(p.show_tiling))
            .on_press(Message::Preview(PreviewMessage::ToggleTiling)),
        "Repeat the image around itself the way the node tiles",
        Position::Bottom,
    ));
    bar = bar.push(tip(
        button(icon(Icon::Fit, 15.0, style::TEXT_DIM)).padding(4).style(style::flat_button).on_press(Message::Preview(PreviewMessage::Fit)),
        "Fit the image in the view (double-click the view)",
        Position::Bottom,
    ));
    bar = bar.push(tip(
        button(text("1:1").size(style::SMALL)).padding([3.0, pad]).style(style::flat_button).on_press(Message::Preview(PreviewMessage::OneToOne)),
        "One image pixel per screen pixel",
        Position::Bottom,
    ));
    if let Some(channels) = channels {
        // (Its line padded less: it fits the narrowest view, whatever the system draws its letters with.)
        let line = container(channels.height(Length::Fixed(crate::view::HEADER_H))).padding([0, 4]).width(Length::Fill).clip(true).style(style::header_bar);
        return column![header(bar), line].into();
    }
    header(bar)
}

/// How wide the 2D view is at least for its buttons to keep their room between them, and to have
/// them all on one line (with the closer ones: the icon, the channels, three buttons and the gaps).
const NARROW_2D: f32 = 340.0;
const TWO_LINES_2D: f32 = 262.0;

/// The header of the 3D view: how fast it draws, the shape and the light.
fn toolbar_3d(app: &App) -> Element<'_, Message> {
    // In a narrow view the fields go under the tools. (How fast it draws is said in the view's
    // corner, whole: cut short beside the tools, it read "0 F".)
    let (tools, under) = crate::view::view3d::toolbar(app, crate::view::pane_width(app, crate::app::Pane::View3D));
    let bar = row![icon(Icon::Cube, 14.0, style::TEXT_DIM), iced::widget::space().width(Length::Fill), tools].spacing(6).align_y(Alignment::Center);
    match under {
        Some(fields) => column![header(bar), header(fields)].into(),
        None => header(bar),
    }
}

/// The header of a view: one line, cut short at its edge (not drawn over the view beside it).
fn header<'a>(bar: iced::widget::Row<'a, Message>) -> Element<'a, Message> {
    container(bar.align_y(Alignment::Center).height(Length::Fixed(crate::view::HEADER_H)))
        .padding([0, 8])
        .width(Length::Fill)
        .clip(true)
        .style(style::header_bar)
        .into()
}

/// The line under the 2D view: the size of the image, the zoom, and the pixel under the
/// cursor. Words in the interface font, numbers in the monospaced one: they change under
/// the cursor and should not make the line jump.
fn footer_2d(app: &App) -> Element<'_, Message> {
    let p = &app.ui.preview;
    let mut numbers = Vec::new();
    if let Some(img) = &app.res.preview {
        numbers.push(format!("{}×{}", img.size[0], img.size[1]));
        numbers.push(format!("{:.0}%", p.zoom * 100.0));
    }
    if let (Some(px), Some(node)) = (p.probe, p.node) {
        numbers.push(format!("x {} y {}", px[0], px[1]));
        if let Some((n, at, c)) = app.res.probe
            && n == node
            && at == px
        {
            numbers.push(format!("R {:.3}  G {:.3}  B {:.3}  A {:.3}", c[0], c[1], c[2], c[3]));
        }
    }
    footer(numbers, Vec::new())
}

/// The line under the 3D view: the maps on the model, and how to move it. One line, whatever
/// the view's width: what does not fit is cut short there, and said whole in its tip (wrapped,
/// it was five lines of a narrow view, and the model had a strip of what was left).
fn footer_3d(app: &App) -> Element<'_, Message> {
    let whole = footer_3d_words(app);
    let room = crate::view::pane_width(app, crate::app::Pane::View3D) - 2.0 * FOOTER_PAD;
    let shown = crate::view::cut(&whole, style::SMALL, style::ui_font(), room);
    let cut_short = shown != whole;
    let line = container(text(shown).size(style::SMALL).color(style::TEXT_FAINT).wrapping(text::Wrapping::None))
        .padding([4.0, FOOTER_PAD])
        .width(Length::Fill)
        .clip(true)
        .style(style::panel_dark);
    if cut_short { tip(line, whole, Position::Top) } else { line.into() }
}

/// What the line under the 3D view says, whole.
pub(crate) fn footer_3d_words(app: &App) -> String {
    let maps = crate::view::view3d::map_names(app);
    let lighting = &app.ui.preview.lighting;
    let mut words = vec![if maps.is_empty() { "no maps: give an Output a Usage".to_string() } else { maps.join(" · ") }];
    if let Some(file) = &lighting.reading {
        words.push(format!("reading {}…", brewed_texture_core::package::file_name(file)));
    }
    let light = if app.environment_shown().is_some() { "the environment" } else { "the light" };
    words.push(format!("left drag orbits, middle drag moves, right drag turns {light}, the wheel zooms"));
    words.join("   ·   ")
}

/// The room left and right of what a view's footer says.
const FOOTER_PAD: f32 = 10.0;

fn footer<'a>(numbers: Vec<String>, words: Vec<String>) -> Element<'a, Message> {
    let line = row![
        text(numbers.join("   ·   ")).size(style::SMALL).font(style::mono()).color(style::TEXT_DIM),
        text(words.join("   ·   ")).size(style::SMALL).color(style::TEXT_FAINT),
    ]
    .spacing(if numbers.is_empty() || words.is_empty() { 0 } else { 14 })
    .align_y(Alignment::Center);
    container(line).padding([4.0, FOOTER_PAD]).width(Length::Fill).style(style::panel_dark).into()
}

fn content_2d(app: &App) -> Element<'_, Message> {
    let p = &app.ui.preview;
    let node = p.node.and_then(|id| app.graph().node(id));
    // Value nodes have no image: show their value.
    if let Some(n) = node
        && !n.def().has_image()
    {
        let msg = match app.eval.node(n.id) {
            Some(e) if e.error().is_some() => e.error().unwrap_or_default().to_string(),
            Some(e) => match e.value {
                Some(v) => format!("{} = {}", crate::view::graph::node_title(n), v.display()),
                None => "No value".to_string(),
            },
            None => String::new(),
        };
        let color = if matches!(n.def().role, Role::Value | Role::Variable(_)) { style::TEXT } else { style::TEXT_DIM };
        return container(center(text(msg).size(20).font(style::mono()).color(color))).style(style::canvas_frame).into();
    }
    let image = app.res.preview.as_ref().filter(|img| Some(img.node) == p.node);
    let size = image.map(|img| img.size);
    let bounds = p.bounds.size();
    let rect = match size {
        Some(s) if bounds.width > 1.0 => overlay::image_rect(bounds, s, p.zoom, p.pan),
        _ => iced::Rectangle::default(),
    };
    let tiling = p.node.and_then(|id| app.eval.params(id)).map_or(0, |x| x.tiling());
    let scene = render::Scene {
        image: image.map(|img| (img.view.clone(), img.size)),
        gray: image.is_some_and(|img| img.gray),
        rect,
        channel: p.channel,
        tile: [p.show_tiling && tiling & 1 != 0, p.show_tiling && tiling & 2 != 0],
    };
    let mut layers: Vec<Element<'_, Message>> = vec![
        shader(scene).width(Length::Fill).height(Length::Fill).into(),
        iced::widget::canvas(overlay::Overlay { app, image: size }).width(Length::Fill).height(Length::Fill).into(),
    ];
    let hint = if app.engine.is_none() {
        Some("Starting the GPU…")
    } else if node.is_none() {
        Some("No node shown")
    } else if image.is_none() {
        Some("Computing…")
    } else {
        None
    };
    if let Some(h) = hint {
        layers.push(center(text(h).size(15).color(style::TEXT_FAINT)).into());
    }
    if let Some(e) =
        p.node.and_then(|id| app.eval.node(id).and_then(|n| n.error().map(str::to_string)).or_else(|| app.res.reports.get(&id).and_then(|r| r.error.clone())))
    {
        layers.push(container(text(e).size(style::TEXT_SIZE).color(style::ERROR)).padding(8).into());
    }
    container(stack(layers)).style(style::canvas_frame).width(Length::Fill).height(Length::Fill).into()
}

/// The 2D view: the image the shown node computes.
pub fn view_2d(app: &App) -> Element<'_, Message> {
    column![toolbar_2d(app), content_2d(app), footer_2d(app)].into()
}

/// The 3D view: the model wearing the maps the graph outputs.
pub fn view_3d(app: &App) -> Element<'_, Message> {
    column![toolbar_3d(app), crate::view::view3d::view(app), footer_3d(app)].into()
}
