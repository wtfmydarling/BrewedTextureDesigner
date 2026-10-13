//! Headless UI tests: the real views are built and driven with input events the way the
//! window does it, keeping the widget state between frames.
//!
//! With `BREWED_UI_SNAPSHOTS=<folder>` the tests also save what they draw as PNG files.

use brewed_texture_core::files::Held;
use brewed_texture_core::{Edit, NodeKind, ParamValue};
use iced::advanced::clipboard;
use iced::advanced::renderer::{self, Headless};
use iced::advanced::widget::operation::{self, Operation, Outcome, focusable};
use iced::keyboard::{self, Key};
use iced::{Color, Element, Pixels, Point, Size, mouse, window};
use iced_runtime::user_interface::{Cache, UserInterface};

use crate::app::{
    App, ColorTarget, ContextMenu, ContextTarget, Dialog, DragPayload, ExplorerMessage, GraphMessage, Message, PendingAction, PreviewMessage, ScreenPick,
    SelectMode, StartupOptions,
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
        Harness::started(view, size, StartupOptions::default())
    }

    /// [`Harness::new`], the program started with `opts`.
    fn started(view: View, size: Size, opts: StartupOptions) -> Option<Harness> {
        // The interface's fonts, as the program gives them to the window.
        crate::style::load_fonts();
        let renderer = futures::executor::block_on(<iced::Renderer as Headless>::new(crate::style::ui_font(), Pixels(13.0), None))?;
        let (mut app, _) = App::boot(opts);
        app.ui.window_size = size;
        // (As the window it opens in fits the side columns to its size.)
        app.fit_columns();
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

    /// Feed input events; returns the messages the widgets produced, not applied to the app.
    fn produced(&mut self, events: &[iced::Event]) -> Vec<Message> {
        let mut messages = Vec::new();
        let cursor = self.cursor;
        self.frame(|ui, renderer| {
            let _ = ui.update(events, cursor, renderer, &mut clipboard::Null, &mut messages);
        });
        messages
    }

    /// What the cursor looks like moved to `at` (what that move produces is applied).
    fn interaction(&mut self, at: Point) -> mouse::Interaction {
        self.cursor = mouse::Cursor::Available(at);
        let mut messages = Vec::new();
        let cursor = self.cursor;
        let state = self.frame(|ui, renderer| {
            let moved = iced::Event::Mouse(mouse::Event::CursorMoved { position: at });
            ui.update(&[moved], cursor, renderer, &mut clipboard::Null, &mut messages).0
        });
        for m in messages {
            self.send(m);
        }
        match state {
            iced_runtime::user_interface::State::Updated { mouse_interaction, .. } => mouse_interaction,
            iced_runtime::user_interface::State::Outdated => mouse::Interaction::None,
        }
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

    /// Look at another of the program's windows, `size` large: what `view` shows (the Color
    /// Editor's window, for one). The widgets of the one looked at before forget what they kept.
    fn look_at(&mut self, view: View, size: Size) {
        self.view = view;
        self.size = size;
        self.cache = Cache::default();
        self.cursor = mouse::Cursor::Unavailable;
    }

    /// What the window's event subscription reports.
    fn window_event(&mut self, window: window::Id, event: iced::Event) {
        self.send(Message::Runtime(event, iced::event::Status::Ignored, window));
    }

    fn main_window(&self) -> window::Id {
        self.app.main_window().expect("main window")
    }

    /// What the view draws: its pixels, RGBA, row by row (`size` wide and high).
    fn pixels(&mut self) -> Vec<u8> {
        let (cursor, size) = (self.cursor, self.size);
        let theme = crate::style::theme();
        self.frame(|ui, renderer| {
            let redraw = iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()));
            let _ = ui.update(&[redraw], cursor, renderer, &mut clipboard::Null, &mut Vec::new());
            ui.draw(renderer, &theme, &renderer::Style { text_color: Color::WHITE }, cursor);
            Headless::screenshot(renderer, iced::Size::new(size.width as u32, size.height as u32), 1.0, crate::style::CANVAS)
        })
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

// (After the macro: the tests of each part of the window, which use it.)
mod dialogs;
mod graph_canvas;
mod menus;
mod panels;
mod text_fits;
mod title_bar;

/// A Math node reads the size of the texture as Substance Designer's functions read `$size`:
/// `width` and `height` are built in. Typed into its expression in Properties, they take no
/// socket on the node (the graph has none to connect), Properties says each is the texture's
/// size and how large it is, and its list of functions names them; the value follows the
/// graph's size, and so does a parameter it drives, at once.
#[test]
fn math_reads_the_size_of_the_texture() {
    use brewed_texture_core::graph::SocketRef;
    use brewed_texture_core::types::Value;
    let mut panel = harness!(view::properties::view, 420.0, 1400.0);
    panel.send(Message::Edit(Edit::SetResolution([2048, 1024])));
    panel.send(Message::Edit(Edit::AddNode { kind: NodeKind::MATH, pos: [1600.0, -600.0] }));
    let math = panel.app.graph().nodes_of(NodeKind::MATH).last().expect("the new Math").id;
    panel.send(Message::Edit(Edit::SetParam { node: math, key: "expression".into(), value: ParamValue::Text("width / 8".into()) }));
    panel.send(Message::Seal);
    panel.send(Message::Graph(GraphMessage::Select { node: math, mode: SelectMode::Replace }));
    panel.input(&[]);
    // Typed on, in the editor.
    assert!(matches!(panel.operate(focusable::focus::<()>(editor_id())), Outcome::None));
    assert!(panel.input(&typing(" + height / 1024")) > 0, "the editor takes what is typed");
    assert_eq!(panel.app.graph().node(math).unwrap().text("expression"), "width / 8 + height / 1024");
    let value = |w: &Harness| w.app.eval.node(math).and_then(|e| e.value);
    assert_eq!(value(&panel), Some(Value::Float(257.0)));
    // No socket: nothing on the node to connect, and Properties says what each is.
    let node = panel.app.graph().node(math).unwrap();
    assert!(node.expression_vars().is_empty() && panel.app.graph().input_sockets(node).is_empty(), "no inputs on the node");
    let said = texts(&mut panel, "the texture's size, in pixels (built in)");
    assert_eq!(said.len(), 2, "width and height, each said built in");
    assert!(!texts(&mut panel, "2048").is_empty() && !texts(&mut panel, "1024").is_empty(), "and how large it is");
    if !panel.app.ui.show_functions {
        panel.send(Message::ShowFunctions);
    }
    assert!(!texts(&mut panel, "Built-ins: width, height").is_empty(), "the list of functions names them");
    // A parameter it drives: a blur's radius, as large as the size says.
    panel.send(Message::Edit(Edit::AddNode { kind: NodeKind::BLUR, pos: [1900.0, -600.0] }));
    let blur = panel.app.graph().nodes_of(NodeKind::BLUR).last().expect("the new Blur").id;
    panel.send(Message::Edit(Edit::SetExposed { node: blur, key: "radius".into(), exposed: true }));
    panel.send(Message::Edit(Edit::Connect {
        a: SocketRef { node: math, key: "o:result".into(), output: true },
        b: SocketRef { node: blur, key: "p:radius".into(), output: false },
    }));
    let radius = |w: &Harness| w.app.eval.params(blur).map(|p| p.f("radius"));
    assert_eq!(radius(&panel), Some(257.0));
    // Another size: another value, at once.
    panel.send(Message::Edit(Edit::SetResolution([512, 512])));
    assert_eq!((value(&panel), radius(&panel)), (Some(Value::Float(64.5)), Some(64.5)));
    // Undone, the size it was.
    panel.send(Message::Undo);
    assert_eq!(value(&panel), Some(Value::Float(257.0)));
}

/// What decides what the other parameters of a light mean — a Plane Light's Placement, a Line
/// Light's Position Mode, as Color Mode — has no toggle in Properties that exposes it, where the
/// light's other rows have one at the same place (a click there exposes its hardness, and on its
/// placement or position mode, nothing); no edit exposes it, nor an Output's usage.
#[test]
fn the_modes_of_the_lights_cannot_be_exposed() {
    let mut panel = harness!(view::properties::view, 420.0, 1400.0);
    let add = |w: &mut Harness, kind: NodeKind| {
        w.send(Message::Edit(Edit::AddNode { kind, pos: [1900.0, -600.0] }));
        w.send(Message::Seal);
        let id = w.app.graph().nodes_of(kind).last().expect("the new node").id;
        w.send(Message::Graph(GraphMessage::Select { node: id, mode: SelectMode::Replace }));
        w.input(&[]);
        id
    };
    let row = |w: &mut Harness, label: &str| *texts(w, label).first().unwrap_or_else(|| panic!("the {label} row"));
    // Where a row's toggle is: from the right of the hardness row, the first place a click exposes it.
    let panel_light = add(&mut panel, NodeKind::HDRI_PLANE_LIGHT);
    let hardness = row(&mut panel, "Hardness");
    let mut toggle = None;
    for x in (300..420).rev().step_by(2) {
        click(&mut panel, Point::new(x as f32, hardness.center_y()));
        if panel.app.graph().node(panel_light).unwrap().is_exposed("hardness") {
            toggle = Some(x as f32);
            break;
        }
    }
    let x = toggle.expect("a toggle exposes the hardness");
    let tried = |w: &mut Harness, node, label: &str, key: &str| {
        let at = row(w, label);
        let before = w.app.doc.undo_label();
        click(w, Point::new(x, at.center_y()));
        assert!(!w.app.graph().node(node).unwrap().is_exposed(key), "no toggle exposes the {label}");
        assert_eq!(w.app.doc.undo_label(), before, "nothing was done");
        w.send(Message::Edit(Edit::SetExposed { node, key: key.into(), exposed: true }));
        assert!(!w.app.graph().node(node).unwrap().is_exposed(key), "nor an edit");
    };
    tried(&mut panel, panel_light, "Placement", "placement");
    tried(&mut panel, panel_light, "Color Mode", "color_mode");
    let tube = add(&mut panel, NodeKind::HDRI_LINE_LIGHT);
    tried(&mut panel, tube, "Position Mode", "position_mode");
    let out = panel.app.graph().nodes_of(NodeKind::OUTPUT).map(|n| n.id).next().expect("an output");
    panel.send(Message::Edit(Edit::SetExposed { node: out, key: "usage".into(), exposed: true }));
    assert!(!panel.app.graph().node(out).unwrap().is_exposed("usage"), "an Output's usage is set");
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

/// The program of an FX-Map is typed as an expression is, a line after another: what it reads
/// and never sets becomes an input of the node, and where it is wrong it says so.
#[test]
fn programs_of_fx_maps_are_typed_line_after_line() {
    let mut panel = harness!(view::properties::view, 420.0, 1600.0);
    panel.send(Message::Edit(Edit::AddNode { kind: NodeKind::FX_MAP, pos: [0.0, 0.0] }));
    let fx = panel.app.graph().nodes_of(NodeKind::FX_MAP).next().expect("an FX-Map added").id;
    panel.send(Message::Graph(GraphMessage::Select { node: fx, mode: SelectMode::Replace }));
    panel.input(&[]);
    assert!(!texts(&mut panel, "PROGRAM").is_empty(), "a section of its program");
    assert!(matches!(panel.operate(focusable::focus::<()>(editor_id())), Outcome::None));
    assert_eq!(panel.focused(), Some(editor_id()), "the editor takes the focus");

    // A new line at the end, where typing goes on, and a statement on it.
    let enter = Key::Named(keyboard::key::Named::Enter);
    let physical = keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified);
    let pressed = iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: enter.clone(),
        modified_key: enter,
        physical_key: physical,
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::default(),
        text: Some("\n".into()),
        repeat: false,
    });
    assert!(panel.input(&[pressed]) > 0);
    assert!(panel.input(&typing("opacity = mask")) > 0);
    let program = |panel: &Harness| panel.app.graph().node(fx).unwrap().text("program").to_string();
    assert!(program(&panel).ends_with("\nopacity = mask"), "{:?}", program(&panel));
    let error = |panel: &Harness| panel.app.eval.node(fx).and_then(|e| e.error().map(str::to_string));
    assert_eq!(error(&panel), None);
    let sockets = |panel: &Harness| {
        let g = panel.app.graph();
        g.input_sockets(g.node(fx).unwrap()).into_iter().map(|s| s.key).collect::<Vec<_>>()
    };
    assert!(sockets(&panel).contains(&"v:mask".to_string()), "{:?}", sockets(&panel));
    // Wrong while it is typed, and the sockets stay; right again.
    panel.input(&typing(" *"));
    assert!(error(&panel).is_some_and(|e| e.contains("program is incorrect")), "{:?}", error(&panel));
    assert!(sockets(&panel).contains(&"v:mask".to_string()));
    assert_eq!(panel.focused(), Some(editor_id()), "the editor keeps the focus");
    panel.input(&typing(" 2"));
    assert_eq!(error(&panel), None);
    panel.snapshot("fx_map_program");
}

/// The graphs of the packages in a library folder, as deep as it goes, show in the Library
/// under the categories they say (not those kept out of it, nor those with no Output), found
/// by what they say they make; one added becomes a node that uses it, its package read then;
/// and a package changed on disk shows as it is when the folders are looked at again.
#[test]
fn library_folders_offer_their_graphs() {
    use brewed_texture_core::project;
    use brewed_texture_core::{Graph, GraphAttributes, package::Package};
    let dir = std::env::temp_dir().join(format!("brewed-library-{}", std::process::id()));
    let deeper = dir.join("metals");
    std::fs::create_dir_all(&deeper).unwrap();
    let file = deeper.join("rust.brewtex");
    let write = |description: &str| {
        let mut rust = Graph::default();
        rust.add_node(NodeKind::OUTPUT, [0.0, 0.0]);
        rust.attributes = GraphAttributes { description: description.into(), category: "Metals/Rust".into(), hidden: false };
        let mut package = Package::new("Rust", rust);
        let mut helper = Graph::default();
        helper.add_node(NodeKind::OUTPUT, [0.0, 0.0]);
        helper.attributes.hidden = true;
        package.add_graph("Helper", helper);
        package.add_graph("Nothing", Graph::default());
        project::save(&file, &package, &std::collections::HashMap::new(), package.first()).unwrap();
    };
    write("Orange rust");
    std::fs::write(dir.join("broken.brewtex"), b"no package").unwrap();

    let mut w = harness!(view::library::view, 300.0, 1400.0);
    assert!(w.app.library.graphs.is_empty(), "no folder: nothing but the program's own nodes");
    let mut settings = w.app.settings.clone();
    settings.library_folders = vec![dir.display().to_string()];
    w.app.set_settings(settings);
    w.send(Message::LookAtLibrary);
    let graphs = w.app.library.graphs.clone();
    assert_eq!(graphs.len(), 1, "{graphs:?}");
    assert_eq!((graphs[0].name.as_str(), graphs[0].category.as_str(), graphs[0].description.as_str()), ("Rust", "Metals/Rust", "Orange rust"));
    assert_eq!(graphs[0].package, file.display().to_string());
    assert_eq!(w.app.library.problems.len(), 1, "what is no package says so: {:?}", w.app.library.problems);
    w.input(&[]);
    assert!(!texts(&mut w, "METALS/RUST").is_empty() && !texts(&mut w, "Rust").is_empty(), "in the Library, under its category");
    w.send(Message::LibrarySearch("orange".into()));
    assert!(!texts(&mut w, "Rust").is_empty(), "found by what it says it makes");
    w.send(Message::LibrarySearch("copper".into()));
    assert!(texts(&mut w, "Rust").is_empty());
    w.send(Message::LibrarySearch(String::new()));

    // Added: a node that uses it, its package read.
    let used = |app: &App| app.graph().nodes_of(NodeKind::GRAPH_INSTANCE).count();
    let before = used(&w.app);
    let payload = DragPayload::Library { package: graphs[0].package.clone(), graph: graphs[0].graph, name: "Rust".into() };
    w.send(Message::PanelAdd(payload.clone()));
    assert_eq!(used(&w.app), before + 1);
    let node = w.app.graph().nodes_of(NodeKind::GRAPH_INSTANCE).max_by_key(|n| n.id).unwrap();
    let instance = node.instance.as_deref().unwrap();
    assert_eq!(instance.graph.package.as_deref(), Some(graphs[0].package.as_str()));
    assert_eq!(instance.interface.description, "Orange rust");
    // Again: from the package read already.
    w.send(Message::PanelAdd(payload));
    assert_eq!(used(&w.app), before + 2);

    w.snapshot("library_panel");

    // Changed on disk: as it is now, once looked at again.
    std::thread::sleep(std::time::Duration::from_millis(20));
    write("Rust of iron");
    w.send(Message::LookAtLibrary);
    assert_eq!(w.app.library.graphs[0].description, "Rust of iron");

    // The page of the folders; and what a graph says of itself, with nothing in it chosen.
    let mut root = harness!(view::root, 1400.0, 900.0);
    let mut settings = root.app.settings.clone();
    settings.library_folders = vec![dir.display().to_string()];
    root.app.set_settings(settings);
    root.send(Message::LookAtLibrary);
    root.send(Message::Settings(crate::app::SettingsMessage::OpenPage(crate::settings::Page::Library)));
    root.input(&[]);
    assert!(!texts(&mut root, "Add Folder…").is_empty() && !texts(&mut root, "1 graph in the Library.").is_empty());
    root.snapshot("settings_library");
    root.send(Message::Settings(crate::app::SettingsMessage::Cancel));
    root.send(Message::Graph(GraphMessage::ClearSelection));
    root.send(Message::Edit(Edit::SetAttributes(GraphAttributes { description: "A brick wall".into(), category: "Walls".into(), hidden: false })));
    root.input(&[]);
    assert!(!texts(&mut root, "ATTRIBUTES").is_empty(), "what the graph says of itself");
    root.snapshot("graph_attributes");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Given no package to open, the program starts on the Home screen, with nothing being edited;
/// New Graph… makes one from a template in its place (and the Home screen gives way to the
/// editor); Home comes back, and Escape goes back to what is edited; Learn opens examples beside
/// it.
#[test]
fn the_program_starts_on_the_home_screen() {
    use crate::app::home::{HomeMessage, HomePage, NewGraphMessage};
    use brewed_texture_core::templates::{Example, Template};
    let opts = StartupOptions { home: true, ..StartupOptions::default() };
    let mut w = match Harness::started(Box::new(view::root), Size::new(1400.0, 900.0), opts) {
        Some(h) => h,
        None => return,
    };
    w.input(&[]);
    assert_eq!(w.app.ui.home, Some(HomePage::Home));
    assert!(w.app.blank(), "nothing is being edited");
    for said in [
        "Welcome to Brewed Texture Designer",
        "New Graph…",
        "Open…",
        "Start from a template",
        // The same cards as the window of a new graph's: the template, its category.
        "Base",
        "OpenPBR",
        "Studio",
        "HDRi",
        "Noise",
        "FX-Map",
        "More templates…",
        "Recent",
        "Learn",
        "Resources",
        "Shortcuts",
    ] {
        assert!(!texts(&mut w, said).is_empty(), "{said}");
    }
    w.snapshot("home");
    // Keys of the editor do nothing to what is behind it.
    let nodes = w.app.graph().nodes.len();
    w.send(Message::Paste);
    w.send(Message::Home(HomeMessage::Show(HomePage::Home)));
    assert_eq!(w.app.graph().nodes.len(), nodes);

    // A package made from a template, in place of the blank one.
    w.send(Message::NewGraph(NewGraphMessage::Open(None)));
    w.input(&[]);
    for said in [
        "New Graph",
        "Templates",
        "OpenPBR",
        "Base",
        "HDRi",
        "Studio",
        "Metallic Roughness",
        "Scan Processing",
        "Filters",
        "Empty",
        "Create graph in",
        "Create",
    ] {
        assert!(!texts(&mut w, said).is_empty(), "{said}");
    }
    w.snapshot("new_graph");
    w.send(Message::NewGraph(NewGraphMessage::Layout(home_screen::Layout::List)));
    w.input(&[]);
    w.snapshot("new_graph_list");
    w.send(Message::NewGraph(NewGraphMessage::Layout(home_screen::Layout::Grid)));
    w.send(Message::NewGraph(NewGraphMessage::Name("Rusty Metal".into())));
    // A panorama: square, in 32-bit floats; another template gives back what was set before.
    let before = w.app.ui.dialog.clone();
    w.send(Message::NewGraph(NewGraphMessage::Template(Template::StudioPanorama)));
    let Some(Dialog::NewGraph(panorama)) = w.app.ui.dialog.clone() else { panic!("the dialog") };
    assert_eq!((panorama.size[1], panorama.precision), (panorama.size[0], brewed_texture_core::Precision::Full));
    w.send(Message::NewGraph(NewGraphMessage::Template(Template::Pbr)));
    let (Some(Dialog::NewGraph(a)), Some(Dialog::NewGraph(b))) = (before, w.app.ui.dialog.clone()) else { panic!("the dialog") };
    assert_eq!((a.size, a.linked, a.precision), (b.size, b.linked, b.precision));
    w.send(Message::NewGraph(NewGraphMessage::Width(512)));
    let Some(Dialog::NewGraph(dialog)) = w.app.ui.dialog.clone() else { panic!("the dialog") };
    assert_eq!(dialog.size, [512, 512], "width and height together");
    w.send(Message::NewGraph(NewGraphMessage::Create));
    assert_eq!(w.app.ui.home, None, "in the editor");
    assert_eq!(w.app.packages().len(), 1, "in place of the blank one");
    assert_eq!(w.app.doc.entry().name, "Rusty Metal");
    assert_eq!(w.app.graph().resolution, [512, 512]);
    let outputs: Vec<String> = w.app.graph().nodes_of(NodeKind::OUTPUT).map(|n| n.text("name").to_string()).collect();
    assert_eq!(outputs, Template::Pbr.outputs());

    // A graph not square, in the package open: width and height apart.
    let key = w.app.doc_key;
    w.send(Message::NewGraph(NewGraphMessage::Open(None)));
    w.send(Message::NewGraph(NewGraphMessage::Into(Some(key))));
    w.send(Message::NewGraph(NewGraphMessage::Template(Template::GrayFilter)));
    w.send(Message::NewGraph(NewGraphMessage::Link(false)));
    w.send(Message::NewGraph(NewGraphMessage::Width(1024)));
    w.send(Message::NewGraph(NewGraphMessage::Height(256)));
    w.send(Message::NewGraph(NewGraphMessage::Name("Sharpen".into())));
    w.send(Message::NewGraph(NewGraphMessage::Create));
    assert_eq!((w.app.packages().len(), w.app.doc_key), (1, key), "in the package, not a new one");
    assert_eq!(w.app.doc.package().graphs.len(), 2);
    assert_eq!(w.app.graph().resolution, [1024, 256]);
    assert_eq!(w.app.graph().nodes_of(NodeKind::IMAGE_INPUT).count(), 1, "a filter takes an image");
    w.send(Message::Undo);
    assert_eq!(w.app.doc.package().graphs.len(), 1, "undone in one step");

    // Home again, and back with Escape.
    w.send(Message::Home(HomeMessage::Show(HomePage::Home)));
    assert!(!texts(&mut w, "Back to Untitled").is_empty());
    press(&mut w, keyboard::key::Named::Escape);
    assert_eq!(w.app.ui.home, None);
    // An example of Learn: in place of a package not changed yet (as any package opened),
    // beside one changed.
    w.send(Message::Edit(Edit::SetResolution([1024, 1024])));
    w.send(Message::Home(HomeMessage::Show(HomePage::Learn)));
    w.input(&[]);
    assert!(!texts(&mut w, "FX-Map programs").is_empty());
    w.snapshot("learn");
    w.send(Message::Home(HomeMessage::Example(Example::FxMaps)));
    assert_eq!(w.app.packages().len(), 2);
    assert_eq!(w.app.doc.package().graphs.len(), 5);
    assert_eq!(w.app.ui.home, None);

    // A template of the row, at once: a package of it, as the settings make new graphs.
    w.send(Message::Home(HomeMessage::Show(HomePage::Home)));
    w.send(Message::Home(HomeMessage::Screen(home_screen::Event::Start(Template::Empty.id().into()))));
    assert_eq!(w.app.ui.home, None);
    assert_eq!(w.app.packages().len(), 2, "in place of the example, untouched (as a package opened)");
    assert!(w.app.graph().nodes.is_empty());
    assert_eq!(w.app.graph().resolution, w.app.settings.graph_size);
    // The shortcuts, from Learn and from the resources.
    w.send(Message::Home(HomeMessage::Screen(home_screen::Event::Lesson("shortcuts".into()))));
    assert_eq!(w.app.ui.dialog, Some(Dialog::Shortcuts));
}

/// The packages opened last are on the Home screen, with the pictures their files have; one
/// opens there. A package saved has the thumbnail of its first output for a picture.
#[test]
fn packages_opened_last_have_their_pictures() {
    use crate::app::home::{HomeMessage, HomePage};
    use brewed_texture_core::project;
    let dir = std::env::temp_dir().join(format!("brewed-home-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let walls = dir.join("walls.brewtex");

    // Saved from the demo: its picture, the thumbnail of its first output.
    let mut w = harness!(view::root, 1400.0, 900.0);
    fake_thumbnails(&mut w.app);
    let demo = w.app.here();
    w.send(Message::Asked(demo, Box::new(Message::SavePicked(Some(walls.clone())))));
    let png = project::read_picture(&walls).expect("a picture");
    let picture = image::load_from_memory(&png).unwrap();
    assert_eq!((picture.width(), picture.height()), (64, 64));
    let (package, ..) = (project::load(&walls).unwrap().package, ());
    assert_eq!(package.graphs[0].name, brewed_texture_core::demo::NAME, "the package as it was");

    // On the Home screen, opened last, with its picture; and a file gone, said so.
    let opts = StartupOptions { home: true, ..StartupOptions::default() };
    let mut h = match Harness::started(Box::new(view::root), Size::new(1400.0, 900.0), opts) {
        Some(h) => h,
        None => return,
    };
    h.app.ui.recent = vec![walls.display().to_string(), dir.join("gone.brewtex").display().to_string()];
    h.send(Message::Home(HomeMessage::Show(HomePage::Home)));
    let recent = h.app.ui.home_recent.clone();
    assert_eq!(recent.len(), 2);
    assert!(recent[0].there && recent[0].picture.is_some() && recent[0].name == "walls");
    assert!(!recent[1].there && recent[1].picture.is_none());
    h.input(&[]);
    assert!(!texts(&mut h, "walls").is_empty() && !texts(&mut h, "Not found where it was").is_empty());
    h.snapshot("home_recent");
    // As a list: a column each, and sorted by the one clicked (again: the other way).
    h.send(Message::Home(HomeMessage::Screen(home_screen::Event::Layout(home_screen::Layout::List))));
    h.input(&[]);
    for said in ["Name", "Opened", "Modified", "Size", "Location"] {
        assert!(!texts(&mut h, said).is_empty(), "{said}");
    }
    h.snapshot("home_recent_list");
    h.send(Message::Home(HomeMessage::Screen(home_screen::Event::Sort(home_screen::Column::Name))));
    assert_eq!((h.app.ui.home_sort.by, h.app.ui.home_sort.descending), (home_screen::Column::Name, false));
    h.send(Message::Home(HomeMessage::Screen(home_screen::Event::Sort(home_screen::Column::Name))));
    assert!(h.app.ui.home_sort.descending);
    h.send(Message::Home(HomeMessage::Open(walls.display().to_string())));
    assert_eq!(h.app.ui.home, None);
    assert_eq!(h.app.packages().len(), 1, "in place of the blank one");
    assert_eq!(h.app.doc.path().map(|p| p.to_path_buf()), Some(walls.clone()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// File → Export… (Ctrl+E) asks what is written: the Outputs as they are (as the export wrote
/// them before there were presets), or what an engine expects of a material, the maps packed
/// as it packs them; the files are listed before they are written.
#[test]
fn exports_follow_the_preset_chosen() {
    use crate::app::export_dialog::ExportMessage as X;
    use brewed_texture_core::export_presets::Preset;
    let mut w = harness!(view::root, 1400.0, 900.0);
    let dir = std::env::temp_dir().join(format!("brewed-export-dialog-{}", std::process::id()));
    w.send(Message::Export(X::Open));
    let Some(Dialog::Export(dialog)) = w.app.ui.dialog.clone() else { panic!("the export dialog") };
    assert_eq!((dialog.preset, dialog.directx), (Preset::Outputs, false), "as before; the demo's normals are OpenGL's");
    // As they are: every Output, as the export always wrote them.
    let outputs = w.app.graph().nodes_of(NodeKind::OUTPUT).count();
    assert_eq!(w.app.export_plan(&dialog, &dir).len(), outputs);
    w.send(Message::Export(X::Preset(Preset::Unreal)));
    w.send(Message::Export(X::Folder(Some(dir.clone()))));
    w.input(&[]);
    for said in ["Export Brick Wall", "Unreal Engine", "Unity (HDRP)", "glTF 2.0", "T_Brick_Wall_ORM.png", "T_Brick_Wall_Normal.png"] {
        assert!(!texts(&mut w, said).is_empty(), "{said}");
    }
    w.snapshot("export");
    let Some(Dialog::Export(dialog)) = w.app.ui.dialog.clone() else { panic!() };
    let plan = w.app.export_plan(&dialog, &dir);
    let names: Vec<String> = plan.iter().map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["T_Brick_Wall_BaseColor.png", "T_Brick_Wall_Normal.png", "T_Brick_Wall_ORM.png", "T_Brick_Wall_Height.png"]);
    assert!(plan[1].1.said.contains("green turned over"), "OpenGL normals for Unreal: {}", plan[1].1.said);
    assert!(plan[2].1.said.ends_with("B 0 (no metallic)"), "no metallic map: the value written: {}", plan[2].1.said);
    w.send(Message::Export(X::Export));
    assert_eq!(w.app.ui.dialog, None);
    assert_eq!(w.app.res.exports_pending, 4);
    assert_eq!(w.app.ui.export_preset, Preset::Unreal, "offered next time");
    // Another package at once: the files are written all the same, and still waited for.
    w.send(Message::Proceed(PendingAction::New));
    assert_eq!(w.app.res.exports_pending, 4);
}

/// Each save keeps a version of the package, beside the settings and never beside it: File →
/// Package History… lists them, the newest first; one restored takes the package's place, not
/// saved yet (the unsaved changes it replaces kept as a version first); one opened as a copy is
/// untitled, beside it.
#[test]
fn saved_packages_keep_their_versions() {
    use crate::app::SettingsMessage;
    use crate::app::versions::VersionsMessage as V;
    use brewed_texture_core::versions::{Kind, Versions};
    let dir = scratch("versions");
    let opts = StartupOptions { versions: Some(dir.join("History")), ..StartupOptions::default() };
    let Some(mut w) = Harness::started(Box::new(view::root), Size::new(1280.0, 800.0), opts) else {
        eprintln!("no GPU for the headless renderer: test skipped");
        return;
    };
    let walls = dir.join("walls.brewtex");
    let blurs = |p: &brewed_texture_core::package::Package| p.graphs.iter().map(|g| g.graph.nodes_of(NodeKind::BLUR).count()).sum::<usize>();
    w.send(Message::SavePicked(Some(walls.clone())));
    let first = blurs(w.app.doc.package());
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::BLUR, pos: [0.0, 0.0] }));
    w.send(Message::Save);
    let mut beside: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    beside.sort();
    assert_eq!(beside, ["History", "walls.brewtex"], "nothing beside the package");

    // Changed, not saved; the first version restored.
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::BLUR, pos: [80.0, 0.0] }));
    w.send(Message::Versions(V::Open));
    let Some(Dialog::Versions(dialog)) = w.app.ui.dialog.clone() else { panic!("the history dialog") };
    assert_eq!(dialog.list.iter().map(|v| v.kind).collect::<Vec<_>>(), [Kind::Saved, Kind::Saved]);
    w.input(&[]);
    assert_eq!(texts(&mut w, "History of walls.brewtex").len(), 1);
    assert_eq!(texts(&mut w, "Restore").len(), 2);
    w.snapshot("versions");
    w.send(Message::Versions(V::Restore(dialog.list[1].file.clone())));
    assert_eq!(w.app.ui.dialog, None);
    assert!(w.app.doc.is_dirty() && w.app.doc.path() == Some(&walls), "in its place, not saved yet");
    assert_eq!(blurs(w.app.doc.package()), first, "as it was saved first");
    let versions = Versions::new(dir.join("History"));
    let kept = versions.of(&walls);
    assert_eq!(kept.iter().map(|v| v.kind).collect::<Vec<_>>(), [Kind::Replaced, Kind::Saved, Kind::Saved]);
    assert_eq!(blurs(&versions.load(&kept[0]).unwrap().package), first + 2, "the unsaved changes, kept");

    // Opened as a copy: untitled, beside it.
    w.send(Message::Versions(V::Open));
    let Some(Dialog::Versions(dialog)) = w.app.ui.dialog.clone() else { panic!("the history dialog") };
    w.send(Message::Versions(V::Copy(dialog.list[0].file.clone())));
    assert_eq!((w.app.doc.path(), blurs(w.app.doc.package())), (None, first + 2));
    assert_eq!(w.app.shelf.len(), 1, "beside the package");

    // How many are kept: Settings → Recovery.
    w.send(Message::Settings(SettingsMessage::Open));
    w.send(Message::Settings(SettingsMessage::Page(crate::settings::Page::Recovery)));
    w.input(&[]);
    assert!(!texts(&mut w, "Earlier versions").is_empty());
    w.snapshot("settings-recovery");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A version is read in the background: the package changed meanwhile is not replaced by it (what
/// changed would be lost, kept nowhere), and the status says why; unchanged, it is.
#[test]
fn a_package_changed_while_a_version_is_read_stays() {
    use crate::app::versions::VersionsMessage as V;
    use brewed_texture_core::versions::Versions;
    let dir = scratch("versions-race");
    let opts = StartupOptions { versions: Some(dir.join("History")), ..StartupOptions::default() };
    let Some(mut w) = Harness::started(Box::new(view::root), Size::new(1280.0, 800.0), opts) else {
        eprintln!("no GPU for the headless renderer: test skipped");
        return;
    };
    let walls = dir.join("walls.brewtex");
    w.send(Message::SavePicked(Some(walls.clone())));
    let saved = w.app.graph().nodes.len();
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::BLUR, pos: [0.0, 0.0] }));
    w.send(Message::Save);
    let versions = Versions::new(dir.join("History"));
    let first = versions.of(&walls).last().cloned().expect("the first save, kept");
    let read = |w: &Harness, revision| V::Read {
        key: w.app.doc_key,
        path: walls.clone(),
        restore: true,
        revision,
        time: first.time,
        result: versions.load(&first).map(std::sync::Arc::new).map_err(|e| e.to_string()),
    };
    // Asked for, then changed before it is read: it stays as it is.
    let asked = Some(w.app.doc.revision());
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::LEVELS, pos: [60.0, 0.0] }));
    let changed = w.app.graph().nodes.len();
    w.send(Message::Versions(read(&w, asked)));
    assert_eq!(w.app.graph().nodes.len(), changed, "not replaced");
    let said = w.app.ui.status.as_ref().map(|s| (s.text.clone(), s.error));
    assert!(said.as_ref().is_some_and(|(t, error)| t.starts_with("Nothing was restored") && *error), "{said:?}");
    // Unchanged since it was asked for: restored.
    let asked = Some(w.app.doc.revision());
    w.send(Message::Versions(read(&w, asked)));
    assert_eq!(w.app.graph().nodes.len(), saved, "as it was saved first");
    assert!(w.app.ui.opening.is_empty(), "nothing is being read any more");
    let _ = std::fs::remove_dir_all(&dir);
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

/// The color of a stop of a gradient: the stop chosen on the ramp, its square opens the Color
/// Editor on it, and the editor's square colors that stop.
#[test]
fn a_stop_of_a_gradient_takes_a_color_from_the_picker() {
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let map = w.app.graph().nodes_of(NodeKind::GRADIENT_MAP).next().expect("the demo colors by a gradient").id;
    w.send(Message::Graph(GraphMessage::Select { node: map, mode: SelectMode::Replace }));
    w.send(Message::GradientStop(map, "gradient".into(), 1));
    w.input(&[iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))]);
    let stop = |w: &Harness| gradient_of(&w.app, map).stops[1].color;
    let target = ColorTarget::GradientStop(map, "gradient".into());
    let area = iced::Rectangle::new(Point::new(w.size.width - 400.0, 0.0), Size::new(400.0, w.size.height));
    let square = panels::find_click(&mut w, area, |m| matches!(m, Message::ColorPicker(Some(t)) if *t == target)).expect("the square of the stop");
    click(&mut w, square);
    assert_eq!(w.app.ui.color_picker, Some(target));
    let editor = w.app.color_window().expect("the editor's window");
    assert!(w.app.title(editor).ends_with("stop 2"), "which says the stop it edits: {}", w.app.title(editor));
    // Dragging in the editor's square colors the stop.
    w.look_at(Box::new(view::editors::color_editor), crate::view::editors::COLOR_EDITOR);
    w.snapshot("gradient_stop_picker");
    let before = stop(&w);
    let square = Point::new(110.0, 160.0);
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

    // Selected again by its title bar, Delete removes the frame and keeps the nodes. (As
    // small as it can be, it is a third of its width in, whatever the zoom.)
    let title = Point::new(rect.x + rect.width / 3.0, rect.y + 4.0);
    drag(&mut w, title, title);
    assert_eq!(w.app.ui.selected_comment, Some(id));
    // What it says under its title is typed in the properties panel: one step of undo.
    for c in "Rough mortar".chars() {
        w.send(Message::Description(id, iced::widget::text_editor::Action::Edit(iced::widget::text_editor::Edit::Insert(c))));
    }
    let said = |w: &Harness| w.app.graph().comment(id).expect("the comment").description.clone();
    assert_eq!(said(&w), "Rough mortar");
    w.input(&[]);
    assert!(!texts(&mut w, "Description").is_empty(), "the panel names it");
    w.send(Message::Undo);
    assert_eq!(said(&w), "");
    w.send(Message::Redo);
    assert_eq!(said(&w), "Rough mortar");
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

/// The Color Editor is a window of its own, the system's, kept with the main one (see
/// `crate::tool_window`): it opens as the editor is asked for and closes with it, whatever closes
/// it (its ×, Esc, the system's close, what it edits taken away); its square sets the color; a
/// click and a drag in the main window (the model turned in the 3D view) leave it open; its title
/// bar moves it and its edges size it, where the program draws them (the system does, asked by
/// them); it opens again where it was put, as large as it was made; its title says what it edits.
/// The keys that undo and redo do there what they do in the main window, and nothing else of the
/// main window's is done from it. (Drawn inside the main window, it could not be put elsewhere
/// nor sized, and the color was to be found again for each look at the model.)
#[test]
fn the_color_editor_is_a_window_of_its_own() {
    use crate::app::{ColorEditorMessage, ColorEditorPlace};
    use crate::view::editors::{COLOR_EDITOR, COLOR_EDITOR_MIN};
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let window = w.app.main_window().expect("the main window");
    // A node of its own: the demo's Uniform Color takes its color from an input socket.
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::UNIFORM_COLOR, pos: [0.0, 0.0] }));
    let uniform = w.app.ui.selection.iter().next().copied().expect("the new node");
    w.send(Message::Seal);
    let color = |w: &Harness| match w.app.graph().node(uniform).and_then(|n| n.value("color")) {
        Some(ParamValue::Color(c)) => *c,
        _ => panic!("no color"),
    };
    w.send(Message::Graph(GraphMessage::Select { node: uniform, mode: SelectMode::Replace }));
    w.input(&[iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))]);
    let nodes = w.app.graph().nodes.len();

    // Asked for, its window opens, saying what it edits; the main window draws nothing of it.
    let target = ColorTarget::Param(uniform, "color".into());
    w.send(Message::ColorPicker(Some(target.clone())));
    let editor = w.app.color_window().expect("its window");
    assert_ne!(editor, window);
    assert_eq!(w.app.title(editor), "Color Editor — Uniform Color · Color");
    assert!(texts_where(&mut w, |t| t.starts_with("Color Editor")).is_empty(), "nothing of it in the main window");
    // A click and a drag in the main window (the views, as the model is turned) leave it open.
    drag(&mut w, Point::new(500.0, 900.0), Point::new(560.0, 880.0));
    assert_eq!((w.app.ui.color_picker.clone(), w.app.color_window()), (Some(target.clone()), Some(editor)), "open, in the same window");

    // In its window: its square sets the color.
    w.look_at(Box::new(view::editors::color_editor), COLOR_EDITOR);
    let before = color(&w);
    let square = Point::new(110.0, 160.0);
    drag(&mut w, square, square + iced::Vector::new(-40.0, 20.0));
    assert_ne!(color(&w), before, "the square is not under the cursor");
    w.send(Message::Seal);
    // Ctrl+Z typed in it undoes; Delete there takes nothing away from the graph.
    let changed = color(&w);
    let typed = |w: &mut Harness, key: Key, physical: keyboard::key::Code, modifiers: keyboard::Modifiers| {
        let event = key_event(key, keyboard::key::Physical::Code(physical), modifiers, true);
        w.send(Message::Runtime(event, iced::event::Status::Ignored, editor));
    };
    typed(&mut w, Key::Character("z".into()), keyboard::key::Code::KeyZ, keyboard::Modifiers::CTRL);
    assert_eq!(color(&w), before, "undone");
    typed(&mut w, Key::Character("y".into()), keyboard::key::Code::KeyY, keyboard::Modifiers::CTRL);
    assert_eq!(color(&w), changed, "redone");
    typed(&mut w, Key::Named(keyboard::key::Named::Delete), keyboard::key::Code::Delete, keyboard::Modifiers::default());
    assert_eq!(w.app.graph().nodes.len(), nodes, "the selected node is still there");
    // Where the program draws it, its title bar moves the window, its corner sizes it.
    if crate::app::OWN_TITLE_BAR {
        let bar = iced::Rectangle::new(Point::ORIGIN, Size::new(COLOR_EDITOR.width - 60.0, 28.0));
        assert!(panels::find_click(&mut w, bar, |m| matches!(m, Message::ColorEditor(ColorEditorMessage::Drag))).is_some(), "its title bar");
        w.hover(Point::new(COLOR_EDITOR.width - 2.0, COLOR_EDITOR.height - 2.0));
        let pressed = w.produced(&[iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))]);
        assert!(pressed.iter().any(|m| matches!(m, Message::ColorEditor(ColorEditorMessage::Resize(window::Direction::SouthEast)))), "its corner: {pressed:?}");
        w.mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    }
    // Its × closes it (the system's close does, where the system draws the title bar).
    if crate::app::OWN_TITLE_BAR {
        let bar = iced::Rectangle::new(Point::ORIGIN, Size::new(COLOR_EDITOR.width, 28.0));
        let close = panels::find_click(&mut w, bar, |m| matches!(m, Message::ColorPicker(None))).expect("its ×");
        click(&mut w, close);
    } else {
        w.window_event(editor, iced::Event::Window(window::Event::CloseRequested));
    }
    assert_eq!((w.app.ui.color_picker.clone(), w.app.color_window()), (None, None));
    assert_eq!(color(&w), changed, "closing it changes no color");

    // Where it was put and how large it was made, it opens again (the system says, as it closes).
    let place = ColorEditorPlace { at: Some(Point::new(40.0, 50.0)), size: Size::new(700.0, 420.0), pixels: None };
    w.send(Message::ColorEditor(ColorEditorMessage::Placed(place)));
    let settings = crate::app::color_editor_settings(w.app.ui.color_editor_place);
    assert_eq!(settings.size, place.size);
    if !cfg!(windows) {
        assert!(matches!(settings.position, window::Position::Specific(p) if p == Point::new(40.0, 50.0)), "{:?}", settings.position);
    }
    assert!(settings.resizable && settings.min_size == Some(COLOR_EDITOR_MIN) && settings.decorations != crate::app::OWN_TITLE_BAR);
    // (Never smaller than it may be.)
    let tiny = ColorEditorPlace { size: Size::new(10.0, 10.0), ..place };
    assert_eq!(crate::app::color_editor_settings(Some(tiny)).size, COLOR_EDITOR_MIN);
    // Closed and asked for at once, it is open again before the system has said where it was:
    // said then, that is where it is from now on (and its window is put there: it had opened
    // where it was the time before).
    w.send(Message::ColorPicker(Some(target.clone())));
    let again = w.app.color_window().expect("open again");
    w.send(Message::ColorPicker(None));
    w.send(Message::ColorPicker(Some(target.clone())));
    let at_once = w.app.color_window().expect("open at once");
    assert_ne!(again, at_once, "a window of its own each time");
    let moved = ColorEditorPlace { at: Some(Point::new(900.0, 300.0)), size: Size::new(640.0, 400.0), pixels: None };
    w.send(Message::ColorEditor(ColorEditorMessage::Placed(moved)));
    assert_eq!(w.app.ui.color_editor_place, Some(moved));
    assert_eq!(w.app.color_window(), Some(at_once), "the window it has stays open");
    w.send(Message::ColorPicker(None));
    w.send(Message::ColorEditor(ColorEditorMessage::Placed(place)));

    // Esc typed in it closes it, and so does the system's close.
    w.send(Message::ColorPicker(Some(target.clone())));
    let editor = w.app.color_window().expect("open again");
    w.send(Message::Runtime(escape(), iced::event::Status::Ignored, editor));
    assert_eq!((w.app.ui.color_picker.clone(), w.app.color_window()), (None, None), "Esc closes it");
    w.send(Message::ColorPicker(Some(target.clone())));
    let editor = w.app.color_window().expect("open again");
    w.window_event(editor, iced::Event::Window(window::Event::CloseRequested));
    assert_eq!((w.app.ui.color_picker.clone(), w.app.color_window()), (None, None), "closed by the system");
    // What it edits taken away (an undo of the node), it closes.
    w.send(Message::ColorPicker(Some(target.clone())));
    assert!(w.app.color_window().is_some());
    for _ in 0..8 {
        if w.app.graph().node(uniform).is_some() {
            w.send(Message::Undo);
        }
    }
    assert_eq!((w.app.ui.color_picker.clone(), w.app.color_window()), (None, None), "closed with its node");
}

/// The Color Editor of a level of gray is the editor of colors (Substance Designer's has no
/// other): a color is picked as any is, and what is kept of it is its gray; the editor shows the
/// color picked (its square and sliders where they were put), the field of the parameter its
/// gray. The color as it was when the editor opened is under the one it is now, and a click on
/// it gives it back.
#[test]
fn a_gray_is_picked_in_the_editor_of_colors_and_the_color_before_comes_back() {
    let mut w = harness!(view::root, 1680.0, 1000.0);
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::UNIFORM_COLOR, pos: [0.0, 0.0] }));
    let uniform = w.app.ui.selection.iter().next().copied().expect("the new node");
    w.send(Message::Edit(Edit::SetParam { node: uniform, key: "output".into(), value: ParamValue::Enum(1) }));
    w.send(Message::Seal);
    let color = |w: &Harness| match w.app.graph().node(uniform).and_then(|n| n.value("color")) {
        Some(ParamValue::Color(c)) => *c,
        _ => panic!("no color"),
    };
    let target = ColorTarget::Param(uniform, "color".into());
    let was = color(&w);
    w.send(Message::ColorPicker(Some(target.clone())));
    w.look_at(Box::new(view::editors::color_editor), crate::view::editors::COLOR_EDITOR);
    // The same editor as of a color: its square, its hue bar, To Gray, the channels of colors.
    for said in ["Pick", "Invert", "To Gray", "Copy", "Paste", "Float"] {
        assert_eq!(texts(&mut w, said).len(), 1, "{said}");
    }
    assert!(texts(&mut w, "HSV").is_empty(), "every channel shown, none to switch to");
    // A red: kept as its gray, shown as the red it is.
    w.send(Message::SetColor(target.clone(), [1.0, 0.0, 0.0, 1.0]));
    w.send(Message::Seal);
    let gray = color(&w);
    assert!(gray[0] == gray[1] && gray[1] == gray[2] && (gray[0] - 0.2126).abs() < 1e-4, "the gray of red: {gray:?}");
    assert_eq!(w.app.color_shown(&target), Some([1.0, 0.0, 0.0, 1.0]), "the editor shows the red");
    w.snapshot("color_editor_gray");
    // Changed otherwise (an undo), it shows what it is.
    w.send(Message::Undo);
    assert_eq!(w.app.color_shown(&target), Some(was));
    w.send(Message::Redo);
    // The color before, under the one now: a click gives it back, a step of its own.
    let below = panels::find_click(
        &mut w,
        iced::Rectangle::new(Point::ORIGIN, crate::view::editors::COLOR_EDITOR),
        |m| matches!(m, Message::Step(inner) if matches!(&**inner, Message::SetColor(_, c) if *c == was)),
    )
    .expect("the color it was");
    click(&mut w, below);
    assert_eq!(color(&w), was, "given back");
    w.send(Message::Undo);
    assert_eq!(color(&w), gray, "a step of its own");
    // Opened again, what it was is what it is then.
    w.send(Message::ColorPicker(None));
    w.send(Message::ColorPicker(Some(target.clone())));
    assert_eq!(w.app.ui.color_before.as_ref().map(|(.., c)| *c), Some(gray));
}

/// The point of the Color Editor's square dragged past its bottom is at its bottom, under the
/// cursor: black, of the hue and the saturation it was dragged at. They are kept, which black
/// does not say: the point jumped to the square's corner, the sliders of hue and saturation
/// fell to 0, and the value raised again made a gray. Dragged back up, or its value raised, the
/// color is of that hue and saturation again; no channel is ever not a number.
#[test]
fn a_color_dragged_to_black_keeps_its_hue_and_saturation() {
    use crate::widget::color::rgb_to_hsv;
    let mut w = harness!(view::root, 1680.0, 1000.0);
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::UNIFORM_COLOR, pos: [0.0, 0.0] }));
    let uniform = w.app.ui.selection.iter().next().copied().expect("the new node");
    let target = ColorTarget::Param(uniform, "color".into());
    let color = |w: &Harness| match w.app.graph().node(uniform).and_then(|n| n.value("color")) {
        Some(ParamValue::Color(c)) => *c,
        _ => panic!("no color"),
    };
    let orange = 30.0 / 360.0;
    w.send(Message::SetColor(target.clone(), [1.0, 0.5, 0.0, 1.0]));
    w.send(Message::Seal);
    w.send(Message::ColorPicker(Some(target.clone())));
    w.look_at(Box::new(view::editors::color_editor), crate::view::editors::COLOR_EDITOR);
    // Pressed in the square, dragged down past its bottom and out of the window, several moves
    // in one frame as a fast mouse sends them.
    let (inside, below) = (Point::new(150.0, 150.0), Point::new(150.0, 900.0));
    w.hover(inside);
    w.mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    let pressed = w.app.hsv_shown(&target).expect("its hue, saturation and value");
    assert!((pressed[0] - orange).abs() < 1e-3 && pressed[1] > 0.3 && pressed[1] < 0.8, "the orange, less saturated: {pressed:?}");
    let moved = |p: Point| iced::Event::Mouse(mouse::Event::CursorMoved { position: p });
    w.cursor = mouse::Cursor::Available(below);
    w.input(&[moved(Point::new(150.0, 250.0)), moved(Point::new(150.0, 330.0)), moved(below)]);
    assert_eq!(color(&w), [0.0, 0.0, 0.0, 1.0], "black, at the bottom of the square");
    let bottom = w.app.hsv_shown(&target).expect("its hue, saturation and value");
    assert!((bottom[0] - orange).abs() < 1e-3 && (bottom[1] - pressed[1]).abs() < 1e-3 && bottom[2] == 0.0, "the hue and the saturation kept: {bottom:?}");
    // Back up into the square, the button still held: the orange again.
    w.hover(inside);
    let back = color(&w);
    assert!(back.iter().all(|x| x.is_finite()) && (rgb_to_hsv([back[0], back[1], back[2]])[0] - orange).abs() < 1e-3, "{back:?}");
    // Down again and let go there; the value raised by its slider: of that hue and saturation.
    w.hover(below);
    w.mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    w.send(Message::Seal);
    assert_eq!(color(&w), [0.0, 0.0, 0.0, 1.0]);
    let [h, s, v] = w.app.hsv_shown(&target).expect("its hue, saturation and value");
    assert!((h - orange).abs() < 1e-3 && (s - pressed[1]).abs() < 1e-3 && v == 0.0, "{:?}", [h, s, v]);
    w.send(Message::SetHsv(target.clone(), [h, s, 0.8]));
    let raised = color(&w);
    let [rh, rs, rv] = rgb_to_hsv([raised[0], raised[1], raised[2]]);
    assert!((rh - orange).abs() < 1e-3 && (rs - s).abs() < 1e-3 && (rv - 0.8).abs() < 1e-3, "the orange as dark, not a gray: {raised:?}");
    // The red, green and blue set to none by their sliders: black, and still of that hue.
    w.send(Message::SetColor(target.clone(), [0.0, 0.0, 0.0, 1.0]));
    assert!((w.app.hsv_shown(&target).expect("shown")[0] - orange).abs() < 1e-3);
    // Another color altogether (pasted, picked from the screen) is shown as it is.
    w.send(Message::SetColor(target.clone(), [0.0, 0.0, 1.0, 1.0]));
    assert!((w.app.hsv_shown(&target).expect("shown")[0] - 240.0 / 360.0).abs() < 1e-3);
    // No number that is not one sets anything a color cannot be.
    w.send(Message::SetHsv(target.clone(), [f32::NAN, f32::INFINITY, -4.0]));
    assert!(color(&w).iter().all(|x| x.is_finite() && (0.0..=1.0).contains(x)), "{:?}", color(&w));
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
    texts_where(w, |text| text == wanted).into_iter().map(|(_, bounds)| bounds).collect()
}

/// Where the view shows `full`: whole, or shortened in its middle to fit (see
/// [`view::shortened`]).
fn texts_reading(w: &mut Harness, full: &str) -> Vec<iced::Rectangle> {
    let reads = |text: &str| match text.split_once('…') {
        _ if text == full => true,
        Some((head, tail)) => head.len() + tail.len() < full.len() && full.starts_with(head) && full.ends_with(tail),
        None => false,
    };
    texts_where(w, reads).into_iter().map(|(_, bounds)| bounds).collect()
}

/// The texts of the view that `wanted` says are wanted, and where each is.
fn texts_where(w: &mut Harness, wanted: impl Fn(&str) -> bool + Send) -> Vec<(String, iced::Rectangle)> {
    struct Find<F> {
        wanted: F,
        found: Vec<(String, iced::Rectangle)>,
    }
    impl<F: Fn(&str) -> bool + Send> Operation for Find<F> {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }
        fn text(&mut self, _: Option<&iced::widget::Id>, bounds: iced::Rectangle, text: &str) {
            if (self.wanted)(text) {
                self.found.push((text.to_owned(), bounds));
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

/// Where the scrollable of `id` shows what it holds.
fn scrollable_bounds(w: &mut Harness, id: &iced::widget::Id) -> Option<iced::Rectangle> {
    struct Find<'a> {
        id: &'a iced::widget::Id,
        found: Option<iced::Rectangle>,
    }
    impl Operation for Find<'_> {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }
        fn scrollable(
            &mut self,
            id: Option<&iced::widget::Id>,
            bounds: iced::Rectangle,
            _: iced::Rectangle,
            _: iced::Vector,
            _: &mut dyn iced::advanced::widget::operation::Scrollable,
        ) {
            if id == Some(self.id) {
                self.found = Some(bounds);
            }
        }
    }
    let mut find = Find { id, found: None };
    w.frame(|ui, renderer| ui.operate(renderer, &mut find));
    find.found
}

/// What pressing the left button at `at` asks for (the cursor moved there first), not applied;
/// the button is then let go (what that asks is not applied either).
fn pressed(w: &mut Harness, at: Point) -> Vec<Message> {
    w.hover(at);
    let asked = w.produced(&[iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))]);
    let _ = w.produced(&[iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))]);
    asked
}

/// What a click at `at` asks for, pressed and let go (the cursor moved there first), not applied.
fn clicked(w: &mut Harness, at: Point) -> Vec<Message> {
    clicked_with(w, at, mouse::Button::Left)
}

/// What a right click at `at` asks for, not applied.
fn right_clicked(w: &mut Harness, at: Point) -> Vec<Message> {
    clicked_with(w, at, mouse::Button::Right)
}

fn clicked_with(w: &mut Harness, at: Point, button: mouse::Button) -> Vec<Message> {
    w.hover(at);
    let mut asked = w.produced(&[iced::Event::Mouse(mouse::Event::ButtonPressed(button))]);
    asked.extend(w.produced(&[iced::Event::Mouse(mouse::Event::ButtonReleased(button))]));
    asked
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
    use crate::app::ChoiceMenu;
    use brewed_texture_core::nodes::NOISE_TYPES;

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

        // Every type is in the menu, what shows of it clear of the field; in a window too low for
        // all of them the menu scrolls, and what of it shows is inside the window.
        let window = iced::Rectangle::new(Point::ORIGIN, Size::new(width, height));
        let inside = |at: iced::Rectangle| at.x >= 0.0 && at.y >= 0.0 && at.x + at.width <= width && at.y + at.height <= height;
        let list = scrollable_bounds(&mut w, &view::choices::list_id()).expect("the menu's list");
        assert!(inside(list) && list.intersection(&field).is_none(), "the list {list:?}, the field {field:?} in {width}×{height}");
        for name in NOISE_TYPES {
            let at = *texts(&mut w, name).last().unwrap_or_else(|| panic!("{name} is not in the menu"));
            if at.intersection(&list).is_some() {
                assert!(at.intersection(&field).is_none(), "{name} covers the field: {at:?} over {field:?} in {width}×{height}");
            }
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
        let cellular = ["Cells", "Voronoi", "Voronoi Fractal", "Cracks", "Crystal", "Mosaic", "Stones"];
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
    // (A folder of its own: the test of the files of environment images has "environment".)
    let dir = scratch("lit-by-an-image");
    let sky = dir.join("sky.hdr");
    let pixels: Vec<image::Rgb<f32>> = (0..32u32).flat_map(|y| (0..64u32).map(move |x| image::Rgb([x as f32 / 16.0, 1.0 - y as f32 / 32.0, 2.0]))).collect();
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
    // (Nothing behind the model until asked.)
    for text in ["○  Studio", "●  sky.hdr", "○  Show It Behind the Model", "Blur"] {
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
    w.send(Message::Preview(P::Background(true)));
    assert!(w.app.ui.preview.lighting.background);
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
        | Message::Export(crate::app::export_dialog::ExportMessage::Browse)
        | Message::ExportNode(_)
        | Message::BrowseFile(..)
        | Message::Pick(_)
        | Message::Host(_)
        | Message::Quit
        // (The window's own close button quits as Quit does.)
        | Message::Window(crate::app::WindowAction::Close)
        | Message::SaveThen(_)
        | Message::Proceed(PendingAction::Quit)
        | Message::Preview(PreviewMessage::BrowseEnvironment)
        | Message::Settings(crate::app::SettingsMessage::AddLibraryFolder)
        | Message::Explorer(
            ExplorerMessage::Save(_)
                | ExplorerMessage::SaveAs(_)
                | ExplorerMessage::LinkFiles(_)
                | ExplorerMessage::ImportFiles(_)
                | ExplorerMessage::SaveResourceAs(..)
                | ExplorerMessage::Locate(..)
                | ExplorerMessage::CollectFiles(_),
        ) => false,
        Message::FromMenu(inner) | Message::Step(inner) => stays_inside(inner),
        _ => true,
    }
}

/// One event, as the window `window` gives it (the main one, else the Color Editor's): to the
/// widgets first, then, with whether they used it, to the app's event subscription (which only
/// listens to some). The messages the widgets make go to the app, but not those that would reach
/// outside the test.
fn feed(w: &mut Harness, event: iced::Event, window: Option<window::Id>) {
    use iced::Event as E;
    if let E::Mouse(mouse::Event::CursorMoved { position }) = event {
        w.cursor = mouse::Cursor::Available(position);
    }
    let mut messages = Vec::new();
    let cursor = w.cursor;
    let statuses = w.frame(|ui, renderer| ui.update(std::slice::from_ref(&event), cursor, renderer, &mut clipboard::Null, &mut messages).1);
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
    if listened && let Some(window) = window.or(w.app.main_window()) {
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
fn key_event(key: Key, physical: keyboard::key::Physical, modifiers: keyboard::Modifiers, pressed: bool) -> iced::Event {
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
        keyboard::Event::KeyReleased { key: key.clone(), modified_key: key, physical_key: physical, location: keyboard::Location::Standard, modifiers }
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
    let mut events = vec![iced::Event::Mouse(mouse::Event::CursorMoved { position: a }), iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))];
    for i in 1..=parts.max(1) {
        let t = i as f32 / parts.max(1) as f32;
        events.push(iced::Event::Mouse(mouse::Event::CursorMoved { position: Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t) }));
    }
    events.push(iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)));
    events
}

/// What is wrong with the app's state, if anything, once an event is handled.
fn broken(app: &App) -> Option<String> {
    use brewed_texture_core::graph::{MAX_COORD, NodeId};
    let g = app.graph();
    // What a file's graph is held to (as the core's test of mangled files has it): ids of nodes
    // and comments each its own, below the next one given; places within reach; a node that uses
    // a graph says which; comments of a color and a size; input values of numbers.
    let mut ids = std::collections::HashSet::new();
    for n in &g.nodes {
        if !n.pos.iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD) {
            return Some(format!("node {} is at {:?}", n.id.0, n.pos));
        }
        if !ids.insert(n.id) || n.id.0 >= g.next_id || n.id.is_inner() {
            return Some(format!("node id {} taken twice, or not below {}", n.id.0, g.next_id));
        }
        if (n.kind == NodeKind::GRAPH_INSTANCE) != n.instance.is_some() {
            return Some(format!("{} with {:?}", n.def().name, n.instance.as_ref().map(|i| &i.graph)));
        }
    }
    for c in &g.comments {
        if !ids.insert(NodeId(c.id.0)) || c.id.0 >= g.next_id {
            return Some(format!("comment id {} taken twice, or not below {}", c.id.0, g.next_id));
        }
        let place = c.pos.iter().chain(&c.size).all(|v| v.is_finite() && v.abs() <= MAX_COORD);
        if !place || !c.color.iter().all(|v| (0.0..=1.0).contains(v)) || c.size[0] < 0.0 || c.size[1] < 0.0 {
            return Some(format!("a comment at {:?}, {:?} large, of {:?}", c.pos, c.size, c.color));
        }
    }
    for v in &g.variables {
        let values = [v.value, v.default, v.min, v.max];
        if !values.iter().all(brewed_texture_core::Value::is_finite) {
            return Some(format!("the input value {} holds {values:?}", v.name));
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
    // Every link one the graph keeps: between sockets its nodes have, of types that go together,
    // or waiting for a socket that comes back (an expression's variable being typed, a graph's
    // output; see `Graph::prune_links`).
    let mut kept = g.clone();
    kept.prune_links();
    if let Some(l) = g.links.iter().find(|l| !kept.links.contains(l)) {
        let name = |id| g.node(id).map_or("?", |n| n.def().name);
        return Some(format!("a link from {} {} to {} {} is one the graph does not keep", name(l.from), l.from_socket, name(l.to), l.to_socket));
    }
    let v = app.ui.graph_view;
    if !(v.zoom.is_finite() && v.zoom > 0.0 && v.pan.x.is_finite() && v.pan.y.is_finite()) {
        return Some(format!("the graph view is at {:?} × {}", v.pan, v.zoom));
    }
    // What the interface points at is in the graph: what an edit took away, it lets go of.
    let ui = &app.ui;
    if let Some(id) = ui.selection.iter().find(|id| !g.contains(**id)) {
        return Some(format!("node {} is selected, and not in the graph", id.0));
    }
    if ui.selected_link.as_ref().is_some_and(|l| !g.links.contains(l)) {
        return Some("a link that is not in the graph is selected".into());
    }
    if let Some(id) = ui.selected_comment.filter(|id| g.comment(*id).is_none()) {
        return Some(format!("comment {id:?} is selected, and not in the graph"));
    }
    if let Some(id) = ui.preview.node.filter(|id| !g.contains(*id)) {
        return Some(format!("the 2D view shows node {}, which is not in the graph", id.0));
    }
    if let Some((id, _)) = ui.expression.as_ref().filter(|(id, _)| !g.contains(*id)) {
        return Some(format!("the expression edited is of node {}, which is not in the graph", id.0));
    }
    let variable = |name: &str| g.variables.iter().any(|v| v.name == name);
    match &ui.color_picker {
        Some(ColorTarget::Param(id, _) | ColorTarget::GradientStop(id, _)) if !g.contains(*id) => {
            return Some(format!("a color picker is open for node {}, which is not in the graph", id.0));
        }
        Some(ColorTarget::Comment(id)) if g.comment(*id).is_none() => {
            return Some(format!("a color picker is open for comment {id:?}, which is not in the graph"));
        }
        Some(ColorTarget::Variable(name, _)) if !variable(name) => {
            return Some(format!("a color picker is open for the input value {name}, which the graph has not"));
        }
        _ => {}
    }
    if let Some((name, _)) = ui.renaming_var.as_ref().filter(|(name, _)| !variable(name)) {
        return Some(format!("the input value {name} is being renamed, which the graph has not"));
    }
    if let Some(m) = ui.choice_menu.as_ref().filter(|m| g.node(m.node).is_none_or(|n| n.param_def(m.key).is_none())) {
        return Some(format!("the menu of the choice {} of node {} is open, which is not in the graph", m.key, m.node.0));
    }
    if let Some((id, _)) = ui.binding.as_ref().filter(|(id, _)| !g.contains(*id)) {
        return Some(format!("the input value of node {} is being typed, which is not in the graph", id.0));
    }
    if let Some((id, _)) = ui.description.as_ref().filter(|(id, _)| g.comment(*id).is_none()) {
        return Some(format!("the description of comment {id:?} is being typed, which is not in the graph"));
    }
    if let Some(socket) = ui.add_menu.as_ref().and_then(|m| m.pending.as_ref()).filter(|s| !g.contains(s.node)) {
        return Some(format!("the menu that adds a node connects it to node {}, which is not in the graph", socket.node.0));
    }
    // Every package open edits a graph it has (none, where it has none: a package may hold
    // files only); what the Explorer and its menus point at is open.
    let packages = app.packages();
    let package = |key: u64| packages.iter().find(|(k, _)| *k == key).map(|(_, doc)| *doc);
    for (key, doc) in &packages {
        if doc.has_graph() == doc.package().graphs.is_empty() {
            return Some(format!("package {key} edits a graph it has not, or none of those it has"));
        }
    }
    let graph_of = |key: u64, graph: brewed_texture_core::GraphId| package(key).is_some_and(|doc| doc.package().graph(graph).is_some());
    let file_of = |key: u64, path: &str| package(key).is_some_and(|doc| doc.package().resources.iter().any(|r| r.path == path));
    if let Some((key, graph, _)) = ui.renaming_graph.as_ref().filter(|(key, graph, _)| !graph_of(*key, *graph)) {
        return Some(format!("graph {graph:?} of package {key} is being renamed, which is not open"));
    }
    if let Some((key, path, _)) = ui.renaming_file.as_ref().filter(|(key, path, _)| !file_of(*key, path)) {
        return Some(format!("the file {path} of package {key} is being renamed, which it has not"));
    }
    if let Some(menu) = &ui.context {
        let there = match &menu.target {
            ContextTarget::Node(id) => g.contains(*id),
            ContextTarget::Link(l) => g.links.contains(l),
            ContextTarget::Comment(id) => g.comment(*id).is_some(),
            ContextTarget::Canvas { .. } | ContextTarget::Explorer => true,
            ContextTarget::Package(key) | ContextTarget::ResourcesFolder(key) => package(*key).is_some(),
            ContextTarget::GraphItem(key, graph) => graph_of(*key, *graph),
            ContextTarget::ResourceItem(key, path) => file_of(*key, path),
        };
        if !there {
            return Some(format!("a context menu is open for {:?}, which is not there", menu.target));
        }
    }
    None
}

/// A monkey at the window: `steps` random moves, clicks, drags, wheel turns, keys and
/// shortcuts, typing, window sizes and dropped files, over the whole interface as it is laid
/// out, on the demo; every few steps a frame is drawn. While the Color Editor is open, it goes
/// to its window now and then, and back. It stops at the first panic or broken state, and says
/// what it did last.
fn monkey(seed: u64, steps: usize) -> Result<usize, String> {
    use keyboard::key::{Code, Named, Physical};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let Some(mut w) = Harness::new(Box::new(view::root), Size::new(1680.0, 1000.0)) else { return Ok(0) };
    let mut dice = Dice(seed.max(1));
    // What it starts on: the demo for most seeds; for the others an example of Learn, or a
    // template (FX-Maps, Pixel Math, HDRi lights, OpenPBR materials, graphs of graphs).
    use brewed_texture_core::templates::{Example, Template};
    let examples = Example::ALL;
    let templates = Template::ALL;
    let open = |w: &mut Harness, dice: &mut Dice| match dice.below(2) {
        0 => w.send(Message::Home(crate::app::home::HomeMessage::Example(examples[dice.below(examples.len() as u64) as usize]))),
        _ => {
            w.send(Message::NewGraph(crate::app::home::NewGraphMessage::Open(None)));
            w.send(Message::NewGraph(crate::app::home::NewGraphMessage::Template(templates[dice.below(templates.len() as u64) as usize])));
            w.send(Message::NewGraph(crate::app::home::NewGraphMessage::Create));
        }
    };
    if !seed.is_multiple_of(3) {
        open(&mut w, &mut dice);
        w.app.ui.home = None;
    }
    let mut log: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let files = std::env::temp_dir().join(format!("brewed-monkey-{seed}"));
    let _ = std::fs::create_dir_all(&files);
    let image = files.join("dropped.png");
    image::RgbaImage::from_pixel(64, 32, image::Rgba([200, 100, 50, 255])).save(&image).expect("a picture to drop");
    let broken_file = files.join("broken.png");
    std::fs::write(&broken_file, b"not a picture").expect("a broken file");
    let graph_file = files.join("graph.brewtex");
    // At the Color Editor's window (its own size), or at the main one (`main` large).
    let (mut at_editor, mut main) = (false, w.size);
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
        // (Its window closed, by whatever closed it: back at the main one.)
        if at_editor && w.app.color_window().is_none() {
            at_editor = false;
            w.look_at(Box::new(view::root), main);
        }
        let size = w.size;
        let mut events: Vec<iced::Event> = Vec::new();
        let mut direct: Vec<Message> = Vec::new();
        let what: String;
        let roll = dice.below(1000);
        let here = w.cursor.position().unwrap_or(Point::ORIGIN);
        let anywhere = |dice: &mut Dice| Point::new(dice.float(-10.0, size.width + 10.0), dice.float(-10.0, size.height + 10.0));
        let moved = |p: Point| iced::Event::Mouse(mouse::Event::CursorMoved { position: p });
        match roll {
            0..=299 => {
                let p = if dice.chance(20) { Point::new(here.x + dice.float(-30.0, 30.0), here.y + dice.float(-30.0, 30.0)) } else { anywhere(&mut dice) };
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
                let button = [mouse::Button::Left, mouse::Button::Left, mouse::Button::Middle, mouse::Button::Right][dice.below(4) as usize];
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
                let m = [keyboard::Modifiers::default(), keyboard::Modifiers::default(), keyboard::Modifiers::SHIFT, keyboard::Modifiers::CTRL]
                    [dice.below(4) as usize];
                what = format!("{m:?} {k:?}");
                let physical = Physical::Unidentified(keyboard::key::NativeCode::Unidentified);
                events.push(key_event(Key::Named(k), physical, m, true));
                events.push(key_event(Key::Named(k), physical, m, false));
            }
            910..=959 => {
                let (c, code) = shortcut_keys[dice.below(shortcut_keys.len() as u64) as usize];
                let m = if dice.chance(20) { keyboard::Modifiers::CTRL | keyboard::Modifiers::SHIFT } else { keyboard::Modifiers::CTRL };
                what = format!("{m:?} {c}");
                events.push(key_event(Key::Character(c.to_string().into()), Physical::Code(code), m, true));
                events.push(key_event(Key::Character(c.to_string().into()), Physical::Code(code), m, false));
            }
            960..=969 => {
                let new = Size::new(dice.float(1000.0, 2560.0).round(), dice.float(640.0, 1440.0).round());
                what = format!("window {new:?}");
                w.size = new;
                if !at_editor {
                    main = new;
                }
                events.push(iced::Event::Window(window::Event::Resized(new)));
            }
            970..=979 => {
                let path = [image.clone(), broken_file.clone(), files.join("missing.png"), files.clone(), graph_file.clone()][dice.below(5) as usize].clone();
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
                let key = w.app.doc_key;
                let graphs: Vec<brewed_texture_core::GraphId> = w.app.doc.package().graphs.iter().map(|g| g.id).collect();
                // (A package may have no graph — the monkey deletes its last one, or makes a
                // package without any: nothing is asked of a graph of it then.)
                let graph = (!graphs.is_empty()).then(|| graphs[dice.below(graphs.len() as u64) as usize]);
                let message = match (dice.below(12), graph) {
                    (0, _) => Message::Undo,
                    (1, _) => Message::Redo,
                    (2, _) => Message::Proceed(PendingAction::LoadDemo),
                    (3, _) => Message::Proceed(PendingAction::New),
                    (4, _) => Message::SavePicked(Some(graph_file.clone())),
                    (5, _) => Message::Proceed(PendingAction::OpenPath(graph_file.clone())),
                    // Graphs of graphs: what is selected made a graph of its own, a graph
                    // copied, another one edited, a graph dropped on the one edited as a node.
                    (6, _) => Message::GraphFromSelection,
                    (7, Some(graph)) => Message::Explorer(ExplorerMessage::DuplicateGraph(key, graph)),
                    (8, Some(graph)) => Message::Explorer(ExplorerMessage::OpenGraph(key, graph)),
                    (9, Some(graph)) => Message::Graph(GraphMessage::Drop {
                        payload: DragPayload::Graph { key, graph, name: "Graph".into() },
                        graph_pos: [dice.float(-400.0, 400.0), dice.float(-400.0, 400.0)],
                    }),
                    (7..=9, None) => Message::Seal,
                    // The Color Editor, of a color of the graph (its window then gone to now
                    // and then: see above).
                    (10, _) => {
                        let colors: Vec<ColorTarget> = w
                            .app
                            .graph()
                            .nodes
                            .iter()
                            .flat_map(|n| n.params.iter().filter(|p| matches!(p.value, ParamValue::Color(_))).map(|p| ColorTarget::Param(n.id, p.key.clone())))
                            .collect();
                        match colors.len() {
                            0 => Message::Seal,
                            n => Message::ColorPicker(Some(colors[dice.below(n as u64) as usize].clone())),
                        }
                    }
                    _ => {
                        open(&mut w, &mut dice);
                        w.app.ui.home = None;
                        Message::Seal
                    }
                };
                what = format!("{message:?}");
                direct.push(message);
            }
            990..=993 if w.app.color_window().is_some() => {
                at_editor = !at_editor;
                if at_editor {
                    what = "to the Color Editor's window".into();
                    main = w.size;
                    w.look_at(Box::new(view::editors::color_editor), view::editors::COLOR_EDITOR);
                } else {
                    what = "to the main window".into();
                    w.look_at(Box::new(view::root), main);
                }
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
            let window = if at_editor { w.app.color_window() } else { w.app.main_window() };
            for e in events {
                feed(&mut w, e, window);
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
                panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default()
            )),
            Ok(Some(state)) => Some(format!("BROKEN: {state}")),
            // Every thousand steps: what is saved reads back as it was, and every step undone
            // then redone gives the package back as it was.
            Ok(None) if step % 1000 == 999 => round_trips(&mut w, &files),
            // Every few hundred steps, in whatever state and size the window is then: no line
            // of text cut short.
            Ok(None) if step % 250 == 249 => {
                let cut = text_fits::cut_short(&mut w);
                let squeezed = text_fits::squeezed(&mut w);
                // (What floats over the window covers what is under it, as it is meant to.)
                let ui = &w.app.ui;
                let floating = ui.menu.is_some()
                    || ui.context.is_some()
                    || ui.add_menu.is_some()
                    || ui.dialog.is_some()
                    || ui.choice_menu.is_some()
                    || ui.preview.lighting.menu
                    || ui.drag.is_some();
                let over = if floating { Vec::new() } else { text_fits::overlapping(&mut w) };
                // (Nor the bar of a box that scrolls over what it holds.)
                let bars = text_fits::scroll_boxes(&mut w).1;
                if !bars.is_empty() {
                    Some(format!("A SCROLLBAR OVER WHAT IT SCROLLS at {}×{}: {}", w.size.width, w.size.height, bars.join("; ")))
                } else if !cut.is_empty() {
                    Some(format!("CUT SHORT at {}×{}: {}", w.size.width, w.size.height, cut.join("; ")))
                } else if !squeezed.is_empty() {
                    Some(format!("SQUEEZED at {}×{}: {}", w.size.width, w.size.height, squeezed.join("; ")))
                } else {
                    (!over.is_empty()).then(|| format!("OVERLAPPING at {}×{}: {}", w.size.width, w.size.height, over.join("; ")))
                }
            }
            Ok(None) => None,
        };
        if step % 5000 == 4999 {
            w.snapshot(&format!("monkey_{seed}_{step}"));
        }
        if let Some(failure) = failure {
            // (What the window showed then, when snapshots are asked for.)
            w.snapshot(&format!("monkey_{seed}_failed"));
            let _ = std::fs::remove_dir_all(&files);
            return Err(format!("seed {seed}, step {step}: {failure}\nlast steps:\n{}", log.iter().cloned().collect::<Vec<_>>().join("\n")));
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

/// What goes wrong when the package being edited is saved and read back, or when every step of
/// it is undone and then redone: each must give the package back as it was.
fn round_trips(w: &mut Harness, folder: &std::path::Path) -> Option<String> {
    use brewed_texture_core::project;
    let package = w.app.doc.package().clone();
    let path = folder.join("round-trip.brewtex");
    if let Err(e) = project::save(&path, &package, &std::collections::HashMap::new(), w.app.doc.current()) {
        return Some(format!("the package could not be saved: {e}"));
    }
    match project::load(&path) {
        Ok(loaded) if loaded.package == package => {}
        Ok(loaded) => {
            let differs: Vec<String> = package
                .graphs
                .iter()
                .zip(&loaded.package.graphs)
                .filter(|(a, b)| a != b)
                .map(|(a, b)| {
                    let nodes = a.graph.nodes.iter().zip(&b.graph.nodes).find(|(x, y)| x != y).map(|(x, y)| format!("{x:?}\nread back as\n{y:?}"));
                    format!("graph '{}': {}", a.name, nodes.unwrap_or_else(|| "not its nodes".into()))
                })
                .collect();
            return Some(format!(
                "the package read back is not what was saved: {}",
                if differs.is_empty() { "not its graphs".into() } else { differs.join("; ") }
            ));
        }
        Err(e) => return Some(format!("the package saved cannot be read: {e}")),
    }
    let _ = std::fs::remove_file(&path);
    let mut undone = 0;
    while w.app.doc.can_undo() && undone < 2000 {
        w.send(Message::Undo);
        undone += 1;
    }
    // (As many redone as were undone: what was undone before is still to be redone after.)
    let mut redone = 0;
    while w.app.doc.can_redo() && redone < undone {
        w.send(Message::Redo);
        redone += 1;
    }
    (*w.app.doc.package() != package).then(|| format!("{undone} steps undone and {redone} redone do not give the package back"))
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
    let text = package_json(&file);
    assert!(text.contains("wall_files/bricks.png"), "saved relative to itself: {text}");
    // A second copy is never written over a file that is there.
    let again = crate::app::packages::copy_files(&plan);
    assert!(again.copied.is_empty() && again.failed.len() == 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// What the parts of a saved package say (see `brewed_texture_core::project`), one after the
/// other.
fn package_json(file: &std::path::Path) -> String {
    let mut archive = brewed_texture_core::archive::Archive::new(std::fs::File::open(file).expect("saved")).expect("a package");
    let parts: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).filter(|n| n.ends_with(".json")).collect();
    parts.iter().map(|n| String::from_utf8(archive.read(n, u64::MAX).expect("read")).expect("text")).collect::<Vec<_>>().join("\n")
}

/// The names of the entries of a package's file.
fn package_entries(file: &std::path::Path) -> Vec<String> {
    let archive = brewed_texture_core::archive::Archive::new(std::fs::File::open(file).expect("saved")).expect("a package");
    archive.entries().iter().map(|e| e.name.clone()).collect()
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
    let text = package_json(&uses);
    assert!(text.contains("\"package\": \"wall.brewtex\""), "{text}");
    draw_frame(&mut w, std::time::Instant::now());

    // Opened again on its own, it finds the wall without the wall in the Explorer.
    for key in package_keys(&w.app) {
        w.send(Message::Explorer(ExplorerMessage::Close(key)));
    }
    assert_eq!(package_keys(&w.app).len(), 1);
    w.send(Message::OpenPicked(vec![uses.clone()]));
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
    w.send(Message::OpenPicked(vec![uses.clone()]));
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
    let of_kinds = |app: &App, kinds: &[NodeKind]| -> Vec<_> { app.graph().nodes.iter().filter(|n| kinds.contains(&n.kind)).map(|n| n.id).collect() };
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
    let files = vec![(font.clone(), Held::new(&font, Arc::new(b"not really a font".to_vec())))];
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
    let names: Vec<String> = w.app.doc.package().resources.iter().map(|r| r.name().to_string()).collect();
    assert_eq!(names, ["a.png"]);
    assert!(!shown(&mut w, "Bitmaps"), "one kind again: no folder of it");

    // a, held in the package: the Bitmap reads it there.
    let bytes = Arc::new(std::fs::read(&a).expect("a"));
    let files = vec![(a_path.clone(), Held::new(&a_path, bytes))];
    w.send(Message::Explorer(ExplorerMessage::FilesRead { key, embed: true, files, failed: Vec::new() }));
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
    w.send(Message::Home(crate::app::home::HomeMessage::Example(brewed_texture_core::templates::Example::BrickWall)));
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

/// An Image Input reads an input value of its graph whose value is a picture, shown and edited
/// in the Input Values as any other: renamed there (a click elsewhere in the window takes the
/// name, and every Image Input of it follows; the window losing the focus does not), a picture
/// chosen (one of the package's files), made its default and put back. A node that uses the
/// graph has a parameter for it among the others: no socket until it is exposed; a picture
/// dragged from the Explorer is dropped on its field, which says at once what it takes (a file
/// of another kind is refused while it is dragged). A graph renamed in the Explorer takes its
/// name at a click elsewhere too. A graph dragged where it cannot be used says why as soon as
/// it is dragged: onto itself, or onto a graph it uses; nor is it pasted there.
#[test]
fn image_inputs_are_input_values_and_drags_say_what_cannot_be() {
    use crate::app::FileTarget;
    use brewed_texture_core::ResourceKind;
    use brewed_texture_core::graph::{SocketRef, param_socket_key};
    use brewed_texture_core::nodes::ImageKind;

    let mut w = harness!(view::root, 1680.0, 1000.0);
    // Pictures chosen are linked, as the settings may say (held by the package, as they say by
    // default: `files_chosen_or_dropped_are_held`).
    apply_settings(&mut w, |s| s.new_files = crate::settings::NewFiles::Link);
    let dir = scratch("image-inputs");
    let picture = dir.join("bricks.png");
    image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 100, 50, 255])).save(&picture).expect("a picture");
    let other = dir.join("stones.png");
    image::RgbaImage::from_pixel(8, 8, image::Rgba([90, 90, 90, 255])).save(&other).expect("a picture");
    let font = dir.join("letters.ttf");
    std::fs::write(&font, b"named as a font").expect("a file");
    let (picture_path, font_path) = (picture.display().to_string(), font.display().to_string());
    let window = w.main_window();
    let now = std::time::Instant::now;
    // Pressed in a panel and moved: dragged, as from the Explorer.
    let start_drag = |w: &mut Harness, payload: DragPayload| {
        let at = Point::new(150.0, 300.0);
        w.window_event(window, iced::Event::Mouse(mouse::Event::CursorMoved { position: at }));
        w.send(Message::PanelPress(payload));
        let to = Point::new(at.x + 30.0, at.y + 10.0);
        w.window_event(window, iced::Event::Mouse(mouse::Event::CursorMoved { position: to }));
        assert!(w.app.ui.drag.is_some(), "dragged");
        draw_frame(w, now());
    };
    // Released where the mouse is: the widgets there, then the window.
    let release = |w: &mut Harness| {
        w.mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        w.window_event(window, iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)));
        assert!(w.app.ui.drag.is_none(), "the drag is over");
    };
    // A click somewhere else in the window (on the 2D view).
    let click_elsewhere = |w: &mut Harness| {
        w.hover(Point::new(1000.0, 800.0));
        w.mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        w.mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    };

    // A graph that takes an image and gives it out, with two Image Inputs of one input.
    w.send(Message::New);
    let key = w.app.doc_key;
    let tex = w.app.doc.current();
    let tex_name = w.app.doc.entry().name.clone();
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::IMAGE_INPUT, pos: [0.0, 0.0] }));
    let input = w.app.graph().nodes_of(NodeKind::IMAGE_INPUT).next().expect("an Image Input").id;
    let first = w.app.graph().node(input).and_then(|n| n.variable.clone()).expect("an input value");
    w.send(Message::Edit(Edit::AddVariableNode { name: first.clone(), pos: [0.0, 300.0] }));
    assert_eq!(w.app.graph().nodes_of(NodeKind::IMAGE_INPUT).count(), 2);
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::OUTPUT, pos: [300.0, 0.0] }));
    let output = w.app.graph().nodes_of(NodeKind::OUTPUT).next().expect("an Output").id;
    w.send(Message::Edit(Edit::Connect {
        a: SocketRef { node: input, key: "o:output".into(), output: true },
        b: SocketRef { node: output, key: "i:input".into(), output: false },
    }));
    w.send(Message::Seal);
    draw_frame(&mut w, now());

    // One row in the Input Values, as any value; no picture yet.
    assert_eq!(w.app.graph().variables.len(), 1);
    assert_eq!(texts(&mut w, &first).len(), 1, "a row of the Input Values");
    assert_eq!(texts_reading(&mut w, "no picture: the color").len(), 1);
    // Renamed there: typed, and a click elsewhere in the window takes it; both nodes follow.
    w.send(Message::ToggleVariable(first.clone()));
    w.send(Message::RenameVariable(first.clone(), "albedo".into()));
    click_elsewhere(&mut w);
    assert!(w.app.ui.renaming_var.is_none(), "taken");
    assert!(w.app.graph().variable("albedo").is_some_and(|v| v.image.is_some()));
    assert!(w.app.graph().nodes_of(NodeKind::IMAGE_INPUT).all(|n| n.variable.as_deref() == Some("albedo")));
    // The window losing the focus (a click in another program) leaves a name being typed.
    w.send(Message::RenameVariable("albedo".into(), "copied".into()));
    w.window_event(window, iced::Event::Window(window::Event::Unfocused));
    assert!(w.app.ui.renaming_var.is_some(), "still being typed");
    press(&mut w, keyboard::key::Named::Escape);
    assert!(w.app.ui.renaming_var.is_none() && w.app.graph().variable("albedo").is_some());

    // It takes color, and a picture chosen: what the graph sees, and one of the package's files.
    w.send(Message::Edit(Edit::SetVariableKind { name: "albedo".into(), kind: ImageKind::Color }));
    w.send(Message::FilePicked(FileTarget::Variable("albedo".into()), Some(picture.clone())));
    let image_of = |w: &Harness| w.app.graph().variable("albedo").and_then(|v| Some(v.image.as_ref()?.picture.clone()));
    assert_eq!(image_of(&w), Some(picture_path.clone()));
    assert!(w.app.doc.package().resources.iter().any(|r| r.path == picture_path), "in the Explorer's Resources");
    assert!(w.app.eval.node(output).is_some_and(|e| e.errors.is_empty()), "{:?}", w.app.eval.node(output));
    draw_frame(&mut w, now());
    assert!(!texts(&mut w, "bricks.png").is_empty(), "the field names the picture");
    assert_eq!(texts(&mut w, "albedo*").len(), 1, "not its picture by default: changed");
    w.snapshot("image-inputs-values");
    // Made its default; another picture, and put back.
    w.send(Message::Edit(Edit::MakeVariableDefault("albedo".into())));
    draw_frame(&mut w, now());
    assert_eq!(texts(&mut w, "albedo").len(), 1, "its default now");
    w.send(Message::FilePicked(FileTarget::Variable("albedo".into()), Some(other.clone())));
    assert_eq!(image_of(&w), Some(other.display().to_string()));
    w.send(Message::Edit(Edit::ResetVariable("albedo".into())));
    assert_eq!(image_of(&w), Some(picture_path.clone()));
    w.send(Message::Undo);
    assert_eq!(image_of(&w), Some(other.display().to_string()), "one step to undo");
    w.send(Message::Redo);
    assert_eq!(image_of(&w), Some(picture_path.clone()));

    // Another graph uses it: a parameter among the others, without a socket.
    w.send(Message::Explorer(ExplorerMessage::NewGraph(key)));
    w.send(Message::Explorer(ExplorerMessage::CommitRename));
    let main = w.app.doc.current();
    assert_ne!(main, tex);
    let payload = DragPayload::Graph { key, graph: tex, name: tex_name.clone() };
    w.send(Message::Graph(GraphMessage::Drop { payload, graph_pos: [0.0, 0.0] }));
    let node = w.app.graph().instances().next().expect("a node that uses the graph").id;
    w.send(Message::Graph(GraphMessage::Select { node, mode: SelectMode::Replace }));
    draw_frame(&mut w, now());
    assert!(texts(&mut w, "IMAGE INPUTS").is_empty(), "not a section of its own");
    assert_eq!(texts(&mut w, "Albedo").len(), 1, "a parameter");
    assert_eq!(texts_reading(&mut w, "the graph's: bricks.png").len(), 1);
    assert!(w.app.graph().input_sockets(w.app.graph().node(node).unwrap()).is_empty(), "nothing to plug into");
    let given = |w: &Harness| w.app.graph().node(node).map(|n| n.text("albedo").to_string());

    // A font dragged from the Explorer onto the parameter's field: refused at once, and so when
    // it is dropped there.
    w.send(Message::Explorer(ExplorerMessage::FilesPicked(key, vec![font.clone()])));
    start_drag(&mut w, DragPayload::Resource { path: font_path.clone(), kind: ResourceKind::Font, name: "letters.ttf".into() });
    let refused = texts_reading(&mut w, "✕ letters.ttf is not a picture");
    assert_eq!(refused.len(), 1, "the field says it takes pictures");
    w.snapshot("image-inputs-refused");
    w.hover(refused[0].center());
    release(&mut w);
    assert_eq!(given(&w), Some(String::new()));
    // The picture: taken, as one step to undo.
    start_drag(&mut w, DragPayload::Resource { path: picture_path.clone(), kind: ResourceKind::Bitmap, name: "bricks.png".into() });
    let zone = texts_reading(&mut w, "Drop to use bricks.png");
    assert_eq!(zone.len(), 1, "the field says it takes the picture");
    w.snapshot("image-inputs-drop");
    w.hover(zone[0].center());
    release(&mut w);
    assert_eq!(given(&w), Some(picture_path.clone()));
    assert!(w.app.eval.node(node).is_some_and(|e| e.errors.is_empty()), "{:?}", w.app.eval.node(node));
    draw_frame(&mut w, now());
    assert_eq!(texts(&mut w, "Albedo*").len(), 1, "a picture of its own: changed");
    w.send(Message::Undo);
    assert_eq!(given(&w), Some(String::new()));
    w.send(Message::Redo);
    assert_eq!(given(&w), Some(picture_path.clone()));
    // Exposed: a socket that takes color, and the row says nothing arrives; not any more, no socket.
    w.send(Message::Edit(Edit::SetExposed { node, key: "albedo".into(), exposed: true }));
    let socket = w.app.graph().input_socket(node, &param_socket_key("albedo")).expect("a socket");
    assert_eq!(socket.ty, brewed_texture_core::DataType::Image);
    draw_frame(&mut w, now());
    // (On one line: whole where the field has room for it, its words giving way before the
    // picture's name where it has not.)
    let said = texts_where(&mut w, |t| t.contains("uses bricks.png"));
    assert!(said.len() == 1 && ["not connected · uses bricks.png", "uses bricks.png"].contains(&said[0].0.as_str()), "{said:?}");
    assert!(said[0].1.height < 1.6 * crate::style::SMALL, "{said:?}");
    w.snapshot("image-inputs-exposed");
    w.send(Message::Edit(Edit::SetExposed { node, key: "albedo".into(), exposed: false }));
    assert!(w.app.graph().input_socket(node, &param_socket_key("albedo")).is_none());
    // The field's button takes the picture away: the graph's again.
    w.send(Message::SetFile(FileTarget::Param(node, "albedo".into()), None));
    assert_eq!(given(&w), Some(String::new()));

    // The graph renamed in the Explorer: typed, and a click elsewhere names it.
    w.send(Message::Explorer(ExplorerMessage::StartRename(key, main)));
    w.send(Message::Explorer(ExplorerMessage::RenameText("Wall".into())));
    draw_frame(&mut w, now());
    click_elsewhere(&mut w);
    assert!(w.app.ui.renaming_graph.is_none(), "taken");
    assert_eq!(w.app.doc.entry().name, "Wall");
    let main_name = w.app.doc.entry().name.clone();

    // The graph dragged onto itself: refused as soon as it is dragged, saying why.
    start_drag(&mut w, DragPayload::Graph { key, graph: main, name: main_name.clone() });
    assert_eq!(texts(&mut w, &format!("✕ '{main_name}' cannot use itself")).len(), 1);
    w.window_event(window, iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)));
    // Onto the graph it uses: that graph cannot use it back.
    w.send(Message::Explorer(ExplorerMessage::OpenGraph(key, tex)));
    let payload = DragPayload::Graph { key, graph: main, name: main_name.clone() };
    start_drag(&mut w, payload.clone());
    let why = format!("'{main_name}' uses '{tex_name}': a graph cannot use a graph that uses it");
    assert_eq!(texts(&mut w, &format!("✕ {why}")).len(), 1);
    w.snapshot("image-inputs-cycle");
    // Dropped anyway: nothing is added, and the status bar says why.
    w.send(Message::Graph(GraphMessage::Drop { payload, graph_pos: [0.0, 0.0] }));
    assert_eq!(w.app.graph().instances().count(), 0);
    let status = |w: &Harness| w.app.ui.status.as_ref().filter(|s| s.error).map(|s| s.text.clone());
    assert_eq!(status(&w).as_deref(), Some(why.as_str()));

    // Nor pasted: the node that uses this graph, copied, cannot go into it; one that uses a graph
    // that uses this one cannot either.
    w.send(Message::Explorer(ExplorerMessage::OpenGraph(key, main)));
    let user = w.app.graph().instances().next().expect("the node that uses the graph").id;
    w.send(Message::Graph(GraphMessage::Select { node: user, mode: SelectMode::Replace }));
    w.send(Message::Copy);
    w.send(Message::Explorer(ExplorerMessage::OpenGraph(key, tex)));
    w.send(Message::ClipboardText(None));
    assert_eq!(w.app.graph().instances().count(), 0, "not pasted");
    assert_eq!(status(&w), Some(format!("Not pasted: '{tex_name}' cannot use itself")));
    // Into the graph it was copied from, it is pasted.
    w.send(Message::Explorer(ExplorerMessage::OpenGraph(key, main)));
    w.send(Message::ClipboardText(None));
    assert_eq!(w.app.graph().instances().count(), 2);
    // A third graph uses the one that uses this one: its node, copied, is not pasted here.
    w.send(Message::Explorer(ExplorerMessage::NewGraph(key)));
    w.send(Message::Explorer(ExplorerMessage::CommitRename));
    let payload = DragPayload::Graph { key, graph: main, name: main_name.clone() };
    w.send(Message::Graph(GraphMessage::Drop { payload, graph_pos: [0.0, 0.0] }));
    let user = w.app.graph().instances().next().expect("a node that uses the second graph").id;
    w.send(Message::Graph(GraphMessage::Select { node: user, mode: SelectMode::Replace }));
    w.send(Message::Copy);
    w.send(Message::Explorer(ExplorerMessage::OpenGraph(key, tex)));
    w.send(Message::ClipboardText(None));
    assert_eq!(w.app.graph().instances().count(), 0, "not pasted");
    assert_eq!(status(&w), Some(format!("Not pasted: {why}")));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Pictures of the files of a package are made a few at a time: of five files looked at, two
/// are being made and three wait; each one made starts the next.
#[test]
fn pictures_of_files_are_made_a_few_at_a_time() {
    let mut w = harness!(view::root, 1280.0, 800.0);
    let dir = scratch("previews-at-once");
    let files: Vec<std::path::PathBuf> = (0..5)
        .map(|i| {
            let path = dir.join(format!("tile{i}.png"));
            image::RgbaImage::from_pixel(4, 4, image::Rgba([10 * i as u8, 0, 0, 255])).save(&path).expect("a picture");
            path
        })
        .collect();
    let key = w.app.doc_key;
    w.send(Message::Explorer(ExplorerMessage::FilesPicked(key, files.clone())));
    let looks = files.iter().map(|p| crate::previews::Look { path: p.display().to_string(), there: true, modified: None, bytes: Some(64) }).collect();
    w.send(Message::FilesLooked(looks));
    assert_eq!((w.app.res.previews_making, w.app.res.previews_waiting.len()), (crate::previews::AT_ONCE, 5 - crate::previews::AT_ONCE));
    let first = files[0].display().to_string();
    w.send(Message::PreviewMade(first.clone(), None, Ok((vec![255; 4 * 4 * 4], 4, 4, "4 × 4".into()))));
    assert_eq!((w.app.res.previews_making, w.app.res.previews_waiting.len()), (crate::previews::AT_ONCE, 4 - crate::previews::AT_ONCE));
    assert!(w.app.res.files.get(&first).is_some_and(|f| f.preview.is_some() && !f.making));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Names being typed are taken when the package is saved (as Enter takes them): the file says
/// what the window shows.
#[test]
fn names_being_typed_are_saved() {
    let mut w = harness!(view::root, 1280.0, 800.0);
    let dir = scratch("typed-names");
    let file = dir.join("typed.brewtex");
    w.send(Message::SavePicked(Some(file.clone())));
    let (key, graph) = (w.app.doc_key, w.app.doc.current());
    w.send(Message::Explorer(ExplorerMessage::StartRename(key, graph)));
    w.send(Message::Explorer(ExplorerMessage::RenameText("Wall".into())));
    let var = w.app.graph().variables[0].name.clone();
    w.send(Message::RenameVariable(var.clone(), "courses".into()));
    w.send(Message::Save);
    assert!(w.app.ui.renaming_graph.is_none() && w.app.ui.renaming_var.is_none());
    let text = package_json(&file);
    assert!(text.contains("\"name\": \"Wall\""), "the graph's name");
    assert!(text.contains("\"courses\"") && !text.contains(&format!("\"{var}\"")), "the input value's name");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Undo goes back to the graph the change was made in: that graph is shown as its view was,
/// and the view of the graph left is kept for it, as when graphs are opened in the Explorer.
#[test]
fn undo_back_to_another_graph_keeps_their_views() {
    let mut w = harness!(view::root, 1280.0, 800.0);
    let key = w.app.doc_key;
    let wall = w.app.doc.current();
    // A change in the wall, seen from far to the left.
    w.app.ui.graph_view.pan = iced::Vector::new(-400.0, 20.0);
    let node = w.app.graph().nodes[0].id;
    w.send(Message::Edit(Edit::MoveNodes { ids: vec![node], delta: [50.0, 0.0] }));
    w.send(Message::Seal);
    // Another graph, seen from elsewhere.
    w.send(Message::Explorer(ExplorerMessage::NewGraph(key)));
    w.send(Message::Explorer(ExplorerMessage::CommitRename));
    let other = w.app.doc.current();
    w.app.ui.graph_view.pan = iced::Vector::new(300.0, -70.0);
    // Undo, twice: the new graph goes, then the move; the wall is shown as it was seen.
    w.send(Message::Undo);
    w.send(Message::Undo);
    assert_eq!(w.app.doc.current(), wall);
    assert_eq!(w.app.ui.graph_view.pan, iced::Vector::new(-400.0, 20.0), "the wall's view");
    // Redone, the new graph is there again, as it was seen.
    w.send(Message::Redo);
    w.send(Message::Redo);
    assert_eq!(w.app.doc.current(), other);
    assert_eq!(w.app.ui.graph_view.pan, iced::Vector::new(300.0, -70.0), "the other graph's view");
}

/// An undo back in another graph that takes away the node its 2D view showed: the view shows the
/// graph's Output, as when the graph is opened (it showed nothing).
#[test]
fn undo_back_to_a_shown_node_taken_away_shows_the_output() {
    let mut w = harness!(view::root, 1280.0, 800.0);
    let key = w.app.doc_key;
    let wall = w.app.doc.current();
    let output = w.app.graph().nodes_of(NodeKind::OUTPUT).next().expect("an Output").id;
    w.send(Message::Explorer(ExplorerMessage::NewGraph(key)));
    w.send(Message::Explorer(ExplorerMessage::CommitRename));
    let other = w.app.doc.current();
    // In the wall, a node added and shown; then the other graph opened.
    w.send(Message::Explorer(ExplorerMessage::OpenGraph(key, wall)));
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::LEVELS, pos: [0.0, 600.0] }));
    w.send(Message::Seal);
    let added = w.app.graph().nodes_of(NodeKind::LEVELS).map(|n| n.id).max().expect("the node added");
    w.send(Message::Graph(GraphMessage::Preview(added)));
    assert_eq!(w.app.ui.preview.node, Some(added));
    w.send(Message::Explorer(ExplorerMessage::OpenGraph(key, other)));
    // The undo takes the node away, back in the wall: its Output is shown.
    w.send(Message::Undo);
    assert_eq!(w.app.doc.current(), wall);
    assert!(w.app.graph().node(added).is_none());
    assert_eq!(w.app.ui.preview.node, Some(output), "the Output, as the graph opened shows it");
}

/// Unsaved work is kept as it changes, and what a program that stopped without quitting kept (a
/// crash: its session's lock goes with it) is offered the next time the program starts:
/// recovered, each package opens with the changes it had, not saved yet, where it was saved.
/// What a running program keeps is not offered to another; Later offers it again, Discard
/// takes it away; a package saved loses its snapshot, and quitting leaves nothing.
#[test]
fn unsaved_work_is_recovered_after_a_crash() {
    use crate::app::autosave::Recovery;
    let dir = scratch("recovery");
    let root = dir.join("Recovery");
    let opts = StartupOptions { recovery: Some(root.clone()), ..StartupOptions::default() };
    let walls = dir.join("walls.brewtex");
    let add = |app: &mut App, kind: NodeKind| {
        let _ = app.update(Message::Edit(Edit::AddNode { kind, pos: [0.0, 0.0] }));
        let _ = app.update(Message::Seal);
    };

    // A package saved, then changed (a file put in it, a node added); another never saved.
    let (mut first, _) = App::boot(opts.clone());
    assert_eq!(first.ui.dialog, None, "nothing to recover the first time");
    let _ = first.update(Message::SavePicked(Some(walls.clone())));
    let bricks = std::sync::Arc::new(b"the bytes of bricks.png".to_vec());
    let files = vec![("C:/textures/bricks.png".to_string(), Held::new("C:/textures/bricks.png", bricks.clone()))];
    let _ = first.update(Message::Explorer(ExplorerMessage::FilesRead { key: first.doc_key, embed: false, files, failed: vec![] }));
    add(&mut first, NodeKind::BLUR);
    let changed = first.graph().nodes.len();
    let _ = first.update(Message::New);
    add(&mut first, NodeKind::NOISE);
    first.autosave_now();
    // Another program, started meanwhile, is offered nothing: the first runs.
    let (second, _) = App::boot(opts.clone());
    assert_eq!(second.ui.dialog, None);
    drop(second);

    // The first stops without quitting.
    drop(first);
    let (mut third, _) = App::boot(opts.clone());
    assert_eq!(third.ui.dialog, Some(Dialog::Recover));
    let mut titles: Vec<String> = third.recoverable().map(|s| s.unsaved.title.clone()).collect();
    titles.sort();
    assert_eq!(titles, ["Graph", "walls.brewtex"]);
    let _ = third.update(Message::Recover(Recovery::Later));
    assert_eq!(third.ui.dialog, None);
    drop(third);

    // Offered again, and recovered.
    let (mut fourth, _) = App::boot(opts.clone());
    assert_eq!(fourth.ui.dialog, Some(Dialog::Recover));
    let _ = fourth.update(Message::Recover(Recovery::Recover));
    assert_eq!(fourth.ui.dialog, None);
    let packages = fourth.packages();
    assert_eq!(packages.len(), 2, "in place of the untouched demo");
    let (walls_key, recovered) = packages.iter().find(|(_, d)| d.path() == Some(&walls)).copied().expect("where it was saved");
    assert!(recovered.is_dirty(), "its changes are not saved yet");
    assert_eq!(recovered.graph().nodes.len(), changed, "with the node added after it was saved");
    let held = recovered.package().resources.iter().find(|r| r.name() == "bricks.png").and_then(|r| r.data.clone());
    assert_eq!(held.as_deref(), Some(&*bricks), "and the file put in it");
    // Which is kept apart, once, not in the snapshot.
    let kept: Vec<std::path::PathBuf> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .flat_map(|session| std::fs::read_dir(session.path()).into_iter().flatten().flatten())
        .map(|f| f.path())
        .collect();
    assert_eq!(kept.iter().filter(|f| f.extension().is_some_and(|e| e == "held")).count(), 1);
    for snapshot in kept.iter().filter(|f| f.extension().is_some_and(|e| e == "brewtex")) {
        let entries = package_entries(snapshot);
        assert!(!entries.iter().any(|n| n.starts_with("files/")), "{}: {entries:?}", snapshot.display());
    }
    let (_, untitled) = packages.iter().find(|(_, d)| d.path().is_none()).copied().expect("the one never saved");
    assert!(untitled.is_dirty() && untitled.graph().nodes_of(NodeKind::NOISE).count() == 1);
    drop(packages);
    // What it keeps now is its own: another program is offered nothing.
    let (fifth, _) = App::boot(opts.clone());
    assert_eq!(fifth.ui.dialog, None);
    drop(fifth);

    // Saved, a package has nothing to recover; quitting, nothing is left.
    let graph = fourth.package_doc(walls_key).unwrap().current();
    let _ = fourth.update(Message::Explorer(ExplorerMessage::OpenGraph(walls_key, graph)));
    let _ = fourth.update(Message::Save);
    assert!(!fourth.doc.is_dirty());
    let _ = fourth.update(Message::Quit);
    assert_eq!(fourth.ui.dialog, Some(Dialog::Unsaved(PendingAction::Quit)), "the untitled one asks");
    let _ = fourth.update(Message::Proceed(PendingAction::Quit));
    assert!(brewed_texture_core::recovery::orphans(&root, None).is_empty());
    assert_eq!(std::fs::read_dir(&root).map(|d| d.count()).unwrap_or(0), 0, "no session left");
    drop(fourth);

    // Discarded, it is not offered again.
    let (mut sixth, _) = App::boot(opts.clone());
    add(&mut sixth, NodeKind::BLUR);
    sixth.autosave_now();
    drop(sixth);
    let (mut seventh, _) = App::boot(opts.clone());
    assert_eq!(seventh.ui.dialog, Some(Dialog::Recover));
    let _ = seventh.update(Message::Recover(Recovery::Discard));
    drop(seventh);
    let (eighth, _) = App::boot(opts);
    assert_eq!(eighth.ui.dialog, None);
    drop(eighth);
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a program that stopped kept is offered in a dialog, over the window, as the program
/// starts: each package by its name and where it was saved, and the three buttons. Recover,
/// clicked, opens it.
#[test]
fn the_recovery_dialog_offers_what_was_kept() {
    let dir = scratch("recovery-dialog");
    let opts = StartupOptions { recovery: Some(dir.join("Recovery")), ..StartupOptions::default() };
    let walls = dir.join("walls.brewtex");
    let (mut crashed, _) = App::boot(opts.clone());
    let _ = crashed.update(Message::SavePicked(Some(walls.clone())));
    let _ = crashed.update(Message::Edit(Edit::AddNode { kind: NodeKind::BLUR, pos: [0.0, 0.0] }));
    let blurs = crashed.graph().nodes_of(NodeKind::BLUR).count();
    crashed.autosave_now();
    drop(crashed);

    let Some(mut w) = Harness::started(Box::new(view::root), Size::new(1280.0, 800.0), opts) else {
        eprintln!("no GPU for the headless renderer: test skipped");
        return;
    };
    assert_eq!(w.app.ui.dialog, Some(Dialog::Recover));
    assert_eq!(texts(&mut w, "Recover unsaved work?").len(), 1);
    assert_eq!(texts(&mut w, "walls.brewtex").len(), 1);
    assert_eq!(texts(&mut w, &walls.display().to_string()).len(), 1, "where it was saved");
    for label in ["Discard", "Later"] {
        assert_eq!(texts(&mut w, label).len(), 1, "{label}");
    }
    w.snapshot("recovery-dialog");
    let button = *texts(&mut w, "Recover").first().expect("the Recover button");
    click(&mut w, button.center());
    assert_eq!(w.app.ui.dialog, None);
    assert!(w.app.doc.is_dirty() && w.app.doc.path() == Some(&walls));
    assert_eq!(w.app.graph().nodes_of(NodeKind::BLUR).count(), blurs, "with the node added after it was saved");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Started on the Home screen, as it starts without files, the program offers what it kept of
/// one that stopped; recovered, it is what is edited: the graph, not the Home screen (it waited
/// behind the Home screen's button). Later leaves the Home screen as it is.
#[test]
fn recovered_work_is_shown_at_once() {
    use crate::app::autosave::Recovery;
    let dir = scratch("recovery-shown");
    let opts = StartupOptions { recovery: Some(dir.join("Recovery")), home: true, ..StartupOptions::default() };
    let (mut crashed, _) = App::boot(opts.clone());
    crashed.ui.home = None;
    let _ = crashed.update(Message::Edit(Edit::AddNode { kind: NodeKind::BLUR, pos: [0.0, 0.0] }));
    crashed.autosave_now();
    drop(crashed);
    let (mut later, _) = App::boot(opts.clone());
    assert!(later.ui.home.is_some(), "on the Home screen");
    assert_eq!(later.ui.dialog, Some(Dialog::Recover));
    let _ = later.update(Message::Recover(Recovery::Later));
    assert!(later.ui.home.is_some(), "Later: still on the Home screen");
    drop(later);
    let (mut next, _) = App::boot(opts);
    assert_eq!(next.ui.dialog, Some(Dialog::Recover));
    let _ = next.update(Message::Recover(Recovery::Recover));
    assert!(next.ui.home.is_none(), "the graph recovered shown");
    assert_eq!(next.graph().nodes_of(NodeKind::BLUR).count(), 1, "with its changes");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A package whose file was saved after its snapshot was kept (by another program) is
/// recovered untitled: saving it asks where, and what is newer is not written over unasked.
#[test]
fn a_file_saved_since_is_not_written_over() {
    use crate::app::autosave::Recovery;
    let dir = scratch("recovery-newer");
    let opts = StartupOptions { recovery: Some(dir.join("Recovery")), ..StartupOptions::default() };
    let walls = dir.join("walls.brewtex");
    let (mut crashed, _) = App::boot(opts.clone());
    let _ = crashed.update(Message::SavePicked(Some(walls.clone())));
    let _ = crashed.update(Message::Edit(Edit::AddNode { kind: NodeKind::BLUR, pos: [0.0, 0.0] }));
    crashed.autosave_now();
    drop(crashed);
    // Saved again since, elsewhere.
    let file = std::fs::File::options().write(true).open(&walls).unwrap();
    file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3600)).unwrap();
    drop(file);

    let (mut next, _) = App::boot(opts);
    let snapshot = next.recoverable().next().expect("offered").clone();
    assert!(snapshot.saved.is_some_and(|saved| saved > snapshot.unsaved.time));
    let _ = next.update(Message::Recover(Recovery::Recover));
    assert!(next.doc.is_dirty() && next.doc.path().is_none(), "untitled");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Quitting asks about each package with unsaved changes in turn; given up with Escape halfway
/// (after one was not to be saved), the next quit asks about that one again: it may have changed
/// since, and it is not lost unasked. A dialog takes the keyboard: Ctrl+S under it saves nothing.
#[test]
fn a_quit_given_up_asks_again_about_every_package() {
    let (mut app, _) = App::boot(StartupOptions::default());
    let main = app.main_window().unwrap();
    let close = || Message::Runtime(iced::Event::Window(window::Event::CloseRequested), iced::event::Status::Ignored, main);
    let key = |named: keyboard::key::Named, modifiers: keyboard::Modifiers, text: Option<&str>| {
        let key = match text {
            Some(t) => Key::Character(t.into()),
            None => Key::Named(named),
        };
        let event = iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        });
        Message::Runtime(event, iced::event::Status::Ignored, main)
    };
    // Two packages with unsaved changes.
    let _ = app.update(Message::Edit(Edit::SetResolution([512, 512])));
    let _ = app.update(Message::New);
    let _ = app.update(Message::Edit(Edit::SetResolution([256, 256])));
    let _ = app.update(close());
    assert_eq!(app.ui.dialog, Some(Dialog::Unsaved(PendingAction::Quit)));
    let first = app.doc_key;
    // Not the first: the second asks.
    let _ = app.update(Message::Proceed(PendingAction::Quit));
    assert_eq!(app.ui.dialog, Some(Dialog::Unsaved(PendingAction::Quit)));
    assert_ne!(app.doc_key, first);
    // Ctrl+S under the dialog saves nothing (the second has no file: it would ask where).
    let _ = app.update(key(keyboard::key::Named::Escape, keyboard::Modifiers::CTRL, Some("s")));
    assert_eq!(app.ui.dialog, Some(Dialog::Unsaved(PendingAction::Quit)), "the dialog stays, nothing else happens");
    // Given up with Escape; the next quit asks about the first again.
    let _ = app.update(key(keyboard::key::Named::Escape, keyboard::Modifiers::empty(), None));
    assert_eq!(app.ui.dialog, None);
    assert!(app.packages().iter().all(|(_, d)| d.is_dirty()), "nothing was lost");
    let _ = app.update(close());
    assert_eq!(app.ui.dialog, Some(Dialog::Unsaved(PendingAction::Quit)));
    assert_eq!(app.doc_key, first, "the first is asked about again");
}

/// Quitting, a package asked to be saved that cannot be (the place is a folder) leaves nothing
/// to follow: the quit is given up, and a save that works later is only a save.
#[test]
fn a_save_that_fails_leaves_no_quit_behind() {
    let dir = scratch("failed-save");
    let (mut app, _) = App::boot(StartupOptions::default());
    let main = app.main_window().unwrap();
    let _ = app.update(Message::Edit(Edit::SetResolution([512, 512])));
    let _ = app.update(Message::Runtime(iced::Event::Window(window::Event::CloseRequested), iced::event::Status::Ignored, main));
    let _ = app.update(Message::SaveThen(PendingAction::Quit));
    assert_eq!(app.ui.after_save, Some(PendingAction::Quit), "the untitled package asks where");
    // Where it cannot be written: a folder (of the name of a package).
    let folder = dir.join("taken.brewtex");
    std::fs::create_dir_all(&folder).unwrap();
    let _ = app.update(Message::SavePicked(Some(folder)));
    assert!(app.doc.is_dirty() && app.ui.status.as_ref().is_some_and(|s| s.error));
    assert_eq!(app.ui.after_save, None, "nothing follows");
    let _ = app.update(Message::SavePicked(Some(dir.join("walls.brewtex"))));
    assert!(!app.doc.is_dirty());
    assert_eq!(app.ui.dialog, None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A package saved as "walls" where "walls.brewtex" is (the dialogs of GNOME and zenity add no
/// extension, and asked only about "walls") does not write over it: nothing is written, the
/// package is not saved, the status bar says why, and what was to follow the save (closing the
/// package) does not. Saved as "walls.brewtex", which the dialog asked about, it replaces it.
#[test]
fn a_file_the_dialog_did_not_ask_about_is_not_written_over() {
    let dir = scratch("typed-without-extension");
    let walls = dir.join("walls.brewtex");
    std::fs::write(&walls, b"another package").expect("a file");
    let (mut app, _) = App::boot(StartupOptions::default());
    let key = app.doc_key;
    let _ = app.update(Message::Edit(Edit::SetResolution([512, 512])));
    let _ = app.update(Message::Explorer(ExplorerMessage::Close(key)));
    assert_eq!(app.ui.dialog, Some(Dialog::Unsaved(PendingAction::Close(key))));
    let _ = app.update(Message::SaveThen(PendingAction::Close(key)));
    assert_eq!(app.ui.after_save, Some(PendingAction::Close(key)), "the untitled package asks where");
    let _ = app.update(Message::SavePicked(Some(dir.join("walls"))));
    let said = app.ui.status.as_ref().map(|s| (s.text.as_str(), s.error));
    assert_eq!(said, Some(("walls.brewtex is there already: save it as that to replace it", true)));
    assert_eq!(std::fs::read(&walls).expect("still there"), b"another package", "not written over");
    let files: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(files, ["walls.brewtex"], "nothing else written");
    assert!(app.doc.is_dirty() && app.doc.path().is_none(), "not saved");
    assert_eq!(app.ui.after_save, None, "nothing follows");
    // As "walls.brewtex": replaced, and the package not closed.
    let _ = app.update(Message::SavePicked(Some(walls.clone())));
    assert!(app.package_doc(key).is_some_and(|d| !d.is_dirty() && d.path() == Some(&walls)), "saved, still open");
    assert!(brewed_texture_core::project::load(&walls).is_ok(), "the package is the file now");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A dialog does not keep the window from being used: what it answers is taken for the package
/// and the graph it was asked for, not the one edited when it answers; given up, it changes
/// nothing; for a package closed meanwhile, nothing is done.
#[test]
fn answers_of_dialogs_go_where_they_were_asked() {
    let dir = scratch("answers");
    let (mut app, _) = App::boot(StartupOptions::default());
    // Asked to save the demo, changed…
    let _ = app.update(Message::Edit(Edit::SetResolution([512, 512])));
    let demo = app.here();
    // …another package edited meanwhile.
    let _ = app.update(Message::New);
    let _ = app.update(Message::Edit(Edit::SetResolution([256, 256])));
    let other = app.here();
    assert_ne!(demo.key, other.key);
    let _ = app.update(Message::Asked(demo, Box::new(Message::SavePicked(Some(dir.join("wall.brewtex"))))));
    assert_eq!(app.here(), demo, "the demo is edited again");
    assert!(!app.package_doc(demo.key).unwrap().is_dirty(), "and saved");
    let unsaved = app.package_doc(other.key).unwrap();
    assert!(unsaved.is_dirty() && unsaved.path().is_none(), "not the other");
    // Given up, nothing changes.
    let _ = app.update(Message::Explorer(ExplorerMessage::OpenGraph(other.key, other.graph)));
    let _ = app.update(Message::Asked(demo, Box::new(Message::SavePicked(None))));
    assert_eq!(app.here(), other);
    // Closed meanwhile, nothing is done.
    let _ = app.update(Message::Explorer(ExplorerMessage::Close(other.key)));
    let _ = app.update(Message::Proceed(PendingAction::Close(other.key)));
    assert!(app.package_doc(other.key).is_none());
    let _ = app.update(Message::Asked(other, Box::new(Message::SavePicked(Some(dir.join("gone.brewtex"))))));
    assert!(!dir.join("gone.brewtex").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The settings changed by `change`, as Apply leaves them in the app (the dialog opened, the
/// change made, applied).
fn apply_settings(w: &mut Harness, change: impl FnOnce(&mut crate::settings::Settings)) {
    use crate::app::SettingsMessage as S;
    let mut settings = w.app.settings.clone();
    change(&mut settings);
    w.send(Message::Settings(S::Open));
    if let Some(Dialog::Settings(dialog)) = &mut w.app.ui.dialog {
        dialog.draft = settings;
    }
    w.send(Message::Settings(S::Apply));
    w.send(Message::Settings(S::Cancel));
}

/// Where the view shows `wanted`, to click it: the last place, what floats over the window
/// (a dialog, the offer to start again) before the window.
fn place_of(w: &mut Harness, wanted: &str) -> Point {
    texts(w, wanted).last().unwrap_or_else(|| panic!("{wanted} is not shown")).center()
}

/// The settings: Ctrl+, (the key of the comma, on every layout) or Edit → Settings… shows them,
/// a page at a time, the one shown last. A change is marked on its page and taken only when it
/// is applied: Apply does nothing before a change, Cancel (Escape) forgets it, and a page is set
/// back as it is by default. New graphs are as the settings say.
#[test]
fn settings_are_changed_and_applied() {
    use crate::app::SettingsMessage as S;
    use crate::settings::{Page, Setting, Settings};
    use brewed_texture_core::Precision;
    use keyboard::key::{Code, Physical};
    let mut w = harness!(view::root, 1280.0, 800.0);
    let ctrl = keyboard::Modifiers::CTRL;
    let page = |w: &Harness| match &w.app.ui.dialog {
        Some(Dialog::Settings(dialog)) => Some(dialog.page),
        _ => None,
    };
    // A Russian keyboard types "б" on the key of the comma.
    key(&mut w, "б", Physical::Code(Code::Comma), ctrl);
    assert_eq!(page(&w), Some(Page::General));
    draw_frame(&mut w, std::time::Instant::now());
    for label in ["General", "Packages", "Recovery", "Graphics", "Shortcuts", "Undo steps", "Recent packages", "File dialog folder", "Invert zoom"] {
        assert!(!texts(&mut w, label).is_empty(), "{label}");
    }
    assert!(texts(&mut w, "Keep unsaved work").is_empty(), "on a page of its own");
    assert!(!texts(&mut w, "Not kept: the program runs with the settings as they are by default.").is_empty());
    w.snapshot("settings");
    // Nothing changed: nothing to apply, and the dialog is closed rather than cancelled.
    assert!(texts(&mut w, "Cancel").is_empty() && !texts(&mut w, "Close").is_empty());
    let at = place_of(&mut w, "Apply");
    click(&mut w, at);
    assert_eq!(page(&w), Some(Page::General), "Apply does nothing yet");

    // Another page, and a change: marked, not taken.
    let at = place_of(&mut w, "Packages");
    click(&mut w, at);
    assert_eq!(page(&w), Some(Page::Packages));
    w.send(Message::Settings(S::Set(Setting::GraphSize([512, 512]))));
    w.send(Message::Settings(S::Set(Setting::GraphPrecision(Precision::Full))));
    draw_frame(&mut w, std::time::Instant::now());
    assert_eq!(w.app.settings.graph_size, [2048, 2048], "not taken before Apply");
    assert!(!texts(&mut w, "●").is_empty() && !texts(&mut w, "Cancel").is_empty(), "marked, and to be cancelled");
    w.snapshot("settings-changed");
    press(&mut w, keyboard::key::Named::Escape);
    assert_eq!(w.app.ui.dialog, None);
    assert_eq!(w.app.settings, Settings::default(), "cancelled: nothing taken");

    // Opened again on the page shown last; applied, taken, and the dialog stays.
    key(&mut w, ",", Physical::Code(Code::Comma), ctrl);
    assert_eq!(page(&w), Some(Page::Packages));
    w.send(Message::Settings(S::Set(Setting::GraphSize([512, 512]))));
    w.send(Message::Settings(S::Set(Setting::GraphPrecision(Precision::Full))));
    draw_frame(&mut w, std::time::Instant::now());
    let at = place_of(&mut w, "Apply");
    click(&mut w, at);
    assert_eq!((w.app.settings.graph_size, w.app.settings.graph_precision), ([512, 512], Precision::Full));
    assert_eq!(page(&w), Some(Page::Packages));
    draw_frame(&mut w, std::time::Instant::now());
    assert!(texts(&mut w, "●").is_empty() && !texts(&mut w, "Close").is_empty(), "nothing waits to be applied");
    w.send(Message::Settings(S::Cancel));
    w.send(Message::New);
    assert_eq!((w.app.graph().resolution, w.app.graph().precision), ([512, 512], Precision::Full));
    w.send(Message::Explorer(ExplorerMessage::NewGraph(w.app.doc_key)));
    assert_eq!(w.app.doc.package().graphs.len(), 2);
    assert_eq!(w.app.graph().resolution, [512, 512], "a graph added to a package too");

    // The page set back as it is by default.
    w.send(Message::Settings(S::Open));
    draw_frame(&mut w, std::time::Instant::now());
    let at = place_of(&mut w, "Reset Page to Defaults");
    click(&mut w, at);
    w.send(Message::Settings(S::Apply));
    assert_eq!(w.app.settings, Settings::default());
}

/// A setting the program takes only when it starts says so, and offers to start it again once
/// it is applied, in the corner: Later puts the offer away, a setting as it started takes it
/// away. Restart asks about unsaved changes as quitting does ("before restarting"): given up,
/// the offer is back; else the program quits and starts again with the packages open.
#[test]
fn settings_taken_at_a_start_offer_to_start_again() {
    use crate::app::SettingsMessage as S;
    use crate::settings::{Graphics, Page, Setting};
    let mut w = harness!(view::root, 1280.0, 800.0);
    w.send(Message::Settings(S::OpenPage(Page::Graphics)));
    draw_frame(&mut w, std::time::Instant::now());
    for label in ["Graphics API", "Antialiasing", "Restart"] {
        assert!(!texts(&mut w, label).is_empty(), "{label}");
    }
    w.send(Message::Settings(S::Set(Setting::Graphics(Graphics::Vulkan))));
    assert!(!w.app.ui.restart_offer, "not before it is applied");
    w.send(Message::Settings(S::Apply));
    assert!(w.app.ui.restart_offer);
    draw_frame(&mut w, std::time::Instant::now());
    assert!(!texts(&mut w, "Taken when the program starts again.").is_empty());
    assert_eq!(texts(&mut w, "Restart the program?").len(), 1, "in the dialog, while it is open");
    w.snapshot("restart-offer");
    let at = place_of(&mut w, "Later");
    click(&mut w, at);
    assert!(!w.app.ui.restart_offer);
    w.send(Message::Settings(S::Set(Setting::Graphics(Graphics::Auto))));
    w.send(Message::Settings(S::Apply));
    assert!(!w.app.ui.restart_offer, "as it started: nothing to start again for");
    w.send(Message::Settings(S::Set(Setting::Antialiasing(false))));
    w.send(Message::Settings(S::Apply));
    assert!(w.app.ui.restart_offer);
    w.send(Message::Settings(S::Cancel));
    draw_frame(&mut w, std::time::Instant::now());
    assert_eq!(texts(&mut w, "Restart the program?").len(), 1, "in the corner, once it is closed");
    w.snapshot("restart-offer-corner");

    // Unsaved changes ask first; given up, the offer is back.
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::BLUR, pos: [0.0, 0.0] }));
    draw_frame(&mut w, std::time::Instant::now());
    let at = place_of(&mut w, "Restart Now");
    click(&mut w, at);
    assert!(matches!(w.app.ui.dialog, Some(Dialog::Unsaved(_))));
    draw_frame(&mut w, std::time::Instant::now());
    let question = format!("\"{}\" has unsaved changes. Do you want to save them before restarting?", w.app.doc.title());
    let asked = texts(&mut w, &question);
    assert!(!asked.is_empty(), "before restarting");
    press(&mut w, keyboard::key::Named::Escape);
    assert!(w.app.ui.restart_offer && w.app.ui.started_again.is_none());

    // Saved: the program starts again with the package.
    let dir = scratch("restart");
    let walls = dir.join("walls.brewtex");
    w.send(Message::SavePicked(Some(walls.clone())));
    w.send(Message::Settings(S::Restart));
    assert_eq!(w.app.ui.started_again.as_deref(), Some(&[walls][..]));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Settings chosen are kept in their file and taken again the next time, only what changed
/// written; a file that is no TOML is not written over, and says so. Unsaved work not kept any
/// more: what was kept of it goes, and nothing more is written.
#[test]
fn settings_are_kept_in_their_file() {
    use crate::settings::{NewFiles, Settings, Store};
    let dir = scratch("settings");
    let file = dir.join("settings.toml");
    let opts = |file: &std::path::Path| StartupOptions {
        settings: Store::open(Some(file.to_path_buf())),
        recovery: Some(dir.join("Recovery")),
        ..StartupOptions::default()
    };
    let (mut first, _) = App::boot(opts(&file));
    assert!(!file.exists(), "nothing written before a setting changes");
    first.set_settings(Settings { keep_every: 5, new_files: NewFiles::Link, ..first.settings.clone() });
    let text = std::fs::read_to_string(&file).expect("written");
    let said = |key: &str| text.lines().any(|l| l.starts_with(key));
    assert!(said("every_minutes = 5") && said("new = \"link\""), "{text}");
    assert!(!said("undo_steps") && !said("keep"), "what is as it is by default is not said: {text}");

    // Unsaved work, kept; then not kept any more: what was kept goes.
    let _ = first.update(Message::Edit(Edit::AddNode { kind: NodeKind::BLUR, pos: [0.0, 0.0] }));
    let _ = first.update(Message::Seal);
    first.autosave_now();
    let snapshots = |dir: &std::path::Path| -> usize {
        std::fs::read_dir(dir.join("Recovery"))
            .into_iter()
            .flatten()
            .flatten()
            .flat_map(|session| std::fs::read_dir(session.path()).into_iter().flatten().flatten())
            .filter(|f| f.path().extension().is_some_and(|e| e == "brewtex"))
            .count()
    };
    assert_eq!(snapshots(&dir), 1);
    first.set_settings(Settings { keep_unsaved: false, ..first.settings.clone() });
    assert_eq!(snapshots(&dir), 0, "not kept any more: gone");
    let _ = first.update(Message::Edit(Edit::AddNode { kind: NodeKind::BLUR, pos: [100.0, 0.0] }));
    first.autosave_now();
    assert_eq!(snapshots(&dir), 0, "and nothing more written");
    drop(first);

    let (second, _) = App::boot(opts(&file));
    assert_eq!((second.settings.keep_every, second.settings.new_files, second.settings.keep_unsaved), (5, NewFiles::Link, false));
    drop(second);

    std::fs::write(&file, "keep = [").unwrap();
    let (mut third, _) = App::boot(opts(&file));
    assert!(third.settings_file().1.iter().any(|p| p.contains("not TOML")), "{:?}", third.settings_file().1);
    third.set_settings(Settings { undo_steps: 50, ..third.settings.clone() });
    assert_eq!(third.settings.undo_steps, 50, "taken all the same, while the program runs");
    assert!(third.ui.status.as_ref().is_some_and(|s| s.error && s.text.contains("not saved")));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "keep = [", "not written over");
    drop(third);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The keys of the commands are given on the Shortcuts page: a key is clicked, and the next
/// key pressed takes its place (Escape stops waiting). A key another command had goes from it,
/// and the page says so. Applied, the key does what it was given to, the menus and the list of
/// keys say it, and the settings file keeps it.
#[test]
fn shortcuts_are_given_other_keys() {
    use crate::app::SettingsMessage as S;
    use crate::settings::{Page, Store};
    use crate::shortcuts::{Chord, Command};
    use keyboard::key::{Code, Physical};
    let dir = scratch("shortcuts");
    let file = dir.join("settings.toml");
    let opts = StartupOptions { settings: Store::open(Some(file.clone())), ..StartupOptions::default() };
    let Some(mut w) = Harness::started(Box::new(view::root), Size::new(1280.0, 800.0), opts) else { return };
    let ctrl = keyboard::Modifiers::CTRL;
    let capturing = |w: &Harness| match &w.app.ui.dialog {
        Some(Dialog::Settings(dialog)) => dialog.capturing,
        _ => None,
    };
    w.send(Message::Settings(S::OpenPage(Page::Shortcuts)));
    draw_frame(&mut w, std::time::Instant::now());
    for label in ["Undo", "Ctrl+Z", "Ctrl+Y", "Ctrl+Shift+Z", "Frame Selection / All", "Search and Add a Node"] {
        assert!(!texts(&mut w, label).is_empty(), "{label}");
    }
    w.snapshot("settings-shortcuts");
    // Undo's key clicked: it waits; Escape stops it waiting, the dialog stays.
    let at = place_of(&mut w, "Ctrl+Z");
    click(&mut w, at);
    assert_eq!(capturing(&w), Some((Command::Undo, 0)));
    press(&mut w, keyboard::key::Named::Escape);
    assert_eq!(capturing(&w), None);
    assert!(w.app.ui.dialog.is_some());
    // Given Ctrl+D (on a Russian keyboard), which Duplicate had.
    let at = place_of(&mut w, "Ctrl+Z");
    click(&mut w, at);
    draw_frame(&mut w, std::time::Instant::now());
    assert!(!texts(&mut w, "Press a key…").is_empty());
    key(&mut w, "в", Physical::Code(Code::KeyD), ctrl);
    let ctrl_d = Chord::parse("Ctrl+D").unwrap();
    let draft = match &w.app.ui.dialog {
        Some(Dialog::Settings(dialog)) => dialog.clone(),
        _ => panic!("the dialog stays"),
    };
    assert_eq!((draft.draft.shortcuts.of(Command::Undo), draft.draft.shortcuts.of(Command::Duplicate)), (vec![ctrl_d], vec![]));
    assert_eq!(draft.note.as_deref(), Some("Ctrl+D was the key of \"Duplicate\", which has no key now."));
    draw_frame(&mut w, std::time::Instant::now());
    assert!(!texts(&mut w, "Ctrl+D was the key of \"Duplicate\", which has no key now.").is_empty(), "the page says so");
    assert_eq!(w.app.key_of(Command::Undo), "Ctrl+Z", "not before Apply");
    w.send(Message::Settings(S::Apply));
    w.send(Message::Settings(S::Cancel));

    // Ctrl+D undoes now, and duplicates nothing; Ctrl+Z does nothing.
    let nodes = w.app.graph().nodes.len();
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::BLUR, pos: [0.0, 0.0] }));
    w.send(Message::Seal);
    key(&mut w, "z", Physical::Code(Code::KeyZ), ctrl);
    assert_eq!(w.app.graph().nodes.len(), nodes + 1, "Ctrl+Z is no key now");
    key(&mut w, "d", Physical::Code(Code::KeyD), ctrl);
    assert_eq!(w.app.graph().nodes.len(), nodes, "undone, nothing duplicated");
    assert_eq!((w.app.key_of(Command::Undo), w.app.key_of(Command::Duplicate)), ("Ctrl+D".to_string(), String::new()));
    w.send(Message::Dialog(Some(Dialog::Shortcuts)));
    draw_frame(&mut w, std::time::Instant::now());
    assert!(!texts(&mut w, "Ctrl+D").is_empty(), "the list of keys says it");
    w.send(Message::Dialog(None));
    let text = std::fs::read_to_string(&file).expect("kept");
    assert!(text.contains("undo = [\"Ctrl+D\"]") && text.contains("duplicate = []"), "{text}");

    // A key taken away, and the keys by default again.
    w.send(Message::Settings(S::Open));
    w.send(Message::Settings(S::ClearKey(Command::Redo, 0)));
    w.send(Message::Settings(S::ResetKeys(Command::Undo)));
    w.send(Message::Settings(S::ResetKeys(Command::Duplicate)));
    w.send(Message::Settings(S::Apply));
    assert_eq!(w.app.settings.shortcuts.of(Command::Redo), [Chord::parse("Ctrl+Shift+Z").unwrap()]);
    assert_eq!((w.app.key_of(Command::Undo), w.app.key_of(Command::Duplicate)), ("Ctrl+Z".to_string(), "Ctrl+D".to_string()));
    let text = std::fs::read_to_string(&file).unwrap();
    let said = |key: &str| text.lines().any(|l| l.starts_with(key));
    assert!(text.contains("redo = [\"Ctrl+Shift+Z\"]") && !said("undo =") && !said("duplicate ="), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The mouse wheel zooms the graph and the 2D and 3D views one way, and the other way when the
/// settings say so. Undo goes back as many steps as they say; File → Open Recent offers as many
/// packages as they say.
#[test]
fn the_wheel_undo_and_recent_packages_are_as_the_settings_say() {
    use crate::app::MenuId;
    let mut w = harness!(view::root, 1280.0, 800.0);
    draw_frame(&mut w, std::time::Instant::now());
    let middle = w.app.ui.graph_view.bounds.center();
    let wheel = |w: &mut Harness| {
        w.hover(middle);
        w.mouse(mouse::Event::WheelScrolled { delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 } });
    };
    let zoom = w.app.ui.graph_view.zoom;
    wheel(&mut w);
    assert!(w.app.ui.graph_view.zoom > zoom, "pushed away, it zooms in");
    apply_settings(&mut w, |s| s.invert_zoom = true);
    let zoom = w.app.ui.graph_view.zoom;
    wheel(&mut w);
    assert!(w.app.ui.graph_view.zoom < zoom, "the other way");

    apply_settings(&mut w, |s| s.undo_steps = 10);
    for i in 0..15 {
        w.send(Message::Edit(Edit::AddNode { kind: NodeKind::BLUR, pos: [i as f32 * 10.0, 0.0] }));
        w.send(Message::Seal);
    }
    let mut undone = 0;
    while w.app.doc.can_undo() {
        w.send(Message::Undo);
        undone += 1;
    }
    assert_eq!(undone, 10);

    let dir = scratch("recent-count");
    for name in ["a", "b", "c"] {
        w.send(Message::New);
        w.send(Message::SavePicked(Some(dir.join(format!("{name}.brewtex")))));
    }
    assert_eq!(w.app.ui.recent.len(), 3);
    apply_settings(&mut w, |s| s.recent_packages = 2);
    w.send(Message::Menu(Some(MenuId::File)));
    w.send(Message::SubMenu(Some(crate::app::SubMenu::Recent)));
    draw_frame(&mut w, std::time::Instant::now());
    let offered = ["a.brewtex", "b.brewtex", "c.brewtex"].map(|name| texts(&mut w, name).len());
    // (The Explorer names them too: the menu, once more for each it offers.)
    assert_eq!(offered, [1, 2, 2], "the two opened last");
    assert_eq!(w.app.ui.recent.len(), 3, "the others are remembered still");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The file dialogs open as the settings say. By default each where it did before there was
/// a setting: saving the package or its files, in the package's folder; any other where the
/// system opens it. Else all in the package's folder, or all where a dialog was answered
/// last: each in the other where that one is not there.
#[test]
fn file_dialogs_open_where_the_settings_say() {
    use crate::settings::FileDialogs;
    let mut w = harness!(view::root, 1280.0, 800.0);
    let dir = scratch("dialog-folders");
    let (package, other) = (dir.join("package"), dir.join("other"));
    std::fs::create_dir_all(&package).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    assert_eq!((w.app.dialog_folder(None, true), w.app.dialog_folder(None, false)), (None, None), "nothing to go by");
    w.send(Message::SavePicked(Some(package.join("walls.brewtex"))));
    assert_eq!(w.app.ui.last_folder.as_deref(), Some(package.as_path()));
    let key = w.app.doc_key;
    w.send(Message::Explorer(ExplorerMessage::FilesPicked(key, vec![other.join("bricks.png")])));
    assert_eq!(w.app.ui.last_folder.as_deref(), Some(other.as_path()), "answered from there last");
    // As before.
    assert_eq!(w.app.dialog_folder(None, true).as_deref(), Some(package.as_path()), "saving: in the package's folder");
    assert_eq!(w.app.dialog_folder(None, false), None, "any other: where the system opens it");
    apply_settings(&mut w, |s| s.file_dialogs = FileDialogs::Package);
    assert_eq!(w.app.dialog_folder(None, false).as_deref(), Some(package.as_path()));
    apply_settings(&mut w, |s| s.file_dialogs = FileDialogs::Last);
    assert_eq!(w.app.dialog_folder(None, true).as_deref(), Some(other.as_path()));
    std::fs::remove_dir_all(&other).unwrap();
    assert_eq!(w.app.dialog_folder(None, false).as_deref(), Some(package.as_path()), "not there: the package's");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Escape in a field of the settings dialog (a number being typed) stops the typing, and the
/// dialog stays with what was changed in it; Escape anywhere else cancels it.
#[test]
fn escape_in_a_field_of_the_settings_keeps_them() {
    use crate::app::SettingsMessage as S;
    use crate::settings::Setting;
    let mut w = harness!(view::root, 1280.0, 800.0);
    w.send(Message::Settings(S::Open));
    w.send(Message::Settings(S::Set(Setting::UndoSteps(50))));
    let main = w.main_window();
    w.send(Message::Runtime(escape(), iced::event::Status::Captured, main));
    assert!(matches!(&w.app.ui.dialog, Some(Dialog::Settings(d)) if d.draft.undo_steps == 50), "a field took it");
    w.send(Message::Runtime(escape(), iced::event::Status::Ignored, main));
    assert_eq!(w.app.ui.dialog, None);
    assert_eq!(w.app.settings.undo_steps, 256, "cancelled");
}

/// A file chosen for a node or dropped on the window is held by the package, as the settings
/// say by default: read and hashed in the background, it is taken where it was asked for, in
/// one step of undo; one that cannot be read says so. Linked, as they may say, a node reads it
/// where it is, at once.
#[test]
fn files_chosen_or_dropped_are_held() {
    use crate::app::FileTarget;
    use crate::settings::NewFiles;
    use brewed_texture_core::ResourceKind;
    use std::sync::Arc;
    let mut w = harness!(view::root, 1280.0, 800.0);
    let dir = scratch("held");
    let picture = dir.join("bricks.png");
    image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 100, 50, 255])).save(&picture).expect("a picture");
    let path = picture.display().to_string();
    w.send(Message::Edit(Edit::AddNode { kind: NodeKind::BITMAP, pos: [0.0, 0.0] }));
    w.send(Message::Seal);
    let bitmap = w.app.graph().nodes_of(NodeKind::BITMAP).next().expect("a Bitmap").id;
    let target = FileTarget::Param(bitmap, "path".into());
    let read_by = |w: &Harness, node| w.app.graph().node(node).map(|n| n.text("path").to_string());

    // Chosen: read in the background first (nothing yet), then held.
    w.send(Message::FilePicked(target.clone(), Some(picture.clone())));
    assert_eq!(read_by(&w, bitmap).as_deref(), Some(""));
    let held = Held::new(&path, Arc::new(std::fs::read(&picture).unwrap()));
    w.send(Message::FileHeld(target.clone(), Ok(held.clone())));
    assert_eq!(read_by(&w, bitmap).as_deref(), Some(held.path()));
    let resource = w.app.doc.package().resources.iter().find(|r| r.path == held.path()).expect("in the package");
    assert!(resource.is_embedded() && resource.name() == "bricks.png");
    assert!(w.app.eval.node(bitmap).is_some_and(|e| e.errors.is_empty()), "{:?}", w.app.eval.node(bitmap));
    w.send(Message::Undo);
    assert_eq!(read_by(&w, bitmap).as_deref(), Some(""));
    assert!(w.app.doc.package().resources.is_empty(), "one step of undo: {:?}", w.app.doc.package().resources);
    w.send(Message::Redo);

    // Dropped on the window: the node that reads it, where it was dropped.
    w.send(Message::DropHeld(ResourceKind::Bitmap, [300.0, 40.0], Ok(held.clone())));
    let dropped = w.app.graph().nodes_of(NodeKind::BITMAP).find(|n| n.id != bitmap).expect("a node of the drop");
    assert_eq!((dropped.text("path"), dropped.pos), (held.path(), [300.0, 40.0]));
    assert_eq!(w.app.doc.package().resources.len(), 1, "held once");

    // What cannot be read says so.
    w.send(Message::FileHeld(target.clone(), Err("Cannot read gone.png: it is not there".into())));
    assert!(w.app.ui.status.as_ref().is_some_and(|s| s.error && s.text.contains("gone.png")));

    // Linked: the file where it is, at once.
    apply_settings(&mut w, |s| s.new_files = NewFiles::Link);
    w.send(Message::FilePicked(target, Some(picture.clone())));
    assert_eq!(read_by(&w, bitmap), Some(path));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The packages opened or saved last are in File → Open Recent Package, the latest first, each
/// by its name and folder: one opens it again; one that is not there any more goes off the list,
/// saying so; Clear Recent Packages forgets them all, and the submenu says there are none.
#[test]
fn packages_opened_last_are_offered_again() {
    use crate::app::{MenuId, SubMenu};
    let mut w = harness!(view::root, 1280.0, 800.0);
    let dir = scratch("recent");
    let (walls, floors) = (dir.join("walls.brewtex"), dir.join("floors.brewtex"));
    assert!(w.app.ui.recent.is_empty());
    w.send(Message::SavePicked(Some(walls.clone())));
    w.send(Message::New);
    w.send(Message::SavePicked(Some(floors.clone())));
    let said = |p: &std::path::Path| p.display().to_string();
    assert_eq!(w.app.ui.recent, [said(&floors), said(&walls)]);
    // (The Explorer names the packages too: the menu, once more.)
    let names = ["walls.brewtex", "floors.brewtex", "Clear Recent Packages"];
    draw_frame(&mut w, std::time::Instant::now());
    let before = names.map(|name| texts(&mut w, name).len());
    w.send(Message::Menu(Some(MenuId::File)));
    draw_frame(&mut w, std::time::Instant::now());
    assert!(!texts(&mut w, "Open Recent Package").is_empty());
    assert_eq!(texts(&mut w, "Clear Recent Packages").len(), before[2], "in the submenu, not in the menu");
    assert!(texts(&mut w, "Open the Demo").is_empty());
    w.send(Message::SubMenu(Some(SubMenu::Recent)));
    draw_frame(&mut w, std::time::Instant::now());
    for (name, before) in names.into_iter().zip(before) {
        assert_eq!(texts(&mut w, name).len(), before + 1, "{name}");
    }
    w.snapshot("recent-packages");
    w.send(Message::Menu(None));
    assert_eq!(w.app.ui.submenu, None, "closed with the menu");

    // Opened again from the menu: in the Explorer, the latest now.
    for key in package_keys(&w.app) {
        w.send(Message::Explorer(ExplorerMessage::Close(key)));
    }
    w.send(Message::OpenRecent(said(&walls)));
    assert_eq!(w.app.doc.path(), Some(&walls));
    assert_eq!(w.app.ui.recent, [said(&walls), said(&floors)]);

    // Gone from the disk: off the list, and said.
    std::fs::remove_file(&floors).unwrap();
    w.send(Message::OpenRecent(said(&floors)));
    assert_eq!(w.app.ui.recent, [said(&walls)]);
    assert!(w.app.ui.status.as_ref().is_some_and(|s| s.error && s.text.contains("not there")));
    w.send(Message::ClearRecent);
    assert!(w.app.ui.recent.is_empty());
    w.send(Message::Menu(Some(MenuId::File)));
    w.send(Message::SubMenu(Some(SubMenu::Recent)));
    draw_frame(&mut w, std::time::Instant::now());
    assert!(texts(&mut w, "Clear Recent Packages").is_empty(), "no list, no clearing it");
    assert!(!texts(&mut w, "No recent packages").is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file of a package is renamed in the Explorer as a graph is: typed, and taken on a click
/// elsewhere in the window (or Enter); Escape leaves it as it was; undone, it is called as it
/// was. The file is the same: what reads it reads it still.
#[test]
fn files_are_renamed_in_the_explorer() {
    use std::sync::Arc;
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let key = w.app.doc_key;
    let held = Held::new("C:/tex/bricks.png", Arc::new(b"the bytes of bricks.png".to_vec()));
    let path = held.path().to_string();
    w.send(Message::Explorer(ExplorerMessage::FilesRead { key, embed: false, files: vec![("C:/tex/bricks.png".into(), held)], failed: vec![] }));
    let name = |w: &Harness| w.app.doc.package().resources.iter().find(|r| r.path == path).map(|r| r.name.clone());
    assert_eq!(name(&w).as_deref(), Some("bricks.png"));
    draw_frame(&mut w, std::time::Instant::now());
    assert!(!texts(&mut w, "bricks.png").is_empty());

    w.send(Message::Explorer(ExplorerMessage::StartRenameFile(key, path.clone())));
    assert!(w.app.ui.renaming_file.is_some());
    w.send(Message::Explorer(ExplorerMessage::RenameFileText("Wall bricks.png".into())));
    // A click on the graph.
    draw_frame(&mut w, std::time::Instant::now());
    w.hover(Point::new(700.0, 250.0));
    w.mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    w.mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    assert!(w.app.ui.renaming_file.is_none());
    assert_eq!(name(&w).as_deref(), Some("Wall bricks.png"));
    draw_frame(&mut w, std::time::Instant::now());
    assert!(!texts(&mut w, "Wall bricks.png").is_empty() && texts(&mut w, "bricks.png").is_empty());
    assert!(w.app.doc.package().resources.iter().any(|r| r.path == path), "the same file");

    // Escape leaves the name as it was.
    w.send(Message::Explorer(ExplorerMessage::StartRenameFile(key, path.clone())));
    w.send(Message::Explorer(ExplorerMessage::RenameFileText("Stones.png".into())));
    press(&mut w, keyboard::key::Named::Escape);
    assert_eq!(name(&w).as_deref(), Some("Wall bricks.png"));

    w.send(Message::Undo);
    assert_eq!(name(&w).as_deref(), Some("bricks.png"));
}

/// Under an open menu nothing takes the cursor (see `widget::blind`): over a menu whose submenu
/// was open, the Explorer under it took the cursor as its own and showed its tooltips. What is
/// under the menus is told the cursor came in while they are closed, and not while one is open.
#[test]
fn nothing_under_an_open_menu_takes_the_cursor() {
    use crate::app::MenuId;
    use crate::widget::blind::blind;
    // What is under the menus says the cursor came in by opening the Help menu.
    fn under_a_menu(app: &App) -> Element<'_, Message> {
        let under = iced::widget::container(iced::widget::text("under")).width(iced::Length::Fill).height(iced::Length::Fill);
        blind(iced::widget::mouse_area(under).on_enter(Message::Menu(Some(MenuId::Help))), app.ui.menu.is_some()).into()
    }
    let Some(mut w) = Harness::started(Box::new(under_a_menu), Size::new(400.0, 300.0), StartupOptions::default()) else {
        eprintln!("no GPU for the headless renderer: test skipped");
        return;
    };
    w.hover(Point::new(200.0, 150.0));
    assert_eq!(w.app.ui.menu, Some(MenuId::Help), "with the menus closed, it takes the cursor");
    w.app.ui.menu = Some(MenuId::File);
    w.hover(Point::new(-50.0, -50.0));
    w.hover(Point::new(200.0, 150.0));
    assert_eq!(w.app.ui.menu, Some(MenuId::File), "under an open menu, it does not");
}

/// An Output whose usage is Panorama lights the 3D view only when asked to: by its node (or the
/// lighting menu, which offers it first). Asked, its image lights the view once the engine has
/// it; the studio or an image chosen in its place light the view; the panorama asked again, it
/// does. Of another document, or once the Output has another usage, it lights nothing. And an
/// environment shows behind the model only when asked to.
#[test]
fn the_graphs_panorama_lights_the_3d_view_when_asked() {
    use crate::app::PreviewMessage as P;
    use brewed_texture_core::nodes::Usage;
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let shown = |w: &mut Harness, text: &str| !texts(w, text).is_empty();
    assert!(!w.app.ui.preview.lighting.background, "nothing behind the model at first");
    let out = w.app.graph().nodes_of(NodeKind::OUTPUT).map(|n| n.id).next().expect("an output");
    let name = w.app.graph().node(out).unwrap().text("name").to_string();
    w.send(Message::PickChoice(out, "usage", Usage::Panorama.index()));
    assert_eq!(w.app.panorama_outputs(), vec![out]);
    assert!(w.app.panorama_node().is_none() && w.app.environment_shown().is_none(), "not until asked");
    // Asked on its node.
    w.app.ui.selection = [out].into_iter().collect();
    w.send(Message::Preview(P::LightBy(Some(out))));
    assert_eq!(w.app.panorama_node(), Some(out));
    assert!(w.app.environment_shown().is_none(), "until the engine has it");
    let sky = std::sync::Arc::new(crate::environment::Hdri::new("panorama".into(), 8, 8, vec![2.0; 8 * 8 * 3]));
    let doc = w.app.doc_key;
    w.send(Message::Preview(P::PanoramaRead(doc, out, Some(sky.clone()))));
    assert!(w.app.environment_shown().is_some_and(|h| std::sync::Arc::ptr_eq(&h, &sky)), "lit by the graph's panorama");
    assert!(shown(&mut w, "Light the 3D view with this panorama"), "its node says so");
    w.send(Message::Preview(P::LightingMenu(true)));
    for text in [format!("●  The Graph's Panorama: {name}"), "○  Studio".to_string(), "○  Show It Behind the Model".to_string()] {
        assert!(shown(&mut w, &text), "{text} is in the menu");
    }
    // The studio in its place; the panorama again.
    w.send(Message::Preview(P::Environment(None)));
    assert!(w.app.environment_shown().is_none() && !w.app.lit_by_panorama(), "the studio, chosen");
    w.send(Message::Preview(P::LightBy(Some(out))));
    assert!(w.app.lit_by_panorama() && w.app.environment_shown().is_some());
    w.send(Message::Preview(P::LightBy(None)));
    assert!(!w.app.lit_by_panorama(), "turned off on its node");
    // Of an older document, nothing; once the Output is of another usage, nothing.
    w.send(Message::Preview(P::LightBy(Some(out))));
    w.send(Message::Preview(P::PanoramaRead(doc + 1, out, Some(sky.clone()))));
    w.send(Message::PickChoice(out, "usage", Usage::BaseColor.index()));
    assert!(w.app.panorama_node().is_none() && w.app.environment_shown().is_none());
}

/// Help → Third-Party Notices lists, as programs list them, each work of others the program is
/// made with, its license, and a link to where it is: Matball's model (its author), the font,
/// Rust's standard library and every crate; searched, only what is found. A click on one opens
/// its link (not in the tests). About says no more of what the program is built with, but opens
/// this window; and says under what the program itself is.
#[test]
fn the_third_party_notices_say_whose_work_is_used() {
    use crate::app::NoticesMessage;
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let shown = |w: &mut Harness, text: &str| !texts(w, text).is_empty();
    w.send(Message::Dialog(Some(Dialog::Notices(Default::default()))));
    for text in ["Third-Party Notices", "Shader Ball JL 01 by Craneo (the Matball, changed)", "[CC BY 4.0]", "Inter", "Rust standard library"] {
        assert!(shown(&mut w, text), "{text} is in the window");
    }
    w.send(Message::Notices(NoticesMessage::Search("wgpu".into())));
    assert!(shown(&mut w, "wgpu") && !shown(&mut w, "Shader Ball JL 01 by Craneo (the Matball, changed)"), "only what the search finds");
    let wgpu = crate::third_party::works().iter().find(|work| work.name == "wgpu").expect("wgpu");
    assert!(wgpu.link.contains("github.com/gfx-rs/wgpu"), "{}", wgpu.link);
    let row = *texts(&mut w, "wgpu").first().expect("its row");
    click(&mut w, row.center());
    assert!(matches!(w.app.ui.dialog, Some(Dialog::Notices(_))), "a link opened, the window stays");
    w.snapshot("third_party_notices");
    w.send(Message::Dialog(Some(Dialog::About)));
    assert!(!shown(&mut w, "Rust · wgpu · iced"), "About says no more of what the program is built with");
    assert!(shown(&mut w, crate::third_party::OWN_LICENSE), "About says under what the program itself is");
    let notices = *texts(&mut w, "Third-Party Notices…").last().expect("About opens the notices");
    click(&mut w, notices.center());
    assert!(matches!(w.app.ui.dialog, Some(Dialog::Notices(_))));
}

/// Help → Reveal Log File shows the log in the file manager (not in the tests), where a log file
/// was made; where none was, the item is there but does nothing.
#[test]
fn the_log_file_is_revealed_from_the_help_menu() {
    for log in [Some(std::env::temp_dir().join(crate::log_file::NAME)), None] {
        let opts = StartupOptions { log: log.clone(), ..StartupOptions::default() };
        let mut w = match Harness::started(Box::new(view::root), Size::new(1680.0, 1000.0), opts) {
            Some(w) => w,
            None => {
                eprintln!("no GPU for the headless renderer: test skipped");
                return;
            }
        };
        w.send(Message::Menu(Some(crate::app::MenuId::Help)));
        let item = *texts(&mut w, "Reveal Log File").first().expect("the item is in the Help menu");
        click(&mut w, item.center());
        assert_eq!(w.app.ui.menu.is_none(), log.is_some(), "chosen, the menu closes; with no log, nothing happens");
    }
}

/// A file to open whose name is not Unicode text (given on the command line by a file manager
/// on Linux) is not opened, and the status says why: the program keeps its paths as text, and
/// would read another file, or none, under the name it could write for it.
#[test]
fn a_file_whose_name_is_not_text_is_said_not_to_open() {
    let odd = crate::tests::not_unicode();
    let (app, _) = App::boot(StartupOptions { open: vec![odd.into()], ..StartupOptions::default() });
    let status = app.ui.status.as_ref().expect("a status");
    assert!(status.error && status.text.contains("not Unicode"), "{}", status.text);
    assert!(app.ui.opening.is_empty(), "nothing is read");
}

/// The menu of an Output's usage is one list under headings: a click on a heading closes what is
/// under it (and opens it again), and goes no further than the menu, as no click on it does;
/// a closed heading stays closed when the menu opens again; a search finds what is under it
/// all the same.
#[test]
fn the_usage_menu_opens_and_closes_its_headings() {
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let shown = |w: &mut Harness, text: &str| !texts(w, text).is_empty();
    let out = w.app.graph().nodes_of(NodeKind::OUTPUT).map(|n| n.id).next().expect("an output");
    w.app.ui.selection = [out].into_iter().collect();
    w.send(Message::ChoiceMenu(Some((out, "usage"))));
    assert!(shown(&mut w, "Base Weight") && shown(&mut w, "Coat Weight"), "everything is open at first");
    let selection = w.app.ui.selection.clone();
    let base = *texts(&mut w, "Base").last().expect("the heading Base");
    click(&mut w, base.center());
    assert!(w.app.ui.choice_menu.is_some(), "the menu stays open");
    assert_eq!(w.app.ui.selection, selection, "the click went no further than the menu");
    assert!(w.app.ui.closed_headings.contains("Base") && !shown(&mut w, "Base Weight") && shown(&mut w, "Coat Weight"));
    // Closed when it opens again; searched, what is under it is found.
    w.send(Message::ChoiceMenu(None));
    w.send(Message::ChoiceMenu(Some((out, "usage"))));
    assert!(!shown(&mut w, "Base Weight"), "still closed");
    w.send(Message::ChoiceSearch("weight".into()));
    assert!(shown(&mut w, "Base Weight") && shown(&mut w, "Coat Weight") && !shown(&mut w, "Roughness"));
    w.send(Message::ChoiceSearch(String::new()));
    let base = *texts(&mut w, "Base").last().expect("the heading Base");
    click(&mut w, base.center());
    assert!(!w.app.ui.closed_headings.contains("Base") && shown(&mut w, "Base Weight"), "open again");
    w.snapshot("usage_menu");
}

/// A double click on a template in the window of a new graph makes it at once; one click
/// chooses it.
#[test]
fn a_double_click_on_a_template_makes_it() {
    use crate::app::home::NewGraphMessage;
    use brewed_texture_core::templates::Template;
    let mut w = harness!(view::root, 1680.0, 1000.0);
    let before = w.app.packages().len();
    w.send(Message::NewGraph(NewGraphMessage::Open(None)));
    w.send(Message::NewGraph(NewGraphMessage::Template(Template::Coat)));
    assert!(matches!(w.app.ui.dialog, Some(Dialog::NewGraph(_))), "one click chooses it");
    w.send(Message::NewGraph(NewGraphMessage::Template(Template::Coat)));
    assert!(w.app.ui.dialog.is_none(), "a second at once makes it");
    assert!(w.app.packages().len() >= before);
    assert!(w.app.graph().nodes_of(NodeKind::OUTPUT).any(|n| n.text("name") == "coat_weight"), "the graph of the template");
}

/// A graph made of the HDRi template lights the 3D view with its own panorama at once. The scene
/// of a graph's 3D view (its model, relief, repeat, light and exposure) is saved with its package
/// and is what the view shows when the package is opened again.
#[test]
fn the_scene_of_the_3d_view_is_kept_with_the_package() {
    use crate::app::home::NewGraphMessage;
    use crate::app::{PendingAction, PreviewMessage};
    use crate::view::view3d::{Camera, Shape};
    use brewed_texture_core::templates::Template;
    let mut w = harness!(view::root, 1680.0, 1000.0);
    w.send(Message::NewGraph(NewGraphMessage::Open(None)));
    w.send(Message::NewGraph(NewGraphMessage::Template(Template::StudioPanorama)));
    w.send(Message::NewGraph(NewGraphMessage::Create));
    let output = w.app.graph().nodes_of(NodeKind::OUTPUT).map(|n| n.id).next().expect("its output");
    assert_eq!(w.app.panorama_node(), Some(output), "the template's panorama lights the 3D view");
    // A scene of its own, saved; the view put back as it starts; the package opened again.
    let camera = w.app.ui.preview.view3d;
    w.send(Message::Preview(PreviewMessage::Camera(Camera { shape: Shape::Sphere, displacement: 0.1, tiling: 2.0, ..camera })));
    w.app.ui.preview.lighting.exposure = -1.0;
    let views = w.app.views_of(w.app.doc_key);
    let path = std::env::temp_dir().join(format!("brewed-scene-{}.brewtex", std::process::id()));
    brewed_texture_core::project::save(&path, w.app.doc.package(), &views, w.app.doc.current()).expect("a file");
    w.send(Message::Preview(PreviewMessage::Camera(Camera::default())));
    w.send(Message::Preview(PreviewMessage::LightBy(None)));
    w.app.ui.preview.lighting.exposure = 0.0;
    w.send(Message::Proceed(PendingAction::OpenPath(path.clone())));
    let _ = std::fs::remove_file(&path);
    let shown = w.app.ui.preview.view3d;
    assert_eq!((shown.shape, shown.displacement, shown.tiling), (Shape::Sphere, 0.1, 2.0), "the model, its relief and repeat");
    assert_eq!(w.app.ui.preview.lighting.exposure, -1.0);
    let output = w.app.graph().nodes_of(NodeKind::OUTPUT).map(|n| n.id).next().expect("its output");
    assert_eq!(w.app.panorama_node(), Some(output), "lit by its panorama, as it was");
}
