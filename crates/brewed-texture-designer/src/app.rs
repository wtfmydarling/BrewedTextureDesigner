//! Application state and the update loop.
//!
//! The UI never changes the graph itself: every user action becomes a
//! [`brewed_texture_core::Edit`] applied to the [`Document`], which records undo steps. After every
//! change the graph is evaluated on the CPU (values, effective parameters) and sent to the
//! engine thread, which answers with images, thumbnails and reports as [`Event`]s.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::{image, operation, pane_grid, text_editor};
use iced::{Point, Rectangle, Size, Subscription, Task, Theme, Vector, keyboard, mouse, window};

use brewed_texture_core::eval::{self, Evaluation};
use brewed_texture_core::graph::{Clipboard, CommentId, Graph, Link, NodeId, SocketRef};
use brewed_texture_core::project::{self, ViewState};
use brewed_texture_core::{Category, Document, Edit, NodeKind, ParamValue, Value};
use brewed_texture_engine::{DisplayImage, EngineService, Event, ExportJob, ImageReport, Request};
use brewed_texture_engine::simd::ImageStats;

use crate::gpu::{self, EventSource};
use crate::eyedropper;
use crate::picker::{self, PickTarget, Shot};
use crate::view;

// ---------------------------------------------------------------------------- options

#[derive(Clone, Debug, Default)]
pub struct StartupOptions {
    pub open: Option<PathBuf>,
    /// Save a screenshot once everything is computed, then quit.
    pub screenshot: Option<PathBuf>,
    /// UI state to show in the screenshot.
    pub scenario: Option<String>,
    /// Select the first node of this type at startup.
    pub select: Option<String>,
    /// Show the first node of this type in the 2D view.
    pub preview: Option<String>,
}

// ---------------------------------------------------------------------------- ui state

/// Something dragged from a panel onto the graph.
#[derive(Clone, Debug, PartialEq)]
pub enum DragPayload {
    Node(NodeKind),
    Variable(String),
}

impl DragPayload {
    pub fn label(&self) -> String {
        match self {
            DragPayload::Node(k) => k.def().name.to_string(),
            DragPayload::Variable(v) => format!("Input '{v}'"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphView {
    pub pan: Vector,
    pub zoom: f32,
    /// Window-space bounds of the canvas (reported by the canvas).
    pub bounds: Rectangle,
}

impl GraphView {
    pub fn to_graph(self, window: Point) -> [f32; 2] {
        let local = window - self.bounds.position();
        [(local.x - self.pan.x) / self.zoom, (local.y - self.pan.y) / self.zoom]
    }

    pub fn center(&self) -> [f32; 2] {
        self.to_graph(self.bounds.center())
    }

    pub fn contains(&self, window: Point) -> bool {
        self.bounds.contains(window)
    }
}

#[derive(Clone, Debug)]
pub struct PreviewState {
    pub node: Option<NodeId>,
    /// 0 RGBA, 1 RGB, 2 R, 3 G, 4 B, 5 A
    pub channel: u32,
    pub show_tiling: bool,
    pub zoom: f32,
    pub pan: Vector,
    pub fit: bool,
    pub bounds: Rectangle,
    pub probe: Option<[u32; 2]>,
    pub view3d: view::view3d::Camera,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pane {
    Library,
    Graph,
    /// The model, on the left of the 2D view, as in the programs this one follows.
    View3D,
    View2D,
    Inputs,
    Properties,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuId {
    File,
    Edit,
    View,
    Help,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ContextTarget {
    Node(NodeId),
    Link(Link),
    Comment(CommentId),
    Canvas { graph_pos: [f32; 2] },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContextMenu {
    pub at: Point,
    pub target: ContextTarget,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AddMenu {
    pub at: Point,
    pub graph_pos: [f32; 2],
    pub query: String,
    pub selected: usize,
    /// Scroll offset of the list, to keep the keyboard selection visible.
    pub scroll: f32,
    /// Dropping a link on empty space: the new node gets connected to this socket.
    pub pending: Option<SocketRef>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PendingAction {
    New,
    Open,
    OpenPath(PathBuf),
    LoadDemo,
    Quit,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    Unsaved(PendingAction),
    About,
    Shortcuts,
}

#[derive(Clone, Debug)]
pub struct Status {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}

/// Which color value an open color picker edits.
#[derive(Clone, Debug, PartialEq)]
pub enum ColorTarget {
    Param(NodeId, String),
    Variable(String, VarField),
    GradientStop(NodeId, String),
    Comment(CommentId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarField {
    Value,
    Default,
}

/// The open menu of a choice picked by its pictures (see [`crate::view::choices`]).
#[derive(Clone, Debug, PartialEq)]
pub struct ChoiceMenu {
    pub node: NodeId,
    pub key: &'static str,
    /// The option the arrow keys are on (Enter picks it).
    pub cursor: usize,
}

/// Pictures of what a node makes with each option of a choice, as the engine drew them.
pub struct ChoicePictures {
    /// The version of the node's image they were drawn for.
    pub version: u64,
    pub pictures: Vec<image::Handle>,
}

/// What the last change that computed anything did, and how long it took.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Update {
    pub recomputed: usize,
    pub image_nodes: usize,
    /// From the moment the engine started on the change until the GPU was done with it.
    pub millis: f32,
}

/// A color or gradient being picked from the screen. Exists from the click on the
/// eyedropper until the pick is done or cancelled.
#[derive(Debug)]
pub struct ScreenPick {
    pub target: PickTarget,
    /// Picking live ([`crate::eyedropper`]); false when picking on a capture instead
    /// ([`crate::picker`]).
    pub live: bool,
    /// The captured monitors (empty while capturing).
    pub shots: Vec<Shot>,
    /// The window covering each monitor, in the order of `shots`.
    pub windows: Vec<window::Id>,
    /// Picked or cancelled. The windows keep showing the capture until they are closed.
    pub done: bool,
    /// Windows that are closed already.
    pub closed: Vec<window::Id>,
}

impl ScreenPick {
    pub fn new(target: PickTarget) -> ScreenPick {
        ScreenPick { target, live: true, shots: Vec::new(), windows: Vec::new(), done: false, closed: Vec::new() }
    }

    /// The shot a picker window shows.
    pub fn shot_of(&self, window: window::Id) -> Option<usize> {
        self.windows.iter().position(|w| *w == window)
    }
}

pub struct Ui {
    pub selection: BTreeSet<NodeId>,
    pub selected_link: Option<Link>,
    /// The comment frame being worked on: it shows its text and color in the properties.
    pub selected_comment: Option<CommentId>,
    pub graph_view: GraphView,
    pub frame_request: bool,
    pub preview: PreviewState,
    pub panes: pane_grid::State<Pane>,
    pub library_search: String,
    pub library_collapsed: HashSet<Category>,
    pub menu: Option<MenuId>,
    pub context: Option<ContextMenu>,
    pub add_menu: Option<AddMenu>,
    pub dialog: Option<Dialog>,
    pub after_save: Option<PendingAction>,
    pub drag: Option<DragPayload>,
    /// A panel item was pressed; it becomes a drag once the cursor moves.
    pub press: Option<(DragPayload, Point)>,
    pub cursor: Point,
    pub modifiers: keyboard::Modifiers,
    pub window_size: Size,
    pub lock_square: bool,
    pub status: Option<Status>,
    pub clipboard: Option<Clipboard>,
    pub color_picker: Option<ColorTarget>,
    /// Where the color picker floats: where it was opened from.
    pub color_picker_at: Point,
    pub choice_menu: Option<ChoiceMenu>,
    /// Selected stop of each gradient editor.
    pub gradient_stop: HashMap<(NodeId, String), usize>,
    /// The expression editor's text buffer (node, content).
    pub expression: Option<(NodeId, text_editor::Content)>,
    pub plot_var: usize,
    pub plot_range: (f32, f32),
    pub expanded_vars: HashSet<String>,
    pub renaming_var: Option<(String, String)>,
    /// Variable name being typed for an Input node.
    pub binding: Option<(NodeId, String)>,
    pub show_functions: bool,
    /// Picking a color or a gradient from the screen (see [`crate::picker`]).
    pub screen_pick: Option<ScreenPick>,
    /// Files dropped on the window in quick succession (time of the last, count), to
    /// lay the new nodes out in a column.
    pub drop_burst: Option<(Instant, u32)>,
}

/// What the engine told us.
#[derive(Default)]
pub struct Results {
    pub reports: HashMap<NodeId, ImageReport>,
    pub thumbs: HashMap<NodeId, image::Handle>,
    pub preview: Option<DisplayImage>,
    pub materials: Vec<DisplayImage>,
    pub stats: HashMap<NodeId, (u64, Arc<ImageStats>)>,
    pub stats_requested: HashMap<NodeId, u64>,
    pub probe: Option<(NodeId, [u32; 2], [f32; 4])>,
    /// The pictures of the options of the choices whose menus were opened.
    pub choices: HashMap<(NodeId, String), ChoicePictures>,
    /// The last change that computed anything (changes that computed nothing, like moving a
    /// node, leave it as it was).
    pub update: Option<Update>,
    pub evaluated_revision: u64,
    /// The last revision the GPU is done with.
    pub finished_revision: u64,
    /// Since when the engine has been at the change it is computing now: since it was sent,
    /// or since the change before it was done. None once every change sent is done.
    pub busy_since: Option<Instant>,
    /// A change took long enough to say so. "Computing…" then stays until every change is
    /// done, rather than blink between the changes of a drag.
    pub slow: bool,
    pub exports_pending: usize,
}

pub struct App {
    pub doc: Document,
    pub eval: Arc<Evaluation>,
    eval_revision: u64,
    pub engine: Option<EngineService>,
    events: Option<EventSource>,
    pub adapter: String,
    pub res: Results,
    pub ui: Ui,
    opts: StartupOptions,
    window: Option<window::Id>,
    started: Instant,
    screenshot_requested: bool,
    /// What the eyedropper needs to know about the app (its Wayland connection).
    host: eyedropper::Host,
}

// ---------------------------------------------------------------------------- messages

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectMode {
    Replace,
    Toggle,
    Add,
}

#[derive(Clone, Debug)]
pub enum GraphMessage {
    Select { node: NodeId, mode: SelectMode },
    SelectBox { ids: Vec<NodeId>, additive: bool },
    ClearSelection,
    SelectLink(Link),
    SelectComment(CommentId),
    /// Select a comment and put the cursor in its text field (double-click on its title).
    EditComment(CommentId),
    Move { ids: Vec<NodeId>, delta: [f32; 2] },
    /// A comment dragged by its title bar, with the nodes it holds.
    MoveComment { id: CommentId, delta: [f32; 2], nodes: Vec<NodeId> },
    /// A comment resized by its corner.
    ResizeComment { id: CommentId, size: [f32; 2] },
    /// A link dragged to a socket; `replacing` is the link it was picked up from.
    Connect { from: SocketRef, to: SocketRef, replacing: Option<Link> },
    /// A link dropped on empty space (the add menu opens); a picked up link is removed.
    LinkDropped { from: SocketRef, at: Point, graph_pos: [f32; 2], detached: Option<Link> },
    OpenAddMenu { at: Point, graph_pos: [f32; 2] },
    Preview(NodeId),
    Context { at: Point, target: ContextTarget },
    /// The canvas moved by this much, in pixels.
    PanBy(Vector),
    /// Zoom by `factor`, keeping the point at `at` (canvas pixels) where it is.
    ZoomAt { at: Point, factor: f32 },
    Bounds(Rectangle),
    Drop { payload: DragPayload, graph_pos: [f32; 2] },
    Error(String),
}

#[derive(Clone, Debug)]
pub enum PreviewMessage {
    Channel(u32),
    ToggleTiling,
    Fit,
    OneToOne,
    View { zoom: f32, pan: Vector, fit: bool },
    /// The image moved by this much, in pixels.
    PanBy(Vector),
    /// Zoom by `factor`, keeping the point at `at` (view pixels) where it is.
    ZoomAt { at: Point, factor: f32 },
    /// The camera of the 3D view moved by the mouse.
    Move3d(view::view3d::Move),
    Bounds(Rectangle),
    Probe(Option<[u32; 2]>),
    Camera(view::view3d::Camera),
}

#[derive(Clone, Debug)]
pub enum PickMessage {
    /// The eyedropper was clicked: start picking.
    Start(PickTarget),
    /// The live pick ended.
    Picked(eyedropper::Outcome),
    /// Picking on a capture: the monitors are captured.
    Captured(Result<Vec<Shot>, String>),
    /// A click on a picker window, at a pixel of its shot.
    Color { shot: usize, at: [f32; 2] },
    /// A path dragged on a picker window, in the coordinates of that window.
    Path { shot: usize, points: Vec<[f32; 2]> },
    Cancel,
}

#[derive(Clone, Debug)]
pub enum AddMenuMessage {
    Query(String),
    Choose(NodeKind),
    /// Move the keyboard selection by this many rows.
    Move(i32),
    /// The list was scrolled to this offset.
    Scrolled(f32),
    Submit,
    Close,
}

#[derive(Clone, Debug)]
pub enum Message {
    PollGpu,
    GpuName(String),
    Engine(Event),
    Runtime(iced::Event, iced::event::Status, window::Id),
    Tick,
    // document
    Edit(Edit),
    Edits(Vec<Edit>),
    Seal,
    Undo,
    Redo,
    // files
    New,
    Open,
    OpenPicked(Option<PathBuf>),
    Save,
    SaveAs,
    SavePicked(Option<PathBuf>),
    ExportAll,
    ExportFolderPicked(Option<PathBuf>),
    ExportNode(NodeId),
    ExportNodePicked(NodeId, Option<PathBuf>),
    LoadDemo,
    Quit,
    Proceed(PendingAction),
    /// Save, then continue with the action (unsaved changes dialog).
    SaveThen(PendingAction),
    Dialog(Option<Dialog>),
    // selection & clipboard
    Graph(GraphMessage),
    SelectNode(NodeId),
    Copy,
    Cut,
    Paste,
    ClipboardText(Option<String>),
    Duplicate,
    Delete,
    /// Frame the selected nodes in a comment (or add an empty one at the cursor).
    AddComment,
    SelectAll,
    /// Frame the selection, or everything when nothing is selected.
    Frame,
    FrameAll,
    ToggleBypass(NodeId),
    // layout
    PaneResized(pane_grid::ResizeEvent),
    ResetLayout,
    LockSquare,
    Menu(Option<MenuId>),
    /// The mouse is over a menu title: switch to it if another menu is open.
    MenuHover(MenuId),
    /// A menu item was chosen: close the menus, then handle the item's message.
    FromMenu(Box<Message>),
    Context(Option<ContextMenu>),
    AddMenu(AddMenuMessage),
    // library & drag and drop
    LibrarySearch(String),
    LibraryToggle(Category),
    PanelPress(DragPayload),
    PanelAdd(DragPayload),
    // properties
    /// Open the menu of a choice picked by its pictures, or close it.
    ChoiceMenu(Option<(NodeId, &'static str)>),
    /// An option of such a choice was picked.
    PickChoice(NodeId, &'static str, u32),
    Expression(NodeId, text_editor::Action),
    ColorPicker(Option<ColorTarget>),
    GradientStop(NodeId, String, usize),
    /// Replace a gradient and select one of its stops.
    SetGradient(NodeId, String, brewed_texture_core::Gradient, Option<usize>),
    /// Typing the variable name of an Input node (applied on Enter).
    BindingText(NodeId, String),
    CommitBinding,
    ShowFunctions,
    PlotVar(usize),
    PlotRange(f32, f32),
    BrowseFile(NodeId, String),
    FilePicked(NodeId, String, Option<PathBuf>),
    ReloadBitmap(NodeId),
    AutoLevels(NodeId),
    // input values
    ToggleVariable(String),
    RenameVariable(String, String),
    CommitRename,
    // 2D / 3D view
    Preview(PreviewMessage),
    // eyedropper
    Pick(PickMessage),
    /// What the eyedropper needs to know about the app, found once its window is open.
    Host(eyedropper::Host),
    Screenshot(window::Screenshot),
}

// ---------------------------------------------------------------------------- boot

fn main_window_settings() -> window::Settings {
    window::Settings {
        size: Size::new(1680.0, 1000.0),
        min_size: Some(Size::new(1000.0, 640.0)),
        // Closing asks about unsaved changes first.
        exit_on_close_request: false,
        icon: crate::brand::window_icon(),
        ..Default::default()
    }
}

fn default_panes() -> pane_grid::State<Pane> {
    use pane_grid::{Axis, Configuration as C};
    let split = |axis, ratio, a, b| C::Split { axis, ratio, a: Box::new(a), b: Box::new(b) };
    pane_grid::State::with_configuration(split(
        Axis::Vertical,
        0.125,
        C::Pane(Pane::Library),
        split(
            Axis::Vertical,
            0.765,
            split(
                Axis::Horizontal,
                0.6,
                C::Pane(Pane::Graph),
                split(Axis::Vertical, 0.42, C::Pane(Pane::View3D), C::Pane(Pane::View2D)),
            ),
            split(Axis::Horizontal, 0.34, C::Pane(Pane::Inputs), C::Pane(Pane::Properties)),
        ),
    ))
}

fn find_kind(graph: &Graph, name: &str) -> Option<NodeId> {
    let name = name.to_lowercase().replace([' ', '-'], "_");
    graph
        .nodes
        .iter()
        .find(|n| n.kind.id() == name || n.def().name.to_lowercase().replace(' ', "_") == name)
        .map(|n| n.id)
}

impl App {
    pub fn boot(opts: StartupOptions) -> (App, Task<Message>) {
        let (graph, preview_node) = brewed_texture_core::demo::graph();
        let eval = Arc::new(eval::evaluate(&graph));
        let mut app = App {
            doc: Document::new(graph, None),
            eval,
            eval_revision: 0,
            engine: None,
            events: None,
            adapter: String::new(),
            res: Results::default(),
            ui: Ui {
                selection: BTreeSet::new(),
                selected_link: None,
                selected_comment: None,
                graph_view: GraphView { pan: Vector::ZERO, zoom: 1.0, bounds: Rectangle::default() },
                frame_request: true,
                preview: PreviewState {
                    node: preview_node,
                    channel: 0,
                    show_tiling: false,
                    zoom: 1.0,
                    pan: Vector::ZERO,
                    fit: true,
                    bounds: Rectangle::default(),
                    probe: None,
                    view3d: view::view3d::Camera::default(),
                },
                panes: default_panes(),
                library_search: String::new(),
                library_collapsed: HashSet::new(),
                menu: None,
                context: None,
                add_menu: None,
                dialog: None,
                after_save: None,
                drag: None,
                press: None,
                cursor: Point::ORIGIN,
                modifiers: keyboard::Modifiers::default(),
                window_size: Size::new(1680.0, 1000.0),
                lock_square: true,
                status: None,
                clipboard: None,
                color_picker: None,
                color_picker_at: Point::ORIGIN,
                choice_menu: None,
                gradient_stop: HashMap::new(),
                expression: None,
                plot_var: 0,
                plot_range: (-5.0, 5.0),
                expanded_vars: HashSet::new(),
                renaming_var: None,
                binding: None,
                show_functions: false,
                screen_pick: None,
                drop_burst: None,
            },
            opts: opts.clone(),
            window: None,
            started: Instant::now(),
            screenshot_requested: false,
            host: eyedropper::Host::default(),
        };
        if let Some(path) = opts.open.clone() {
            app.open_path(path);
        }
        let graph = app.doc.graph();
        if let Some(id) = opts.select.as_deref().and_then(|k| find_kind(graph, k)) {
            app.ui.selection.insert(id);
        }
        if let Some(id) = opts.preview.as_deref().and_then(|k| find_kind(graph, k)) {
            app.ui.preview.node = Some(id);
        }
        app.refresh();
        let scenario = match opts.scenario.as_deref() {
            Some(s) => app.apply_scenario(s),
            None => Task::none(),
        };
        app.sync_expression();
        // The app runs as a daemon (the eyedropper opens more windows): it opens its main
        // window itself.
        let (main, open) = window::open(main_window_settings());
        app.window = Some(main);
        let setup = open.then(|id| {
            #[cfg(windows)]
            let icon = window::run(id, crate::brand::use_embedded_icon).discard();
            #[cfg(not(windows))]
            let icon = Task::none();
            let host = window::run(id, |window: &dyn iced::window::Window| {
                // Where the pointer is locked while a value is dragged, and what the
                // eyedropper needs to cover the screen.
                crate::scrub::remember(window);
                eyedropper::Host::of(window)
            });
            Task::batch([icon, host.map(Message::Host)])
        });
        (app, Task::batch([setup, scenario]))
    }

    /// The main window (as opposed to the eyedropper's windows).
    #[cfg(test)]
    pub fn main_window(&self) -> Option<window::Id> {
        self.window
    }

    pub fn title(&self, window: window::Id) -> String {
        if Some(window) != self.window {
            return format!("{} — pick from the screen", brewed_texture_core::APP_NAME);
        }
        let dirty = if self.doc.is_dirty() { " •" } else { "" };
        format!("{}{dirty} — {}", self.doc.title(), brewed_texture_core::APP_NAME)
    }

    pub fn theme(&self, _window: window::Id) -> Theme {
        crate::style::theme()
    }

    pub fn graph(&self) -> &Graph {
        self.doc.graph()
    }

    /// What the eyedropper is picking, while it is.
    pub fn picking(&self) -> Option<&PickTarget> {
        self.ui.screen_pick.as_ref().map(|p| &p.target)
    }

    // ------------------------------------------------------------------ subscriptions

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![iced::event::listen_with(|event, status, window| {
            use iced::Event as E;
            let relevant = matches!(
                &event,
                E::Keyboard(_)
                    | E::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::ButtonReleased(_))
                    | E::Window(
                        window::Event::CloseRequested
                            | window::Event::Closed
                            | window::Event::Opened { .. }
                            | window::Event::Resized(_)
                            | window::Event::FileDropped(_),
                    )
            );
            relevant.then_some(Message::Runtime(event, status, window))
        })];
        match &self.events {
            Some(source) => subs.push(source.subscription().map(Message::Engine)),
            None => subs.push(iced::time::every(Duration::from_millis(30)).map(|_| Message::PollGpu)),
        }
        if self.ui.status.is_some() || self.opts.screenshot.is_some() {
            subs.push(iced::time::every(Duration::from_millis(500)).map(|_| Message::Tick));
        }
        // A change that takes long says so in the status bar (see `view::toolbar`), without
        // waiting for anything else to happen.
        if self.res.busy_since.is_some() {
            subs.push(iced::time::every(Duration::from_millis(100)).map(|_| Message::Tick));
        }
        Subscription::batch(subs)
    }

    // ------------------------------------------------------------------ helpers

    pub fn set_status(&mut self, text: impl Into<String>, error: bool) {
        let text = text.into();
        if error {
            log::warn!("{text}");
        } else {
            log::info!("{text}");
        }
        self.ui.status = Some(Status { text, error, at: Instant::now() });
    }

    fn send(&self, request: Request) {
        if let Some(engine) = &self.engine {
            engine.send(request);
        }
    }

    /// Re-evaluate after document changes and keep the UI state consistent with the graph.
    fn refresh(&mut self) {
        if self.doc.revision() == self.eval_revision {
            return;
        }
        let graph = self.doc.graph();
        self.eval = Arc::new(eval::evaluate(graph));
        self.eval_revision = self.doc.revision();
        self.ui.selection.retain(|id| graph.contains(*id));
        if self.ui.selected_link.as_ref().is_some_and(|l| !graph.links.contains(l)) {
            self.ui.selected_link = None;
        }
        if self.ui.preview.node.is_some_and(|id| !graph.contains(id)) {
            self.ui.preview.node = None;
        }
        self.res.thumbs.retain(|id, _| graph.contains(*id));
        self.res.choices.retain(|(id, _), _| graph.contains(*id));
        // A menu of pictures belongs to a choice that is still there to be made.
        if let Some(menu) = &self.ui.choice_menu
            && !graph.node(menu.node).is_some_and(|n| n.def().param(menu.key).is_some_and(|pd| n.param_visible(pd)) && !n.is_exposed(menu.key))
        {
            self.ui.choice_menu = None;
        }
        self.send_graph();
    }

    fn send_graph(&mut self) {
        if self.engine.is_none() {
            return;
        }
        self.res.busy_since.get_or_insert_with(Instant::now);
        self.send(Request::Evaluate {
            revision: self.doc.revision(),
            graph: Arc::new(self.doc.graph().clone()),
            eval: self.eval.clone(),
        });
        self.send(Request::Preview(self.ui.preview.node));
        self.send(Request::Materials(self.material_nodes()));
    }

    /// Output nodes the 3D view uses (one per usage). The 3D view is always on screen, so
    /// the maps it shows are always kept up to date.
    pub fn material_nodes(&self) -> Vec<NodeId> {
        let mut seen = HashSet::new();
        self.graph()
            .nodes_of(NodeKind::OUTPUT)
            .filter(|n| n.enum_value("usage") != 0 && seen.insert(n.enum_value("usage")))
            .map(|n| n.id)
            .collect()
    }

    fn apply(&mut self, edit: Edit) -> bool {
        match self.doc.apply(edit) {
            Ok(result) => {
                if let Some(n) = result.notice {
                    self.set_status(n, true);
                }
                if !result.created.is_empty() {
                    self.ui.selection = result.created.iter().copied().collect();
                    self.ui.selected_link = None;
                }
                result.changed
            }
            Err(e) => {
                self.set_status(e, true);
                false
            }
        }
    }

    /// Several edits from one action: one undo step. (A single edit is applied as is, so
    /// that the steps of a drag still merge.)
    fn apply_all(&mut self, mut edits: Vec<Edit>) {
        match edits.len() {
            0 => {}
            1 => {
                self.apply(edits.remove(0));
            }
            _ => {
                self.apply(Edit::Batch(edits));
            }
        }
    }

    fn selected_ids(&self) -> Vec<NodeId> {
        self.ui.selection.iter().copied().collect()
    }

    fn set_preview(&mut self, node: Option<NodeId>) {
        if self.ui.preview.node != node {
            self.ui.preview.node = node;
            self.ui.preview.fit = true;
            self.res.probe = None;
            self.send(Request::Preview(node));
        }
    }

    fn close_popups(&mut self) {
        self.ui.menu = None;
        self.ui.context = None;
        self.ui.add_menu = None;
        self.ui.choice_menu = None;
    }

    /// Open the menu of a choice picked by its pictures, and have the engine draw them.
    fn open_choice_menu(&mut self, node: NodeId, key: &'static str) {
        self.close_popups();
        let Some(current) = self.graph().node(node).map(|n| n.enum_value(key) as usize) else {
            return;
        };
        self.ui.choice_menu = Some(ChoiceMenu { node, key, cursor: current });
        self.send(Request::Choices { node, key: key.to_string() });
    }

    /// Move the keyboard through the open menu of pictures: left and right go from option
    /// to option, up and down from row to row.
    fn move_in_choice_menu(&mut self, key: &keyboard::Key) -> bool {
        use keyboard::key::Named;
        let Some(menu) = &self.ui.choice_menu else { return false };
        let options = self.graph().node(menu.node).and_then(|n| n.def().param(menu.key)).map_or(0, |pd| pd.enum_options().len());
        if options == 0 {
            return false;
        }
        let columns = view::choices::columns(options) as isize;
        let step = match key {
            keyboard::Key::Named(Named::ArrowLeft) => -1,
            keyboard::Key::Named(Named::ArrowRight) => 1,
            keyboard::Key::Named(Named::ArrowUp) => -columns,
            keyboard::Key::Named(Named::ArrowDown) => columns,
            keyboard::Key::Named(Named::Home) => -(options as isize),
            keyboard::Key::Named(Named::End) => options as isize,
            _ => return false,
        };
        let cursor = (menu.cursor as isize + step).clamp(0, options as isize - 1) as usize;
        if let Some(menu) = &mut self.ui.choice_menu {
            menu.cursor = cursor;
        }
        true
    }

    /// Frame the selection (or everything) in the graph view.
    fn frame(&mut self, only_selected: bool) {
        let ids = self.selected_ids();
        let graph = self.doc.graph();
        let size = |n: &brewed_texture_core::Node| view::graph::layout::node_size(graph, n);
        let bounds = if only_selected && !ids.is_empty() { graph.bounds(Some(&ids), size) } else { graph.bounds(None, size) };
        let canvas = self.ui.graph_view.bounds;
        if canvas.width < 10.0 {
            self.ui.frame_request = true;
            return;
        }
        self.ui.frame_request = false;
        let Some([x0, y0, x1, y1]) = bounds else {
            self.ui.graph_view.zoom = 1.0;
            self.ui.graph_view.pan = Vector::new(canvas.width * 0.5, canvas.height * 0.5);
            return;
        };
        let margin = 50.0;
        let zoom = ((canvas.width - 2.0 * margin) / (x1 - x0).max(1.0))
            .min((canvas.height - 2.0 * margin) / (y1 - y0).max(1.0))
            .clamp(0.15, 1.25);
        let center = [(x0 + x1) * 0.5, (y0 + y1) * 0.5];
        self.ui.graph_view.zoom = zoom;
        self.ui.graph_view.pan =
            Vector::new(canvas.width * 0.5 - center[0] * zoom, canvas.height * 0.5 - center[1] * zoom);
    }

    /// Pan the graph view so that a node is fully visible; the zoom stays.
    fn reveal(&mut self, id: NodeId) {
        let graph = self.doc.graph();
        let Some(node) = graph.node(id) else { return };
        let size = view::graph::layout::node_size(graph, node);
        let v = &mut self.ui.graph_view;
        let canvas = v.bounds;
        if canvas.width < 10.0 {
            return;
        }
        let x0 = node.pos[0] * v.zoom + v.pan.x;
        let y0 = node.pos[1] * v.zoom + v.pan.y;
        let (x1, y1) = (x0 + size[0] * v.zoom, y0 + size[1] * v.zoom);
        let margin = 24.0;
        if x0 >= margin && y0 >= margin && x1 <= canvas.width - margin && y1 <= canvas.height - margin {
            return;
        }
        let center = [node.pos[0] + size[0] * 0.5, node.pos[1] + size[1] * 0.5];
        v.pan = Vector::new(canvas.width * 0.5 - center[0] * v.zoom, canvas.height * 0.5 - center[1] * v.zoom);
    }

    fn graph_pos_for_paste(&self) -> [f32; 2] {
        let v = &self.ui.graph_view;
        if v.contains(self.ui.cursor) { v.to_graph(self.ui.cursor) } else { v.center() }
    }

    fn copy_selection(&mut self) -> Task<Message> {
        let ids = self.selected_ids();
        if ids.is_empty() {
            return Task::none();
        }
        let clip = self.doc.graph().copy_nodes(&ids);
        let text = clip.to_text();
        self.ui.clipboard = Some(clip);
        iced::clipboard::write(text)
    }

    fn paste_clipboard(&mut self, clip: Clipboard) {
        let at = self.graph_pos_for_paste();
        self.apply(Edit::Paste { clipboard: clip, at });
        self.doc.seal();
    }

    fn delete_selection(&mut self) {
        if let Some(link) = self.ui.selected_link.take() {
            self.apply(Edit::RemoveLink(link));
        }
        // A comment goes away on its own: the nodes it framed stay where they are.
        if let Some(id) = self.ui.selected_comment.take() {
            self.apply(Edit::RemoveComments(vec![id]));
        }
        let ids = self.selected_ids();
        if !ids.is_empty() {
            self.apply(Edit::RemoveNodes(ids));
            self.ui.selection.clear();
        }
        self.doc.seal();
    }

    /// Frame the selected nodes in a comment, or add an empty one where the cursor is.
    fn add_comment(&mut self) {
        let graph = self.graph();
        let mut bounds: Option<[f32; 4]> = None;
        for node in graph.nodes.iter().filter(|n| self.ui.selection.contains(&n.id)) {
            let size = view::graph::layout::node_size(graph, node);
            let r = [node.pos[0], node.pos[1], node.pos[0] + size[0], node.pos[1] + size[1]];
            bounds = Some(match bounds {
                Some(b) => [b[0].min(r[0]), b[1].min(r[1]), b[2].max(r[2]), b[3].max(r[3])],
                None => r,
            });
        }
        // Room for the title bar above the nodes, and a margin all around them.
        const MARGIN: f32 = 26.0;
        use view::graph::layout;
        let (pos, size) = match bounds {
            Some(b) => (
                [b[0] - MARGIN, b[1] - MARGIN - layout::COMMENT_HEADER_H],
                [b[2] - b[0] + 2.0 * MARGIN, b[3] - b[1] + 2.0 * MARGIN + layout::COMMENT_HEADER_H],
            ),
            None => {
                let at = self.ui.graph_view.to_graph(self.ui.cursor);
                ([at[0] - 20.0, at[1] - 20.0], [340.0, 260.0])
            }
        };
        self.close_popups();
        if self.apply(Edit::AddComment { text: String::new(), pos, size }) {
            self.doc.seal();
            self.ui.selected_comment = self.graph().comments.last().map(|c| c.id);
            self.ui.selection.clear();
            self.ui.selected_link = None;
        }
    }

    fn open_add_menu(&mut self, at: Point, graph_pos: [f32; 2], pending: Option<SocketRef>) -> Task<Message> {
        self.close_popups();
        self.ui.add_menu = Some(AddMenu { at, graph_pos, query: String::new(), selected: 0, scroll: 0.0, pending });
        operation::focus(view::add_menu::search_id())
    }

    // ------------------------------------------------------------------ files

    fn open_path(&mut self, path: PathBuf) {
        match project::load(&path) {
            Ok(loaded) => {
                let preview = loaded.view.preview.or_else(|| loaded.graph.nodes_of(NodeKind::OUTPUT).next().map(|n| n.id));
                self.doc.replace(loaded.graph, Some(path.clone()));
                self.after_document_change(preview);
                if let (Some(pan), Some(zoom)) = (loaded.view.graph_pan, loaded.view.graph_zoom) {
                    self.ui.graph_view.pan = Vector::new(pan[0], pan[1]);
                    self.ui.graph_view.zoom = zoom.clamp(0.1, 3.0);
                    self.ui.frame_request = false;
                }
                let mut msg = format!("Opened {}", path.display());
                if !loaded.notes.is_empty() {
                    msg.push_str(&format!(" ({})", loaded.notes.join("; ")));
                }
                self.set_status(msg, false);
            }
            Err(e) => self.set_status(e, true),
        }
    }

    fn after_document_change(&mut self, preview: Option<NodeId>) {
        // Node ids start over in the new document: forget everything that points at nodes
        // or variables of the old one.
        self.close_popups();
        self.ui.selection.clear();
        self.ui.selected_link = None;
        self.ui.expression = None;
        self.ui.color_picker = None;
        self.ui.gradient_stop.clear();
        self.ui.expanded_vars.clear();
        self.ui.renaming_var = None;
        self.ui.binding = None;
        self.ui.plot_var = 0;
        self.ui.drag = None;
        self.ui.press = None;
        self.ui.frame_request = true;
        self.ui.preview.node = preview;
        self.ui.preview.fit = true;
        self.res = Results::default();
        self.send(Request::Reset);
        self.refresh();
    }

    fn save_to(&mut self, path: PathBuf) -> bool {
        let view = ViewState {
            preview: self.ui.preview.node,
            graph_pan: Some([self.ui.graph_view.pan.x, self.ui.graph_view.pan.y]),
            graph_zoom: Some(self.ui.graph_view.zoom),
        };
        match project::save(&path, self.doc.graph(), &view) {
            Ok(()) => {
                self.set_status(format!("Saved {}", path.display()), false);
                self.doc.mark_saved(path);
                true
            }
            Err(e) => {
                self.set_status(e, true);
                false
            }
        }
    }

    fn pick_save_path(&self) -> Task<Message> {
        let hint = self.doc.path();
        let name = hint.and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string());
        let name = name.unwrap_or_else(|| format!("untitled.{}", project::FILE_EXTENSION));
        let folder = hint.and_then(|p| p.parent()).filter(|d| d.is_dir()).map(PathBuf::from);
        Task::perform(
            blocking(move || {
                let mut dialog = rfd::FileDialog::new()
                    .set_title("Save graph")
                    .add_filter(format!("{} graph", brewed_texture_core::APP_NAME), &[project::FILE_EXTENSION])
                    .set_file_name(name);
                if let Some(folder) = folder {
                    dialog = dialog.set_directory(folder);
                }
                dialog.save_file()
            }),
            |p| Message::SavePicked(p.flatten()),
        )
    }

    /// Run an action that would discard unsaved changes, asking first.
    fn guarded(&mut self, action: PendingAction) -> Task<Message> {
        self.close_popups();
        if self.doc.is_dirty() {
            self.ui.dialog = Some(Dialog::Unsaved(action));
            Task::none()
        } else {
            self.proceed(action)
        }
    }

    fn proceed(&mut self, action: PendingAction) -> Task<Message> {
        self.ui.dialog = None;
        match action {
            PendingAction::New => {
                self.doc.replace(Graph::default(), None);
                self.after_document_change(None);
                Task::none()
            }
            PendingAction::LoadDemo => {
                let (g, p) = brewed_texture_core::demo::graph();
                self.doc.replace(g, None);
                self.after_document_change(p);
                Task::none()
            }
            PendingAction::OpenPath(path) => {
                self.open_path(path);
                Task::none()
            }
            PendingAction::Open => Task::perform(
                blocking(|| {
                    rfd::FileDialog::new()
                        .set_title("Open graph")
                        .add_filter(format!("{} graph", brewed_texture_core::APP_NAME), &[project::FILE_EXTENSION, "json"])
                        .pick_file()
                }),
                |p| Message::OpenPicked(p.flatten()),
            ),
            PendingAction::Quit => iced::exit(),
        }
    }

    fn export_jobs(&self, dir: &std::path::Path) -> Vec<ExportJob> {
        let graph = self.doc.graph();
        let outputs: Vec<&brewed_texture_core::Node> = graph.nodes_of(NodeKind::OUTPUT).collect();
        let names: Vec<(String, u32)> =
            outputs.iter().map(|n| (n.text("name").to_string(), n.enum_value("bit_depth"))).collect();
        let paths = brewed_texture_engine::export::output_paths(dir, &names);
        outputs
            .iter()
            .zip(paths)
            .map(|(n, path)| ExportJob {
                node: n.id,
                path,
                format: self.eval.params(n.id).map_or(n.enum_value("format"), |p| p.u("format")),
                bit_depth: n.enum_value("bit_depth"),
            })
            .collect()
    }

    // ------------------------------------------------------------------ update

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.handle(message);
        self.refresh();
        self.sync_expression();
        task
    }

    /// Keep the expression editor's buffer in sync with the selected node. The buffer is
    /// only replaced when the text changed from outside (selection, undo), so typing never
    /// loses the cursor or the focus.
    fn sync_expression(&mut self) {
        let single = (self.ui.selection.len() == 1).then(|| self.ui.selection.iter().next().copied()).flatten();
        let Some(node) = single.and_then(|id| self.doc.graph().node(id)) else {
            self.ui.expression = None;
            return;
        };
        let Some(pd) = node.def().expression_param() else {
            self.ui.expression = None;
            return;
        };
        let text = node.text(pd.key);
        match &self.ui.expression {
            Some((id, content)) if *id == node.id && content.text().trim_end_matches(['\n', '\r']) == text => {}
            _ => {
                // Changed elsewhere (another node selected, undo): typing continues at the end.
                let mut content = text_editor::Content::with_text(text);
                content.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
                self.ui.expression = Some((node.id, content));
            }
        }
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PollGpu => {
                if let Some(handles) = gpu::take() {
                    let (service, source) = gpu::start_engine(handles, "GPU".into());
                    self.engine = Some(service);
                    self.events = Some(source);
                    self.send_graph();
                    return Task::perform(blocking(gpu::adapter_name), |name| Message::GpuName(name.unwrap_or_default()));
                }
            }
            Message::GpuName(name) => {
                log::info!("GPU: {name}");
                self.adapter = name;
            }
            Message::Engine(event) => return self.engine_event(event),
            Message::Runtime(event, status, id) => return self.runtime_event(event, status, id),
            Message::Tick => {
                if self.ui.status.as_ref().is_some_and(|s| s.at.elapsed() > Duration::from_secs(if s.error { 12 } else { 6 })) {
                    self.ui.status = None;
                }
                if self.res.busy_since.is_some_and(|since| since.elapsed() >= view::toolbar::COMPUTING_AFTER) {
                    self.res.slow = true;
                }
                return self.maybe_screenshot();
            }
            Message::Edit(edit) => {
                let large = match &edit {
                    Edit::SetResolution([w, h]) => (*w as u64 * *h as u64 >= 4096 * 4096).then_some([*w, *h]),
                    _ => None,
                };
                if self.apply(edit)
                    && let Some([w, h]) = large
                {
                    // Every image node keeps an rgba16float image (8 bytes a pixel).
                    let images = self.graph().nodes.iter().filter(|n| n.def().has_image()).count().max(1) as u64;
                    let gb = (w as u64 * h as u64 * 8 * images) as f64 / (1u64 << 30) as f64;
                    self.set_status(format!("{w}×{h}: about {gb:.1} GB of GPU memory for the {images} image nodes"), false);
                }
            }
            Message::Edits(edits) => self.apply_all(edits),
            Message::Seal => self.doc.seal(),
            // The expression editor's text follows through `sync_expression`; its buffer is
            // kept so the editor stays focused.
            Message::Undo => {
                self.close_popups();
                if !self.doc.undo() {
                    self.set_status("Nothing to undo", false);
                }
            }
            Message::Redo => {
                self.close_popups();
                if !self.doc.redo() {
                    self.set_status("Nothing to redo", false);
                }
            }
            Message::New => return self.guarded(PendingAction::New),
            Message::Open => return self.guarded(PendingAction::Open),
            Message::LoadDemo => return self.guarded(PendingAction::LoadDemo),
            Message::Quit => return self.guarded(PendingAction::Quit),
            Message::Proceed(action) => return self.proceed(action),
            Message::SaveThen(action) => {
                self.ui.dialog = None;
                self.ui.after_save = Some(action);
                return self.handle(Message::Save);
            }
            Message::OpenPicked(path) => {
                if let Some(path) = path {
                    self.open_path(path);
                }
            }
            Message::Save => {
                self.close_popups();
                match self.doc.path().cloned() {
                    Some(path) => {
                        if self.save_to(path) {
                            return self.continue_after_save();
                        }
                    }
                    None => return self.pick_save_path(),
                }
            }
            Message::SaveAs => {
                self.close_popups();
                return self.pick_save_path();
            }
            Message::SavePicked(path) => match path {
                Some(mut path) => {
                    if path.extension().is_none() {
                        path.set_extension(project::FILE_EXTENSION);
                    }
                    if self.save_to(path) {
                        return self.continue_after_save();
                    }
                }
                None => self.ui.after_save = None,
            },
            Message::ExportAll => {
                self.close_popups();
                if self.graph().nodes_of(NodeKind::OUTPUT).next().is_none() {
                    self.set_status("The graph has no Output nodes to export", true);
                } else {
                    return Task::perform(
                        blocking(|| rfd::FileDialog::new().set_title("Export outputs to folder").pick_folder()),
                        |p| Message::ExportFolderPicked(p.flatten()),
                    );
                }
            }
            Message::ExportFolderPicked(Some(dir)) => {
                let jobs = self.export_jobs(&dir);
                self.res.exports_pending += jobs.len();
                self.set_status(format!("Exporting {} output(s) to {}…", jobs.len(), dir.display()), false);
                self.send(Request::Export(jobs));
            }
            Message::ExportFolderPicked(None) => {}
            Message::ExportNode(id) => {
                let Some(node) = self.graph().node(id) else { return Task::none() };
                let depth = node.enum_value("bit_depth");
                let name = format!(
                    "{}.{}",
                    brewed_texture_engine::export::sanitize_file_name(node.text("name")),
                    brewed_texture_engine::export::extension(depth)
                );
                let ext = brewed_texture_engine::export::extension(depth);
                return Task::perform(
                    blocking(move || {
                        rfd::FileDialog::new()
                            .set_title("Export output")
                            .add_filter(ext.to_uppercase(), &[ext])
                            .set_file_name(name)
                            .save_file()
                    }),
                    move |p| Message::ExportNodePicked(id, p.flatten()),
                );
            }
            Message::ExportNodePicked(id, Some(path)) => {
                if let Some(node) = self.graph().node(id) {
                    let job = ExportJob {
                        node: id,
                        path,
                        format: self.eval.params(id).map_or(node.enum_value("format"), |p| p.u("format")),
                        bit_depth: node.enum_value("bit_depth"),
                    };
                    self.res.exports_pending += 1;
                    self.send(Request::Export(vec![job]));
                }
            }
            Message::ExportNodePicked(_, None) => {}
            Message::Dialog(d) => {
                self.close_popups();
                self.ui.dialog = d;
            }
            Message::Graph(msg) => return self.graph_message(msg),
            Message::SelectNode(id) => {
                self.close_popups();
                self.ui.selection = [id].into_iter().collect();
                self.ui.selected_link = None;
                self.doc.seal();
                self.reveal(id);
                self.request_stats();
            }
            Message::Copy => {
                self.close_popups();
                return self.copy_selection();
            }
            Message::Cut => {
                self.close_popups();
                let task = self.copy_selection();
                self.delete_selection();
                return task;
            }
            Message::Paste => {
                self.close_popups();
                return iced::clipboard::read().map(Message::ClipboardText);
            }
            Message::ClipboardText(text) => {
                let clip = text.as_deref().and_then(Clipboard::from_text).or_else(|| self.ui.clipboard.clone());
                match clip {
                    Some(clip) => self.paste_clipboard(clip),
                    None => self.set_status("The clipboard holds no nodes", false),
                }
            }
            Message::Duplicate => {
                self.close_popups();
                let ids = self.selected_ids();
                if !ids.is_empty() {
                    self.apply(Edit::Duplicate { ids, offset: [30.0, 30.0] });
                    self.doc.seal();
                }
            }
            Message::Delete => {
                self.close_popups();
                self.delete_selection();
            }
            Message::AddComment => self.add_comment(),
            Message::SelectAll => {
                self.close_popups();
                self.ui.selection = self.graph().nodes.iter().map(|n| n.id).collect();
            }
            Message::Frame => {
                self.close_popups();
                let only = !self.ui.selection.is_empty();
                self.frame(only);
            }
            Message::FrameAll => {
                self.close_popups();
                self.frame(false);
            }
            Message::ToggleBypass(id) => {
                self.close_popups();
                let bypass = self.graph().node(id).is_some_and(|n| !n.bypass);
                self.apply(Edit::SetBypass { node: id, bypass });
                self.doc.seal();
            }
            Message::PaneResized(e) => self.ui.panes.resize(e.split, e.ratio),
            Message::ResetLayout => self.ui.panes = default_panes(),
            Message::LockSquare => {
                self.ui.lock_square = !self.ui.lock_square;
                let [w, h] = self.graph().resolution;
                if self.ui.lock_square && w != h {
                    self.apply(Edit::SetResolution([w, w]));
                    self.doc.seal();
                }
            }
            Message::FromMenu(inner) => {
                self.close_popups();
                return self.handle(*inner);
            }
            Message::Menu(m) => {
                self.ui.context = None;
                self.ui.add_menu = None;
                self.ui.menu = if self.ui.menu == m { None } else { m };
            }
            Message::MenuHover(id) => {
                if self.ui.menu.is_some() {
                    self.ui.menu = Some(id);
                }
            }
            Message::Context(c) => {
                self.ui.menu = None;
                self.ui.add_menu = None;
                self.ui.choice_menu = None;
                self.ui.context = c;
            }
            Message::ChoiceMenu(Some((node, key))) => self.open_choice_menu(node, key),
            Message::ChoiceMenu(None) => self.ui.choice_menu = None,
            Message::PickChoice(node, key, option) => {
                self.ui.choice_menu = None;
                // A pick is a step of its own in the history.
                self.doc.seal();
                self.apply(Edit::SetParam { node, key: key.to_string(), value: ParamValue::Enum(option) });
                self.doc.seal();
            }
            Message::AddMenu(msg) => return self.add_menu_message(msg),
            Message::LibrarySearch(q) => self.ui.library_search = q,
            Message::LibraryToggle(c) => {
                if !self.ui.library_collapsed.remove(&c) {
                    self.ui.library_collapsed.insert(c);
                }
            }
            Message::PanelPress(payload) => {
                self.close_popups();
                self.ui.press = Some((payload, self.ui.cursor));
            }
            Message::PanelAdd(payload) => {
                self.ui.press = None;
                self.ui.drag = None;
                let pos = self.ui.graph_view.center();
                self.add_payload(payload, [pos[0] - 80.0, pos[1] - 40.0]);
            }
            Message::Expression(node, action) => self.expression_action(node, action),
            Message::ColorPicker(target) => {
                // The picker floats where it was opened, so nothing in the panels moves.
                self.ui.color_picker_at = self.ui.cursor + Vector::new(-120.0, 18.0);
                self.ui.color_picker = if self.ui.color_picker == target { None } else { target };
                self.doc.seal();
            }
            Message::GradientStop(node, key, index) => {
                self.ui.gradient_stop.insert((node, key), index);
            }
            Message::SetGradient(node, key, gradient, select) => {
                self.apply(brewed_texture_core::edit::gradient_edit(node, &key, gradient));
                if let Some(i) = select {
                    self.ui.gradient_stop.insert((node, key), i);
                }
            }
            Message::BindingText(node, text) => self.ui.binding = Some((node, text)),
            Message::CommitBinding => {
                if let Some((node, name)) = self.ui.binding.take() {
                    self.apply(Edit::BindVariable { node, name });
                    self.doc.seal();
                }
            }
            Message::ShowFunctions => self.ui.show_functions = !self.ui.show_functions,
            Message::PlotVar(i) => self.ui.plot_var = i,
            Message::PlotRange(lo, hi) => {
                if lo.is_finite() && hi.is_finite() {
                    self.ui.plot_range = (lo.min(hi - 1e-3), hi.max(lo + 1e-3));
                }
            }
            Message::BrowseFile(node, key) => {
                let extensions = self.graph().node(node).map_or(&[][..], |n| file_extensions(n.kind, &key));
                return Task::perform(
                    blocking(move || {
                        let mut dialog = rfd::FileDialog::new().set_title("Choose a file");
                        if !extensions.is_empty() {
                            dialog = dialog.add_filter("Supported files", extensions);
                        }
                        dialog.pick_file()
                    }),
                    move |p| Message::FilePicked(node, key.clone(), p.flatten()),
                );
            }
            Message::FilePicked(node, key, Some(path)) => {
                self.apply(Edit::SetParam { node, key, value: ParamValue::Text(path.display().to_string()) });
                self.doc.seal();
            }
            Message::FilePicked(_, _, None) => {}
            Message::ReloadBitmap(node) => {
                if let Some(n) = self.graph().node(node) {
                    let path = n.text("path").to_string();
                    self.send(Request::ReloadBitmap(path));
                }
            }
            Message::AutoLevels(node) => self.auto_levels(node),
            Message::ToggleVariable(name) => {
                if !self.ui.expanded_vars.remove(&name) {
                    self.ui.expanded_vars.insert(name);
                }
            }
            Message::RenameVariable(from, text) => self.ui.renaming_var = Some((from, text)),
            Message::CommitRename => {
                if let Some((from, to)) = self.ui.renaming_var.take()
                    && from != to
                {
                    let expanded = self.ui.expanded_vars.remove(&from);
                    if self.apply(Edit::RenameVariable { from: from.clone(), to: to.clone() }) {
                        self.doc.seal();
                        let new = brewed_texture_core::graph::sanitize_identifier(&to);
                        if expanded {
                            self.ui.expanded_vars.insert(new);
                        }
                    } else if expanded {
                        self.ui.expanded_vars.insert(from);
                    }
                }
            }
            Message::Preview(msg) => return self.preview_message(msg),
            Message::Pick(msg) => return self.pick_message(msg),
            Message::Host(host) => self.host = host,
            Message::Screenshot(shot) => return self.save_screenshot(shot),
        }
        Task::none()
    }

    fn continue_after_save(&mut self) -> Task<Message> {
        match self.ui.after_save.take() {
            Some(action) => self.proceed(action),
            None => Task::none(),
        }
    }

    fn add_payload(&mut self, payload: DragPayload, pos: [f32; 2]) {
        let edit = match payload {
            DragPayload::Node(kind) => Edit::AddNode { kind, pos },
            DragPayload::Variable(name) => Edit::AddVariableNode { name, pos },
        };
        self.apply(edit);
        self.doc.seal();
    }

    fn auto_levels(&mut self, node: NodeId) {
        let Some(source) = self.graph().link_into(node, "i:input").map(|l| l.from) else { return };
        let Some((_, stats)) = self.res.stats.get(&source).cloned() else { return };
        let mn = stats.min[..3].iter().copied().fold(f32::INFINITY, f32::min).clamp(0.0, 1.0);
        let mx = stats.max[..3].iter().copied().fold(f32::NEG_INFINITY, f32::max).clamp(0.0, 1.0);
        if mx > mn {
            // One undo step; exposed levels come from their sockets and are left alone.
            let levels = self.graph().node(node);
            let edits: Vec<Edit> = [("in_low", mn), ("in_high", mx), ("gamma", 1.0)]
                .into_iter()
                .filter(|(k, _)| levels.is_some_and(|n| !n.is_exposed(k)))
                .map(|(k, v)| Edit::SetParam { node, key: k.into(), value: ParamValue::Float(v) })
                .collect();
            self.apply_all(edits);
            self.doc.seal();
        }
    }

    fn expression_action(&mut self, node: NodeId, action: text_editor::Action) {
        let Some(key) = self.graph().node(node).and_then(|n| n.def().expression_param()).map(|p| p.key) else {
            return;
        };
        let current = self.graph().node(node).map(|n| n.text(key).to_string()).unwrap_or_default();
        let content = match &mut self.ui.expression {
            Some((id, content)) if *id == node => content,
            _ => {
                self.ui.expression = Some((node, text_editor::Content::with_text(&current)));
                &mut self.ui.expression.as_mut().expect("just set").1
            }
        };
        let is_edit = action.is_edit();
        content.perform(action);
        if is_edit {
            // The editor adds a trailing newline to multi-line content; expressions are one value.
            let text = content.text().trim_end_matches(['\n', '\r']).to_string();
            if text != current {
                self.apply(Edit::SetParam { node, key: key.into(), value: ParamValue::Text(text) });
            }
        }
    }

    // ------------------------------------------------------------------ engine events

    fn engine_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Evaluated { revision, reports } => {
                self.res.reports = reports;
                self.res.evaluated_revision = revision;
                self.request_stats();
                // An open menu of pictures shows the node as it is now.
                if let Some(menu) = &self.ui.choice_menu {
                    let drawn = self.res.choices.get(&(menu.node, menu.key.to_string())).map(|c| c.version);
                    if drawn != self.res.reports.get(&menu.node).map(|r| r.version) {
                        self.send(Request::Choices { node: menu.node, key: menu.key.to_string() });
                    }
                }
            }
            Event::Finished { revision, report, millis } => {
                self.res.finished_revision = self.res.finished_revision.max(revision);
                if self.res.finished_revision >= self.eval_revision {
                    self.res.busy_since = None;
                    self.res.slow = false;
                } else {
                    // The engine is at the next change already.
                    self.res.busy_since = Some(Instant::now());
                }
                if report.image_nodes == 0 {
                    self.res.update = None;
                } else if report.recomputed > 0 {
                    self.res.update = Some(Update { recomputed: report.recomputed, image_nodes: report.image_nodes, millis });
                }
            }
            Event::Choices(c) => {
                let pictures = c.pictures.iter().map(|rgba| image::Handle::from_rgba(c.size, c.size, rgba.clone())).collect();
                self.res.choices.insert((c.node, c.key), ChoicePictures { version: c.version, pictures });
            }
            Event::Thumbnails(thumbs) => {
                for t in thumbs {
                    self.res.thumbs.insert(t.node, image::Handle::from_rgba(t.size, t.size, t.rgba));
                }
            }
            Event::Preview(p) => self.res.preview = p,
            Event::Materials(m) => self.res.materials = m,
            Event::Stats { node, version, stats } => {
                self.res.stats.insert(node, (version, stats));
            }
            Event::Probe { node, pixel, rgba } => {
                if let Some(rgba) = rgba {
                    self.res.probe = Some((node, pixel, rgba));
                }
            }
            Event::Exported(result) => {
                self.res.exports_pending = self.res.exports_pending.saturating_sub(1);
                match result {
                    Ok(path) => self.set_status(format!("Exported {}", path.display()), false),
                    Err(e) => self.set_status(e, true),
                }
            }
            Event::GpuErrors(errors) => {
                if let Some(e) = errors.last() {
                    self.set_status(format!("GPU error: {e}"), true);
                }
            }
        }
        Task::none()
    }

    /// Keep histograms fresh for Levels nodes that are selected.
    fn request_stats(&mut self) {
        let graph = self.doc.graph();
        let mut wanted = Vec::new();
        for id in &self.ui.selection {
            if let Some(n) = graph.node(*id)
                && n.kind == NodeKind::LEVELS
                && let Some(link) = graph.link_into(*id, "i:input")
            {
                wanted.push(link.from);
            }
        }
        for source in wanted {
            let Some(version) = self.res.reports.get(&source).map(|r| r.version) else { continue };
            let have = self.res.stats.get(&source).map(|s| s.0);
            let asked = self.res.stats_requested.get(&source).copied();
            if have != Some(version) && asked != Some(version) {
                self.res.stats_requested.insert(source, version);
                self.send(Request::Stats(source));
            }
        }
    }

    // ------------------------------------------------------------------ runtime events

    fn runtime_event(&mut self, event: iced::Event, status: iced::event::Status, id: window::Id) -> Task<Message> {
        use iced::Event as E;
        if Some(id) != self.window {
            // A picker window: Esc or closing it (Alt+F4) cancels the pick; its own canvas
            // handles the mouse.
            if let E::Window(window::Event::Closed) = event {
                self.picker_window_closed(id);
                return Task::none();
            }
            let cancel = matches!(
                &event,
                E::Window(window::Event::CloseRequested)
                    | E::Keyboard(keyboard::Event::KeyPressed { key: keyboard::Key::Named(keyboard::key::Named::Escape), .. })
            );
            return if cancel { self.pick_message(PickMessage::Cancel) } else { Task::none() };
        }
        match event {
            E::Window(window::Event::CloseRequested) => {
                // A pick in progress ends first.
                let cancel = self.pick_message(PickMessage::Cancel);
                return Task::batch([cancel, self.guarded(PendingAction::Quit)]);
            }
            E::Window(window::Event::Opened { size, .. } | window::Event::Resized(size)) => {
                self.ui.window_size = size;
            }
            E::Window(window::Event::FileDropped(path)) => return self.file_dropped(path),
            E::Window(_) => {}
            E::Mouse(mouse::Event::CursorMoved { position }) => {
                self.ui.cursor = position;
                if let Some((payload, start)) = &self.ui.press
                    && position.distance(*start) > 4.0
                {
                    self.ui.drag = Some(payload.clone());
                    self.ui.press = None;
                }
            }
            E::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                // Drops on the graph are handled by the canvas (it knows its bounds).
                self.ui.press = None;
                self.ui.drag = None;
            }
            E::Mouse(_) => {}
            E::Keyboard(keyboard::Event::ModifiersChanged(m)) => self.ui.modifiers = m,
            E::Keyboard(keyboard::Event::KeyPressed { key, physical_key, modifiers, .. }) => {
                return self.shortcut(key, physical_key, modifiers, status);
            }
            _ => {}
        }
        Task::none()
    }

    /// A file dropped on the window: a graph is opened, an image becomes a Bitmap node.
    fn file_dropped(&mut self, path: PathBuf) -> Task<Message> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
        if ext == project::FILE_EXTENSION {
            return self.guarded(PendingAction::OpenPath(path));
        }
        if file_extensions(NodeKind::BITMAP, "path").contains(&ext.as_str()) {
            self.close_popups();
            // Several files dropped at once arrive one by one: stack their nodes.
            let burst = self.ui.drop_burst.filter(|(at, _)| at.elapsed() < Duration::from_millis(800)).map_or(0, |(_, n)| n);
            self.ui.drop_burst = Some((Instant::now(), burst + 1));
            let [x, y] = self.graph_pos_for_paste();
            let path = ParamValue::Text(path.display().to_string());
            let edit = Edit::AddNodeWith { kind: NodeKind::BITMAP, pos: [x, y + burst as f32 * 200.0], params: vec![("path".into(), path)] };
            if self.apply(edit) {
                self.doc.seal();
                let created = self.ui.selection.first().copied();
                self.set_preview(created);
            }
            return Task::none();
        }
        self.set_status(format!("Cannot use '{}': drop a graph or an image", path.display()), true);
        Task::none()
    }

    /// `physical` makes the shortcuts the same on every keyboard layout: a key that types
    /// no Latin letter (Cyrillic, Greek…) counts as the Latin one in its place.
    fn shortcut(&mut self, key: keyboard::Key, physical: keyboard::key::Physical, m: keyboard::Modifiers, status: iced::event::Status) -> Task<Message> {
        use keyboard::Key;
        use keyboard::key::Named;
        let ctrl = m.command();
        let latin = key.to_latin(physical).map(|c| c.to_ascii_lowercase());
        let char = |c: char| latin == Some(c);
        // Escape closes whatever is open, even when a text field has focus.
        if key == Key::Named(Named::Escape) {
            if self.ui.screen_pick.is_some() {
                return self.pick_message(PickMessage::Cancel);
            }
            if self.ui.add_menu.is_some() || self.ui.menu.is_some() || self.ui.context.is_some() || self.ui.choice_menu.is_some() {
                self.close_popups();
            } else if self.ui.dialog.is_some() {
                self.ui.dialog = None;
            } else if self.ui.renaming_var.is_some() || self.ui.binding.is_some() {
                // Typing a variable name: forget the text, the name stays as it was.
                self.ui.renaming_var = None;
                self.ui.binding = None;
            } else if self.ui.color_picker.is_some() {
                self.ui.color_picker = None;
            }
            return Task::none();
        }
        // Nothing else while picking from the screen (the keyboard may still reach the
        // window).
        if self.ui.screen_pick.is_some() {
            return Task::none();
        }
        if let Some(menu) = &self.ui.choice_menu {
            if key == Key::Named(Named::Enter) {
                let (node, key, option) = (menu.node, menu.key, menu.cursor as u32);
                return self.update_nested(Message::PickChoice(node, key, option));
            }
            if self.move_in_choice_menu(&key) {
                return Task::none();
            }
        }
        if self.ui.add_menu.is_some() {
            return match key {
                Key::Named(Named::ArrowDown) => self.add_menu_message(AddMenuMessage::Move(1)),
                Key::Named(Named::ArrowUp) => self.add_menu_message(AddMenuMessage::Move(-1)),
                Key::Named(Named::PageDown) => self.add_menu_message(AddMenuMessage::Move(8)),
                Key::Named(Named::PageUp) => self.add_menu_message(AddMenuMessage::Move(-8)),
                _ => Task::none(),
            };
        }
        // Global shortcuts that also work while typing.
        if ctrl && char('s') {
            return self.update_nested(if m.shift() { Message::SaveAs } else { Message::Save });
        }
        if ctrl && char('o') {
            return self.update_nested(Message::Open);
        }
        if ctrl && char('e') {
            return self.update_nested(Message::ExportAll);
        }
        if status == iced::event::Status::Captured || self.ui.dialog.is_some() {
            return Task::none(); // a text field or widget used the key
        }
        let over_graph = self.ui.graph_view.contains(self.ui.cursor);
        let message = match key.as_ref() {
            _ if ctrl && (char('y') || (m.shift() && char('z'))) => Message::Redo,
            _ if ctrl && char('z') => Message::Undo,
            _ if ctrl && char('n') => Message::New,
            _ if ctrl && char('c') => Message::Copy,
            _ if ctrl && char('x') => Message::Cut,
            _ if ctrl && char('v') => Message::Paste,
            _ if ctrl && char('d') => Message::Duplicate,
            _ if ctrl && char('a') => Message::SelectAll,
            _ if ctrl && char('b') => match self.ui.selection.iter().next() {
                Some(id) => Message::ToggleBypass(*id),
                None => return Task::none(),
            },
            Key::Named(Named::Delete | Named::Backspace) => Message::Delete,
            Key::Named(Named::F1) => Message::Dialog(Some(Dialog::Shortcuts)),
            _ if !ctrl && char('f') => Message::Frame,
            _ if !ctrl && char('c') && over_graph => Message::AddComment,
            Key::Named(Named::Space) if over_graph => {
                let at = self.ui.cursor;
                let pos = self.ui.graph_view.to_graph(at);
                return self.open_add_menu(at, pos, None);
            }
            // Over the 2D view, Space is the tiling: it is what one keeps looking at.
            Key::Named(Named::Space) if self.ui.preview.bounds.contains(self.ui.cursor) => {
                Message::Preview(PreviewMessage::ToggleTiling)
            }
            _ => return Task::none(),
        };
        self.update_nested(message)
    }

    fn update_nested(&mut self, message: Message) -> Task<Message> {
        self.handle(message)
    }

    // ------------------------------------------------------------------ graph canvas

    fn graph_message(&mut self, msg: GraphMessage) -> Task<Message> {
        match msg {
            GraphMessage::Select { node, mode } => {
                self.close_popups();
                match mode {
                    SelectMode::Replace => self.ui.selection = [node].into_iter().collect(),
                    SelectMode::Add => {
                        self.ui.selection.insert(node);
                    }
                    SelectMode::Toggle => {
                        if !self.ui.selection.remove(&node) {
                            self.ui.selection.insert(node);
                        }
                    }
                }
                self.ui.selected_link = None;
                self.ui.selected_comment = None;
                self.doc.seal();
                self.request_stats();
            }
            GraphMessage::SelectBox { ids, additive } => {
                self.close_popups();
                if !additive {
                    self.ui.selection.clear();
                }
                self.ui.selection.extend(ids);
                self.ui.selected_link = None;
                self.ui.selected_comment = None;
            }
            GraphMessage::ClearSelection => {
                self.close_popups();
                self.ui.selection.clear();
                self.ui.selected_link = None;
                self.ui.selected_comment = None;
            }
            GraphMessage::SelectLink(link) => {
                self.close_popups();
                self.ui.selection.clear();
                self.ui.selected_comment = None;
                self.ui.selected_link = Some(link);
            }
            GraphMessage::SelectComment(id) => {
                self.close_popups();
                self.ui.selection.clear();
                self.ui.selected_link = None;
                self.ui.selected_comment = Some(id);
            }
            GraphMessage::EditComment(id) => {
                self.close_popups();
                self.ui.selection.clear();
                self.ui.selected_link = None;
                self.ui.selected_comment = Some(id);
                return operation::focus(view::properties::comment_text_id());
            }
            GraphMessage::MoveComment { id, delta, nodes } => {
                self.apply(Edit::MoveComment { id, delta, nodes });
                self.doc.seal();
            }
            GraphMessage::ResizeComment { id, size } => {
                if let Some(pos) = self.graph().comment(id).map(|c| c.pos) {
                    self.apply(Edit::ResizeComment { id, pos, size });
                    self.doc.seal();
                }
            }
            GraphMessage::Move { ids, delta } => {
                self.apply(Edit::MoveNodes { ids, delta });
                self.doc.seal();
            }
            GraphMessage::Connect { from, to, replacing } => {
                let connect = Edit::Connect { a: from, b: to };
                // Re-routing is one step: the old link goes, the new one comes.
                let edit = match replacing {
                    Some(old) => Edit::Batch(vec![Edit::RemoveLink(old), connect]),
                    None => connect,
                };
                self.apply(edit);
                self.doc.seal();
            }
            GraphMessage::LinkDropped { from, at, graph_pos, detached } => {
                if let Some(old) = detached {
                    self.apply(Edit::RemoveLink(old));
                }
                self.doc.seal();
                return self.open_add_menu(at, [graph_pos[0] - 20.0, graph_pos[1] - 12.0], Some(from));
            }
            GraphMessage::OpenAddMenu { at, graph_pos } => {
                return self.open_add_menu(at, [graph_pos[0] - 80.0, graph_pos[1] - 12.0], None);
            }
            GraphMessage::Preview(node) => {
                self.close_popups();
                self.set_preview(Some(node));
            }
            GraphMessage::Context { at, target } => {
                if let ContextTarget::Node(id) = &target
                    && !self.ui.selection.contains(id)
                {
                    self.ui.selection = [*id].into_iter().collect();
                }
                self.ui.menu = None;
                self.ui.add_menu = None;
                self.ui.context = Some(ContextMenu { at, target });
            }
            GraphMessage::PanBy(delta) => self.ui.graph_view.pan += delta,
            GraphMessage::ZoomAt { at, factor } => {
                // The point of the graph under the cursor stays under it.
                let view = &mut self.ui.graph_view;
                let zoom = (view.zoom * factor).clamp(0.12, 3.0);
                let g = [(at.x - view.pan.x) / view.zoom, (at.y - view.pan.y) / view.zoom];
                view.pan = Vector::new(at.x - g[0] * zoom, at.y - g[1] * zoom);
                view.zoom = zoom;
            }
            GraphMessage::Bounds(bounds) => {
                self.ui.graph_view.bounds = bounds;
                if self.ui.frame_request {
                    self.frame(false);
                }
            }
            GraphMessage::Drop { payload, graph_pos } => {
                self.ui.drag = None;
                self.ui.press = None;
                self.add_payload(payload, [graph_pos[0] - 80.0, graph_pos[1] - 12.0]);
            }
            GraphMessage::Error(e) => self.set_status(e, true),
        }
        Task::none()
    }

    fn add_menu_message(&mut self, msg: AddMenuMessage) -> Task<Message> {
        let count = view::add_menu::items(self).len();
        let Some(menu) = &mut self.ui.add_menu else { return Task::none() };
        match msg {
            AddMenuMessage::Query(q) => {
                menu.query = q;
                menu.selected = 0;
                menu.scroll = 0.0;
                return operation::scroll_to(view::add_menu::list_id(), operation::AbsoluteOffset { x: None, y: Some(0.0) });
            }
            AddMenuMessage::Move(delta) => {
                let last = count.saturating_sub(1) as i64;
                menu.selected = (menu.selected.min(last as usize) as i64 + delta as i64).clamp(0, last) as usize;
                if let Some(y) = view::add_menu::scroll_to_row(menu.selected, menu.scroll, count) {
                    menu.scroll = y;
                    return operation::scroll_to(view::add_menu::list_id(), operation::AbsoluteOffset { x: None, y: Some(y) });
                }
            }
            AddMenuMessage::Scrolled(y) => menu.scroll = y,
            AddMenuMessage::Close => self.ui.add_menu = None,
            AddMenuMessage::Choose(kind) => self.add_from_menu(kind),
            AddMenuMessage::Submit => {
                let items = view::add_menu::items(self);
                if let Some(kind) = items.get(menu_index(self.ui.add_menu.as_ref(), items.len())).copied() {
                    self.add_from_menu(kind);
                }
            }
        }
        Task::none()
    }

    fn add_from_menu(&mut self, kind: NodeKind) {
        let Some(menu) = self.ui.add_menu.take() else { return };
        let edit = match menu.pending {
            Some(socket) => Edit::AddNodeConnected { kind, pos: menu.graph_pos, socket },
            None => Edit::AddNode { kind, pos: menu.graph_pos },
        };
        self.apply(edit);
        self.doc.seal();
    }

    // ------------------------------------------------------------------ 2D / 3D view

    fn preview_message(&mut self, msg: PreviewMessage) -> Task<Message> {
        let p = &mut self.ui.preview;
        match msg {
            PreviewMessage::Channel(c) => p.channel = c,
            PreviewMessage::ToggleTiling => p.show_tiling = !p.show_tiling,
            PreviewMessage::Fit => p.fit = true,
            PreviewMessage::OneToOne => {
                p.zoom = 1.0;
                p.pan = Vector::ZERO;
                p.fit = false;
            }
            PreviewMessage::View { zoom, pan, fit } => {
                p.zoom = zoom;
                p.pan = pan;
                p.fit = fit;
            }
            PreviewMessage::PanBy(d) => {
                p.pan += d;
                p.fit = false;
            }
            PreviewMessage::ZoomAt { at, factor } => {
                // The point of the image under the cursor stays under it.
                let zoom = (p.zoom * factor).clamp(0.02, 64.0);
                let half = Vector::new(p.bounds.width / 2.0, p.bounds.height / 2.0);
                let rel = (at - (Point::ORIGIN + half + p.pan)) * (1.0 / p.zoom);
                p.pan = (at - Point::ORIGIN) - half - rel * zoom;
                p.zoom = zoom;
                p.fit = false;
            }
            PreviewMessage::Move3d(m) => p.view3d = p.view3d.moved(m),
            PreviewMessage::Bounds(b) => p.bounds = b,
            PreviewMessage::Probe(px) => {
                p.probe = px;
                if let (Some(px), Some(node)) = (px, p.node) {
                    self.send(Request::Probe { node, pixel: px });
                }
            }
            PreviewMessage::Camera(c) => p.view3d = c,
        }
        Task::none()
    }

    // ------------------------------------------------------------------ eyedropper

    fn pick_message(&mut self, msg: PickMessage) -> Task<Message> {
        match msg {
            PickMessage::Start(target) => {
                if self.ui.screen_pick.is_some() {
                    return Task::none();
                }
                self.close_popups();
                let mode = target.mode();
                self.ui.screen_pick = Some(ScreenPick::new(target));
                Task::perform(eyedropper::pick(mode, self.host), |outcome| Message::Pick(PickMessage::Picked(outcome)))
            }
            PickMessage::Picked(outcome) => {
                let Some(pick) = self.ui.screen_pick.take_if(|p| p.live) else { return Task::none() };
                if outcome == eyedropper::Outcome::Unsupported {
                    // Not possible live here: pick on a capture of the screen.
                    self.ui.screen_pick = Some(ScreenPick { live: false, ..pick });
                    return Task::perform(blocking(picker::capture), |shots| {
                        Message::Pick(PickMessage::Captured(shots.unwrap_or_else(|| Err("The screen capture failed".into()))))
                    });
                }
                self.picked(&pick.target, outcome);
                Task::none()
            }
            PickMessage::Captured(Err(e)) => {
                self.ui.screen_pick = None;
                self.set_status(e, true);
                Task::none()
            }
            PickMessage::Captured(Ok(shots)) => {
                // Cancelled (Esc) while capturing: nothing to show.
                let Some(pick) = self.ui.screen_pick.as_mut().filter(|p| !p.live) else { return Task::none() };
                let mut tasks = Vec::new();
                for shot in &shots {
                    let (id, open) = window::open(picker::overlay_settings(shot, crate::brand::window_icon()));
                    pick.windows.push(id);
                    tasks.push(open.then(|id| Task::batch([window::run(id, eyedropper::prepare_overlay).discard(), window::gain_focus(id)])));
                }
                pick.shots = shots;
                Task::batch(tasks)
            }
            PickMessage::Color { shot, at } => {
                let Some((target, shots)) = self.finish_capture_pick() else { return Task::none() };
                if let Some(shot) = shots.get(shot) {
                    let color = shot.pixel(at[0].floor() as i64, at[1].floor() as i64);
                    self.picked(&target, eyedropper::Outcome::Color(color));
                }
                self.close_picker_windows()
            }
            PickMessage::Path { shot, points } => {
                let Some((target, shots)) = self.finish_capture_pick() else { return Task::none() };
                let colors = picker::path_colors(&shots, shot, &points);
                if colors.len() >= 2 {
                    self.picked(&target, eyedropper::Outcome::Path(colors));
                }
                self.close_picker_windows()
            }
            PickMessage::Cancel => {
                let Some(pick) = &mut self.ui.screen_pick else { return Task::none() };
                if pick.live {
                    // The pick ends on its own thread; the app forgets it right away, so
                    // that nothing can leave the eyedropper stuck.
                    eyedropper::cancel();
                    self.ui.screen_pick = None;
                    return Task::none();
                }
                if pick.done {
                    return Task::none();
                }
                if pick.windows.is_empty() {
                    // Still capturing: forget the pick, the capture will find nothing to do.
                    self.ui.screen_pick = None;
                    return Task::none();
                }
                pick.done = true;
                self.close_picker_windows()
            }
        }
    }

    /// Put the result of a pick where the eyedropper was clicked.
    fn picked(&mut self, target: &PickTarget, outcome: eyedropper::Outcome) {
        use eyedropper::Outcome;
        match (outcome, target) {
            (Outcome::Color(color), PickTarget::Color(target)) => self.apply_picked_color(target, color),
            (Outcome::Path(colors), PickTarget::Gradient(node, key)) => {
                let gradient = eyedropper::gradient_from_path(&colors);
                let stops = gradient.stops.len();
                if self.apply(brewed_texture_core::edit::gradient_edit(*node, key, gradient)) {
                    self.doc.seal();
                    self.ui.gradient_stop.remove(&(*node, key.clone()));
                    self.set_status(format!("Picked a gradient with {stops} stops"), false);
                } else {
                    self.set_status("The gradient to set no longer exists", true);
                }
            }
            (Outcome::Failed(e), _) => self.set_status(e, true),
            _ => {}
        }
    }

    /// End a pick on a capture: its target and the captured monitors; `None` if it is over
    /// already. The windows keep showing the capture until they close.
    fn finish_capture_pick(&mut self) -> Option<(PickTarget, Vec<Shot>)> {
        let pick = self.ui.screen_pick.as_mut().filter(|p| !p.live && !p.done)?;
        pick.done = true;
        Some((pick.target.clone(), pick.shots.clone()))
    }

    fn close_picker_windows(&self) -> Task<Message> {
        let Some(pick) = &self.ui.screen_pick else { return Task::none() };
        let mut tasks: Vec<Task<Message>> = pick.windows.iter().map(|id| window::close(*id)).collect();
        if let Some(main) = self.window {
            tasks.push(window::gain_focus(main));
        }
        Task::batch(tasks)
    }

    /// A picker window has closed; the pick is gone with the last one.
    fn picker_window_closed(&mut self, id: window::Id) {
        let Some(pick) = &mut self.ui.screen_pick else { return };
        if pick.windows.contains(&id) && !pick.closed.contains(&id) {
            pick.closed.push(id);
        }
        if !pick.live && pick.closed.len() == pick.windows.len() {
            self.ui.screen_pick = None;
        }
    }

    /// Put a color picked from the screen where the eyedropper was clicked. A screen has
    /// no transparency: the target keeps its alpha.
    fn apply_picked_color(&mut self, target: &ColorTarget, rgb: [f32; 4]) {
        let graph = self.doc.graph();
        let with_alpha = |a: f32| [rgb[0], rgb[1], rgb[2], a];
        let edit = match target {
            ColorTarget::Param(node, key) => {
                let alpha = match graph.node(*node).and_then(|n| n.value(key)) {
                    Some(ParamValue::Color(c)) => c[3],
                    _ => 1.0,
                };
                Some(Edit::SetParam { node: *node, key: key.clone(), value: ParamValue::Color(with_alpha(alpha)) })
            }
            ColorTarget::Variable(name, field) => graph.variable(name).map(|var| {
                let current = if *field == VarField::Value { var.value } else { var.default };
                let alpha = if let Value::Color(c) = current { c[3] } else { 1.0 };
                let value = Value::Color(with_alpha(alpha));
                match field {
                    VarField::Value => Edit::SetVariable { name: name.clone(), value },
                    VarField::Default => Edit::SetVariableDefault { name: name.clone(), value },
                }
            }),
            ColorTarget::Comment(id) => graph.comment(*id).map(|c| Edit::SetCommentColor { id: *id, color: with_alpha(c.color[3]) }),
            ColorTarget::GradientStop(node, key) => {
                let gradient = graph.node(*node).and_then(|n| n.value(key)).and_then(ParamValue::as_gradient).cloned();
                let stop = self.ui.gradient_stop.get(&(*node, key.clone())).copied();
                match (gradient, stop) {
                    (Some(mut g), Some(i)) if i < g.stops.len() => {
                        g.stops[i].color = with_alpha(g.stops[i].color[3]);
                        Some(brewed_texture_core::edit::gradient_edit(*node, key, g))
                    }
                    _ => None,
                }
            }
        };
        match edit {
            Some(edit) => {
                if self.apply(edit) {
                    self.doc.seal();
                }
                self.set_status(format!("Picked {}", picker::hex(rgb)), false);
            }
            None => self.set_status("The color to set no longer exists", true),
        }
    }

    // ------------------------------------------------------------------ screenshots (testing)

    fn apply_scenario(&mut self, scenario: &str) -> Task<Message> {
        let graph = self.doc.graph().clone();
        let first = |kind: NodeKind| graph.nodes_of(kind).next().map(|n| n.id);
        match scenario {
            "file-menu" => self.ui.menu = Some(MenuId::File),
            "add-menu" => {
                self.ui.add_menu = Some(AddMenu {
                    at: Point::new(700.0, 300.0),
                    graph_pos: [0.0, 0.0],
                    query: "bl".into(),
                    selected: 0,
                    scroll: 0.0,
                    pending: None,
                });
            }
            "context-menu" => {
                if let Some(id) = first(NodeKind::LEVELS) {
                    self.ui.selection = [id].into_iter().collect();
                    self.ui.context = Some(ContextMenu { at: Point::new(640.0, 360.0), target: ContextTarget::Node(id) });
                }
            }
            "gradient" => {
                if let Some(id) = first(NodeKind::GRADIENT_MAP) {
                    self.ui.selection = [id].into_iter().collect();
                    self.ui.preview.node = Some(id);
                    self.ui.gradient_stop.insert((id, "gradient".into()), 1);
                    self.ui.color_picker = Some(ColorTarget::GradientStop(id, "gradient".into()));
                }
            }
            "math" => {
                if let Some(id) = first(NodeKind::MATH) {
                    self.ui.selection = [id].into_iter().collect();
                }
            }
            "levels" => {
                if let Some(id) = first(NodeKind::LEVELS) {
                    self.ui.selection = [id].into_iter().collect();
                    self.ui.preview.node = Some(id);
                }
            }
            "inputs" => {
                self.ui.expanded_vars.insert("mortar".into());
                self.ui.expanded_vars.insert("tint".into());
                // The picker floats where it was opened from: the swatch of the tint value.
                self.ui.cursor = Point::new(1840.0, 330.0);
                self.ui.color_picker_at = self.ui.cursor + Vector::new(-120.0, 18.0);
                self.ui.color_picker = Some(ColorTarget::Variable("tint".into(), VarField::Value));
            }
            "transform" => {
                // A Transform 2D of the noise, rotated so its gizmo shows.
                if let Some(noise) = first(NodeKind::NOISE)
                    && let Ok(done) = self.doc.apply(Edit::AddNode { kind: NodeKind::TRANSFORM_2D, pos: [160.0, 520.0] })
                    && let Some(&id) = done.created.first()
                {
                    let _ = self.doc.apply(Edit::Connect {
                        a: SocketRef { node: noise, key: "o:output".into(), output: true },
                        b: SocketRef { node: id, key: "i:input".into(), output: false },
                    });
                    let _ = self.doc.apply(Edit::SetParam { node: id, key: "rotation".into(), value: ParamValue::Float(30.0) });
                    self.ui.selection = [id].into_iter().collect();
                    self.ui.preview.node = Some(id);
                }
            }
            "tiling" => {
                // The wall repeated: no seams.
                if let Some(id) = first(NodeKind::OUTPUT) {
                    self.ui.selection = [id].into_iter().collect();
                    self.ui.preview.node = Some(id);
                }
                self.ui.preview.show_tiling = true;
            }
            "choices" => {
                if let Some(id) = first(NodeKind::NOISE) {
                    self.ui.selection = [id].into_iter().collect();
                    self.ui.preview.node = Some(id);
                    self.open_choice_menu(id, "type");
                }
            }
            "dialog" => {
                let _ = self.doc.apply(Edit::SetResolution([1024, 1024]));
                self.ui.dialog = Some(Dialog::Unsaved(PendingAction::Quit));
            }
            "tile" => {
                if let Some(id) = first(NodeKind::TILE_GENERATOR) {
                    self.ui.selection = [id].into_iter().collect();
                    self.ui.preview.node = Some(id);
                }
            }
            other => log::warn!("unknown scenario '{other}'"),
        }
        Task::none()
    }

    fn maybe_screenshot(&mut self) -> Task<Message> {
        if self.opts.screenshot.is_none() || self.screenshot_requested {
            return Task::none();
        }
        let settled = self.res.evaluated_revision == self.doc.revision()
            && self.started.elapsed() > Duration::from_millis(2500)
            && (self.ui.preview.node.is_none() || self.res.preview.is_some())
            // The model wears the maps in every screenshot: they take longest to arrive.
            && self.res.materials.len() >= self.material_nodes().len();
        if !settled || self.started.elapsed() < Duration::from_secs(3) {
            if self.started.elapsed() > Duration::from_secs(30) {
                log::error!("screenshot: gave up waiting for the engine");
                return iced::exit();
            }
            return Task::none();
        }
        let Some(id) = self.window else { return Task::none() };
        self.screenshot_requested = true;
        window::screenshot(id).map(Message::Screenshot)
    }

    fn save_screenshot(&mut self, shot: window::Screenshot) -> Task<Message> {
        if let Some(path) = &self.opts.screenshot {
            let (w, h) = (shot.size.width, shot.size.height);
            match ::image::RgbaImage::from_raw(w, h, shot.rgba.to_vec()).map(|img| img.save(path)) {
                Some(Ok(())) => println!("screenshot saved to {}", path.display()),
                other => eprintln!("screenshot failed: {other:?}"),
            }
        }
        iced::exit()
    }

    pub fn view(&self, window: window::Id) -> iced::Element<'_, Message> {
        if Some(window) == self.window { view::root(self) } else { view::picker::view(self, window) }
    }
}

/// File extensions a node's File parameter accepts.
pub fn file_extensions(kind: NodeKind, key: &str) -> &'static [&'static str] {
    match kind.def().param(key).map(|pd| pd.kind) {
        Some(brewed_texture_core::nodes::ParamKind::File { extensions }) => extensions,
        _ => &[],
    }
}

/// Clamp the add menu's selection to the number of items.
pub fn menu_index(menu: Option<&AddMenu>, len: usize) -> usize {
    menu.map_or(0, |m| m.selected.min(len.saturating_sub(1)))
}

/// Run blocking work (file dialogs, screen capture) on a thread without blocking the UI.
/// `None` if the work panicked (the app carries on).
pub fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> impl Future<Output = Option<T>> + Send {
    let (tx, rx) = futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    async move { rx.await.ok() }
}
