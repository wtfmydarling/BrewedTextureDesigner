//! The context menus of the graph, each item clicked as a hand clicks it: it does what it says.

use super::*;

/// The canvas, framed and told where it is.
fn canvas() -> Option<Harness> {
    let mut w = Harness::new(Box::new(view::root), Size::new(1680.0, 1000.0))?;
    w.input(&[iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))]);
    w.send(Message::Graph(GraphMessage::ClearSelection));
    w.send(Message::Frame);
    w.input(&[]);
    Some(w)
}

macro_rules! canvas {
    () => {
        match canvas() {
            Some(w) => w,
            None => {
                eprintln!("no GPU for the headless renderer: test skipped");
                return;
            }
        }
    };
}

/// Opens the context menu of `target` and clicks its item `label`.
fn choose(w: &mut Harness, target: ContextTarget, label: &str) {
    w.send(Message::Graph(GraphMessage::Context { at: Point::new(600.0, 300.0), target }));
    w.input(&[]);
    let item = *texts(w, label).last().unwrap_or_else(|| panic!("{label} in the menu"));
    click(w, item.center());
    assert!(w.app.ui.context.is_none(), "the menu closes once {label} is chosen");
}

fn first(w: &Harness, kind: NodeKind) -> brewed_texture_core::graph::NodeId {
    w.app.graph().nodes_of(kind).next().unwrap_or_else(|| panic!("a {kind:?}")).id
}

/// A node's menu: Bypass Node bypasses it and then says Enable Node, which gives it back;
/// Disconnect Inputs lets go of what goes into it; Duplicate copies it; Frame in a Comment frames
/// it; Delete takes it away. Each is one undo step.
#[test]
fn the_menu_of_a_node_does_what_it_says() {
    let mut w = canvas!();
    let levels = first(&w, NodeKind::LEVELS);
    let bypassed = |w: &Harness| w.app.graph().node(levels).is_some_and(|n| n.bypass);
    choose(&mut w, ContextTarget::Node(levels), "Bypass Node");
    assert!(bypassed(&w), "bypassed");
    choose(&mut w, ContextTarget::Node(levels), "Enable Node");
    assert!(!bypassed(&w), "and given back");
    // Disconnect Inputs: nothing goes into it any more; one undo puts it back.
    let into = |w: &Harness| w.app.graph().links.iter().filter(|l| l.to == levels).count();
    let before = into(&w);
    assert!(before > 0);
    choose(&mut w, ContextTarget::Node(levels), "Disconnect Inputs");
    assert_eq!(into(&w), 0);
    w.send(Message::Undo);
    assert_eq!(into(&w), before, "one undo puts them back");
    // Duplicate.
    let count = |w: &Harness| w.app.graph().nodes_of(NodeKind::LEVELS).count();
    let levels_before = count(&w);
    choose(&mut w, ContextTarget::Node(levels), "Duplicate");
    assert_eq!(count(&w), levels_before + 1);
    w.send(Message::Undo);
    // Frame in a Comment: a comment around it.
    let comments = w.app.graph().comments.len();
    w.send(Message::Graph(GraphMessage::Select { node: levels, mode: SelectMode::Replace }));
    choose(&mut w, ContextTarget::Node(levels), "Frame in a Comment");
    assert_eq!(w.app.graph().comments.len(), comments + 1, "a comment");
    let framed = w.app.graph().comments.last().unwrap().clone();
    let node = w.app.graph().node(levels).unwrap();
    assert!(
        framed.pos[0] <= node.pos[0] && framed.pos[1] <= node.pos[1] && framed.pos[0] + framed.size[0] >= node.pos[0] + 100.0,
        "around the node: {framed:?} for {:?}",
        node.pos
    );
    w.send(Message::Undo);
    // Delete.
    choose(&mut w, ContextTarget::Node(levels), "Delete");
    assert!(w.app.graph().node(levels).is_none(), "taken away");
    w.send(Message::Undo);
    assert!(w.app.graph().node(levels).is_some(), "one undo brings it back");
}

/// A generator has nothing to pass through: Bypass Node is not offered; nor is Open Reference
/// on a node that uses no graph.
#[test]
fn the_menu_of_a_node_offers_only_what_it_can_do() {
    let mut w = canvas!();
    let noise = first(&w, NodeKind::NOISE);
    w.send(Message::Graph(GraphMessage::Context { at: Point::new(600.0, 300.0), target: ContextTarget::Node(noise) }));
    w.input(&[]);
    for label in ["Bypass Node", "Open Reference", "Export Output…"] {
        let item = *texts(&mut w, label).last().unwrap_or_else(|| panic!("{label} in the menu"));
        let asked = clicked(&mut w, item.center());
        assert!(!asked.iter().any(|m| matches!(m, Message::FromMenu(_))), "{label} is not offered for a generator: {asked:?}");
    }
    w.send(Message::Context(None));
}

/// The canvas's menu: Add Node… opens the menu that adds a node where the canvas was clicked;
/// Select All selects every node.
#[test]
fn the_menu_of_the_canvas_does_what_it_says() {
    let mut w = canvas!();
    let at = Point::new(600.0, 300.0);
    let graph_pos = w.app.ui.graph_view.to_graph(at);
    choose(&mut w, ContextTarget::Canvas { graph_pos }, "Add Node…");
    let menu = w.app.ui.add_menu.clone().expect("the add menu");
    assert!(menu.at.distance(at) < 1.0, "where the canvas was clicked: {:?}", menu.at);
    w.send(Message::AddMenu(crate::app::AddMenuMessage::Close));
    choose(&mut w, ContextTarget::Canvas { graph_pos }, "Select All");
    assert_eq!(w.app.ui.selection.len(), w.app.graph().nodes.len(), "every node");
}

/// A link's menu deletes it, in one undo step.
#[test]
fn the_menu_of_a_link_deletes_it() {
    let mut w = canvas!();
    let link = w.app.graph().links.first().cloned().expect("a link");
    choose(&mut w, ContextTarget::Link(link.clone()), "Delete Link");
    assert!(!w.app.graph().links.contains(&link), "gone");
    w.send(Message::Undo);
    assert!(w.app.graph().links.contains(&link), "back");
}

/// A comment's menu: Edit Text starts typing its text, Color… opens a picker of its color, and
/// Delete Comment takes it away (what it framed stays).
#[test]
fn the_menu_of_a_comment_does_what_it_says() {
    let mut w = canvas!();
    let comment = w.app.graph().comments.first().map(|c| c.id).expect("a comment");
    choose(&mut w, ContextTarget::Comment(comment), "Edit Text");
    assert!(w.app.ui.description.as_ref().is_some_and(|(id, _)| *id == comment) || w.app.ui.selected_comment == Some(comment), "being edited");
    choose(&mut w, ContextTarget::Comment(comment), "Color…");
    assert_eq!(w.app.ui.color_picker, Some(ColorTarget::Comment(comment)), "a picker of its color");
    w.send(Message::ColorPicker(None));
    let nodes = w.app.graph().nodes.len();
    choose(&mut w, ContextTarget::Comment(comment), "Delete Comment");
    assert!(w.app.graph().comment(comment).is_none(), "gone");
    assert_eq!(w.app.graph().nodes.len(), nodes, "what it framed stays");
}
