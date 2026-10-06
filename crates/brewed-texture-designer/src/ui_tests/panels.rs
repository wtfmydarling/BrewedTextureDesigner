//! The panels beside the graph, driven as a hand drives them: Input Values, the library and the
//! Explorer.

use brewed_texture_core::Value;

use super::*;
use crate::app::Fold;

fn value_of(w: &Harness, name: &str) -> Option<Value> {
    w.app.graph().variables.iter().find(|v| v.name == name).map(|v| v.value)
}

fn middle_click(w: &mut Harness, at: Point) {
    w.hover(at);
    w.mouse(mouse::Event::ButtonPressed(mouse::Button::Middle));
    w.mouse(mouse::Event::ButtonReleased(mouse::Button::Middle));
}

/// An input value changed says so in its row (its name with a star, until it is back); a middle
/// click on the row puts the default back in one undo step, and on a row at its default changes
/// nothing.
#[test]
fn a_changed_input_value_says_so_and_middle_click_puts_it_back() {
    let mut w = harness!(view::root, 1400.0, 900.0);
    let default = value_of(&w, "wear").expect("the demo's wear");
    w.send(Message::Edit(Edit::SetVariable { name: "wear".into(), value: Value::Float(0.8) }));
    w.send(Message::Seal);
    w.input(&[]);
    assert!(texts(&mut w, "wear").is_empty() && !texts(&mut w, "wear*").is_empty(), "the name says it was changed");
    let row = *texts(&mut w, "wear*").first().unwrap();
    let steps = w.app.doc.undo_label().map(str::to_string);
    middle_click(&mut w, row.center());
    assert_eq!(value_of(&w, "wear"), Some(default), "the default, back");
    assert!(!texts(&mut w, "wear").is_empty() && texts(&mut w, "wear*").is_empty(), "and the name says so");
    w.send(Message::Undo);
    assert_eq!(value_of(&w, "wear"), Some(Value::Float(0.8)), "one undo: the change again");
    w.send(Message::Undo);
    assert_eq!(value_of(&w, "wear"), Some(default));
    // At its default: a middle click is no step.
    let steps_now = w.app.doc.undo_label().map(str::to_string);
    let row = *texts(&mut w, "wear").first().unwrap();
    middle_click(&mut w, row.center());
    assert_eq!(w.app.doc.undo_label().map(str::to_string), steps_now, "nothing was done");
    let _ = steps;
}

/// An input value opened shows its settings, and its buttons are there only while they would
/// do something: at its default neither resets nor makes a default; changed, Reset to default
/// puts the default back, Set as default makes the value the default (and the star goes).
#[test]
fn an_opened_input_value_resets_or_keeps_its_value() {
    // (Tall enough for the opened value's settings to show in the panel, where they are clicked.)
    let mut w = harness!(view::root, 1400.0, 1400.0);
    w.send(Message::ToggleVariable("mortar".into()));
    w.input(&[]);
    for label in ["Name", "Description", "Default", "Min", "Max", "Reset to default", "Set as default"] {
        assert!(!texts(&mut w, label).is_empty(), "{label} is shown");
    }
    let button = |w: &mut Harness, label: &str| *texts(w, label).first().unwrap_or_else(|| panic!("{label}"));
    let at = button(&mut w, "Set as default").center();
    assert!(clicked(&mut w, at).is_empty(), "at its default, Set as default does nothing");
    w.send(Message::Edit(Edit::SetVariable { name: "mortar".into(), value: Value::Float(0.1) }));
    w.send(Message::Seal);
    w.input(&[]);
    let at = button(&mut w, "Set as default").center();
    click(&mut w, at);
    let var = w.app.graph().variables.iter().find(|v| v.name == "mortar").unwrap().clone();
    assert_eq!((var.value, var.default), (Value::Float(0.1), Value::Float(0.1)), "the value is the default now");
    assert!(texts(&mut w, "mortar*").is_empty(), "no star");
    w.send(Message::Edit(Edit::SetVariable { name: "mortar".into(), value: Value::Float(0.2) }));
    w.send(Message::Seal);
    w.input(&[]);
    let at = button(&mut w, "Reset to default").center();
    click(&mut w, at);
    assert_eq!(value_of(&w, "mortar"), Some(Value::Float(0.1)), "back to the default");
    // Closed: the settings go.
    w.send(Message::ToggleVariable("mortar".into()));
    w.input(&[]);
    assert!(texts(&mut w, "Set as default").is_empty());
}

/// The grip of an input value, dragged onto the graph, is another Input node of it: the same
/// value, where it is dropped.
#[test]
fn an_input_value_dragged_onto_the_graph_is_another_input_node() {
    let mut w = harness!(view::root, 1400.0, 900.0);
    w.input(&[iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))]);
    let users = |w: &Harness| w.app.graph().variable_users("wear").count();
    let before = users(&w);
    w.send(Message::PanelPress(DragPayload::Variable("wear".into())));
    let to = w.app.ui.graph_view.bounds.center();
    let main = w.main_window();
    w.window_event(main, iced::Event::Mouse(mouse::Event::CursorMoved { position: Point::new(to.x - 40.0, to.y) }));
    w.window_event(main, iced::Event::Mouse(mouse::Event::CursorMoved { position: to }));
    assert!(w.app.ui.drag.is_some(), "being dragged");
    w.hover(to);
    w.mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    assert_eq!(users(&w), before + 1, "one more Input node of it");
    assert!(w.app.ui.drag.is_none());
}

/// What is typed in the library's search leaves the nodes it names or describes, in collapsed
/// categories too; nothing found says so; the search let go, every category is as it was.
#[test]
fn the_library_is_searched_and_its_categories_fold() {
    let mut w = harness!(view::root, 1400.0, 900.0);
    let shown = |w: &mut Harness, name: &str| !texts(w, name).is_empty();
    assert!(shown(&mut w, "Uniform Color"), "a generator, in the library");
    // Folded: its nodes go, its count stays.
    w.send(Message::LibraryToggle(brewed_texture_core::Category::Generators));
    w.input(&[]);
    assert!(!shown(&mut w, "Uniform Color"), "folded away");
    assert!(shown(&mut w, "GENERATORS"), "the heading stays");
    // Searched: what it finds, in the folded category too.
    w.send(Message::LibrarySearch("uniform".into()));
    w.input(&[]);
    assert!(shown(&mut w, "Uniform Color"), "found, though its category is folded");
    assert!(!shown(&mut w, "Blur"), "what it does not name goes");
    w.send(Message::LibrarySearch("qqqqzzzz".into()));
    w.input(&[]);
    assert!(shown(&mut w, "No matching node"));
    w.send(Message::LibrarySearch(String::new()));
    w.input(&[]);
    assert!(!shown(&mut w, "Uniform Color") && shown(&mut w, "Blur"), "as it was: the generators folded");
    w.send(Message::LibraryToggle(brewed_texture_core::Category::Generators));
    w.input(&[]);
    assert!(shown(&mut w, "Uniform Color"), "unfolded");
}

/// A node double-clicked in the library is added in the middle of the graph's view, selected.
#[test]
fn a_node_double_clicked_in_the_library_is_added_in_the_middle() {
    let mut w = harness!(view::root, 1400.0, 900.0);
    w.input(&[iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))]);
    let before = w.app.graph().nodes_of(NodeKind::INVERT).count();
    // (Found first: far down the list, it would be out of the panel.)
    w.send(Message::LibrarySearch("invert".into()));
    w.input(&[]);
    let item = *texts(&mut w, "Invert").first().expect("Invert in the library");
    let at = item.center();
    w.hover(at);
    for _ in 0..2 {
        w.mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        w.mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    }
    let added: Vec<_> = w.app.graph().nodes_of(NodeKind::INVERT).map(|n| (n.id, n.pos)).collect();
    assert_eq!(added.len(), before + 1, "an Invert");
    let (id, pos) = *added.last().unwrap();
    let middle = w.app.ui.graph_view.center();
    let size = view::graph::layout::node_size(w.app.graph(), w.app.graph().node(id).unwrap());
    let center = [pos[0] + size[0] / 2.0, pos[1] + size[1] / 2.0];
    assert!((center[0] - middle[0]).abs() < size[0] && (center[1] - middle[1]).abs() < size[1], "in the middle: {center:?} for {middle:?}");
    assert!(w.app.ui.selection.contains(&id), "selected");
}

/// A graph of the package is opened by a double click in the Explorer, duplicated by its menu
/// (named after it), and deleted by its menu while the package keeps another; folded, the
/// package shows none of them.
#[test]
fn graphs_are_opened_duplicated_and_deleted_in_the_explorer() {
    let mut w = harness!(view::root, 1400.0, 900.0);
    let key = w.app.doc_key;
    let first = w.app.doc.current();
    // With one graph, Delete is not offered.
    w.send(Message::Context(Some(ContextMenu { at: Point::new(120.0, 200.0), target: ContextTarget::GraphItem(key, first) })));
    w.input(&[]);
    let delete = *texts(&mut w, "Delete").last().expect("Delete in the menu");
    assert!(!clicked(&mut w, delete.center()).iter().any(|m| matches!(m, Message::FromMenu(_))), "the last graph is not deleted");
    w.send(Message::Context(None));
    w.send(Message::Explorer(ExplorerMessage::DuplicateGraph(key, first)));
    w.input(&[]);
    let names: Vec<String> = w.app.doc.package().graphs.iter().map(|g| g.name.clone()).collect();
    assert_eq!(names, ["Brick Wall", "Brick Wall 2"]);
    let copy = w.app.doc.package().graphs[1].id;
    // A double click on the copy's row opens it.
    let row = *texts(&mut w, "Brick Wall 2").first().expect("the copy in the Explorer");
    w.hover(row.center());
    for _ in 0..2 {
        w.mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        w.mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    }
    assert_eq!(w.app.doc.current(), copy, "the copy is edited");
    // Deleted while it is edited: the other is edited.
    w.send(Message::Explorer(ExplorerMessage::DeleteGraph(key, copy)));
    assert_eq!(w.app.doc.current(), first);
    assert_eq!(w.app.doc.package().graphs.len(), 1);
    w.input(&[]);
    assert!(texts(&mut w, "Brick Wall 2").is_empty(), "gone from the Explorer");
    // Folded.
    w.send(Message::Explorer(ExplorerMessage::Toggle(Fold::Package(key))));
    w.input(&[]);
    assert!(texts(&mut w, "Brick Wall").iter().all(|r| r.x > 300.0), "the package's graph is not listed");
    w.send(Message::Explorer(ExplorerMessage::Toggle(Fold::Package(key))));
    w.input(&[]);
    assert!(texts(&mut w, "Brick Wall").iter().any(|r| r.x < 300.0), "listed again");
}

/// A name too long for the Explorer's column is shortened in its middle, its end kept: graphs
/// whose names differ at their ends ("… 2") are told apart in the narrowest column (cut at its
/// edge, they read the same), and in a wide one each shows whole.
#[test]
fn long_names_in_the_explorer_are_shortened_in_their_middle() {
    let mut w = harness!(view::root, 1000.0, 640.0);
    let key = w.app.doc_key;
    let first = w.app.doc.current();
    let long = "Normal from Height of the Old Bricks";
    w.send(Message::Explorer(ExplorerMessage::StartRename(key, first)));
    w.send(Message::Explorer(ExplorerMessage::RenameText(long.into())));
    w.send(Message::Explorer(ExplorerMessage::CommitRename));
    w.send(Message::Explorer(ExplorerMessage::DuplicateGraph(key, first)));
    let names: Vec<String> = w.app.doc.package().graphs.iter().map(|g| g.name.clone()).collect();
    assert_eq!(names, [long.to_string(), format!("{long} 2")]);
    w.input(&[]);
    // What the Explorer shows of each.
    let shown = |w: &mut Harness| -> Vec<String> {
        struct Lines(Vec<(String, iced::Rectangle)>);
        impl Operation for Lines {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
                operate(self);
            }
            fn text(&mut self, _: Option<&iced::widget::Id>, bounds: iced::Rectangle, text: &str) {
                self.0.push((text.to_owned(), bounds));
            }
        }
        let mut lines = Lines(Vec::new());
        w.frame(|ui, renderer| ui.operate(renderer, &mut lines));
        lines.0.into_iter().filter(|(t, r)| r.x < 300.0 && t.starts_with("Normal fr")).map(|(t, _)| t).collect()
    };
    let narrow = shown(&mut w);
    assert_eq!(narrow.len(), 2, "{narrow:?}");
    assert!(narrow.iter().all(|t| t.contains('…') && t.starts_with("Normal")), "shortened in the middle: {narrow:?}");
    assert!(narrow[0].ends_with("Bricks") && narrow[1].ends_with("Bricks 2"), "their ends kept: {narrow:?}");
    assert_ne!(narrow[0], narrow[1], "told apart");
    assert!(text_fits::cut_short(&mut w).is_empty(), "{:?}", text_fits::cut_short(&mut w));
    // In a wide column, whole.
    w.size = Size::new(2560.0, 1400.0);
    w.app.ui.window_size = w.size;
    let split = match w.app.ui.panes.layout() {
        iced::widget::pane_grid::Node::Split { id, .. } => *id,
        _ => panic!("the panels are split"),
    };
    w.send(Message::PaneResized(iced::widget::pane_grid::ResizeEvent { split, ratio: 0.3 }));
    w.input(&[]);
    assert_eq!(shown(&mut w), [long.to_string(), format!("{long} 2")]);
}

/// Where in `area` a click asks for what `wanted` says, looked for every few pixels as a hand
/// finds a button by its look (what the clicks ask for is not done).
fn find_click(w: &mut Harness, area: iced::Rectangle, wanted: impl Fn(&Message) -> bool) -> Option<Point> {
    let mut y = area.y;
    while y < area.y + area.height {
        let mut x = area.x;
        while x < area.x + area.width {
            let at = Point::new(x, y);
            w.cursor = mouse::Cursor::Available(at);
            let asked = w.produced(&[
                iced::Event::Mouse(mouse::Event::CursorMoved { position: at }),
                iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            ]);
            if asked.iter().any(&wanted) {
                return Some(at);
            }
            x += 4.0;
        }
        y += 4.0;
    }
    None
}

/// The value a message sets the parameter `key` of a node to.
fn set_to(m: &Message, key: &str) -> Option<ParamValue> {
    match m {
        Message::Edit(Edit::SetParam { key: k, value, .. }) if k == key => Some(value.clone()),
        Message::SetGradient(_, k, g, _) if k == key => Some(ParamValue::Gradient(g.clone())),
        Message::Step(inner) => set_to(inner, key),
        _ => None,
    }
}

/// The buttons of a gradient (Reverse, Spread the stops evenly, remove the stop) and of a curve
/// (upside down, straight) are each a step of their own: pressed one after the other, an undo
/// takes back only the last (it took back every press of the last second and a half).
#[test]
fn the_buttons_of_a_gradient_and_a_curve_are_steps_of_their_own() {
    use brewed_texture_core::{Curve, GradientStop};
    let mut w = harness!(view::root, 1680.0, 1000.0);
    // (The buttons are on the right, on the row of the parameter or under it: its name, with a
    // star once it is changed, the lowest of the texts that say it: the node's name is above.)
    let right = |w: &mut Harness, label: &str| {
        let edge = w.size.width - 400.0;
        let starred = format!("{label}*");
        let rows = texts_where(w, |t| t == label || t == starred);
        let row = rows.into_iter().filter(|(_, r)| r.x > edge).map(|(_, r)| r.y).fold(f32::NEG_INFINITY, f32::max);
        assert!(row.is_finite(), "the row of {label}");
        iced::Rectangle::new(Point::new(w.size.width - 120.0, row - 8.0), Size::new(120.0, 240.0))
    };
    // A Gradient Map whose stops are not evenly spread, and one of them selected.
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::GRADIENT_MAP, pos: [-400.0, 900.0] }));
    let id = w.app.graph().nodes_of(NodeKind::GRADIENT_MAP).map(|n| n.id).max().expect("the node");
    let gradient = |w: &Harness| w.app.graph().node(id).and_then(|n| n.value("gradient")).and_then(ParamValue::as_gradient).cloned().unwrap();
    let mut uneven = gradient(&w);
    uneven.stops = [(0.0, 0.1), (0.2, 0.5), (1.0, 0.9)].map(|(pos, v)| GradientStop { pos, color: [v, v, v, 1.0] }).to_vec();
    w.send(Message::Edit(Edit::SetParam { node: id, key: "gradient".into(), value: ParamValue::Gradient(uneven.clone()) }));
    w.send(Message::Seal);
    w.send(Message::Graph(GraphMessage::Select { node: id, mode: SelectMode::Replace }));
    w.send(Message::GradientStop(id, "gradient".into(), 1));
    draw_frame(&mut w, std::time::Instant::now());
    // The buttons, found by what each does to the gradient as it is.
    let mut without = uneven.clone();
    without.stops.remove(1);
    let mut find = |made: brewed_texture_core::Gradient| {
        let area = right(&mut w, "Gradient");
        find_click(&mut w, area, |m| set_to(m, "gradient") == Some(ParamValue::Gradient(made.clone()))).expect("the button")
    };
    let (reverse, spread, remove) = (find(uneven.reversed()), find(uneven.distributed()), find(without));
    // Pressed one after the other, at once: reversed, spread, the stop removed.
    let reversed = uneven.reversed();
    let spread_out = reversed.distributed();
    let mut removed = spread_out.clone();
    removed.stops.remove(1);
    for (at, made) in [(reverse, &reversed), (spread, &spread_out), (remove, &removed)] {
        click(&mut w, at);
        assert_eq!(gradient(&w), *made);
    }
    // Each undone on its own.
    w.send(Message::Undo);
    assert_eq!(gradient(&w), spread_out, "the stop back");
    w.send(Message::Undo);
    assert_eq!(gradient(&w), reversed, "spread no more");
    w.send(Message::Undo);
    assert_eq!(gradient(&w), uneven, "reversed no more");

    // A Curve: upside down, then straight again; each undone on its own.
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::CURVE, pos: [-400.0, 1200.0] }));
    w.send(Message::Seal);
    let node = w.app.graph().nodes_of(NodeKind::CURVE).map(|n| n.id).max().expect("the curve");
    w.send(Message::Graph(GraphMessage::Select { node, mode: SelectMode::Replace }));
    draw_frame(&mut w, std::time::Instant::now());
    let curve = |w: &Harness| w.app.graph().node(node).and_then(|n| n.value("curve")).and_then(|v| v.as_curve()).cloned().unwrap();
    let straight = curve(&w);
    let mut find = |made: Curve| {
        let area = right(&mut w, "Curve");
        find_click(&mut w, area, |m| set_to(m, "curve") == Some(ParamValue::Curve(made.clone()))).expect("the button")
    };
    let (invert, reset) = (find(straight.inverted()), find(Curve::default()));
    click(&mut w, invert);
    assert_eq!(curve(&w), straight.inverted());
    click(&mut w, reset);
    assert_eq!(curve(&w), Curve::default());
    w.send(Message::Undo);
    assert_eq!(curve(&w), straight.inverted(), "straight no more");
    w.send(Message::Undo);
    assert_eq!(curve(&w), straight, "upside down no more");
}

/// A parameter put back by a middle click on its row is a step of its own: a drag of it right
/// after is another (the drag was one step with it, and its undo took the value back to before
/// the middle click).
#[test]
fn a_parameter_put_back_by_a_middle_click_is_a_step_of_its_own() {
    let mut w = harness!(view::root, 1680.0, 1000.0);
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::BLUR, pos: [-400.0, 900.0] }));
    w.send(Message::Seal);
    let id = w.app.graph().nodes_of(NodeKind::BLUR).map(|n| n.id).max().expect("the blur");
    let radius = |w: &Harness| w.app.graph().node(id).and_then(|n| n.value("radius")).cloned();
    let drag_to = |w: &mut Harness, v: f32| {
        // (As its field sends it while it is dragged, and then seals it.)
        w.send(Message::Edit(Edit::SetParam { node: id, key: "radius".into(), value: ParamValue::Float(v) }));
        w.send(Message::Seal);
    };
    drag_to(&mut w, 20.0);
    w.send(Message::Graph(GraphMessage::Select { node: id, mode: SelectMode::Replace }));
    draw_frame(&mut w, std::time::Instant::now());
    // (Its name has a star: it is changed.)
    let row = *texts(&mut w, "Radius*").iter().rfind(|r| r.x > 1300.0).expect("the row of the radius");
    middle_click(&mut w, row.center());
    assert_eq!(radius(&w), Some(ParamValue::Float(8.0)), "the default back");
    // Dragged again at once: one undo takes the drag back, not the middle click.
    w.send(Message::Edit(Edit::SetParam { node: id, key: "radius".into(), value: ParamValue::Float(30.0) }));
    w.send(Message::Seal);
    w.send(Message::Undo);
    assert_eq!(radius(&w), Some(ParamValue::Float(8.0)), "the drag undone, the default left");
    w.send(Message::Undo);
    assert_eq!(radius(&w), Some(ParamValue::Float(20.0)), "the middle click undone");
}
