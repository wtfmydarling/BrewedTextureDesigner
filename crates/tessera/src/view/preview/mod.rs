//! The 2D / 3D view pane.

pub mod overlay;
pub mod render;

use iced::widget::{button, center, column, container, row, shader, space, stack, text};
use iced::{Alignment, Element, Length};
use tessera_core::nodes::Role;

use crate::app::{App, Message, PreviewMessage, ViewMode};
use crate::icons::{Icon, icon};
use crate::style;
use crate::view::tip;
use iced::widget::tooltip::Position;

const CHANNELS: [&str; 6] = ["RGBA", "RGB", "R", "G", "B", "A"];

fn tab<'a>(label: &'a str, ic: Icon, active: bool, message: Message) -> Element<'a, Message> {
    button(row![icon(ic, 14.0, if active { style::ACCENT } else { style::TEXT_DIM }), text(label).size(style::TEXT_SIZE)].spacing(5).align_y(Alignment::Center))
        .padding([3, 8])
        .style(style::toggle_button(active))
        .on_press(message)
        .into()
}

fn toolbar(app: &App) -> Element<'_, Message> {
    let p = &app.ui.preview;
    let node_name = p.node.and_then(|id| app.graph().node(id)).map(crate::view::graph::node_title);
    let mut bar = row![
        tab("2D", Icon::Image, p.mode == ViewMode::TwoD, Message::Preview(PreviewMessage::Mode(ViewMode::TwoD))),
        tab("3D", Icon::Cube, p.mode == ViewMode::ThreeD, Message::Preview(PreviewMessage::Mode(ViewMode::ThreeD))),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    if p.mode == ViewMode::TwoD {
        bar = bar.push(match node_name {
            Some(n) => text(n).size(style::TEXT_SIZE).color(style::ACCENT),
            None => text("double-click a node to show it here").size(style::SMALL).color(style::TEXT_DIM),
        });
        if p.pick.is_some() {
            bar = bar.push(text("· picking a gradient").size(style::SMALL).color(style::TEXT_DIM));
            bar = bar.push(
                button(text("Cancel").size(style::SMALL))
                    .padding([2, 8])
                    .style(style::button)
                    .on_press(Message::Preview(PreviewMessage::CancelPick)),
            );
        }
        bar = bar.push(space().width(Length::Fill));
        for (i, name) in CHANNELS.iter().enumerate() {
            let active = p.channel == i as u32;
            bar = bar.push(
                button(text(*name).size(style::SMALL))
                    .padding([3, 7])
                    .style(style::toggle_button(active))
                    .on_press(Message::Preview(PreviewMessage::Channel(i as u32))),
            );
        }
        bar = bar.push(tip(
            button(icon(Icon::Grid, 15.0, if p.show_tiling { style::ACCENT } else { style::TEXT_DIM }))
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
            button(text("1:1").size(style::SMALL)).padding([3, 6]).style(style::flat_button).on_press(Message::Preview(PreviewMessage::OneToOne)),
            "One image pixel per screen pixel",
            Position::Bottom,
        ));
    } else {
        bar = bar.push(text("Outputs with a Usage are shown on the model").size(style::SMALL).color(style::TEXT_DIM));
        bar = bar.push(space().width(Length::Fill));
        bar = bar.push(crate::view::view3d::toolbar(app));
    }
    container(bar.align_y(Alignment::Center).height(Length::Fixed(28.0))).padding([3, 8]).width(Length::Fill).style(style::header_bar).into()
}

fn footer(app: &App) -> Element<'_, Message> {
    let p = &app.ui.preview;
    let mut parts = Vec::new();
    if p.mode == ViewMode::ThreeD {
        let maps = crate::view::view3d::map_names(app);
        parts.push(if maps.is_empty() { "no maps".to_string() } else { maps.join(" · ") });
        parts.push("left drag orbits, right drag moves the light, the wheel zooms".to_string());
    } else if let Some(img) = &app.res.preview {
        parts.push(format!("{}×{}", img.size[0], img.size[1]));
        parts.push(format!("{:.0}%", p.zoom * 100.0));
    }
    if let (ViewMode::TwoD, Some(px), Some(node)) = (p.mode, p.probe, p.node) {
        parts.push(format!("x {} y {}", px[0], px[1]));
        if let Some((n, at, c)) = app.res.probe
            && n == node
            && at == px
        {
            parts.push(format!("R {:.3}  G {:.3}  B {:.3}  A {:.3}", c[0], c[1], c[2], c[3]));
        }
    }
    container(text(parts.join("   ·   ")).size(style::SMALL).font(style::mono()).color(style::TEXT_DIM))
        .padding([4, 10])
        .width(Length::Fill)
        .style(style::panel_dark)
        .into()
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
    if let Some(e) = p.node.and_then(|id| {
        app.eval.node(id).and_then(|n| n.error().map(str::to_string)).or_else(|| app.res.reports.get(&id).and_then(|r| r.error.clone()))
    }) {
        layers.push(container(text(e).size(style::TEXT_SIZE).color(style::ERROR)).padding(8).into());
    }
    container(stack(layers)).style(style::canvas_frame).width(Length::Fill).height(Length::Fill).into()
}

pub fn view(app: &App) -> Element<'_, Message> {
    let content = match app.ui.preview.mode {
        ViewMode::TwoD => content_2d(app),
        ViewMode::ThreeD => crate::view::view3d::view(app),
    };
    column![toolbar(app), content, footer(app)].into()
}
