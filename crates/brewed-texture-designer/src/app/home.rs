//! The Home screen, as Adobe's programs have one: the program starts on it when it is not given
//! packages to open, and File → Home Screen, or the logo in the menu bar, comes back to it.
//! There: New Package… (from a template), Open…, the packages opened last with their pictures
//! (see `project::save_with_picture`), and a package dropped on the window opens; Learn opens
//! the examples. The editor is behind it all the while, with what it has.
//!
//! And the dialog of a new package, or of a new graph of one (File → New Package…, New
//! Graph…): its name, its template (see [`Template`]), the size and precision of its images.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use brewed_texture_core::project::{self, ViewState};
use brewed_texture_core::templates::{Example, Template};
use brewed_texture_core::{NodeKind, PackageEdit, Precision};
use brewed_texture_core::package::Package;
use iced::Task;
use iced::widget::image;

use super::{App, Dialog, Message};

/// A page of the Home screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HomePage {
    Home,
    Learn,
}

/// A package opened last, as the Home screen shows it.
#[derive(Clone, Debug)]
pub struct Recent {
    pub path: String,
    pub name: String,
    /// The folder it is in.
    pub folder: String,
    pub modified: Option<SystemTime>,
    /// Whether its file is there still.
    pub there: bool,
    /// Its picture, if its file has one.
    pub picture: Option<image::Handle>,
}

#[derive(Clone, Debug)]
pub enum HomeMessage {
    Show(HomePage),
    /// Back to the editor.
    Close,
    /// The packages opened last, looked at (their files, and their pictures).
    Looked(Vec<Recent>),
    /// Open a package opened last.
    Open(String),
    /// Open an example (Learn).
    Example(Example),
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

/// The packages at `paths`: whether they are there, when they were written, and their pictures.
fn look(paths: Vec<String>) -> Vec<Recent> {
    paths
        .into_iter()
        .map(|path| {
            let p = Path::new(&path);
            let meta = std::fs::metadata(p).ok().filter(|m| m.is_file());
            let picture = meta.as_ref().and_then(|_| project::read_picture(p)).and_then(|png| decode(&png));
            Recent {
                name: p.file_stem().map_or_else(|| path.clone(), |s| s.to_string_lossy().into_owned()),
                folder: p.parent().map(|f| f.display().to_string()).unwrap_or_default(),
                modified: meta.as_ref().and_then(|m| m.modified().ok()),
                there: meta.is_some(),
                picture,
                path,
            }
        })
        .collect()
}

impl App {
    pub(crate) fn home_message(&mut self, message: HomeMessage) -> Task<Message> {
        match message {
            HomeMessage::Show(page) => {
                self.close_popups();
                self.ui.home = Some(page);
                // (As many as File → Open Recent offers.)
                let offered = self.settings.recent_packages as usize;
                let paths: Vec<String> = self.ui.recent.iter().take(offered).cloned().collect();
                return self.in_background(move || look(paths), |recent| Message::Home(HomeMessage::Looked(recent.unwrap_or_default())));
            }
            HomeMessage::Close => self.ui.home = None,
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
                        let Some(doc) = self.package_doc_mut(key) else { return Task::none() };
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
        }
        Task::none()
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
    /// edited, the thumbnail of the first Output of its graph (else of the node the 2D view
    /// shows), as a PNG; else the one its file had.
    pub(crate) fn picture_for(&mut self, key: u64, path: &str) -> Option<Arc<Vec<u8>>> {
        if key == self.doc_key {
            let graph = self.graph();
            let first = brewed_texture_core::instance::port_nodes(graph).first().map(|n| n.id);
            let shown = first.or(self.ui.preview.node);
            let fresh = shown.and_then(|id| self.res.thumbs.get(&id)).and_then(encode);
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

/// A PNG as a picture to show (decoded here: iced takes pictures as pixels).
fn decode(png: &[u8]) -> Option<image::Handle> {
    let picture = ::image::load_from_memory_with_format(png, ::image::ImageFormat::Png).ok()?.to_rgba8();
    Some(image::Handle::from_rgba(picture.width(), picture.height(), picture.into_raw()))
}

/// A thumbnail as a PNG.
fn encode(handle: &image::Handle) -> Option<Vec<u8>> {
    let image::Handle::Rgba { width, height, pixels, .. } = handle else { return None };
    let picture = ::image::RgbaImage::from_raw(*width, *height, pixels.to_vec())?;
    let mut png = Vec::new();
    picture.write_to(&mut std::io::Cursor::new(&mut png), ::image::ImageFormat::Png).ok()?;
    Some(png)
}
