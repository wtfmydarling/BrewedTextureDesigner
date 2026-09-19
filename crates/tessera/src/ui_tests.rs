//! Headless UI tests: the real views are built and driven with input events the way the
//! window does it, keeping the widget state between frames.

use iced::advanced::clipboard;
use iced::advanced::renderer::Headless;
use iced::advanced::widget::operation::{self, Operation, Outcome, focusable};
use iced::keyboard::{self, Key};
use iced::{Element, Pixels, Point, Size, mouse, window};
use iced_runtime::user_interface::{Cache, UserInterface};
use tessera_core::{Edit, NodeKind};

use crate::app::{App, Dialog, DragPayload, GraphMessage, Message, PendingAction, SelectMode, StartupOptions};
use crate::view;
use crate::view::properties::expression::editor_id;

type View = for<'a> fn(&'a App) -> Element<'a, Message>;

/// A view of an app, rendered without a window.
struct Harness {
    app: App,
    view: View,
    renderer: iced::Renderer,
    cache: Cache,
    size: Size,
}

impl Harness {
    /// `None` when no GPU is available for the headless renderer.
    fn new(view: View, size: Size) -> Option<Harness> {
        let renderer = futures::executor::block_on(<iced::Renderer as Headless>::new(crate::style::ui_font(), Pixels(13.0), None))?;
        let (mut app, _) = App::boot(StartupOptions::default());
        app.ui.window_size = size;
        Some(Harness { app, view, renderer, cache: Cache::default(), size })
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

    /// Feed input events with the cursor at `cursor`; returns how many messages the
    /// widgets produced (they are applied to the app).
    fn input_at(&mut self, cursor: mouse::Cursor, events: &[iced::Event]) -> usize {
        let mut messages = Vec::new();
        self.frame(|ui, renderer| {
            let _ = ui.update(events, cursor, renderer, &mut clipboard::Null, &mut messages);
        });
        let count = messages.len();
        for m in messages {
            self.send(m);
        }
        count
    }

    fn input(&mut self, events: &[iced::Event]) -> usize {
        self.input_at(mouse::Cursor::Unavailable, events)
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

    /// What the window's event subscription reports for a cursor move.
    fn move_cursor(&mut self, to: Point) {
        let event = iced::Event::Mouse(mouse::Event::CursorMoved { position: to });
        self.send(Message::Runtime(event, iced::event::Status::Ignored, window::Id::unique()));
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

macro_rules! harness {
    ($view:expr, $w:expr, $h:expr) => {
        match Harness::new($view, Size::new($w, $h)) {
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

    let before = window.app.graph().nodes.len();
    window.move_cursor(Point::new(60.0, 200.0));
    window.send(Message::PanelPress(DragPayload::Node(NodeKind::BLUR)));
    assert!(window.app.ui.drag.is_none(), "a press alone is not a drag");
    let target = Point::new(bounds.x + 40.0, bounds.y + bounds.height - 40.0);
    window.move_cursor(target);
    assert_eq!(window.app.ui.drag, Some(DragPayload::Node(NodeKind::BLUR)));

    let release = [
        iced::Event::Mouse(mouse::Event::CursorMoved { position: target }),
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
    ];
    window.input_at(mouse::Cursor::Available(target), &release);
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
    let close = || Message::Runtime(iced::Event::Window(window::Event::CloseRequested), iced::event::Status::Ignored, window::Id::unique());
    let _ = app.update(Message::Edit(Edit::SetResolution([512, 512])));
    assert!(app.doc.is_dirty());
    let _ = app.update(close());
    assert_eq!(app.ui.dialog, Some(Dialog::Unsaved(PendingAction::Quit)));
    let _ = app.update(Message::Dialog(None));
    assert_eq!(app.ui.dialog, None);
    assert!(app.doc.is_dirty(), "cancelling keeps the changes");
    assert_eq!(app.graph().resolution, [512, 512]);
}
