//! Expression section of Math and Pixel Math nodes (the program of an FX-Map): highlighted
//! editor, errors, variables, a plot of the function and a reference of functions and
//! constants.

use iced::alignment::Vertical;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::widget::text::{Alignment as TextAlign, LineHeight, Shaping};
use iced::widget::{button, column, container, pick_list, row, text, text_editor};
use iced::{Alignment, Element, Length, Pixels, Point, Rectangle, mouse};
use brewed_texture_core::eval::VarSource;
use brewed_texture_core::expr::{self, Language, builtins};
use brewed_texture_core::graph::Node;
use brewed_texture_core::nodes::{FX_MAP_BUILTINS, FX_MAP_OUTPUTS, MATH_BUILTINS, PIXEL_MATH_BUILTINS};
use brewed_texture_core::{Edit, NodeKind, Value};

use crate::app::{App, Message};
use crate::highlight::{ExpressionHighlighter, Settings};
use crate::icons::{Icon, icon};
use crate::style;
use crate::view::section;
use crate::widget::number::number;

/// Id of the expression editor (for focusing it).
pub fn editor_id() -> iced::widget::Id {
    iced::widget::Id::new("brewed-expression-editor")
}

pub fn view<'a>(app: &'a App, node: &'a Node) -> Element<'a, Message> {
    let id = node.id;
    let source = node.expression();
    let language = node.def().expression_language().unwrap_or(Language::Math);
    let script = language == Language::Splats;
    let error = language.error(source);
    let settings = Settings::for_error(source, error.as_ref().map(|e| e.span));

    let editor: Element<'a, Message> = match &app.ui.expression {
        // A finite maximum height matters: inside the scrollable panel the editor would
        // otherwise be laid out with an infinite height and draw no lines at all.
        Some((eid, content)) if *eid == id => text_editor(content)
            .id(editor_id())
            .on_action(move |a| Message::Expression(id, a))
            .highlight_with::<ExpressionHighlighter>(settings, |h, theme| h.to_format(theme))
            .font(style::mono())
            .size(14)
            .padding(8)
            .min_height(38)
            .max_height(if script { 360.0 } else { 200.0 })
            .style(style::text_editor)
            .into(),
        _ => container(text(source).font(style::mono()).size(14)).padding(8).style(style::field_box).into(),
    };

    // The status line under the editor always exists, so the editor keeps its place in
    // the widget tree (and its focus) when errors come and go while typing.
    let status: Element<'a, Message> = match &error {
        Some(e) if script => text(format!("The program is incorrect: {}", e.message)).size(style::SMALL).color(style::ERROR).into(),
        Some(e) => text(format!("The expression is incorrect: {}", e.message)).size(style::SMALL).color(style::ERROR).into(),
        None if node.kind == NodeKind::MATH => match app.eval.value(id) {
            Some(v) => text(format!("= {}", v.display())).size(15).font(style::mono()).color(style::TEXT).into(),
            None => text("").size(style::SMALL).into(),
        },
        None if script => text("Run for every splat on the GPU").size(style::SMALL).color(style::TEXT_FAINT).into(),
        None => text("Evaluated for every pixel on the GPU").size(style::SMALL).color(style::TEXT_FAINT).into(),
    };

    let title = section(if script { "Program" } else { "Expression" });
    let mut col = column![title, editor, container(status).height(Length::Fixed(22.0)).align_y(Vertical::Center)].spacing(6);

    // Variables.
    let vars = app.eval.node(id).map(|e| e.vars.clone()).unwrap_or_default();
    if !vars.is_empty() {
        col = col.push(section("Variables"));
        for v in vars {
            let name = text(v.name.clone()).size(style::TEXT_SIZE).font(style::mono()).color(style::rgb(156, 220, 254)).width(Length::Fixed(90.0));
            let detail: Element<'a, Message> = match &v.source {
                VarSource::Local => {
                    let var_name = v.name.clone();
                    row![
                        container(
                            number(v.value, -10.0, 10.0)
                                .on_change(move |x| Message::Edit(Edit::SetLocalVar { node: id, name: var_name.clone(), value: x as f32 }))
                                .on_release(Message::Seal)
                        )
                        .width(Length::Fixed(110.0)),
                        text("own value · connect or name a graph input").size(style::SMALL).color(style::TEXT_FAINT),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center)
                    .wrap()
                    .vertical_spacing(2)
                    .into()
                }
                VarSource::Link(from) => {
                    let title = app.graph().node(*from).map(crate::view::graph::node_title).unwrap_or_default();
                    let (from, value, label) = (*from, Value::Float(v.value as f32).display(), format!("← {title}"));
                    // (Under the value where they are not both whole beside each other, and no
                    // wider than the panel: a long name of a node ran out of it.)
                    iced::widget::responsive(move |room| {
                        row![text(value.clone()).size(style::SMALL), super::source_link_in(from, &label, room.width)]
                            .spacing(8)
                            .align_y(Alignment::Center)
                            .wrap()
                            .vertical_spacing(2)
                            .into()
                    })
                    .height(Length::Shrink)
                    .into()
                }
                VarSource::Variable(name) => row![
                    text(Value::Float(v.value as f32).display()).size(style::SMALL),
                    text(format!("input value '{name}'")).size(style::SMALL).color(style::TEXT_FAINT),
                ]
                .spacing(8)
                .wrap()
                .vertical_spacing(2)
                .into(),
                // (Of Math, the size of the texture: as much as it is now.)
                VarSource::Builtin if node.kind == NodeKind::MATH => row![
                    text(Value::Float(v.value as f32).display()).size(style::SMALL),
                    text("the texture's size, in pixels (built in)").size(style::SMALL).color(style::TEXT_FAINT),
                ]
                .spacing(8)
                .wrap()
                .vertical_spacing(2)
                .into(),
                VarSource::Builtin => {
                    text(if script { "per splat (built in)" } else { "per pixel (built in)" }).size(style::SMALL).color(style::TEXT_FAINT).into()
                }
                VarSource::Image(from) => {
                    let title = app.graph().node(*from).map(crate::view::graph::node_title).unwrap_or_default();
                    let (from, label) = (*from, format!("image ← {title}"));
                    iced::widget::responsive(move |room| super::source_link_in(from, &label, room.width)).height(Length::Shrink).into()
                }
            };
            // (What it is goes under its field where the panel is narrow: squeezed beside it, it
            // stood a word a line.)
            col = col.push(row![name, detail].spacing(6).align_y(Alignment::Center));
        }
    }

    // Plot of a Math expression over one variable.
    let compiled = expr::compile_cached(source);
    if node.kind == NodeKind::MATH
        && let Ok(program) = &*compiled
        && !program.vars.is_empty()
    {
        let var = app.ui.plot_var.min(program.vars.len() - 1);
        let (lo, hi) = app.ui.plot_range;
        col = col.push(section("Plot"));
        // (On as many lines as the panel needs, each part whole: in a narrow one the last field ran
        // off its edge.)
        let part = |r: iced::widget::Row<'a, Message>| r.spacing(6).align_y(Alignment::Center);
        col = col.push(
            row![
                part(row![
                    text("f(").size(style::SMALL),
                    pick_list(program.vars.clone(), Some(program.vars[var].clone()), {
                        let names = program.vars.clone();
                        move |n: String| Message::PlotVar(names.iter().position(|x| *x == n).unwrap_or(0))
                    })
                    .text_size(style::SMALL)
                    .padding([2, 6])
                    .style(style::pick_list),
                    text(")").size(style::SMALL),
                ]),
                part(row![
                    text("from").size(style::SMALL),
                    container(number(lo as f64, -100.0, 100.0).on_change(move |x| Message::PlotRange(x as f32, hi))).width(Length::Fixed(70.0)),
                ]),
                part(row![
                    text("to").size(style::SMALL),
                    container(number(hi as f64, -100.0, 100.0).on_change(move |x| Message::PlotRange(lo, x as f32))).width(Length::Fixed(70.0)),
                ]),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .wrap()
            .vertical_spacing(4),
        );
        let args: Vec<f64> = app.eval.node(id).map(|e| e.vars.iter().map(|v| v.value).collect()).unwrap_or_default();
        col = col.push(
            iced::widget::canvas(Plot { program: program.clone(), args, var, lo, hi })
                .width(Length::Fill)
                .height(Length::Fixed(130.0)),
        );
    }

    // Reference.
    let open = app.ui.show_functions;
    col = col.push(
        button(
            row![
                icon(if open { Icon::ChevronDown } else { Icon::ChevronRight }, 12.0, style::TEXT_DIM),
                text("Functions & constants").size(style::SMALL).font(style::bold()).color(style::TEXT_DIM)
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([6, 2])
        .style(style::flat_button)
        .on_press(Message::ShowFunctions),
    );
    if open {
        let mut list = column![].spacing(2);
        if node.kind == NodeKind::MATH {
            list = list.push(text(format!("Built-ins: {}", MATH_BUILTINS.join(", "))).size(style::SMALL));
            list = list.push(
                text("width, height: the size of the texture the graph is computed at, in pixels (as $size in Substance Designer)")
                    .size(style::SMALL)
                    .color(style::TEXT_DIM),
            );
        }
        if node.kind == NodeKind::PIXEL_MATH {
            list = list.push(text(format!("Per-pixel built-ins: {}", PIXEL_MATH_BUILTINS.join(", "))).size(style::SMALL));
            list = list.push(
                text("u, v: 0..1 image coordinates · px, py: pixel coordinates · width, height: image size")
                    .size(style::SMALL)
                    .color(style::TEXT_DIM),
            );
        }
        if script {
            let outputs: Vec<&str> = FX_MAP_OUTPUTS.iter().map(|(o, ..)| *o).collect();
            list = list.push(text(format!("A splat is: {}", outputs.join(", "))).size(style::SMALL));
            list = list.push(
                text(
                    "x, y: where in its cell, 0..1 (0.5) · size: a share of the cell, or width, height (1) · angle: \
                     degrees (0) · value (1) · color: r, g, b[, a] · opacity (1) · draw: 0 leaves it out (1) · image: \
                     pattern input 1..4 (1)",
                )
                .size(style::SMALL)
                .color(style::TEXT_DIM),
            );
            list = list.push(text(format!("Per-splat built-ins: {}", FX_MAP_BUILTINS.join(", "))).size(style::SMALL));
            list = list.push(
                text(
                    "i: its number in the cell, from 0 · count · depth · cells: across at the depth · col, row: its cell · \
                     u, v: the middle of its cell, 0..1 · seed · width, height: image size",
                )
                .size(style::SMALL)
                .color(style::TEXT_DIM),
            );
            list = list.push(
                text("One name = value after another (a line each, or ; between them); what is set is read after it; # comments")
                    .size(style::SMALL)
                    .color(style::TEXT_DIM),
            );
        }
        list = list.push(
            text(
                "Operators: + - * / % ^ (or **), < <= > >= == !=, && || !, cond ? a : b, implicit 2x · Vectors: vec2, vec3, \
                 vec4, worked on component by component; .x .xy .rgb … take their components (of an image input, its \
                 channels) · name = value lines before the value (or ; between them) give names values; # comments",
            )
                .size(style::SMALL)
                .color(style::TEXT_DIM),
        );
        for f in builtins::FUNCTIONS {
            list = list.push(
                row![
                    text(f.signature).size(style::SMALL).font(style::mono()).width(Length::Fixed(170.0)),
                    text(f.doc).size(style::SMALL).color(style::TEXT_DIM)
                ]
                .spacing(8),
            );
        }
        for c in builtins::CONSTANTS {
            list = list.push(
                row![
                    text(c.name).size(style::SMALL).font(style::mono()).width(Length::Fixed(170.0)),
                    text(c.doc).size(style::SMALL).color(style::TEXT_DIM)
                ]
                .spacing(8),
            );
        }
        col = col.push(container(list).padding(6).style(style::field_box));
    }
    col.into()
}

struct Plot {
    program: expr::Program,
    args: Vec<f64>,
    var: usize,
    lo: f32,
    hi: f32,
}

impl canvas::Program<Message> for Plot {
    type State = ();

    fn draw(&self, _state: &(), renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), style::FIELD);
        let (lo, hi) = (self.lo as f64, self.hi as f64);
        let mut args = self.args.clone();
        args.resize(self.program.vars.len(), 0.0);
        let current = args.get(self.var).copied().unwrap_or(0.0);
        let n = 256;
        let samples: Vec<(f64, f64)> = (0..n)
            .map(|i| {
                let x = lo + (hi - lo) * i as f64 / (n - 1) as f64;
                args[self.var] = x;
                (x, self.program.eval(&args))
            })
            .collect();
        let finite: Vec<f64> = samples.iter().map(|s| s.1).filter(|v| v.is_finite()).collect();
        let (mut ymin, mut ymax) = finite.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
        if !ymin.is_finite() {
            ymin = -1.0;
            ymax = 1.0;
        }
        if (ymax - ymin).abs() < 1e-9 {
            ymin -= 1.0;
            ymax += 1.0;
        }
        let (w, h) = (bounds.width, bounds.height);
        let to = |x: f64, y: f64| {
            Point::new(((x - lo) / (hi - lo)) as f32 * w, h - 6.0 - ((y - ymin) / (ymax - ymin)) as f32 * (h - 12.0))
        };
        let axis = Stroke::default().with_width(1.0).with_color(style::gray(64));
        if ymin < 0.0 && ymax > 0.0 {
            let y = to(0.0, 0.0).y;
            frame.stroke(&Path::line(Point::new(0.0, y), Point::new(w, y)), axis);
        }
        if lo < 0.0 && hi > 0.0 {
            let x = to(0.0, 0.0).x;
            frame.stroke(&Path::line(Point::new(x, 0.0), Point::new(x, h)), axis);
        }
        // Break the curve at non-finite values and at jumps (asymptotes).
        let span = ymax - ymin;
        let mut segment: Vec<Point> = Vec::new();
        let flush = |frame: &mut Frame, segment: &mut Vec<Point>| {
            if segment.len() > 1 {
                let path = Path::new(|b| {
                    b.move_to(segment[0]);
                    for p in &segment[1..] {
                        b.line_to(*p);
                    }
                });
                frame.stroke(&path, Stroke::default().with_width(1.8).with_color(style::ACCENT));
            }
            segment.clear();
        };
        for pair in samples.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if a.1.is_finite() && b.1.is_finite() && (b.1 - a.1).abs() < span * 0.9 {
                if segment.is_empty() {
                    segment.push(to(a.0, a.1));
                }
                segment.push(to(b.0, b.1));
            } else {
                flush(&mut frame, &mut segment);
            }
        }
        flush(&mut frame, &mut segment);
        if current >= lo && current <= hi {
            let x = to(current, 0.0).x;
            frame.stroke(&Path::line(Point::new(x, 0.0), Point::new(x, h)), Stroke::default().with_width(1.0).with_color(style::gray(170)));
        }
        let label = |frame: &mut Frame, s: String, p: Point, v: Vertical| {
            // On a patch of the field: the curve passes under the number, not through it.
            let (wide, tall) = (crate::view::text_width_in(&s, 10.5, style::mono()) + 4.0, 10.5 * 1.3);
            let top = if v == Vertical::Bottom { p.y - tall } else { p.y };
            frame.fill_rectangle(Point::new(p.x - 2.0, top), iced::Size::new(wide, tall), style::alpha(style::FIELD, 0.85));
            frame.fill_text(Text {
                content: s,
                position: p,
                color: style::TEXT_FAINT,
                size: Pixels(10.5),
                font: style::mono(),
                align_x: TextAlign::Left,
                align_y: v,
                line_height: LineHeight::default(),
                shaping: Shaping::Basic,
                max_width: f32::INFINITY,
            })
        };
        label(&mut frame, format!("{ymax:.3}"), Point::new(4.0, 2.0), Vertical::Top);
        label(&mut frame, format!("{ymin:.3}"), Point::new(4.0, h - 2.0), Vertical::Bottom);
        vec![frame.into_geometry()]
    }
}
