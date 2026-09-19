//! Input Values: the graph's variables. A variable exists while at least one Input node
//! uses its name; Input nodes with the same name share the value (like material
//! parameters in Unreal). Values are edited here or in an Input node's properties.

use iced::widget::{button, column, container, row, scrollable, space, text, text_input, tooltip};
use iced::{Alignment, Element, Length};
use tessera_core::Edit;
use tessera_core::graph::Variable;

use crate::app::{App, ColorTarget, DragPayload, Message, VarField};
use crate::icons::{Icon, icon};
use crate::style;
use crate::view::editors::{color_editor, value_editor};
use crate::widget::pressable::pressable;
use crate::widget::socket_icon;

fn field_row<'a>(label: &'a str, editor: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    row![text(label).size(style::SMALL).color(style::TEXT_DIM).width(Length::Fixed(78.0)), editor.into()]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
}

fn set_value(name: &str) -> impl Fn(tessera_core::Value) -> Message + Clone + 'static {
    let name = name.to_string();
    move |value| Message::Edit(Edit::SetVariable { name: name.clone(), value })
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
    let mut col = column![].spacing(5);
    if rename {
        col = col.push(field_row("Name", name_input));
    }
    if rename && renaming.is_some_and(|t| t != var.name) {
        col = col.push(field_row("", text("Enter renames it everywhere, Esc cancels").size(style::SMALL).color(style::ACCENT_TEXT)));
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

    let range = var.has_range().then_some((var.min, var.max));
    let default_open = app.ui.color_picker == Some(ColorTarget::Variable(var.name.clone(), VarField::Default));
    let default_editor = value_editor(
        var.default,
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
    if default_open && let tessera_core::Value::Color(c) = var.default {
        let name = name.clone();
        col = col.push(color_editor(
            c,
            ColorTarget::Variable(var.name.clone(), VarField::Default),
            move |c| Message::Edit(Edit::SetVariableDefault { name: name.clone(), value: tessera_core::Value::Color(c) }),
            Message::Seal,
        ));
    }
    if var.has_range() {
        let min_name = name.clone();
        let max = var.max;
        let min_editor = value_editor(
            var.min,
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
            move |v| Message::Edit(Edit::SetVariableRange { name: max_name.clone(), min, max: v }),
            Message::Seal,
            false,
            Message::Seal,
        );
        col = col.push(field_row("Min", min_editor));
        col = col.push(field_row("Max", max_editor));
    }
    let at_default = var.value == var.default;
    col = col.push(
        row![
            space().width(Length::Fixed(78.0)),
            button(row![icon(Icon::Reset, 13.0, style::TEXT_DIM), text("Reset to default").size(style::SMALL)].spacing(5).align_y(Alignment::Center))
                .padding([3, 8])
                .style(style::button)
                .on_press_maybe((!at_default).then(|| Message::Edit(Edit::ResetVariable(name.clone())))),
            button(text("Set as default").size(style::SMALL))
                .padding([3, 8])
                .style(style::button)
                .on_press_maybe((!at_default).then(|| Message::Edit(Edit::SetVariableDefault { name: name.clone(), value: var.value }))),
        ]
        .spacing(6),
    );
    let users = app.graph().variable_users(&var.name).count();
    col = col.push(
        text(format!("{} · used by {users} Input node{}", var.ty.name(), if users == 1 { "" } else { "s" }))
            .size(style::SMALL)
            .color(style::TEXT_FAINT),
    );
    col.into()
}

fn variable_row<'a>(app: &'a App, var: &'a Variable, index: usize) -> Element<'a, Message> {
    let expanded = app.ui.expanded_vars.contains(&var.name);
    let color_open = app.ui.color_picker == Some(ColorTarget::Variable(var.name.clone(), VarField::Value));
    let range = var.has_range().then_some((var.min, var.max));
    let editor = value_editor(
        var.value,
        range,
        set_value(&var.name),
        Message::Seal,
        color_open,
        Message::ColorPicker(Some(ColorTarget::Variable(var.name.clone(), VarField::Value))),
    );
    let grip = crate::view::tip(
        pressable(icon(Icon::Grip, 14.0, style::TEXT_FAINT))
            .padding([2, 1])
            .on_press(Message::PanelPress(DragPayload::Variable(var.name.clone()))),
        "Drag onto the graph to add another Input node with this value",
        tooltip::Position::Right,
    );
    let name = row![socket_icon(var.ty, true, 12.0, style::type_color(var.ty)), text(&var.name).size(style::TEXT_SIZE).font(style::bold())]
        .spacing(5)
        .align_y(Alignment::Center);
    let name: Element<'a, Message> = if var.description.is_empty() {
        name.into()
    } else {
        crate::view::tip(name, &var.description, tooltip::Position::Bottom)
    };
    let header = row![
        grip,
        container(name).width(Length::FillPortion(2)),
        container(editor).width(Length::FillPortion(3)),
        button(icon(if expanded { Icon::ChevronDown } else { Icon::ChevronRight }, 13.0, style::TEXT_DIM))
            .padding(4)
            .style(style::flat_button)
            .on_press(Message::ToggleVariable(var.name.clone())),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let mut col = column![header].spacing(6);
    if color_open && let tessera_core::Value::Color(c) = var.value {
        let name = var.name.clone();
        col = col.push(color_editor(
            c,
            ColorTarget::Variable(var.name.clone(), VarField::Value),
            move |c| Message::Edit(Edit::SetVariable { name: name.clone(), value: tessera_core::Value::Color(c) }),
            Message::Seal,
        ));
    }
    if expanded {
        col = col.push(container(details(app, var, true)).padding(iced::Padding { left: 22.0, ..iced::Padding::ZERO }));
    }
    container(col).padding([5, 6]).width(Length::Fill).style(style::stripe(index.is_multiple_of(2))).into()
}

pub fn view(app: &App) -> Element<'_, Message> {
    let vars = &app.graph().variables;
    let header = crate::view::pane_header("Input Values", text(vars.len().to_string()).size(style::SMALL).color(style::TEXT_FAINT));
    let body: Element<'_, Message> = if vars.is_empty() {
        container(
            column![
                text("No input values yet.").size(style::TEXT_SIZE),
                text(
                    "Add Float, Integer, Boolean, Vec2 or Color Input nodes from the library. The name of each \
                     Input node is a value here; Input nodes with the same name share it."
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
