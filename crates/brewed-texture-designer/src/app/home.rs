//! The Home screen, as Adobe's programs have one (the crate `home-screen`, the same in every
//! program of the set: this program gives it what it has and does what is chosen there): the
//! program starts on it when it is not given packages to open, and File → Home Screen, or the
//! logo in the menu bar, comes back to it. There: New… (from a template), Open…, the templates
//! to start from at once, the packages opened last with their pictures (see
//! `project::save_with_picture`) and a package dropped on the window opens; Learn opens the
//! examples. The editor is behind it all the while, with what it has.
//!
//! And the dialog of a new package, or of a new graph of one (File → New Package…, New
//! Graph…): its name, its template (see [`Template`]), the size and precision of its images.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use brewed_texture_core::project::{self, ViewState};
use brewed_texture_core::templates::{Example, Template};
use brewed_texture_core::{NodeKind, PackageEdit, Precision};
use brewed_texture_core::package::Package;
use home_screen::{Event, Glyph, Layout, Lesson, Resource, Sort, Start};
use iced::Task;
use iced::widget::image;

use super::{App, Dialog, Message, SettingsMessage};

/// A page of the Home screen.
pub type HomePage = home_screen::Page;

/// A package opened last, as the Home screen shows it.
pub type Recent = home_screen::Recent;

#[derive(Clone, Debug)]
pub enum HomeMessage {
    Show(HomePage),
    /// The packages opened last, looked at (their files, and their pictures).
    Looked(Vec<Recent>),
    /// Open a package opened last.
    Open(String),
    /// Open an example (Learn).
    Example(Example),
    /// What was chosen on the Home screen.
    Screen(Event),
}

/// What the Home screen shows that stays as it is: the templates to start from, the lessons,
/// the resources.
pub struct HomeLists {
    pub start: Vec<Start>,
    pub lessons: Vec<Lesson>,
    pub resources: Vec<Resource>,
}

/// The shortcuts, a lesson and a resource.
const SHORTCUTS: &str = "shortcuts";

impl HomeLists {
    pub fn new() -> HomeLists {
        let start = Template::ALL
            .into_iter()
            .map(|t| {
                let detail = match t.outputs().len() {
                    0 => "Nothing in it".to_string(),
                    n => format!("{n} outputs for the 3D view"),
                };
                Start {
                    id: t.id().into(),
                    title: t.name().into(),
                    detail,
                    glyph: if t == Template::Empty { Glyph::Graph } else { Glyph::Material },
                }
            })
            .collect();
        let mut lessons: Vec<Lesson> = Example::ALL
            .into_iter()
            .map(|e| Lesson {
                id: e.id().into(),
                title: e.name().into(),
                about: e.about().into(),
                picture: None,
                glyph: match e {
                    Example::BrickWall => Glyph::Material,
                    Example::FxMaps => Glyph::Pattern,
                    Example::PixelMath => Glyph::Graph,
                },
            })
            .collect();
        lessons.push(Lesson {
            id: SHORTCUTS.into(),
            title: "Shortcuts and the mouse".into(),
            about: "What every key does (each can be given another in the settings), and the mouse in the graph and the views.".into(),
            picture: None,
            glyph: Glyph::Keyboard,
        });
        let resource = |id: &str, label: &str, glyph: Glyph, available: bool| Resource { id: id.into(), label: label.into(), glyph, available };
        let resources = vec![
            resource(SHORTCUTS, "Shortcuts", Glyph::Keyboard, true),
            resource("settings", "Settings", Glyph::Settings, true),
            resource("about", &format!("About {}", brewed_texture_core::APP_NAME), Glyph::Info, true),
            resource("website", "Website", Glyph::Globe, false),
            resource("documentation", "Documentation", Glyph::Book, false),
            resource("forum", "Forum", Glyph::Chat, false),
            resource("community", "Community Resources", Glyph::People, false),
        ];
        HomeLists { start, lessons, resources }
    }
}

/// How the packages opened last are shown, as `workspace.json` keeps it.
pub fn layout_said(layout: Layout) -> &'static str {
    match layout {
        Layout::Grid => "grid",
        Layout::List => "list",
    }
}

pub fn layout_of(said: Option<&str>) -> Layout {
    if said == Some("list") { Layout::List } else { Layout::Grid }
}

/// How the list is sorted, as `workspace.json` keeps it: the column, and `-desc` for the other
/// way.
pub fn sort_said(sort: Sort) -> String {
    let column = match sort.by {
        home_screen::Column::Name => "name",
        home_screen::Column::Opened => "opened",
        home_screen::Column::Modified => "modified",
        home_screen::Column::Size => "size",
        home_screen::Column::Location => "location",
    };
    format!("{column}{}", if sort.descending { "-desc" } else { "" })
}

pub fn sort_of(said: Option<&str>) -> Sort {
    let Some(said) = said else { return Sort::default() };
    let (column, descending) = match said.strip_suffix("-desc") {
        Some(column) => (column, true),
        None => (said, false),
    };
    let by = match column {
        "name" => home_screen::Column::Name,
        "modified" => home_screen::Column::Modified,
        "size" => home_screen::Column::Size,
        "location" => home_screen::Column::Location,
        "opened" => home_screen::Column::Opened,
        _ => return Sort::default(),
    };
    Sort { by, descending }
}

/// The packages at `paths`: whether they are there, when they were opened last and written,
/// how big, and their pictures.
fn look(paths: Vec<(String, Option<u64>)>) -> Vec<Recent> {
    paths
        .into_iter()
        .map(|(path, opened)| {
            let p = Path::new(&path);
            let meta = std::fs::metadata(p).ok().filter(|m| m.is_file());
            let picture = meta.as_ref().and_then(|_| project::read_picture(p)).and_then(|png| decode(&png));
            Recent {
                name: p.file_stem().map_or_else(|| path.clone(), |s| s.to_string_lossy().into_owned()),
                folder: p.parent().map(|f| f.display().to_string()).unwrap_or_default(),
                opened: opened.map(|s| SystemTime::UNIX_EPOCH + Duration::from_secs(s)),
                modified: meta.as_ref().and_then(|m| m.modified().ok()),
                size: meta.as_ref().map(|m| m.len()),
                there: meta.is_some(),
                picture,
                path,
            }
        })
        .collect()
}

/// `path` opened or saved now: first of the packages opened last.
pub(crate) fn remember(recent: &mut Vec<String>, opened: &mut BTreeMap<String, u64>, path: &str) {
    crate::workspace::remember(recent, path);
    opened.insert(path.to_string(), brewed_texture_core::recovery::now());
    opened.retain(|p, _| recent.iter().any(|r| r == p));
}

impl App {
    pub(crate) fn home_message(&mut self, message: HomeMessage) -> Task<Message> {
        match message {
            HomeMessage::Show(page) => {
                self.close_popups();
                self.ui.home = Some(page);
                // (As many as File → Open Recent Package offers.)
                let offered = self.settings.recent_packages as usize;
                let paths: Vec<(String, Option<u64>)> =
                    self.ui.recent.iter().take(offered).map(|p| (p.clone(), self.ui.recent_opened.get(p).copied())).collect();
                return self.in_background(move || look(paths), |recent| Message::Home(HomeMessage::Looked(recent.unwrap_or_default())));
            }
            HomeMessage::Looked(recent) => self.ui.home_recent = recent,
            HomeMessage::Open(path) => {
                self.ui.home = None;
                let replace = self.blank();
                return self.open_file(std::path::PathBuf::from(path), replace);
            }
            HomeMessage::Example(example) => {
                self.ui.home = None;
                let (package, preview) = example.package();
                let open = package.first();
                let views = [(open, ViewState { preview, ..ViewState::default() })].into_iter().collect();
                self.take_package(package, views, open);
            }
            HomeMessage::Screen(event) => return self.home_event(event),
        }
        Task::none()
    }

    /// What was chosen on the Home screen.
    fn home_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Page(page) => self.ui.home = Some(page),
            Event::Layout(layout) => self.ui.home_layout = layout,
            Event::Sort(column) => self.ui.home_sort = self.ui.home_sort.by(column),
            Event::New | Event::More => return self.new_graph_message(NewGraphMessage::Open(None)),
            Event::Open => return self.handle(Message::Open),
            Event::Back => self.ui.home = None,
            Event::Recent(path) => return self.home_message(HomeMessage::Open(path)),
            Event::Start(id) => {
                if let Some(template) = Template::from_id(&id) {
                    self.ui.last_template = template;
                    let dialog = NewGraphDialog {
                        into: None,
                        name: "Graph".into(),
                        template,
                        size: self.settings.graph_size,
                        precision: self.settings.graph_precision,
                    };
                    self.create_graph(dialog);
                }
            }
            Event::Lesson(id) if id == SHORTCUTS => self.ui.dialog = Some(Dialog::Shortcuts),
            Event::Lesson(id) => {
                if let Some(example) = Example::from_id(&id) {
                    return self.home_message(HomeMessage::Example(example));
                }
            }
            Event::Resource(id) => match id.as_str() {
                SHORTCUTS => self.ui.dialog = Some(Dialog::Shortcuts),
                "settings" => return self.handle(Message::Settings(SettingsMessage::Open)),
                "about" => self.ui.dialog = Some(Dialog::About),
                _ => {}
            },
        }
        Task::none()
    }

    pub(crate) fn new_graph_message(&mut self, message: NewGraphMessage) -> Task<Message> {
        if let NewGraphMessage::Open(into) = message {
            self.close_popups();
            let dialog = NewGraphDialog {
                into,
                name: "Graph".into(),
                template: self.ui.last_template,
                size: self.settings.graph_size,
                precision: self.settings.graph_precision,
            };
            self.ui.dialog = Some(Dialog::NewGraph(dialog));
            return iced::widget::operation::focus(crate::view::dialogs::new_graph_name_id());
        }
        let Some(Dialog::NewGraph(dialog)) = &mut self.ui.dialog else { return Task::none() };
        match message {
            NewGraphMessage::Open(_) => {}
            NewGraphMessage::Name(name) => dialog.name = name,
            NewGraphMessage::Template(template) => dialog.template = template,
            NewGraphMessage::Size(size) => dialog.size = size.clamp(16, 8192),
            NewGraphMessage::Precision(precision) => dialog.precision = precision,
            NewGraphMessage::Create => {
                let dialog = dialog.clone();
                self.ui.dialog = None;
                self.ui.last_template = dialog.template;
                self.create_graph(dialog);
            }
        }
        Task::none()
    }

    /// The graph `dialog` says: in a package of its own, or in the package it names.
    fn create_graph(&mut self, dialog: NewGraphDialog) {
        let name = if dialog.name.trim().is_empty() { "Graph".to_string() } else { dialog.name.trim().to_string() };
        let graph = dialog.template.graph(dialog.size, dialog.precision);
        // Shown in the 2D view: its first map.
        let preview = graph.nodes_of(NodeKind::OUTPUT).next().map(|n| n.id);
        match dialog.into {
            None => {
                self.ui.home = None;
                let package = Package::new(&name, graph);
                let open = package.first();
                let views = [(open, ViewState { preview, ..ViewState::default() })].into_iter().collect();
                self.take_package(package, views, open);
            }
            Some(key) => {
                let Some(doc) = self.package_doc_mut(key) else { return };
                match doc.apply_package(PackageEdit::AddGraph { name, graph }) {
                    Ok(result) => {
                        if let Some(graph) = result.graph {
                            self.views.insert((key, graph), ViewState { preview, ..ViewState::default() });
                            self.activate(key, Some(graph));
                        }
                    }
                    Err(e) => self.set_status(e, true),
                }
            }
        }
    }

    /// Whether the package being edited is one to give way to what is made or opened: the only
    /// one, never saved and unchanged, with nothing in its graphs (as when the program starts
    /// on the Home screen).
    pub(crate) fn blank(&self) -> bool {
        self.shelf.is_empty()
            && self.doc.path().is_none()
            && !self.doc.is_dirty()
            && self.doc.package().graphs.iter().all(|g| g.graph.nodes.is_empty() && g.graph.comments.is_empty())
    }

    /// `package` in the Explorer, being edited: in place of a blank one.
    fn take_package(&mut self, package: Package, views: HashMap<brewed_texture_core::GraphId, ViewState>, open: brewed_texture_core::GraphId) {
        if self.blank() {
            self.replace_package(package, None, views, open);
        } else {
            self.add_package(package, None, views, open);
        }
    }

    /// The picture of the package open as `key`, for its file at `path`: for the package being
    /// edited, the model as the 3D view shows it wearing the package's maps (else the thumbnail
    /// of the first Output of its graph, or of the node the 2D view shows), as a PNG; else the
    /// one its file had.
    pub(crate) fn picture_for(&mut self, key: u64, path: &str) -> Option<Arc<Vec<u8>>> {
        if key == self.doc_key {
            let model = self.gpu.clone().and_then(|gpu| {
                let mut pipeline = self.picture_pipeline.take();
                let picture = crate::view::view3d::picture(self, &mut pipeline, &gpu, PICTURE);
                self.picture_pipeline = pipeline;
                png(&picture?)
            });
            let graph = self.graph();
            let first = brewed_texture_core::instance::port_nodes(graph).first().map(|n| n.id);
            let shown = first.or(self.ui.preview.node);
            let fresh = model.or_else(|| shown.and_then(|id| self.res.thumbs.get(&id)).and_then(encode));
            if let Some(png) = fresh {
                let png = Arc::new(png);
                self.pictures.insert(path.to_string(), png.clone());
                return Some(png);
            }
        }
        let before = self.package_doc(key).and_then(|d| d.key()).and_then(|file| self.pictures.get(&file).cloned());
        let picture = before.or_else(|| self.pictures.get(path).cloned());
        if let Some(png) = &picture {
            self.pictures.insert(path.to_string(), png.clone());
        }
        picture
    }
}

/// The dialog of a new package, or of a new graph of an open one.
#[derive(Clone, Debug, PartialEq)]
pub struct NewGraphDialog {
    /// The package the graph goes into (its key in the Explorer); none: a package of its own.
    pub into: Option<u64>,
    /// The name of the graph (and of the package, until it is saved).
    pub name: String,
    pub template: Template,
    pub size: u32,
    pub precision: Precision,
}

#[derive(Clone, Debug)]
pub enum NewGraphMessage {
    /// Open the dialog: for a package of its own, or for a graph of the package open as the key.
    Open(Option<u64>),
    Name(String),
    Template(Template),
    Size(u32),
    Precision(Precision),
    Create,
}

/// A PNG as a picture to show (decoded here: iced takes pictures as pixels).
fn decode(png: &[u8]) -> Option<image::Handle> {
    let picture = ::image::load_from_memory_with_format(png, ::image::ImageFormat::Png).ok()?.to_rgba8();
    Some(image::Handle::from_rgba(picture.width(), picture.height(), picture.into_raw()))
}

/// How big the picture of a package is, in pixels (the Home screen shows it at 176 points,
/// and on a screen of twice the pixels).
const PICTURE: u32 = 256;

/// A thumbnail as a PNG.
fn encode(handle: &image::Handle) -> Option<Vec<u8>> {
    let image::Handle::Rgba { width, height, pixels, .. } = handle else { return None };
    png(&::image::RgbaImage::from_raw(*width, *height, pixels.to_vec())?)
}

fn png(picture: &::image::RgbaImage) -> Option<Vec<u8>> {
    let mut png = Vec::new();
    picture.write_to(&mut std::io::Cursor::new(&mut png), ::image::ImageFormat::Png).ok()?;
    Some(png)
}
