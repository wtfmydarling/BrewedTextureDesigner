//! Texts are shown whole: in each state of the window, a line of text is laid out as wide as it
//! is drawn (a label cut short, or run into what is beside it, is not), at every size the window
//! is let be.

use iced::Rectangle;
use iced::advanced::widget::Id;

use super::*;
use crate::app::home::{HomeMessage, HomePage, NewGraphMessage};
use crate::app::{AddMenu, ContextMenu, ContextTarget, MenuId, SettingsMessage};

/// The texts of the window, and where each is laid out.
struct Lines(Vec<(String, Rectangle)>);

impl Operation for Lines {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn text(&mut self, _: Option<&Id>, bounds: Rectangle, text: &str) {
        self.0.push((text.to_owned(), bounds));
    }
}

/// The sizes of the interface's text, known by the height of a line of each. (Not the largest,
/// whose line is as high as two of a smaller one: a heading is not cut short.)
const SIZES: [f32; 8] = [11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 18.0, 20.0];

/// The window sizes the tests look at: the smallest the window is let be, a television at 300 %,
/// a laptop, and the size it opens at.
const WINDOWS: [(f32, f32); 4] = [(1000.0, 640.0), (1280.0, 720.0), (1536.0, 864.0), (1680.0, 1000.0)];

/// How wide a line of text is drawn at the least, in any of the interface's fonts (a text says
/// what it is, not which font it is in): the monospaced one only for what is made of numbers
/// (a value, a color, a reading), which are the ones in it.
fn narrowest(text: &str, size: f32) -> f32 {
    use crate::style::{bold, heavy, italic, mono, ui_font};
    let numbers = text.chars().all(|c| c.is_ascii_hexdigit() || "#.,:×x+-% ".contains(c)) || text.contains(" FPS") || text.starts_with("R ");
    let mut fonts = vec![ui_font(), bold(), heavy(), italic()];
    if numbers {
        fonts.push(mono());
    }
    fonts.into_iter().map(|font| view::text_width_in(text, size, font)).fold(f32::INFINITY, f32::min)
}

/// The lines of text the window shows cut short: each with how wide it is drawn, and how wide it
/// is laid out. (Lines that wrap are as wide as they are let be: only lines of one line count,
/// known by their height.)
pub(super) fn cut_short(w: &mut Harness) -> Vec<String> {
    let mut lines = Lines(Vec::new());
    w.input(&[]);
    w.frame(|ui, renderer| ui.operate(renderer, &mut lines));
    // What is cut short at its edge as it is meant to be: the option chosen in a field of a
    // choice, a value driven by a link: their fields cut them at their edge; the menu, the node
    // they come from, say them whole. (Names are shortened in their middle, with "…", to fit.)
    let mut chosen: Vec<String> = Vec::new();
    for node in w.app.ui.selection.iter().filter_map(|id| w.app.graph().node(*id)) {
        for pd in node.def().params {
            chosen.extend(pd.enum_options().iter().map(|o| o.to_string()));
            if let Some(v) = w.app.eval.node(node.id).and_then(|e| e.driven.get(pd.key)) {
                chosen.push(v.as_value().map_or_else(String::new, |x| x.display()));
            }
        }
    }
    let meant = |text: &String| text.starts_with("Set the Usage of Output nodes") || chosen.contains(text);
    lines
        .0
        .into_iter()
        .filter(|(text, _)| !text.trim().is_empty() && !meant(text))
        .filter_map(|(text, bounds)| {
            let size = SIZES.into_iter().find(|s| (bounds.height - s * 1.3).abs() < 0.6)?;
            let wide = narrowest(&text, size);
            (wide > bounds.width + 1.0).then(|| format!("{text:?} at {size} px: {wide:.1} px wide, laid out {:.1} at {:.0},{:.0}", bounds.width, bounds.x, bounds.y))
        })
        .collect()
}

/// The short texts of the window (a name, a button's word or two) that are cut in two lines: each
/// with how wide it is drawn on one, and how wide it is laid out. (Known by being as high as two
/// lines of a size, and wider than its room at that size.)
fn wrapped(w: &mut Harness) -> Vec<String> {
    let mut lines = Lines(Vec::new());
    w.input(&[]);
    w.frame(|ui, renderer| ui.operate(renderer, &mut lines));
    // (The names of the rows of Properties are as wide as their column: a long one goes on two
    // lines there, as it is meant to.)
    let names = view::properties::label_width(view::pane_width(&w.app, crate::app::Pane::Properties));
    // (Nor is the title of the node: a long name of one word goes on in a second line.)
    let titles: Vec<String> = w.app.ui.selection.iter().filter_map(|id| w.app.graph().node(*id)).map(crate::view::graph::node_title).collect();
    lines
        .0
        .into_iter()
        .filter(|(text, bounds)| (bounds.width - names).abs() > 0.5 && !titles.contains(text))
        .filter(|(text, _)| !text.trim().is_empty() && text.split_whitespace().count() <= 3 && text.chars().count() <= 32)
        .filter_map(|(text, bounds)| {
            let size = SIZES.into_iter().find(|s| (bounds.height - 2.0 * s * 1.3).abs() < 0.6 && narrowest(&text, *s) > bounds.width + 1.0)?;
            Some(format!("{text:?} at {size} px in two lines: {:.1} px wide, laid out {:.1} at {:.0},{:.0}", narrowest(&text, size), bounds.width, bounds.x, bounds.y))
        })
        .collect()
}

/// The texts of the window squeezed into a column too narrow for them: four lines high or more
/// in less than 80 pixels, a word or two a line (what a variable of a Math node is stood so,
/// beside its field, ten lines high).
pub(super) fn squeezed(w: &mut Harness) -> Vec<String> {
    let mut lines = Lines(Vec::new());
    w.input(&[]);
    w.frame(|ui, renderer| ui.operate(renderer, &mut lines));
    lines
        .0
        .into_iter()
        .filter(|(text, bounds)| bounds.width < 80.0 && text.split_whitespace().count() >= 4)
        .filter_map(|(text, bounds)| {
            let size = [crate::style::SMALL, crate::style::TEXT_SIZE].into_iter().find(|s| {
                let n = bounds.height / (s * 1.3);
                (n - n.round()).abs() < 0.05 && n.round() >= 4.0
            })?;
            let lines = (bounds.height / (size * 1.3)).round();
            Some(format!("{text:?} at {size} px in {lines} lines {:.1} px wide at {:.0},{:.0}", bounds.width, bounds.x, bounds.y))
        })
        .collect()
}

/// What `state` cuts short that the window did not before, and what it squeezes into a column a
/// word a line.
fn newly_cut(w: &mut Harness, before: &[String], state: &str, found: &mut Vec<String>) {
    for line in cut_short(w).into_iter().chain(squeezed(w)) {
        if !before.contains(&line) {
            found.push(format!("{state}: {line}"));
        }
    }
}

fn first(w: &Harness, kind: NodeKind) -> brewed_texture_core::graph::NodeId {
    w.app.graph().nodes_of(kind).next().unwrap_or_else(|| panic!("a {kind:?}")).id
}

/// The window shows its names and fields whole at every size it is let be: the side columns as
/// wide as their names need, the fields of Properties left their room, the headers of the views
/// in their own room; above the smallest size, no short name is cut in two lines (an input
/// value opened, its buttons too).
#[test]
fn the_window_shows_its_names_whole_at_every_size() {
    let mut found = Vec::new();
    for (width, height) in WINDOWS {
        let mut w = harness!(view::root, width, height);
        let levels = first(&w, NodeKind::LEVELS);
        w.send(Message::Graph(GraphMessage::Select { node: levels, mode: SelectMode::Replace }));
        for line in cut_short(&mut w) {
            found.push(format!("{width}×{height}: {line}"));
        }
        let mortar = w.app.graph().variables.iter().find(|v| v.name == "mortar").map(|v| v.name.clone()).expect("the demo's mortar");
        w.send(Message::ToggleVariable(mortar.clone()));
        for line in cut_short(&mut w) {
            found.push(format!("{width}×{height}, {mortar} open: {line}"));
        }
        if width > 1000.0 {
            for line in wrapped(&mut w) {
                found.push(format!("{width}×{height}, {mortar} open: {line}"));
            }
        }
        w.send(Message::ToggleVariable(mortar));
        // Each column as wide as it is let be at the least (where the window has room; the
        // panels keep a few pixels between them).
        let column = |which| view::pane_width(&w.app, which);
        let least = view::PANE_SPACING + 1.0;
        assert!(column(crate::app::Pane::Explorer) >= crate::app::LEFT_MIN - least, "{width}: the Explorer {}", column(crate::app::Pane::Explorer));
        assert!(column(crate::app::Pane::Properties) >= crate::app::RIGHT_MIN - least, "{width}: Properties {}", column(crate::app::Pane::Properties));
    }
    assert!(found.is_empty(), "cut short:\n{}", found.join("\n"));
}

/// The menus of the menu bar, their submenu, every context menu and the menu that adds a node
/// show their items whole, at the narrowest the window is let be: long labels, keys, how many
/// are selected.
#[test]
fn menus_show_their_items_whole() {
    let mut w = harness!(view::root, 1000.0, 640.0);
    let mut found = Vec::new();
    // Selected, so that what is about a selection says how many.
    let levels = first(&w, NodeKind::LEVELS);
    w.send(Message::SelectAll);
    let before = cut_short(&mut w);
    for (id, name) in [(MenuId::File, "File"), (MenuId::Edit, "Edit"), (MenuId::View, "View"), (MenuId::Help, "Help")] {
        w.send(Message::Menu(Some(id)));
        newly_cut(&mut w, &before, name, &mut found);
    }
    // Open Recent Package ▸, with packages in it.
    w.app.ui.recent = vec![
        "C:/Users/someone/Documents/Textures/A rather long name of a package.brewtex".into(),
        "/home/someone/textures/bricks.brewtex".into(),
    ];
    w.send(Message::Menu(Some(MenuId::File)));
    w.send(Message::SubMenu(Some(crate::app::SubMenu::Recent)));
    newly_cut(&mut w, &before, "File > Open Recent Package", &mut found);
    w.send(Message::Menu(None));
    // Every context menu.
    let key = w.app.doc_key;
    let graph = w.app.doc.current();
    let link = w.app.graph().links.first().cloned().expect("a link");
    let comment = w.app.graph().comments.first().map(|c| c.id).expect("a comment");
    for (target, name) in [
        (ContextTarget::Node(levels), "of a node"),
        (ContextTarget::Link(link), "of a link"),
        (ContextTarget::Comment(comment), "of a comment"),
        (ContextTarget::Package(key), "of a package"),
        (ContextTarget::ResourcesFolder(key), "of the files"),
        (ContextTarget::GraphItem(key, graph), "of a graph"),
        (ContextTarget::ResourceItem(key, "textures/a picture with a long name.png".into()), "of a file"),
        (ContextTarget::Canvas { graph_pos: [0.0, 0.0] }, "of the canvas"),
    ] {
        w.send(Message::Context(Some(ContextMenu { at: Point::new(400.0, 200.0), target })));
        newly_cut(&mut w, &before, &format!("the context menu {name}"), &mut found);
        w.send(Message::Context(None));
    }
    // The menu that adds a node, as it opens and searched.
    for query in ["", "bl", "hdri"] {
        w.app.ui.add_menu = Some(AddMenu { at: Point::new(500.0, 200.0), graph_pos: [0.0, 0.0], query: query.into(), selected: 0, scroll: 0.0, pending: None });
        newly_cut(&mut w, &before, &format!("the add menu, {query:?}"), &mut found);
    }
    w.app.ui.add_menu = None;
    assert!(found.is_empty(), "cut short:\n{}", found.join("\n"));
}

/// The properties of every kind of node show whole in the narrowest Properties the window is let
/// have: their names, their lists, their fields.
#[test]
fn the_properties_of_every_node_show_whole() {
    let mut w = harness!(view::root, 1000.0, 640.0);
    let mut found = Vec::new();
    w.send(Message::Graph(GraphMessage::ClearSelection));
    let before = cut_short(&mut w);
    for &kind in NodeKind::ALL {
        w.send(Message::Edit(Edit::AddNode { kind, pos: [2600.0, -900.0] }));
        w.send(Message::Seal);
        let Some(id) = w.app.graph().nodes_of(kind).last().map(|n| n.id) else { continue };
        w.send(Message::Graph(GraphMessage::Select { node: id, mode: SelectMode::Replace }));
        newly_cut(&mut w, &before, &format!("{kind:?}"), &mut found);
        w.send(Message::Delete);
    }
    // Where Properties has a little more room (a television at 300 %), no short name of them is
    // cut in two lines.
    let mut w = harness!(view::root, 1280.0, 720.0);
    w.send(Message::Graph(GraphMessage::ClearSelection));
    let before = wrapped(&mut w);
    for &kind in NodeKind::ALL {
        w.send(Message::Edit(Edit::AddNode { kind, pos: [2600.0, -900.0] }));
        w.send(Message::Seal);
        let Some(id) = w.app.graph().nodes_of(kind).last().map(|n| n.id) else { continue };
        w.send(Message::Graph(GraphMessage::Select { node: id, mode: SelectMode::Replace }));
        for line in wrapped(&mut w) {
            if !before.contains(&line) {
                found.push(format!("{kind:?} at 1280×720: {line}"));
            }
        }
        w.send(Message::Delete);
    }
    assert!(found.is_empty(), "cut short:\n{}", found.join("\n"));
}

/// Every dialog shows whole at the narrowest the window is let be: the settings, each of their
/// pages, a new graph, the export, the shortcuts, about the program, the notices, a question
/// about unsaved changes.
#[test]
fn dialogs_show_whole_in_the_smallest_window() {
    let mut w = harness!(view::root, 1000.0, 640.0);
    let mut found = Vec::new();
    let before = cut_short(&mut w);
    w.send(Message::Settings(SettingsMessage::Open));
    for page in crate::settings::Page::ALL {
        w.send(Message::Settings(SettingsMessage::OpenPage(page)));
        newly_cut(&mut w, &before, &format!("Settings, {}", page.name()), &mut found);
    }
    w.send(Message::Settings(SettingsMessage::Cancel));
    w.send(Message::NewGraph(NewGraphMessage::Open(None)));
    newly_cut(&mut w, &before, "New Graph", &mut found);
    w.send(Message::Dialog(None));
    w.send(Message::Export(crate::app::export_dialog::ExportMessage::Open));
    newly_cut(&mut w, &before, "Export", &mut found);
    w.send(Message::Dialog(None));
    for (dialog, name) in [
        (Dialog::Shortcuts, "Shortcuts"),
        (Dialog::About, "About"),
        (Dialog::Notices(Default::default()), "Third-Party Notices"),
        (Dialog::Unsaved(PendingAction::Quit), "Unsaved changes"),
    ] {
        w.send(Message::Dialog(Some(dialog)));
        newly_cut(&mut w, &before, name, &mut found);
        w.send(Message::Dialog(None));
    }
    assert!(found.is_empty(), "cut short:\n{}", found.join("\n"));
}

/// The Home screen and Learn show whole at every size.
#[test]
fn the_home_screen_shows_whole() {
    let mut found = Vec::new();
    for (width, height) in WINDOWS {
        let opts = StartupOptions { home: true, ..StartupOptions::default() };
        let Some(mut w) = Harness::started(Box::new(view::root), Size::new(width, height), opts) else { return };
        for page in [HomePage::Home, HomePage::Learn] {
            w.send(Message::Home(HomeMessage::Show(page)));
            for line in cut_short(&mut w) {
                found.push(format!("{width}×{height}, {page:?}: {line}"));
            }
        }
    }
    assert!(found.is_empty(), "cut short:\n{}", found.join("\n"));
}

/// A context menu opens where it is asked to, its corner at the cursor, as long as the window has
/// room for it there; near an edge, it is moved only as far as it has to be, and stays whole.
#[test]
fn context_menus_open_at_the_cursor_where_they_fit() {
    let (width, height) = (1280.0, 800.0);
    let mut w = harness!(view::root, width, height);
    let levels = first(&w, NodeKind::LEVELS);
    let menu_at = |w: &mut Harness, at: Point| {
        w.send(Message::Context(Some(ContextMenu { at, target: ContextTarget::Node(levels) })));
        w.input(&[]);
        let first = *texts(w, "Show in 2D View").first().expect("the first item");
        let last = *texts(w, "Delete").last().expect("the last item");
        w.send(Message::Context(None));
        (first, last)
    };
    // Where there is room: its first item just inside its corner.
    let (first, last) = menu_at(&mut w, Point::new(300.0, 200.0));
    assert!((first.x - 300.0).abs() < 20.0 && (first.y - 200.0).abs() < 20.0, "{first:?}");
    let tall = last.y + last.height - first.y;
    // Just enough room below: where it is asked to be still.
    let at = Point::new(300.0, height - tall - 4.0 - 4.0 - 8.0);
    let (first, _) = menu_at(&mut w, at);
    assert!((first.y - at.y).abs() < 20.0, "with room enough below, at the cursor: {first:?} for {at:?}");
    // No room: moved up only as far as it must, the last item whole, just inside the window.
    let (_, last) = menu_at(&mut w, Point::new(300.0, height - 10.0));
    let bottom = last.y + last.height;
    assert!(bottom <= height && bottom > height - 30.0, "kept inside, and as low as it can be: {last:?}");
    // At the right edge too.
    let (first, _) = menu_at(&mut w, Point::new(width - 5.0, 200.0));
    assert!(first.x < width - 100.0, "moved in from the right edge: {first:?}");
}

/// Every template and every example of Learn, made or opened in the window: their graphs say no
/// errors, and the properties of each of their nodes show whole, their short names on one line
/// (on a television at 300 %).
#[test]
fn templates_and_examples_show_whole() {
    use brewed_texture_core::templates::{Example, Template};
    let mut found = Vec::new();
    let mut w = harness!(view::root, 1280.0, 720.0);
    let made: Vec<(String, Message)> = Template::ALL
        .into_iter()
        .map(|t| (format!("{t:?}"), Message::NewGraph(NewGraphMessage::Template(t))))
        .chain(Example::ALL.into_iter().map(|e| (format!("{e:?}"), Message::Home(HomeMessage::Example(e)))))
        .collect();
    for (name, message) in made {
        if matches!(message, Message::NewGraph(_)) {
            w.send(Message::NewGraph(NewGraphMessage::Open(None)));
            w.send(message);
            w.send(Message::NewGraph(NewGraphMessage::Create));
        } else {
            w.send(message);
        }
        w.send(Message::Home(HomeMessage::Show(HomePage::Home)));
        w.app.ui.home = None;
        w.send(Message::Graph(GraphMessage::ClearSelection));
        let before = (cut_short(&mut w), wrapped(&mut w));
        let ids: Vec<_> = w.app.graph().nodes.iter().map(|n| n.id).collect();
        for id in ids {
            if let Some(e) = w.app.eval.node(id).filter(|e| !e.errors.is_empty()) {
                found.push(format!("{name}: node {} says {:?}", id.0, e.errors));
            }
            w.send(Message::Graph(GraphMessage::Select { node: id, mode: SelectMode::Replace }));
            let kind = w.app.graph().node(id).map(|n| n.def().name).unwrap_or_default();
            for line in cut_short(&mut w).into_iter().filter(|l| !before.0.contains(l)) {
                found.push(format!("{name}, {kind}: {line}"));
            }
            for line in wrapped(&mut w).into_iter().filter(|l| !before.1.contains(l)) {
                found.push(format!("{name}, {kind}: {line}"));
            }
        }
    }
    found.dedup();
    assert!(found.is_empty(), "{}", found.join("\n"));
}

/// The headers of the 3D and 2D views show their words whole however the split between the
/// views is dragged: narrower, they put their fields or channels on a line under the others.
#[test]
fn the_headers_of_the_views_fit_wherever_they_are_split() {
    let mut found = Vec::new();
    let mut w = harness!(view::root, 1280.0, 720.0);
    let levels = first(&w, NodeKind::LEVELS);
    w.send(Message::Graph(GraphMessage::Preview(levels)));
    for i in 0..=40 {
        let split = 0.02 + 0.96 * i as f32 / 40.0;
        let mut splits = crate::app::splits_of(&w.app.ui.panes);
        splits[4] = split;
        w.app.ui.panes = crate::app::panes(&splits);
        w.app.ui.side_splits = [splits[0], splits[2]];
        w.app.fit_columns();
        let (a, b) = (view::pane_width(&w.app, crate::app::Pane::View3D), view::pane_width(&w.app, crate::app::Pane::View2D));
        for line in cut_short(&mut w) {
            found.push(format!("3D {a:.0} px, 2D {b:.0} px: {line}"));
        }
    }
    found.dedup();
    assert!(found.is_empty(), "cut short:\n{}", found.join("\n"));
}

/// The side columns are fitted to every width the window may have, whatever their splits say
/// (dragged as far as they go), and none of it panics: at the narrowest the bounds meet.
#[test]
fn the_columns_fit_any_window_and_any_split() {
    let mut w = harness!(view::root, 1000.0, 640.0);
    for width in [880.0, 880.5, 999.0, 1000.0, 1280.0, 1680.0, 3840.0] {
        for (left, right) in [(0.0, 0.0), (0.99, 0.99), (0.99, 0.01), (0.01, 0.99), (0.5, 0.5)] {
            w.app.ui.window_size = Size::new(width, 900.0);
            w.app.ui.side_splits = [left, right];
            w.app.fit_columns();
            let column = |which| view::pane_width(&w.app, which);
            let least = view::PANE_SPACING + 1.0;
            assert!(column(crate::app::Pane::Explorer) >= crate::app::LEFT_MIN - least, "{width} {left} {right}: the Explorer {}", column(crate::app::Pane::Explorer));
            assert!(column(crate::app::Pane::Properties) >= crate::app::RIGHT_MIN - least, "{width} {left} {right}: Properties {}", column(crate::app::Pane::Properties));
        }
    }
}
