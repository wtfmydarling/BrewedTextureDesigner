//! Input Values: the graph's variables. A variable exists while at least one Input node
//! uses its name; Input nodes with the same name share the value (like material
//! parameters in Unreal). Values are edited here or in an Input node's properties.
//!
//! An image input is one of them: its value is a picture (Image Input nodes of one name
//! share it), with a picture by default, what it takes and its color while it has none.

use iced::widget::{button, column, container, pick_list, row, scrollable, space, text, text_input, tooltip};
use iced::{Alignment, Element, Length};
use brewed_texture_core::graph::{ImageInput, Variable};
use brewed_texture_core::nodes::ImageKind;
use brewed_texture_core::package::Package;
use brewed_texture_core::{DataType, Edit, Value};

use crate::app::{App, ColorTarget, DragPayload, FileTarget, Message, VarField};
use crate::icons::{Icon, icon};
use crate::style;
use crate::view::editors::{file_field, swatch, value_editor};
use crate::widget::outside::outside;
use crate::widget::pressable::pressable;
use crate::widget::socket_icon;

fn field_row<'a>(label: &'a str, editor: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    row![text(label).size(style::SMALL).color(style::TEXT_DIM).width(Length::Fixed(78.0)), editor.into()]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
}

fn set_value(name: &str) -> impl Fn(brewed_texture_core::Value) -> Message + Clone + 'static {
    let name = name.to_string();
    move |value| Message::Edit(Edit::SetVariable { name: name.clone(), value })
}

/// The color an image input shows while it has no picture.
pub fn image_color(var: &Variable) -> [f32; 4] {
    match var.value {
        Value::Color(c) => c,
        _ => brewed_texture_core::graph::IMAGE_INPUT_COLOR,
    }
}

/// What a value of an input value is, in a few words: its value, or a picture by its name.
fn said(var: &Variable, default: bool, package: &Package) -> String {
    match &var.image {
        Some(image) => {
            let picture = if default { &image.default } else { &image.picture };
            if picture.is_empty() { "no picture".to_string() } else { package.file_label(picture) }
        }
        None => (if default { var.default } else { var.value }).display(),
    }
}

/// The socket an input value is on a node that uses the graph, drawn: a diamond in the color
/// of its type, or a circle for an image input, in the color of what it takes.
fn socket_of<'a>(var: &Variable) -> Element<'a, Message> {
    match &var.image {
        Some(image) => socket_icon(DataType::Image, true, 12.0, style::image_color(Some(image.kind))),
        None => socket_icon(var.ty, true, 12.0, style::type_color(var.ty)),
    }
}

/// The settings of an image input that are not its picture: what it takes, the picture it
/// has by default, and its color while it has none.
fn image_settings<'a>(app: &'a App, var: &'a Variable, image: &'a ImageInput, mut col: iced::widget::Column<'a, Message>) -> iced::widget::Column<'a, Message> {
    let name = var.name.clone();
    let kinds = [ImageKind::Gray, ImageKind::Color];
    let kind = {
        let name = name.clone();
        pick_list(kinds.map(Kind), Some(Kind(image.kind)), move |k: Kind| Message::Edit(Edit::SetVariableKind { name: name.clone(), kind: k.0 }))
            .width(Length::Fill)
            .text_size(style::TEXT_SIZE)
            .padding([3, 6])
            .style(style::pick_list)
    };
    col = col.push(field_row("Kind", crate::view::tip(kind, "What the graph takes here: an image that arrives is made grayscale or color.", tooltip::Position::Top)));
    let color = image_color(var);
    col = col.push(field_row("Default", file_field(app, FileTarget::VariableDefault(name.clone()), "no picture: the color".into(), Some(color))));
    let open = app.ui.color_picker == Some(ColorTarget::Variable(name.clone(), VarField::Value));
    let chip = swatch(color, open, Message::ColorPicker(Some(ColorTarget::Variable(name, VarField::Value))));
    col.push(field_row("Color", crate::view::tip(chip, "What the graph sees here while the input has no picture.", tooltip::Position::Top)))
}

/// An image kind in a pick list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Kind(ImageKind);

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.name())
    }
}

/// Settings of a variable (also shown in the properties of an Input node). `rename`
/// adds the Name field, which renames the variable for all its Input nodes.
pub fn details<'a>(app: &'a App, var: &'a Variable, rename: bool) -> Element<'a, Message> {
    let name = var.name.clone();
    let renaming = app.ui.renaming_var.as_ref().filter(|(from, _)| *from == var.name).map(|(_, t)| t.as_str());
    let name_input = text_input("name", renaming.unwrap_or(&var.name))
        .on_input({
            let name = name.clone();
            move |t| Message::RenameVariable(name.clone(), t)
        })
        .on_submit(Message::CommitRename)
        .size(style::TEXT_SIZE)
        .padding([3, 6])
        .style(style::text_input);
    // A click anywhere else in the window renames it as Enter does.
    let name_input = outside(name_input).on_press(renaming.map(|_| Message::CommitRename));
    let mut col = column![].spacing(5);
    if rename {
        col = col.push(field_row("Name", name_input));
    }
    if rename && renaming.is_some_and(|t| t != var.name) {
        col = col.push(field_row(
            "",
            text("Enter or a click elsewhere renames it everywhere, Esc cancels").size(style::SMALL).color(style::ACCENT_TEXT),
        ));
    }
    col = col.push(field_row(
        "Description",
        text_input("what this value controls", &var.description)
            .on_input({
                let name = name.clone();
                move |t| Message::Edit(Edit::SetVariableDescription { name: name.clone(), text: t })
            })
            .size(style::TEXT_SIZE)
            .padding([3, 6])
            .style(style::text_input),
    ));

    match &var.image {
        Some(image) => col = image_settings(app, var, image, col),
        None => {
            let range = var.has_range().then_some((var.min, var.max));
            let default_open = app.ui.color_picker == Some(ColorTarget::Variable(var.name.clone(), VarField::Default));
            let default_editor = value_editor(
                var.default,
                None,
                range,
                {
                    let name = name.clone();
                    move |value| Message::Edit(Edit::SetVariableDefault { name: name.clone(), value })
                },
                Message::Seal,
                default_open,
                Message::ColorPicker(Some(ColorTarget::Variable(var.name.clone(), VarField::Default))),
            );
            col = col.push(field_row("Default", default_editor));
        }
    }
    if var.has_range() {
        let min_name = name.clone();
        let max = var.max;
        let min_editor = value_editor(
            var.min,
            None,
            None,
            move |v| Message::Edit(Edit::SetVariableRange { name: min_name.clone(), min: v, max }),
            Message::Seal,
            false,
            Message::Seal,
        );
        let max_name = name.clone();
        let min = var.min;
        let max_editor = value_editor(
            var.max,
            None,
            None,
            move |v| Message::Edit(Edit::SetVariableRange { name: max_name.clone(), min, max: v }),
            Message::Seal,
            false,
            Message::Seal,
        );
        col = col.push(field_row("Min", min_editor));
        col = col.push(field_row("Max", max_editor));
    }
    let at_default = var.is_default();
    // Where the two do not fit side by side, the second goes under the first (its name was cut
    // in two in its button).
    let buttons = row![
        button(
            row![icon(Icon::Reset, 13.0, style::TEXT_DIM), text("Reset to default").size(style::SMALL).wrapping(text::Wrapping::None)]
                .spacing(5)
                .align_y(Alignment::Center)
        )
        .padding([3, 8])
        .style(style::button)
        .on_press_maybe((!at_default).then(|| Message::Edit(Edit::ResetVariable(name.clone())))),
        button(text("Set as default").size(style::SMALL).wrapping(text::Wrapping::None))
            .padding([3, 8])
            .style(style::button)
            .on_press_maybe((!at_default).then(|| Message::Edit(Edit::MakeVariableDefault(name.clone())))),
    ]
    .spacing(6)
    .wrap()
    .vertical_spacing(4);
    col = col.push(row![space().width(Length::Fixed(78.0)), buttons]);
    let users = app.graph().variable_users(&var.name).count();
    let (what, node) = match &var.image {
        Some(image) => (format!("{} image", image.kind.name()), "Image Input node"),
        None => (var.ty.name().to_string(), "Input node"),
    };
    col = col.push(
        text(format!("{what} · used by {users} {node}{}", if users == 1 { "" } else { "s" }))
            .size(style::SMALL)
            .color(style::TEXT_FAINT),
    );
    col.into()
}

fn variable_row<'a>(app: &'a App, var: &'a Variable, index: usize) -> Element<'a, Message> {
    let expanded = app.ui.expanded_vars.contains(&var.name);
    let color_open = app.ui.color_picker == Some(ColorTarget::Variable(var.name.clone(), VarField::Value));
    let range = var.has_range().then_some((var.min, var.max));
    // The value of a variable is compared with its own default, so a changed input says so
    // in the same way a changed parameter does. The value of an image input is its picture.
    let editor: Element<'a, Message> = match &var.image {
        Some(_) => file_field(app, FileTarget::Variable(var.name.clone()), "no picture: the color".into(), Some(image_color(var))),
        None => value_editor(
            var.value,
            Some(var.default),
            range,
            set_value(&var.name),
            Message::Seal,
            color_open,
            Message::ColorPicker(Some(ColorTarget::Variable(var.name.clone(), VarField::Value))),
        ),
    };
    let grip = crate::view::tip(
        pressable(icon(Icon::Grip, 14.0, style::TEXT_FAINT))
            .padding([2, 1])
            .on_press(Message::PanelPress(DragPayload::Variable(var.name.clone()))),
        if var.image.is_some() {
            "Drag onto the graph to add another Image Input with this picture"
        } else {
            "Drag onto the graph to add another Input node with this value"
        },
        tooltip::Position::Right,
    );
    // A value that is no longer the one the input came with says so, and the middle button
    // puts that one back.
    let changed = !var.is_default();
    let title = if changed { format!("{}*", var.name) } else { var.name.clone() };
    let name = row![
        socket_of(var),
        text(title)
            .size(style::TEXT_SIZE)
            .font(if changed { style::italic() } else { style::bold() })
            .color(if changed { style::ACCENT } else { style::TEXT })
            .wrapping(text::Wrapping::None),
    ]
    .spacing(5)
    .align_y(Alignment::Center);
    let hint = match (var.description.is_empty(), changed) {
        (true, false) => String::new(),
        (false, false) => var.description.clone(),
        (true, true) => format!("Changed — middle-click the row to put {} back", said(var, true, app.doc.package())),
        (false, true) => format!("{}

Changed — middle-click the row to put {} back", var.description, said(var, true, app.doc.package())),
    };
    let name: Element<'a, Message> = crate::view::tip(name, hint, tooltip::Position::Bottom);
    let header = row![
        grip,
        // In a narrow panel the name is cut short rather than drawn under the value.
        container(name).width(Length::FillPortion(2)).clip(true),
        container(editor).width(Length::FillPortion(3)),
        button(icon(if expanded { Icon::ChevronDown } else { Icon::ChevronRight }, 13.0, style::TEXT_DIM))
            .padding(4)
            .style(style::flat_button)
            .on_press(Message::ToggleVariable(var.name.clone())),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let mut col = column![header].spacing(6);
    if expanded {
        col = col.push(container(details(app, var, true)).padding(iced::Padding { left: 22.0, ..iced::Padding::ZERO }));
    }
    // The same widget around the row either way, so that a field being dragged is not
    // rebuilt under the mouse; resetting a value that is already the default does nothing.
    let row = container(col).padding([5, 6]).width(Length::Fill).style(style::stripe(index.is_multiple_of(2)));
    iced::widget::mouse_area(row).on_middle_press(Message::Edit(Edit::ResetVariable(var.name.clone()))).into()
}

pub fn view(app: &App) -> Element<'_, Message> {
    let vars = &app.graph().variables;
    let header = crate::view::pane_header("Input Values", text(vars.len().to_string()).size(style::SMALL).color(style::TEXT_FAINT));
    let body: Element<'_, Message> = if vars.is_empty() {
        container(
            column![
                text("No input values yet.").size(style::TEXT_SIZE),
                text(
                    "Add Float, Integer, Boolean, Vec2, Color or Image Input nodes from the library. The name of \
                     each Input node is a value here; Input nodes with the same name share it. The value of an \
                     image input is a picture."
                )
                .size(style::SMALL)
                .color(style::TEXT_DIM),
            ]
            .spacing(6),
        )
        .padding(10)
        .into()
    } else {
        let mut list = column![].spacing(2);
        for (i, v) in vars.iter().enumerate() {
            list = list.push(variable_row(app, v, i));
        }
        scrollable(container(list).padding([0, 6])).height(Length::Fill).style(style::scrollable).into()
    };
    column![header, body].into()
}
