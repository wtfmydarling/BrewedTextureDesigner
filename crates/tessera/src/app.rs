//! Application state and the update loop.
//!
//! The UI never changes the graph itself: every user action becomes a
//! [`tessera_core::Edit`] applied to the [`Document`], which records undo steps. After every
//! change the graph is evaluated on the CPU (values, effective parameters) and sent to the
//! engine thread, which answers with images, thumbnails and reports as [`Event`]s.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::{image, operation, pane_grid, text_editor};
use iced::{Point, Rectangle, Size, Subscription, Task, Theme, Vector, keyboard, mouse, window};

use tessera_core::eval::{self, Evaluation};
use tessera_core::graph::{Clipboard, Graph, Link, NodeId, SocketRef};
use tessera_core::project::{self, ViewState};
use tessera_core::{Category, Document, Edit, NodeKind, ParamValue, Value};
use tessera_engine::{DisplayImage, EngineService, EvalReport, Event, ExportJob, ImageReport, Request};
use tessera_engine::simd::ImageStats;

use crate::gpu::{self, EventSource};
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
    pub fn to_graph(&self, window: Point) -> [f32; 2] {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    TwoD,
    ThreeD,
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
    pub mode: ViewMode,
    /// Picking a gradient along a line: the Gradient Map node and parameter to fill.
    pub pick: Option<(NodeId, String)>,
    pub probe: Option<[u32; 2]>,
    pub view3d: view::view3d::Camera,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pane {
    Library,
    Graph,
    Preview,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarField {
    Value,
    Default,
}

pub struct Ui {
    pub selection: BTreeSet<NodeId>,
    pub selected_link: Option<Link>,
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
    /// Pending line pick request tag (gradient picking).
    pub pick_tag: u64,
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
    pub report: EvalReport,
    pub evaluated_revision: u64,
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
    Move { ids: Vec<NodeId>, delta: [f32; 2] },
    Connect(SocketRef, SocketRef),
    Detach { node: NodeId, socket: String },
    LinkDropped { from: SocketRef, at: Point, graph_pos: [f32; 2] },
    OpenAddMenu { at: Point, graph_pos: [f32; 2] },
    Preview(NodeId),
    Context { at: Point, target: ContextTarget },
    View { pan: Vector, zoom: f32 },
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
    Mode(ViewMode),
    View { zoom: f32, pan: Vector, fit: bool },
    Bounds(Rectangle),
    Probe(Option<[u32; 2]>),
    PickLine { from: [f32; 2], to: [f32; 2] },
    CancelPick,
    Camera(view::view3d::Camera),
}

#[derive(Clone, Debug)]
pub enum AddMenuMessage {
    Query(String),
    Choose(NodeKind),
    Up,
    Down,
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
    SelectAll,
    Frame,
    ToggleBypass(NodeId),
    // layout
    PaneResized(pane_grid::ResizeEvent),
    ResetLayout,
    LockSquare,
    Menu(Option<MenuId>),
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
    Expression(NodeId, text_editor::Action),
    ColorPicker(Option<ColorTarget>),
    GradientStop(NodeId, String, usize),
    /// Replace a gradient and select one of its stops.
    SetGradient(NodeId, String, tessera_core::Gradient, Option<usize>),
    /// Typing the variable name of an Input node (applied on Enter).
    BindingText(NodeId, String),
    CommitBinding,
    ShowFunctions,
    PickGradient(NodeId, String),
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
    Screenshot(window::Screenshot),
}

// ---------------------------------------------------------------------------- boot

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
            split(Axis::Horizontal, 0.6, C::Pane(Pane::Graph), C::Pane(Pane::Preview)),
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
        let (graph, preview_node) = tessera_core::demo::brick_wall();
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
                graph_view: GraphView { pan: Vector::ZERO, zoom: 1.0, bounds: Rectangle::default() },
                frame_request: true,
                preview: PreviewState {
                    node: Some(preview_node),
                    channel: 0,
                    show_tiling: false,
                    zoom: 1.0,
                    pan: Vector::ZERO,
                    fit: true,
                    bounds: Rectangle::default(),
                    mode: ViewMode::TwoD,
                    pick: None,
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
                gradient_stop: HashMap::new(),
                expression: None,
                plot_var: 0,
                plot_range: (-5.0, 5.0),
                expanded_vars: HashSet::new(),
                renaming_var: None,
                binding: None,
                show_functions: false,
                pick_tag: 0,
            },
            opts: opts.clone(),
            window: None,
            started: Instant::now(),
            screenshot_requested: false,
        };
        if let Some(path) = opts.open.clone() {
            app.open_path(path);
        }
        let graph = app.doc.graph();
        if let Some(id) = find_kind(graph, opts.select.as_deref().unwrap_or("math")) {
            app.ui.selection.insert(id);
        }
        if let Some(id) = opts.preview.as_deref().and_then(|k| find_kind(graph, k)) {
            app.ui.preview.node = Some(id);
        }
        app.refresh();
        let task = match opts.scenario.as_deref() {
            Some(s) => app.apply_scenario(s),
            None => Task::none(),
        };
        app.sync_expression();
        (app, task)
    }

    pub fn title(&self) -> String {
        let dirty = if self.doc.is_dirty() { " •" } else { "" };
        format!("{}{dirty} — {}", self.doc.title(), tessera_core::APP_NAME)
    }

    pub fn theme(&self) -> Theme {
        crate::style::theme()
    }

    pub fn graph(&self) -> &Graph {
        self.doc.graph()
    }

    // ------------------------------------------------------------------ subscriptions

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![iced::event::listen_with(|event, status, window| {
            use iced::Event as E;
            let relevant = match &event {
                E::Keyboard(_) => true,
                E::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::ButtonReleased(_)) => true,
                E::Window(
                    window::Event::CloseRequested | window::Event::Opened { .. } | window::Event::Resized(_),
                ) => true,
                _ => false,
            };
            relevant.then_some(Message::Runtime(event, status, window))
        })];
        match &self.events {
            Some(source) => subs.push(source.subscription().map(Message::Engine)),
            None => subs.push(iced::time::every(Duration::from_millis(30)).map(|_| Message::PollGpu)),
        }
        if self.ui.status.is_some() || self.opts.screenshot.is_some() {
            subs.push(iced::time::every(Duration::from_millis(500)).map(|_| Message::Tick));
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
        if let Some((id, _)) = &self.ui.preview.pick
            && !graph.contains(*id)
        {
            self.ui.preview.pick = None;
        }
        self.res.thumbs.retain(|id, _| graph.contains(*id));
        self.send_graph();
    }

    fn send_graph(&self) {
        if self.engine.is_none() {
            return;
        }
        self.send(Request::Evaluate {
            revision: self.doc.revision(),
            graph: Arc::new(self.doc.graph().clone()),
            eval: self.eval.clone(),
        });
        self.send(Request::Preview(self.ui.preview.node));
        self.send(Request::Materials(self.material_nodes()));
    }

    /// Output nodes the 3D view uses (one per usage).
    pub fn material_nodes(&self) -> Vec<NodeId> {
        if self.ui.preview.mode != ViewMode::ThreeD {
            return Vec::new();
        }
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

    fn apply_all(&mut self, edits: Vec<Edit>) {
        for e in edits {
            self.apply(e);
        }
    }

    fn selected_ids(&self) -> Vec<NodeId> {
        self.ui.selection.iter().copied().collect()
    }

    fn set_preview(&mut self, node: Option<NodeId>) {
        if self.ui.preview.node != node {
            self.ui.preview.node = node;
            self.ui.preview.fit = true;
            self.ui.preview.pick = None;
            self.res.probe = None;
            self.send(Request::Preview(node));
        }
    }

    fn close_popups(&mut self) {
        self.ui.menu = None;
        self.ui.context = None;
        self.ui.add_menu = None;
    }

    /// Frame the selection (or everything) in the graph view.
    fn frame(&mut self, only_selected: bool) {
        let ids = self.selected_ids();
        let graph = self.doc.graph();
        let size = |n: &tessera_core::Node| view::graph::layout::node_size(graph, n);
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
        let ids = self.selected_ids();
        if !ids.is_empty() {
            self.apply(Edit::RemoveNodes(ids));
            self.ui.selection.clear();
        }
        self.doc.seal();
    }

    fn open_add_menu(&mut self, at: Point, graph_pos: [f32; 2], pending: Option<SocketRef>) -> Task<Message> {
        self.close_popups();
        self.ui.add_menu = Some(AddMenu { at, graph_pos, query: String::new(), selected: 0, pending });
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
        self.ui.selection.clear();
        self.ui.selected_link = None;
        self.ui.expression = None;
        self.ui.color_picker = None;
        self.ui.frame_request = true;
        self.ui.preview.node = preview;
        self.ui.preview.fit = true;
        self.ui.preview.pick = None;
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
        let name = self.doc.path().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string());
        let name = name.unwrap_or_else(|| format!("untitled.{}", project::FILE_EXTENSION));
        Task::perform(
            blocking(move || {
                rfd::FileDialog::new()
                    .set_title("Save Tessera graph")
                    .add_filter("Tessera graph", &[project::FILE_EXTENSION])
                    .set_file_name(name)
                    .save_file()
            }),
            Message::SavePicked,
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
                let (g, p) = tessera_core::demo::brick_wall();
                self.doc.replace(g, None);
                self.after_document_change(Some(p));
                Task::none()
            }
            PendingAction::OpenPath(path) => {
                self.open_path(path);
                Task::none()
            }
            PendingAction::Open => Task::perform(
                blocking(|| {
                    rfd::FileDialog::new()
                        .set_title("Open Tessera graph")
                        .add_filter("Tessera graph", &[project::FILE_EXTENSION, project::LEGACY_EXTENSION, "json"])
                        .pick_file()
                }),
                Message::OpenPicked,
            ),
            PendingAction::Quit => match self.window {
                Some(id) => window::close(id),
                None => iced::exit(),
            },
        }
    }

    fn export_jobs(&self, dir: &std::path::Path) -> Vec<ExportJob> {
        let graph = self.doc.graph();
        let outputs: Vec<&tessera_core::Node> = graph.nodes_of(NodeKind::OUTPUT).collect();
        let names: Vec<(String, u32)> =
            outputs.iter().map(|n| (n.text("name").to_string(), n.enum_value("bit_depth"))).collect();
        let paths = tessera_engine::export::output_paths(dir, &names);
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
            _ => self.ui.expression = Some((node.id, text_editor::Content::with_text(text))),
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
                    return Task::perform(blocking(gpu::adapter_name), Message::GpuName);
                }
            }
            Message::GpuName(name) => {
                log::info!("GPU: {name}");
                self.adapter = name;
            }
            Message::Engine(event) => return self.engine_event(event),
            Message::Runtime(event, status, id) => {
                self.window.get_or_insert(id);
                return self.runtime_event(event, status, id);
            }
            Message::Tick => {
                if self.ui.status.as_ref().is_some_and(|s| s.at.elapsed() > Duration::from_secs(if s.error { 12 } else { 6 })) {
                    self.ui.status = None;
                }
                return self.maybe_screenshot();
            }
            Message::Edit(edit) => {
                self.apply(edit);
            }
            Message::Edits(edits) => self.apply_all(edits),
            Message::Seal => self.doc.seal(),
            Message::Undo => {
                self.close_popups();
                if self.doc.undo() {
                    self.ui.expression = None;
                } else {
                    self.set_status("Nothing to undo", false);
                }
            }
            Message::Redo => {
                self.close_popups();
                if self.doc.redo() {
                    self.ui.expression = None;
                } else {
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
                        Message::ExportFolderPicked,
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
                    tessera_engine::export::sanitize_file_name(node.text("name")),
                    tessera_engine::export::extension(depth)
                );
                let ext = tessera_engine::export::extension(depth);
                return Task::perform(
                    blocking(move || {
                        rfd::FileDialog::new()
                            .set_title("Export output")
                            .add_filter(ext.to_uppercase(), &[ext])
                            .set_file_name(name)
                            .save_file()
                    }),
                    move |p| Message::ExportNodePicked(id, p),
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
                self.ui.selection = [id].into_iter().collect();
                self.ui.selected_link = None;
                self.doc.seal();
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
                    None => self.set_status("The clipboard holds no Tessera nodes", false),
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
            Message::SelectAll => {
                self.close_popups();
                self.ui.selection = self.graph().nodes.iter().map(|n| n.id).collect();
            }
            Message::Frame => {
                self.close_popups();
                let only = !self.ui.selection.is_empty();
                self.frame(only);
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
            Message::Context(c) => {
                self.ui.menu = None;
                self.ui.add_menu = None;
                self.ui.context = c;
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
                self.ui.color_picker = if self.ui.color_picker == target { None } else { target };
                self.doc.seal();
            }
            Message::GradientStop(node, key, index) => {
                self.ui.gradient_stop.insert((node, key), index);
            }
            Message::SetGradient(node, key, gradient, select) => {
                self.apply(tessera_core::edit::gradient_edit(node, &key, gradient));
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
            Message::PickGradient(node, key) => {
                let active = self.ui.preview.pick.as_ref() == Some(&(node, key.clone()));
                self.ui.preview.pick = if active { None } else { Some((node, key)) };
                if self.ui.preview.pick.is_some() {
                    self.ui.preview.mode = ViewMode::TwoD;
                    self.set_status("Drag a line across the 2D view to pick the gradient along it", false);
                }
            }
            Message::PlotVar(i) => self.ui.plot_var = i,
            Message::PlotRange(lo, hi) => {
                if lo.is_finite() && hi.is_finite() {
                    self.ui.plot_range = (lo.min(hi - 1e-3), hi.max(lo + 1e-3));
                }
            }
            Message::BrowseFile(node, key) => {
                return Task::perform(
                    blocking(|| {
                        rfd::FileDialog::new()
                            .set_title("Choose an image")
                            .add_filter("Images", &["png", "jpg", "jpeg", "tga", "bmp", "tif", "tiff", "exr"])
                            .pick_file()
                    }),
                    move |p| Message::FilePicked(node, key.clone(), p),
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
                        let new = tessera_core::graph::sanitize_identifier(&to);
                        if expanded {
                            self.ui.expanded_vars.insert(new);
                        }
                    } else if expanded {
                        self.ui.expanded_vars.insert(from);
                    }
                }
            }
            Message::Preview(msg) => return self.preview_message(msg),
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
            for (k, v) in [("in_low", mn), ("in_high", mx), ("gamma", 1.0)] {
                self.apply(Edit::SetParam { node, key: k.into(), value: ParamValue::Float(v) });
            }
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
            Event::Evaluated { revision, reports, report } => {
                self.res.reports = reports;
                self.res.report = report;
                self.res.evaluated_revision = revision;
                self.request_stats();
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
            Event::LineSamples { tag, colors } => {
                if tag == self.ui.pick_tag
                    && let Some((node, key)) = self.ui.preview.pick.take()
                    && colors.len() >= 2
                {
                    let gradient = tessera_core::Gradient::from_samples(&colors, 0.012, tessera_core::Gradient::MAX_STOPS);
                    let stops = gradient.stops.len();
                    self.apply(tessera_core::edit::gradient_edit(node, &key, gradient));
                    self.doc.seal();
                    self.set_status(format!("Picked a gradient with {stops} stops"), false);
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
        match event {
            E::Window(window::Event::CloseRequested) => {
                self.window = Some(id);
                return self.guarded(PendingAction::Quit);
            }
            E::Window(window::Event::Opened { size, .. } | window::Event::Resized(size)) => {
                self.ui.window_size = size;
            }
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
            E::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                return self.shortcut(key, modifiers, status);
            }
            _ => {}
        }
        Task::none()
    }

    fn shortcut(&mut self, key: keyboard::Key, m: keyboard::Modifiers, status: iced::event::Status) -> Task<Message> {
        use keyboard::key::Named;
        use keyboard::Key;
        let ctrl = m.command();
        let char = |c: &str| matches!(&key, Key::Character(s) if s.as_str().eq_ignore_ascii_case(c));
        // Escape closes whatever is open, even when a text field has focus.
        if key == Key::Named(Named::Escape) {
            if self.ui.add_menu.is_some() || self.ui.menu.is_some() || self.ui.context.is_some() {
                self.close_popups();
            } else if self.ui.dialog.is_some() {
                self.ui.dialog = None;
            } else if self.ui.preview.pick.is_some() {
                self.ui.preview.pick = None;
            } else if self.ui.color_picker.is_some() {
                self.ui.color_picker = None;
            }
            return Task::none();
        }
        if let Some(menu) = &mut self.ui.add_menu {
            match key {
                Key::Named(Named::ArrowDown) => menu.selected = menu.selected.saturating_add(1),
                Key::Named(Named::ArrowUp) => menu.selected = menu.selected.saturating_sub(1),
                _ => {}
            }
            return Task::none();
        }
        // Global shortcuts that also work while typing.
        if ctrl && char("s") {
            return self.update_nested(if m.shift() { Message::SaveAs } else { Message::Save });
        }
        if ctrl && char("o") {
            return self.update_nested(Message::Open);
        }
        if ctrl && char("e") {
            return self.update_nested(Message::ExportAll);
        }
        if status == iced::event::Status::Captured || self.ui.dialog.is_some() {
            return Task::none(); // a text field or widget used the key
        }
        let over_graph = self.ui.graph_view.contains(self.ui.cursor);
        let message = match key.as_ref() {
            _ if ctrl && (char("y") || (m.shift() && char("z"))) => Message::Redo,
            _ if ctrl && char("z") => Message::Undo,
            _ if ctrl && char("n") => Message::New,
            _ if ctrl && char("c") => Message::Copy,
            _ if ctrl && char("x") => Message::Cut,
            _ if ctrl && char("v") => Message::Paste,
            _ if ctrl && char("d") => Message::Duplicate,
            _ if ctrl && char("a") => Message::SelectAll,
            _ if ctrl && char("b") => match self.ui.selection.iter().next() {
                Some(id) => Message::ToggleBypass(*id),
                None => return Task::none(),
            },
            Key::Named(Named::Delete | Named::Backspace) => Message::Delete,
            Key::Named(Named::F1) => Message::Dialog(Some(Dialog::Shortcuts)),
            _ if !ctrl && char("f") => Message::Frame,
            Key::Named(Named::Space) if over_graph => {
                let at = self.ui.cursor;
                let pos = self.ui.graph_view.to_graph(at);
                return self.open_add_menu(at, pos, None);
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
            }
            GraphMessage::ClearSelection => {
                self.close_popups();
                self.ui.selection.clear();
                self.ui.selected_link = None;
            }
            GraphMessage::SelectLink(link) => {
                self.close_popups();
                self.ui.selection.clear();
                self.ui.selected_link = Some(link);
            }
            GraphMessage::Move { ids, delta } => {
                self.apply(Edit::MoveNodes { ids, delta });
                self.doc.seal();
            }
            GraphMessage::Connect(a, b) => {
                self.apply(Edit::Connect { a, b });
                self.doc.seal();
            }
            GraphMessage::Detach { node, socket } => {
                self.apply(Edit::Disconnect { node, socket });
            }
            GraphMessage::LinkDropped { from, at, graph_pos } => {
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
            GraphMessage::View { pan, zoom } => {
                self.ui.graph_view.pan = pan;
                self.ui.graph_view.zoom = zoom;
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
        let Some(menu) = &mut self.ui.add_menu else { return Task::none() };
        match msg {
            AddMenuMessage::Query(q) => {
                menu.query = q;
                menu.selected = 0;
            }
            AddMenuMessage::Up => menu.selected = menu.selected.saturating_sub(1),
            AddMenuMessage::Down => menu.selected += 1,
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
            PreviewMessage::Mode(mode) => {
                p.mode = mode;
                if mode == ViewMode::ThreeD {
                    p.pick = None;
                }
                let nodes = self.material_nodes();
                self.send(Request::Materials(nodes));
            }
            PreviewMessage::View { zoom, pan, fit } => {
                p.zoom = zoom;
                p.pan = pan;
                p.fit = fit;
            }
            PreviewMessage::Bounds(b) => p.bounds = b,
            PreviewMessage::Probe(px) => {
                p.probe = px;
                if let (Some(px), Some(node)) = (px, p.node) {
                    self.send(Request::Probe { node, pixel: px });
                }
            }
            PreviewMessage::PickLine { from, to } => {
                if let Some(node) = p.node {
                    self.ui.pick_tag += 1;
                    self.send(Request::SampleLine { node, from, to, count: 256, tag: self.ui.pick_tag });
                }
            }
            PreviewMessage::CancelPick => p.pick = None,
            PreviewMessage::Camera(c) => p.view3d = c,
        }
        Task::none()
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
                self.ui.color_picker = Some(ColorTarget::Variable("tint".into(), VarField::Value));
            }
            "transform" => {
                if let Some(id) = first(NodeKind::TRANSFORM_2D) {
                    self.ui.selection = [id].into_iter().collect();
                    self.ui.preview.node = Some(id);
                }
            }
            "3d" => self.ui.preview.mode = ViewMode::ThreeD,
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
            && (self.ui.preview.node.is_none() || self.res.preview.is_some());
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

    pub fn view(&self) -> iced::Element<'_, Message> {
        view::root(self)
    }
}

/// Clamp the add menu's selection to the number of items.
pub fn menu_index(menu: Option<&AddMenu>, len: usize) -> usize {
    menu.map_or(0, |m| m.selected.min(len.saturating_sub(1)))
}

/// Run blocking work (file dialogs) on a thread without blocking the UI.
pub fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> impl Future<Output = T> + Send {
    let (tx, rx) = futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    async move { rx.await.expect("worker thread") }
}

/// Current value of a variable (helper for views).
pub fn variable_value(graph: &Graph, name: &str) -> Option<Value> {
    graph.variable(name).map(|v| v.value)
}

pub fn window_size_hint() -> Size {
    Size::new(1680.0, 1000.0)
}
