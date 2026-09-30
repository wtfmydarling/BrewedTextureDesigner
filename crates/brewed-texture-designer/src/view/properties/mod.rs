//! Properties of the selected node.
//!
//! Every parameter row has the same three columns: label, editor, expose toggle. An
//! exposed parameter gets its value from its socket, so its editor is replaced by the
//! value arriving there (or a note that nothing is connected) and cannot be edited.

pub mod expression;
mod levels;

use iced::widget::{button, column, container, mouse_area, pick_list, row, scrollable, space, text, text_input, toggler, tooltip};
use iced::{Alignment, Element, Length};
use brewed_texture_core::graph::{Node, NodeId, param_socket_key};
use brewed_texture_core::nodes::{NodeKind, ParamDef, ParamKind};
use brewed_texture_core::package::Package;
use brewed_texture_core::{Curve, DataType, Edit, Gradient, GraphAttributes, Interpolation, ParamValue, Tiling, Value};

use crate::app::{App, ColorTarget, FileTarget, Message};
use crate::icons::{Icon, icon};
use crate::style;
use crate::picker::PickTarget;
use crate::view::editors::{eyedropper, file_field, swatch};
use crate::view::{tip, section};
use crate::widget::curve::curve_editor;
use crate::widget::gradient::gradient_editor;
use crate::widget::number::number;
use crate::widget::outside::outside;
use crate::widget::{expose_toggle, socket_icon};

const LABEL_W: f32 = 118.0;

/// An enum entry for pick lists.
#[derive(Clone, Debug, PartialEq)]
struct Choice {
    index: u32,
    label: &'static str,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label)
    }
}

fn set(node: NodeId, key: &str, value: ParamValue) -> Message {
    Message::Edit(Edit::SetParam { node, key: key.to_string(), value })
}

pub fn view(app: &App) -> Element<'_, Message> {
    let header = crate::view::pane_header("Properties", space());
    let ids: Vec<NodeId> = app.ui.selection.iter().copied().filter(|id| app.graph().contains(*id)).collect();
    let body: Element<'_, Message> = match ids.len() {
        0 if app.ui.selected_comment.is_some_and(|id| app.graph().comment(id).is_some()) => {
            comment_view(app, app.ui.selected_comment.expect("a selected comment"))
        }
        0 => scrollable(container(graph_view(app)).padding(iced::Padding { top: 0.0, right: 12.0, bottom: 16.0, left: 10.0 }))
            .height(Length::Fill)
            .style(style::scrollable)
            .into(),
        1 => scrollable(container(node_view(app, ids[0])).padding(iced::Padding { top: 0.0, right: 12.0, bottom: 16.0, left: 10.0 }))
            .height(Length::Fill)
            .style(style::scrollable)
            .into(),
        n => multi_view(app, &ids, n),
    };
    column![header, body].into()
}

/// Id of the text field of a comment (for focusing it from the graph).
pub fn comment_text_id() -> iced::widget::Id {
    iced::widget::Id::new("brewed-comment-text")
}

/// The comment frame being worked on: what it says, and what color it is.
/// The graph being edited, when nothing in it is chosen: what it holds, and what it says of
/// itself for the Library of the packages that have it (Settings → Library).
fn graph_view(app: &App) -> Element<'_, Message> {
    let g = app.graph();
    let a = g.attributes.clone();
    let with = |change: fn(&mut GraphAttributes, String), a: GraphAttributes| {
        move |value: String| {
            let mut a = a.clone();
            change(&mut a, value);
            Message::Edit(Edit::SetAttributes(a))
        }
    };
    let label = |name: &'static str| text(name).size(style::TEXT_SIZE).color(style::TEXT_DIM).width(Length::Fixed(LABEL_W));
    let shown = {
        let a = a.clone();
        move |on: bool| Message::Edit(Edit::SetAttributes(GraphAttributes { hidden: !on, ..a.clone() }))
    };
    column![
        text("Select a node to edit its parameters.").size(style::TEXT_SIZE).color(style::TEXT_DIM),
        text(format!(
            "Graph: {} nodes, {} links, {} input values, {}×{} px",
            g.nodes.len(),
            g.links.len(),
            g.variables.len(),
            g.resolution[0],
            g.resolution[1]
        ))
        .size(style::SMALL)
        .color(style::TEXT_FAINT),
        section("Attributes"),
        text("What the graph says of itself: in the Library, where its package is in a library folder (Settings → Library), and in the tips of the nodes that use it.")
            .size(style::SMALL)
            .color(style::TEXT_DIM),
        row![
            label("Description"),
            text_input("What it makes", &a.description)
                .on_input(with(|a, v| a.description = v, a.clone()))
                .size(style::TEXT_SIZE)
                .padding([4, 6])
                .style(style::text_input),
        ]
        .align_y(Alignment::Center),
        row![
            label("Category"),
            text_input("Noises/Grunge (none: the package's name)", &a.category)
                .on_input(with(|a, v| a.category = v, a.clone()))
                .size(style::TEXT_SIZE)
                .padding([4, 6])
                .style(style::text_input),
        ]
        .align_y(Alignment::Center),
        row![
            label("Show in Library"),
            toggler(!a.hidden).on_toggle(shown).size(16.0),
            text("Off for a graph the others use, not one to use itself").size(style::SMALL).color(style::TEXT_FAINT),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(10)
    .padding(iced::Padding { top: 10.0, ..iced::Padding::ZERO })
    .into()
}

fn comment_view(app: &App, id: brewed_texture_core::CommentId) -> Element<'_, Message> {
    let Some(comment) = app.graph().comment(id) else { return space().into() };
    let open = app.ui.color_picker == Some(ColorTarget::Comment(id));
    container(
        column![
            row![
                container(space().width(12).height(12)).style(style::chip(style::color_of(comment.color))),
                text("Comment").size(16).font(style::bold()),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            text("A frame around a group of nodes; drag its title bar to move them together.")
                .size(style::SMALL)
                .color(style::TEXT_DIM),
            text_input("What are these nodes for?", &comment.text)
                .id(comment_text_id())
                .on_input(move |t| Message::Edit(Edit::SetCommentText { id, text: t }))
                .size(style::TEXT_SIZE)
                .padding([4, 6])
                .style(style::text_input),
            row![
                text("Color").size(style::SMALL).color(style::TEXT_DIM).width(Length::Fixed(LABEL_W)),
                swatch(comment.color, open, Message::ColorPicker(Some(ColorTarget::Comment(id)))),
            ]
            .align_y(Alignment::Center),
            row![
                button(text("Delete Comment").size(style::SMALL))
                    .padding([3, 8])
                    .style(style::button)
                    .on_press(Message::Edit(Edit::RemoveComments(vec![id]))),
            ],
        ]
        .spacing(10),
    )
    .padding(10)
    .into()
}

fn multi_view<'a>(app: &'a App, ids: &[NodeId], n: usize) -> Element<'a, Message> {
    let images: Vec<NodeId> = ids.iter().copied().filter(|id| app.graph().node(*id).is_some_and(|n| n.def().has_image())).collect();
    let mut tiling = row![].spacing(4);
    for (i, label) in Tiling::LABELS.iter().enumerate() {
        tiling = tiling.push(
            button(text(*label).size(style::SMALL))
                .padding([3, 7])
                .style(style::button)
                .on_press_maybe((!images.is_empty()).then(|| {
                    Message::Edit(Edit::SetParamMany { nodes: images.clone(), key: "tiling".into(), value: ParamValue::Enum(i as u32) })
                })),
        );
    }
    // The precision of those that keep an image of their own.
    let own: Vec<NodeId> = ids.iter().copied().filter(|id| app.graph().node(*id).is_some_and(|n| n.def().has_precision())).collect();
    let mut precision = row![].spacing(4);
    for (i, label) in brewed_texture_core::nodes::PRECISION_LABELS.iter().enumerate() {
        precision = precision.push(
            button(text(*label).size(style::SMALL))
                .padding([3, 7])
                .style(style::button)
                .on_press_maybe((!own.is_empty()).then(|| {
                    Message::Edit(Edit::SetParamMany { nodes: own.clone(), key: "precision".into(), value: ParamValue::Enum(i as u32) })
                })),
        );
    }
    container(
        column![
            text(format!("{n} nodes selected")).size(style::TEXT_SIZE),
            text("Tiling of all selected image nodes:").size(style::SMALL).color(style::TEXT_DIM),
            tiling,
            text("Precision of all selected image nodes:").size(style::SMALL).color(style::TEXT_DIM),
            precision,
            row![
                button(text("Duplicate").size(style::SMALL)).padding([3, 8]).style(style::button).on_press(Message::Duplicate),
                button(text("Delete").size(style::SMALL)).padding([3, 8]).style(style::button).on_press(Message::Delete),
            ]
            .spacing(6),
        ]
        .spacing(8),
    )
    .padding(10)
    .into()
}

fn messages(app: &App, node: &Node) -> Element<'static, Message> {
    let mut list = column![].spacing(4);
    let eval = app.eval.node(node.id);
    let report = app.res.reports.get(&node.id);
    // What went wrong inside the graph a node uses is the node's too.
    let inner = app.res.inner.get(&node.id).cloned();
    let errors = eval.map(|e| e.errors.clone()).unwrap_or_default().into_iter().chain(report.and_then(|r| r.error.clone())).chain(inner);
    for e in errors {
        list = list.push(container(text(e).size(style::SMALL)).padding(6).width(Length::Fill).style(style::message_box(style::ERROR)));
    }
    let warnings = eval.map(|e| e.warnings.clone()).unwrap_or_default().into_iter().chain(report.and_then(|r| r.warning.clone()));
    for w in warnings {
        list = list.push(container(text(w).size(style::SMALL)).padding(6).width(Length::Fill).style(style::message_box(style::WARNING)));
    }
    // Always present, so the widgets below keep their place (and focus) when messages
    // appear or disappear while typing.
    container(list).width(Length::Fill).into()
}

fn node_view(app: &App, id: NodeId) -> Element<'_, Message> {
    let Some(node) = app.graph().node(id) else { return space().into() };
    let d = node.def();
    let title = crate::view::graph::node_title(node);
    let mut head = row![
        container(space().width(12).height(12)).style(style::chip(style::category_color(d.category))),
        text(title).size(16).font(style::bold()),
        space().width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if d.has_image() {
        let shown = app.ui.preview.node == Some(id);
        head = head.push(tip(
            button(icon(Icon::Eye, 15.0, if shown { style::ACCENT } else { style::TEXT_DIM }))
                .padding(4)
                .style(style::toggle_button(shown))
                .on_press(Message::Graph(crate::app::GraphMessage::Preview(id))),
            "Show in the 2D view (or double-click the node)",
            tooltip::Position::Left,
        ));
    }
    if d.has_image() && !d.inputs.is_empty() && node.kind != NodeKind::OUTPUT {
        head = head.push(tip(
            button(icon(Icon::Bypass, 15.0, if node.bypass { style::ACCENT } else { style::TEXT_DIM }))
                .padding(4)
                .style(style::toggle_button(node.bypass))
                .on_press(Message::ToggleBypass(id)),
            "Bypass: pass the first input through unchanged (Ctrl+B)",
            tooltip::Position::Left,
        ));
    }
    let kind = match &node.instance {
        Some(instance) => {
            let from = instance.graph.package.as_deref().map_or_else(|| "this package".to_string(), brewed_texture_core::package::file_name);
            format!("Graph · {from}")
        }
        None => format!("{} · {}", d.name, d.category.name()),
    };
    // A node that uses a graph: what the graph says it makes, if it says.
    let about = match &node.instance {
        Some(instance) if !instance.interface.description.is_empty() => instance.interface.description.as_str(),
        _ => d.description,
    };
    let mut col = column![
        head,
        text(kind).size(style::SMALL).color(style::TEXT_FAINT),
        text(about).size(style::SMALL).color(style::TEXT_DIM),
        messages(app, node),
    ]
    .spacing(6);
    if node.instance.is_some() {
        col = col.push(
            row![
                button(text("Open Reference").size(style::SMALL)).padding([4, 10]).style(style::button).on_press(Message::OpenReference(id)),
                text("Edit the graph it uses").size(style::SMALL).color(style::TEXT_FAINT),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
    }

    if node.variable_type().is_some() {
        col = col.push(variable_section(app, node));
    }

    col = col.push(section("Parameters"));
    let mut index = 0;
    for pd in node.param_defs() {
        if !node.param_visible(&pd) || matches!(pd.kind, ParamKind::Expression { .. }) {
            continue;
        }
        col = col.push(param_row(app, node, pd, index));
        index += 1;
    }
    if index == 0 && node.instance.is_some() {
        col = col.push(text("The graph has no input values: its Input nodes are its parameters.").size(style::SMALL).color(style::TEXT_FAINT));
    }

    // Node specific sections.
    if d.expression_param().is_some() {
        col = col.push(expression::view(app, node));
    }
    if node.kind == NodeKind::LEVELS {
        col = col.push(levels::view(app, node));
    }
    if node.kind == NodeKind::OUTPUT {
        col = col.push(section("Export"));
        col = col.push(
            row![
                button(text("Export this output…").size(style::SMALL)).padding([4, 10]).style(style::button).on_press(Message::ExportNode(id)),
                text("Ctrl+E exports all outputs").size(style::SMALL).color(style::TEXT_FAINT),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
    }
    if node.kind == NodeKind::TRANSFORM_2D {
        col = col.push(section("Gizmo"));
        col = col.push(text("Drag in the 2D view: the center moves, the ring rotates, the handles scale; Shift snaps rotation and keeps proportions.").size(style::SMALL).color(style::TEXT_DIM));
        let reset = ["offset", "rotation", "scale", "pivot"]
            .iter()
            .filter(|k| !node.is_exposed(k))
            .map(|k| Edit::SetParam { node: id, key: k.to_string(), value: d.param(k).expect("transform param").default_value() })
            .collect::<Vec<_>>();
        col = col.push(button(text("Reset transform").size(style::SMALL)).padding([4, 10]).style(style::button).on_press(Message::Edits(reset)));
    }
    col.into()
}

fn variable_section<'a>(app: &'a App, node: &'a Node) -> Element<'a, Message> {
    let id = node.id;
    let current = node.variable.clone().unwrap_or_default();
    let ty = node.variable_type();
    let editing = app.ui.binding.as_ref().filter(|(n, _)| *n == id).map(|(_, t)| t.clone());
    let others: Vec<String> =
        app.graph().variables.iter().filter(|v| Some(v.ty) == ty && v.name != current).map(|v| v.name.clone()).collect();
    let mut col = column![
        section("Input value"),
        row![
            text("Name").size(style::SMALL).color(style::TEXT_DIM).width(Length::Fixed(LABEL_W)),
            // A click anywhere else in the window takes the name as Enter does.
            outside(
                text_input("variable name", editing.as_deref().unwrap_or(&current))
                    .on_input(move |t| Message::BindingText(id, t))
                    .on_submit(Message::CommitBinding)
                    .size(style::TEXT_SIZE)
                    .padding([3, 6])
                    .style(style::text_input),
            )
            .on_press(editing.is_some().then_some(Message::CommitBinding)),
        ]
        .align_y(Alignment::Center),
        text("Enter a name to use another value. Input nodes with the same name share one value.")
            .size(style::SMALL)
            .color(style::TEXT_FAINT),
    ]
    .spacing(6);
    if !others.is_empty() {
        col = col.push(
            row![
                text("Use existing").size(style::SMALL).color(style::TEXT_DIM).width(Length::Fixed(LABEL_W)),
                pick_list(others, None::<String>, move |name| Message::Edit(Edit::BindVariable { node: id, name }))
                    .placeholder("choose…")
                    .text_size(style::TEXT_SIZE)
                    .padding([3, 6])
                    .style(style::pick_list),
            ]
            .align_y(Alignment::Center),
        );
    }
    if let Some(var) = node.variable.as_deref().and_then(|n| app.graph().variable(n)) {
        let color_open = app.ui.color_picker == Some(ColorTarget::Variable(var.name.clone(), crate::app::VarField::Value));
        let name = var.name.clone();
        // The value of an image input is a picture.
        let editor: Element<'a, Message> = match &var.image {
            Some(_) => {
                let color = crate::view::inputs::image_color(var);
                file_field(app, FileTarget::Variable(name), "no picture: the color".into(), Some(color))
            }
            None => crate::view::editors::value_editor(
                var.value,
                Some(var.default),
                var.has_range().then_some((var.min, var.max)),
                move |value| Message::Edit(Edit::SetVariable { name: name.clone(), value }),
                Message::Seal,
                color_open,
                Message::ColorPicker(Some(ColorTarget::Variable(var.name.clone(), crate::app::VarField::Value))),
            ),
        };
        col = col.push(row![text("Value").size(style::SMALL).color(style::TEXT_DIM).width(Length::Fixed(LABEL_W)), editor].align_y(Alignment::Center));
        col = col.push(crate::view::inputs::details(app, var, false));
    }
    col.into()
}

/// The value driving an exposed parameter, or a note that nothing is connected.
/// "← node" text that selects the node a value comes from.
fn source_link<'a>(from: NodeId, label: String) -> Element<'a, Message> {
    tip(
        button(text(label).size(style::SMALL).color(style::TEXT_FAINT))
            .padding([0, 3])
            .style(style::flat_button)
            .on_press(Message::SelectNode(from)),
        "Select the node this value comes from",
        tooltip::Position::Top,
    )
}

fn locked_editor<'a>(app: &'a App, node: &'a Node, pd: &ParamDef<'a>, slot_value: &'a ParamValue) -> Element<'a, Message> {
    // An image input: the node images arrive from, or the picture it takes without one.
    if let ParamKind::Image { kind } = pd.kind {
        let color = style::image_color(Some(kind));
        let says: Element<'a, Message> = match app.graph().link_into(node.id, &param_socket_key(pd.key)) {
            Some(link) => {
                let from = app.graph().node(link.from).map(crate::view::graph::node_title).unwrap_or_default();
                row![socket_icon(DataType::Image, true, 12.0, color), source_link(link.from, format!("← {from}"))]
                    .spacing(6)
                    .align_y(Alignment::Center)
                    .into()
            }
            None => row![
                socket_icon(DataType::Image, false, 12.0, style::WARNING),
                text(format!("not connected · uses {}", graph_picture(node, pd.key, slot_value, app.doc.package()).0))
                    .size(style::SMALL)
                    .color(style::WARNING)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into(),
        };
        return container(says).padding([4, 6]).width(Length::Fill).clip(true).style(style::field_box).into();
    }
    let eval = app.eval.node(node.id);
    let ty = slot_value.socket_type().unwrap_or(brewed_texture_core::DataType::Float);
    let color = style::type_color(ty);
    let link = app.graph().link_into(node.id, &param_socket_key(pd.key));
    match (eval.and_then(|e| e.driven.get(pd.key)), link) {
        (Some(v), Some(link)) => {
            let shown = match v {
                ParamValue::Enum(i) => pd.enum_options().get(*i as usize).copied().unwrap_or("?").to_string(),
                other => other.as_value().map_or_else(String::new, |x| x.display()),
            };
            let from = app.graph().node(link.from).map(crate::view::graph::node_title).unwrap_or_default();
            let mut r = row![socket_icon(ty, true, 12.0, color)].spacing(6).align_y(Alignment::Center);
            if let ParamValue::Color(c) = v {
                r = r.push(container(space().width(Length::Fixed(22.0)).height(Length::Fixed(12.0))).style(style::chip(style::color_of(*c))));
            }
            r = r.push(text(shown).size(style::SMALL).color(color));
            r = r.push(source_link(link.from, format!("← {from}")));
            container(r).padding([4, 6]).width(Length::Fill).style(style::field_box).into()
        }
        _ => {
            let stored = match slot_value {
                ParamValue::Enum(i) => pd.enum_options().get(*i as usize).copied().unwrap_or("?").to_string(),
                other => other.as_value().map_or_else(String::new, |x| x.display()),
            };
            container(
                row![
                    socket_icon(ty, false, 12.0, style::WARNING),
                    text(format!("not connected · uses {stored}")).size(style::SMALL).color(style::WARNING),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([4, 6])
            .width(Length::Fill)
            .style(style::field_box)
            .into()
        }
    }
}

/// What the picture of an image input of the graph a node uses is, in a few words, and the
/// color seen when it is none: its own (`value`), else the graph's.
fn graph_picture(node: &Node, key: &str, value: &ParamValue, package: &Package) -> (String, Option<[f32; 4]>) {
    if let ParamValue::Text(own) = value
        && !own.trim().is_empty()
    {
        return (package.file_label(own), None);
    }
    let param = node.instance.as_ref().and_then(|i| i.interface.param(key));
    match param.and_then(|p| Some((p.image.as_ref()?, p.value))) {
        Some((image, _)) if !image.picture.is_empty() => (format!("the graph's: {}", package.file_label(&image.picture)), None),
        Some((_, Value::Color(c))) => ("the graph's color".to_string(), Some(c)),
        _ => ("the graph's".to_string(), None),
    }
}

/// A parameter value as a few words, for the hint that offers to put it back.
fn shown(pd: &ParamDef<'_>, value: &ParamValue) -> String {
    if let (ParamKind::Image { .. }, ParamValue::Text(t)) = (pd.kind, value)
        && t.trim().is_empty()
    {
        return "the graph's picture".to_string();
    }
    match value {
        ParamValue::Enum(i) => pd.enum_options().get(*i as usize).map_or_else(|| i.to_string(), |s| s.to_string()),
        ParamValue::Color(c) => brewed_texture_core::Value::Color(*c).display(),
        ParamValue::Text(t) if t.is_empty() => "nothing".to_string(),
        ParamValue::Text(t) => format!("'{}'", crate::view::graph::truncate(t, 24)),
        ParamValue::Gradient(_) => "the gradient it came with".to_string(),
        ParamValue::Curve(_) => "the curve it came with".to_string(),
        other => other.as_value().map_or_else(|| "its default".to_string(), |v| v.display()),
    }
}

fn param_row<'a>(app: &'a App, node: &'a Node, pd: ParamDef<'a>, index: usize) -> Element<'a, Message> {
    let id = node.id;
    let Some(slot) = node.param(pd.key) else { return space().into() };
    let d = node.def();
    // Value nodes produce numbers, not images: tiling is shown but does not apply.
    let not_applicable = pd.key == "tiling" && !d.has_image();
    let exposed = slot.exposed;
    // A value the user set reads differently from the one the node came with, and the
    // middle button puts the original back.
    let default = pd.default_value();
    let changed = !exposed && !not_applicable && slot.value != default;
    let label_color = match (not_applicable, changed) {
        (true, _) => style::TEXT_FAINT,
        (false, true) => style::ACCENT,
        (false, false) => style::TEXT,
    };
    // Italics and a star: the name says the value is no longer the one it came with, and
    // so does the field itself.
    let name = if changed { format!("{}*", pd.label) } else { pd.label.to_string() };
    let mut caption = text(name).size(style::TEXT_SIZE).color(label_color).width(Length::Fixed(LABEL_W));
    if changed {
        caption = caption.font(style::italic());
    }
    let hint = if not_applicable {
        "Value nodes produce numbers, not images: tiling does not apply to them".to_string()
    } else if changed {
        format!("{}

Changed — middle-click the row to put {} back", pd.tooltip, shown(&pd, &default))
    } else {
        pd.tooltip.to_string()
    };
    let label = tip(caption, hint, tooltip::Position::Top);
    let editor: Element<'a, Message> = if exposed {
        locked_editor(app, node, &pd, &slot.value)
    } else if not_applicable {
        container(text(Tiling::LABELS[node.enum_value("tiling") as usize % 4]).size(style::SMALL).color(style::TEXT_FAINT))
            .padding([4, 6])
            .width(Length::Fill)
            .style(style::field_box)
            .into()
    } else {
        editor(app, node, pd, &slot.value)
    };
    let toggle = expose_toggle(
        pd.can_expose().then(|| pd.socket(&slot.value).map(|(ty, _)| ty)).flatten(),
        exposed,
        (pd.can_expose() && !not_applicable)
            .then(|| Message::Edit(Edit::SetExposed { node: id, key: pd.key.to_string(), exposed: !exposed })),
    );
    // The editor fills the row, so the expose toggles line up whatever the editor's own width
    // (a toggler, for instance, is narrow).
    let editor = container(editor).width(Length::Fill);
    let mut col = column![row![label, editor, toggle].spacing(6).align_y(Alignment::Center)].spacing(6);

    // A gradient is wide enough to need the room under the row; the color picker floats
    // over the interface instead, so that opening it moves nothing.
    if !exposed && let ParamValue::Gradient(g) = &slot.value {
        col = col.push(gradient_block(app, node, pd.key, g));
    }
    // So is a curve, whose square is under the row.
    if let ParamValue::Curve(c) = &slot.value {
        let (id, key) = (node.id, pd.key);
        col = col.push(curve_editor(c, move |c| set(id, key, ParamValue::Curve(c))).on_release(Message::Seal));
    }
    // Always the same widget around the row, whether the value was changed or not: a row
    // that changed shape under the mouse would lose what the field was doing. Putting back
    // a value that is already there changes nothing.
    let row = container(col).padding([3, 4]).width(Length::Fill).style(style::stripe(index.is_multiple_of(2)));
    let back = Edit::SetParam { node: id, key: pd.key.to_string(), value: default };
    mouse_area(row).on_middle_press(Message::Edits(vec![back])).into()
}

fn editor<'a>(app: &'a App, node: &'a Node, pd: ParamDef<'a>, value: &'a ParamValue) -> Element<'a, Message> {
    let id = node.id;
    let key = pd.key;
    // Most parameters may be dragged past their slider; the few that would mean nothing
    // outside it stop there.
    let default = pd.default_value();
    let held = |field: crate::widget::number::NumberField<'a, Message>, at: usize| {
        let field = match &default {
            ParamValue::Float(d) => field.default_value(*d as f64),
            ParamValue::Int(d) => field.default_value(*d as f64),
            ParamValue::Vec2(d) => field.default_value(d[at] as f64),
            _ => field,
        };
        match pd.limits {
            Some((lo, hi)) => field.limits(lo as f64, hi as f64),
            None => field,
        }
    };
    match (pd.kind, value) {
        (ParamKind::Float { min, max, log, suffix, .. }, ParamValue::Float(v)) => held(number(*v as f64, min as f64, max as f64), 0)
            .logarithmic(log)
            .suffix(suffix)
            .on_change(move |x| set(id, key, ParamValue::Float(x as f32)))
            .on_release(Message::Seal)
            .into(),
        (ParamKind::Int { min, max, .. }, ParamValue::Int(v)) => held(number(*v as f64, min as f64, max as f64), 0)
            .integer(true)
            .on_change(move |x| set(id, key, ParamValue::Int(x as i32)))
            .on_release(Message::Seal)
            .into(),
        (ParamKind::Vec2 { min, max, .. }, ParamValue::Vec2(v)) => {
            let v = *v;
            row![
                held(number(v[0] as f64, min as f64, max as f64), 0)
                    .prefix("x ")
                    .on_change(move |x| set(id, key, ParamValue::Vec2([x as f32, v[1]])))
                    .on_release(Message::Seal),
                held(number(v[1] as f64, min as f64, max as f64), 1)
                    .prefix("y ")
                    .on_change(move |y| set(id, key, ParamValue::Vec2([v[0], y as f32])))
                    .on_release(Message::Seal),
            ]
            .spacing(4)
            .into()
        }
        (ParamKind::Bool { .. }, ParamValue::Bool(b)) => toggler(*b)
            .on_toggle(move |x| set(id, key, ParamValue::Bool(x)))
            .size(16)
            .style(style::toggler)
            .into(),
        (ParamKind::Color { .. }, ParamValue::Color(c)) => {
            let open = app.ui.color_picker == Some(ColorTarget::Param(id, key.to_string()));
            swatch(*c, open, Message::ColorPicker(Some(ColorTarget::Param(id, key.to_string()))))
        }
        // (Only a node type's choices have pictures.)
        (ParamKind::Enum { .. }, ParamValue::Enum(v)) if pd.pictured && node.def().param(key).is_some() => {
            crate::view::choices::field(app, node, node.def().param(key).expect("checked"), *v)
        }
        (ParamKind::Enum { options, .. }, ParamValue::Enum(v)) => {
            let choices: Vec<Choice> = options.iter().enumerate().map(|(i, l)| Choice { index: i as u32, label: l }).collect();
            let selected = choices.get(*v as usize).cloned();
            pick_list(choices, selected, move |c: Choice| set(id, key, ParamValue::Enum(c.index)))
                .width(Length::Fill)
                .text_size(style::TEXT_SIZE)
                .padding([3, 6])
                .style(style::pick_list)
                .into()
        }
        // A file: chosen, or dragged from the Explorer; not a path to type.
        (ParamKind::File { .. }, ParamValue::Text(_)) => {
            let none = if node.kind == NodeKind::TEXT { "none: Inter, built in" } else { "no file" };
            file_field(app, FileTarget::Param(id, key.to_string()), none.to_string(), None)
        }
        // The picture a node gives an image input of the graph it uses; none of its own: the
        // graph's, which the field says.
        (ParamKind::Image { .. }, ParamValue::Text(_)) => {
            let (none, color) = graph_picture(node, key, &ParamValue::Text(String::new()), app.doc.package());
            file_field(app, FileTarget::Param(id, key.to_string()), none, color)
        }
        (ParamKind::Text { .. }, ParamValue::Text(s)) => text_input("", s)
            .on_input(move |t| set(id, key, ParamValue::Text(t)))
            .size(style::TEXT_SIZE)
            .padding([3, 6])
            .style(style::text_input)
            .into(),
        (ParamKind::Curve { .. }, ParamValue::Curve(c)) => row![
            space().width(Length::Fill),
            tip(
                button(icon(Icon::Reverse, 14.0, style::TEXT_DIM)).padding(4).style(style::button).on_press(set(id, key, ParamValue::Curve(c.inverted()))),
                "Upside down: dark for light",
                tooltip::Position::Top,
            ),
            tip(
                button(icon(Icon::Reset, 14.0, style::TEXT_DIM)).padding(4).style(style::button).on_press(set(id, key, ParamValue::Curve(Curve::default()))),
                "Straight: every value as it is",
                tooltip::Position::Top,
            ),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into(),
        (ParamKind::Gradient { .. }, ParamValue::Gradient(g)) => {
            let interpolation = g.interpolation;
            let g2 = g.clone();
            pick_list(Interpolation::ALL, Some(interpolation), move |i| {
                set(id, key, ParamValue::Gradient(Gradient { interpolation: i, ..g2.clone() }))
            })
            .width(Length::Fill)
            .text_size(style::TEXT_SIZE)
            .padding([3, 6])
            .style(style::pick_list)
            .into()
        }
        _ => text(format!("{value:?}")).size(style::SMALL).into(),
    }
}

/// Gradient editor with its tools and the selected stop.
fn gradient_block<'a>(app: &'a App, node: &'a Node, key: &'a str, g: &'a Gradient) -> Element<'a, Message> {
    let id = node.id;
    let selected = app.ui.gradient_stop.get(&(id, key.to_string())).copied().filter(|i| *i < g.stops.len());
    let editor = gradient_editor(
        g,
        selected,
        move |g| set(id, key, ParamValue::Gradient(g)),
        move |i| Message::GradientStop(id, key.to_string(), i),
    )
    .on_release(Message::Seal);
    let tools = row![
        eyedropper(
            PickTarget::Gradient(id, key.to_string()),
            app.picking(),
            "Pick gradient",
            "Drag across any image on any monitor: the colors along the path become the gradient. Esc or a right click cancels.",
        ),
        space().width(Length::Fill),
        tip(
            button(icon(Icon::Reverse, 14.0, style::TEXT_DIM)).padding(4).style(style::button).on_press(set(id, key, ParamValue::Gradient(g.reversed()))),
            "Reverse",
            tooltip::Position::Top,
        ),
        tip(
            button(icon(Icon::Distribute, 14.0, style::TEXT_DIM)).padding(4).style(style::button).on_press(set(id, key, ParamValue::Gradient(g.distributed()))),
            "Spread the stops evenly",
            tooltip::Position::Top,
        ),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    let mut col = column![editor, tools].spacing(6);
    if let Some(i) = selected {
        let stop = g.stops[i];
        let removable = g.stops.len() > 2;
        let mut removed = g.clone();
        if removable {
            removed.stops.remove(i);
        }
        let g_pos = g.clone();
        col = col.push(
            row![
                text(format!("Stop {} of {}", i + 1, g.stops.len())).size(style::SMALL).color(style::TEXT_DIM),
                space().width(Length::Fill),
                text("Position").size(style::SMALL).color(style::TEXT_DIM),
                container(
                    number(stop.pos as f64, 0.0, 1.0)
                        .limits(0.0, 1.0)
                        .on_change(move |p| {
                            // Keep the stop selected while it moves past others.
                            let mut n = g_pos.clone();
                            let mut s = n.stops.remove(i);
                            s.pos = p as f32;
                            let j = n.stops.partition_point(|x| x.pos <= s.pos);
                            n.stops.insert(j, s);
                            Message::SetGradient(id, key.to_string(), n, Some(j))
                        })
                        .on_release(Message::Seal)
                )
                .width(Length::Fixed(90.0)),
                tip(
                    button(icon(Icon::Trash, 14.0, if removable { style::TEXT_DIM } else { style::TEXT_FAINT }))
                        .padding(4)
                        .style(style::button)
                        .on_press_maybe(removable.then(|| {
                            Message::SetGradient(id, key.to_string(), removed.clone(), Some(i.saturating_sub(1)))
                        })),
                    "Remove the stop (or drag it down, off the ramp)",
                    tooltip::Position::Top,
                ),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
        // Its color, in the picker that floats where it is opened.
        let target = ColorTarget::GradientStop(id, key.to_string());
        let open = app.ui.color_picker.as_ref() == Some(&target);
        col = col.push(
            row![
                text("Color").size(style::SMALL).color(style::TEXT_DIM),
                swatch(stop.color, open, Message::ColorPicker(Some(target))),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    } else {
        col = col.push(
            text("Click the ramp to add a stop, or a handle to change its color and position.")
                .size(style::SMALL)
                .color(style::TEXT_FAINT),
        );
    }
    container(col).padding(iced::Padding { top: 2.0, right: 0.0, bottom: 4.0, left: 0.0 }).into()
}
