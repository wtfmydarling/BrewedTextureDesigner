//! Application state and the update loop.
//!
//! The UI never changes the graph itself: every user action becomes a
//! [`brewed_texture_core::Edit`] applied to the [`Document`], which records undo steps. After every
//! change the graph is evaluated on the CPU (values, effective parameters), every graph it uses
//! is laid out in it (see [`packages`]), and it is sent to the engine thread, which answers
//! with images, thumbnails and reports as [`Event`]s.

pub(crate) mod autosave;
pub(crate) mod export_dialog;
#[cfg(any(test, feature = "dev-tools"))]
pub(crate) mod dev;
pub(crate) mod home;
pub(crate) mod library;
pub(crate) mod packages;
pub(crate) mod settings_dialog;
pub(crate) mod versions;

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::{image, operation, pane_grid, text_editor};
use iced::{Point, Rectangle, Size, Subscription, Task, Theme, Vector, keyboard, mouse, window};

use brewed_texture_core::eval::{self, Evaluation};
use brewed_texture_core::files::Held;
use brewed_texture_core::graph::{Clipboard, CommentId, Graph, Link, NodeId, SocketRef};
use brewed_texture_core::package::{Missing, Package, ResourceKind};
use brewed_texture_core::project::{self, ViewState};
use brewed_texture_core::{Category, Document, Edit, GraphId, NodeKind, ParamValue, Value};

pub use packages::OpenPackage;
pub use settings_dialog::{SettingsDialog, SettingsMessage};
use brewed_texture_engine::{DisplayImage, EngineService, Event, ExportJob, ImageReport, Request};
use brewed_texture_engine::simd::ImageStats;

use crate::gpu::{self, EventSource};
use crate::eyedropper;
use crate::picker::{self, PickTarget, Shot};
use crate::view;
use crate::view::frames::FrameRate;
use crate::settings::{self, Settings};
use crate::shortcuts::{Chord, Command};
use crate::workspace::Workspace;

// ---------------------------------------------------------------------------- options

#[derive(Clone, Debug, Default)]
pub struct StartupOptions {
    /// The packages to open (the last is edited).
    pub open: Vec<PathBuf>,
    /// How the window was laid out last time: kept, and saved again on quitting. `None`
    /// starts from the defaults and saves nothing.
    pub workspace: Option<Workspace>,
    /// Where unsaved work is kept to be recovered after a crash (see [`autosave`]), and what
    /// programs that stopped kept is found. `None` keeps nothing.
    pub recovery: Option<PathBuf>,
    /// Where the earlier versions of the packages are kept as they are saved (see [`versions`]).
    /// `None` keeps none.
    pub versions: Option<PathBuf>,
    /// The settings, and where they are kept (by default: as they are by default, nowhere).
    pub settings: settings::Store,
    /// The graphics API is the settings' choice, given in the environment (see
    /// [`settings::choose_graphics`]).
    pub graphics_from_settings: bool,
    /// Start on the Home screen (see [`home`]), with nothing being edited: as the program
    /// starts when it is not given packages to open.
    pub home: bool,
    /// What the tools for developing the program are asked for (see [`dev`]).
    #[cfg(any(test, feature = "dev-tools"))]
    pub dev: dev::DevOptions,
}

// ---------------------------------------------------------------------------- ui state

/// Something dragged from a panel onto the graph.
#[derive(Clone, Debug, PartialEq)]
pub enum DragPayload {
    Node(NodeKind),
    Variable(String),
    /// A graph of a package open in the Explorer: it becomes a node that uses it.
    Graph { key: u64, graph: GraphId, name: String },
    /// A graph of a package in a library folder (see [`library`]): it becomes a node that uses
    /// it, its package read first if it has to be.
    Library { package: String, graph: GraphId, name: String },
    /// A file of a package (and what the package calls it): it becomes the node that reads it.
    Resource { path: String, kind: ResourceKind, name: String },
}

/// A field that says which file something reads.
#[derive(Clone, Debug, PartialEq)]
pub enum FileTarget {
    /// A file parameter of a node: the file of a Bitmap, the font of a Text, the picture a node
    /// that uses a graph gives an image input of it…
    Param(NodeId, String),
    /// The picture of an image input of the graph (an input value).
    Variable(String),
    /// The picture an image input of the graph has by default.
    VariableDefault(String),
}

impl DragPayload {
    pub fn label(&self) -> String {
        match self {
            DragPayload::Node(k) => k.def().name.to_string(),
            DragPayload::Variable(v) => format!("Input '{v}'"),
            DragPayload::Graph { name, .. } | DragPayload::Library { name, .. } => format!("Graph '{name}'"),
            DragPayload::Resource { name, .. } => name.clone(),
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
    /// What lights the 3D view.
    pub lighting: view::view3d::Lighting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pane {
    /// The packages open, their graphs and their files.
    Explorer,
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

/// A submenu of a menu's item, open while the item is pointed at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubMenu {
    /// File → Open Recent Package.
    Recent,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ContextTarget {
    Node(NodeId),
    Link(Link),
    Comment(CommentId),
    Canvas { graph_pos: [f32; 2] },
    /// A package in the Explorer (by its key).
    Package(u64),
    /// A graph of a package in the Explorer.
    GraphItem(u64, GraphId),
    /// A file of a package in the Explorer.
    ResourceItem(u64, String),
    /// The Resources folder of a package in the Explorer.
    ResourcesFolder(u64),
}

/// What folds in the Explorer: a package, its Resources folder, and the folder of a kind of
/// file in it (there when the package has files of more than one kind).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fold {
    Package(u64),
    Resources(u64),
    Kind(u64, brewed_texture_core::ResourceKind),
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

/// Something that discards the unsaved changes of a package: asked about first.
#[derive(Clone, Debug, PartialEq)]
pub enum PendingAction {
    /// A new package, a package's file or the demo in place of the one being edited: how the
    /// autopilot and the tests start again (the menus open another package beside it).
    #[cfg(any(test, feature = "dev-tools"))]
    New,
    #[cfg(any(test, feature = "dev-tools"))]
    OpenPath(PathBuf),
    #[cfg(any(test, feature = "dev-tools"))]
    LoadDemo,
    /// Close the package open in the Explorer as this key.
    Close(u64),
    Quit,
}

/// Open `link` (a web page) in the browser the system has for it; not in the tests, which open
/// nothing outside the program.
fn open_link(link: &str) {
    if cfg!(test) || !link.starts_with("https://") {
        log::info!("not opened: {link}");
        return;
    }
    #[cfg(windows)]
    let opened = std::process::Command::new("explorer").arg(link).spawn();
    #[cfg(target_os = "macos")]
    let opened = std::process::Command::new("open").arg(link).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let opened = std::process::Command::new("xdg-open").arg(link).spawn();
    if let Err(e) = opened {
        log::warn!("{link}: {e}");
    }
}

/// What the window of the third-party notices is told.
#[derive(Clone, Debug)]
pub enum NoticesMessage {
    Search(String),
    /// Open where a work is (its repository, its page), in the browser.
    Open(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    Unsaved(PendingAction),
    /// What programs that stopped without quitting kept (see [`autosave`]).
    Recover,
    About,
    Shortcuts,
    Settings(Box<SettingsDialog>),
    /// A new package, or a new graph of one, from a template (see [`home`]).
    NewGraph(home::NewGraphDialog),
    /// What is exported of the graph, in what and where (see [`export_dialog`]).
    Export(export_dialog::ExportDialog),
    /// The earlier versions of a package (see [`versions`]).
    Versions(versions::VersionsDialog),
    /// Files a package names that are not there (see [`MissingFiles`]).
    Missing(MissingFiles),
    /// The works of others the program is made with, and their licenses (see
    /// [`crate::third_party`]).
    Notices(view::notices::NoticesDialog),
}

/// The files a package open in the Explorer names that are not there: they are looked for
/// one by one, and the package says where they are from then on.
#[derive(Clone, Debug, PartialEq)]
pub struct MissingFiles {
    /// The package's key in the Explorer.
    pub key: u64,
    pub items: Vec<Missing>,
    /// What the last file found said (another package than the one that was there, others
    /// found in the same folder).
    pub note: Option<String>,
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
    /// What is typed in the menu's search field: only the options it matches show.
    pub query: String,
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
    /// Where the side columns were split (by a drag, or the layout saved): a window too narrow
    /// for them makes one wider than its split says, and gives it back its split when it grows
    /// again (see [`LEFT_MIN`]).
    pub side_splits: [f32; 2],
    pub library_search: String,
    pub library_collapsed: HashSet<Category>,
    /// The categories of the library folders' graphs folded in the Library.
    pub library_folded: HashSet<String>,
    /// The page of the Home screen shown in place of the editor, if it is shown.
    pub home: Option<home::HomePage>,
    /// The packages opened last, as the Home screen shows them.
    pub home_recent: Vec<home::Recent>,
    /// How the Home screen shows the packages opened last, and sorts them; and how the dialog of
    /// a new graph shows its templates.
    pub home_layout: home_screen::Layout,
    pub template_layout: home_screen::Layout,
    pub home_sort: home_screen::Sort,
    /// The template a new graph was made from last (the next one is offered it).
    pub last_template: brewed_texture_core::templates::Template,
    /// What the last export wrote, and where (the next is offered them).
    pub export_preset: brewed_texture_core::export_presets::Preset,
    pub export_folder: Option<PathBuf>,
    pub menu: Option<MenuId>,
    pub submenu: Option<SubMenu>,
    pub context: Option<ContextMenu>,
    pub add_menu: Option<AddMenu>,
    pub dialog: Option<Dialog>,
    /// What follows the next save that is asked for (see `App::save_package`).
    pub after_save: Option<PendingAction>,
    /// The packages being saved, by their key.
    pub saving: HashMap<u64, packages::Saving>,
    /// The package files being opened.
    pub opening: HashSet<String>,
    /// What waits for the packages being saved to be written (quitting, closing one).
    pub after_saving: Option<PendingAction>,
    pub drag: Option<DragPayload>,
    /// A panel item was pressed; it becomes a drag once the cursor moves.
    pub press: Option<(DragPayload, Point)>,
    pub cursor: Point,
    pub modifiers: keyboard::Modifiers,
    pub window_size: Size,
    /// Whether the window fills the screen (its title bar's button says what it does then).
    pub maximized: bool,
    /// When the title bar was last pressed: a second press soon after is a double click.
    pub title_pressed: Option<Instant>,
    pub lock_square: bool,
    pub status: Option<Status>,
    pub clipboard: Option<Clipboard>,
    pub color_picker: Option<ColorTarget>,
    /// Where the color picker floats: where it was opened from.
    pub color_picker_at: Point,
    pub choice_menu: Option<ChoiceMenu>,
    /// The headings of the menus of choices under headings (an Output's usage) that are
    /// closed: their options hidden. Kept from one session to the next.
    pub closed_headings: std::collections::BTreeSet<String>,
    /// The 3D view draws frame after frame, rather than only when something changes.
    pub realtime: bool,
    /// The main window is on screen: on Windows, not until it has drawn a frame (see
    /// `Message::FirstFrame`).
    pub shown: bool,
    /// The main window, shown cloaked: drawn, not yet on screen.
    pub cloaked: Option<window::Id>,
    /// How fast the 3D view drew over the last second, and when it last said so.
    pub frame_rate: FrameRate,
    pub frame_rate_at: Option<Instant>,
    /// Selected stop of each gradient editor.
    pub gradient_stop: HashMap<(NodeId, String), usize>,
    /// The expression editor's text buffer (node, content).
    pub expression: Option<(NodeId, text_editor::Content)>,
    /// The text buffer of the editor of the selected comment's description.
    pub description: Option<(CommentId, text_editor::Content)>,
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
    /// Packages of the Explorer whose graphs and files are not shown.
    /// What is folded in the Explorer.
    pub explorer_folded: HashSet<Fold>,
    /// A graph being renamed in the Explorer: its package, itself, and the name typed.
    pub renaming_graph: Option<(u64, GraphId, String)>,
    /// The file of a package being renamed in the Explorer (the package's key, the file's path)
    /// and the name typed so far.
    pub renaming_file: Option<(u64, String, String)>,
    /// Quitting: the packages whose unsaved changes go.
    pub discarded: HashSet<u64>,
    /// The package files opened or saved last, the latest first (File menu).
    pub recent: Vec<String>,
    /// When each of them was opened or saved last, in seconds since 1970.
    pub recent_opened: std::collections::BTreeMap<String, u64>,
    /// The page of the settings dialog shown last.
    pub settings_page: settings::Page,
    /// Settings the program takes only when it starts were applied: it offers to start again.
    pub restart_offer: bool,
    /// Quitting to start again (see [`SettingsMessage::Restart`]).
    pub restarting: bool,
    /// What the program was started again with (it is not, in the tests).
    #[cfg(test)]
    pub started_again: Option<Vec<PathBuf>>,
    /// The folder a file dialog was answered from last.
    pub last_folder: Option<PathBuf>,
}

/// What the engine told us.
#[derive(Default)]
pub struct Results {
    pub reports: HashMap<NodeId, ImageReport>,
    pub thumbs: HashMap<NodeId, image::Handle>,
    pub preview: Option<DisplayImage>,
    pub materials: Vec<DisplayImage>,
    /// The image of the graph's Output of usage Panorama, made an environment (see
    /// `App::environment_shown`), and the node it is of.
    pub panorama: Option<(NodeId, Arc<crate::environment::Hdri>)>,
    pub stats: HashMap<NodeId, (u64, Arc<ImageStats>)>,
    pub stats_requested: HashMap<NodeId, u64>,
    pub probe: Option<(NodeId, [u32; 2], [f32; 4])>,
    /// What went wrong inside the graphs the nodes use, by the node of the graph being edited
    /// that uses them.
    pub inner: HashMap<NodeId, String>,
    /// Pictures of what a node of a kind makes with each option of a choice, everything else
    /// as it is when the node is added, by node kind and parameter: drawn once for the kinds
    /// the graph has, and kept.
    pub choices: HashMap<(&'static str, &'static str), Vec<image::Handle>>,
    /// The pictures asked for (and those drawn).
    pub choices_asked: HashSet<(&'static str, &'static str)>,
    /// The changes of the last second: when each was done, how long it took from the
    /// moment the engine started on it until the GPU was done with it, and how many nodes
    /// it computed again. Changes that computed nothing (moving a node, say) are not among
    /// them.
    pub updates: VecDeque<(Instant, f32, usize)>,
    /// GPU memory the engine's images take.
    pub memory: brewed_texture_engine::Memory,
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
    /// What is known of the files the open packages use, by path (see [`crate::previews`]).
    pub files: HashMap<String, crate::previews::FileState>,
    /// The files are being looked at.
    pub looking: bool,
    /// The files the open packages used when they were looked at last.
    pub looked: Vec<String>,
    /// Pictures of files waiting to be made (the file, its kind, when it changed last), and
    /// how many are being made: a few at a time, each a whole picture decoded (see
    /// [`crate::previews::AT_ONCE`]).
    pub previews_waiting: VecDeque<(String, ResourceKind, Option<std::time::SystemTime>)>,
    pub previews_making: usize,
    /// Why the engine stopped, if it did (it never should): nothing is computed any more.
    pub stopped: Option<String>,
}

pub struct App {
    /// The package being edited, whose graph `doc.graph()` is in the graph view.
    pub doc: Document,
    /// Its key in the Explorer.
    pub doc_key: u64,
    /// The other packages open in the Explorer.
    pub shelf: Vec<OpenPackage>,
    next_key: u64,
    /// Makes the numbers of the packages that nodes are copied from while they have no file
    /// (see [`App::origin`]): random for each run of the program.
    origins: std::collections::hash_map::RandomState,
    /// Packages whose graphs are used but that are not open in the Explorer, read from their
    /// files, by path.
    deps: HashMap<String, Package>,
    /// Package files that are used and could not be read.
    unreadable: HashSet<String>,
    /// Where the view of each graph was when another was edited, by package key and graph.
    views: HashMap<(u64, GraphId), ViewState>,
    /// Unsaved work kept for recovery after a crash (see [`autosave`]).
    autosave: autosave::Autosave,
    /// The settings, and where they are kept.
    pub settings: Settings,
    settings_store: settings::Store,
    pub eval: Arc<Evaluation>,
    /// What the engine computes: the graph being edited with the graphs it uses laid out in
    /// it, and its evaluation.
    flat: (Arc<Graph>, Arc<Evaluation>),
    /// For each node laid out from a graph used as a node, the node of the graph being edited
    /// that uses it.
    owners: Arc<HashMap<NodeId, NodeId>>,
    /// The package and the revision of it the evaluation is of: another package edited, or
    /// another revision of it, is evaluated again.
    seen: (u64, u64),
    /// How many graphs were sent to the engine: the engine answers with the number of the one
    /// it computed. (Revisions of one package would not do: another package may be at a
    /// revision the engine was at already.)
    eval_revision: u64,
    pub engine: Option<EngineService>,
    events: Option<EventSource>,
    pub adapter: String,
    pub res: Results,
    pub ui: Ui,
    opts: StartupOptions,
    window: Option<window::Id>,
    started: Instant,
    /// What the eyedropper needs to know about the app (its Wayland connection).
    host: eyedropper::Host,
    /// The graphs of the library folders (see [`library`]).
    pub library: library::Library,
    /// The pictures of package files, as PNG, by path: what their files had, or were saved with
    /// (see [`home`]).
    pictures: HashMap<String, Arc<Vec<u8>>>,
    /// What the Home screen shows that stays as it is (see [`home`]).
    pub home: home::HomeLists,
    /// The device the window and the engine share, once iced made it, and the pipeline of the
    /// 3D view that draws the pictures of packages with it (see `view3d::picture`).
    gpu: Option<gpu::GpuHandles>,
    picture_pipeline: Option<view::view3d::Pipeline>,
    /// Where the earlier versions of the packages are kept (see [`versions`]).
    versions: Option<brewed_texture_core::versions::Versions>,
    /// The tools for developing the program: screenshots, the autopilot (see [`dev`]).
    #[cfg(any(test, feature = "dev-tools"))]
    dev: dev::Dev,
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
    /// Open or close the menu of what lights the 3D view.
    LightingMenu(bool),
    /// Light the 3D view by an environment image (its file), or by the studio (`None`).
    Environment(Option<String>),
    /// Look for an environment image in a file dialog.
    BrowseEnvironment,
    EnvironmentPicked(Option<PathBuf>),
    /// An environment image was read from its file, or could not be.
    EnvironmentRead(String, Result<Arc<crate::environment::Hdri>, String>),
    /// Light the 3D view by the panorama of this Output of the graph (its usage is Panorama), or
    /// no more by one.
    LightBy(Option<NodeId>),
    /// The graph's panorama was made an environment: of the document (its key) and the node.
    PanoramaRead(u64, NodeId, Option<Arc<crate::environment::Hdri>>),
    /// The 3D view's brightness, in stops.
    Exposure(f32),
    /// Whether the environment image shows behind the model.
    Background(bool),
    /// How blurred it shows there.
    Blur(f32),
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

/// What is done in the Explorer.
#[derive(Clone, Debug)]
pub enum ExplorerMessage {
    /// Show or hide what a package, or a folder of it, holds.
    Toggle(Fold),
    OpenGraph(u64, GraphId),
    /// A new, empty graph in a package, at once (the tests'; File → New Graph… asks, see
    /// [`home`]).
    #[cfg(test)]
    NewGraph(u64),
    DuplicateGraph(u64, GraphId),
    DeleteGraph(u64, GraphId),
    StartRename(u64, GraphId),
    RenameText(String),
    CommitRename,
    /// Rename a file of a package (its key, the file's path), as a graph is renamed.
    StartRenameFile(u64, String),
    RenameFileText(String),
    CommitRenameFile,
    Save(u64),
    SaveAs(u64),
    Close(u64),
    /// Link files to a package (a file dialog).
    LinkFiles(u64),
    FilesPicked(u64, Vec<PathBuf>),
    RemoveResource(u64, String),
    /// Look for a file a package names that is not there (a file dialog).
    Locate(u64, String),
    Located(u64, String, Option<PathBuf>),
    Context(ContextTarget),
    /// Add the node that reads a file of a package (in the middle of the view, when no place
    /// is given).
    AddResourceNode(String, ResourceKind, Option<[f32; 2]>),
    /// Copy the files a package uses that are not beside it into a folder beside it, and have
    /// the package use the copies (see [`brewed_texture_core::package::collect_plan`]).
    CollectFiles(u64),
    /// The files were copied, or some could not be.
    FilesCollected(u64, Collected),
    /// Hold files in a package itself (a file dialog).
    ImportFiles(u64),
    ImportPicked(u64, Vec<PathBuf>),
    /// Hold these files of a package, linked now, in the package itself.
    Embed(u64, Vec<String>),
    /// Files were read for a package, to import (or, `embed`, to hold those it links to), and
    /// what could not be read.
    FilesRead { key: u64, embed: bool, files: Vec<(String, Held)>, failed: Vec<String> },
    /// Write a file a package holds to disk, and link to it instead (a file dialog).
    SaveResourceAs(u64, String),
    ResourceSavePicked(u64, String, Option<PathBuf>),
    /// A file a package held was written to disk (where), or could not be.
    ResourceSaved(u64, String, Result<String, String>),
    /// Take out of a package the files no node reads.
    RemoveUnused(u64),
}

/// What copying the files of a package did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Collected {
    /// Where each file was, and where its copy is.
    pub copied: Vec<(String, String)>,
    /// What could not be copied, and why.
    pub failed: Vec<String>,
}

/// Where a dialog was asked for: the package open as `key`, and its graph being edited. A
/// dialog does not keep the window from being used: what it answers is taken there (see
/// [`Message::Asked`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Asked {
    pub key: u64,
    pub graph: GraphId,
}

#[derive(Clone, Debug)]
pub enum Message {
    PollGpu,
    GpuName(String),
    /// Look at the files the open packages use (see [`crate::previews`]).
    LookAtFiles,
    FilesLooked(Vec<crate::previews::Look>),
    /// A picture of a file (of it as it was when it changed last), or why none could be made.
    PreviewMade(String, Option<std::time::SystemTime>, Result<(Vec<u8>, u32, u32, String), String>),
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
    /// A new, empty package beside the others, at once (the tests' and the autopilot's: File →
    /// New Package… asks what it is to be, see [`home`]).
    #[cfg(any(test, feature = "dev-tools"))]
    New,
    Open,
    OpenPicked(Option<PathBuf>),
    Save,
    SaveAs,
    SavePicked(Option<PathBuf>),
    /// All the Outputs exported into a folder (the autopilot's; people use the export dialog).
    #[cfg_attr(not(any(test, feature = "dev-tools")), allow(dead_code))]
    ExportFolderPicked(Option<PathBuf>),
    ExportNode(NodeId),
    ExportNodePicked(NodeId, Option<PathBuf>),
    Quit,
    /// The window's own title bar (where the program draws it, see [`OWN_TITLE_BAR`]).
    Window(WindowAction),
    Proceed(PendingAction),
    /// Save, then continue with the action (unsaved changes dialog).
    SaveThen(PendingAction),
    Dialog(Option<Dialog>),
    /// Open a package opened or saved before (the File menu).
    OpenRecent(String),
    /// Forget the packages opened or saved before.
    ClearRecent,
    /// The settings dialog (Edit → Settings…).
    Settings(SettingsMessage),
    /// Keep the unsaved work for recovery (see [`autosave`]).
    Autosave,
    /// Snapshots were written, or could not be (see [`autosave::Written`]).
    Autosaved(Vec<autosave::Written>),
    /// What becomes of what programs that stopped kept.
    Recover(autosave::Recovery),
    /// What a dialog answered (the message), for where it was asked: the package and graph
    /// are edited again first and the answer is taken there; closed meanwhile, it goes.
    Asked(Asked, Box<Message>),
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
    /// A submenu of the open menu is opened, or closed.
    SubMenu(Option<SubMenu>),
    /// A menu item was chosen: close the menus, then handle the item's message.
    FromMenu(Box<Message>),
    Context(Option<ContextMenu>),
    AddMenu(AddMenuMessage),
    // packages
    Explorer(ExplorerMessage),
    /// Open the graph a node uses.
    OpenReference(NodeId),
    /// Put the selected nodes in a graph of their own, used in their place.
    GraphFromSelection,
    // library & drag and drop
    LibrarySearch(String),
    LibraryToggle(Category),
    /// Fold or unfold a category of the library folders' graphs.
    LibraryFold(String),
    /// Look at the library folders again (see [`library`]).
    LookAtLibrary,
    LibraryLooked(Option<Arc<HashMap<String, library::Looked>>>),
    /// The package of a graph of the Library dropped on the graph was read.
    LibraryRead { package: String, graph: GraphId, name: String, pos: [f32; 2], result: Result<Arc<packages::Opened>, String> },
    /// The Home screen (see [`home`]).
    Home(home::HomeMessage),
    /// The dialog of a new package or graph.
    NewGraph(home::NewGraphMessage),
    /// The window of the third-party notices.
    Notices(NoticesMessage),
    /// The export dialog (see [`export_dialog`]).
    Export(export_dialog::ExportMessage),
    /// The earlier versions of a package (see [`versions`]).
    Versions(versions::VersionsMessage),
    PanelPress(DragPayload),
    PanelAdd(DragPayload),
    // properties
    /// Open the menu of a choice picked by its pictures, or close it.
    ChoiceMenu(Option<(NodeId, &'static str)>),
    /// Something was typed in the search field of such a menu.
    ChoiceSearch(String),
    /// A heading of a menu of choices under headings was opened or closed.
    ToggleHeading(String),
    /// An option of such a choice was picked.
    PickChoice(NodeId, &'static str, u32),
    // status bar
    /// How fast the 3D view draws (it says so a few times a second while it draws).
    FrameRate(FrameRate),
    ToggleRealtime,
    /// The main window was shown cloaked (see `crate::brand::cloak`).
    #[cfg(windows)]
    Cloaked(window::Id),
    /// A window drew a frame: the main window shows once it has one to show (or a second
    /// after it was shown cloaked, whatever happens).
    FirstFrame(window::Id),
    /// Quitting: whether the window fills the screen, for the next session.
    Quitting(bool),
    /// A package was written, or could not be (see `App::save_package`): its key, and where.
    /// A package was saved (and why its version could not be kept, if it could not), or not.
    Saved(u64, PathBuf, Result<Option<String>, String>),
    /// A package file was read, with the packages it uses, or could not be (see
    /// `App::open_file`): where, and whether it takes the place of the package being edited.
    Opened(String, bool, Result<Arc<packages::Opened>, String>),
    Expression(NodeId, text_editor::Action),
    /// Typing in the description of a comment.
    Description(CommentId, text_editor::Action),
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
    /// Choose the file a field reads, in a dialog.
    BrowseFile(FileTarget),
    FilePicked(FileTarget, Option<PathBuf>),
    /// A file chosen for a field, read to be held by the package (the settings say so), or why
    /// it could not be.
    FileHeld(FileTarget, Result<Held, String>),
    /// A file dropped on the window, read to be held by the package: the node that reads it,
    /// of its kind, goes where it was dropped.
    DropHeld(ResourceKind, [f32; 2], Result<Held, String>),
    /// Make a field read a file (one dropped on it from the Explorer), or none.
    SetFile(FileTarget, Option<String>),
    /// Read a file again (edited elsewhere).
    ReloadFile(String),
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
    #[cfg(any(test, feature = "dev-tools"))]
    Screenshot(window::Screenshot),
    /// The autopilot's turn (see [`crate::autopilot`]).
    #[cfg(any(test, feature = "dev-tools"))]
    Autopilot,
}

/// The folder a file dialog was answered from, when `message` is its answer: where the file
/// chosen is, or the folder chosen.
fn answered_from(message: &Message) -> Option<PathBuf> {
    let file = |p: &Option<PathBuf>| p.as_deref().and_then(Path::parent).map(Path::to_path_buf);
    match message {
        Message::Asked(_, answer) => answered_from(answer),
        Message::OpenPicked(p) | Message::SavePicked(p) | Message::ExportNodePicked(_, p) | Message::FilePicked(_, p) => file(p),
        Message::ExportFolderPicked(folder) | Message::Export(export_dialog::ExportMessage::Folder(folder)) => folder.clone(),
        Message::Preview(PreviewMessage::EnvironmentPicked(p)) => file(p),
        Message::Explorer(ExplorerMessage::Located(_, _, p) | ExplorerMessage::ResourceSavePicked(_, _, p)) => file(p),
        Message::Explorer(ExplorerMessage::FilesPicked(_, files) | ExplorerMessage::ImportPicked(_, files)) => {
            files.first().and_then(|f| f.parent()).map(Path::to_path_buf)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------- boot

const MIN_WINDOW: Size = Size::new(1000.0, 640.0);

/// Whether the program draws the window's title bar itself: its buttons at the right of the menu
/// bar, the rest of the bar what moves the window, the edges what size it. Not on macOS, whose
/// own (its buttons on the left, over the menu bar of the screen) programs keep.
pub const OWN_TITLE_BAR: bool = !cfg!(target_os = "macos");

/// What the window's own title bar does.
#[derive(Clone, Copy, Debug)]
pub enum WindowAction {
    Minimize,
    /// Fill the screen, or give the window back its size.
    ToggleMaximize,
    /// As closing a window does: unsaved changes asked about first.
    Close,
    /// The bar is pressed: it moves the window; pressed again at once, it fills the screen.
    TitlePressed,
    /// The bar is right-clicked: the system's menu of the window (on Windows and Wayland).
    TitleMenu,
    /// An edge or a corner is pressed: it sizes the window.
    Resize(window::Direction),
    /// What the system says of the window: whether it fills the screen.
    Maximized(bool),
}

/// What the window is asked to do by its own title bar.
#[derive(Clone, Copy, Debug)]
pub enum WindowRequest {
    Minimize,
    ToggleMaximize,
    /// Move with the cursor, the button held.
    Drag,
    /// The system's menu of the window (on Windows and Wayland; nothing elsewhere).
    SystemMenu,
    Resize(window::Direction),
}

fn main_window_settings(workspace: Option<&Workspace>) -> window::Settings {
    let size = workspace.and_then(|w| w.size).map_or(Size::new(1680.0, 1000.0), |[w, h]| {
        Size::new(w.max(MIN_WINDOW.width), h.max(MIN_WINDOW.height))
    });
    window::Settings {
        size,
        // On Windows it is cloaked, made to fill the screen and shown in that order, once
        // it is there (see `App::boot`).
        maximized: !cfg!(windows) && workspace.is_some_and(|w| w.maximized),
        min_size: Some(MIN_WINDOW),
        visible: !cfg!(windows),
        // Its own title bar, where the program draws it (see `OWN_TITLE_BAR`).
        decorations: !OWN_TITLE_BAR,
        // Closing asks about unsaved changes first.
        exit_on_close_request: false,
        icon: crate::brand::window_icon(),
        // Without the system's frame, a window of Windows 11 still has its shadow and round
        // corners.
        #[cfg(windows)]
        platform_specific: window::settings::PlatformSpecific {
            undecorated_shadow: true,
            corner_preference: window::settings::platform::CornerPreference::Round,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Where the panels are split at first.
const SPLITS: [f32; 6] = [0.13, 0.5, 0.765, 0.6, 0.42, 0.34];

/// The narrowest the column of the Explorer and the library, and the one of Input Values and
/// Properties, are let be while the window has room for them and the graph: narrower, the names
/// in them are cut short, and the fields of Properties left no room.
pub(crate) const LEFT_MIN: f32 = 190.0;
pub(crate) const RIGHT_MIN: f32 = 270.0;
/// What the graph and the views keep between them, at the least.
const MIDDLE_MIN: f32 = 420.0;

/// The panels, split where `splits` says (in the order the splits are made), or where they
/// are at first when it does not say that.
pub(crate) fn panes(splits: &[f32]) -> pane_grid::State<Pane> {
    use pane_grid::{Axis, Configuration as C};
    let r = match splits {
        s if s.len() == SPLITS.len() && s.iter().all(|r| (0.02..=0.98).contains(r)) => s,
        _ => &SPLITS[..],
    };
    let split = |axis, ratio, a, b| C::Split { axis, ratio, a: Box::new(a), b: Box::new(b) };
    pane_grid::State::with_configuration(split(
        Axis::Vertical,
        r[0],
        // The Explorer over the library, half and half, as in the programs this one follows.
        split(Axis::Horizontal, r[1], C::Pane(Pane::Explorer), C::Pane(Pane::Library)),
        split(
            Axis::Vertical,
            r[2],
            split(Axis::Horizontal, r[3], C::Pane(Pane::Graph), split(Axis::Vertical, r[4], C::Pane(Pane::View3D), C::Pane(Pane::View2D))),
            split(Axis::Horizontal, r[5], C::Pane(Pane::Inputs), C::Pane(Pane::Properties)),
        ),
    ))
}

fn default_panes() -> pane_grid::State<Pane> {
    panes(&SPLITS)
}

/// The splits of the side columns, and where they are: the one that has the Explorer and the
/// library on its left, the one that has Input Values and Properties on its right.
fn side_splits(panes: &pane_grid::State<Pane>) -> [(pane_grid::Split, f32); 2] {
    match panes.layout() {
        pane_grid::Node::Split { id, ratio, b, .. } => match b.as_ref() {
            pane_grid::Node::Split { id: inner, ratio: right, .. } => [(*id, *ratio), (*inner, *right)],
            _ => unreachable!("the panels are split as `panes` splits them"),
        },
        _ => unreachable!("the panels are split as `panes` splits them"),
    }
}

/// Where the panels are split now, in the order [`panes`] makes the splits in.
pub(crate) fn splits_of(panes: &pane_grid::State<Pane>) -> Vec<f32> {
    fn walk(node: &pane_grid::Node, out: &mut Vec<f32>) {
        if let pane_grid::Node::Split { ratio, a, b, .. } = node {
            out.push(*ratio);
            walk(a, out);
            walk(b, out);
        }
    }
    let mut out = Vec::new();
    walk(panes.layout(), &mut out);
    out
}

impl App {
    pub fn boot(opts: StartupOptions) -> (App, Task<Message>) {
        // Starting on the Home screen, nothing is being edited: a blank package, which gives way
        // to what is made or opened there. Else the demo.
        let (package, preview_node) = if opts.home {
            let settings = opts.settings.settings();
            (Package::new("Graph", Graph { resolution: settings.graph_size, precision: settings.graph_precision, ..Graph::default() }), None)
        } else {
            brewed_texture_core::demo::package()
        };
        let eval = Arc::new(eval::evaluate(&package.graphs[0].graph));
        let mut doc = Document::new(package, None);
        doc.set_undo_steps(opts.settings.settings().undo_steps as usize);
        let mut app = App {
            doc,
            doc_key: 1,
            shelf: Vec::new(),
            next_key: 1,
            origins: std::collections::hash_map::RandomState::new(),
            deps: HashMap::new(),
            unreadable: HashSet::new(),
            views: HashMap::new(),
            autosave: autosave::Autosave::default(),
            settings: opts.settings.settings(),
            settings_store: opts.settings.clone(),
            eval,
            flat: (Arc::new(Graph::default()), Arc::new(Evaluation::default())),
            owners: Arc::new(HashMap::new()),
            seen: (0, 0),
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
                    lighting: opts.workspace.as_ref().map_or_else(view::view3d::Lighting::default, Workspace::lighting),
                },
                panes: opts.workspace.as_ref().map_or_else(default_panes, |w| panes(&w.splits)),
                side_splits: [SPLITS[0], SPLITS[2]],
                library_search: String::new(),
                library_collapsed: HashSet::new(),
                library_folded: HashSet::new(),
                home: opts.home.then_some(home::HomePage::Home),
                home_recent: Vec::new(),
                home_layout: home::layout_of(opts.workspace.as_ref().and_then(|w| w.home_layout.as_deref())),
                home_sort: home::sort_of(opts.workspace.as_ref().and_then(|w| w.home_sort.as_deref())),
                template_layout: home::layout_of(opts.workspace.as_ref().and_then(|w| w.template_layout.as_deref())),
                last_template: brewed_texture_core::templates::Template::Pbr,
                export_preset: brewed_texture_core::export_presets::Preset::Outputs,
                export_folder: None,
                menu: None,
                submenu: None,
                context: None,
                add_menu: None,
                dialog: None,
                after_save: None,
                saving: HashMap::new(),
                opening: HashSet::new(),
                after_saving: None,
                drag: None,
                press: None,
                cursor: Point::ORIGIN,
                modifiers: keyboard::Modifiers::default(),
                window_size: Size::new(1680.0, 1000.0),
                maximized: false,
                title_pressed: None,
                lock_square: true,
                status: None,
                clipboard: None,
                color_picker: None,
                color_picker_at: Point::ORIGIN,
                choice_menu: None,
                closed_headings: opts.workspace.as_ref().map(|w| w.closed_headings.iter().cloned().collect()).unwrap_or_default(),
                realtime: opts.workspace.as_ref().and_then(|w| w.realtime).unwrap_or(true),
                shown: !cfg!(windows),
                cloaked: None,
                frame_rate: FrameRate::default(),
                frame_rate_at: None,
                gradient_stop: HashMap::new(),
                expression: None,
                description: None,
                plot_var: 0,
                plot_range: (-5.0, 5.0),
                expanded_vars: HashSet::new(),
                renaming_var: None,
                binding: None,
                show_functions: false,
                screen_pick: None,
                drop_burst: None,
                explorer_folded: HashSet::new(),
                renaming_graph: None,
                renaming_file: None,
                discarded: HashSet::new(),
                recent: opts.workspace.as_ref().map(|w| w.recent.clone()).unwrap_or_default(),
                recent_opened: opts.workspace.as_ref().map(|w| w.opened.clone()).unwrap_or_default(),
                settings_page: settings::Page::General,
                restart_offer: false,
                restarting: false,
                #[cfg(test)]
                started_again: None,
                last_folder: opts.workspace.as_ref().and_then(|w| w.last_folder.clone()).map(PathBuf::from),
            },
            opts: opts.clone(),
            window: None,
            started: Instant::now(),
            host: eyedropper::Host::default(),
            library: library::Library::default(),
            pictures: HashMap::new(),
            home: home::HomeLists::new(),
            gpu: None,
            picture_pipeline: None,
            versions: opts.versions.clone().map(brewed_texture_core::versions::Versions::new),
            #[cfg(any(test, feature = "dev-tools"))]
            dev: dev::Dev::new(&opts.dev),
        };
        // (The side columns as the saved layout split them.)
        app.ui.side_splits = side_splits(&app.ui.panes).map(|(_, ratio)| ratio);
        let mut opened: Vec<Task<Message>> = opts.open.iter().map(|path| app.open_file(path.clone(), false)).collect();
        opened.push(app.look_at_library());
        if opts.home {
            opened.push(app.home_message(home::HomeMessage::Show(home::HomePage::Home)));
        }
        if let Some(root) = &opts.recovery {
            app.start_autosave(root);
        }
        // The versions held to what the settings keep now; what no version needs any more goes.
        if let Some((versions, keep)) = app.versions_kept() {
            opened.push(Task::perform(blocking(move || versions.tidy(keep)), |_| Message::Tick));
        }
        #[cfg(any(test, feature = "dev-tools"))]
        app.select_at_start();
        app.refresh();
        // The state a screenshot shows (see `dev`).
        #[cfg(any(test, feature = "dev-tools"))]
        let scenario = app.scenario_at_start();
        #[cfg(not(any(test, feature = "dev-tools")))]
        let scenario = Task::none();
        // The environment image of the last session, read again.
        let lighting = match app.ui.preview.lighting.file.clone() {
            Some(file) => app.choose_environment(Some(file)),
            None => Task::none(),
        };
        app.sync_expression();
        app.sync_description();
        // The app runs as a daemon (the eyedropper opens more windows): it opens its main
        // window itself.
        let (main, open) = window::open(main_window_settings(opts.workspace.as_ref()));
        app.window = Some(main);
        #[cfg(windows)]
        let maximized = opts.workspace.as_ref().is_some_and(|w| w.maximized);
        let setup = open.then(move |id| {
            // A new window on Windows shows white until its first frame is drawn: it is shown
            // cloaked, which has it drawn without showing it (and made to fill the screen
            // there, if it did last time), and uncloaked once it has drawn a frame
            // (`Message::FirstFrame`). Elsewhere its settings show it.
            #[cfg(windows)]
            let appear = window::run(id, crate::brand::use_embedded_icon)
                .discard()
                .chain(window::run(id, |window| crate::brand::cloak(window, true)).discard())
                .chain(if maximized { window::maximize(id, true) } else { Task::none() })
                .chain(window::set_mode(id, window::Mode::Windowed))
                .chain(Task::done(Message::Cloaked(id)));
            #[cfg(not(windows))]
            let appear = Task::none();
            let host = window::run(id, |window: &dyn iced::window::Window| {
                // Where the pointer is locked while a value is dragged, and what the
                // eyedropper needs to cover the screen.
                crate::scrub::remember(window);
                eyedropper::Host::of(window)
            });
            Task::batch([appear, host.map(Message::Host), crate::window_size::hold(id)])
        });
        (app, Task::batch([setup, scenario, lighting, Task::batch(opened)]))
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
        format!("{} — {}{dirty} — {}", self.doc.entry().name, self.doc.title(), brewed_texture_core::APP_NAME)
    }

    pub fn theme(&self, _window: window::Id) -> Theme {
        crate::style::theme()
    }

    /// A new graph, as the settings say new graphs are.
    pub(crate) fn new_graph(&self) -> Graph {
        Graph { resolution: self.settings.graph_size, precision: self.settings.graph_precision, ..Graph::default() }
    }

    /// A document of `package`, as the settings say documents are.
    pub(crate) fn document(&self, package: Package, path: Option<PathBuf>) -> Document {
        let mut doc = Document::new(package, path);
        doc.set_undo_steps(self.settings.undo_steps as usize);
        doc
    }

    /// Where a file dialog opens, as the settings say (see [`settings::FileDialogs`]):
    /// `package` is the file of the package it is about (else the one being edited), and
    /// `saving_the_package`, it saves the package or one of its files. None: where the system
    /// opens it.
    pub(crate) fn dialog_folder(&self, package: Option<&Path>, saving_the_package: bool) -> Option<PathBuf> {
        let package = package.or(self.doc.path().map(PathBuf::as_path)).and_then(Path::parent).filter(|d| d.is_dir()).map(Path::to_path_buf);
        let last = self.ui.last_folder.clone().filter(|d| d.is_dir());
        match self.settings.file_dialogs {
            settings::FileDialogs::Auto => package.filter(|_| saving_the_package),
            settings::FileDialogs::Package => package.or(last),
            settings::FileDialogs::Last => last.or(package),
        }
    }

    /// The key of a command, as a menu shows it: its first ("" for none).
    pub fn key_of(&self, command: Command) -> String {
        self.settings.shortcuts.of(command).first().map(Chord::shown).unwrap_or_default()
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
                            | window::Event::Focused
                            | window::Event::FileDropped(_),
                    )
            );
            relevant.then_some(Message::Runtime(event, status, window))
        })];
        match &self.events {
            Some(source) => subs.push(source.subscription().map(Message::Engine)),
            None => subs.push(iced::time::every(Duration::from_millis(30)).map(|_| Message::PollGpu)),
        }
        if self.ui.status.is_some() {
            subs.push(iced::time::every(Duration::from_millis(500)).map(|_| Message::Tick));
        }
        // Unsaved work, kept for recovery.
        if self.autosave.session.is_some() && self.settings.keep_unsaved {
            let every = Duration::from_secs(60 * u64::from(self.settings.keep_every));
            subs.push(iced::time::every(every).map(|_| Message::Autosave));
        }
        // The files the packages use, on disk: they can come, go or change.
        if !self.res.looked.is_empty() {
            subs.push(iced::time::every(Duration::from_secs(2)).map(|_| Message::LookAtFiles));
        }
        #[cfg(any(test, feature = "dev-tools"))]
        self.dev_subscriptions(&mut subs);
        // The status bar's readings change with time alone: a change that takes long says
        // so, and what the last second held grows old, the frames of a 3D view that stopped
        // drawing too.
        let aging = self.res.busy_since.is_some() || !self.res.updates.is_empty() || self.ui.frame_rate_at.is_some();
        if aging {
            subs.push(iced::time::every(Duration::from_millis(200)).map(|_| Message::Tick));
        }
        // Until the main window shows: it does once it has drawn a frame, or a second after it
        // was shown cloaked whatever happens (should a system not draw a cloaked window).
        if !self.ui.shown {
            // (Raw: the others leave frames out.)
            subs.push(iced::event::listen_raw(|event, _, window| match event {
                iced::Event::Window(window::Event::RedrawRequested(_)) => Some(Message::FirstFrame(window)),
                _ => None,
            }));
            if let Some(id) = self.ui.cloaked {
                subs.push(iced::time::every(Duration::from_secs(1)).with(id).map(|(id, _)| Message::FirstFrame(id)));
            }
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
        #[cfg(any(test, feature = "dev-tools"))]
        if let Some(pilot) = &mut self.dev.pilot {
            pilot.status(&text, error);
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
        if (self.doc_key, self.doc.revision()) == self.seen {
            return;
        }
        // The nodes that use graphs follow what the graphs are now, in every open package.
        self.sync_instances();
        let mut evaluation = eval::evaluate(self.doc.graph());
        // What the engine computes: every graph used laid out in the graph. What keeps a node
        // that uses a graph from making it is that node's first problem.
        let flat = self.flatten();
        for (id, problem) in &flat.problems {
            if let Some(ne) = evaluation.nodes.get_mut(id) {
                ne.errors.insert(0, problem.clone());
            }
        }
        self.eval = Arc::new(evaluation);
        let flat_eval = Arc::new(eval::evaluate(&flat.graph));
        self.flat = (Arc::new(flat.graph), flat_eval);
        self.owners = Arc::new(flat.owners);
        self.seen = (self.doc_key, self.doc.revision());
        self.eval_revision += 1;
        let graph = self.doc.graph();
        self.ui.selection.retain(|id| graph.contains(*id));
        if self.ui.selected_link.as_ref().is_some_and(|l| !graph.links.contains(l)) {
            self.ui.selected_link = None;
        }
        // (An undo takes a comment away as well as a node: it was still selected, and Delete
        // offered to take it away.)
        if self.ui.selected_comment.is_some_and(|id| graph.comment(id).is_none()) {
            self.ui.selected_comment = None;
        }
        if self.ui.preview.node.is_some_and(|id| !graph.contains(id)) {
            self.ui.preview.node = None;
        }
        // A color picker open for what an edit took away closes (shown for nothing, it kept the
        // next Esc).
        let gone = match &self.ui.color_picker {
            Some(ColorTarget::Param(id, _) | ColorTarget::GradientStop(id, _)) => !graph.contains(*id),
            Some(ColorTarget::Comment(id)) => graph.comment(*id).is_none(),
            Some(ColorTarget::Variable(name, _)) => !graph.variables.iter().any(|v| v.name == *name),
            None => false,
        };
        if gone {
            self.ui.color_picker = None;
        }
        self.res.thumbs.retain(|id, _| graph.contains(*id));
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
        self.send(Request::Evaluate { revision: self.eval_revision, graph: self.flat.0.clone(), eval: self.flat.1.clone() });
        self.send(Request::Preview(self.ui.preview.node));
        self.send(Request::Materials(self.material_nodes()));
        self.send(Request::Panorama(self.panorama_node()));
        self.ask_for_choices();
    }

    /// The Outputs of the open graph whose usage is Panorama.
    pub fn panorama_outputs(&self) -> Vec<NodeId> {
        let panorama = brewed_texture_core::nodes::Usage::Panorama.index();
        self.graph().nodes_of(NodeKind::OUTPUT).filter(|n| n.enum_value("usage") == panorama).map(|n| n.id).collect()
    }

    /// The Output of the open graph whose panorama lights the 3D view: the one its node or the
    /// lighting menu asked for, while it is one of usage Panorama.
    pub fn panorama_node(&self) -> Option<NodeId> {
        self.ui.preview.lighting.panorama.filter(|id| self.panorama_outputs().contains(id))
    }

    /// What lights the 3D view: the graph's panorama, where one was asked for (once the engine
    /// has it); else the image chosen, or the studio (none).
    pub fn environment_shown(&self) -> Option<Arc<crate::environment::Hdri>> {
        if let Some((node, hdri)) = &self.res.panorama
            && self.panorama_node() == Some(*node)
        {
            return Some(hdri.clone());
        }
        self.ui.preview.lighting.shown.clone()
    }

    /// Whether the graph's panorama lights the 3D view now.
    pub fn lit_by_panorama(&self) -> bool {
        self.panorama_node().is_some()
    }

    /// Have the engine draw the pictures of the choices of the kinds of nodes the graph has,
    /// those it has not drawn yet: by the time a menu of them opens, they are there.
    fn ask_for_choices(&mut self) {
        // Once the engine is there (it asks when it starts).
        if self.engine.is_none() {
            return;
        }
        let mut asked = Vec::new();
        for node in &self.doc.graph().nodes {
            let kind = node.kind;
            for pd in kind.def().params.iter().filter(|pd| pd.pictured) {
                if self.res.choices_asked.insert((kind.id(), pd.key)) {
                    asked.push(Request::Choices { kind, key: pd.key.to_string() });
                }
            }
        }
        for request in asked {
            self.send(request);
        }
    }

    /// Output nodes the 3D view uses (one per usage, of those it shows). The 3D view is
    /// always on screen, so the maps it shows are always kept up to date.
    pub fn material_nodes(&self) -> Vec<NodeId> {
        let mut seen = HashSet::new();
        self.graph()
            .nodes_of(NodeKind::OUTPUT)
            .filter(|n| {
                brewed_texture_core::nodes::Usage::of(n.enum_value("usage")).is_some_and(|u| u.shown_in_3d()) && seen.insert(n.enum_value("usage"))
            })
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

    /// Take the names being typed (a graph's in the Explorer, an input value's, an Input node's)
    /// as Enter takes them: what is saved is what is shown.
    fn take_typed_names(&mut self) {
        if self.ui.renaming_graph.is_some() {
            let _ = self.explorer_message(ExplorerMessage::CommitRename);
        }
        if self.ui.renaming_file.is_some() {
            let _ = self.explorer_message(ExplorerMessage::CommitRenameFile);
        }
        if self.ui.renaming_var.is_some() {
            let _ = self.handle(Message::CommitRename);
        }
        if self.ui.binding.is_some() {
            let _ = self.handle(Message::CommitBinding);
        }
    }

    fn close_popups(&mut self) {
        self.ui.menu = None;
        self.ui.submenu = None;
        self.ui.context = None;
        self.ui.add_menu = None;
        self.ui.choice_menu = None;
        self.ui.preview.lighting.menu = false;
    }

    /// Keep the status bar's readings current: forget the updates older than a second and the
    /// frames of a 3D view that has drawn none since, and note a change that has taken long
    /// enough to say so.
    fn age_readings(&mut self) {
        while self.res.updates.front().is_some_and(|(at, ..)| at.elapsed() > view::toolbar::READING_SPAN) {
            self.res.updates.pop_front();
        }
        if self.ui.frame_rate_at.is_some_and(|at| at.elapsed() > view::frames::WINDOW) {
            self.ui.frame_rate = FrameRate::default();
            self.ui.frame_rate_at = None;
        }
        if self.res.busy_since.is_some_and(|since| since.elapsed() >= view::toolbar::COMPUTING_AFTER) {
            self.res.slow = true;
        }
    }

    /// Open the menu of a choice picked by its pictures, and have the engine draw them. A
    /// long menu has a search field, and what is typed goes into it right away.
    fn open_choice_menu(&mut self, node: NodeId, key: &'static str) -> Task<Message> {
        let Some((current, searched)) = self
            .graph()
            .node(node)
            .map(|n| (n.enum_value(key) as usize, n.def().param(key).is_some_and(|pd| view::choices::searched(pd) || !pd.groups.is_empty())))
        else {
            return Task::none();
        };
        self.close_popups();
        self.ui.choice_menu = Some(ChoiceMenu { node, key, cursor: current, query: String::new() });
        self.ask_for_choices();
        if searched { operation::focus(view::choices::search_id()) } else { Task::none() }
    }

    /// The options of the open menu of pictures that its search leaves, and how many
    /// there are to a row.
    fn choice_menu_options(&self) -> Option<(Vec<usize>, usize)> {
        let menu = self.ui.choice_menu.as_ref()?;
        let pd = self.graph().node(menu.node)?.def().param(menu.key)?;
        Some((view::choices::shown(pd, &menu.query, &self.ui.closed_headings), view::choices::columns_of(pd)))
    }

    /// Put the keyboard of the open menu on the option `at` of those its search leaves, and
    /// scroll the menu as far down as that option's row is down the list, which keeps the
    /// row in sight.
    fn choice_menu_to(&mut self, at: usize) -> Task<Message> {
        let Some((shown, columns)) = self.choice_menu_options() else { return Task::none() };
        let Some(menu) = &mut self.ui.choice_menu else { return Task::none() };
        let Some(&option) = shown.get(at) else { return Task::none() };
        menu.cursor = option;
        let rows = shown.len().div_ceil(columns);
        let down = if rows > 1 { (at / columns) as f32 / (rows - 1) as f32 } else { 0.0 };
        operation::snap_to(view::choices::list_id(), operation::RelativeOffset { x: None, y: Some(down) })
    }

    /// Show only the options of the open menu that match `query`; the keyboard stays on its
    /// option if that still shows, or goes to the first that does.
    fn search_choice_menu(&mut self, query: String) -> Task<Message> {
        let Some(menu) = &mut self.ui.choice_menu else { return Task::none() };
        menu.query = query;
        let cursor = menu.cursor;
        let Some((shown, _)) = self.choice_menu_options() else { return Task::none() };
        self.choice_menu_to(shown.iter().position(|&o| o == cursor).unwrap_or(0))
    }

    /// Move the keyboard through the open menu of pictures, among the options its search
    /// leaves: left and right go from option to option, up and down from row to row. While
    /// something is typed in the search field, left, right, Home and End move in the text.
    /// `None` when the key is not for the menu.
    fn move_in_choice_menu(&mut self, key: &keyboard::Key) -> Option<Task<Message>> {
        use keyboard::key::Named;
        let typing = !self.ui.choice_menu.as_ref()?.query.is_empty();
        let (shown, columns) = self.choice_menu_options()?;
        let columns = columns as isize;
        let step = match key {
            keyboard::Key::Named(Named::ArrowUp) => -columns,
            keyboard::Key::Named(Named::ArrowDown) => columns,
            keyboard::Key::Named(Named::ArrowLeft) if !typing => -1,
            keyboard::Key::Named(Named::ArrowRight) if !typing => 1,
            keyboard::Key::Named(Named::Home) if !typing => -(shown.len() as isize),
            keyboard::Key::Named(Named::End) if !typing => shown.len() as isize,
            _ => return None,
        };
        if shown.is_empty() {
            return Some(Task::none());
        }
        let cursor = self.ui.choice_menu.as_ref()?.cursor;
        let at = shown.iter().position(|&o| o == cursor).unwrap_or(0) as isize;
        Some(self.choice_menu_to((at + step).clamp(0, shown.len() as isize - 1) as usize))
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
        let mut clip = self.doc.graph().copy_nodes(&ids);
        // Nodes that use graphs of this package find them from another package by its file;
        // while it has none, no other package can.
        clip.package = self.doc.key();
        clip.origin = clip.package.is_none().then(|| self.origin());
        let text = clip.to_text();
        self.ui.clipboard = Some(clip);
        iced::clipboard::write(text)
    }

    /// A number of the package being edited for what is copied from it while it has no file
    /// (see [`Clipboard::origin`]): of this package in this run of the program only.
    fn origin(&self) -> u64 {
        use std::hash::BuildHasher;
        self.origins.hash_one(self.doc_key)
    }

    fn paste_clipboard(&mut self, mut clip: Clipboard) {
        // Pasted into another package, nodes that used graphs of their own package use them in
        // that package's file; of a package without one, they cannot.
        let elsewhere = match (&clip.package, clip.origin) {
            (Some(path), _) => Some(path) != self.doc.key().as_ref(),
            (None, Some(origin)) => origin != self.origin(),
            (None, None) => false,
        };
        if elsewhere {
            if clip.package.is_some() {
                clip.use_package();
            } else if clip.uses_graphs_of_its_package() {
                self.set_status(
                    "The nodes use graphs of a package that has no file: save it, and copy them again",
                    true,
                );
                return;
            }
        }
        // Nodes that use this graph, or a graph that uses it, cannot be here: not pasted, and
        // the status bar says why.
        let refused = clip.nodes.iter().filter_map(|n| n.instance.as_ref()).find_map(|i| self.use_refusal(&i.graph));
        if let Some(why) = refused {
            self.set_status(format!("Not pasted: {why}"), true);
            return;
        }
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

    fn after_document_change(&mut self, preview: Option<NodeId>) {
        // Node ids start over in the new document: forget everything that points at nodes
        // or variables of the old one.
        self.close_popups();
        self.ui.selection.clear();
        self.ui.selected_link = None;
        self.ui.selected_comment = None;
        self.ui.expression = None;
        self.ui.description = None;
        self.ui.color_picker = None;
        self.ui.gradient_stop.clear();
        self.ui.expanded_vars.clear();
        self.ui.renaming_var = None;
        self.ui.binding = None;
        self.ui.plot_var = 0;
        self.ui.drag = None;
        self.ui.press = None;
        self.ui.renaming_graph = None;
        self.ui.renaming_file = None;
        self.ui.frame_request = true;
        self.ui.preview.node = preview;
        self.ui.preview.fit = true;
        // The files being exported are written all the same, of the document before.
        self.res = Results { exports_pending: self.res.exports_pending, ..Results::default() };
        // The engine forgets the document before, and is told this one (see `Request::Reset`).
        self.send(Request::Reset);
        if (self.doc_key, self.doc.revision()) == self.seen {
            self.send_graph();
        } else {
            self.refresh();
        }
    }

    /// Do `job` off the UI thread (reading or writing a package may take a while), and take
    /// what it comes to as `then` says: none, if what did it stopped. In the tests, at once:
    /// they take the work of a message as done once it is handled.
    pub(crate) fn in_background<T: Send + 'static>(
        &mut self,
        job: impl FnOnce() -> T + Send + 'static,
        then: impl Fn(Option<T>) -> Message + Send + 'static,
    ) -> Task<Message> {
        #[cfg(test)]
        let task = self.handle(then(Some(job())));
        #[cfg(not(test))]
        let task = Task::perform(blocking(job), then);
        task
    }

    fn pick_save_path(&self) -> Task<Message> {
        let here = self.here();
        let hint = self.doc.path();
        let name = hint.and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string());
        let name = name.unwrap_or_else(|| format!("untitled.{}", project::FILE_EXTENSION));
        let folder = self.dialog_folder(None, true);
        Task::perform(
            blocking(move || {
                let mut dialog = rfd::FileDialog::new()
                    .set_title("Save package")
                    .add_filter(format!("{} package", brewed_texture_core::APP_NAME), &[project::FILE_EXTENSION])
                    .set_file_name(name);
                if let Some(folder) = folder {
                    dialog = dialog.set_directory(folder);
                }
                dialog.save_file()
            }),
            move |p| Message::Asked(here, Box::new(Message::SavePicked(p.flatten()))),
        )
    }

    /// Where a dialog asked for now is asked for (see [`Asked`]).
    pub fn here(&self) -> Asked {
        Asked { key: self.doc_key, graph: self.doc.current() }
    }

    /// What a dialog answered, for where it was asked (see [`Asked`]): the package and graph
    /// it was asked for are edited again, and the answer is taken there, not in whatever is
    /// edited when it comes. Closed meanwhile, the answer goes (and what was to follow a save);
    /// a dialog given up changes nothing.
    fn answered(&mut self, asked: Asked, answer: Message) -> Task<Message> {
        let given = matches!(
            answer,
            Message::SavePicked(Some(_))
                | Message::ExportFolderPicked(Some(_))
                | Message::ExportNodePicked(_, Some(_))
                | Message::FilePicked(_, Some(_))
        );
        if given {
            let there = self.package_doc(asked.key).is_some_and(|d| d.package().graph(asked.graph).is_some());
            if !there {
                self.ui.after_save = None;
                self.set_status("What the dialog was for is not open any more: nothing was done", true);
                return Task::none();
            }
            self.activate(asked.key, Some(asked.graph));
        }
        self.handle(answer)
    }

    /// Close the dialog that is open, as its Cancel does (Escape too): a quit asked about is
    /// given up, and the next one asks again about every package with unsaved changes, one not
    /// to be saved this time too (it may change meanwhile); what programs that stopped kept is
    /// offered again the next time the program starts.
    fn cancel_dialog(&mut self) {
        self.ui.discarded.clear();
        if self.ui.dialog == Some(Dialog::Recover) {
            self.recovery(autosave::Recovery::Later);
        }
        if std::mem::take(&mut self.ui.restarting) {
            // Not started again: still to be.
            self.ui.restart_offer = true;
        }
        self.ui.dialog = None;
    }

    /// Run an action that would discard unsaved changes, asking first: about the package it
    /// discards, or, quitting, about each package with unsaved changes in turn.
    fn guarded(&mut self, action: PendingAction) -> Task<Message> {
        self.close_popups();
        // What is being saved is written first: quitting or closing waits for it.
        if !self.ui.saving.is_empty() {
            self.ui.after_saving = Some(action);
            return Task::none();
        }
        let asking = match &action {
            PendingAction::Quit => self.dirty_packages().into_iter().find(|k| !self.ui.discarded.contains(k)),
            PendingAction::Close(key) => self.package_doc(*key).filter(|d| d.is_dirty()).map(|_| *key),
            #[cfg(any(test, feature = "dev-tools"))]
            _ => self.doc.is_dirty().then_some(self.doc_key),
        };
        match asking {
            Some(key) => {
                // The dialog is about the package being edited: that one.
                self.activate(key, None);
                self.ui.dialog = Some(Dialog::Unsaved(action));
                Task::none()
            }
            None => self.proceed(action),
        }
    }

    fn proceed(&mut self, action: PendingAction) -> Task<Message> {
        self.ui.dialog = None;
        match action {
            #[cfg(any(test, feature = "dev-tools"))]
            PendingAction::New => {
                let package = Package::new("Graph", self.new_graph());
                let open = package.first();
                self.replace_package(package, None, HashMap::new(), open);
                Task::none()
            }
            #[cfg(any(test, feature = "dev-tools"))]
            PendingAction::LoadDemo => {
                let (package, preview) = brewed_texture_core::demo::package();
                let open = package.first();
                let views = [(open, ViewState { preview, ..ViewState::default() })].into_iter().collect();
                self.replace_package(package, None, views, open);
                Task::none()
            }
            #[cfg(any(test, feature = "dev-tools"))]
            PendingAction::OpenPath(path) => self.open_file(path, true),
            PendingAction::Close(key) => {
                self.close_package(key);
                Task::none()
            }
            PendingAction::Quit => self.quit(),
        }
    }

    /// The file dialog to open a package.
    fn pick_package(&self) -> Task<Message> {
        let folder = self.dialog_folder(None, false);
        Task::perform(
            blocking(move || {
                let mut dialog = rfd::FileDialog::new()
                    .set_title("Open package")
                    .add_filter(format!("{} package", brewed_texture_core::APP_NAME), &[project::FILE_EXTENSION, "json"]);
                if let Some(folder) = folder {
                    dialog = dialog.set_directory(folder);
                }
                dialog.pick_file()
            }),
            |p| Message::OpenPicked(p.flatten()),
        )
    }

    /// Quit: save how the window is laid out (when the session keeps it), then go.
    fn quit(&mut self) -> Task<Message> {
        match self.window {
            Some(id) if self.opts.workspace.is_some() => window::is_maximized(id).map(Message::Quitting),
            _ => self.hide_and_exit(),
        }
    }

    /// Hide the window first: a window whose pixels go before it does flashes white. Nothing
    /// is left to recover: what was not saved, the user chose not to save.
    fn hide_and_exit(&mut self) -> Task<Message> {
        self.end_autosave();
        if self.ui.restarting {
            self.start_again();
        }
        match self.window {
            Some(id) => window::set_mode(id, window::Mode::Hidden).chain(iced::exit()),
            None => iced::exit(),
        }
    }

    fn save_workspace(&mut self, maximized: bool) {
        let Some(last) = &self.opts.workspace else { return };
        let size = self.ui.window_size;
        let workspace = Workspace {
            // (Where the side columns were split, not how wide a narrow window made them.)
            splits: {
                let mut splits = splits_of(&self.ui.panes);
                splits[0] = self.ui.side_splits[0];
                splits[2] = self.ui.side_splits[1];
                splits
            },
            // Filling the screen, the window is as big as the screen: the size it has when it
            // does not is the one it had.
            size: if maximized { last.size } else { Some([size.width, size.height]) },
            maximized,
            realtime: Some(self.ui.realtime),
            recent: self.ui.recent.clone(),
            opened: self.ui.recent_opened.iter().filter(|(p, _)| self.ui.recent.contains(p)).map(|(p, t)| (p.clone(), *t)).collect(),
            home_layout: Some(home::layout_said(self.ui.home_layout).into()),
            home_sort: Some(home::sort_said(self.ui.home_sort)),
            template_layout: Some(home::layout_said(self.ui.template_layout).into()),
            closed_headings: self.ui.closed_headings.iter().cloned().collect(),
            last_folder: self.ui.last_folder.as_ref().map(|f| f.display().to_string()),
            ..Workspace::default()
        }
        .with_lighting(&self.ui.preview.lighting);
        if let Err(e) = workspace.save() {
            log::warn!("the layout was not saved: {e}");
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
                pack: Vec::new(),
            })
            .collect()
    }

    // ------------------------------------------------------------------ update

    pub fn update(&mut self, message: Message) -> Task<Message> {
        if let Some(folder) = answered_from(&message) {
            self.ui.last_folder = Some(folder);
        }
        let mut task = self.handle(message);
        // An environment image a graph's view asked for as it was shown again: read now.
        if let Some(file) = self.ui.preview.lighting.wanted.take() {
            task = Task::batch([task, self.choose_environment(Some(file))]);
        }
        self.refresh();
        self.sync_expression();
        self.sync_description();
        // A file a package began to use is looked at now, not at the next look.
        if !self.res.looking && self.new_files() {
            return Task::batch([task, self.look_at_files()]);
        }
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
            Message::LookAtFiles => return self.look_at_files(),
            Message::FilesLooked(looks) => return self.files_looked(looks),
            Message::PreviewMade(path, of, made) => return self.preview_made(path, of, made),
            Message::PollGpu => {
                if let Some(handles) = gpu::take() {
                    self.gpu = Some(handles.clone());
                    #[cfg(any(test, feature = "dev-tools"))]
                    if let Some(pilot) = &mut self.dev.pilot {
                        pilot.watch(handles.device.clone());
                    }
                    let (service, source) = gpu::start_engine(handles, "GPU".into());
                    self.engine = Some(service);
                    self.events = Some(source);
                    self.send_graph();
                    // (The device says how large a texture, so the window, may be.)
                    let held = self.window.map_or_else(Task::none, crate::window_size::hold);
                    let named = Task::perform(blocking(gpu::adapter_name), |name| Message::GpuName(name.unwrap_or_default()));
                    return Task::batch([held, named]);
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
                self.age_readings();
                #[cfg(any(test, feature = "dev-tools"))]
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
                    // What the image nodes take at most, all at once: each image in its
                    // precision (2 bytes a pixel of a grayscale image in half floats, 8 of a
                    // color one), and what an output writes.
                    use brewed_texture_core::nodes::ImageKind;
                    use brewed_texture_engine::gpu::{format_of, output_format};
                    let graph = self.graph();
                    let nodes: Vec<&brewed_texture_core::graph::Node> = graph.nodes.iter().filter(|n| n.def().has_image()).collect();
                    let per_pixel: u64 = nodes
                        .iter()
                        .map(|n| {
                            let kind = view::graph::made_kind(self, n.id).unwrap_or(ImageKind::Color);
                            let precision = brewed_texture_core::Precision::of_node(n.enum_value("precision"), graph.precision);
                            let format = if n.kind == NodeKind::OUTPUT {
                                output_format(n.enum_value("format"), n.enum_value("bit_depth"), kind, Some(format_of(kind, graph.precision)))
                            } else {
                                format_of(kind, precision)
                            };
                            u64::from(format.block_copy_size(None).unwrap_or(8))
                        })
                        .sum();
                    let gb = (w as u64 * h as u64 * per_pixel) as f64 / (1u64 << 30) as f64;
                    let images = nodes.len();
                    self.set_status(format!("{w}×{h}: up to {gb:.1} GB of GPU memory for the {images} image nodes"), false);
                }
            }
            Message::Edits(edits) => self.apply_all(edits),
            Message::Seal => self.doc.seal(),
            // The expression editor's text follows through `sync_expression`; its buffer is
            // kept so the editor stays focused.
            Message::Undo => {
                self.close_popups();
                let (graph, view) = (self.doc.current(), self.view_state());
                if !self.doc.undo() {
                    self.set_status("Nothing to undo", false);
                }
                self.stepped_from(graph, view);
            }
            Message::Redo => {
                self.close_popups();
                let (graph, view) = (self.doc.current(), self.view_state());
                if !self.doc.redo() {
                    self.set_status("Nothing to redo", false);
                }
                self.stepped_from(graph, view);
            }
            // A new package, another one or the demo come to the Explorer: nothing is discarded.
            #[cfg(any(test, feature = "dev-tools"))]
            Message::New => {
                self.close_popups();
                self.new_package();
            }
            Message::Open => {
                self.close_popups();
                return self.pick_package();
            }
            Message::Quit => {
                self.ui.discarded.clear();
                return self.guarded(PendingAction::Quit);
            }
            Message::Window(action) => return self.window_action(action),
            // "Don't Save": quitting, the next package with unsaved changes asks in its turn.
            Message::Proceed(PendingAction::Quit) => {
                self.ui.discarded.insert(self.doc_key);
                return self.guarded(PendingAction::Quit);
            }
            Message::Proceed(action) => return self.proceed(action),
            Message::SaveThen(action) => {
                self.ui.dialog = None;
                self.ui.after_save = Some(action);
                return self.handle(Message::Save);
            }
            Message::OpenPicked(path) => {
                if let Some(path) = path {
                    return self.open_file(path, false);
                }
            }
            Message::OpenRecent(path) => {
                self.close_popups();
                // (Asked of the disk once, on the click: the menu itself never is.)
                if !Path::new(&path).is_file() {
                    self.ui.recent.retain(|p| *p != path);
                    self.set_status(format!("{path} is not there any more: it is taken off the recent packages"), true);
                } else {
                    return self.open_file(PathBuf::from(path), false);
                }
            }
            Message::ClearRecent => {
                self.close_popups();
                self.ui.recent.clear();
                self.ui.recent_opened.clear();
            }
            Message::Save => {
                self.close_popups();
                self.take_typed_names();
                match self.doc.path().cloned() {
                    Some(path) => return self.save_package(self.doc_key, path),
                    None => return self.pick_save_path(),
                }
            }
            Message::SaveAs => {
                self.close_popups();
                self.take_typed_names();
                return self.pick_save_path();
            }
            Message::SavePicked(path) => match path {
                Some(mut path) => {
                    if path.extension().is_none() {
                        path.set_extension(project::FILE_EXTENSION);
                    }
                    return self.save_package(self.doc_key, path);
                }
                None => self.ui.after_save = None,
            },
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
                let here = self.here();
                let folder = self.dialog_folder(None, false);
                return Task::perform(
                    blocking(move || {
                        let mut dialog = rfd::FileDialog::new().set_title("Export output").add_filter(ext.to_uppercase(), &[ext]).set_file_name(name);
                        if let Some(folder) = folder {
                            dialog = dialog.set_directory(folder);
                        }
                        dialog.save_file()
                    }),
                    move |p| Message::Asked(here, Box::new(Message::ExportNodePicked(id, p.flatten()))),
                );
            }
            Message::ExportNodePicked(id, Some(path)) => {
                if let Some(node) = self.graph().node(id) {
                    let job = ExportJob {
                        node: id,
                        path,
                        format: self.eval.params(id).map_or(node.enum_value("format"), |p| p.u("format")),
                        bit_depth: node.enum_value("bit_depth"),
                        pack: Vec::new(),
                    };
                    self.res.exports_pending += 1;
                    self.send(Request::Export(vec![job]));
                }
            }
            Message::ExportNodePicked(_, None) => {}
            Message::Dialog(None) => {
                self.close_popups();
                self.cancel_dialog();
            }
            Message::Dialog(Some(d)) => {
                self.close_popups();
                // What was found is offered again next time.
                if self.ui.dialog == Some(Dialog::Recover) && d != Dialog::Recover {
                    self.recovery(autosave::Recovery::Later);
                }
                self.ui.dialog = Some(d);
            }
            Message::Asked(asked, answer) => return self.answered(asked, *answer),
            Message::Autosave => return self.autosave(),
            Message::Settings(message) => return self.settings_message(message),
            Message::Autosaved(done) => self.autosaved(done),
            Message::Recover(choice) => self.recovery(choice),
            Message::Explorer(msg) => return self.explorer_message(msg),
            Message::OpenReference(node) => {
                self.close_popups();
                self.open_reference(node);
            }
            Message::GraphFromSelection => self.graph_from_selection(),
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
            Message::PaneResized(e) => {
                self.ui.panes.resize(e.split, e.ratio);
                if let Some(side) = side_splits(&self.ui.panes).iter().position(|(id, _)| *id == e.split) {
                    self.ui.side_splits[side] = e.ratio;
                }
                self.fit_columns();
            }
            Message::ResetLayout => {
                self.ui.panes = default_panes();
                self.ui.side_splits = [SPLITS[0], SPLITS[2]];
                self.fit_columns();
            }
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
                self.ui.submenu = None;
                self.ui.menu = if self.ui.menu == m { None } else { m };
            }
            Message::MenuHover(id) => {
                if self.ui.menu.is_some_and(|open| open != id) {
                    self.ui.menu = Some(id);
                    self.ui.submenu = None;
                }
            }
            Message::SubMenu(s) => {
                if self.ui.menu.is_some() {
                    self.ui.submenu = s;
                }
            }
            Message::Context(c) => {
                self.ui.menu = None;
                self.ui.add_menu = None;
                self.ui.choice_menu = None;
                self.ui.context = c;
            }
            Message::FrameRate(rate) => {
                self.ui.frame_rate = rate;
                self.ui.frame_rate_at = Some(Instant::now());
                self.age_readings();
            }
            Message::ToggleRealtime => self.ui.realtime = !self.ui.realtime,
            #[cfg(windows)]
            Message::Cloaked(id) => self.ui.cloaked = Some(id),
            Message::FirstFrame(id) => {
                if Some(id) == self.window && !self.ui.shown {
                    self.ui.shown = true;
                    log::debug!("the main window shows, {:?} after the start", self.started.elapsed());
                    #[cfg(windows)]
                    return window::run(id, |window| crate::brand::cloak(window, false)).discard();
                }
            }
            Message::Quitting(maximized) => {
                self.save_workspace(maximized);
                return self.hide_and_exit();
            }
            Message::Saved(key, path, result) => return self.saved(key, path, result),
            Message::Opened(file, replace, result) => self.opened(file, replace, result),
            Message::ChoiceMenu(Some((node, key))) => return self.open_choice_menu(node, key),
            Message::ChoiceMenu(None) => self.ui.choice_menu = None,
            Message::ChoiceSearch(query) => return self.search_choice_menu(query),
            Message::ToggleHeading(heading) => {
                if !self.ui.closed_headings.remove(&heading) {
                    self.ui.closed_headings.insert(heading);
                }
            }
            Message::PickChoice(node, key, option) => {
                self.ui.choice_menu = None;
                // A pick is a step of its own in the history.
                self.doc.seal();
                self.apply(Edit::SetParam { node, key: key.to_string(), value: ParamValue::Enum(option) });
                self.doc.seal();
            }
            Message::AddMenu(msg) => return self.add_menu_message(msg),
            Message::LibrarySearch(q) => self.ui.library_search = q,
            Message::LibraryFold(category) => {
                if !self.ui.library_folded.remove(&category) {
                    self.ui.library_folded.insert(category);
                }
            }
            Message::LookAtLibrary => return self.look_at_library(),
            Message::Home(message) => return self.home_message(message),
            Message::NewGraph(message) => return self.new_graph_message(message),
            Message::Notices(message) => {
                if let Some(Dialog::Notices(dialog)) = &mut self.ui.dialog {
                    match message {
                        NoticesMessage::Search(query) => dialog.query = query,
                        NoticesMessage::Open(link) => open_link(&link),
                    }
                }
            }
            Message::Export(message) => return self.export_message(message),
            Message::Versions(message) => return self.versions_message(message),
            Message::LibraryLooked(found) => return self.library_looked(found),
            Message::LibraryRead { package, graph, name, pos, result } => self.library_read(package, graph, name, pos, result),
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
                return self.add_payload(payload, [pos[0] - 80.0, pos[1] - 40.0]);
            }
            Message::Expression(node, action) => self.expression_action(node, action),
            Message::Description(id, action) => self.description_action(id, action),
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
            Message::BrowseFile(target) => {
                let extensions = self.takes(&target);
                let here = self.here();
                let folder = self.dialog_folder(None, false);
                return Task::perform(
                    blocking(move || {
                        let mut dialog = rfd::FileDialog::new().set_title("Choose a file");
                        if !extensions.is_empty() {
                            dialog = dialog.add_filter("Supported files", extensions);
                        }
                        if let Some(folder) = folder {
                            dialog = dialog.set_directory(folder);
                        }
                        dialog.pick_file()
                    }),
                    move |p| Message::Asked(here, Box::new(Message::FilePicked(target.clone(), p.flatten()))),
                );
            }
            Message::FilePicked(target, Some(path)) => {
                let path = packages::clean(&path);
                return match self.settings.new_files {
                    settings::NewFiles::Link => self.set_file(target, Some(path)),
                    settings::NewFiles::Embed => {
                        let here = self.here();
                        packages::read_held(path, move |held| Message::Asked(here, Box::new(Message::FileHeld(target, held))))
                    }
                };
            }
            Message::FilePicked(_, None) => {}
            Message::FileHeld(target, Ok(held)) => {
                let path = packages::hold(held);
                return self.set_file(target, Some(path));
            }
            Message::DropHeld(kind, pos, Ok(held)) => {
                let path = packages::hold(held);
                self.add_resource_node(path, kind, Some(pos));
            }
            Message::FileHeld(_, Err(e)) | Message::DropHeld(_, _, Err(e)) => self.set_status(e, true),
            Message::SetFile(target, path) => {
                // (Dropped from the Explorer: the drag ends here.)
                self.ui.drag = None;
                self.ui.press = None;
                return self.set_file(target, path);
            }
            Message::ReloadFile(path) => self.send(Request::ReloadFile(path)),
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
            #[cfg(any(test, feature = "dev-tools"))]
            Message::Screenshot(shot) => return self.save_screenshot(shot),
            #[cfg(any(test, feature = "dev-tools"))]
            Message::Autopilot => return self.fly(),
        }
        Task::none()
    }

    /// What was to follow a save, once it is done.
    fn continue_after(&mut self, then: Option<PendingAction>) -> Task<Message> {
        match then {
            // Quitting: the other packages with unsaved changes ask in their turn.
            Some(PendingAction::Quit) => self.guarded(PendingAction::Quit),
            Some(action) => self.proceed(action),
            None => Task::none(),
        }
    }

    fn add_payload(&mut self, payload: DragPayload, pos: [f32; 2]) -> Task<Message> {
        let edit = match payload {
            DragPayload::Node(kind) => Edit::AddNode { kind, pos },
            DragPayload::Variable(name) => Edit::AddVariableNode { name, pos },
            other => return self.add_package_payload(other, pos),
        };
        self.apply(edit);
        self.doc.seal();
        Task::none()
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

    /// Typing in the description of a comment: it says it as it is typed (one step of undo
    /// for all of it, see `Edit::merge_key`).
    fn description_action(&mut self, id: CommentId, action: text_editor::Action) {
        let Some(current) = self.graph().comment(id).map(|c| c.description.clone()) else { return };
        let content = match &mut self.ui.description {
            Some((cid, content)) if *cid == id => content,
            _ => {
                self.ui.description = Some((id, text_editor::Content::with_text(&current)));
                &mut self.ui.description.as_mut().expect("just set").1
            }
        };
        let is_edit = action.is_edit();
        content.perform(action);
        if is_edit {
            // (The editor ends what it holds with a line of its own.)
            let text = content.text().trim_end_matches(['\n', '\r']).to_string();
            if text != current {
                self.apply(Edit::SetCommentDescription { id, text });
            }
        }
    }

    /// Keep the buffer of the description editor as the selected comment's description, as
    /// `sync_expression` does the expression's: replaced only when it changed from outside.
    fn sync_description(&mut self) {
        let Some(comment) = self.ui.selected_comment.and_then(|id| self.doc.graph().comment(id)) else {
            self.ui.description = None;
            return;
        };
        match &self.ui.description {
            Some((id, content)) if *id == comment.id && content.text().trim_end_matches(['\n', '\r']) == comment.description => {}
            _ => {
                let mut content = text_editor::Content::with_text(&comment.description);
                content.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
                self.ui.description = Some((comment.id, content));
            }
        }
    }

    // ------------------------------------------------------------------ engine events

    fn engine_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Evaluated { revision, reports } => {
                // What went wrong inside a graph a node uses is that node's.
                self.res.inner.clear();
                let mut inner: Vec<(&NodeId, &ImageReport)> = reports.iter().filter(|(id, r)| id.is_inner() && r.error.is_some()).collect();
                inner.sort_by_key(|(id, _)| **id);
                for (id, report) in inner {
                    if let (Some(owner), Some(e)) = (self.owners.get(id), &report.error) {
                        // (What reads the picture of an Image Input is the input's, not inside it.)
                        let inside = self.graph().node(*owner).is_some_and(|n| n.instance.is_some());
                        self.res.inner.entry(*owner).or_insert_with(|| if inside { format!("Inside: {e}") } else { e.clone() });
                    }
                }
                self.res.reports = reports;
                self.res.evaluated_revision = revision;
                self.request_stats();
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
                if report.recomputed > 0 {
                    self.res.updates.push_back((Instant::now(), millis, report.recomputed));
                }
                self.age_readings();
                #[cfg(any(test, feature = "dev-tools"))]
                if let Some(pilot) = &mut self.dev.pilot {
                    pilot.finished(millis, report.recomputed);
                }
            }
            Event::Memory(bytes) => self.res.memory = bytes,
            Event::Choices(c) => {
                if let Some(pd) = c.kind.def().param(&c.key) {
                    let pictures = c.pictures.iter().map(|rgba| image::Handle::from_rgba(c.size, c.size, rgba.clone())).collect();
                    self.res.choices.insert((c.kind.id(), pd.key), pictures);
                }
            }
            Event::Thumbnails(thumbs) => {
                for t in thumbs {
                    self.res.thumbs.insert(t.node, image::Handle::from_rgba(t.size, t.size, t.rgba));
                }
            }
            Event::Preview(p) => self.res.preview = p,
            Event::Materials(m) => self.res.materials = m,
            Event::Panorama(None) => self.res.panorama = None,
            Event::Panorama(Some(p)) => {
                // Made an environment off the UI thread, as an image of a file is.
                let doc = self.doc_key;
                let name = self.graph().node(p.node).map_or_else(|| "panorama".to_string(), |n| format!("{} (the graph's panorama)", n.text("name")));
                let node = p.node;
                return self.in_background(
                    move || Arc::new(crate::environment::Hdri::new(name, p.width as usize, p.height as usize, p.rgb.to_vec())),
                    move |hdri| Message::Preview(PreviewMessage::PanoramaRead(doc, node, hdri)),
                );
            }
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
            Event::Stopped(why) => {
                log::error!("the engine stopped: {why}");
                #[cfg(any(test, feature = "dev-tools"))]
                if let Some(pilot) = &mut self.dev.pilot {
                    pilot.finding(format!("the engine stopped: {why}"));
                }
                self.set_status(format!("The engine stopped ({why}): save the packages and start the program again"), true);
                self.res.stopped = Some(why);
            }
            Event::GpuErrors(errors) => {
                #[cfg(any(test, feature = "dev-tools"))]
                if let Some(pilot) = &mut self.dev.pilot {
                    for e in &errors {
                        pilot.finding(format!("GPU error: {e}"));
                    }
                }
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

    /// The side columns as wide as they must be, where the window has room (see [`LEFT_MIN`]):
    /// wider when the window gets smaller, or a split is dragged too far.
    pub(crate) fn fit_columns(&mut self) {
        let [(outer, _), (inner, _)] = side_splits(&self.ui.panes);
        let [mut left, mut right] = self.ui.side_splits;
        let width = self.ui.window_size.width;
        if width >= LEFT_MIN + RIGHT_MIN + MIDDLE_MIN {
            // Each side column as wide as it must be, and no wider than leaves the graph and the
            // views their room (a column dragged across the window squeezed them, and the other
            // column with them).
            // (Bounds that cannot cross: at the narrowest the two meet, a rounding apart, and a
            // clamp between them would panic.)
            let between = |v: f32, lo: f32, hi: f32| v.min(hi.max(lo)).max(lo);
            left = between(left, LEFT_MIN / width, 1.0 - (MIDDLE_MIN + RIGHT_MIN) / width);
            let rest = width * (1.0 - left);
            right = between(right, MIDDLE_MIN / rest, 1.0 - RIGHT_MIN / rest);
        }
        self.ui.panes.resize(outer, left);
        self.ui.panes.resize(inner, right);
    }

    /// The window is to close, as its button or the system says: a pick in progress ends first;
    /// each quit asks anew about every package with unsaved changes (one given up earlier may
    /// have changed since).
    fn close_requested(&mut self) -> Task<Message> {
        let cancel = self.pick_message(PickMessage::Cancel);
        self.ui.discarded.clear();
        Task::batch([cancel, self.guarded(PendingAction::Quit)])
    }

    /// What the window's own title bar does.
    fn window_action(&mut self, action: WindowAction) -> Task<Message> {
        if let WindowAction::Close = action {
            return self.close_requested();
        }
        let (Some(id), Some(request)) = (self.window, self.window_request(action)) else { return Task::none() };
        match request {
            WindowRequest::Minimize => window::minimize(id, true),
            WindowRequest::ToggleMaximize => window::toggle_maximize(id),
            WindowRequest::Drag => window::drag(id),
            WindowRequest::SystemMenu => window::show_system_menu(id),
            WindowRequest::Resize(direction) => window::drag_resize(id, direction),
        }
    }

    /// What the window is asked to do by its own title bar, and what the bar keeps of it: the bar
    /// pressed moves the window, pressed again at once it fills the screen (or gives the window
    /// back its size).
    pub(crate) fn window_request(&mut self, action: WindowAction) -> Option<WindowRequest> {
        // Only a press of the bar right after one is a double click.
        let pressed = std::mem::take(&mut self.ui.title_pressed);
        match action {
            WindowAction::Minimize => Some(WindowRequest::Minimize),
            WindowAction::ToggleMaximize => Some(WindowRequest::ToggleMaximize),
            WindowAction::Close => None,
            WindowAction::TitlePressed if pressed.is_some_and(|at| at.elapsed() < home::DOUBLE_CLICK) => Some(WindowRequest::ToggleMaximize),
            WindowAction::TitlePressed => {
                self.ui.title_pressed = Some(Instant::now());
                Some(WindowRequest::Drag)
            }
            WindowAction::TitleMenu => Some(WindowRequest::SystemMenu),
            WindowAction::Resize(direction) => Some(WindowRequest::Resize(direction)),
            WindowAction::Maximized(on) => {
                // (What the system says comes between the presses of a double click too.)
                self.ui.title_pressed = pressed;
                self.ui.maximized = on;
                None
            }
        }
    }

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
            E::Window(window::Event::CloseRequested) => return self.close_requested(),
            E::Window(window::Event::Opened { size, .. } | window::Event::Resized(size)) => {
                self.ui.window_size = size;
                self.fit_columns();
                // Whether it fills the screen now (the system, or a key, may have made it).
                if OWN_TITLE_BAR {
                    return window::is_maximized(id).map(|m| Message::Window(WindowAction::Maximized(m)));
                }
            }
            E::Window(window::Event::FileDropped(path)) => return self.file_dropped(path),
            // Come back to: what was put in the library folders meanwhile shows.
            E::Window(window::Event::Focused) => return self.window_come_back(),
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

    /// A file dropped on the window: a package is opened in the Explorer, an image becomes a
    /// Bitmap node, vector graphics and fonts are linked to the package.
    fn file_dropped(&mut self, path: PathBuf) -> Task<Message> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
        if ext == project::FILE_EXTENSION {
            self.close_popups();
            return self.open_file(path, false);
        }
        // (The Home screen opens packages, and takes nothing else.)
        if self.ui.home.is_some() {
            self.set_status(format!("Drop a package here: '{}' is not one", path.display()), true);
            return Task::none();
        }
        let path = packages::clean(&path);
        // An image, vector graphics or a font: the node that reads it (and the package lists it).
        if let Some(kind) = ResourceKind::of_path(&path) {
            self.close_popups();
            // Several files dropped at once arrive one by one: stack their nodes.
            let burst = self.ui.drop_burst.filter(|(at, _)| at.elapsed() < Duration::from_millis(800)).map_or(0, |(_, n)| n);
            self.ui.drop_burst = Some((Instant::now(), burst + 1));
            let [x, y] = self.graph_pos_for_paste();
            let pos = [x, y + burst as f32 * 200.0];
            return match self.settings.new_files {
                settings::NewFiles::Link => {
                    self.add_resource_node(path, kind, Some(pos));
                    Task::none()
                }
                settings::NewFiles::Embed => {
                    let here = self.here();
                    packages::read_held(path, move |held| Message::Asked(here, Box::new(Message::DropHeld(kind, pos, held))))
                }
            };
        }
        self.set_status(format!("Cannot use '{path}': drop a package, an image, vector graphics or a font"), true);
        Task::none()
    }

    /// A key pressed: Escape closes what is open; a command's key does what it does (see
    /// [`crate::shortcuts`]: the keys are the settings', the same on every keyboard layout).
    fn shortcut(&mut self, key: keyboard::Key, physical: keyboard::key::Physical, m: keyboard::Modifiers, status: iced::event::Status) -> Task<Message> {
        use keyboard::Key;
        use keyboard::key::Named;
        // Escape closes whatever is open, even when a text field has focus.
        if key == Key::Named(Named::Escape) {
            if self.ui.screen_pick.is_some() {
                return self.pick_message(PickMessage::Cancel);
            }
            if self.ui.add_menu.is_some() || self.ui.menu.is_some() || self.ui.context.is_some() || self.ui.choice_menu.is_some() {
                self.close_popups();
            } else if self.capturing_key() {
                return self.settings_message(SettingsMessage::StopCapture);
            } else if self.ui.dialog.is_some() {
                // (A field of the dialog that took it, a number being typed, stops being typed
                // in: the dialog stays, with what was changed in it.)
                if status != iced::event::Status::Captured {
                    self.cancel_dialog();
                }
            } else if self.ui.renaming_var.is_some()
                || self.ui.binding.is_some()
                || self.ui.renaming_graph.is_some()
                || self.ui.renaming_file.is_some()
            {
                // Typing a name: forget the text, the name stays as it was.
                self.ui.renaming_var = None;
                self.ui.binding = None;
                self.ui.renaming_graph = None;
                self.ui.renaming_file = None;
            } else if self.ui.color_picker.is_some() {
                self.ui.color_picker = None;
            } else if self.ui.home.is_some() && !self.blank() {
                // Back to what is being edited.
                self.ui.home = None;
            }
            return Task::none();
        }
        // Nothing else while picking from the screen (the keyboard may still reach the
        // window).
        if self.ui.screen_pick.is_some() {
            return Task::none();
        }
        // The settings dialog waiting for a key takes the next one (not a modifier alone).
        if self.capturing_key() {
            if let Some(chord) = Chord::of(&key, physical, m) {
                self.capture_key(chord);
            }
            return Task::none();
        }
        if let Some(menu) = &self.ui.choice_menu {
            if key == Key::Named(Named::Enter) {
                // The option the keyboard is on, or else the first the search leaves.
                let (node, key, cursor) = (menu.node, menu.key, menu.cursor);
                let shown = self.choice_menu_options().map(|(shown, _)| shown).unwrap_or_default();
                let option = if shown.contains(&cursor) { Some(cursor) } else { shown.first().copied() };
                return match option {
                    Some(option) => self.update_nested(Message::PickChoice(node, key, option as u32)),
                    None => Task::none(),
                };
            }
            if let Some(task) = self.move_in_choice_menu(&key) {
                return task;
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
        // A dialog takes the keyboard: nothing reaches what is under it.
        if self.ui.dialog.is_some() {
            return Task::none();
        }
        let Some(chord) = Chord::of(&key, physical, m) else { return Task::none() };
        // (Captured: a text field or a widget used the key.)
        let typing = status == iced::event::Status::Captured;
        let over_graph = self.ui.graph_view.contains(self.ui.cursor);
        let over_2d = self.ui.preview.bounds.contains(self.ui.cursor);
        match self.settings.shortcuts.command(chord, typing, over_graph, over_2d) {
            Some(command) => self.command(command),
            None => Task::none(),
        }
    }

    /// Do what a command of the keyboard does.
    fn command(&mut self, command: Command) -> Task<Message> {
        // The Home screen shown: nothing of the editor behind it is done.
        if self.ui.home.is_some() && !matches!(command, Command::New | Command::Open | Command::Settings | Command::Help) {
            return Task::none();
        }
        let message = match command {
            Command::New => Message::NewGraph(home::NewGraphMessage::Open(None)),
            Command::Open => Message::Open,
            Command::Save => Message::Save,
            Command::SaveAs => Message::SaveAs,
            Command::Export => Message::Export(export_dialog::ExportMessage::Open),
            Command::Settings => Message::Settings(SettingsMessage::Open),
            Command::Undo => Message::Undo,
            Command::Redo => Message::Redo,
            Command::Copy => Message::Copy,
            Command::Cut => Message::Cut,
            Command::Paste => Message::Paste,
            Command::Duplicate => Message::Duplicate,
            Command::Delete => Message::Delete,
            Command::SelectAll => Message::SelectAll,
            Command::Bypass => match self.ui.selection.iter().next() {
                Some(id) => Message::ToggleBypass(*id),
                None => return Task::none(),
            },
            Command::GraphFromSelection => Message::GraphFromSelection,
            Command::Frame => Message::Frame,
            Command::AddComment => Message::AddComment,
            Command::AddNode => {
                let at = self.ui.cursor;
                let pos = self.ui.graph_view.to_graph(at);
                return self.open_add_menu(at, pos, None);
            }
            // Over the 2D view, the tiling: it is what one keeps looking at.
            Command::ShowTiling => Message::Preview(PreviewMessage::ToggleTiling),
            Command::Help => Message::Dialog(Some(Dialog::Shortcuts)),
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
                return self.add_payload(payload, [graph_pos[0] - 80.0, graph_pos[1] - 12.0]);
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
            PreviewMessage::LightingMenu(open) => {
                self.close_popups();
                self.ui.preview.lighting.menu = open;
            }
            PreviewMessage::Environment(file) => {
                self.close_popups();
                return self.choose_environment(file);
            }
            PreviewMessage::BrowseEnvironment => {
                // Where the image in use is, to choose another beside it.
                let beside = p.lighting.file.as_deref().and_then(|f| std::path::Path::new(f).parent()).filter(|d| d.is_dir()).map(PathBuf::from);
                let folder = beside.or_else(|| self.dialog_folder(None, false));
                self.close_popups();
                return Task::perform(
                    blocking(move || {
                        let mut dialog = rfd::FileDialog::new()
                            .set_title("Load an environment image")
                            .add_filter("HDR images", &["hdr", "exr"])
                            .add_filter("All pictures", &["hdr", "exr", "png", "jpg", "jpeg", "tga", "bmp", "tif", "tiff"]);
                        if let Some(folder) = folder {
                            dialog = dialog.set_directory(folder);
                        }
                        dialog.pick_file()
                    }),
                    |p| Message::Preview(PreviewMessage::EnvironmentPicked(p.flatten())),
                );
            }
            PreviewMessage::EnvironmentPicked(Some(path)) => return self.choose_environment(Some(path.display().to_string())),
            PreviewMessage::EnvironmentPicked(None) => {}
            PreviewMessage::EnvironmentRead(file, result) => self.environment_read(file, result),
            PreviewMessage::LightBy(output) => {
                self.close_popups();
                self.ui.preview.lighting.panorama = output;
                self.send(Request::Panorama(self.panorama_node()));
            }
            PreviewMessage::PanoramaRead(doc, node, Some(hdri)) => {
                // (Of the document and the graph still open.)
                if doc == self.doc_key && self.panorama_node() == Some(node) {
                    self.res.panorama = Some((node, hdri));
                }
            }
            PreviewMessage::PanoramaRead(_, _, None) => {}
            PreviewMessage::Exposure(ev) => {
                if ev.is_finite() {
                    p.lighting.exposure = ev.clamp(-12.0, 12.0);
                }
            }
            PreviewMessage::Background(on) => p.lighting.background = on,
            PreviewMessage::Blur(blur) => {
                if blur.is_finite() {
                    p.lighting.blur = blur.clamp(0.0, 1.0);
                }
            }
        }
        Task::none()
    }

    /// Light the 3D view by the environment image in `file` (or by the studio): at once when
    /// it is the one read last, else once it is read, on a thread of its own; until then the
    /// view shows what it did.
    fn choose_environment(&mut self, file: Option<String>) -> Task<Message> {
        let l = &mut self.ui.preview.lighting;
        // Chosen over the graph's panorama.
        l.panorama = None;
        l.file = file.clone();
        let Some(file) = file else {
            l.shown = None;
            return Task::none();
        };
        l.remember(&file);
        if let Some(kept) = l.kept.as_ref().filter(|k| k.path == file) {
            l.shown = Some(kept.clone());
            return Task::none();
        }
        if l.reading.as_deref() == Some(file.as_str()) {
            return Task::none();
        }
        l.reading = Some(file.clone());
        let path = PathBuf::from(&file);
        Task::perform(blocking(move || crate::environment::load(&path)), move |read| {
            let read = read.unwrap_or_else(|| Err("The file could not be read".to_string())).map(Arc::new);
            Message::Preview(PreviewMessage::EnvironmentRead(file.clone(), read))
        })
    }

    /// An environment image was read: it lights the view if it is still the one chosen. One
    /// that cannot be read is said so, and forgotten.
    fn environment_read(&mut self, file: String, read: Result<Arc<crate::environment::Hdri>, String>) {
        let l = &mut self.ui.preview.lighting;
        if l.reading.as_deref() == Some(file.as_str()) {
            l.reading = None;
        }
        match read {
            Ok(hdri) => {
                if l.file.as_deref() == Some(file.as_str()) {
                    l.shown = Some(hdri.clone());
                }
                l.kept = Some(hdri);
            }
            Err(e) => {
                l.recent.retain(|f| *f != file);
                if l.file.as_deref() == Some(file.as_str()) {
                    // Back to what the view shows.
                    l.file = l.shown.as_ref().map(|h| h.path.clone());
                }
                self.set_status(format!("The environment image cannot be used: {e}"), true);
            }
        }
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

    pub fn view(&self, window: window::Id) -> iced::Element<'_, Message> {
        if Some(window) == self.window { view::root(self) } else { view::picker::view(self, window) }
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
