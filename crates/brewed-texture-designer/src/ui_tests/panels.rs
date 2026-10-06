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
