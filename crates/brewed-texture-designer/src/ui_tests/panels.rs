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
pub(super) fn find_click(w: &mut Harness, area: iced::Rectangle, wanted: impl Fn(&Message) -> bool) -> Option<Point> {
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
    // (The stop removed, the one before it is selected.)
    assert_eq!(w.app.ui.gradient_stop.get(&(id, "gradient".to_string())).copied(), Some(0), "the stop before the one removed");
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

/// The split between the panels `a` (above or left of it) and `b`.
fn split_between(w: &Harness, a: crate::app::Pane, b: crate::app::Pane) -> iced::widget::pane_grid::Split {
    use iced::widget::pane_grid::Node;
    fn panes_of(node: &Node, out: &mut Vec<iced::widget::pane_grid::Pane>) {
        match node {
            Node::Split { a, b, .. } => {
                panes_of(a, out);
                panes_of(b, out);
            }
            Node::Pane(p) => out.push(*p),
        }
    }
    let panes = &w.app.ui.panes;
    let has = |node: &Node, which: crate::app::Pane| {
        let mut of = Vec::new();
        panes_of(node, &mut of);
        of.iter().any(|p| panes.get(*p) == Some(&which))
    };
    fn find(node: &Node, test: &dyn Fn(&Node, &Node) -> bool) -> Option<iced::widget::pane_grid::Split> {
        match node {
            Node::Split { id, a, b, .. } => (test(a, b)).then_some(*id).or_else(|| find(a, test).or_else(|| find(b, test))),
            Node::Pane(_) => None,
        }
    }
    find(panes.layout(), &|x, y| has(x, a) && has(y, b)).expect("the split between the panels")
}

/// The scrollables of the view whose middle is in `area`: where each is, how large its content
/// is, and how far it is scrolled.
fn scrollables_in(w: &mut Harness, area: iced::Rectangle) -> Vec<(iced::Rectangle, iced::Rectangle, iced::Vector)> {
    struct Find(Vec<(iced::Rectangle, iced::Rectangle, iced::Vector)>);
    impl Operation for Find {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }
        fn scrollable(
            &mut self,
            _: Option<&iced::widget::Id>,
            bounds: iced::Rectangle,
            content: iced::Rectangle,
            translation: iced::Vector,
            _: &mut dyn iced::advanced::widget::operation::Scrollable,
        ) {
            self.0.push((bounds, content, translation));
        }
    }
    let mut find = Find(Vec::new());
    w.frame(|ui, renderer| ui.operate(renderer, &mut find));
    find.0.into_iter().filter(|(bounds, ..)| area.contains(bounds.center())).collect()
}

/// Whatever Properties shows scrolls when the panel is short, down to its end: the graph, a node,
/// a comment, several nodes. (A comment's and several nodes' were cut at its bottom: their fields
/// and buttons out of reach.)
#[test]
fn a_short_properties_panel_scrolls_whatever_it_shows() {
    use crate::app::Pane;
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let split = split_between(&w, Pane::Inputs, Pane::Properties);
    w.send(Message::PaneResized(iced::widget::pane_grid::ResizeEvent { split, ratio: 0.86 }));
    w.input(&[]);
    let comment = w.app.graph().comments.first().map(|c| c.id).expect("a comment");
    let levels = w.app.graph().nodes_of(NodeKind::LEVELS).next().expect("a Levels").id;
    // What Properties is made to show.
    type Show = Box<dyn Fn(&mut Harness)>;
    let states: [(&str, Show); 4] = [
        ("the graph", Box::new(|w: &mut Harness| w.send(Message::Graph(GraphMessage::ClearSelection)))),
        ("a comment", Box::new(move |w: &mut Harness| w.send(Message::Graph(GraphMessage::SelectComment(comment))))),
        ("several nodes", Box::new(|w: &mut Harness| w.send(Message::SelectAll))),
        ("a node", Box::new(move |w: &mut Harness| w.send(Message::Graph(GraphMessage::Select { node: levels, mode: SelectMode::Replace })))),
    ];
    for (what, show) in states {
        show(&mut w);
        draw_frame(&mut w, std::time::Instant::now());
        let header = *texts(&mut w, "PROPERTIES").first().expect("the panel");
        let panel = iced::Rectangle::new(Point::new(header.x - 10.0, header.y), Size::new(w.size.width - header.x + 10.0, w.size.height - header.y));
        let found = scrollables_in(&mut w, panel);
        let &[(bounds, content, _)] = found.as_slice() else { panic!("{what}: one scrollable in Properties, not {found:?}") };
        assert!(content.height > bounds.height + 40.0, "{what}: more than the short panel shows ({content:?} in {bounds:?})");
        // The wheel takes it down to its end.
        w.hover(bounds.center());
        for _ in 0..20 {
            w.mouse(mouse::Event::WheelScrolled { delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 } });
        }
        let (bounds, content, scrolled) = scrollables_in(&mut w, panel)[0];
        assert!((scrolled.y - (content.height - bounds.height)).abs() < 1.0, "{what}: scrolled to its end ({scrolled:?}, {content:?} in {bounds:?})");
        // (Back up, for the next.)
        for _ in 0..20 {
            w.mouse(mouse::Event::WheelScrolled { delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 } });
        }
    }
}

/// The options of the nodes selected, in a narrow Properties: every button whole, on as many
/// lines as they take; the one all the nodes have stands out; each press a step of its own.
#[test]
fn the_options_of_the_nodes_selected_fit_and_are_steps_of_their_own() {
    use brewed_texture_core::Tiling;
    let mut w = harness!(view::root, 1000.0, 640.0);
    w.send(Message::SelectAll);
    draw_frame(&mut w, std::time::Instant::now());
    w.snapshot("several_nodes_narrow");
    for label in Tiling::LABELS.iter().chain(&brewed_texture_core::nodes::PRECISION_LABELS) {
        assert_eq!(texts(&mut w, label).iter().filter(|r| r.x > 600.0).count(), 1, "{label} whole");
    }
    assert!(text_fits::cut_short(&mut w).is_empty(), "{:?}", text_fits::cut_short(&mut w));
    // Pressed one after the other, at once: each undone on its own.
    let tiling_of = |w: &Harness| -> std::collections::BTreeSet<u32> {
        w.app.graph().nodes.iter().filter(|n| n.def().has_image()).map(|n| n.enum_value("tiling")).collect()
    };
    let before = tiling_of(&w);
    let button = |w: &mut Harness, label: &str| *texts(w, label).iter().rfind(|r| r.x > 600.0).expect("the button");
    let (none, horizontal) = (button(&mut w, Tiling::LABELS[0]), button(&mut w, Tiling::LABELS[1]));
    click(&mut w, none.center());
    assert_eq!(tiling_of(&w), [0].into());
    click(&mut w, horizontal.center());
    assert_eq!(tiling_of(&w), [1].into());
    w.send(Message::Undo);
    assert_eq!(tiling_of(&w), [0].into(), "the second press undone");
    w.send(Message::Undo);
    assert_eq!(tiling_of(&w), before, "the first");
}

/// The menu of what lights the 3D view, with as many images as it keeps, in the smallest window
/// (it opens above its button, where there is more room): what does not fit in that room
/// scrolls, down to its last field (cut there, the fields under the images were out of reach).
#[test]
fn the_lighting_menu_scrolls_to_its_last_field_in_the_smallest_window() {
    let mut w = harness!(view::root, 1000.0, 640.0);
    let l = &mut w.app.ui.preview.lighting;
    l.recent = (0..crate::view::view3d::RECENT).map(|i| format!("C:/skies/sky number {i}.hdr")).collect();
    l.file = l.recent.first().cloned();
    w.send(Message::Preview(PreviewMessage::LightingMenu(true)));
    draw_frame(&mut w, std::time::Instant::now());
    w.snapshot("lighting_menu_small");
    // (Where the texts of a scrollable's content are laid out, not scrolled.)
    let blur = *texts(&mut w, "Blur").last().expect("the last field of the menu");
    let menu = *texts(&mut w, "Environment").last().expect("the menu's heading");
    let area = iced::Rectangle::new(Point::new(menu.x - 20.0, 0.0), Size::new(300.0, w.size.height));
    let holds = |content: &iced::Rectangle| content.y <= blur.y && blur.y + blur.height <= content.y + content.height + 0.5;
    let (bounds, content, _) = *scrollables_in(&mut w, area).iter().find(|(_, c, _)| holds(c)).expect("the menu scrolls");
    assert!(bounds.y >= 0.0 && bounds.y + bounds.height <= w.size.height, "the menu in the window: {bounds:?}");
    w.hover(bounds.center());
    for _ in 0..20 {
        w.mouse(mouse::Event::WheelScrolled { delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 } });
    }
    let (bounds, _, scrolled) = scrollables_in(&mut w, area).into_iter().find(|(_, c, _)| holds(c)).expect("the menu");
    let shown = (blur.y - scrolled.y, blur.y + blur.height - scrolled.y);
    assert!(bounds.y <= shown.0 && shown.1 <= bounds.y + bounds.height + 0.5, "Blur shown: {shown:?} in {bounds:?} ({content:?})");
}

/// A message of Properties that says a long path (a file that cannot be read, deep in folders of
/// long names) breaks where it has to: nothing of it is drawn past the edge of its box (a folder's
/// name longer than the panel ran out of it, to the edge of the window).
#[test]
fn a_long_path_in_a_message_stays_in_its_box() {
    let mut w = harness!(view::root, 1000.0, 1000.0);
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::BITMAP, pos: [-400.0, 900.0] }));
    let id = w.app.graph().nodes_of(NodeKind::BITMAP).map(|n| n.id).max().expect("the Bitmap");
    let path = "C:/Users/somebody/Textures/A_Folder_Of_A_Name_Longer_Than_The_Panel_Is_Wide_Without_Spaces/bricks.png";
    w.send(Message::Edit(Edit::SetParam { node: id, key: "path".into(), value: ParamValue::Text(path.into()) }));
    w.send(Message::Seal);
    // (What the engine says of a file it cannot read.)
    let said = format!("Cannot read {path}: The system cannot find the path specified. (os error 3)");
    w.app.res.reports.insert(id, brewed_texture_engine::ImageReport { version: 1, error: Some(said.clone()), warning: None, kind: None });
    w.send(Message::Graph(GraphMessage::Select { node: id, mode: SelectMode::Replace }));
    draw_frame(&mut w, std::time::Instant::now());
    w.snapshot("long_path_in_a_message");
    let message = *texts(&mut w, &said).first().expect("the message");
    // Right of its box (padded by 6 pixels, its edge a pixel wide), to the window's edge: no red of
    // its words.
    let pixels = w.pixels();
    let width = w.size.width as usize;
    let from = (message.x + message.width + 6.0 + 2.0).ceil() as usize;
    let red = (message.y as usize..(message.y + message.height) as usize)
        .flat_map(|y| (from..width).map(move |x| (x, y)))
        .filter(|(x, y)| {
            let p = &pixels[(y * width + x) * 4..][..3];
            i32::from(p[0]) > i32::from(p[1]) + 40
        })
        .count();
    assert_eq!(red, 0, "red pixels of the message past its box, right of x {from}");
}

/// A Math node in the narrowest Properties: what each variable is goes under its field, on a
/// couple of lines (beside it, it stood a word a line, ten lines high), and the plot's fields go
/// on as many lines as they need, each whole in the panel (the last one ran off its edge).
#[test]
fn the_variables_and_the_plot_of_a_math_node_fit_a_narrow_panel() {
    let mut w = harness!(view::root, 1000.0, 1700.0);
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::MATH, pos: [-400.0, 900.0] }));
    let id = w.app.graph().nodes_of(NodeKind::MATH).map(|n| n.id).max().expect("the Math node");
    w.send(Message::Graph(GraphMessage::Select { node: id, mode: SelectMode::Replace }));
    draw_frame(&mut w, std::time::Instant::now());
    w.snapshot("math_narrow");
    let hints = texts(&mut w, "own value · connect or name a graph input");
    assert!(!hints.is_empty(), "the variables of its expression");
    for hint in &hints {
        assert!(hint.height <= 3.0 * 12.0 * 1.3 + 1.0, "on a few lines: {hint:?}");
    }
    let to = *texts(&mut w, "to").iter().rfind(|r| r.x > 700.0).expect("the end of the plot's range");
    // (Its field is 70 pixels wide, 6 after it; the panel is padded 12 at its right.)
    assert!(to.x + to.width + 6.0 + 70.0 <= w.size.width - 12.0 + 0.5, "the field of the end in the panel: {to:?}");
}

/// A graph of a library folder with a long name, under a long category, in the narrowest Library:
/// its name and its category shown whole (on as many lines as they take), nothing cut short.
#[test]
fn long_names_of_library_graphs_show_whole_in_a_narrow_library() {
    use brewed_texture_core::project;
    use brewed_texture_core::{Graph, GraphAttributes, package::Package};
    let dir = std::env::temp_dir().join(format!("brewed-long-library-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (name, category) = ("Weathered Copper Roof Tiles With Moss", "Materials/Architecture/Roofs/Weathered Metal");
    let mut roof = Graph::default();
    roof.add_node(NodeKind::OUTPUT, [0.0, 0.0]);
    roof.attributes = GraphAttributes { description: "Copper gone green".into(), category: category.into(), hidden: false };
    let package = Package::new(name, roof);
    let file = dir.join("roof.brewtex");
    project::save(&file, &package, &std::collections::HashMap::new(), package.first()).unwrap();
    let mut w = harness!(view::root, 1000.0, 640.0);
    let mut settings = w.app.settings.clone();
    settings.library_folders = vec![dir.display().to_string()];
    w.app.set_settings(settings);
    w.send(Message::LookAtLibrary);
    // (Only it in the Library: what is searched for.)
    w.send(Message::LibrarySearch("copper".into()));
    draw_frame(&mut w, std::time::Instant::now());
    w.snapshot("library_long_names");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!texts(&mut w, name).is_empty() && !texts(&mut w, &category.to_uppercase()).is_empty(), "the graph under its category");
    let cut = text_fits::cut_short(&mut w);
    let squeezed = text_fits::squeezed(&mut w);
    assert!(cut.is_empty() && squeezed.is_empty(), "{cut:?} {squeezed:?}");
}

/// The name of an input value being typed for an Input node that an undo takes away is forgotten
/// with it (it was kept: the next Esc went to it, and Enter bound it to a node that is not there).
#[test]
fn a_name_typed_for_an_input_node_an_undo_takes_away_is_forgotten() {
    let mut w = harness!(view::root, 1400.0, 900.0);
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::FLOAT_INPUT, pos: [-400.0, 900.0] }));
    w.send(Message::Seal);
    let id = w.app.graph().nodes_of(NodeKind::FLOAT_INPUT).map(|n| n.id).max().expect("the Input node");
    w.send(Message::Graph(GraphMessage::Select { node: id, mode: SelectMode::Replace }));
    w.send(Message::BindingText(id, "roughness".into()));
    assert!(w.app.ui.binding.is_some(), "being typed");
    w.send(Message::Undo);
    assert!(w.app.graph().node(id).is_none(), "the node taken away");
    assert!(w.app.ui.binding.is_none(), "its name being typed forgotten");
}

/// A name being typed for an input value, a graph or a file that an undo (Ctrl+Z works while a
/// field is typed in) takes away is forgotten with it.
#[test]
fn names_being_typed_for_what_an_undo_takes_away_are_forgotten() {
    use std::sync::Arc;
    let mut w = harness!(view::root, 1400.0, 900.0);
    let key = w.app.doc_key;
    // An input value, made by an Input node.
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::FLOAT_INPUT, pos: [-400.0, 900.0] }));
    w.send(Message::Seal);
    let id = w.app.graph().nodes_of(NodeKind::FLOAT_INPUT).map(|n| n.id).max().expect("the Input node");
    let name = w.app.graph().node(id).and_then(|n| n.variable.clone()).expect("its input value");
    w.send(Message::RenameVariable(name.clone(), "roughness".into()));
    w.send(Message::Undo);
    assert!(!w.app.graph().variables.iter().any(|v| v.name == name), "the input value taken away");
    assert!(w.app.ui.renaming_var.is_none(), "its name being typed forgotten");
    // A graph: one made, and another edited while it is renamed.
    let wall = w.app.doc.current();
    w.send(Message::Explorer(ExplorerMessage::NewGraph(key)));
    let graph = w.app.doc.current();
    w.send(Message::Explorer(ExplorerMessage::CommitRename));
    w.send(Message::Explorer(ExplorerMessage::OpenGraph(key, wall)));
    w.send(Message::Explorer(ExplorerMessage::StartRename(key, graph)));
    assert!(w.app.ui.renaming_graph.as_ref().is_some_and(|(k, g, _)| *k == key && *g == graph), "being renamed");
    w.send(Message::Undo);
    assert!(w.app.doc.package().graph(graph).is_none(), "the graph taken away");
    assert_eq!(w.app.doc.current(), wall, "the graph edited still edited");
    assert!(w.app.ui.renaming_graph.is_none(), "its name being typed forgotten");
    // A file.
    let held = Held::new("C:/tex/bricks.png", Arc::new(b"the bytes of bricks.png".to_vec()));
    let path = held.path().to_string();
    w.send(Message::Explorer(ExplorerMessage::FilesRead { key, embed: false, files: vec![("C:/tex/bricks.png".into(), held)], failed: vec![] }));
    w.send(Message::Explorer(ExplorerMessage::StartRenameFile(key, path.clone())));
    assert!(w.app.ui.renaming_file.is_some());
    w.send(Message::Undo);
    assert!(!w.app.doc.package().resources.iter().any(|r| r.path == path), "the file taken away");
    assert!(w.app.ui.renaming_file.is_none(), "its name being typed forgotten");
}

/// A package saved as the file another open package is from: the other goes where it has nothing
/// to save, its menu in the Explorer closing with it; where it has changes not saved, it stays,
/// untitled, with them (they went with it, unsaid), and the status bar says so.
#[test]
fn a_package_saved_over_another_open_ones_file_keeps_its_unsaved_changes() {
    let dir = scratch("saved-over");
    let file = dir.join("bricks.brewtex");
    let mut w = harness!(view::root, 1400.0, 900.0);
    let same = |doc: &brewed_texture_core::Document| doc.path().map(|p| p.display().to_string()) == Some(crate::app::packages::clean(&file));
    // Saved, then nothing changed: saved over, it goes, its menu with it.
    w.send(Message::SavePicked(Some(file.clone())));
    let first = w.app.doc_key;
    let graph = w.app.doc.current();
    assert!(same(&w.app.doc) && !w.app.doc.is_dirty(), "saved as bricks.brewtex");
    w.send(Message::New);
    w.send(Message::Context(Some(ContextMenu { at: Point::new(100.0, 100.0), target: ContextTarget::GraphItem(first, graph) })));
    w.send(Message::SavePicked(Some(file.clone())));
    assert!(w.app.package_doc(first).is_none(), "the package of the file saved over goes");
    assert!(w.app.ui.context.is_none(), "its menu closes with it");
    // Changed and not saved: saved over, it stays, untitled, with its change.
    let second = w.app.doc_key;
    w.send(Message::New);
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::BLUR, pos: [0.0, 0.0] }));
    w.send(Message::Seal);
    let changed = w.app.doc_key;
    w.send(Message::Explorer(ExplorerMessage::OpenGraph(second, w.app.package_doc(second).expect("open").current())));
    w.send(Message::SavePicked(Some(dir.join("other.brewtex"))));
    w.send(Message::Explorer(ExplorerMessage::OpenGraph(changed, w.app.package_doc(changed).expect("open").current())));
    // (The changed one saved as bricks.brewtex first, then changed again: what it has is not the file's.)
    w.send(Message::SavePicked(Some(file.clone())));
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::LEVELS, pos: [200.0, 0.0] }));
    w.send(Message::Seal);
    w.send(Message::Explorer(ExplorerMessage::OpenGraph(second, w.app.package_doc(second).expect("open").current())));
    w.send(Message::SavePicked(Some(file.clone())));
    let kept = w.app.package_doc(changed).expect("the changed package stays open");
    assert!(kept.path().is_none() && kept.is_dirty(), "untitled, with changes to save");
    assert!(kept.package().graphs.iter().any(|g| g.graph.nodes_of(NodeKind::LEVELS).count() > 0), "its change kept");
    assert!(w.app.ui.status.as_ref().is_some_and(|s| s.text.contains("kept, untitled")), "said: {:?}", w.app.ui.status);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A short 3D view without maps: what to do to see them and how fast it draws are one under the
/// other, never over each other (the rate in its corner lay on the hint).
#[test]
fn the_hint_and_the_rate_of_a_short_3d_view_do_not_overlap() {
    use crate::app::Pane;
    let mut w = harness!(view::root, 1218.0, 795.0);
    let split = split_between(&w, Pane::Graph, Pane::View3D);
    for ratio in [0.8, 0.86, 0.9, 0.94] {
        w.send(Message::PaneResized(iced::widget::pane_grid::ResizeEvent { split, ratio }));
        draw_frame(&mut w, std::time::Instant::now());
        assert!(!texts_reading(&mut w, "Set the Usage of Output nodes (Base Color, Normal, Height …) to see them here").is_empty(), "the hint");
        let over = text_fits::overlapping(&mut w);
        assert!(over.is_empty(), "at {ratio}: {over:?}");
    }
}

/// Mangle the part `at` of a value (counted depth first), `how` says how: a number far out, a
/// string odd, an array emptied, doubled or turned around, a field gone (as the core's test of
/// mangled files does).
fn mangle(v: &mut serde_json::Value, at: &mut usize, how: usize) {
    use serde_json::Value as Json;
    let here = *at == 0;
    *at = at.wrapping_sub(1);
    match v {
        Json::Number(_) if here => {
            let far = [1e39, -1e39, 3e38, -3e38, 1e30, 0.0, -0.0, 1e-40, 4294967296.0, -1.0, 0.5, 8192.0, 16384.0];
            *v = serde_json::json!(far[how % far.len()]);
        }
        Json::String(text) if here => {
            let odd = ["", " ", "a b", "i:input", "o:output", "../x", "\u{0}", "ы"];
            *text = match how % 9 {
                8 => "ы".repeat(3000),
                k => odd[k].to_string(),
            };
        }
        Json::Bool(b) if here => *b = !*b,
        Json::Array(items) if here => match how % 3 {
            0 => items.clear(),
            1 => items.extend(items.clone()),
            _ => items.reverse(),
        },
        Json::Object(fields) if here => {
            if let Some(key) = fields.keys().nth(how % fields.len().max(1)).cloned() {
                fields.remove(&key);
            }
        }
        Json::Array(items) => items.iter_mut().for_each(|i| mangle(i, at, how)),
        Json::Object(fields) => fields.values_mut().for_each(|f| mangle(f, at, how)),
        _ => {}
    }
}

fn parts(v: &serde_json::Value) -> usize {
    1 + match v {
        serde_json::Value::Array(items) => items.iter().map(parts).sum(),
        serde_json::Value::Object(fields) => fields.values().map(parts).sum(),
        _ => 0,
    }
}

/// Packages mangled at random, opened in the window: each that opens is drawn, its nodes shown in
/// Properties one after the other, and the interface holds nothing that is not there (the core
/// reads them whole or says why it does not; the window must show whatever it reads).
#[test]
fn mangled_packages_open_in_the_window() {
    use brewed_texture_core::archive::{Archive, Method, Writer};
    use std::io::Cursor;
    let dir = scratch("mangled");
    let mut w = harness!(view::root, 1400.0, 900.0);
    let file = dir.join("demo.brewtex");
    w.send(Message::SavePicked(Some(file.clone())));
    let bytes = std::fs::read(&file).expect("the package saved");
    let mut archive = Archive::new(Cursor::new(bytes)).expect("its archive");
    let names: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).collect();
    let contents: Vec<(String, Vec<u8>)> = names.into_iter().map(|n| (n.clone(), archive.read(&n, 1 << 30).unwrap())).collect();
    let jsons: Vec<(usize, serde_json::Value)> = contents
        .iter()
        .enumerate()
        .filter(|(_, (n, _))| n.ends_with(".json"))
        .map(|(i, (_, c))| (i, serde_json::from_slice(c).unwrap()))
        .collect();
    let mut seed = 7u32;
    let mut opened = 0;
    let mangled = dir.join("mangled.brewtex");
    for round in 0..150 {
        let mut files = jsons.clone();
        for _ in 0..1 + round % 3 {
            seed = brewed_texture_core::expr::builtins::pcg_hash(seed);
            let which = seed as usize % files.len();
            let f = &mut files[which].1;
            let mut at = (seed >> 8) as usize % parts(f);
            mangle(f, &mut at, (seed >> 20) as usize);
        }
        let mut zip = Writer::new(Vec::new());
        for (i, (name, content)) in contents.iter().enumerate() {
            match files.iter().find(|(k, _)| *k == i) {
                Some((_, json)) => zip.add(name, json.to_string().as_bytes(), Method::Stored).unwrap(),
                None => zip.add(name, content, Method::Stored).unwrap(),
            }
        }
        std::fs::write(&mangled, zip.finish().unwrap()).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            w.send(Message::Proceed(PendingAction::OpenPath(mangled.clone())));
            draw_frame(&mut w, std::time::Instant::now());
            let ids: Vec<_> = w.app.graph().nodes.iter().map(|n| n.id).collect();
            for id in ids {
                w.send(Message::Graph(GraphMessage::Select { node: id, mode: SelectMode::Replace }));
                w.input(&[]);
            }
            w.send(Message::SelectAll);
            draw_frame(&mut w, std::time::Instant::now());
            broken(&w.app)
        }));
        match result {
            Ok(None) => {}
            Ok(Some(why)) => panic!("round {round}: {why}"),
            Err(_) => panic!("round {round}: the window panicked on the package"),
        }
        if w.app.doc.path().is_some_and(|p| p.ends_with("mangled.brewtex")) {
            opened += 1;
        }
        // (Each opened in place of the one before: no package piles up.)
        w.send(Message::Proceed(PendingAction::LoadDemo));
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(opened > 30, "{opened} of 150 opened");

    // Pastes mangled so, of every node of the demo: what is pasted shows as what opens does.
    use brewed_texture_core::graph::Clipboard;
    w.send(Message::Proceed(PendingAction::LoadDemo));
    w.send(Message::SelectAll);
    let _ = w.app.update(Message::Copy);
    let copied = w.app.ui.clipboard.clone().expect("the nodes copied").to_text();
    let clip: serde_json::Value = serde_json::from_str(copied.strip_prefix(Clipboard::MAGIC).expect("the clipboard's mark")).unwrap();
    let mut pasted = 0;
    for round in 0..150 {
        let mut c = clip.clone();
        for _ in 0..1 + round % 3 {
            seed = brewed_texture_core::expr::builtins::pcg_hash(seed);
            let mut at = seed as usize % parts(&c);
            mangle(&mut c, &mut at, (seed >> 20) as usize);
        }
        let text = format!("{}{c}", Clipboard::MAGIC);
        let before = w.app.graph().nodes.len();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            w.send(Message::ClipboardText(Some(text.clone())));
            draw_frame(&mut w, std::time::Instant::now());
            broken(&w.app)
        }));
        match result {
            Ok(None) => {}
            Ok(Some(why)) => panic!("paste {round}: {why}"),
            Err(_) => panic!("paste {round}: the window panicked on what was pasted"),
        }
        if w.app.graph().nodes.len() > before {
            pasted += 1;
        }
        w.send(Message::Undo);
    }
    assert!(pasted > 30, "{pasted} of 150 pasted");
}

/// A file dropped on the window while a dialog is open is not taken (the dialog takes what comes
/// to the window first, as it takes the keyboard), and the status bar says so; closed, the window
/// takes it again.
#[test]
fn a_file_dropped_while_a_dialog_is_open_waits_for_it() {
    let dir = scratch("drop-dialog");
    let picture = dir.join("bricks.png");
    image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 80, 40, 255])).save(&picture).unwrap();
    let mut w = harness!(view::root, 1400.0, 900.0);
    // (Linked, as the settings may say: taken at once, not read in the background first.)
    w.app.settings.new_files = crate::settings::NewFiles::Link;
    let window = w.main_window();
    let bitmaps = |w: &Harness| w.app.graph().nodes_of(NodeKind::BITMAP).count();
    let before = bitmaps(&w);
    for dialog in [Dialog::About, Dialog::Shortcuts] {
        w.send(Message::Dialog(Some(dialog)));
        w.window_event(window, iced::Event::Window(window::Event::FileDropped(picture.clone())));
        assert_eq!(bitmaps(&w), before, "nothing added under the dialog");
        assert!(w.app.ui.status.as_ref().is_some_and(|s| s.error && s.text.contains("close it first")), "said: {:?}", w.app.ui.status);
        w.send(Message::Dialog(None));
    }
    w.window_event(window, iced::Event::Window(window::Event::FileDropped(picture.clone())));
    assert_eq!(bitmaps(&w), before + 1, "taken with no dialog open");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The buttons of the 2D view's header, clicked: each channel shows that channel (the one shown
/// stands out), and 1:1 puts a pixel of the image on a pixel of the screen, in the middle.
#[test]
fn the_buttons_of_the_2d_view_show_what_they_say() {
    let mut w = harness!(view::root, 1680.0, 1000.0);
    w.input(&[]);
    // (Right of the 3D view's header: the 2D view's.)
    let button = |w: &mut Harness, label: &str| -> Point {
        let found = texts(w, label);
        found.iter().rfind(|r| r.x > 700.0 && r.y > 500.0).map(|r| r.center()).unwrap_or_else(|| panic!("{label} in the 2D view's header: {found:?}"))
    };
    for (label, channel) in [("R", 2), ("G", 3), ("B", 4), ("A", 5), ("RGB", 1), ("RGBA", 0)] {
        let at = button(&mut w, label);
        click(&mut w, at);
        assert_eq!(w.app.ui.preview.channel, channel, "{label}");
    }
    let p = &mut w.app.ui.preview;
    (p.zoom, p.pan, p.fit) = (3.0, iced::Vector::new(40.0, -20.0), false);
    let at = button(&mut w, "1:1");
    click(&mut w, at);
    let p = &w.app.ui.preview;
    assert_eq!((p.zoom, p.pan, p.fit), (1.0, iced::Vector::ZERO, false), "a pixel a pixel, in the middle");
}

/// What is pasted is what the clipboard holds: nodes copied, then a word copied in another
/// program, and Paste pastes no nodes (it pasted the nodes copied before); where the clipboard
/// cannot be read at all, the nodes copied here last are pasted.
#[test]
fn paste_pastes_what_the_clipboard_holds_now() {
    let mut w = harness!(view::root, 1400.0, 900.0);
    let levels = w.app.graph().nodes_of(NodeKind::LEVELS).next().expect("a Levels").id;
    w.send(Message::Graph(GraphMessage::Select { node: levels, mode: SelectMode::Replace }));
    let _ = w.app.update(Message::Copy);
    let count = |w: &Harness| w.app.graph().nodes_of(NodeKind::LEVELS).count();
    let before = count(&w);
    // A word, copied in another program since.
    w.send(Message::ClipboardText(Some("bricks".into())));
    assert_eq!(count(&w), before, "no nodes pasted");
    assert!(w.app.ui.status.as_ref().is_some_and(|s| s.text.contains("no nodes")), "said: {:?}", w.app.ui.status);
    // What was copied here, in the clipboard: pasted.
    let copied = w.app.ui.clipboard.as_ref().expect("copied").to_text();
    w.send(Message::ClipboardText(Some(copied)));
    assert_eq!(count(&w), before + 1, "the Levels pasted");
    // The clipboard not read: the nodes copied here last.
    w.send(Message::ClipboardText(None));
    assert_eq!(count(&w), before + 2, "pasted from what was copied here");
}

/// What is read in the background for a graph (a picture dropped on it, a file chosen for a
/// field of one of its nodes) goes to that graph, edited again, whichever is edited when it is
/// read: it went to the graph edited then, a Bitmap in another graph, the file to the node of the
/// same number there.
#[test]
fn what_is_read_for_a_graph_goes_to_it() {
    use crate::app::{Asked, FileTarget};
    use std::sync::Arc;
    let mut w = harness!(view::root, 1400.0, 900.0);
    let key = w.app.doc_key;
    let wall = w.app.doc.current();
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::BITMAP, pos: [0.0, 900.0] }));
    w.send(Message::Seal);
    let bitmap = w.app.graph().nodes_of(NodeKind::BITMAP).map(|n| n.id).max().expect("a Bitmap in the wall");
    let asked = Asked { key, graph: wall };
    // Another graph, with a node of the same number, edited by the time the files are read.
    w.send(Message::Explorer(ExplorerMessage::NewGraph(key)));
    w.send(Message::Explorer(ExplorerMessage::CommitRename));
    let other = w.app.doc.current();
    while w.app.graph().nodes.iter().all(|n| n.id < bitmap) {
        w.send(Message::Edit(Edit::AddNode { kind: NodeKind::BITMAP, pos: [0.0, 0.0] }));
    }
    w.send(Message::Seal);
    let in_graph = |w: &Harness, graph| w.app.doc.package().graph(graph).map(|g| g.graph.clone()).expect("the graph");
    let before = (in_graph(&w, wall), in_graph(&w, other));
    let held = || Held::new("C:/tex/bricks.png", Arc::new(b"the bytes of bricks.png".to_vec()));
    w.send(Message::Asked(asked, Box::new(Message::DropHeld(brewed_texture_core::ResourceKind::Bitmap, [300.0, 900.0], Ok(held())))));
    assert_eq!(w.app.doc.current(), wall, "the wall edited again");
    assert_eq!(in_graph(&w, wall).nodes_of(NodeKind::BITMAP).count(), before.0.nodes_of(NodeKind::BITMAP).count() + 1, "the picture's Bitmap in the wall");
    assert_eq!(in_graph(&w, other), before.1, "nothing in the other graph");
    // A file chosen for the wall's Bitmap, read while the other graph is edited.
    w.send(Message::Explorer(ExplorerMessage::OpenGraph(key, other)));
    let before = in_graph(&w, other);
    w.send(Message::Asked(asked, Box::new(Message::FileHeld(FileTarget::Param(bitmap, "path".into()), Ok(held())))));
    assert_eq!(w.app.doc.current(), wall);
    assert!(in_graph(&w, wall).node(bitmap).is_some_and(|n| !n.text("path").is_empty()), "the file the wall's Bitmap reads");
    assert_eq!(in_graph(&w, other), before, "the other graph's node of that number untouched");
}

/// A file the package holds is saved as a file of its own under the name the package calls it
/// by, with the extension of what it is: not the name it is held by (the hash of its bytes, after
/// a colon no file of Windows may have); a picture called "Bricks" is saved "Bricks.png", and,
/// typed so where a "Bricks.png" is, not written over it unasked.
#[test]
fn files_held_are_saved_as_the_package_calls_them() {
    use brewed_texture_core::PackageEdit;
    let dir = scratch("held-saved-as");
    let mut w = harness!(view::root, 1400.0, 900.0);
    let key = w.app.doc_key;
    let held = Held::new("C:/tex/bricks.png", std::sync::Arc::new(b"the bytes of bricks.png".to_vec()));
    let path = held.path().to_string();
    w.send(Message::Explorer(ExplorerMessage::FilesRead { key, embed: false, files: vec![("C:/tex/bricks.png".into(), held)], failed: vec![] }));
    assert!(w.app.doc.package().resources.iter().any(|r| r.path == path), "held");
    assert_eq!(w.app.save_name(key, &path), "bricks.png");
    w.app.doc.apply_package(PackageEdit::RenameResource { path: path.clone(), name: "Bricks".into() }).expect("renamed");
    assert_eq!(w.app.save_name(key, &path), "Bricks.png");
    assert_eq!(crate::app::packages::with_extension_of("C:/out/Bricks", &path), "C:/out/Bricks.png");
    assert_eq!(crate::app::packages::with_extension_of("C:/out/Bricks.PNG", &path), "C:/out/Bricks.PNG");
    // Typed without its extension where the file with it is: not written over.
    let there = dir.join("Bricks.png");
    std::fs::write(&there, b"another picture").expect("a file");
    w.send(Message::Explorer(ExplorerMessage::ResourceSavePicked(key, path.clone(), Some(dir.join("Bricks")))));
    assert!(w.app.ui.status.as_ref().is_some_and(|s| s.error && s.text.contains("is there already")), "said: {:?}", w.app.ui.status);
    assert_eq!(std::fs::read(&there).expect("still there"), b"another picture");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Files missing from one folder, found in another: one located, the others there are relinked
/// with it, in one step of undo (each was a step of its own); a file the package cannot use is
/// not taken for one, and the status bar says so (it said it was relinked, and the package, read
/// back, left the file out).
#[test]
fn files_found_in_one_folder_are_relinked_in_one_step() {
    use brewed_texture_core::{Graph, Package};
    let dir = scratch("relinked-together");
    let (old, new) = (dir.join("old"), dir.join("new"));
    std::fs::create_dir_all(&new).expect("a folder");
    for name in ["a.png", "b.png"] {
        image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 100, 50, 255])).save(new.join(name)).expect("a picture");
    }
    std::fs::write(new.join("a.doc"), b"not a picture").expect("a file");
    let mut g = Graph::default();
    for (i, name) in ["a.png", "b.png"].iter().enumerate() {
        let id = g.add_node(NodeKind::BITMAP, [0.0, 200.0 * i as f32]);
        g.node_mut(id).unwrap().param_mut("path").unwrap().value = ParamValue::Text(crate::app::packages::clean(&old.join(name)));
    }
    let file = dir.join("walls.brewtex");
    let package = Package::new("Walls", g);
    brewed_texture_core::project::save(&file, &package, &Default::default(), package.first()).expect("a file");
    let mut w = harness!(view::root, 1400.0, 900.0);
    w.send(Message::OpenPicked(Some(file.clone())));
    let Some(Dialog::Missing(missing)) = w.app.ui.dialog.clone() else { panic!("the missing files are asked for: {:?}", w.app.ui.dialog) };
    assert_eq!(missing.items.len(), 2, "{:?}", missing.items);
    let paths = |w: &Harness| -> Vec<String> { w.app.doc.package().resources.iter().map(|r| r.path.clone()).collect() };
    let before = paths(&w);
    let a = missing.items.iter().find(|i| i.path.ends_with("a.png")).expect("a.png asked for").path.clone();
    // Not a picture: not taken for one.
    w.send(Message::Explorer(ExplorerMessage::Located(missing.key, a.clone(), Some(new.join("a.doc")))));
    assert_eq!(paths(&w), before);
    assert!(!w.app.doc.can_undo(), "no step of undo");
    assert!(w.app.ui.status.as_ref().is_some_and(|s| s.error && s.text.contains("is not relinked")), "said: {:?}", w.app.ui.status);
    let Some(Dialog::Missing(still)) = &w.app.ui.dialog else { panic!("still asked for") };
    assert_eq!(still.items.len(), 2);
    // The picture: the other one found beside it.
    w.send(Message::Explorer(ExplorerMessage::Located(missing.key, a, Some(new.join("a.png")))));
    assert!(w.app.ui.dialog.is_none(), "both found");
    let clean = |name: &str| crate::app::packages::clean(&new.join(name));
    assert_eq!(paths(&w), [clean("a.png"), clean("b.png")]);
    w.send(Message::Undo);
    assert_eq!(paths(&w), before, "one step undoes both");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The demo's gradient, in its editor alone.
fn the_demo_ramp(app: &App) -> iced::Element<'_, Message> {
    let map = app.graph().nodes_of(NodeKind::GRADIENT_MAP).next().expect("the demo colors by a gradient").id;
    let g = app.graph().node(map).and_then(|n| n.value("gradient")).and_then(ParamValue::as_gradient).expect("its gradient");
    let selected = app.ui.gradient_stop.get(&(map, "gradient".to_string())).copied();
    crate::widget::gradient::gradient_editor(
        g,
        selected,
        move |g| Message::SetGradient(map, "gradient".into(), g, None),
        move |i| Message::GradientStop(map, "gradient".into(), i),
    )
    .into()
}

/// A stop of a gradient dragged past another is the stop that moves, however many moves come
/// before the window is drawn again (the second of two went to the stop that had taken its
/// place); and a stop added on the ramp and dragged at once is not lost.
#[test]
fn a_stop_dragged_fast_is_the_stop_that_moves() {
    use brewed_texture_core::Gradient;
    let mut w = harness!(the_demo_ramp, 400.0, 60.0);
    let map = w.app.graph().nodes_of(NodeKind::GRADIENT_MAP).next().expect("a gradient map").id;
    let (black, gray, white) = ([0.0, 0.0, 0.0, 1.0], [0.5, 0.5, 0.5, 1.0], [1.0, 1.0, 1.0, 1.0]);
    w.send(Message::SetGradient(map, "gradient".into(), Gradient::from_pairs(&[(0.0, black), (0.5, gray), (1.0, white)]), None));
    w.send(Message::Seal);
    // Where a stop's handle is, and the ramp (as the editor lays them out).
    let x = |pos: f32| 5.5 + pos * (400.0 - 11.0);
    let (handle, ramp) = (35.0, 13.0);
    let moved = |x: f32, y: f32| iced::Event::Mouse(mouse::Event::CursorMoved { position: Point::new(x, y) });
    let stops = |w: &Harness| -> Vec<(f32, [f32; 4])> { gradient_of(&w.app, map).stops.iter().map(|s| ((s.pos * 100.0).round() / 100.0, s.color)).collect() };
    // The black one, past the gray one and on, in one go.
    w.hover(Point::new(x(0.0), handle));
    w.mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    w.input(&[moved(x(0.7), handle), moved(x(0.8), handle)]);
    w.mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    assert_eq!(stops(&w), [(0.5, gray), (0.8, black), (1.0, white)]);
    // One added on the ramp and dragged at once.
    w.hover(Point::new(x(0.25), ramp));
    w.input(&[iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)), moved(x(0.3), ramp), moved(x(0.35), ramp)]);
    w.mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    let after = stops(&w);
    assert_eq!(after.len(), 4, "the stop added is there: {after:?}");
    assert_eq!(after.iter().map(|s| s.0).collect::<Vec<_>>(), [0.35, 0.5, 0.8, 1.0], "and where it was dragged: {after:?}");
}

/// The curve of the last Curve node, in its editor alone (a drag of it sets it as Properties
/// does).
fn the_last_curve(app: &App) -> iced::Element<'_, Message> {
    let Some(node) = app.graph().nodes_of(NodeKind::CURVE).last() else { return iced::widget::space().into() };
    let id = node.id;
    let curve = node.value("curve").and_then(ParamValue::as_curve).expect("its curve");
    crate::widget::curve::curve_editor(curve, move |c| Message::Edit(Edit::SetParam { node: id, key: "curve".into(), value: ParamValue::Curve(c) }))
        .into()
}

/// A point added to a curve and dragged before the window is drawn again is the point that
/// moves (it was lost, and the point after it moved in its place).
#[test]
fn a_point_added_to_a_curve_and_dragged_at_once_is_kept() {
    let mut w = harness!(the_last_curve, 400.0, 200.0);
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::CURVE, pos: [2600.0, -900.0] }));
    w.send(Message::Seal);
    let node = w.app.graph().nodes_of(NodeKind::CURVE).last().map(|n| n.id).expect("a curve");
    let points = |w: &Harness| -> Vec<[f32; 2]> {
        let curve = w.app.graph().node(node).and_then(|n| n.value("curve")).and_then(ParamValue::as_curve).cloned().expect("its curve");
        curve.points.iter().map(|p| p.map(|v| (v * 100.0).round() / 100.0)).collect()
    };
    assert_eq!(points(&w), [[0.0, 0.0], [1.0, 1.0]]);
    // (Where a point of the curve is, as the editor lays it out: a square 180 high, 6 in.)
    let at = |x: f32, y: f32| Point::new(6.0 + x * (400.0 - 12.0), 6.0 + (1.0 - y) * 180.0);
    let moved = |p: Point| iced::Event::Mouse(mouse::Event::CursorMoved { position: p });
    w.hover(at(0.5, 0.5));
    w.input(&[iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)), moved(at(0.5, 0.8)), moved(at(0.4, 0.9))]);
    w.mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    assert_eq!(points(&w), [[0.0, 0.0], [0.4, 0.9], [1.0, 1.0]]);
}

/// Reversed, a gradient keeps the stop that was selected selected: the stop, at its new place
/// (the one that came to its old place was selected instead, and its color shown).
#[test]
fn a_reversed_gradient_keeps_its_stop_selected() {
    use brewed_texture_core::GradientStop;
    let mut w = harness!(view::root, 1680.0, 1000.0);
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::GRADIENT_MAP, pos: [-400.0, 900.0] }));
    let id = w.app.graph().nodes_of(NodeKind::GRADIENT_MAP).map(|n| n.id).max().expect("the node");
    let gradient = |w: &Harness| w.app.graph().node(id).and_then(|n| n.value("gradient")).and_then(ParamValue::as_gradient).cloned().unwrap();
    let mut three = gradient(&w);
    three.stops = [(0.0, 0.1), (0.2, 0.5), (1.0, 0.9)].map(|(pos, v)| GradientStop { pos, color: [v, v, v, 1.0] }).to_vec();
    w.send(Message::Edit(Edit::SetParam { node: id, key: "gradient".into(), value: ParamValue::Gradient(three.clone()) }));
    w.send(Message::Seal);
    w.send(Message::Graph(GraphMessage::Select { node: id, mode: SelectMode::Replace }));
    // The darkest stop, the first.
    w.send(Message::GradientStop(id, "gradient".into(), 0));
    draw_frame(&mut w, std::time::Instant::now());
    let area = iced::Rectangle::new(Point::new(w.size.width - 400.0, 0.0), Size::new(400.0, w.size.height));
    let reverse = find_click(&mut w, area, |m| set_to(m, "gradient") == Some(ParamValue::Gradient(three.reversed()))).expect("Reverse");
    click(&mut w, reverse);
    let selected = w.app.ui.gradient_stop.get(&(id, "gradient".to_string())).copied().expect("a stop selected");
    assert_eq!(gradient(&w).stops[selected].color, [0.1, 0.1, 0.1, 1.0], "the darkest, now the last");
    assert!(!texts(&mut w, "Stop 3 of 3").is_empty(), "said so");
}

/// A graph another open package uses, renamed and deleted from the Explorer while that package
/// is edited, and put back in its own package: the node that uses it is called as it is now at
/// each step, says it cannot be found once it is gone, and works again once an undo puts it
/// back (renamed or taken away so, the graph edited went on drawing it as it was, until it
/// changed itself); and the other package's undo is its own.
#[test]
fn a_graph_another_package_uses_is_followed_through_rename_delete_and_undo() {
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let dir = scratch("followed");
    let wall = dir.join("wall.brewtex");
    w.send(Message::SavePicked(Some(wall.clone())));
    let (a, graph) = (w.app.doc_key, w.app.doc.current());
    // A second graph, so that the wall may go.
    w.send(Message::Explorer(ExplorerMessage::NewGraph(a)));
    w.send(Message::Explorer(ExplorerMessage::CommitRename));
    // Another package uses the wall.
    w.send(Message::New);
    let b = w.app.doc_key;
    let name = w.app.package_doc(a).unwrap().package().graph(graph).unwrap().name.clone();
    w.send(Message::Graph(GraphMessage::Drop { payload: DragPayload::Graph { key: a, graph, name: name.clone() }, graph_pos: [0.0, 0.0] }));
    let node = w.app.graph().instances().next().map(|n| n.id).expect("a node that uses the wall");
    let user = |w: &Harness| -> (String, bool) {
        let doc = w.app.package_doc(b).expect("the other package");
        let n = doc.graph().node(node).expect("the node");
        let works = if w.app.doc_key == b { w.app.eval.node(node).is_some_and(|e| e.errors.is_empty()) } else { true };
        (crate::view::graph::node_title(n), works)
    };
    assert_eq!(user(&w), (name.clone(), true));
    // Renamed in its package, from the Explorer: the node is called so.
    w.send(Message::Explorer(ExplorerMessage::StartRename(a, graph)));
    w.send(Message::Explorer(ExplorerMessage::RenameText("Old Walls".into())));
    w.send(Message::Explorer(ExplorerMessage::CommitRename));
    assert_eq!(w.app.package_doc(a).unwrap().package().graph(graph).unwrap().name, "Old Walls");
    assert_eq!(w.app.doc_key, b, "the other package still edited");
    assert_eq!(user(&w), ("Old Walls".to_string(), true), "called as the graph is now");
    // Deleted, from the Explorer: the node says it cannot be found.
    w.send(Message::Explorer(ExplorerMessage::DeleteGraph(a, graph)));
    assert!(w.app.package_doc(a).unwrap().package().graph(graph).is_none());
    assert_eq!(w.app.doc_key, b);
    let errors = w.app.eval.node(node).map(|e| e.errors.clone()).unwrap_or_default();
    assert!(!errors.is_empty(), "the node says the graph is gone: {:?}", w.app.eval.node(node));
    // The other package's undo is its own: nothing to undo of the wall's in it.
    let steps = w.app.doc.can_undo();
    assert!(steps, "the drop is a step of its own package");
    // The wall's package undoes the deletion: the node works again.
    w.send(Message::Explorer(ExplorerMessage::OpenGraph(a, w.app.package_doc(a).unwrap().current())));
    w.send(Message::Undo);
    assert!(w.app.package_doc(a).unwrap().package().graph(graph).is_some(), "the wall is back");
    w.send(Message::Explorer(ExplorerMessage::OpenGraph(b, w.app.package_doc(b).unwrap().current())));
    assert_eq!(user(&w), ("Old Walls".to_string(), true), "it works again");
    // And the rename undone: called as it was.
    w.send(Message::Explorer(ExplorerMessage::OpenGraph(a, graph)));
    w.send(Message::Undo);
    w.send(Message::Explorer(ExplorerMessage::OpenGraph(b, w.app.package_doc(b).unwrap().current())));
    assert_eq!(user(&w), (name, true), "called as it was");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A Uniform Color added and selected, and the color it has.
fn a_uniform_color(w: &mut Harness) -> brewed_texture_core::graph::NodeId {
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::UNIFORM_COLOR, pos: [0.0, 0.0] }));
    w.send(Message::Seal);
    let id = w.app.graph().nodes_of(NodeKind::UNIFORM_COLOR).map(|n| n.id).max().expect("the node");
    w.send(Message::Graph(GraphMessage::Select { node: id, mode: SelectMode::Replace }));
    draw_frame(w, std::time::Instant::now());
    id
}

fn color_of(w: &Harness, id: brewed_texture_core::graph::NodeId) -> [f32; 4] {
    match w.app.graph().node(id).and_then(|n| n.value("color")) {
        Some(ParamValue::Color(c)) => *c,
        other => panic!("no color: {other:?}"),
    }
}

/// A color parameter is set in Properties without the Color Editor: a click on its strip takes
/// the color there, its channels are said from 0 to 255 or from 0 to 1 and as red, green, blue or
/// hue, saturation, value, and none goes past its ends (a color could be given negative
/// channels). A Uniform Color that makes grayscale has a level of gray: its strip is black to
/// white, and whatever it is given, it keeps the gray of.
#[test]
fn a_color_is_set_in_properties_and_a_gray_is_a_level() {
    use crate::app::{ColorEditorMessage, ColorTarget, ColorUnits};
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let id = a_uniform_color(&mut w);
    let target = ColorTarget::Param(id, "color".into());
    let area = iced::Rectangle::new(Point::new(w.size.width - 400.0, 0.0), Size::new(400.0, w.size.height));
    // Its strip: a click takes the color there, as bright as colors are.
    let on_strip = |w: &mut Harness| {
        let target = target.clone();
        find_click(w, area, move |m| matches!(m, Message::SetColor(t, _) if *t == target)).expect("the strip of the color")
    };
    let strip = on_strip(&mut w);
    click(&mut w, strip);
    let picked = color_of(&w, id);
    let (most, least) = (picked[..3].iter().copied().fold(0.0, f32::max), picked[..3].iter().copied().fold(1.0, f32::min));
    assert!(most > 0.5 && least < 0.01 && picked[3] == 1.0, "a color of the strip, its alpha kept: {picked:?}");
    w.snapshot("color_field");
    // No channel past its ends, whatever is asked.
    w.send(Message::SetColor(target.clone(), [-0.5, 1.5, 0.25, 7.0]));
    assert_eq!(color_of(&w, id), [0.0, 1.0, 0.25, 1.0]);
    // How channels are said: from 0 to 1, and as hue, saturation and value.
    assert_eq!(w.app.ui.color_units, ColorUnits::default(), "from 0 to 255, red, green and blue, at first");
    let units = |w: &mut Harness, wanted: ColorUnits| {
        find_click(w, area, move |m| matches!(m, Message::ColorEditor(ColorEditorMessage::Units(u)) if *u == wanted)).expect("the button")
    };
    let float = units(&mut w, ColorUnits { float: true, hsv: false });
    click(&mut w, float);
    let hsv = units(&mut w, ColorUnits { float: true, hsv: true });
    click(&mut w, hsv);
    assert_eq!(w.app.ui.color_units, ColorUnits { float: true, hsv: true });
    assert!(text_fits::cut_short(&mut w).is_empty(), "{:?}", text_fits::cut_short(&mut w));
    w.snapshot("color_field_hsv");

    // Grayscale: a level of gray. Whatever it is given, the gray of it.
    w.send(Message::Edit(Edit::SetParam { node: id, key: "output".into(), value: ParamValue::Enum(1) }));
    w.send(Message::Seal);
    assert!(w.app.target_gray(&target));
    w.send(Message::SetColor(target.clone(), [1.0, 0.0, 0.0, 1.0]));
    let gray = color_of(&w, id);
    assert!(gray[0] == gray[1] && gray[1] == gray[2] && (gray[0] - 0.2126).abs() < 1e-4, "the gray of red: {gray:?}");
    // Its strip, black to white: a click near its right end, a light gray.
    draw_frame(&mut w, std::time::Instant::now());
    let strip = on_strip(&mut w);
    let near_its_end = Point::new(w.size.width - 60.0, strip.y);
    click(&mut w, near_its_end);
    let light = color_of(&w, id);
    assert!(light[0] == light[1] && light[1] == light[2] && light[0] > 0.5, "a light gray: {light:?}");
    // (No hue to say of a gray: its button is not there.)
    let with_hue = ColorUnits { float: true, hsv: false };
    assert!(find_click(&mut w, area, move |m| matches!(m, Message::ColorEditor(ColorEditorMessage::Units(u)) if *u == with_hue)).is_none());
    w.snapshot("color_field_gray");
    // In color again, it is a color again.
    w.send(Message::Edit(Edit::SetParam { node: id, key: "output".into(), value: ParamValue::Enum(0) }));
    assert!(!w.app.target_gray(&target));
}

/// The Color Editor's own controls: Paste takes the color the clipboard says in hexadecimal, as
/// other programs copy it, and says so of what is none; its hexadecimal typed is taken as soon as
/// it says a color, and kept as typed meanwhile; Invert turns each channel over and To Gray makes
/// it its gray, a step of undo each; Copy says what it copied. Of a level of gray, what is pasted
/// is its gray.
#[test]
fn the_color_editor_pastes_reads_hexadecimal_inverts_and_grays() {
    use crate::app::{ColorEditorMessage as E, ColorTarget};
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let id = a_uniform_color(&mut w);
    let target = ColorTarget::Param(id, "color".into());
    w.send(Message::ColorPicker(Some(target.clone())));
    let said = |w: &Harness| w.app.ui.status.as_ref().map(|s| (s.text.clone(), s.error)).unwrap_or_default();
    let step = |w: &mut Harness, m: E| w.send(Message::Step(Box::new(Message::ColorEditor(m))));
    // Pasted, as Photoshop or Unreal copy a color.
    step(&mut w, E::Pasted(Some(" #FF8000 ".into())));
    assert_eq!(color_of(&w, id), [1.0, 128.0 / 255.0, 0.0, 1.0]);
    step(&mut w, E::Pasted(Some("ff0000".into())));
    assert_eq!(color_of(&w, id), [1.0, 0.0, 0.0, 1.0]);
    // What is no color is not taken, and said.
    step(&mut w, E::Pasted(Some("a brick wall".into())));
    assert_eq!(color_of(&w, id), [1.0, 0.0, 0.0, 1.0]);
    assert!(said(&w).1 && said(&w).0.contains("is not a color"), "{:?}", said(&w));
    step(&mut w, E::Pasted(None));
    assert!(said(&w).1 && said(&w).0.contains("no color"), "{:?}", said(&w));
    // Typed: taken once it says a color, as typed until then.
    w.send(Message::ColorEditor(E::Hex("#00F".into())));
    assert_eq!(color_of(&w, id), [0.0, 0.0, 1.0, 1.0]);
    w.send(Message::ColorEditor(E::Hex("#00FF".into())));
    assert_eq!(color_of(&w, id), [0.0, 0.0, 1.0, 1.0], "not a color yet");
    assert_eq!(w.app.ui.color_hex.as_ref().map(|(typed, _)| typed.as_str()), Some("#00FF"), "kept as typed");
    w.send(Message::ColorEditor(E::Hex("#00FF00".into())));
    assert_eq!(color_of(&w, id), [0.0, 1.0, 0.0, 1.0]);
    w.send(Message::Seal);
    // Copy says what it copied.
    step(&mut w, E::Copy);
    assert_eq!(said(&w), ("Copied #00FF00".to_string(), false));
    // Invert, then To Gray: a step each.
    step(&mut w, E::Invert);
    assert_eq!(color_of(&w, id), [1.0, 0.0, 1.0, 1.0]);
    step(&mut w, E::ToGray);
    let gray = color_of(&w, id);
    assert!(gray[0] == gray[1] && gray[1] == gray[2] && (gray[0] - (0.2126 + 0.0722)).abs() < 1e-4, "{gray:?}");
    w.send(Message::Undo);
    assert_eq!(color_of(&w, id), [1.0, 0.0, 1.0, 1.0], "gray no more");
    w.send(Message::Undo);
    assert_eq!(color_of(&w, id), [0.0, 1.0, 0.0, 1.0], "turned back over");
    assert!(text_fits::cut_short(&mut w).is_empty(), "{:?}", text_fits::cut_short(&mut w));
    w.snapshot("color_editor");

    // A level of gray: what is pasted is its gray, and there is no To Gray to press.
    w.send(Message::Edit(Edit::SetParam { node: id, key: "output".into(), value: ParamValue::Enum(1) }));
    w.send(Message::Seal);
    step(&mut w, E::Pasted(Some("#0000FF".into())));
    let gray = color_of(&w, id);
    assert!(gray[0] == gray[1] && gray[1] == gray[2] && (gray[0] - 0.0722).abs() < 1e-4, "the gray of blue: {gray:?}");
    assert!(texts(&mut w, "To Gray").is_empty() && !texts(&mut w, "Invert").is_empty());
    assert!(text_fits::cut_short(&mut w).is_empty(), "{:?}", text_fits::cut_short(&mut w));
    w.snapshot("color_editor_gray");
}
