//! Headless UI tests: the real views are built and driven with input events the way the
//! window does it, keeping the widget state between frames.
//!
//! With `BREWED_UI_SNAPSHOTS=<folder>` the tests also save what they draw as PNG files.

use iced::advanced::clipboard;
use iced::advanced::renderer::{self, Headless};
use iced::advanced::widget::operation::{self, Operation, Outcome, focusable};
use iced::keyboard::{self, Key};
use iced::{Color, Element, Pixels, Point, Size, mouse, window};
use iced_runtime::user_interface::{Cache, UserInterface};
use brewed_texture_core::{Edit, NodeKind, ParamValue};

use crate::app::{
    App, ColorTarget, ContextMenu, ContextTarget, Dialog, DragPayload, ExplorerMessage, GraphMessage, Message, PendingAction, PreviewMessage,
    ScreenPick, SelectMode, StartupOptions,
};
use crate::picker::{PickTarget, Shot};
use crate::view;
use crate::view::properties::expression::editor_id;

type View = Box<dyn for<'a> Fn(&'a App) -> Element<'a, Message>>;

/// A view of an app, rendered without a window.
struct Harness {
    app: App,
    view: View,
    renderer: iced::Renderer,
    cache: Cache,
    size: Size,
    cursor: mouse::Cursor,
}

impl Harness {
    /// `None` when no GPU is available for the headless renderer.
    fn new(view: View, size: Size) -> Option<Harness> {
        let renderer = futures::executor::block_on(<iced::Renderer as Headless>::new(crate::style::ui_font(), Pixels(13.0), None))?;
        let (mut app, _) = App::boot(StartupOptions::default());
        app.ui.window_size = size;
        Some(Harness { app, view, renderer, cache: Cache::default(), size, cursor: mouse::Cursor::Unavailable })
    }

    fn send(&mut self, message: Message) {
        let _ = self.app.update(message);
    }

    /// Build the view from the current state and run `f` on it; the widget state is kept
    /// for the next frame, like the window's event loop does.
    fn frame<T>(&mut self, f: impl FnOnce(&mut UserInterface<'_, Message, iced::Theme, iced::Renderer>, &mut iced::Renderer) -> T) -> T {
        let cache = std::mem::take(&mut self.cache);
        let mut ui = UserInterface::build((self.view)(&self.app), self.size, cache, &mut self.renderer);
        let out = f(&mut ui, &mut self.renderer);
        self.cache = ui.into_cache();
        out
    }

    /// Feed input events; returns how many messages the widgets produced (they are
    /// applied to the app).
    fn input(&mut self, events: &[iced::Event]) -> usize {
        let mut messages = Vec::new();
        let cursor = self.cursor;
        self.frame(|ui, renderer| {
            let _ = ui.update(events, cursor, renderer, &mut clipboard::Null, &mut messages);
        });
        let count = messages.len();
        for m in messages {
            self.send(m);
        }
        count
    }

    /// Move the mouse (the widgets see a CursorMoved event).
    fn hover(&mut self, to: Point) -> usize {
        self.cursor = mouse::Cursor::Available(to);
        self.input(&[iced::Event::Mouse(mouse::Event::CursorMoved { position: to })])
    }

    fn mouse(&mut self, event: mouse::Event) -> usize {
        self.input(&[iced::Event::Mouse(event)])
    }

    fn operate<T>(&mut self, mut operation: impl Operation<T>) -> Outcome<T> {
        self.frame(|ui, renderer| ui.operate(renderer, &mut operation::black_box::<T, ()>(&mut operation)));
        operation.finish()
    }

    fn focused(&mut self) -> Option<iced::widget::Id> {
        match self.operate(focusable::find_focused()) {
            Outcome::Some(id) => Some(id),
            _ => None,
        }
    }

    /// What the window's event subscription reports.
    fn window_event(&mut self, window: window::Id, event: iced::Event) {
        self.send(Message::Runtime(event, iced::event::Status::Ignored, window));
    }

    fn main_window(&self) -> window::Id {
        self.app.main_window().expect("main window")
    }

    /// Save what the view draws, when snapshots are asked for.
    fn snapshot(&mut self, name: &str) {
        let Some(dir) = std::env::var_os("BREWED_UI_SNAPSHOTS") else { return };
        let (cursor, size) = (self.cursor, self.size);
        let theme = crate::style::theme();
        let rgba = self.frame(|ui, renderer| {
            // Widgets take their look from the redraw event the window sends before drawing.
            let redraw = iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()));
            let _ = ui.update(&[redraw], cursor, renderer, &mut clipboard::Null, &mut Vec::new());
            ui.draw(renderer, &theme, &renderer::Style { text_color: Color::WHITE }, cursor);
            Headless::screenshot(renderer, iced::Size::new(size.width as u32, size.height as u32), 1.0, crate::style::CANVAS)
        });
        let path = std::path::Path::new(&dir).join(format!("{name}.png"));
        image::RgbaImage::from_raw(size.width as u32, size.height as u32, rgba).expect("snapshot size").save(&path).expect("save snapshot");
    }
}

/// Key presses and releases typing `text`.
fn typing(text: &str) -> Vec<iced::Event> {
    let physical = keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified);
    text.chars()
        .flat_map(|c| {
            let key = Key::Character(c.to_string().into());
            [
                iced::Event::Keyboard(keyboard::Event::KeyPressed {
                    key: key.clone(),
                    modified_key: key.clone(),
                    physical_key: physical,
                    location: keyboard::Location::Standard,
                    modifiers: keyboard::Modifiers::default(),
                    text: Some(c.to_string().into()),
                    repeat: false,
                }),
                iced::Event::Keyboard(keyboard::Event::KeyReleased {
                    key: key.clone(),
                    modified_key: key,
                    physical_key: physical,
                    location: keyboard::Location::Standard,
                    modifiers: keyboard::Modifiers::default(),
                }),
            ]
        })
        .collect()
}

fn escape() -> iced::Event {
    let key = Key::Named(keyboard::key::Named::Escape);
    iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::default(),
        text: None,
        repeat: false,
    })
}

macro_rules! harness {
    ($view:expr, $w:expr, $h:expr) => {
        match Harness::new(Box::new($view), Size::new($w, $h)) {
            Some(h) => h,
            None => {
                eprintln!("no GPU for the headless renderer: test skipped");
                return;
            }
        }
    };
}

/// Typing into the Math expression must never lose the focus, however often the
/// expression turns incorrect and correct again (the error line appears and goes).
#[test]
fn expression_editor_keeps_focus_while_typing() {
    let mut panel = harness!(view::properties::view, 420.0, 1400.0);
    let math = panel.app.graph().nodes_of(NodeKind::MATH).find(|n| n.text("expression") == "1 - 2*mortar").expect("demo Math node").id;
    panel.send(Message::Graph(GraphMessage::Select { node: math, mode: SelectMode::Replace }));
    panel.input(&[]);
    assert!(matches!(panel.operate(focusable::focus::<()>(editor_id())), Outcome::None));
    assert_eq!(panel.focused(), Some(editor_id()), "the editor takes the focus");

    let error = |panel: &Harness| panel.app.eval.node(math).is_some_and(|e| e.error().is_some());
    for (typed, incorrect) in [("+", true), ("1", false), (" *(", true), ("2", true), (")", false), ("/", true), ("4", false)] {
        let produced = panel.input(&typing(typed));
        assert!(produced > 0, "typing {typed:?} reached no widget");
        assert_eq!(error(&panel), incorrect, "after typing {typed:?}");
        assert_eq!(panel.focused(), Some(editor_id()), "the editor lost the focus after typing {typed:?}");
    }
    assert_eq!(panel.app.graph().node(math).unwrap().text("expression"), "1 - 2*mortar+1 *(2)/4");

    // Undo restores the text without taking the focus away.
    panel.send(Message::Undo);
    panel.input(&[]);
    assert_eq!(panel.focused(), Some(editor_id()), "the editor lost the focus on undo");
}

/// Pressing a library item, moving onto the graph and releasing adds the node there.
#[test]
fn library_items_can_be_dropped_on_the_graph() {
    let mut window = harness!(view::root, 1680.0, 1000.0);
    // The canvas reports where it is on its first event.
    window.input(&[iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))]);
    let bounds = window.app.ui.graph_view.bounds;
    assert!(bounds.width > 300.0 && bounds.height > 200.0, "graph bounds {bounds:?}");

    let main = window.main_window();
    let before = window.app.graph().nodes.len();
    window.window_event(main, iced::Event::Mouse(mouse::Event::CursorMoved { position: Point::new(60.0, 200.0) }));
    window.send(Message::PanelPress(DragPayload::Node(NodeKind::BLUR)));
    assert!(window.app.ui.drag.is_none(), "a press alone is not a drag");
    let target = Point::new(bounds.x + 40.0, bounds.y + bounds.height - 40.0);
    window.window_event(main, iced::Event::Mouse(mouse::Event::CursorMoved { position: target }));
    assert_eq!(window.app.ui.drag, Some(DragPayload::Node(NodeKind::BLUR)));

    window.hover(target);
    window.mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    assert_eq!(window.app.graph().nodes.len(), before + 1, "the drop added a node");
    let added = window.app.graph().nodes.iter().max_by_key(|n| n.id.0).unwrap();
    assert_eq!(added.kind, NodeKind::BLUR);
    assert!(window.app.ui.selection.contains(&added.id), "the new node is selected");
    let at = window.app.ui.graph_view.to_graph(target);
    assert!((added.pos[0] - (at[0] - 80.0)).abs() < 1.0 && (added.pos[1] - (at[1] - 12.0)).abs() < 1.0, "placed under the cursor");
    assert!(window.app.ui.drag.is_none() && window.app.ui.press.is_none());
}

/// Closing the window with unsaved changes asks first; Cancel keeps everything.
#[test]
fn closing_with_unsaved_changes_asks_first() {
    let (mut app, _) = App::boot(StartupOptions::default());
    let main = app.main_window().unwrap();
    let close = || Message::Runtime(iced::Event::Window(window::Event::CloseRequested), iced::event::Status::Ignored, main);
    let _ = app.update(Message::Edit(Edit::SetResolution([512, 512])));
    assert!(app.doc.is_dirty());
    let _ = app.update(close());
    assert_eq!(app.ui.dialog, Some(Dialog::Unsaved(PendingAction::Quit)));
    let _ = app.update(Message::Dialog(None));
    assert_eq!(app.ui.dialog, None);
    assert!(app.doc.is_dirty(), "cancelling keeps the changes");
    assert_eq!(app.graph().resolution, [512, 512]);
}

/// The color of a stop of a gradient: the stop chosen on the ramp, its swatch opens the
/// picker, and the picker colors that stop.
#[test]
fn a_stop_of_a_gradient_takes_a_color_from_the_picker() {
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let window = w.app.main_window().expect("the main window");
    let map = w.app.graph().nodes_of(NodeKind::GRADIENT_MAP).next().expect("the demo colors by a gradient").id;
    w.send(Message::Graph(GraphMessage::Select { node: map, mode: SelectMode::Replace }));
    w.send(Message::GradientStop(map, "gradient".into(), 1));
    w.input(&[iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))]);
    let stop = |w: &Harness| gradient_of(&w.app, map).stops[1].color;
    let hex = crate::widget::color::to_hex(stop(&w));
    let swatch = *texts(&mut w, &hex).first().expect("the swatch of the stop");
    // (The window tells the app where the cursor is: the picker opens there.)
    w.window_event(window, iced::Event::Mouse(mouse::Event::CursorMoved { position: swatch.center() }));
    click(&mut w, swatch.center());
    assert_eq!(w.app.ui.color_picker, Some(ColorTarget::GradientStop(map, "gradient".into())));
    w.snapshot("gradient_stop_picker");
    // Dragging in the saturation square of the picker colors the stop.
    let before = stop(&w);
    let square = w.app.ui.color_picker_at + iced::Vector::new(118.0, 70.0);
    drag(&mut w, square, square + iced::Vector::new(-40.0, 20.0));
    assert_ne!(stop(&w), before);
    assert_eq!(gradient_of(&w.app, map).stops.len(), 4, "no stop added or taken away");
}

/// Nodes that use graphs of a package that has no file are pasted in that package, where their
/// graphs are; another package could not find them, and says so rather than take graphs of its
/// own for them. (The clipboard of the program, not the system's.)
#[test]
fn nodes_that_use_graphs_are_pasted_where_their_graphs_are() {
    let (mut app, _) = App::boot(StartupOptions::default());
    let nodes = |app: &App| app.graph().nodes.len();
    let blur = app.graph().nodes_of(NodeKind::BLUR).next().expect("the demo blurs").id;
    app.ui.selection = [blur].into_iter().collect();
    let _ = app.update(Message::GraphFromSelection);
    let used = *app.ui.selection.iter().next().expect("the node that uses the new graph");
    assert!(app.graph().node(used).is_some_and(|n| n.instance.is_some()));
    let _ = app.update(Message::Copy);
    let before = nodes(&app);
    let _ = app.update(Message::ClipboardText(None));
    assert_eq!(nodes(&app), before + 1, "pasted in its package");
    let _ = app.update(Message::New);
    let _ = app.update(Message::ClipboardText(None));
    assert!(app.graph().nodes.is_empty(), "not pasted in another");
    let status = app.ui.status.as_ref().expect("a status");
    assert!(status.error && status.text.contains("no file"), "{}", status.text);
}

// ---------------------------------------------------------------------------- eyedropper

const SCREEN: [u32; 2] = [800, 240];

/// A fake capture of a monitor: a red → green → blue ramp across, darker at the bottom.
fn fake_screen() -> Shot {
    let [w, h] = SCREEN;
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let shade = 1.0 - 0.5 * y as f32 / h as f32;
        for x in 0..w {
            let t = x as f32 / (w - 1) as f32;
            let (r, g, b) = if t < 0.5 { (1.0 - t * 2.0, t * 2.0, 0.0) } else { (0.0, 2.0 - t * 2.0, t * 2.0 - 1.0) };
            let q = |v: f32| (v * shade * 255.0).round() as u8;
            rgba.extend([q(r), q(g), q(b), 255]);
        }
    }
    Shot::from_image(SCREEN, rgba)
}

/// A picker window showing the fake screen, for the given target.
fn picker_window(target: PickTarget) -> Option<(Harness, window::Id)> {
    let overlay = window::Id::unique();
    let mut h = Harness::new(Box::new(move |app: &App| view::picker::view(app, overlay)), Size::new(SCREEN[0] as f32, SCREEN[1] as f32))?;
    h.app.ui.screen_pick = Some(ScreenPick { live: false, shots: vec![fake_screen()], windows: vec![overlay], ..ScreenPick::new(target) });
    Some((h, overlay))
}

fn gradient_of(app: &App, node: brewed_texture_core::graph::NodeId) -> brewed_texture_core::Gradient {
    app.graph().node(node).and_then(|n| n.value("gradient")).and_then(ParamValue::as_gradient).cloned().expect("gradient")
}

/// Dragging a line on a picker window turns the colors along it into the gradient; the
/// windows then close and the pick is over.
#[test]
fn a_gradient_is_picked_along_a_line_on_the_screen() {
    let (graph, _) = brewed_texture_core::demo::graph();
    let gm = graph.nodes_of(NodeKind::GRADIENT_MAP).next().unwrap().id;
    let Some((mut w, overlay)) = picker_window(PickTarget::Gradient(gm, "gradient".into())) else {
        eprintln!("no GPU for the headless renderer: test skipped");
        return;
    };
    let y = 20.0;
    w.hover(Point::new(0.0, y));
    w.mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    w.hover(Point::new(400.0, y));
    w.snapshot("eyedropper_gradient_dragging");
    assert!(!w.app.ui.screen_pick.as_ref().unwrap().done, "still dragging");
    w.hover(Point::new(799.0, y));
    w.mouse(mouse::Event::ButtonReleased(mouse::Button::Left));

    let pick = w.app.ui.screen_pick.as_ref().expect("windows still closing");
    assert!(pick.done);
    let g = gradient_of(&w.app, gm);
    assert!(g.stops.len() <= 6, "few stops: {}", g.stops.len());
    let shade = 1.0 - 0.5 * y / SCREEN[1] as f32;
    for (t, expected) in [(0.0, [1.0, 0.0, 0.0]), (0.5, [0.0, 1.0, 0.0]), (1.0, [0.0, 0.0, 1.0])] {
        let c = g.sample(t);
        for i in 0..3 {
            assert!((c[i] - expected[i] * shade).abs() < 0.05, "at {t}: {c:?}");
        }
    }
    assert!(w.app.doc.can_undo());
    // The window closes; with it the pick is gone.
    w.window_event(overlay, iced::Event::Window(window::Event::Closed));
    assert!(w.app.ui.screen_pick.is_none());
}

/// A click picks the color under the cursor; the parameter keeps its alpha.
#[test]
fn a_color_is_picked_with_a_click_and_keeps_its_alpha() {
    // A new node (the demo's Uniform Color is driven by an input, so its color is locked).
    let (mut graph, _) = brewed_texture_core::demo::graph();
    let uniform = graph.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
    let target = PickTarget::Color(ColorTarget::Param(uniform, "color".into()));
    let Some((mut w, _)) = picker_window(target) else {
        eprintln!("no GPU for the headless renderer: test skipped");
        return;
    };
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::UNIFORM_COLOR, pos: [0.0, 0.0] }));
    assert!(w.app.graph().contains(uniform), "same id as in the copy of the demo");
    w.send(Message::Edit(Edit::SetParam { node: uniform, key: "color".into(), value: ParamValue::Color([0.1, 0.1, 0.1, 0.5]) }));
    w.hover(Point::new(600.0, 120.0));
    w.snapshot("eyedropper_color_loupe");
    w.mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    assert!(w.app.graph().node(uniform).is_some_and(|n| n.value("color") == Some(&ParamValue::Color([0.1, 0.1, 0.1, 0.5]))), "picked on release");
    w.mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    let expected = fake_screen().pixel(600, 120);
    let got = match w.app.graph().node(uniform).and_then(|n| n.value("color")) {
        Some(ParamValue::Color(c)) => *c,
        other => panic!("{other:?}"),
    };
    assert_eq!(&got[..3], &expected[..3]);
    assert_eq!(got[3], 0.5, "alpha kept");
}

/// What the eyedropper brings back goes where it was started from, and the app never
/// stays stuck on a pick: a second click changes nothing, cancelling ends it, and a
/// failure is told in the status bar.
#[test]
fn a_pick_ends_up_where_the_eyedropper_was_clicked() {
    use crate::app::PickMessage;
    use crate::eyedropper::Outcome as Picked;

    let mut w = harness!(|app: &App| view::root(app), 1680.0, 1000.0);
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::UNIFORM_COLOR, pos: [0.0, 0.0] }));
    let uniform = w.app.ui.selection.first().copied().expect("the new node is selected");
    w.send(Message::Edit(Edit::SetParam { node: uniform, key: "color".into(), value: ParamValue::Color([0.1, 0.1, 0.1, 0.5]) }));
    let color_target = PickTarget::Color(ColorTarget::Param(uniform, "color".into()));

    // The button starts a pick, and a second click on another one changes nothing.
    w.send(Message::Pick(PickMessage::Start(color_target.clone())));
    assert_eq!(w.app.picking(), Some(&color_target));
    let gradient_map = w.app.graph().nodes_of(NodeKind::GRADIENT_MAP).next().map(|n| n.id).expect("a Gradient Map");
    let gradient_target = PickTarget::Gradient(gradient_map, "gradient".into());
    w.send(Message::Pick(PickMessage::Start(gradient_target.clone())));
    assert_eq!(w.app.picking(), Some(&color_target), "one pick at a time");

    // A color keeps the alpha of the parameter it lands in.
    w.send(Message::Pick(PickMessage::Picked(Picked::Color([0.2, 0.4, 0.6, 1.0]))));
    assert_eq!(w.app.picking(), None);
    let color = match w.app.graph().node(uniform).and_then(|n| n.value("color")) {
        Some(ParamValue::Color(c)) => *c,
        other => panic!("{other:?}"),
    };
    assert_eq!(color, [0.2, 0.4, 0.6, 0.5]);

    // The colors along a path become the gradient.
    w.send(Message::Pick(PickMessage::Start(gradient_target.clone())));
    let path: Vec<[f32; 4]> = (0..64).map(|i| [i as f32 / 63.0, 0.0, 0.0, 1.0]).collect();
    w.send(Message::Pick(PickMessage::Picked(Picked::Path(path))));
    assert_eq!(w.app.picking(), None);
    let gradient = gradient_of(&w.app, gradient_map);
    assert!(gradient.stops.len() <= 4, "{} stops", gradient.stops.len());
    assert!(gradient.sample(0.0)[0] < 0.05 && gradient.sample(1.0)[0] > 0.95, "{gradient:?}");

    // Cancelling ends the pick at once, and what comes late is ignored.
    w.send(Message::Pick(PickMessage::Start(color_target.clone())));
    w.send(Message::Pick(PickMessage::Cancel));
    assert_eq!(w.app.picking(), None);
    w.send(Message::Pick(PickMessage::Picked(Picked::Color([1.0, 1.0, 1.0, 1.0]))));
    assert_eq!(w.app.graph().node(uniform).and_then(|n| n.value("color")), Some(&ParamValue::Color([0.2, 0.4, 0.6, 0.5])));

    // A pick that cannot be done says why.
    w.send(Message::Pick(PickMessage::Start(color_target)));
    w.send(Message::Pick(PickMessage::Picked(Picked::Failed("no screen here".into()))));
    assert_eq!(w.app.picking(), None);
    let status = w.app.ui.status.as_ref().expect("a status");
    assert!(status.error && status.text.contains("no screen here"), "{status:?}");
}

/// Esc (or a right click) ends the pick without changing anything.
#[test]
fn escape_cancels_a_pick() {
    let (graph, _) = brewed_texture_core::demo::graph();
    let gm = graph.nodes_of(NodeKind::GRADIENT_MAP).next().unwrap().id;
    let Some((mut w, overlay)) = picker_window(PickTarget::Gradient(gm, "gradient".into())) else {
        eprintln!("no GPU for the headless renderer: test skipped");
        return;
    };
    let before = gradient_of(&w.app, gm);
    w.hover(Point::new(10.0, 10.0));
    w.mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    w.window_event(overlay, escape());
    assert!(w.app.ui.screen_pick.as_ref().is_some_and(|p| p.done));
    // The release that ends the drag changes nothing either.
    w.hover(Point::new(700.0, 10.0));
    w.mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    assert_eq!(gradient_of(&w.app, gm), before);
    assert!(!w.app.doc.can_undo());
}

// ---------------------------------------------------------------------------- whole window

/// Thumbnails as the engine would send them: a pattern per node.
fn fake_thumbnails(app: &mut App) {
    let ids: Vec<_> = app.graph().nodes.iter().filter(|n| n.def().has_image()).map(|n| n.id).collect();
    for (k, id) in ids.into_iter().enumerate() {
        let size = 64u32;
        let mut rgba = Vec::with_capacity((size * size * 4) as usize);
        for y in 0..size {
            for x in 0..size {
                let v = (((x / 8 + y / 8 + k as u32) % 2) * 120 + 60) as u8;
                rgba.extend([v, (v as u32 * 3 / 4) as u8, (v as u32 / 2) as u8, 255]);
            }
        }
        app.res.thumbs.insert(id, iced::widget::image::Handle::from_rgba(size, size, rgba));
    }
}

/// The whole window renders (and, with snapshots on, is saved for a look at the design).
/// A node dragged over another stays on top of the other's thumbnail.
#[test]
fn the_window_renders() {
    let mut w = harness!(view::root, 1680.0, 1000.0);
    fake_thumbnails(&mut w.app);
    let gm = w.app.graph().nodes_of(NodeKind::GRADIENT_MAP).next().unwrap().id;
    w.send(Message::Graph(GraphMessage::Select { node: gm, mode: SelectMode::Replace }));
    // The graph canvas reports its bounds and frames the graph.
    w.input(&[iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))]);
    w.input(&[iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))]);
    w.snapshot("window");
    // The graph at 100 %, around the Tile Generator.
    let tile = w.app.graph().nodes_of(NodeKind::TILE_GENERATOR).next().unwrap().pos;
    let bounds = w.app.ui.graph_view.bounds;
    let pan = iced::Vector::new(bounds.width * 0.3 - tile[0], bounds.height * 0.25 - tile[1]);
    w.app.ui.graph_view.pan = pan;
    w.app.ui.graph_view.zoom = 1.0;
    w.input(&[]);
    w.snapshot("window_graph_100");
    w.send(Message::Frame);

    // Put the Levels node over the Blur node's thumbnail.
    let levels = w.app.graph().nodes_of(NodeKind::LEVELS).next().unwrap();
    let blur = w.app.graph().nodes_of(NodeKind::BLUR).next().unwrap();
    let delta = [blur.pos[0] - levels.pos[0] + 30.0, blur.pos[1] - levels.pos[1] + 40.0];
    let id = levels.id;
    w.send(Message::Graph(GraphMessage::Move { ids: vec![id], delta }));
    w.send(Message::Graph(GraphMessage::Select { node: id, mode: SelectMode::Replace }));
    w.input(&[]);
    w.snapshot("window_overlap");
}

/// The main states of the window render; with snapshots on, each is saved for review.
#[test]
fn window_states_render() {
    use crate::app::{AddMenu, ContextMenu, ContextTarget, MenuId, VarField};
    let mut w = harness!(view::root, 1680.0, 1000.0);
    fake_thumbnails(&mut w.app);
    w.input(&[iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))]);
    w.input(&[iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))]);
    let first = |w: &Harness, kind: NodeKind| w.app.graph().nodes_of(kind).next().unwrap().id;
    let select = |w: &mut Harness, id| w.send(Message::Graph(GraphMessage::Select { node: id, mode: SelectMode::Replace }));

    let math = first(&w, NodeKind::MATH);
    select(&mut w, math);
    w.snapshot("state_math");
    let levels = first(&w, NodeKind::LEVELS);
    select(&mut w, levels);
    w.snapshot("state_levels");
    // A Transform 2D of its own (the demo has none: nothing in it may break the tiling).
    let noise = first(&w, NodeKind::NOISE);
    w.send(Message::Edit(Edit::AddNodeConnected {
        kind: NodeKind::TRANSFORM_2D,
        pos: [160.0, 520.0],
        socket: brewed_texture_core::graph::SocketRef { node: noise, key: "o:output".into(), output: true },
    }));
    let transform = first(&w, NodeKind::TRANSFORM_2D);
    select(&mut w, transform);
    w.snapshot("state_transform");
    w.send(Message::Undo);

    select(&mut w, noise);
    w.send(Message::ChoiceMenu(Some((noise, "type"))));
    w.snapshot("state_noise_types");
    w.send(Message::ChoiceMenu(None));

    w.send(Message::ToggleVariable("mortar".into()));
    w.send(Message::ColorPicker(Some(ColorTarget::Variable("tint".into(), VarField::Value))));
    w.snapshot("state_inputs");
    w.send(Message::ToggleVariable("mortar".into()));
    w.send(Message::ColorPicker(None));

    w.send(Message::Menu(Some(MenuId::Edit)));
    w.snapshot("state_menu");
    w.send(Message::Menu(None));

    w.app.ui.add_menu = Some(AddMenu { at: Point::new(700.0, 300.0), graph_pos: [0.0, 0.0], query: "bl".into(), selected: 0, scroll: 0.0, pending: None });
    w.snapshot("state_add_menu");
    w.app.ui.add_menu = None;

    w.send(Message::Context(Some(ContextMenu { at: Point::new(640.0, 360.0), target: ContextTarget::Node(levels) })));
    w.snapshot("state_context_menu");
    w.send(Message::Context(None));

    w.send(Message::Edit(Edit::SetResolution([1024, 1024])));
    w.send(Message::Dialog(Some(Dialog::Unsaved(PendingAction::Quit))));
    w.snapshot("state_dialog_unsaved");
    w.send(Message::Dialog(Some(Dialog::Shortcuts)));
    w.snapshot("state_dialog_shortcuts");
    w.send(Message::Dialog(Some(Dialog::About)));
    w.snapshot("state_dialog_about");
}

/// Window position of a node's input socket.
fn input_socket(w: &Harness, node: brewed_texture_core::graph::NodeId, key: &str) -> Point {
    let graph = w.app.graph();
    let view = w.app.ui.graph_view;
    let t = view::graph::layout::Transform { pan: view.pan, zoom: view.zoom };
    let l = view::graph::layout::layout(graph, graph.node(node).unwrap(), &t, iced::Vector::ZERO);
    let (_, p) = l.inputs.iter().find(|(s, _)| s.key == key).expect("socket");
    *p + iced::Vector::new(view.bounds.x, view.bounds.y)
}

fn drag(w: &mut Harness, from: Point, to: Point) {
    w.hover(from);
    w.mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    w.hover(Point::new((from.x + to.x) / 2.0, (from.y + to.y) / 2.0));
    w.hover(to);
    w.mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
}

/// A link picked up from an input moves to another input in one undo step; dropped on a
/// socket that cannot take it, or clicked without moving, it stays where it was.
#[test]
fn links_are_rerouted_in_one_step() {
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let levels = w.app.graph().nodes_of(NodeKind::LEVELS).next().unwrap().id;
    let source = w.app.graph().link_into(levels, "i:input").unwrap().from;
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::INVERT, pos: [1300.0, -200.0] }));
    let invert = *w.app.ui.selection.iter().next().unwrap();
    w.send(Message::Seal);
    w.input(&[iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))]);
    // Everything in view.
    w.send(Message::Graph(GraphMessage::ClearSelection));
    w.send(Message::Frame);
    w.input(&[]);
    let steps = |w: &Harness| w.app.doc.undo_label().map(str::to_string);

    // Re-route.
    let before = steps(&w);
    let (from, to) = (input_socket(&w, levels, "i:input"), input_socket(&w, invert, "i:input"));
    drag(&mut w, from, to);
    assert!(w.app.graph().link_into(levels, "i:input").is_none());
    assert_eq!(w.app.graph().link_into(invert, "i:input").map(|l| l.from), Some(source));
    w.send(Message::Undo);
    assert_eq!(w.app.graph().link_into(levels, "i:input").map(|l| l.from), Some(source), "one undo restores it");
    assert!(w.app.graph().link_into(invert, "i:input").is_none());
    assert_eq!(steps(&w), before);

    // Dropped on a socket of the wrong type: nothing changes.
    let tile = w.app.graph().nodes_of(NodeKind::TILE_GENERATOR).next().unwrap().id;
    let x_amount = brewed_texture_core::graph::param_socket_key("x_amount");
    let (from, to) = (input_socket(&w, levels, "i:input"), input_socket(&w, tile, &x_amount));
    drag(&mut w, from, to);
    assert_eq!(w.app.graph().link_into(levels, "i:input").map(|l| l.from), Some(source));
    assert_eq!(steps(&w), before);

    // A click on a connected input changes nothing either.
    let at = input_socket(&w, levels, "i:input");
    drag(&mut w, at, at);
    assert_eq!(w.app.graph().link_into(levels, "i:input").map(|l| l.from), Some(source));
    assert_eq!(steps(&w), before);
}

/// Presses a key as the window reports it: `typed` is what it types on the layout in use,
/// `physical` is which key it is on the keyboard.
fn key(w: &mut Harness, typed: &str, physical: keyboard::key::Physical, modifiers: keyboard::Modifiers) {
    let key = Key::Character(typed.into());
    let window = w.app.main_window().expect("the main window");
    let event = iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: physical,
        location: keyboard::Location::Standard,
        modifiers,
        text: None,
        repeat: false,
    });
    w.send(Message::Runtime(event, iced::event::Status::Ignored, window));
}

/// Shortcuts are on the same keys whatever the keyboard layout is: a key that types no
/// Latin letter (Cyrillic, Greek, Arabic…) counts as the Latin letter in its place, and a
/// layout that moves the Latin letters around (AZERTY, QWERTZ) keeps them where they are
/// printed on the keys.
#[test]
fn shortcuts_work_on_every_keyboard_layout() {
    use keyboard::key::{Code, Physical};

    let mut w = harness!(|app: &App| view::root(app), 1680.0, 1000.0);
    let ctrl = keyboard::Modifiers::CTRL;
    let count = |w: &Harness| w.app.graph().nodes.len();
    let before = count(&w);
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::BLUR, pos: [0.0, 0.0] }));
    let added = count(&w);
    assert_eq!(added, before + 1);

    // Russian: the key printed "Z" types "я", and still undoes.
    key(&mut w, "я", Physical::Code(Code::KeyZ), ctrl);
    assert_eq!(count(&w), before, "Ctrl+Z on a Cyrillic layout undoes");
    // Ctrl+Shift+Z redoes, in the capitals the layout types.
    key(&mut w, "Я", Physical::Code(Code::KeyZ), ctrl | keyboard::Modifiers::SHIFT);
    assert_eq!(count(&w), added, "Ctrl+Shift+Z on a Cyrillic layout redoes");
    // Greek "ω" sits on the "Z" key, "ψ" on the "Y" key: Ctrl+Y redoes as well.
    key(&mut w, "ω", Physical::Code(Code::KeyZ), ctrl);
    assert_eq!(count(&w), before);
    key(&mut w, "ψ", Physical::Code(Code::KeyY), ctrl);
    assert_eq!(count(&w), added);

    // AZERTY types "z" where QWERTY has "w": the shortcut follows the letter, so it stays
    // under the key that has "Z" printed on it.
    key(&mut w, "z", Physical::Code(Code::KeyW), ctrl);
    assert_eq!(count(&w), before, "Ctrl+Z on AZERTY undoes");
    key(&mut w, "w", Physical::Code(Code::KeyZ), ctrl);
    assert_eq!(count(&w), before, "the key that types 'w' does not undo");
    key(&mut w, "y", Physical::Code(Code::KeyZ), ctrl);
    assert_eq!(count(&w), added, "Ctrl+Y on QWERTZ redoes");

    // The others too: a Cyrillic "ф" on the "A" key selects everything, "в" on the "D"
    // key duplicates it, and "и" on the "B" key bypasses a node.
    w.send(Message::Graph(GraphMessage::ClearSelection));
    let blur = w.app.graph().nodes_of(NodeKind::BLUR).next().expect("the added node").id;
    w.send(Message::Graph(GraphMessage::Select { node: blur, mode: SelectMode::Replace }));
    key(&mut w, "в", Physical::Code(Code::KeyD), ctrl);
    assert_eq!(count(&w), added + 1, "Ctrl+D duplicates");
    key(&mut w, "ф", Physical::Code(Code::KeyA), ctrl);
    assert_eq!(w.app.ui.selection.len(), added + 1, "Ctrl+A selects every node");
    w.send(Message::Graph(GraphMessage::Select { node: blur, mode: SelectMode::Replace }));
    key(&mut w, "и", Physical::Code(Code::KeyB), ctrl);
    assert!(w.app.graph().node(blur).is_some_and(|n| n.bypass), "Ctrl+B bypasses");

    // A key the window cannot place on the keyboard and that types nothing Latin does
    // nothing at all, rather than the wrong thing.
    let nodes = count(&w);
    key(&mut w, "ж", Physical::Unidentified(keyboard::key::NativeCode::Unidentified), ctrl);
    assert_eq!(count(&w), nodes);
}

/// Where a comment is on the screen, in window coordinates.
fn comment_rect(w: &Harness, id: brewed_texture_core::CommentId) -> iced::Rectangle {
    let view = w.app.ui.graph_view;
    let t = view::graph::layout::Transform { pan: view.pan, zoom: view.zoom };
    let comment = w.app.graph().comment(id).expect("the comment");
    let l = view::graph::layout::comment_layout(comment, &t, iced::Vector::ZERO, None);
    l.rect + iced::Vector::new(view.bounds.x, view.bounds.y)
}

/// A comment frames the selected nodes, is dragged around with them, resized by its
/// corner, recolored, written in and deleted — and its body never steals the mouse from
/// the canvas underneath.
#[test]
fn comments_group_nodes_and_travel_with_them() {
    let mut w = harness!(view::root, 1680.0, 1000.0);
    // The canvas reports where it is on the first frame, and the graph is framed in it.
    w.input(&[iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))]);
    w.send(Message::Frame);
    w.input(&[]);
    let levels = w.app.graph().nodes_of(NodeKind::LEVELS).next().expect("a Levels node").id;
    let start = w.app.graph().node(levels).expect("the node").pos;

    // A comment around the selection, and the node it holds is inside it.
    w.send(Message::Graph(GraphMessage::Select { node: levels, mode: SelectMode::Replace }));
    w.send(Message::AddComment);
    let id = w.app.ui.selected_comment.expect("the new comment is selected");
    let comment = w.app.graph().comment(id).expect("the comment").clone();
    let size = view::graph::layout::node_size(w.app.graph(), w.app.graph().node(levels).unwrap());
    assert!(comment.holds(start, size), "{comment:?} does not hold the node at {start:?}");

    // Dragging the title bar moves the comment and the node in it, by as much of the graph
    // as the mouse crossed on the screen.
    let zoom = w.app.ui.graph_view.zoom;
    let (dx, dy) = (60.0 / zoom, 30.0 / zoom);
    let rect = comment_rect(&w, id);
    let title = Point::new(rect.x + 40.0, rect.y + 8.0);
    drag(&mut w, title, Point::new(title.x + 60.0, title.y + 30.0));
    let moved = w.app.graph().comment(id).expect("the comment").pos;
    assert!((moved[0] - comment.pos[0] - dx).abs() < 2.0 && (moved[1] - comment.pos[1] - dy).abs() < 2.0, "{moved:?}");
    let node_now = w.app.graph().node(levels).expect("the node").pos;
    assert!((node_now[0] - start[0] - dx).abs() < 2.0 && (node_now[1] - start[1] - dy).abs() < 2.0, "{node_now:?}");

    // One undo puts both back.
    w.send(Message::Undo);
    assert_eq!(w.app.graph().node(levels).expect("the node").pos, start);
    assert_eq!(w.app.graph().comment(id).expect("the comment").pos, comment.pos);

    // The corner resizes it, and nothing can make it smaller than its minimum.
    let rect = comment_rect(&w, id);
    let corner = Point::new(rect.x + rect.width - 4.0, rect.y + rect.height - 4.0);
    drag(&mut w, corner, Point::new(corner.x + 80.0, corner.y + 50.0));
    let grown = w.app.graph().comment(id).expect("the comment").size;
    let (gx, gy) = (80.0 / zoom, 50.0 / zoom);
    assert!((grown[0] - comment.size[0] - gx).abs() < 3.0 && (grown[1] - comment.size[1] - gy).abs() < 3.0, "{grown:?}");
    let rect = comment_rect(&w, id);
    let corner = Point::new(rect.x + rect.width - 4.0, rect.y + rect.height - 4.0);
    drag(&mut w, corner, Point::new(rect.x - 400.0, rect.y - 400.0));
    let small = w.app.graph().comment(id).expect("the comment").size;
    assert_eq!(small, brewed_texture_core::Comment::MIN_SIZE);

    // Its text and color are the comment's own, and the properties panel edits them.
    w.send(Message::Edit(Edit::SetCommentText { id, text: "Roughness".into() }));
    let blue = [0.28, 0.51, 0.80, 1.0];
    w.send(Message::Edit(Edit::SetCommentColor { id, color: blue }));
    let comment = w.app.graph().comment(id).expect("the comment").clone();
    assert_eq!((comment.text.as_str(), comment.color), ("Roughness", blue));
    // A color it cannot be given leaves it as it was.
    w.send(Message::Edit(Edit::SetCommentColor { id, color: [f32::NAN, 0.0, 0.0, 1.0] }));
    assert_eq!(w.app.graph().comment(id).expect("the comment").color, blue);

    // A press in the middle of a comment goes to the canvas: it box-selects instead.
    let rect = comment_rect(&w, id);
    let inside = Point::new(rect.x + rect.width - 30.0, rect.y + rect.height / 2.0);
    drag(&mut w, inside, Point::new(inside.x - 10.0, inside.y + 10.0));
    assert_eq!(w.app.ui.selected_comment, None, "the body of a comment is not a handle");

    // Selected again by its title bar, Delete removes the frame and keeps the nodes.
    let title = Point::new(rect.x + 40.0, rect.y + 8.0);
    drag(&mut w, title, title);
    assert_eq!(w.app.ui.selected_comment, Some(id));
    w.send(Message::Delete);
    assert!(w.app.graph().comment(id).is_none());
    assert!(w.app.graph().node(levels).is_some(), "deleting a comment keeps its nodes");
    w.send(Message::Undo);
    assert_eq!(w.app.graph().comment(id).map(|c| c.text.clone()), Some("Roughness".into()));
}

/// A value can be dragged as far as the user likes: the pointer is put back where the drag
/// started after every move, so the edge of the screen never ends it. Parameters that would
/// mean nothing outside their range still stop there.
#[test]
fn a_value_is_dragged_as_far_as_the_pointer_can_be_pushed() {
    use crate::scrub::fake;
    use crate::widget::number::number;

    // A window barely wider than the field: a plain drag would run out of room at once.
    fn radius(app: &App) -> f32 {
        match app.graph().nodes_of(NodeKind::BLUR).next().and_then(|n| n.value("radius")) {
            Some(ParamValue::Float(v)) => *v,
            _ => 0.0,
        }
    }
    let mut w = harness!(
        |app: &App| {
            let node = app.graph().nodes_of(NodeKind::BLUR).next().expect("the demo Blur").id;
            number(radius(app) as f64, 0.0, 128.0)
                .on_change(move |v| Message::Edit(Edit::SetParam { node, key: "radius".into(), value: ParamValue::Float(v as f32) }))
                .into()
        },
        400.0,
        30.0
    );
    let start = radius(&w.app);
    let (origin, edge) = (Point::new(200.0, 15.0), Point::new(380.0, 15.0));

    // With no pointer to hold, the drag is what it always was: it stops at the edge.
    fake::forget();
    drag(&mut w, origin, edge);
    let plain = radius(&w.app);
    assert!(plain > start, "the drag changed nothing");

    // Holding the pointer, the same room is crossed again and again.
    let blur = w.app.graph().nodes_of(NodeKind::BLUR).next().expect("the demo Blur").id;
    w.send(Message::Edit(Edit::SetParam { node: blur, key: "radius".into(), value: ParamValue::Float(start) }));
    fake::place((640.0, 400.0));
    // Away from the first drag, so that this press is a press and not a double click.
    let start_at = Point::new(60.0, 15.0);
    w.hover(start_at);
    w.mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    for _ in 0..4 {
        w.hover(Point::new(start_at.x + 180.0, start_at.y));
        // The pointer is put back where the drag started, and the window says so.
        w.hover(start_at);
    }
    w.mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    let held = radius(&w.app);
    assert_eq!(fake::recentered(), 4, "the pointer is put back after every move");
    assert!(held > start + (plain - start) * 3.5, "{held} is not four screens' worth of dragging from {start}");
    fake::forget();

    // A parameter that means nothing outside its range stops at it, however far the drag
    // goes: a gamma of zero is no curve at all.
    let levels = w.app.graph().nodes_of(NodeKind::LEVELS).next().expect("the demo Levels").id;
    let gamma = brewed_texture_core::NodeKind::LEVELS.def().param("gamma").expect("gamma");
    let (lo, hi) = gamma.limits.expect("gamma is held to a range");
    for value in [-1000.0, 1000.0] {
        w.send(Message::Edit(Edit::SetParam { node: levels, key: "gamma".into(), value: ParamValue::Float(value) }));
        let set = w.app.graph().node(levels).and_then(|n| n.value("gamma")).cloned();
        assert_eq!(set, Some(ParamValue::Float(value.clamp(lo, hi))), "gamma {value}");
    }
    // Most parameters are free: unusual values are how unusual textures are found.
    assert!(brewed_texture_core::NodeKind::TRANSFORM_2D.def().param("rotation").expect("rotation").limits.is_none());
}

/// The color picker floats where it was opened instead of unfolding inside a panel, so
/// nothing under it moves; a click anywhere else puts it away.
#[test]
fn the_color_picker_floats_over_the_interface() {
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let window = w.app.main_window().expect("the main window");
    // A node of its own: the demo's Uniform Color takes its color from an input socket.
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::UNIFORM_COLOR, pos: [0.0, 0.0] }));
    let uniform = w.app.ui.selection.iter().next().copied().expect("the new node");
    let color = |w: &Harness| match w.app.graph().node(uniform).and_then(|n| n.value("color")) {
        Some(ParamValue::Color(c)) => *c,
        _ => panic!("no color"),
    };
    w.send(Message::Graph(GraphMessage::Select { node: uniform, mode: SelectMode::Replace }));
    w.input(&[iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))]);

    // Opened from the swatch: the picker appears under the cursor, over everything.
    let at = Point::new(1500.0, 400.0);
    w.window_event(window, iced::Event::Mouse(mouse::Event::CursorMoved { position: at }));
    w.send(Message::ColorPicker(Some(ColorTarget::Param(uniform, "color".into()))));
    w.input(&[]);
    assert!(w.app.ui.color_picker.is_some());

    // The saturation square of the floating picker takes the mouse: dragging in it changes
    // the color, which means the picker really is there and not in the panel.
    let before = color(&w);
    let square = w.app.ui.color_picker_at + iced::Vector::new(118.0, 70.0);
    drag(&mut w, square, square + iced::Vector::new(-40.0, 20.0));
    assert_ne!(color(&w), before, "the picker is not under the cursor");

    // A click anywhere else puts it away, and the color it set stays.
    let changed = color(&w);
    drag(&mut w, Point::new(500.0, 900.0), Point::new(500.0, 900.0));
    assert_eq!(w.app.ui.color_picker, None, "a click elsewhere closes the picker");
    assert_eq!(color(&w), changed);
}

/// Panning the graph keeps the point that was grabbed under the mouse, however fast the
/// mouse goes round: several moves arrive in one frame, and none of them may be lost.
#[test]
fn the_graph_stays_under_the_mouse_while_it_is_dragged() {
    let mut w = harness!(view::root, 1680.0, 1000.0);
    w.input(&[iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))]);
    w.send(Message::Frame);
    w.input(&[]);
    let view = w.app.ui.graph_view;
    let start = Point::new(view.bounds.x + view.bounds.width * 0.5, view.bounds.y + view.bounds.height * 0.5);
    let grabbed = view.to_graph(start);

    // Fast circles with the middle button, many moves delivered at once, as the window
    // does when the mouse is quicker than the frames.
    w.hover(start);
    w.mouse(mouse::Event::ButtonPressed(mouse::Button::Middle));
    let mut at = start;
    for turn in 0..6 {
        let moves: Vec<iced::Event> = (0..24)
            .map(|i| {
                let a = (turn * 24 + i) as f32 * 0.35;
                at = Point::new(start.x + 120.0 * a.cos() - 120.0, start.y + 120.0 * a.sin());
                iced::Event::Mouse(mouse::Event::CursorMoved { position: at })
            })
            .collect();
        w.cursor = mouse::Cursor::Available(at);
        w.input(&moves);
    }
    w.mouse(mouse::Event::ButtonReleased(mouse::Button::Middle));

    // The point that was grabbed is still right under the mouse.
    let under = w.app.ui.graph_view.to_graph(at);
    assert!((under[0] - grabbed[0]).abs() < 0.5 && (under[1] - grabbed[1]).abs() < 0.5, "{under:?} drifted from {grabbed:?}");
}

/// Where the view shows exactly `wanted`, in the order the widgets are laid out (the
/// window first, then what floats over it).
fn texts(w: &mut Harness, wanted: &str) -> Vec<iced::Rectangle> {
    struct Find<'a> {
        wanted: &'a str,
        found: Vec<iced::Rectangle>,
    }
    impl Operation for Find<'_> {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }
        fn text(&mut self, _: Option<&iced::widget::Id>, bounds: iced::Rectangle, text: &str) {
            if text == self.wanted {
                self.found.push(bounds);
            }
        }
    }
    let mut find = Find { wanted, found: Vec::new() };
    w.frame(|ui, renderer| ui.operate(renderer, &mut find));
    find.found
}

fn click(w: &mut Harness, at: Point) {
    drag(w, at, at);
}

/// Presses a key that types nothing (arrows, Enter…) as the window reports it.
fn press(w: &mut Harness, named: keyboard::key::Named) {
    let key = Key::Named(named);
    let window = w.app.main_window().expect("the main window");
    let event = iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::default(),
        text: None,
        repeat: false,
    });
    w.send(Message::Runtime(event, iced::event::Status::Ignored, window));
}

/// A noise is picked by how it looks: the menu shows a picture of every type next to its
/// name, below the field or above it but never over it, and a click, or the arrow keys and
/// Enter, pick one in a single undo step. What is typed finds types by their names and by
/// what they look like.
#[test]
fn choices_are_picked_by_their_pictures() {
    use brewed_texture_core::nodes::NOISE_TYPES;
    use crate::app::ChoiceMenu;

    for (width, height) in [(1680.0, 1000.0), (1000.0, 640.0)] {
        let mut w = harness!(view::root, width, height);
        fake_thumbnails(&mut w.app);
        let noise = w.app.graph().nodes_of(NodeKind::NOISE).next().expect("the demo's noise").id;
        let kind = |w: &Harness| w.app.graph().node(noise).map(|n| n.enum_value("type"));
        w.send(Message::Graph(GraphMessage::Select { node: noise, mode: SelectMode::Replace }));
        w.input(&[iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))]);

        // The field shows the current type; a click on it opens the menu.
        let field = texts(&mut w, "Perlin").first().copied().expect("the field says Perlin");
        click(&mut w, field.center());
        let opened = ChoiceMenu { node: noise, key: "type", cursor: 0, query: String::new() };
        assert_eq!(w.app.ui.choice_menu, Some(opened), "{width}×{height}");
        // The engine draws the pictures (here, one of each gray).
        let pictures = (0..NOISE_TYPES.len()).map(|i| vec![(i * 15) as u8; 4 * 4 * 4]).collect();
        w.send(Message::Engine(brewed_texture_engine::Event::Choices(brewed_texture_engine::ChoicePictures {
            kind: NodeKind::NOISE,
            key: "type".into(),
            size: 4,
            pictures: std::sync::Arc::new(pictures),
        })));
        assert_eq!(w.app.res.choices.get(&(NodeKind::NOISE.id(), "type")).map(Vec::len), Some(NOISE_TYPES.len()));
        w.input(&[]);
        w.snapshot(&format!("choices_{width}x{height}"));

        // Every type is in the menu, clear of the field; in a window too low for all of them
        // the menu scrolls, and what of it shows is inside the window.
        let window = iced::Rectangle::new(Point::ORIGIN, Size::new(width, height));
        let inside = |at: iced::Rectangle| at.x >= 0.0 && at.y >= 0.0 && at.x + at.width <= width && at.y + at.height <= height;
        for name in NOISE_TYPES {
            let at = *texts(&mut w, name).last().unwrap_or_else(|| panic!("{name} is not in the menu"));
            assert!(at.intersection(&field).is_none(), "{name} covers the field");
            assert!(inside(at) || height < 1000.0, "{name} at {at:?}, outside {window:?}");
        }
        let first = *texts(&mut w, "Perlin").last().expect("Perlin in the menu");
        assert!(inside(first), "the top of the menu at {first:?}, outside {window:?}");

        // A click on a type picks it and closes the menu; one undo puts the old one back.
        let cells = *texts(&mut w, "Cells").last().expect("Cells in the menu");
        click(&mut w, cells.center());
        assert_eq!(kind(&w), Some(brewed_texture_core::nodes::NOISE_CELLS));
        assert_eq!(w.app.ui.choice_menu, None);
        w.send(Message::Undo);
        assert_eq!(kind(&w), Some(0), "one undo puts Perlin back");

        // The keyboard walks the menu: right, right, down (to the same place in the next row)
        // and Enter.
        let walked = 2 + view::choices::columns(NOISE_TYPES.len()) as u32;
        w.send(Message::ChoiceMenu(Some((noise, "type"))));
        for named in [keyboard::key::Named::ArrowRight, keyboard::key::Named::ArrowRight, keyboard::key::Named::ArrowDown] {
            press(&mut w, named);
        }
        assert_eq!(w.app.ui.choice_menu.as_ref().map(|m| m.cursor as u32), Some(walked));
        press(&mut w, keyboard::key::Named::Enter);
        assert_eq!(kind(&w), Some(walked));
        assert_eq!(w.app.ui.choice_menu, None);

        // Esc, or a click anywhere else, closes it without picking anything.
        w.send(Message::ChoiceMenu(Some((noise, "type"))));
        press(&mut w, keyboard::key::Named::ArrowLeft);
        press(&mut w, keyboard::key::Named::Escape);
        assert_eq!(w.app.ui.choice_menu, None);
        w.send(Message::ChoiceMenu(Some((noise, "type"))));
        w.input(&[]);
        click(&mut w, Point::new(width * 0.05, height * 0.5));
        assert_eq!(w.app.ui.choice_menu, None);
        assert_eq!(kind(&w), Some(walked));

        // Typed into the search field (which opening the menu gives the keyboard to), a word
        // leaves the types it names or describes: these are all made of cells.
        w.send(Message::ChoiceMenu(Some((noise, "type"))));
        w.input(&[]);
        assert!(matches!(w.operate(focusable::focus::<()>(view::choices::search_id())), Outcome::None));
        assert_eq!(w.focused(), Some(view::choices::search_id()), "the search field takes the keyboard");
        assert!(w.input(&typing("voronoi")) > 0, "the search field got nothing");
        let cellular = ["Cells", "Cracks", "Crystal", "Mosaic", "Stones"];
        for (i, name) in NOISE_TYPES.iter().enumerate() {
            // (The field says what the type is now.)
            let in_field = usize::from(i as u32 == walked);
            let shows = texts(&mut w, name).len() > in_field;
            assert_eq!(shows, cellular.contains(name), "{name} {}", if shows { "shows" } else { "is gone" });
        }
        // The keyboard is on the first of them; down goes to the next row of what is left.
        let option = |name: &str| NOISE_TYPES.iter().position(|n| *n == name).map(|i| i as u32);
        assert_eq!(w.app.ui.choice_menu.as_ref().map(|m| m.cursor as u32), option("Cells"));
        press(&mut w, keyboard::key::Named::ArrowDown);
        let below = cellular[view::choices::columns(NOISE_TYPES.len()).min(cellular.len() - 1)];
        assert_eq!(w.app.ui.choice_menu.as_ref().map(|m| m.cursor as u32), option(below));
        // Left and right move in the text while there is some, not from picture to picture.
        press(&mut w, keyboard::key::Named::ArrowLeft);
        assert_eq!(w.app.ui.choice_menu.as_ref().map(|m| m.cursor as u32), option(below));
        // Two words must both match; nothing matching says so, and Enter then picks nothing.
        w.send(Message::ChoiceSearch("voronoi flat".into()));
        assert!(texts(&mut w, "Cells").is_empty() && !texts(&mut w, "Mosaic").is_empty());
        w.send(Message::ChoiceSearch("zzz".into()));
        assert!(!texts(&mut w, "Nothing matches").is_empty());
        press(&mut w, keyboard::key::Named::Enter);
        assert_eq!(kind(&w), Some(walked));
        // A name picks with Enter, whatever the keyboard was on before.
        w.send(Message::ChoiceSearch("maze".into()));
        press(&mut w, keyboard::key::Named::Enter);
        assert_eq!(kind(&w), option("Maze"));
        assert_eq!(w.app.ui.choice_menu, None);
        // The next time the menu opens, the search is empty and every type shows again.
        w.send(Message::ChoiceMenu(Some((noise, "type"))));
        assert_eq!(w.app.ui.choice_menu.as_ref().map(|m| m.query.as_str()), Some(""));
        assert!(!texts(&mut w, "Cells").is_empty());
        w.send(Message::ChoiceMenu(None));
    }
}

/// What drawing one frame of the whole window costs on the CPU (prints; for tuning).
#[test]
#[ignore = "a measurement"]
fn frame_cost() {
    let mut w = harness!(view::root, 1680.0, 1000.0);
    fake_thumbnails(&mut w.app);
    let redraw = || iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()));
    w.input(&[redraw()]);
    w.input(&[redraw()]);
    let theme = crate::style::theme();
    let (cursor, runs) = (mouse::Cursor::Available(Point::new(800.0, 300.0)), 200);
    let (update, draw) = w.frame(|ui, renderer| {
        let (mut update, mut draw) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
        for _ in 0..runs {
            let t = std::time::Instant::now();
            let _ = ui.update(&[redraw()], cursor, renderer, &mut clipboard::Null, &mut Vec::new());
            update += t.elapsed();
            let t = std::time::Instant::now();
            ui.draw(renderer, &theme, &renderer::Style { text_color: Color::WHITE }, cursor);
            draw += t.elapsed();
        }
        (update, draw)
    });
    println!("update {:.3} ms, draw {:.3} ms a frame", update.as_secs_f64() * 1000.0 / runs as f64, draw.as_secs_f64() * 1000.0 / runs as f64);
}


/// Draw one frame, as the window does at `at`, and say what comes next: what the widgets ask
/// for, or another frame when they published anything (the runtime draws again after
/// messages).
fn draw_frame(w: &mut Harness, at: std::time::Instant) -> window::RedrawRequest {
    let mut messages = Vec::new();
    let cursor = w.cursor;
    let redraw = iced::Event::Window(window::Event::RedrawRequested(at));
    let state = w.frame(|ui, renderer| ui.update(&[redraw], cursor, renderer, &mut clipboard::Null, &mut messages).0);
    let published = !messages.is_empty();
    for m in messages {
        w.send(m);
    }
    match state {
        _ if published => window::RedrawRequest::NextFrame,
        iced_runtime::user_interface::State::Updated { redraw_request, .. } => redraw_request,
        iced_runtime::user_interface::State::Outdated => window::RedrawRequest::NextFrame,
    }
}

/// The 3D view draws frame after frame in real time and counts them; its header says how many
/// it drew in the last second and how long a frame took, and says so no longer once it stops.
#[test]
fn the_3d_view_draws_in_real_time() {
    use std::time::{Duration, Instant};
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let start = Instant::now();
    // A monitor that shows 144 frames a second.
    let at = |frame: u64| start + Duration::from_nanos(frame * 1_000_000_000 / 144);
    for frame in 0..300 {
        assert_eq!(draw_frame(&mut w, at(frame)), window::RedrawRequest::NextFrame, "frame {frame}");
    }
    let rate = w.app.ui.frame_rate;
    assert!((rate.fps - 144.0).abs() < 0.5, "{rate:?}");
    assert!(!texts(&mut w, "144 FPS · 6.9 ms").is_empty(), "the 3D view says so");

    // Drawing on demand, a frame asks for no other.
    w.send(Message::ToggleRealtime);
    assert_eq!(draw_frame(&mut w, at(300)), window::RedrawRequest::Wait, "on demand, a frame asks for none");
    // A second after its last frame, the 3D view has drawn none in the last second.
    w.app.ui.frame_rate_at = Some(Instant::now() - Duration::from_secs(2));
    w.send(Message::Tick);
    assert!(!texts(&mut w, "0 FPS · 0.0 ms").is_empty());
}

/// A curve is drawn with the mouse in its square, under its row in the properties: a click
/// adds a point there and drags it, in one step to undo.
#[test]
fn a_curve_is_drawn_with_the_mouse() {
    use brewed_texture_core::Curve;
    let mut w = harness!(view::root, 1680.0, 1000.0);
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::CURVE, pos: [-400.0, 900.0] }));
    w.send(Message::Seal);
    let id = w.app.graph().nodes_of(NodeKind::CURVE).next().expect("the curve").id;
    w.send(Message::Graph(GraphMessage::Select { node: id, mode: SelectMode::Replace }));
    w.input(&[iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))]);
    let curve = |w: &Harness| w.app.graph().node(id).and_then(|n| n.value("curve")).and_then(|v| v.as_curve()).cloned().expect("a curve");
    assert_eq!(curve(&w), Curve::default());
    w.snapshot("curve_editor");

    // The square lies between the row of the curve and the row after it.
    let right = |r: &&iced::Rectangle| r.x > 1300.0;
    let label = *texts(&mut w, "Curve").iter().filter(right).max_by(|a, b| a.y.total_cmp(&b.y)).expect("the row of the curve");
    let next = *texts(&mut w, "Channels").iter().find(right).expect("the row after it");
    let inside = Point::new(label.x + 150.0, (label.y + label.height + next.y) / 2.0);
    drag(&mut w, inside, inside + iced::Vector::new(0.0, -40.0));
    let drawn = curve(&w);
    assert_eq!(drawn.points.len(), 3, "{drawn:?}");
    let middle = drawn.points[1];
    assert!(middle[1] > middle[0] + 0.1, "dragged up: {middle:?}");
    w.send(Message::Undo);
    assert_eq!(curve(&w), Curve::default(), "one undo puts the straight line back");
    w.send(Message::Redo);
    assert_eq!(curve(&w), drawn);
}

/// The 3D view is lit by the studio or by an environment image, chosen in the menu of its
/// header: the studio, the images used last, and one to look for; how bright, and how far it
/// is turned. An image that cannot be read says so and is forgotten, the view staying lit as
/// it was; one that can lights the view, and shows behind the model as blurred as asked. The
/// studio comes back at once, and so does the image read last.
#[test]
fn the_3d_view_is_lit_by_an_environment_image() {
    use crate::app::PreviewMessage as P;
    let dir = scratch("environment");
    let sky = dir.join("sky.hdr");
    let pixels: Vec<image::Rgb<f32>> =
        (0..32u32).flat_map(|y| (0..64u32).map(move |x| image::Rgb([x as f32 / 16.0, 1.0 - y as f32 / 32.0, 2.0]))).collect();
    image::codecs::hdr::HdrEncoder::new(std::fs::File::create(&sky).unwrap()).encode(&pixels, 64, 32).unwrap();
    let (sky, missing) = (sky.display().to_string(), dir.join("missing.hdr").display().to_string());
    let read = |file: &str| P::EnvironmentRead(file.to_string(), crate::environment::load(std::path::Path::new(file)).map(std::sync::Arc::new));
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let shown = |w: &mut Harness, text: &str| !texts(w, text).is_empty();

    // The menu: the studio, and a file to look for; the exposure and the turn.
    w.send(Message::Preview(P::LightingMenu(true)));
    for text in ["Environment", "●  Studio", "Load an Environment Image…", "Exposure", "Turn"] {
        assert!(shown(&mut w, text), "{text} is in the menu");
    }
    assert!(!shown(&mut w, "Show It Behind the Model"), "the studio has nothing to show behind the model");

    // An image that is not there: the view stays lit by the studio, and the menu forgets it.
    w.send(Message::Preview(P::Environment(Some(missing.clone()))));
    assert_eq!(w.app.ui.preview.lighting.reading.as_deref(), Some(missing.as_str()));
    w.send(Message::Preview(read(&missing)));
    let l = &w.app.ui.preview.lighting;
    assert_eq!((l.file.as_deref(), l.shown.is_none(), l.reading.as_deref()), (None, true, None));
    assert!(l.recent.is_empty());
    assert!(w.app.ui.status.as_ref().is_some_and(|s| s.error && s.text.contains("missing.hdr")));

    // One that is: once read it lights the view, and the menu has it, with the background.
    w.send(Message::Preview(P::Environment(Some(sky.clone()))));
    assert!(w.app.ui.preview.lighting.shown.is_none(), "the studio until the image is read");
    w.send(Message::Preview(read(&sky)));
    let l = &w.app.ui.preview.lighting;
    assert_eq!(l.shown.as_ref().map(|h| h.path.as_str()), Some(sky.as_str()));
    assert_eq!((l.shown.as_ref().map(|h| h.width), l.reading.as_deref()), (Some(64), None));
    w.send(Message::Preview(P::LightingMenu(true)));
    for text in ["○  Studio", "●  sky.hdr", "●  Show It Behind the Model", "Blur"] {
        assert!(shown(&mut w, text), "{text} is in the menu");
    }
    w.snapshot("lighting_menu");

    // The studio at once, from the menu; the image again at once, as it was read last.
    let studio = *texts(&mut w, "○  Studio").last().expect("the studio in the menu");
    click(&mut w, studio.center());
    let l = &w.app.ui.preview.lighting;
    assert!(l.file.is_none() && l.shown.is_none() && !l.menu, "a click on the studio chooses it and closes the menu");
    w.send(Message::Preview(P::Environment(Some(sky.clone()))));
    let l = &w.app.ui.preview.lighting;
    assert!(l.shown.is_some() && l.reading.is_none(), "the image read last comes back at once");

    // Brightness and blur stay within bounds, whatever is asked.
    for (ev, blur) in [(3.0, 0.5), (f32::NAN, f32::NAN), (99.0, -1.0)] {
        w.send(Message::Preview(P::Exposure(ev)));
        w.send(Message::Preview(P::Blur(blur)));
    }
    let l = &w.app.ui.preview.lighting;
    assert_eq!((l.exposure, l.blur), (12.0, 0.0));
    w.send(Message::Preview(P::Background(false)));
    assert!(!w.app.ui.preview.lighting.background);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The status bar says what happens now, and nothing older than a second: how many nodes the
/// graph has, how many of them its last change computed again and how long its changes take,
/// or how long a change has been computed when it takes long; and the GPU memory.
#[test]
fn the_status_bar_says_what_happens_now() {
    use brewed_texture_engine::{EvalReport, Event, Memory};
    use std::time::{Duration, Instant};
    let mut w = harness!(view::toolbar::status_bar, 1680.0, 24.0);
    let shown = |w: &mut Harness, text: &str| !texts(w, text).is_empty();
    let nodes = w.app.graph().nodes.len();
    // What the engine tells after two changes that computed three nodes each.
    let report = EvalReport { recomputed: 3, image_nodes: 17 };
    let mb = |n: u64| n * 1024 * 1024;
    let memory = Memory { shown: mb(64), shown_images: 5, kept: mb(16), scratch: mb(264), files: 0 };
    w.send(Message::Engine(Event::Memory(memory)));
    w.send(Message::Engine(Event::Finished { revision: 1, report, millis: 7.0 }));
    w.send(Message::Engine(Event::Finished { revision: 1, report, millis: 9.0 }));
    assert!(shown(&mut w, &format!("{nodes} nodes · 3 recomputed · 8.0 ms")), "the average of the last second");
    assert!(shown(&mut w, "VRAM 344 MB"));
    // A change that computed nothing (a node moved) is not an update of the graph.
    w.send(Message::Engine(Event::Finished { revision: 1, report: EvalReport { recomputed: 0, image_nodes: 17 }, millis: 0.1 }));
    assert!(shown(&mut w, &format!("{nodes} nodes · 3 recomputed · 8.0 ms")));
    // A second later nothing is left of those updates: nothing is computed now, in no time.
    for update in &mut w.app.res.updates {
        update.0 -= Duration::from_secs(2);
    }
    assert!(shown(&mut w, &format!("{nodes} nodes · 0 recomputed · 0.0 ms")));
    // A change that takes long says so, and for how long it has taken.
    w.app.res.busy_since = Some(Instant::now() - Duration::from_millis(1500));
    assert!(shown(&mut w, &format!("{nodes} nodes · computing… 1.5 s")));
}

/// The panels come back split where they were when the program was closed; a layout that
/// does not fit (another program's, or a broken file) gives way to the first one.
#[test]
fn the_panels_come_back_as_they_were() {
    let saved = [0.2, 0.45, 0.7, 0.55, 0.5, 0.3];
    assert_eq!(crate::app::splits_of(&crate::app::panes(&saved)), saved.to_vec());
    let first = crate::app::splits_of(&crate::app::panes(&[]));
    assert_eq!(first.len(), saved.len());
    // (Five splits: the layout of a version without the Explorer.)
    for broken in [&[0.5][..], &[0.2, 0.7, 0.55, 0.5, 0.3], &[0.2, 0.45, 0.7, 0.55, 0.5, 1.5], &[0.2, 0.45, 0.7, 0.55, 0.5, 0.3, 0.4]] {
        assert_eq!(crate::app::splits_of(&crate::app::panes(broken)), first, "{broken:?}");
    }
    // Resized in the window, a panel is saved where it was left.
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let split = match w.app.ui.panes.layout() {
        iced::widget::pane_grid::Node::Split { id, .. } => *id,
        _ => panic!("the panels are split"),
    };
    w.send(Message::PaneResized(iced::widget::pane_grid::ResizeEvent { split, ratio: 0.25 }));
    assert_eq!(crate::app::splits_of(&w.app.ui.panes)[0], 0.25);
}

// ---------------------------------------------------------------------------- stress

/// Whether a message the widgets make stays inside the app: a file dialog, the eyedropper
/// (it looks at the screen) or quitting would reach the computer the test runs on.
fn stays_inside(message: &Message) -> bool {
    match message {
        Message::Open
        | Message::Save
        | Message::SaveAs
        | Message::ExportAll
        | Message::ExportNode(_)
        | Message::BrowseFile(..)
        | Message::Pick(_)
        | Message::Host(_)
        | Message::Quit
        | Message::SaveThen(_)
        | Message::Proceed(PendingAction::Quit)
        | Message::Preview(PreviewMessage::BrowseEnvironment)
        | Message::Explorer(
            ExplorerMessage::Save(_)
                | ExplorerMessage::SaveAs(_)
                | ExplorerMessage::LinkFiles(_)
                | ExplorerMessage::ImportFiles(_)
                | ExplorerMessage::SaveResourceAs(..)
                | ExplorerMessage::Locate(..)
                | ExplorerMessage::CollectFiles(_),
        ) => false,
        Message::FromMenu(inner) => stays_inside(inner),
        _ => true,
    }
}

/// One event, as the window gives it: to the widgets first, then, with whether they used it,
/// to the app's event subscription (which only listens to some). The messages the widgets
/// make go to the app, but not those that would reach outside the test.
fn feed(w: &mut Harness, event: iced::Event) {
    use iced::Event as E;
    if let E::Mouse(mouse::Event::CursorMoved { position }) = event {
        w.cursor = mouse::Cursor::Available(position);
    }
    let mut messages = Vec::new();
    let cursor = w.cursor;
    let statuses = w.frame(|ui, renderer| {
        ui.update(std::slice::from_ref(&event), cursor, renderer, &mut clipboard::Null, &mut messages).1
    });
    for m in messages.into_iter().filter(stays_inside) {
        w.send(m);
    }
    let status = statuses.first().copied().unwrap_or(iced::event::Status::Ignored);
    let listened = matches!(
        &event,
        E::Keyboard(_)
            | E::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::ButtonReleased(_))
            | E::Window(window::Event::Resized(_) | window::Event::FileDropped(_))
    );
    if listened && let Some(window) = w.app.main_window() {
        w.send(Message::Runtime(event, status, window));
    }
}

/// A small generator of numbers for the monkey: the same seed, the same session.
struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
    fn float(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (self.below(1 << 24) as f32 / (1 << 24) as f32) * (hi - lo)
    }
}

/// A key as the window reports it, with what it types when it types anything.
fn key_event(
    key: Key,
    physical: keyboard::key::Physical,
    modifiers: keyboard::Modifiers,
    pressed: bool,
) -> iced::Event {
    let text = match (&key, modifiers.control()) {
        (Key::Character(c), false) => Some(c.clone()),
        _ => None,
    };
    iced::Event::Keyboard(if pressed {
        keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: physical,
            location: keyboard::Location::Standard,
            modifiers,
            text,
            repeat: false,
        }
    } else {
        keyboard::Event::KeyReleased {
            key: key.clone(),
            modified_key: key,
            physical_key: physical,
            location: keyboard::Location::Standard,
            modifiers,
        }
    })
}

/// Where the sockets of the graph's nodes are in the window: outputs, then inputs.
fn sockets(w: &Harness) -> (Vec<Point>, Vec<Point>) {
    let graph = w.app.graph();
    let view = w.app.ui.graph_view;
    let t = view::graph::layout::Transform { pan: view.pan, zoom: view.zoom };
    let at = iced::Vector::new(view.bounds.x, view.bounds.y);
    let (mut outputs, mut inputs) = (Vec::new(), Vec::new());
    for node in &graph.nodes {
        let l = view::graph::layout::layout(graph, node, &t, iced::Vector::ZERO);
        outputs.extend(l.outputs.iter().map(|(_, p)| *p + at));
        inputs.extend(l.inputs.iter().map(|(_, p)| *p + at));
    }
    (outputs, inputs)
}

/// The events of a left drag from `a` to `b` in `parts` moves.
fn drag_events(a: Point, b: Point, parts: u64) -> Vec<iced::Event> {
    let mut events = vec![
        iced::Event::Mouse(mouse::Event::CursorMoved { position: a }),
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
    ];
    for i in 1..=parts.max(1) {
        let t = i as f32 / parts.max(1) as f32;
        events.push(iced::Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t),
        }));
    }
    events.push(iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)));
    events
}

/// What is wrong with the app's state, if anything, once an event is handled.
fn broken(app: &App) -> Option<String> {
    let g = app.graph();
    for n in &g.nodes {
        if !n.pos.iter().all(|v| v.is_finite()) {
            return Some(format!("node {} is at {:?}", n.id.0, n.pos));
        }
    }
    for l in &g.links {
        if g.node(l.from).is_none() || g.node(l.to).is_none() {
            return Some(format!("a link from {} to {} has no node at an end", l.from.0, l.to.0));
        }
    }
    let mut inputs = std::collections::HashSet::new();
    for l in &g.links {
        if !inputs.insert((l.to, l.to_socket.clone())) {
            return Some(format!("two links go into {} {}", l.to.0, l.to_socket));
        }
    }
    let v = app.ui.graph_view;
    if !(v.zoom.is_finite() && v.zoom > 0.0 && v.pan.x.is_finite() && v.pan.y.is_finite()) {
        return Some(format!("the graph view is at {:?} × {}", v.pan, v.zoom));
    }
    None
}

/// A monkey at the window: `steps` random moves, clicks, drags, wheel turns, keys and
/// shortcuts, typing, window sizes and dropped files, over the whole interface as it is laid
/// out, on the demo; every few steps a frame is drawn. It stops at the first panic or broken
/// state, and says what it did last.
fn monkey(seed: u64, steps: usize) -> Result<usize, String> {
    use keyboard::key::{Code, Named, Physical};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let Some(mut w) = Harness::new(Box::new(view::root), Size::new(1680.0, 1000.0)) else { return Ok(0) };
    let mut dice = Dice(seed.max(1));
    let mut log: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let files = std::env::temp_dir().join(format!("brewed-monkey-{seed}"));
    let _ = std::fs::create_dir_all(&files);
    let image = files.join("dropped.png");
    image::RgbaImage::from_pixel(64, 32, image::Rgba([200, 100, 50, 255])).save(&image).expect("a picture to drop");
    let broken_file = files.join("broken.png");
    std::fs::write(&broken_file, b"not a picture").expect("a broken file");
    let graph_file = files.join("graph.brewtex");
    let start = std::time::Instant::now();
    let letters = "abcdefghijklmnopqrstuvwxyz0123456789 .,-+*/()=";
    let shortcut_keys: [(char, Code); 9] = [
        ('z', Code::KeyZ),
        ('y', Code::KeyY),
        ('c', Code::KeyC),
        ('x', Code::KeyX),
        ('v', Code::KeyV),
        ('d', Code::KeyD),
        ('a', Code::KeyA),
        ('b', Code::KeyB),
        ('n', Code::KeyN),
    ];
    let named = [
        Named::Enter,
        Named::Escape,
        Named::Tab,
        Named::Backspace,
        Named::Delete,
        Named::ArrowLeft,
        Named::ArrowRight,
        Named::ArrowUp,
        Named::ArrowDown,
        Named::Home,
        Named::End,
        Named::PageUp,
        Named::PageDown,
        Named::Space,
        Named::F1,
    ];
    for step in 0..steps {
        let size = w.size;
        let mut events: Vec<iced::Event> = Vec::new();
        let mut direct: Vec<Message> = Vec::new();
        let what: String;
        let roll = dice.below(1000);
        let here = w.cursor.position().unwrap_or(Point::ORIGIN);
        let anywhere =
            |dice: &mut Dice| Point::new(dice.float(-10.0, size.width + 10.0), dice.float(-10.0, size.height + 10.0));
        let moved = |p: Point| iced::Event::Mouse(mouse::Event::CursorMoved { position: p });
        match roll {
            0..=299 => {
                let p = if dice.chance(20) {
                    Point::new(here.x + dice.float(-30.0, 30.0), here.y + dice.float(-30.0, 30.0))
                } else {
                    anywhere(&mut dice)
                };
                what = format!("move to {:.0},{:.0}", p.x, p.y);
                events.push(moved(p));
            }
            300..=449 => {
                let p = anywhere(&mut dice);
                what = format!("click at {:.0},{:.0}", p.x, p.y);
                events.extend([
                    moved(p),
                    iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                    iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                ]);
            }
            450..=499 => {
                let p = anywhere(&mut dice);
                what = format!("double click at {:.0},{:.0}", p.x, p.y);
                for _ in 0..2 {
                    events.extend([
                        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                    ]);
                }
                events.insert(0, moved(p));
            }
            500..=599 => {
                let button = [mouse::Button::Left, mouse::Button::Left, mouse::Button::Middle, mouse::Button::Right]
                    [dice.below(4) as usize];
                let (a, b) = (anywhere(&mut dice), anywhere(&mut dice));
                what = format!("{button:?} drag {:.0},{:.0} to {:.0},{:.0}", a.x, a.y, b.x, b.y);
                events.push(moved(a));
                events.push(iced::Event::Mouse(mouse::Event::ButtonPressed(button)));
                let parts = 1 + dice.below(8);
                for i in 1..=parts {
                    let t = i as f32 / parts as f32;
                    events.push(moved(Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)));
                }
                events.push(iced::Event::Mouse(mouse::Event::ButtonReleased(button)));
            }
            600..=629 => {
                let p = anywhere(&mut dice);
                what = format!("right click at {:.0},{:.0}", p.x, p.y);
                events.extend([
                    moved(p),
                    iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
                    iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right)),
                ]);
            }
            630..=709 => {
                let delta = if dice.chance(50) {
                    mouse::ScrollDelta::Lines { x: 0.0, y: dice.float(-6.0, 6.0) }
                } else {
                    mouse::ScrollDelta::Pixels { x: dice.float(-200.0, 200.0), y: dice.float(-400.0, 400.0) }
                };
                what = format!("wheel {delta:?}");
                events.push(iced::Event::Mouse(mouse::Event::WheelScrolled { delta }));
            }
            710..=829 => {
                let c = letters.chars().nth(dice.below(letters.len() as u64) as usize).unwrap();
                let shift = dice.chance(10);
                let text = if shift { c.to_ascii_uppercase() } else { c }.to_string();
                what = format!("type {text:?}");
                let physical = Physical::Unidentified(keyboard::key::NativeCode::Unidentified);
                let m = if shift { keyboard::Modifiers::SHIFT } else { keyboard::Modifiers::default() };
                events.push(key_event(Key::Character(text.clone().into()), physical, m, true));
                events.push(key_event(Key::Character(text.into()), physical, m, false));
            }
            830..=909 => {
                let k = named[dice.below(named.len() as u64) as usize];
                let m = [
                    keyboard::Modifiers::default(),
                    keyboard::Modifiers::default(),
                    keyboard::Modifiers::SHIFT,
                    keyboard::Modifiers::CTRL,
                ][dice.below(4) as usize];
                what = format!("{m:?} {k:?}");
                let physical = Physical::Unidentified(keyboard::key::NativeCode::Unidentified);
                events.push(key_event(Key::Named(k), physical, m, true));
                events.push(key_event(Key::Named(k), physical, m, false));
            }
            910..=959 => {
                let (c, code) = shortcut_keys[dice.below(shortcut_keys.len() as u64) as usize];
                let m = if dice.chance(20) {
                    keyboard::Modifiers::CTRL | keyboard::Modifiers::SHIFT
                } else {
                    keyboard::Modifiers::CTRL
                };
                what = format!("{m:?} {c}");
                events.push(key_event(Key::Character(c.to_string().into()), Physical::Code(code), m, true));
                events.push(key_event(Key::Character(c.to_string().into()), Physical::Code(code), m, false));
            }
            960..=969 => {
                let new = Size::new(dice.float(1000.0, 2560.0).round(), dice.float(640.0, 1440.0).round());
                what = format!("window {new:?}");
                w.size = new;
                events.push(iced::Event::Window(window::Event::Resized(new)));
            }
            970..=979 => {
                let path =
                    [image.clone(), broken_file.clone(), files.join("missing.png"), files.clone(), graph_file.clone()]
                        [dice.below(5) as usize]
                        .clone();
                what = format!("drop {}", path.display());
                events.push(iced::Event::Window(window::Event::FileDropped(path)));
            }
            980..=984 => {
                // On purpose: from an output to an input, from the library onto the graph, or
                // along a field of the panels.
                let (outputs, inputs) = sockets(&w);
                let graph_area = w.app.ui.graph_view.bounds;
                match dice.below(3) {
                    0 if !outputs.is_empty() && !inputs.is_empty() => {
                        let a = outputs[dice.below(outputs.len() as u64) as usize];
                        let b = inputs[dice.below(inputs.len() as u64) as usize];
                        what = format!("connect {:.0},{:.0} to {:.0},{:.0}", a.x, a.y, b.x, b.y);
                        events.extend(drag_events(a, b, 1 + dice.below(4)));
                    }
                    1 => {
                        let kinds = NodeKind::ALL;
                        let kind = kinds[dice.below(kinds.len() as u64) as usize];
                        let found = texts(&mut w, kind.def().name);
                        match found.first() {
                            Some(r) => {
                                let to = Point::new(
                                    dice.float(graph_area.x, graph_area.x + graph_area.width.max(1.0)),
                                    dice.float(graph_area.y, graph_area.y + graph_area.height.max(1.0)),
                                );
                                what = format!("add {} at {:.0},{:.0}", kind.def().name, to.x, to.y);
                                events.extend(drag_events(r.center(), to, 1 + dice.below(4)));
                            }
                            None => what = format!("{} not in the library", kind.def().name),
                        }
                    }
                    _ => {
                        let a = anywhere(&mut dice);
                        let b = Point::new(a.x + dice.float(-300.0, 300.0), a.y + dice.float(-4.0, 4.0));
                        what = format!("scrub {:.0},{:.0} by {:.0}", a.x, a.y, b.x - a.x);
                        events.extend(drag_events(a, b, 2 + dice.below(10)));
                    }
                }
            }
            985..=989 => {
                let message = match dice.below(6) {
                    0 => Message::Undo,
                    1 => Message::Redo,
                    2 => Message::Proceed(PendingAction::LoadDemo),
                    3 => Message::Proceed(PendingAction::New),
                    4 => Message::SavePicked(Some(graph_file.clone())),
                    _ => Message::Proceed(PendingAction::OpenPath(graph_file.clone())),
                };
                what = format!("{message:?}");
                direct.push(message);
            }
            _ => {
                what = "a frame".into();
            }
        }
        if w.app.graph().nodes.len() < 4 && dice.chance(3) {
            direct.insert(0, Message::Proceed(PendingAction::LoadDemo));
        }
        log.push_back(format!("{step}: {what}"));
        if log.len() > 40 {
            log.pop_front();
        }
        let result = catch_unwind(AssertUnwindSafe(|| {
            for e in events {
                feed(&mut w, e);
            }
            for m in direct {
                w.send(m);
            }
            if step % 4 == 0 {
                draw_frame(&mut w, std::time::Instant::now());
            }
            broken(&w.app)
        }));
        let failure = match result {
            Err(panic) => Some(format!(
                "PANIC: {}",
                panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default()
            )),
            Ok(Some(state)) => Some(format!("BROKEN: {state}")),
            Ok(None) => None,
        };
        if step % 5000 == 4999 {
            w.snapshot(&format!("monkey_{seed}_{step}"));
        }
        if let Some(failure) = failure {
            let _ = std::fs::remove_dir_all(&files);
            return Err(format!(
                "seed {seed}, step {step}: {failure}\nlast steps:\n{}",
                log.iter().cloned().collect::<Vec<_>>().join("\n")
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&files);
    println!(
        "seed {seed}: {steps} steps in {:.1} s, {} nodes and {} links at the end",
        start.elapsed().as_secs_f64(),
        w.app.graph().nodes.len(),
        w.app.graph().links.len()
    );
    Ok(steps)
}

/// The monkey on a few seeds (see `monkey`).
#[test]
#[ignore = "a stress test"]
fn a_monkey_at_the_window() {
    let steps: usize = std::env::var("BREWED_MONKEY_STEPS").ok().and_then(|s| s.parse().ok()).unwrap_or(4000);
    let seeds: Vec<u64> = match std::env::var("BREWED_MONKEY_SEEDS") {
        Ok(s) => s.split(',').filter_map(|x| x.trim().parse().ok()).collect(),
        Err(_) => (1..=8).collect(),
    };
    let mut failures = Vec::new();
    for seed in seeds {
        if let Err(e) = monkey(seed, steps) {
            println!("{e}\n");
            failures.push(seed);
        }
    }
    assert!(failures.is_empty(), "the monkey broke the app with seeds {failures:?}");
}

/// A file can say a node is as far away as a float goes, or a comment as large: the graph
/// frames them, zooms on them and draws them (the file is held to what can be drawn).
#[test]
fn far_away_nodes_are_drawn() {
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let (mut g, _) = brewed_texture_core::demo::graph();
    for (i, n) in g.nodes.iter_mut().enumerate() {
        n.pos = [if i % 2 == 0 { 3e38 } else { -3e38 }, i as f32 * 1e37];
    }
    for c in &mut g.comments {
        c.pos = [-3e38, 1e30];
        c.size = [3e38, f32::MAX];
    }
    let path = std::env::temp_dir().join(format!("brewed-far-{}.brewtex", std::process::id()));
    let package = brewed_texture_core::Package::new("Far", g.clone());
    brewed_texture_core::project::save(&path, &package, &Default::default(), package.first()).expect("a file");
    w.send(Message::Proceed(PendingAction::OpenPath(path.clone())));
    let _ = std::fs::remove_file(&path);
    assert_eq!(w.app.graph().nodes.len(), g.nodes.len(), "the file opens");
    let theme = crate::style::theme();
    let middle = Point::new(800.0, 500.0);
    let ids: Vec<_> = w.app.graph().nodes.iter().map(|n| n.id).collect();
    let messages = [
        Message::FrameAll,
        Message::Graph(GraphMessage::ZoomAt { at: middle, factor: 1000.0 }),
        Message::Graph(GraphMessage::ZoomAt { at: middle, factor: 0.0001 }),
        Message::Graph(GraphMessage::PanBy(iced::Vector::new(1e6, -1e6))),
        Message::SelectAll,
        Message::Frame,
        Message::Edit(Edit::Duplicate { ids, offset: [1e7, 1e7] }),
        Message::FrameAll,
    ];
    for message in messages {
        let what = format!("{message:?}").chars().take(80).collect::<String>();
        w.send(message);
        let cursor = w.cursor;
        w.frame(|ui, renderer| {
            let redraw = iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()));
            let _ = ui.update(&[redraw], cursor, renderer, &mut clipboard::Null, &mut Vec::new());
            ui.draw(renderer, &theme, &renderer::Style { text_color: Color::WHITE }, cursor);
        });
        assert_eq!(broken(&w.app), None, "after {what}");
        let far = brewed_texture_core::graph::MAX_COORD;
        let g = w.app.graph();
        assert!(g.nodes.iter().flat_map(|n| n.pos).all(|p| p.abs() <= far), "after {what}");
        assert!(g.comments.iter().flat_map(|c| c.pos.into_iter().chain(c.size)).all(|p| p.abs() <= far), "after {what}");
    }
}

// ---------------------------------------------------------------------------- packages

/// The files a package uses are collected beside it: a picture from elsewhere is copied into
/// `<package>_files`, the node reads the copy (one step to undo), and saved, the package says
/// where the copy is relative to itself; a package without a file says it has nowhere to put
/// them.
#[test]
fn the_files_of_a_package_are_collected() {
    use brewed_texture_core::package;
    let mut w = harness!(view::root, 1680.0, 1000.0);
    // Unsaved, the demo has nowhere to put them.
    w.send(Message::Explorer(ExplorerMessage::CollectFiles(w.app.doc_key)));
    assert!(w.app.ui.status.as_ref().is_some_and(|s| s.error && s.text.contains("Save")), "{:?}", w.app.ui.status);

    let dir = scratch("collect");
    let stock = dir.join("stock");
    std::fs::create_dir_all(&stock).unwrap();
    let bricks = stock.join("bricks.png");
    image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 60, 40, 255])).save(&bricks).unwrap();
    let home = dir.join("wall");
    std::fs::create_dir_all(&home).unwrap();
    let file = home.join("wall.brewtex");
    w.send(Message::SavePicked(Some(file.clone())));
    w.send(Message::Edit(Edit::AddNodeWith {
        kind: NodeKind::BITMAP,
        pos: [0.0, 900.0],
        params: vec![("path".into(), ParamValue::Text(bricks.display().to_string()))],
    }));
    w.send(Message::Seal);
    let bitmap = w.app.graph().nodes_of(NodeKind::BITMAP).next().expect("the bitmap").id;
    let path_of = |w: &Harness| w.app.graph().node(bitmap).map(|n| n.text("path").to_string()).unwrap_or_default();

    // What the app copies (on a thread of its own, which the harness does not run), copied.
    let key = w.app.doc_key;
    let folder = home.join("wall_files");
    let plan = package::collect_plan(&w.app.doc.package().all_files(), &home, &folder, std::path::Path::exists);
    assert_eq!(plan, vec![(bricks.display().to_string(), folder.join("bricks.png").display().to_string())]);
    let collected = crate::app::packages::copy_files(&plan);
    assert!(collected.failed.is_empty(), "{:?}", collected.failed);
    w.send(Message::Explorer(ExplorerMessage::FilesCollected(key, collected)));
    let copy = folder.join("bricks.png");
    assert_eq!(path_of(&w), copy.display().to_string());
    assert!(copy.is_file());
    // Nothing left to collect; one undo reads the original again.
    assert!(package::collect_plan(&w.app.doc.package().all_files(), &home, &folder, std::path::Path::exists).is_empty());
    w.send(Message::Undo);
    assert_eq!(path_of(&w), bricks.display().to_string());
    w.send(Message::Redo);
    w.send(Message::SavePicked(Some(file.clone())));
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("wall_files/bricks.png"), "saved relative to itself: {text}");
    // A second copy is never written over a file that is there.
    let again = crate::app::packages::copy_files(&plan);
    assert!(again.copied.is_empty() && again.failed.len() == 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A folder of its own for a test's files.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("brewed-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a folder");
    dir
}

/// The keys of the packages open in the Explorer.
fn package_keys(app: &App) -> Vec<u64> {
    app.packages().into_iter().map(|(k, _)| k).collect()
}

/// Packages in the Explorer: the demo saved, a new package beside it using the demo's graph
/// as a node, both saved and opened again, "Open Reference" bringing the demo's package back
/// to the Explorer, and the demo's file moved: its user says so, and follows it once told
/// where it is.
#[test]
fn packages_and_graphs_used_as_nodes() {
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let dir = scratch("packages");
    let wall = dir.join("wall.brewtex");
    w.send(Message::SavePicked(Some(wall.clone())));
    let wall_key = w.app.doc_key;
    let wall_graph = w.app.doc.current();
    // A new package goes beside the saved one.
    w.send(Message::New);
    assert_eq!(package_keys(&w.app).len(), 2);
    assert_ne!(w.app.doc_key, wall_key);
    // The wall, dragged onto the new package's graph, is a node of it.
    let name = w.app.package_doc(wall_key).unwrap().entry().name.clone();
    let payload = DragPayload::Graph { key: wall_key, graph: wall_graph, name: name.clone() };
    w.send(Message::Graph(GraphMessage::Drop { payload, graph_pos: [0.0, 0.0] }));
    let node = w.app.graph().nodes.first().expect("a node that uses the wall").clone();
    let instance = node.instance.as_ref().expect("it uses a graph");
    assert_eq!(instance.graph.package.as_deref(), Some(wall.display().to_string().as_str()));
    assert_eq!(crate::view::graph::node_title(&node), name);
    // Its outputs are the wall's.
    let outputs = w.app.graph().output_sockets(&node).len();
    assert_eq!(outputs, w.app.package_doc(wall_key).unwrap().graph().nodes_of(NodeKind::OUTPUT).count());
    assert!(w.app.eval.node(node.id).is_some_and(|e| e.errors.is_empty()), "{:?}", w.app.eval.node(node.id));
    // Saved beside it, it says where the wall is relative to itself.
    let uses = dir.join("uses.brewtex");
    w.send(Message::SavePicked(Some(uses.clone())));
    let text = std::fs::read_to_string(&uses).expect("saved");
    assert!(text.contains("\"package\": \"wall.brewtex\""), "{text}");
    draw_frame(&mut w, std::time::Instant::now());

    // Opened again on its own, it finds the wall without the wall in the Explorer.
    for key in package_keys(&w.app) {
        w.send(Message::Explorer(ExplorerMessage::Close(key)));
    }
    assert_eq!(package_keys(&w.app).len(), 1);
    w.send(Message::OpenPicked(Some(uses.clone())));
    assert_eq!(package_keys(&w.app).len(), 1, "the empty package made room");
    assert!(w.app.ui.dialog.is_none());
    let node = w.app.graph().nodes[0].id;
    assert!(w.app.eval.node(node).is_some_and(|e| e.errors.is_empty()));
    // Open Reference: the wall's package comes to the Explorer, its graph edited.
    w.send(Message::OpenReference(node));
    assert_eq!(package_keys(&w.app).len(), 2);
    assert_eq!(w.app.doc.path(), Some(&wall));
    assert_eq!(w.app.doc.entry().name, name);
    draw_frame(&mut w, std::time::Instant::now());

    // The wall moves: its user asks where it is, and follows it once told.
    let moved = dir.join("moved");
    std::fs::create_dir_all(&moved).expect("a folder");
    std::fs::rename(&wall, moved.join("wall.brewtex")).expect("moved");
    for key in package_keys(&w.app) {
        w.send(Message::Explorer(ExplorerMessage::Close(key)));
    }
    w.send(Message::OpenPicked(Some(uses.clone())));
    let node = w.app.graph().nodes[0].id;
    assert!(w.app.eval.node(node).is_some_and(|e| e.error().is_some_and(|e| e.contains("cannot be found"))));
    let Some(Dialog::Missing(missing)) = w.app.ui.dialog.clone() else { panic!("the missing file is asked for: {:?}", w.app.ui.dialog) };
    assert_eq!(missing.items.len(), 1);
    draw_frame(&mut w, std::time::Instant::now());
    // Another package than the one that was there is not taken for it.
    let other = dir.join("other.brewtex");
    let package = brewed_texture_core::Package::new("Other", brewed_texture_core::Graph::default());
    brewed_texture_core::project::save(&other, &package, &Default::default(), package.first()).expect("a file");
    let old = missing.items[0].path.clone();
    w.send(Message::Explorer(ExplorerMessage::Located(missing.key, old.clone(), Some(other))));
    let Some(Dialog::Missing(still)) = &w.app.ui.dialog else { panic!("still asked for") };
    assert!(still.note.as_ref().is_some_and(|n| n.contains("has no graph")), "{:?}", still.note);
    w.send(Message::Explorer(ExplorerMessage::Located(missing.key, old, Some(moved.join("wall.brewtex")))));
    assert!(w.app.ui.dialog.is_none());
    assert!(w.app.doc.is_dirty(), "it says where the wall is now");
    assert!(w.app.eval.node(node).is_some_and(|e| e.errors.is_empty()), "{:?}", w.app.eval.node(node));
    // Undo: it says where the wall was again.
    w.send(Message::Undo);
    assert!(w.app.eval.node(node).is_some_and(|e| e.error().is_some()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Nodes of the demo made a graph of their own: the graph appears in the package, a node
/// that uses it takes their place with nothing wrong, the new graph opens from the Explorer
/// and is renamed there (the node follows), and undo puts everything back. Nodes with one
/// between them that is not chosen cannot go: the status bar says which.
#[test]
fn nodes_made_a_graph_of_their_own() {
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let of_kinds = |app: &App, kinds: &[NodeKind]| -> Vec<_> {
        app.graph().nodes.iter().filter(|n| kinds.contains(&n.kind)).map(|n| n.id).collect()
    };
    // The two Blends, and the Levels and the Blur between them; but the Gradient Map and the
    // Histogram Scan are between them too.
    w.app.ui.selection = of_kinds(&w.app, &[NodeKind::BLEND, NodeKind::LEVELS, NodeKind::BLUR]).into_iter().collect();
    w.send(Message::GraphFromSelection);
    assert_eq!(w.app.doc.package().graphs.len(), 1);
    let status = w.app.ui.status.as_ref().expect("a status");
    assert!(status.error && status.text.starts_with("Gradient Map, Histogram Scan are between"), "{}", status.text);
    // With them.
    let kinds = [NodeKind::BLEND, NodeKind::LEVELS, NodeKind::BLUR, NodeKind::GRADIENT_MAP, NodeKind::HISTOGRAM_SCAN];
    let chosen = of_kinds(&w.app, &kinds);
    w.app.ui.selection = chosen.iter().copied().collect();
    w.send(Message::GraphFromSelection);
    assert_eq!(w.app.doc.package().graphs.len(), 2);
    let node = *w.app.ui.selection.first().expect("the new node is selected");
    let graph = w.app.graph().node(node).and_then(|n| n.instance.as_ref()).expect("a node that uses the graph").graph.graph;
    assert!(chosen.iter().all(|id| !w.app.graph().contains(*id)));
    assert!(w.app.eval.node(node).is_some_and(|e| e.errors.is_empty()), "{:?}", w.app.eval.node(node));
    for out in w.app.graph().nodes_of(NodeKind::OUTPUT) {
        assert!(w.app.graph().link_into(out.id, "i:input").is_some());
    }
    draw_frame(&mut w, std::time::Instant::now());
    // The new graph, renamed in the Explorer: the node is called what it is called.
    let key = w.app.doc_key;
    w.send(Message::Explorer(ExplorerMessage::StartRename(key, graph)));
    w.send(Message::Explorer(ExplorerMessage::RenameText("Worn Bricks".into())));
    draw_frame(&mut w, std::time::Instant::now());
    w.send(Message::Explorer(ExplorerMessage::CommitRename));
    assert_eq!(crate::view::graph::node_title(w.app.graph().node(node).unwrap()), "Worn Bricks");
    // Opened from the Explorer, edited on its own; back, the node is where it was.
    w.send(Message::Explorer(ExplorerMessage::OpenGraph(key, graph)));
    assert_eq!(w.app.doc.current(), graph);
    assert!(w.app.graph().nodes_of(NodeKind::BLEND).next().is_some());
    draw_frame(&mut w, std::time::Instant::now());
    let first = w.app.doc.package().first();
    w.send(Message::Explorer(ExplorerMessage::OpenGraph(key, first)));
    assert!(w.app.graph().contains(node));
    // Undo, twice: the name, then the new graph.
    w.send(Message::Undo);
    w.send(Message::Undo);
    assert_eq!(w.app.doc.package().graphs.len(), 1);
    assert!(chosen.iter().all(|id| w.app.graph().contains(*id)));
}

/// A package's files are in its Resources folder, which is there only while it has files:
/// right in it while they are of one kind, in a folder of each kind once they are of more.
/// Each says whether the package links to it or holds it; what reads a file, and what uses a
/// graph, is in its notes; the files no node reads go with one click; files are imported
/// into the package, and linked ones held in it, every node reading them there.
#[test]
fn the_files_of_a_package_in_its_resources_folder() {
    use crate::app::Fold;
    use crate::view::explorer::{Uses, notes};
    use std::sync::Arc;
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let dir = scratch("resources");
    let (a, b) = (dir.join("a.png"), dir.join("b.png"));
    for p in [&a, &b] {
        image::RgbaImage::from_pixel(4, 4, image::Rgba([90, 60, 30, 255])).save(p).expect("a picture");
    }
    let key = w.app.doc_key;
    let shown = |w: &mut Harness, what: &str| !texts(w, what).is_empty();
    assert!(!shown(&mut w, "Resources"), "no files, no folder");
    // Pictures: right in Resources.
    w.send(Message::Explorer(ExplorerMessage::FilesPicked(key, vec![a.clone(), b.clone()])));
    assert!(shown(&mut w, "Resources") && shown(&mut w, "a.png") && !shown(&mut w, "Bitmaps"));
    // A font, imported (held by the package): a folder of each kind.
    let font = dir.join("Serif.ttf").display().to_string();
    let files = vec![(font.clone(), Arc::new(b"not really a font".to_vec()))];
    w.send(Message::Explorer(ExplorerMessage::FilesRead { key, embed: false, files, failed: Vec::new() }));
    assert!(shown(&mut w, "Bitmaps") && shown(&mut w, "Fonts") && shown(&mut w, "Serif.ttf"));
    let held = w.app.doc.package().resources.iter().find(|r| r.name() == "Serif.ttf").expect("the font");
    assert!(held.is_embedded());
    // Folded, the folder of a kind hides its files.
    w.send(Message::Explorer(ExplorerMessage::Toggle(Fold::Kind(key, brewed_texture_core::ResourceKind::Bitmap))));
    assert!(!shown(&mut w, "a.png") && shown(&mut w, "Serif.ttf"));
    w.send(Message::Explorer(ExplorerMessage::Toggle(Fold::Kind(key, brewed_texture_core::ResourceKind::Bitmap))));

    // What reads a file is in its notes; so is what uses a graph.
    let a_path = a.display().to_string();
    w.send(Message::Explorer(ExplorerMessage::AddResourceNode(a_path.clone(), brewed_texture_core::ResourceKind::Bitmap, None)));
    let graph = w.app.doc.entry().name.clone();
    let uses = Uses::of(&w.app);
    assert_eq!(notes(&uses.files[&(key, a_path.clone())], "Read by"), format!("Notes\n—\nRead by Bitmap in {graph}"));
    assert!(!uses.files.contains_key(&(key, b.display().to_string())), "b is read by nothing");
    let blur = w.app.graph().nodes_of(NodeKind::BLUR).next().expect("the demo blurs").id;
    w.app.ui.selection = [blur].into_iter().collect();
    w.send(Message::GraphFromSelection);
    let made = w.app.doc.package().graphs.last().expect("the graph made").id;
    assert_eq!(notes(&Uses::of(&w.app).graphs[&(key, made)], "Used by"), format!("Notes\n—\nUsed by {graph}"));
    draw_frame(&mut w, std::time::Instant::now());
    assert_eq!(broken(&w.app), None);

    // The files no node reads go: b and the font; a stays.
    for target in [ContextTarget::ResourcesFolder(key), ContextTarget::ResourceItem(key, font.clone())] {
        w.app.ui.context = Some(ContextMenu { at: Point::new(120.0, 200.0), target });
        draw_frame(&mut w, std::time::Instant::now());
    }
    w.app.ui.context = None;
    w.send(Message::Explorer(ExplorerMessage::RemoveUnused(key)));
    let names: Vec<String> = w.app.doc.package().resources.iter().map(|r| r.name()).collect();
    assert_eq!(names, ["a.png"]);
    assert!(!shown(&mut w, "Bitmaps"), "one kind again: no folder of it");

    // a, held in the package: the Bitmap reads it there.
    let bytes = Arc::new(std::fs::read(&a).expect("a"));
    w.send(Message::Explorer(ExplorerMessage::FilesRead { key, embed: true, files: vec![(a_path, bytes)], failed: Vec::new() }));
    let held = &w.app.doc.package().resources[0];
    assert!(held.is_embedded());
    let read = w.app.graph().nodes_of(NodeKind::BITMAP).next().map(|n| n.text("path").to_string());
    assert_eq!(read.as_deref(), Some(held.path.as_str()));
    assert!(shown(&mut w, "a.png"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The Explorer's rows and context menus, drawn with several packages, files that are there
/// and one that is not, and every menu of it open in turn.
#[test]
fn the_explorer_and_its_menus() {
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let dir = scratch("explorer");
    let picture = dir.join("bricks.png");
    image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 100, 50, 255])).save(&picture).expect("a picture");
    let key = w.app.doc_key;
    let gone = dir.join("gone.ttf");
    w.send(Message::Explorer(ExplorerMessage::FilesPicked(key, vec![picture.clone(), gone.clone(), dir.join("notes.txt")])));
    assert_eq!(w.app.doc.package().resources.len(), 2, "a picture and a font; not a text");
    w.send(Message::LoadDemo);
    w.send(Message::Explorer(ExplorerMessage::NewGraph(w.app.doc_key)));
    assert!(w.app.ui.renaming_graph.is_some(), "a new graph is named first");
    w.send(Message::Explorer(ExplorerMessage::CommitRename));
    let graph = w.app.doc.current();
    let targets = [
        ContextTarget::Package(key),
        ContextTarget::GraphItem(w.app.doc_key, graph),
        ContextTarget::ResourceItem(key, picture.display().to_string()),
        ContextTarget::ResourceItem(key, gone.display().to_string()),
        ContextTarget::ResourcesFolder(key),
    ];
    for target in targets {
        w.app.ui.context = Some(ContextMenu { at: Point::new(120.0, 200.0), target });
        draw_frame(&mut w, std::time::Instant::now());
        assert_eq!(broken(&w.app), None);
    }
    w.app.ui.context = None;
    // The picture, double-clicked in the Explorer, is a Bitmap node reading it.
    w.send(Message::Explorer(ExplorerMessage::Toggle(crate::app::Fold::Package(key))));
    let bitmap = brewed_texture_core::ResourceKind::Bitmap;
    w.send(Message::Explorer(ExplorerMessage::AddResourceNode(picture.display().to_string(), bitmap, None)));
    let read = w.app.graph().nodes_of(NodeKind::BITMAP).next().map(|n| n.text("path").to_string());
    assert_eq!(read, Some(picture.display().to_string()));
    // (A graph of an untitled package cannot be used in another: it has no file to be found by.)
    let untitled = w.app.doc_key;
    w.send(Message::New);
    let payload = DragPayload::Graph { key: untitled, graph, name: "Graph".into() };
    w.send(Message::Graph(GraphMessage::Drop { payload, graph_pos: [0.0, 0.0] }));
    assert!(w.app.graph().nodes.is_empty());
    assert!(w.app.ui.status.as_ref().is_some_and(|s| s.text.contains("Save")));
    draw_frame(&mut w, std::time::Instant::now());
    let _ = std::fs::remove_dir_all(&dir);
}
