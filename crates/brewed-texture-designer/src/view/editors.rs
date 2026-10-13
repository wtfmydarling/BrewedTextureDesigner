//! Editors for values, shared by the Input Values panel and the properties panel.

use brewed_texture_core::{ResourceKind, Value, files};
use iced::widget::{button, column, container, image, mouse_area, row, space, text, text_input, toggler, tooltip};
use iced::{Alignment, Border, ContentFit, Element, Length, Size};

use crate::app::{App, ColorEditorMessage, ColorTarget, ColorUnits, DragPayload, FileTarget, Message, PickMessage, VarField};
use crate::icons::{Icon, icon};
use crate::picker::PickTarget;
use crate::style;
use crate::view::tip;
use crate::widget::color::{color_picker, color_strip, luminance, rgb_to_hsv, to_hex};
use crate::widget::number::{NumberField, number};

/// Does a field that takes the files with these extensions (none: any) take the file at `path`?
pub fn takes_file(extensions: &[&str], path: &str) -> bool {
    extensions.is_empty()
        || std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| extensions.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// The kind of the files with these extensions, if they are the files of one.
fn kind_of(extensions: &[&str]) -> Option<ResourceKind> {
    ResourceKind::ALL.into_iter().find(|k| k.extensions() == extensions)
}

/// The file a node or a graph reads: a picture of it and its name, and buttons to choose
/// another and to read none; `none` says what reading none means, and `color` is what is seen
/// then, when it is a color. (A file edited elsewhere is read again by itself.) A file of the
/// packages dragged from the Explorer is dropped on it: while one is dragged, the field says at
/// once whether it takes it.
pub fn file_field<'a>(app: &'a App, target: FileTarget, none: String, color: Option<[f32; 4]>) -> Element<'a, Message> {
    let extensions = app.takes(&target);
    let kind = kind_of(extensions);
    if let Some(DragPayload::Resource { path: dragged, name, .. }) = &app.ui.drag {
        let takes = takes_file(extensions, dragged);
        let says = if takes {
            format!("Drop to use {name}")
        } else {
            let what = match kind {
                Some(ResourceKind::Bitmap) => "a picture",
                Some(ResourceKind::Svg) => "an SVG file",
                Some(ResourceKind::Font) => "a font",
                None => "a file this takes",
            };
            format!("✕ {name} is not {what}")
        };
        let color = if takes { style::ACCENT } else { style::ERROR };
        let zone = container(crate::view::fitted(says, style::SMALL, style::ui_font(), Some(color)))
            .padding([5, 8])
            .width(Length::Fill)
            .clip(true)
            .style(style::drop_zone(takes));
        let zone = mouse_area(zone);
        return if takes { zone.on_release(Message::SetFile(target, Some(dragged.clone()))).into() } else { zone.into() };
    }

    let path = app.file_of(&target);
    let state = (!path.is_empty()).then(|| app.res.files.get(path)).flatten();
    // Not looked at yet: taken as there.
    let there = state.is_none_or(|s| s.there);
    let preview = state.and_then(|s| s.preview.as_ref());
    let picture: Element<'a, Message> = match (preview, color.filter(|_| path.is_empty())) {
        (Some(p), _) => image(p.image.clone()).width(22).height(22).content_fit(ContentFit::Contain).into(),
        (None, Some(c)) => container(container(space().width(16).height(14)).style(style::chip(style::color_of(c)))).width(22).center_x(22).into(),
        (None, None) => {
            let glyph = match kind {
                Some(ResourceKind::Svg) => Icon::Vector,
                Some(ResourceKind::Font) => Icon::Font,
                _ => Icon::Image,
            };
            container(icon(glyph, 14.0, style::TEXT_FAINT)).width(22).center_x(22).into()
        }
    };
    let embedded = files::is_embedded(path);
    let (name, color) = match (path.is_empty(), there) {
        (true, _) => (none, style::TEXT_FAINT),
        (false, true) => (app.doc.package().file_label(path), style::TEXT),
        (false, false) => (format!("{} (not there)", app.doc.package().file_label(path)), style::ERROR),
    };
    let mut about = Vec::new();
    if !path.is_empty() {
        about.push(if embedded { "Held in the package's file (see the Explorer's Resources)".to_string() } else { path.to_string() });
    }
    if let Some(p) = preview {
        about.push(p.about.clone());
    }
    if let Some((_, why)) = state.and_then(|s| s.failed.as_ref()) {
        about.push(why.clone());
    }
    if !path.is_empty() && !embedded {
        about.push("Edited elsewhere, it is read again by itself.".into());
    }
    about.push("Drag a file onto it from the Explorer, or choose one.".into());
    let shown = container(row![picture, crate::view::fitted(name, style::SMALL, style::ui_font(), Some(color))].spacing(6).align_y(Alignment::Center))
        .padding([1, 6])
        .width(Length::Fill)
        .clip(true)
        .style(style::field_box);
    let mut buttons = row![tip(
        button(icon(Icon::Folder, 14.0, style::TEXT_DIM)).padding(4).style(style::button).on_press(Message::BrowseFile(target.clone())),
        if path.is_empty() { "Choose a file" } else { "Choose another file" },
        tooltip::Position::Top,
    )]
    .spacing(4);
    if !path.is_empty() {
        buttons = buttons.push(tip(
            button(icon(Icon::Close, 14.0, style::TEXT_DIM)).padding(4).style(style::button).on_press(Message::SetFile(target, None)),
            "Read no file",
            tooltip::Position::Top,
        ));
    }
    row![tip(shown, about.join("\n"), tooltip::Position::Top), buttons].spacing(4).align_y(Alignment::Center).into()
}

/// A small color swatch that toggles a color picker.
pub fn swatch<'a>(c: [f32; 4], open: bool, on_press: Message) -> Element<'a, Message> {
    let color = style::color_of(c);
    let chip = container(space().width(Length::Fill).height(Length::Fixed(16.0))).style(move |_| container::Style {
        background: Some(iced::Color { a: 1.0, ..color }.into()),
        border: Border { color: style::BORDER, width: 1.0, radius: 2.0.into() },
        ..Default::default()
    });
    // (Close enough together for the color's name to stay whole in the narrowest Input Values.)
    button(
        row![
            container(chip).width(Length::Fixed(32.0)),
            text(to_hex(c)).size(style::SMALL).font(style::mono()).color(style::TEXT_DIM).wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([3, 4])
    .width(Length::Fill)
    .style(style::toggle_button(open))
    .on_press(on_press)
    .into()
}

/// The expanded color editor: picker, numeric RGBA channels and the eyedropper, which
/// takes a color from anywhere on the screen for `target`.
/// A button that starts picking from the screen. While its pick is in progress it stays
/// down, and a click on it cancels.
pub fn eyedropper<'a>(target: PickTarget, picking: Option<&PickTarget>, label: &'a str, tooltip: &'a str) -> Element<'a, Message> {
    let active = picking == Some(&target);
    let message = if active { PickMessage::Cancel } else { PickMessage::Start(target) };
    let button = button(
        row![icon(Icon::Pipette, 14.0, if active { style::TEXT } else { style::TEXT_DIM }), text(label).size(style::SMALL)]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .padding([3, 8])
    .style(move |theme, status| if active { style::toggle_button(true)(theme, status) } else { style::button(theme, status) })
    .on_press(Message::Pick(message));
    tip(button, tooltip, tooltip::Position::Top)
}

/// Editor of a [`Value`] within an optional [min, max] range.
pub fn value_editor<'a>(
    value: Value,
    // What the value would be untouched, when there is such a thing: the field says so.
    default: Option<Value>,
    range: Option<(Value, Value)>,
    on_change: impl Fn(Value) -> Message + Clone + 'a,
    on_release: Message,
    color_open: bool,
    on_color: Message,
) -> Element<'a, Message> {
    let bounds = |i: usize| -> (f64, f64, bool) {
        match range {
            Some((Value::Float(a), Value::Float(b))) if a < b => (a as f64, b as f64, true),
            Some((Value::Int(a), Value::Int(b))) if a < b => (a as f64, b as f64, true),
            Some((Value::Vec2(a), Value::Vec2(b))) if a[i] < b[i] => (a[i] as f64, b[i] as f64, true),
            _ => (0.0, 1.0, false),
        }
    };
    // The default of the same shape as the value; anything else is no default at all.
    let default_at = |i: usize| match default {
        Some(Value::Float(d)) => Some(d as f64),
        Some(Value::Int(d)) => Some(d as f64),
        Some(Value::Vec2(d)) => Some(d[i] as f64),
        _ => None,
    };
    let held = |field: NumberField<'a, Message>, i: usize| match default_at(i) {
        Some(d) => field.default_value(d),
        None => field,
    };
    match value {
        Value::Float(v) => {
            let (lo, hi, limited) = bounds(0);
            let mut n = held(number(v as f64, lo, hi), 0).on_change(move |x| on_change(Value::Float(x as f32))).on_release(on_release);
            if limited {
                n = n.limits(lo, hi);
            }
            n.into()
        }
        Value::Int(v) => {
            let (lo, hi, limited) = bounds(0);
            let (lo, hi) = if limited { (lo, hi) } else { (0.0, 16.0) };
            let mut n = held(number(v as f64, lo, hi), 0).integer(true).on_change(move |x| on_change(Value::Int(x as i32))).on_release(on_release);
            if limited {
                n = n.limits(lo, hi);
            }
            n.into()
        }
        Value::Bool(b) => toggler(b)
            .on_toggle(move |x| on_change(Value::Bool(x)))
            .label(if b { "true" } else { "false" })
            .text_size(style::SMALL)
            .size(16)
            .style(style::toggler)
            .into(),
        Value::Vec2(v) => {
            let mut r = row![].spacing(4);
            for (i, prefix) in ["x ", "y "].iter().enumerate() {
                let (lo, hi, limited) = bounds(i);
                let (lo, hi) = if limited { (lo, hi) } else { (-1.0, 1.0) };
                let f = on_change.clone();
                let mut n = held(number(v[i] as f64, lo, hi), i)
                    .prefix(*prefix)
                    .on_change(move |x| {
                        let mut n = v;
                        n[i] = x as f32;
                        f(Value::Vec2(n))
                    })
                    .on_release(on_release.clone());
                if limited {
                    n = n.limits(lo, hi);
                }
                r = r.push(n);
            }
            r.into()
        }
        Value::Color(c) => {
            let _ = on_change;
            let _ = on_release;
            swatch(c, color_open, on_color)
        }
    }
}

// ---------------------------------------------------------------------------- colors

/// How large the Color Editor's window opens, and how small it is let be.
pub const COLOR_EDITOR: Size = Size::new(600.0, 340.0);
pub const COLOR_EDITOR_MIN: Size = Size::new(520.0, 300.0);
/// How high its own title bar is (where the program draws it).
const COLOR_EDITOR_BAR: f32 = 30.0;

/// A channel of a color, as its sliders say it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Channel {
    R,
    G,
    B,
    H,
    S,
    V,
    A,
    /// Its level of gray (its luminance, what a grayscale image makes of it).
    L,
}

impl Channel {
    fn label(self) -> &'static str {
        match self {
            Channel::R => "R",
            Channel::G => "G",
            Channel::B => "B",
            Channel::H => "H",
            Channel::S => "S",
            Channel::V => "V",
            Channel::A => "A",
            Channel::L => "L",
        }
    }

    /// How much of it `c` has, from 0 to 1 (`hsv`: its hue, saturation and value, as they are
    /// shown: see `App::hsv_shown`).
    fn of(self, c: [f32; 4], [h, s, v]: [f32; 3]) -> f32 {
        match self {
            Channel::R => c[0],
            Channel::G => c[1],
            Channel::B => c[2],
            Channel::H => h,
            Channel::S => s,
            Channel::V => v,
            Channel::A => c[3],
            Channel::L => luminance(c),
        }
    }

    /// What gives `target` (of color `c`, of hue, saturation and value `hsv`) as much of it as
    /// `x` says, from 0 to 1: the hue, the saturation and the value are set as such (the other
    /// two stay as they are shown, whatever the color then says of them), the rest as a color.
    fn set(self, target: ColorTarget, c: [f32; 4], [h, s, v]: [f32; 3], x: f32) -> Message {
        let x = if x.is_finite() { x.clamp(0.0, 1.0) } else { 0.0 };
        match self {
            Channel::R => Message::SetColor(target, [x, c[1], c[2], c[3]]),
            Channel::G => Message::SetColor(target, [c[0], x, c[2], c[3]]),
            Channel::B => Message::SetColor(target, [c[0], c[1], x, c[3]]),
            Channel::H => Message::SetHsv(target, [x.min(0.9999), s, v]),
            Channel::S => Message::SetHsv(target, [h, x, v]),
            Channel::V => Message::SetHsv(target, [h, s, x]),
            Channel::A => Message::SetColor(target, [c[0], c[1], c[2], x]),
            Channel::L => Message::SetColor(target, [x, x, x, c[3]]),
        }
    }

    /// What 1 is said as: 1, or 255 (a hue, 360 degrees).
    fn scale(self, units: ColorUnits) -> f64 {
        match (units.float, self) {
            (true, _) => 1.0,
            (false, Channel::H) => 360.0,
            (false, _) => 255.0,
        }
    }
}

/// A slider each of `channels` of `c` (of hue, saturation and value `hsv`), setting `target`:
/// from 0 to 1 or to 255, as `units` say, never past them (a color could be given negative
/// channels).
fn sliders<'a>(target: &ColorTarget, c: [f32; 4], hsv: [f32; 3], units: ColorUnits, channels: &[Channel]) -> iced::widget::Column<'a, Message> {
    let mut col = column![].spacing(4).width(Length::Fill);
    for &channel in channels {
        let (target, scale) = (target.clone(), channel.scale(units));
        let field = number(f64::from(channel.of(c, hsv)) * scale, 0.0, scale)
            .limits(0.0, scale)
            .integer(!units.float)
            .decimals(3)
            .prefix(format!("{} ", channel.label()))
            .on_change(move |x| channel.set(target.clone(), c, hsv, (x / scale) as f32))
            .on_release(Message::Seal);
        col = col.push(field);
    }
    col
}

/// The switches of how channels are said: Float (on, from 0 to 1; off, from 0 to 255) and, where
/// `hsv` offers it, HSV (on, hue, saturation and value; off, red, green and blue).
fn unit_buttons<'a>(units: ColorUnits, hsv: bool, width: Length) -> Element<'a, Message> {
    let toggle = |label: &'a str, on: bool, then: ColorUnits, said: &'a str| {
        tip(
            button(text(label).size(style::SMALL).wrapping(text::Wrapping::None).width(Length::Fill).center())
                .padding([2, 6])
                .width(width)
                .style(style::switch_button(on))
                .on_press(Message::ColorEditor(ColorEditorMessage::Units(then))),
            said,
            tooltip::Position::Top,
        )
    };
    let mut buttons: Vec<Element<'a, Message>> =
        vec![toggle("Float", units.float, ColorUnits { float: !units.float, ..units }, "Each channel from 0 to 1 (off: from 0 to 255)")];
    if hsv {
        buttons.push(toggle("HSV", units.hsv, ColorUnits { hsv: !units.hsv, ..units }, "Hue, saturation and value (off: red, green and blue)"));
    }
    column(buttons).spacing(4).into()
}

/// A small square of `c` that opens (`open`: shows open) the Color Editor.
fn chip_button<'a>(c: [f32; 4], open: bool, on_press: Message) -> Element<'a, Message> {
    let chip = container(space().width(Length::Fixed(20.0)).height(Length::Fixed(16.0))).style(style::chip(style::color_of(c)));
    tip(button(chip).padding(2).style(style::toggle_button(open)).on_press(on_press), "Open the Color Editor", tooltip::Position::Top)
}

/// A button that picks a color from the screen, as its icon only.
fn eyedropper_icon<'a>(target: PickTarget, picking: Option<&PickTarget>) -> Element<'a, Message> {
    let active = picking == Some(&target);
    let message = if active { PickMessage::Cancel } else { PickMessage::Start(target) };
    let button = button(icon(Icon::Pipette, 14.0, if active { style::TEXT } else { style::TEXT_DIM }))
        .padding(4)
        .style(move |theme, status| if active { style::toggle_button(true)(theme, status) } else { style::button(theme, status) })
        .on_press(Message::Pick(message));
    tip(button, "Pick a color anywhere on any monitor (Esc or a right click cancels)", tooltip::Position::Top)
}

/// A color field: the eyedropper, the square that opens the Color Editor, and the strip a color
/// (`gray`: a level of gray) is picked from at a click or a drag.
pub fn color_field<'a>(app: &'a App, target: ColorTarget, c: [f32; 4], gray: bool) -> Element<'a, Message> {
    let open = app.ui.color_picker.as_ref() == Some(&target);
    let pick = eyedropper_icon(PickTarget::Color(target.clone()), app.picking());
    let chip = chip_button(c, open, Message::ColorPicker(Some(target.clone())));
    let strip = color_strip(c, gray, move |c| Message::SetColor(target.clone(), c)).on_release(Message::Seal);
    row![pick, chip, strip].spacing(6).align_y(Alignment::Center).into()
}

/// The channels of a color field, under it and as wide as it (`left` and `right`: from where the
/// row of the field begins and ends to where the field does): how they are said, and a slider
/// each (of a level of gray, its level only), so that a color is set without opening the Color
/// Editor.
pub fn color_channels<'a>(app: &'a App, target: ColorTarget, c: [f32; 4], gray: bool, left: f32, right: f32) -> Element<'a, Message> {
    use Channel::*;
    let units = app.ui.color_units;
    let channels: &[Channel] = match (gray, units.hsv) {
        (true, _) => &[L],
        (false, true) => &[H, S, V, A],
        (false, false) => &[R, G, B, A],
    };
    let hsv = app.hsv_shown(&target).unwrap_or_else(|| rgb_to_hsv([c[0], c[1], c[2]]));
    let block = row![unit_buttons(units, !gray, Length::Fixed(52.0)), sliders(&target, c, hsv, units, channels)].spacing(8);
    container(block).padding(iced::Padding { left, right, ..iced::Padding::ZERO }).into()
}

/// What `target` is called in the Color Editor's title: the node and its parameter, the input
/// value, the stop of a gradient.
pub fn target_name(app: &App, target: &ColorTarget) -> String {
    let graph = app.graph();
    let node = |id| graph.node(id);
    match target {
        ColorTarget::Param(id, key) => match node(*id) {
            Some(n) => {
                let label = n.param_def(key).map_or_else(|| key.clone(), |pd| pd.label.to_string());
                format!("{} · {label}", crate::view::graph::node_title(n))
            }
            None => key.clone(),
        },
        ColorTarget::Variable(name, VarField::Value) => name.clone(),
        ColorTarget::Variable(name, VarField::Default) => format!("{name}, by default"),
        ColorTarget::GradientStop(id, key) => {
            let stop = app.ui.gradient_stop.get(&(*id, key.clone())).map_or(0, |i| i + 1);
            match node(*id) {
                Some(n) => format!("{} · stop {stop}", crate::view::graph::node_title(n)),
                None => format!("stop {stop}"),
            }
        }
        ColorTarget::Comment(_) => "Comment".to_string(),
    }
}

/// The Color Editor, in a window of its own (see `App::color_window`): the square of the hue with
/// the hue bar beside it, as large as the window lets it be; the color as it is now over the one
/// it was when the editor was opened (a click on that one gives it back); Pick, Invert, To Gray,
/// Copy and Paste (its hexadecimal, as other programs copy it); Float; a slider each channel; its
/// hexadecimal at the bottom, typed or pasted. A level of gray has the same editor: it keeps the
/// gray of what it is given (and the editor, the color it was given: see `App::color_shown`).
/// Where the program draws the window's title bar, it has one (moved by it, closed by its ×) and
/// edges that size it.
pub fn color_editor(app: &App) -> Element<'_, Message> {
    let Some((target, c)) = app.ui.color_picker.clone().and_then(|t| app.target_color(&t).map(|c| (t, c))) else {
        return container(space()).width(Length::Fill).height(Length::Fill).style(style::window_bar).into();
    };
    let edited = target.clone();
    let body = container(iced::widget::responsive(move |room| editor_body(app, &edited, c, room))).padding(10).height(Length::Fill);
    let mut window = column![];
    if crate::app::OWN_TITLE_BAR {
        let title = crate::view::fitted(format!("Color Editor · {}", target_name(app, &target)), style::TEXT_SIZE, style::bold(), Some(style::TEXT));
        let close = tip(
            button(icon(Icon::Close, 14.0, style::TEXT_DIM)).padding(6).style(style::flat_button).on_press(Message::ColorPicker(None)),
            "Close the Color Editor",
            tooltip::Position::Bottom,
        );
        let bar = mouse_area(
            container(row![title, close].spacing(6).align_y(Alignment::Center))
                .padding(iced::Padding { left: 10.0, right: 2.0, ..iced::Padding::ZERO })
                .center_y(Length::Fixed(COLOR_EDITOR_BAR))
                .width(Length::Fill)
                .style(style::window_bar),
        )
        .on_press(Message::ColorEditor(ColorEditorMessage::Drag));
        window = window.push(bar);
    }
    let window = container(window.push(body)).width(Length::Fill).height(Length::Fill).style(style::tool_window);
    if crate::app::OWN_TITLE_BAR {
        iced::widget::stack![window, crate::view::toolbar::window_edges_with(|d| Message::ColorEditor(ColorEditorMessage::Resize(d)))].into()
    } else {
        window.into()
    }
}

/// What the Color Editor shows of `target` (`c` now) in `room`: the square as large as the room
/// lets it be, as high as it and the sliders left their width; the buttons; the sliders, the
/// hexadecimal at the bottom.
fn editor_body<'a>(app: &'a App, target: &ColorTarget, c: [f32; 4], room: Size) -> Element<'a, Message> {
    use Channel::*;
    const MIDDLE_W: f32 = 96.0;
    const SLIDERS_MIN: f32 = 170.0;
    const SPACING: f32 = 12.0;
    let shown = app.color_shown(target).unwrap_or(c);
    let units = app.ui.color_units;
    let side = room.height.min(room.width - crate::widget::color::HUE_ROOM - 2.0 * SPACING - MIDDLE_W - SLIDERS_MIN).max(80.0);
    let hsv = app.hsv_shown(target).unwrap_or_else(|| rgb_to_hsv([shown[0], shown[1], shown[2]]));
    let set = target.clone();
    let square = color_picker(hsv, move |hsv| Message::SetHsv(set.clone(), hsv)).side(side).on_release(Message::Seal);
    let doing = |label: &'static str, message: ColorEditorMessage, said: &'static str| {
        tip(
            button(text(label).size(style::SMALL).width(Length::Fill).center().wrapping(text::Wrapping::None))
                .padding([3, 6])
                .width(Length::Fill)
                .style(style::button)
                .on_press(Message::Step(Box::new(Message::ColorEditor(message)))),
            said,
            tooltip::Position::Left,
        )
    };
    // (As wide as the others: its own button elsewhere is as wide as its words.)
    let from_screen = PickTarget::Color(target.clone());
    let picking = app.picking() == Some(&from_screen);
    let pick = tip(
        button(
            container(
                row![icon(Icon::Pipette, 14.0, if picking { style::TEXT } else { style::TEXT_DIM }), text("Pick").size(style::SMALL)]
                    .spacing(6)
                    .align_y(Alignment::Center),
            )
            .center_x(Length::Fill),
        )
        .padding([3, 6])
        .width(Length::Fill)
        .style(move |theme, status| if picking { style::toggle_button(true)(theme, status) } else { style::button(theme, status) })
        .on_press(Message::Pick(if picking { PickMessage::Cancel } else { PickMessage::Start(from_screen) })),
        "Click anywhere on any monitor to take its color. Esc or a right click cancels.",
        tooltip::Position::Left,
    );
    // The color as it is now, over the one it was when the editor was opened: a click on that
    // one gives it back.
    let swatch = |c: [f32; 4]| container(space().width(Length::Fill).height(Length::Fixed(32.0))).style(style::chip(style::color_of(c)));
    let before = app.ui.color_before.as_ref().filter(|(of, ..)| of == target).map_or(c, |(.., c)| *c);
    let was = tip(
        button(swatch(before))
            .padding(0)
            .width(Length::Fill)
            .style(style::flat_button)
            .on_press(Message::Step(Box::new(Message::SetColor(target.clone(), before)))),
        "The color as it was when the editor was opened: click to give it back",
        tooltip::Position::Left,
    );
    let now = tip(swatch(c), "The color as it is now", tooltip::Position::Left);
    let middle = column![
        column![now, was],
        pick,
        doing("Invert", ColorEditorMessage::Invert, "Each channel turned over: dark for light"),
        doing("To Gray", ColorEditorMessage::ToGray, "Its gray: the level of gray it looks"),
        doing("Copy", ColorEditorMessage::Copy, "Its hexadecimal (#RRGGBB) on the clipboard"),
        doing("Paste", ColorEditorMessage::Paste, "The color the clipboard says (#RRGGBB, as other programs copy it)"),
        unit_buttons(units, false, Length::Fill),
    ]
    .spacing(4)
    .width(Length::Fixed(MIDDLE_W));
    let hex = match &app.ui.color_hex {
        Some((typed, at)) if *at == shown => typed.clone(),
        _ => to_hex(shown),
    };
    let hex = text_input("#RRGGBB", &hex)
        .on_input(|t| Message::ColorEditor(ColorEditorMessage::Hex(t)))
        .font(style::mono())
        .size(style::TEXT_SIZE)
        .padding([3, 6])
        .style(style::text_input);
    // (Its hexadecimal at the bottom, whatever the height of the window.)
    let right = column![sliders(target, shown, hsv, units, &[R, G, B, H, S, V, A]), space().height(Length::Fill), hex].height(Length::Fill).width(Length::Fill);
    row![square, middle, right].spacing(SPACING).height(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A channel set is the channel read, the others as they were; none goes past its ends; a
    /// level of gray set is a gray; and channels are said to 255 (a hue, to 360 degrees) or to 1.
    /// The hue, the saturation and the value are set as such: the value at none (black) or the
    /// saturation at none (a gray) leave the other two as they were, which the color alone
    /// would not say.
    #[test]
    fn channels_are_set_read_and_said() {
        use crate::widget::color::hsv_to_rgb;
        use Channel::*;
        let c = [0.2, 0.4, 0.6, 0.8];
        let hsv = rgb_to_hsv([c[0], c[1], c[2]]);
        let target = ColorTarget::Variable("tint".into(), VarField::Value);
        // What a channel set comes to: the color, and its hue, saturation and value.
        let set = |channel: Channel, x: f32| match channel.set(target.clone(), c, hsv, x) {
            Message::SetColor(_, color) => (color, rgb_to_hsv([color[0], color[1], color[2]])),
            Message::SetHsv(_, hsv) => {
                let [r, g, b] = hsv_to_rgb(hsv);
                ([r, g, b, c[3]], hsv)
            }
            other => panic!("{other:?}"),
        };
        for channel in [R, G, B, A, S, V] {
            for x in [0.0, 0.25, 1.0] {
                let (color, hsv) = set(channel, x);
                assert!((channel.of(color, hsv) - x).abs() < 1e-4, "{channel:?} at {x}");
            }
        }
        let (color, turned) = set(H, 0.5);
        assert!((H.of(color, turned) - 0.5).abs() < 1e-4);
        assert_eq!(set(R, 1.0).0[1..], c[1..], "the others as they were");
        assert_eq!(set(A, 0.1).0[..3], c[..3]);
        assert_eq!((set(R, -3.0).0[0], set(G, 7.0).0[1], set(A, f32::NAN).0[3]), (0.0, 1.0, 0.0), "none past its ends");
        assert_eq!(set(L, 0.5).0, [0.5, 0.5, 0.5, 0.8]);
        assert_eq!(set(V, 0.0), ([0.0, 0.0, 0.0, 0.8], [hsv[0], hsv[1], 0.0]), "black, of the hue and the saturation it had");
        assert_eq!(set(S, 0.0).1, [hsv[0], 0.0, hsv[2]], "a gray, of the hue it had");
        assert!((L.of([1.0, 1.0, 1.0, 1.0], hsv) - 1.0).abs() < 1e-6 && L.of([0.0, 0.0, 0.0, 1.0], hsv) == 0.0);
        let (bytes, float) = (ColorUnits::default(), ColorUnits { float: true, hsv: false });
        assert_eq!((R.scale(bytes), A.scale(bytes), H.scale(bytes), R.scale(float), H.scale(float)), (255.0, 255.0, 360.0, 1.0, 1.0));
    }
}
