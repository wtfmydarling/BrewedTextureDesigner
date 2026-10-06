//! The graph's canvas, driven as a hand drives it: what a click, a drag, a double click, the
//! wheel and the keys do to its nodes and links.

use iced::{Rectangle, Vector};
use brewed_texture_core::graph::{NodeId, SocketRef};

use super::*;
use crate::app::AddMenuMessage;

/// Where a node is in the window.
fn node_rect(w: &Harness, node: NodeId) -> Rectangle {
    let graph = w.app.graph();
    let view = w.app.ui.graph_view;
    let t = view::graph::layout::Transform { pan: view.pan, zoom: view.zoom };
    let l = view::graph::layout::layout(graph, graph.node(node).expect("the node"), &t, Vector::ZERO);
    l.rect + Vector::new(view.bounds.x, view.bounds.y)
}

/// Where a node's output socket is in the window.
fn output_socket(w: &Harness, node: NodeId, key: &str) -> Point {
    let graph = w.app.graph();
    let view = w.app.ui.graph_view;
    let t = view::graph::layout::Transform { pan: view.pan, zoom: view.zoom };
    let l = view::graph::layout::layout(graph, graph.node(node).expect("the node"), &t, Vector::ZERO);
    let (_, p) = l.outputs.iter().find(|(s, _)| s.key == key).expect("the socket");
    *p + Vector::new(view.bounds.x, view.bounds.y)
}

/// A point of a node to take it by: in its body, under its sockets (not on a socket, not on
/// its header's buttons).
fn grip(w: &Harness, node: NodeId) -> Point {
    let r = node_rect(w, node);
    Point::new(r.center_x(), r.y + r.height * 0.75)
}

/// A click on a node, where it is taken by.
fn click_on(w: &mut Harness, node: NodeId) {
    let at = grip(w, node);
    click(w, at);
}

/// A window with the demo in it, the canvas told where it is, everything framed.
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

/// Holds `modifiers` down, as the window says (the canvas reads them from it).
fn hold(w: &mut Harness, modifiers: keyboard::Modifiers) {
    w.input(&[iced::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers))]);
    let main = w.main_window();
    w.window_event(main, iced::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)));
}

/// A key pressed with the cursor at `at`, as the window says it: `typing` when a field took it.
fn key_at(w: &mut Harness, at: Point, named: keyboard::key::Named, modifiers: keyboard::Modifiers, typing: bool) {
    let main = w.main_window();
    w.window_event(main, iced::Event::Mouse(mouse::Event::CursorMoved { position: at }));
    let key = Key::Named(named);
    let event = iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers,
        text: None,
        repeat: false,
    });
    let status = if typing { iced::event::Status::Captured } else { iced::event::Status::Ignored };
    w.send(Message::Runtime(event, status, main));
}

/// A letter key pressed with the cursor at `at` (Ctrl+D, …).
fn letter_at(w: &mut Harness, at: Point, letter: &str, code: keyboard::key::Code, modifiers: keyboard::Modifiers) {
    let main = w.main_window();
    w.window_event(main, iced::Event::Mouse(mouse::Event::CursorMoved { position: at }));
    key(w, letter, keyboard::key::Physical::Code(code), modifiers);
}

fn selected(w: &Harness) -> Vec<NodeId> {
    let mut s: Vec<NodeId> = w.app.ui.selection.iter().copied().collect();
    s.sort_by_key(|id| id.0);
    s
}

fn first(w: &Harness, kind: NodeKind) -> NodeId {
    w.app.graph().nodes_of(kind).next().unwrap_or_else(|| panic!("a {kind:?}")).id
}

/// A click selects a node in place of what was selected; with Shift it is added; with Ctrl it is
/// taken away when it was selected, added when not. A click on the canvas lets every node go,
/// but not with Shift or Ctrl held.
#[test]
fn clicks_select_nodes() {
    let mut w = canvas!();
    let (levels, blur, noise) = (first(&w, NodeKind::LEVELS), first(&w, NodeKind::BLUR), first(&w, NodeKind::NOISE));
    click_on(&mut w, levels);
    assert_eq!(selected(&w), vec![levels]);
    click_on(&mut w, blur);
    assert_eq!(selected(&w), vec![blur], "in place of what was selected");
    hold(&mut w, keyboard::Modifiers::SHIFT);
    click_on(&mut w, levels);
    click_on(&mut w, noise);
    let mut all = vec![levels, blur, noise];
    all.sort_by_key(|id| id.0);
    assert_eq!(selected(&w), all, "Shift adds");
    // A click on the canvas with Shift held keeps them.
    let empty = Point::new(w.app.ui.graph_view.bounds.x + 30.0, w.app.ui.graph_view.bounds.y + 60.0);
    assert!(w.app.graph().nodes.iter().all(|n| !node_rect(&w, n.id).contains(empty)), "the corner is empty");
    click(&mut w, empty);
    assert_eq!(selected(&w).len(), 3, "Shift keeps them on a click beside them");
    hold(&mut w, keyboard::Modifiers::CTRL);
    click_on(&mut w, blur);
    assert!(!w.app.ui.selection.contains(&blur) && w.app.ui.selection.len() == 2, "Ctrl takes it away");
    // (Some pixels aside: within 6 of the first at once, it would be a double click, which
    // shows the node.)
    let aside = grip(&w, blur) + Vector::new(8.0, 0.0);
    click(&mut w, aside);
    assert!(w.app.ui.selection.contains(&blur), "and adds it back");
    hold(&mut w, keyboard::Modifiers::default());
    click(&mut w, empty);
    assert!(w.app.ui.selection.is_empty(), "a click on the canvas lets them go");
}

/// A drag on the canvas selects the nodes its box touches; with Shift, beside those selected.
#[test]
fn a_box_selects_what_it_touches() {
    let mut w = canvas!();
    let (levels, blur) = (first(&w, NodeKind::LEVELS), first(&w, NodeKind::BLUR));
    let r = node_rect(&w, levels);
    // From just above-left of the Levels to its middle: it alone is touched.
    let from = Point::new(r.x - 6.0, r.y - 6.0);
    assert!(w.app.graph().nodes.iter().all(|n| !node_rect(&w, n.id).contains(from)), "the box starts on the canvas");
    drag(&mut w, from, r.center());
    assert!(w.app.ui.selection.contains(&levels), "{:?}", selected(&w));
    let touched: Vec<NodeId> = w.app.graph().nodes.iter().filter(|n| node_rect(&w, n.id).intersects(&Rectangle::new(from, Size::new(r.center().x - from.x, r.center().y - from.y)))).map(|n| n.id).collect();
    assert_eq!(selected(&w).len(), touched.len(), "what the box touches, no more");
    // With Shift: the Blur's box adds it.
    let b = node_rect(&w, blur);
    let from = Point::new(b.x + b.width + 6.0, b.y + b.height + 6.0);
    if w.app.graph().nodes.iter().all(|n| !node_rect(&w, n.id).contains(from)) {
        hold(&mut w, keyboard::Modifiers::SHIFT);
        drag(&mut w, from, b.center());
        hold(&mut w, keyboard::Modifiers::default());
        assert!(w.app.ui.selection.contains(&levels) && w.app.ui.selection.contains(&blur), "{:?}", selected(&w));
    }
}

/// A dragged node moves, with the others selected with it, as far as the mouse went (at the
/// zoom of the view); one undo puts them all back. A node not selected moves alone, and is
/// selected.
#[test]
fn dragged_nodes_move_together_and_come_back_in_one_undo() {
    let mut w = canvas!();
    let (levels, blur, noise) = (first(&w, NodeKind::LEVELS), first(&w, NodeKind::BLUR), first(&w, NodeKind::NOISE));
    let pos = |w: &Harness, id| w.app.graph().node(id).unwrap().pos;
    let (l0, b0, n0) = (pos(&w, levels), pos(&w, blur), pos(&w, noise));
    w.send(Message::Graph(GraphMessage::Select { node: levels, mode: SelectMode::Replace }));
    w.send(Message::Graph(GraphMessage::Select { node: blur, mode: SelectMode::Add }));
    let zoom = w.app.ui.graph_view.zoom;
    let at = grip(&w, levels);
    drag(&mut w, at, at + Vector::new(60.0, 30.0));
    let moved = |a: [f32; 2], b: [f32; 2]| [b[0] - a[0], b[1] - a[1]];
    let d = moved(l0, pos(&w, levels));
    assert!((d[0] - 60.0 / zoom).abs() < 0.5 && (d[1] - 30.0 / zoom).abs() < 0.5, "as far as the mouse went: {d:?} at {zoom}");
    assert_eq!(moved(b0, pos(&w, blur)), d, "the other one with it");
    assert_eq!(pos(&w, noise), n0, "what was not selected stays");
    w.send(Message::Undo);
    assert_eq!((pos(&w, levels), pos(&w, blur)), (l0, b0), "one undo puts both back");
    // Not selected: it alone, and selected.
    let at = grip(&w, noise);
    drag(&mut w, at, at + Vector::new(-40.0, 20.0));
    assert_ne!(pos(&w, noise), n0);
    assert_eq!((pos(&w, levels), pos(&w, blur)), (l0, b0));
    assert_eq!(selected(&w), vec![noise]);
}

/// Delete takes the selected nodes away and their links with them, wherever the cursor is in the
/// window (as Duplicate and Frame do); one undo brings both back. Typed in a field, the key is the
/// field's: it takes nothing away.
#[test]
fn delete_takes_nodes_and_links_and_undo_brings_them_back() {
    let mut w = canvas!();
    let levels = first(&w, NodeKind::LEVELS);
    let count = |w: &Harness| (w.app.graph().nodes.len(), w.app.graph().links.len());
    let before = count(&w);
    let links_of = |w: &Harness| w.app.graph().links.iter().filter(|l| l.from == levels || l.to == levels).count();
    let linked = links_of(&w);
    assert!(linked >= 2, "the Levels is linked both ways");
    w.send(Message::Graph(GraphMessage::Select { node: levels, mode: SelectMode::Replace }));
    let over = w.app.ui.graph_view.bounds.center();
    key_at(&mut w, over, keyboard::key::Named::Delete, keyboard::Modifiers::default(), true);
    assert_eq!(count(&w), before, "typed in a field, Delete is the field's");
    let away = Point::new(20.0, w.app.ui.window_size.height - 60.0);
    key_at(&mut w, away, keyboard::key::Named::Delete, keyboard::Modifiers::default(), false);
    assert_eq!(count(&w), (before.0 - 1, before.1 - linked), "the node and its links, the cursor over the library too");
    assert!(w.app.graph().node(levels).is_none() && w.app.ui.selection.is_empty());
    w.send(Message::Undo);
    assert_eq!(count(&w), before, "one undo brings both back");
    assert_eq!(links_of(&w), linked);
}

/// Ctrl+D duplicates the selection beside it: the copies are selected, the originals not, and
/// the links between the copies are copied with them (not those to what was not copied).
#[test]
fn duplicates_are_selected_beside_the_originals() {
    let mut w = canvas!();
    let (levels, blur) = (first(&w, NodeKind::LEVELS), first(&w, NodeKind::BLUR));
    let linked = w.app.graph().links.iter().any(|l| (l.from == levels && l.to == blur) || (l.from == blur && l.to == levels));
    w.send(Message::Graph(GraphMessage::Select { node: levels, mode: SelectMode::Replace }));
    w.send(Message::Graph(GraphMessage::Select { node: blur, mode: SelectMode::Add }));
    let (nodes, links) = (w.app.graph().nodes.len(), w.app.graph().links.len());
    let over = w.app.ui.graph_view.bounds.center();
    letter_at(&mut w, over, "d", keyboard::key::Code::KeyD, keyboard::Modifiers::CTRL);
    assert_eq!(w.app.graph().nodes.len(), nodes + 2, "two copies");
    let copies = selected(&w);
    assert_eq!(copies.len(), 2);
    assert!(!copies.contains(&levels) && !copies.contains(&blur), "the copies are selected, not the originals");
    let between = w.app.graph().links.iter().filter(|l| copies.contains(&l.from) && copies.contains(&l.to)).count();
    assert_eq!(between, usize::from(linked), "their link between them is copied");
    assert_eq!(w.app.graph().links.len(), links + between, "and no other");
    for copy in &copies {
        let node = w.app.graph().node(*copy).unwrap();
        let original = w.app.graph().nodes.iter().find(|n| n.kind == node.kind && !copies.contains(&n.id) && (n.id == levels || n.id == blur)).unwrap();
        assert_ne!(node.pos, original.pos, "beside, not on top of it");
    }
    w.send(Message::Undo);
    assert_eq!(w.app.graph().nodes.len(), nodes, "one undo takes both away");
}

/// A double click on the canvas opens the menu that adds a node at the cursor; what is typed
/// narrows it, Enter adds the first it leaves, where the menu was opened, selected; Esc closes the
/// menu, adding nothing.
#[test]
fn a_double_click_adds_a_node_where_it_is() {
    let mut w = canvas!();
    let b = w.app.ui.graph_view.bounds;
    let empty = Point::new(b.x + 40.0, b.y + b.height - 50.0);
    assert!(w.app.graph().nodes.iter().all(|n| !node_rect(&w, n.id).contains(empty)), "an empty place");
    click(&mut w, empty);
    click(&mut w, empty);
    let menu = w.app.ui.add_menu.clone().expect("the menu opens");
    assert!(menu.at.distance(empty) < 1.0, "at the cursor: {:?}", menu.at);
    let at = w.app.ui.graph_view.to_graph(empty);
    // Typed: narrowed to what is named so.
    assert!(matches!(w.operate(focusable::focus::<()>(view::add_menu::search_id())), Outcome::None));
    w.input(&typing("invert"));
    assert_eq!(w.app.ui.add_menu.as_ref().map(|m| m.query.as_str()), Some("invert"));
    let before = w.app.graph().nodes_of(NodeKind::INVERT).count();
    let enter = Key::Named(keyboard::key::Named::Enter);
    w.input(&[iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: enter.clone(),
        modified_key: enter,
        physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::default(),
        text: None,
        repeat: false,
    })]);
    assert!(w.app.ui.add_menu.is_none(), "Enter adds it and closes the menu");
    let added: Vec<_> = w.app.graph().nodes_of(NodeKind::INVERT).map(|n| (n.id, n.pos)).collect();
    assert_eq!(added.len(), before + 1, "an Invert");
    let (id, pos) = *added.last().unwrap();
    assert!((pos[0] - (at[0] - 80.0)).abs() < 1.0 && (pos[1] - (at[1] - 12.0)).abs() < 1.0, "where the menu was opened: {pos:?} for {at:?}");
    assert_eq!(selected(&w), vec![id], "selected");
    // Esc: nothing. (Elsewhere: the Invert is where the menu was.)
    let empty = Point::new(b.x + b.width - 40.0, b.y + 60.0);
    assert!(w.app.graph().nodes.iter().all(|n| !node_rect(&w, n.id).contains(empty)), "an empty place");
    click(&mut w, empty);
    click(&mut w, empty);
    assert!(w.app.ui.add_menu.is_some());
    let nodes = w.app.graph().nodes.len();
    press(&mut w, keyboard::key::Named::Escape);
    assert!(w.app.ui.add_menu.is_none() && w.app.graph().nodes.len() == nodes, "Esc adds nothing");
}

/// A link dragged out of an output and dropped on the canvas opens the menu there; the node chosen
/// comes connected to that output, in one undo step.
#[test]
fn a_link_dropped_on_the_canvas_adds_a_connected_node() {
    let mut w = canvas!();
    let noise = first(&w, NodeKind::NOISE);
    let from = output_socket(&w, noise, "o:output");
    let b = w.app.ui.graph_view.bounds;
    let empty = Point::new(b.x + 40.0, b.y + b.height - 50.0);
    drag(&mut w, from, empty);
    let menu = w.app.ui.add_menu.clone().expect("the menu opens where the link was dropped");
    assert_eq!(menu.pending.as_ref().map(|s| (s.node, s.key.as_str())), Some((noise, "o:output")));
    let steps = w.app.doc.undo_label().map(str::to_string);
    w.send(Message::AddMenu(AddMenuMessage::Choose(NodeKind::INVERT)));
    let invert = w.app.graph().nodes_of(NodeKind::INVERT).last().map(|n| n.id).expect("an Invert");
    assert_eq!(w.app.graph().link_into(invert, "i:input").map(|l| l.from), Some(noise), "connected to the output");
    w.send(Message::Undo);
    assert!(w.app.graph().node(invert).is_none(), "one undo takes node and link away");
    assert_eq!(w.app.doc.undo_label().map(str::to_string), steps);
    let _ = SocketRef { node: noise, key: String::new(), output: true };
}

/// The wheel zooms around the cursor: the point of the graph under it stays under it; no further
/// than 12 % out and 300 % in.
#[test]
fn the_wheel_zooms_around_the_cursor_within_limits() {
    let mut w = canvas!();
    let b = w.app.ui.graph_view.bounds;
    let at = Point::new(b.x + b.width * 0.3, b.y + b.height * 0.6);
    let under = w.app.ui.graph_view.to_graph(at);
    w.hover(at);
    w.mouse(mouse::Event::WheelScrolled { delta: mouse::ScrollDelta::Lines { x: 0.0, y: 2.0 } });
    let zoom = w.app.ui.graph_view.zoom;
    let still = w.app.ui.graph_view.to_graph(at);
    assert!((still[0] - under[0]).abs() < 0.5 && (still[1] - under[1]).abs() < 0.5, "{still:?} under the cursor, was {under:?}");
    for _ in 0..80 {
        w.mouse(mouse::Event::WheelScrolled { delta: mouse::ScrollDelta::Lines { x: 0.0, y: 3.0 } });
    }
    assert!((w.app.ui.graph_view.zoom - 3.0).abs() < 1e-3, "in to 300 %: {}", w.app.ui.graph_view.zoom);
    for _ in 0..160 {
        w.mouse(mouse::Event::WheelScrolled { delta: mouse::ScrollDelta::Lines { x: 0.0, y: -3.0 } });
    }
    assert!((w.app.ui.graph_view.zoom - 0.12).abs() < 1e-3, "out to 12 %: {}", w.app.ui.graph_view.zoom);
    let still = w.app.ui.graph_view.to_graph(at);
    assert!((still[0] - under[0]).abs() < 1.0 && (still[1] - under[1]).abs() < 1.0, "still under the cursor at the limits");
    assert!(zoom > 0.0);
}

/// Frame (F over the canvas) brings what is selected into view, or everything when nothing is.
#[test]
fn frame_shows_the_selection_or_everything() {
    let mut w = canvas!();
    let b = w.app.ui.graph_view.bounds;
    let inside = |w: &Harness, id: NodeId| {
        let r = node_rect(w, id);
        r.x >= b.x - 0.5 && r.y >= b.y - 0.5 && r.x + r.width <= b.x + b.width + 0.5 && r.y + r.height <= b.y + b.height + 0.5
    };
    // Away from everything first.
    w.send(Message::Graph(GraphMessage::PanBy(Vector::new(5000.0, 5000.0))));
    let ids: Vec<NodeId> = w.app.graph().nodes.iter().map(|n| n.id).collect();
    assert!(ids.iter().all(|id| !inside(&w, *id)), "nothing in view");
    let over = b.center();
    letter_at(&mut w, over, "f", keyboard::key::Code::KeyF, keyboard::Modifiers::default());
    assert!(ids.iter().all(|id| inside(&w, *id)), "everything in view");
    // One node, selected: it, large.
    let levels = first(&w, NodeKind::LEVELS);
    w.send(Message::Graph(GraphMessage::Select { node: levels, mode: SelectMode::Replace }));
    let before = node_rect(&w, levels).width;
    letter_at(&mut w, over, "f", keyboard::key::Code::KeyF, keyboard::Modifiers::default());
    assert!(inside(&w, levels) && node_rect(&w, levels).width > before, "the selection, larger");
}

/// A comment added and taken away again by an undo is no longer selected: Delete has nothing to
/// take away, and the Edit menu says so.
#[test]
fn an_undone_comment_is_not_selected() {
    let mut w = canvas!();
    let comments = w.app.graph().comments.len();
    w.send(Message::AddComment);
    assert_eq!(w.app.graph().comments.len(), comments + 1);
    assert!(w.app.ui.selected_comment.is_some(), "the new comment is selected");
    w.send(Message::Undo);
    assert_eq!(w.app.graph().comments.len(), comments);
    assert_eq!(w.app.ui.selected_comment, None, "it is not selected any more");
    w.send(Message::Menu(Some(crate::app::MenuId::Edit)));
    let delete = *texts(&mut w, "Delete").first().expect("Edit → Delete");
    let asked = clicked(&mut w, delete.center());
    assert!(!asked.iter().any(|m| matches!(m, Message::FromMenu(inner) if matches!(**inner, Message::Delete))), "Delete is not offered: {asked:?}");
}

/// A color picker open for a node that an undo takes away closes with it: the next Esc does what
/// it does with nothing open (it was kept by the picker, shown for nothing).
#[test]
fn a_color_picker_closes_with_its_node() {
    let mut w = canvas!();
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::UNIFORM_COLOR, pos: [2400.0, -800.0] }));
    w.send(Message::Seal);
    let color = w.app.graph().nodes_of(NodeKind::UNIFORM_COLOR).last().expect("the new Uniform Color").id;
    let key = w.app.graph().node(color).unwrap().def().params.iter().find(|p| matches!(p.kind, brewed_texture_core::nodes::ParamKind::Color { .. })).expect("its color").key;
    w.send(Message::ColorPicker(Some(ColorTarget::Param(color, key.into()))));
    assert!(w.app.ui.color_picker.is_some());
    w.send(Message::Undo);
    assert!(w.app.graph().node(color).is_none());
    assert_eq!(w.app.ui.color_picker, None, "closed with its node");
    // The next Esc, from the Home screen, goes back to what is edited.
    w.send(Message::Home(crate::app::home::HomeMessage::Show(crate::app::home::HomePage::Home)));
    press(&mut w, keyboard::key::Named::Escape);
    assert_eq!(w.app.ui.home, None, "Esc went back from the Home screen");
}

/// What a comment says is drawn in the canvas only: a comment that reaches above the top of the
/// graph's view leaves the menu bar over it as it was (its words were drawn over the menus and
/// the window's title).
#[test]
fn a_comment_above_the_view_draws_nothing_over_the_menu_bar() {
    let mut w = canvas!();
    let comment = w.app.graph().comments.first().map(|c| c.id).expect("a comment");
    let long = "What the nodes in the frame do, said at length so that it runs on over many lines. ".repeat(12);
    w.send(Message::Edit(Edit::SetCommentDescription { id: comment, text: long }));
    w.send(Message::Seal);
    // At 100 % (smaller, its words are too small to be drawn), the comment's top 80 pixels above
    // the top of the view: its first words where the menu bar is, the rest running on below.
    let c = w.app.graph().comment(comment).unwrap().clone();
    w.app.ui.graph_view.zoom = 1.0;
    w.app.ui.graph_view.pan = Vector::new(40.0 - c.pos[0], -80.0 - c.pos[1]);
    w.input(&[]);
    let top = w.app.ui.graph_view.bounds.y;
    let r = comment_rect(&w, comment);
    assert!(r.y < top && r.y + r.height > top, "the comment straddles the top of the view: {r:?}, the view from {top}");
    // The menu bar between its menus and the document's name: the bar's color, unbroken.
    let pixels = w.pixels();
    let width = w.size.width as usize;
    let bar = crate::style::WINDOW;
    let expected = [(bar.r * 255.0).round() as i32, (bar.g * 255.0).round() as i32, (bar.b * 255.0).round() as i32];
    let help = *texts(&mut w, "Help").first().expect("the Help menu");
    let (from, to) = ((help.x + help.width + 40.0) as usize, (w.size.width * 0.55) as usize);
    let mut stray = 0;
    for y in 2..28usize {
        for x in from..to {
            let p = &pixels[(y * width + x) * 4..][..3];
            if p.iter().zip(expected).any(|(v, e)| (i32::from(*v) - e).abs() > 2) {
                stray += 1;
            }
        }
    }
    assert_eq!(stray, 0, "pixels drawn over the menu bar");
}

/// The graph draws in its view only: wherever its nodes, links and comments are — across the
/// top of the view, its bottom, its sides, large or small — every pixel of the window outside
/// the view is what it is with the graph out of sight.
#[test]
fn the_graph_draws_in_its_view_only() {
    let mut w = canvas!();
    // Words in the comments too, at every zoom they are drawn at.
    let comments: Vec<_> = w.app.graph().comments.iter().map(|c| c.id).collect();
    for id in comments {
        w.send(Message::Edit(Edit::SetCommentDescription { id, text: "What the nodes in the frame do, said at length. ".repeat(20) }));
    }
    w.send(Message::Seal);
    w.input(&[]);
    let b = w.app.ui.graph_view.bounds;
    let width = w.size.width as usize;
    let outside = |x: usize, y: usize| {
        let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
        // (A pixel of slack around the view: its edge is drawn by both.)
        x < b.x - 1.0 || x > b.x + b.width + 1.0 || y < b.y - 1.0 || y > b.y + b.height + 1.0
    };
    let view_at = |w: &mut Harness, zoom: f32, pan: Vector| {
        w.app.ui.graph_view.zoom = zoom;
        w.app.ui.graph_view.pan = pan;
        w.input(&[]);
        w.pixels()
    };
    let empty = view_at(&mut w, 1.0, Vector::new(1.0e6, 1.0e6));
    // The graph's middle at each edge and corner of the view, near and far.
    let (lo, hi) = w.app.graph().nodes.iter().fold(([f32::MAX; 2], [f32::MIN; 2]), |(lo, hi), n| {
        ([lo[0].min(n.pos[0]), lo[1].min(n.pos[1])], [hi[0].max(n.pos[0] + 160.0), hi[1].max(n.pos[1] + 200.0)])
    });
    let middle = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let mut found = Vec::new();
    let mut placements: Vec<(f32, Vector, String)> = Vec::new();
    for zoom in [0.3, 1.0, 2.5] {
        for (fx, fy) in [(0.5, 0.0), (0.5, 1.0), (0.0, 0.5), (1.0, 0.5), (0.0, 0.0), (1.0, 1.0)] {
            placements.push((zoom, Vector::new(b.width * fx - middle[0] * zoom, b.height * fy - middle[1] * zoom), format!("the graph's middle at {fx},{fy} of it")));
        }
    }
    // Each comment's words just past each edge: its top above the view, its left beside it, its
    // words running past its bottom and its right.
    let rects: Vec<[f32; 4]> = w.app.graph().comments.iter().map(|c| [c.pos[0], c.pos[1], c.size[0], c.size[1]]).collect();
    for [x, y, _, _] in rects {
        for (dx, dy, edge) in [(40.0, -60.0, "top"), (-60.0, 40.0, "left"), (40.0, b.height - 50.0, "bottom"), (b.width - 80.0, 40.0, "right")] {
            placements.push((1.0, Vector::new(dx - x, dy - y), format!("a comment's words past the {edge}")));
        }
    }
    for (zoom, at, said) in placements {
        {
            let drawn = view_at(&mut w, zoom, at);
            let mut stray = 0;
            for (i, (a, e)) in drawn.chunks(4).zip(empty.chunks(4)).enumerate() {
                let (x, y) = (i % width, i / width);
                if outside(x, y) && a.iter().zip(e).any(|(a, e)| (i32::from(*a) - i32::from(*e)).abs() > 2) {
                    stray += 1;
                }
            }
            if stray > 0 {
                found.push(format!("{stray} pixels outside the view at zoom {zoom}, {said}"));
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}

/// A title too long for its node's header is shortened in its middle, in what the header's
/// badges leave it: the badges (the format of an Output, the marker of the node shown in the 2D
/// view) and what is past the node are drawn as they are with a short title (the title ran over
/// them, and out of the node).
#[test]
fn a_long_title_leaves_the_badges_of_its_node_as_they_are() {
    let mut w = canvas!();
    let output = first(&w, NodeKind::OUTPUT);
    w.send(Message::Graph(GraphMessage::Preview(output)));
    w.app.ui.graph_view.zoom = 2.0;
    let r = node_rect(&w, output);
    let view = w.app.ui.graph_view;
    w.send(Message::Graph(GraphMessage::PanBy(Vector::new(view.bounds.center_x() - r.center_x(), view.bounds.center_y() - r.center_y()))));
    w.input(&[]);
    // The right of the header, where its badges are, and past the node.
    let (r, z) = (node_rect(&w, output), w.app.ui.graph_view.zoom);
    let header_h = view::graph::layout::HEADER_H * z;
    let (from, to) = ((r.x + r.width - 48.0 * z) as usize, (r.x + r.width + 40.0) as usize);
    let (top, bottom) = (r.y as usize + 2, (r.y + header_h) as usize - 1);
    let badges = |w: &mut Harness, name: &str| -> Vec<u8> {
        w.send(Message::Edit(Edit::SetParam { node: output, key: "name".into(), value: ParamValue::Text(name.into()) }));
        w.send(Message::Seal);
        w.input(&[]);
        let pixels = w.pixels();
        let width = w.size.width as usize;
        (top..bottom).flat_map(|y| pixels[(y * width + from) * 4..(y * width + to) * 4].to_vec()).collect()
    };
    let short = badges(&mut w, "Out");
    for long in ["WWWWMMMMWWWWMMMMWWWW", "base_color_of_the_bricks_and_of_the_mortar_between_them"] {
        let drawn = badges(&mut w, long);
        let differ = short.iter().zip(&drawn).filter(|(a, b)| (i32::from(**a) - i32::from(**b)).abs() > 2).count();
        assert_eq!(differ, 0, "{long}: the badges as they are with a short title");
    }
}

/// What a comment says in its title bar is cut, measured, to the frame: past a narrow frame
/// nothing is drawn of a long title of wide letters (it ran on past the frame's edge).
#[test]
fn a_long_title_of_a_comment_stays_in_its_frame() {
    let mut w = canvas!();
    // A narrow comment, alone where it is.
    w.send(Message::Edit(Edit::AddComment { text: "Bricks".into(), pos: [3000.0, 3000.0], size: [220.0, 160.0] }));
    w.send(Message::Seal);
    let id = w.app.graph().comments.last().map(|c| c.id).expect("the comment");
    w.app.ui.graph_view.zoom = 1.5;
    let r = comment_rect(&w, id);
    let view = w.app.ui.graph_view;
    w.send(Message::Graph(GraphMessage::PanBy(Vector::new(view.bounds.center_x() - r.center_x(), view.bounds.center_y() - r.center_y()))));
    w.input(&[]);
    let r = comment_rect(&w, id);
    // Past the frame's right edge, along its title bar.
    let (from, to) = ((r.x + r.width + 3.0) as usize, (r.x + r.width + 200.0) as usize);
    let (top, bottom) = (r.y as usize + 2, (r.y + 24.0) as usize);
    let past = |w: &mut Harness, title: &str| -> Vec<u8> {
        w.send(Message::Edit(Edit::SetCommentText { id, text: title.into() }));
        w.send(Message::Seal);
        w.input(&[]);
        let pixels = w.pixels();
        let width = w.size.width as usize;
        (top..bottom).flat_map(|y| pixels[(y * width + from) * 4..(y * width + to) * 4].to_vec()).collect()
    };
    let short = past(&mut w, "Bricks");
    let long = past(&mut w, "WIDE MORTAR WITH WORN EDGES WHERE THE BRICKS MEET");
    let differ = short.iter().zip(&long).filter(|(a, b)| (i32::from(**a) - i32::from(**b)).abs() > 2).count();
    assert_eq!(differ, 0, "nothing of the title past the frame");
}

/// The arrow keys and Page Up / Page Down move through what the menu that adds a node offers,
/// never past its ends, the list scrolling to keep the one chosen in sight; Enter adds that one.
#[test]
fn the_keys_move_through_the_add_menu_and_enter_adds_the_one_chosen() {
    use keyboard::key::Named;
    let mut w = canvas!();
    let b = w.app.ui.graph_view.bounds;
    let at = Point::new(b.center_x(), b.center_y());
    let graph_pos = w.app.ui.graph_view.to_graph(at);
    w.send(Message::Graph(GraphMessage::OpenAddMenu { at, graph_pos }));
    w.input(&[]);
    let kinds = view::add_menu::items(&w.app);
    let count = kinds.len();
    assert!(count > 20, "every kind of node offered: {count}");
    let chosen = |w: &Harness| w.app.ui.add_menu.as_ref().map(|m| m.selected).expect("the menu open");
    let in_sight = |w: &Harness| {
        let menu = w.app.ui.add_menu.as_ref().expect("the menu open");
        view::add_menu::scroll_to_row(menu.selected, menu.scroll, count).is_none()
    };
    press(&mut w, Named::ArrowUp);
    assert_eq!(chosen(&w), 0, "nothing before the first");
    for _ in 0..3 {
        press(&mut w, Named::ArrowDown);
    }
    assert_eq!(chosen(&w), 3);
    press(&mut w, Named::PageDown);
    assert_eq!(chosen(&w), 11, "a page on");
    for _ in 0..(count / 8 + 2) {
        press(&mut w, Named::PageDown);
        assert!(in_sight(&w), "the one chosen in sight at {}", chosen(&w));
    }
    assert_eq!(chosen(&w), count - 1, "nothing after the last");
    press(&mut w, Named::ArrowDown);
    assert_eq!(chosen(&w), count - 1);
    press(&mut w, Named::PageUp);
    assert_eq!(chosen(&w), count - 9);
    assert!(in_sight(&w));
    // Enter, in the menu's field: the one chosen, added.
    let kind = kinds[count - 9];
    let before = w.app.graph().nodes_of(kind).count();
    let _ = w.operate(focusable::focus::<()>(view::add_menu::search_id()));
    let enter = Key::Named(Named::Enter);
    w.input(&[iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: enter.clone(),
        modified_key: enter,
        physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::default(),
        text: None,
        repeat: false,
    })]);
    assert!(w.app.ui.add_menu.is_none(), "Enter closes the menu");
    assert_eq!(w.app.graph().nodes_of(kind).count(), before + 1, "a {} added", kind.def().name);
}
