//! The Home screen, as Adobe's programs have one (the crate `home-screen`, the same in every
//! program of the set: this program gives it what it has and does what is chosen there): the
//! program starts on it when it is not given packages to open, and File → Home Screen, or the
//! logo in the menu bar, comes back to it. There: New… (from a template), Open…, the templates
//! to start from at once, the packages opened last with their pictures (see
//! `project::save_with_picture`) and a package dropped on the window opens; Learn opens the
//! examples. The editor is behind it all the while, with what it has.
//!
//! And the dialog of a new graph (File → New Graph…, the Explorer's +), as Substance Designer's:
//! the templates on the left, by category (see [`Template`]), as pictures or as a list, each
//! saying what it is on its (i); on the right, the package it goes in (a new one, or one open),
//! its name, the size of its images (wide and high, together or not) and their precision.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use brewed_texture_core::project::{self, ViewState};
use brewed_texture_core::templates::{Category, Example, Template};
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

/// The pictures of the lessons of Learn: the model of each example as the 3D view shows it,
/// made with `--picture assets/lessons/ID.jpg --scenario example:ID` (dev-tools).
const LESSON_PICTURES: [(Example, &[u8]); 3] = [
    (Example::BrickWall, include_bytes!("../../assets/lessons/brick-wall.jpg")),
    (Example::FxMaps, include_bytes!("../../assets/lessons/fx-maps.jpg")),
    (Example::PixelMath, include_bytes!("../../assets/lessons/pixel-math.jpg")),
];

/// What the Home screen shows that stays as it is: the templates to start from, the lessons,
/// the resources.
pub struct HomeLists {
    pub start: Vec<Start>,
    pub lessons: Vec<Lesson>,
    pub resources: Vec<Resource>,
    /// Every template, as the window of a new graph shows it (the same cards as the row).
    pub templates: Vec<(Template, Start)>,
}

/// The pictures of the templates: what each makes, rendered (made with `--picture`,
/// `--export` and `--compose`, see the README).
const TEMPLATE_PICTURES: [(Template, &[u8]); 14] = [
    (Template::Pbr, include_bytes!("../../assets/templates/pbr.jpg")),
    (Template::Anisotropy, include_bytes!("../../assets/templates/openpbr-anisotropy.jpg")),
    (Template::Coat, include_bytes!("../../assets/templates/openpbr-coat.jpg")),
    (Template::Subsurface, include_bytes!("../../assets/templates/openpbr-subsurface.jpg")),
    (Template::Fuzz, include_bytes!("../../assets/templates/openpbr-fuzz.jpg")),
    (Template::Complete, include_bytes!("../../assets/templates/openpbr-complete.jpg")),
    (Template::StudioPanorama, include_bytes!("../../assets/templates/studio-panorama.jpg")),
    (Template::Convolution, include_bytes!("../../assets/templates/convolution.jpg")),
    (Template::Grid, include_bytes!("../../assets/templates/fx-map-grid.jpg")),
    (Template::Noise, include_bytes!("../../assets/templates/fx-map-noise.jpg")),
    (Template::ScanProcessing, include_bytes!("../../assets/templates/scan-processing.jpg")),
    (Template::GrayFilter, include_bytes!("../../assets/templates/gray-filter.jpg")),
    (Template::ColorFilter, include_bytes!("../../assets/templates/color-filter.jpg")),
    (Template::Empty, include_bytes!("../../assets/templates/empty.jpg")),
];

/// The card of `template`, the same on the Home screen and in the window of a new graph: its
/// picture, name and category, and what its (i) says.
fn template_start(template: Template) -> Start {
    let glyph = match template.category() {
        Category::OpenPbr => Glyph::Material,
        Category::Hdri => Glyph::Light,
        Category::PixelProcessor => Glyph::Grid,
        Category::FxMap => Glyph::Pattern,
        Category::ScanProcessing => Glyph::Camera,
        Category::Filters => Glyph::Filter,
        Category::Others => Glyph::Graph,
    };
    Start {
        id: template.id().into(),
        title: template.name().into(),
        detail: template.category().name().into(),
        glyph,
        picture: TEMPLATE_PICTURES.iter().find(|(of, _)| *of == template).map(|(_, bytes)| image::Handle::from_bytes(*bytes)),
        kind: template.kind().into(),
        about: template.about().into(),
        outputs: template.outputs().into_iter().map(String::from).collect(),
    }
}

/// The shortcuts, a lesson and a resource.
const SHORTCUTS: &str = "shortcuts";

impl HomeLists {
    pub fn new() -> HomeLists {
        let templates: Vec<(Template, Start)> = Template::ALL.into_iter().map(|t| (t, template_start(t))).collect();
        let start = Template::STARTERS.iter().filter_map(|t| templates.iter().find(|(of, _)| of == t).map(|(_, s)| s.clone())).collect();
        let mut lessons: Vec<Lesson> = Example::ALL
            .into_iter()
            .map(|e| Lesson {
                id: e.id().into(),
                title: e.name().into(),
                about: e.about().into(),
                picture: LESSON_PICTURES.iter().find(|(of, _)| *of == e).map(|(_, bytes)| image::Handle::from_bytes(*bytes)),
                glyph: match e {
                    Example::BrickWall => Glyph::Material,
                    Example::FxMaps => Glyph::Pattern,
                    Example::PixelMath => Glyph::Graph,
                },
            })
            .collect();
        lessons.push(Lesson {
            id: SHORTCUTS.into(),
            title: "Shortcuts".into(),
            about: "What every key does (each can be given another in the settings), and the mouse in the graph and the views.".into(),
            picture: None,
            glyph: Glyph::Keyboard,
        });
        let resource = |id: &str, label: &str, glyph: Glyph, available: bool| Resource { id: id.into(), label: label.into(), glyph, available };
        let resources = vec![
            resource(SHORTCUTS, "Shortcuts", Glyph::Keyboard, true),
            resource("settings", "Settings", Glyph::Settings, true),
            resource("notices", "Third-Party Notices", Glyph::File, true),
            resource("about", &format!("About {}", brewed_texture_core::APP_NAME), Glyph::Info, true),
            resource("website", "Website", Glyph::Globe, false),
            resource("documentation", "Documentation", Glyph::Book, false),
            resource("forum", "Forum", Glyph::Chat, false),
            resource("community", "Community Resources", Glyph::People, false),
        ];
        HomeLists { start, lessons, resources, templates }
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
                    let dialog = NewGraphDialog::new(None, template, self.settings.graph_size, self.settings.graph_precision);
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
                "notices" => self.ui.dialog = Some(Dialog::Notices(Default::default())),
                "about" => self.ui.dialog = Some(Dialog::About),
                _ => {}
            },
        }
        Task::none()
    }

    pub(crate) fn new_graph_message(&mut self, message: NewGraphMessage) -> Task<Message> {
        if let NewGraphMessage::Open(into) = message {
            self.close_popups();
            let dialog = NewGraphDialog::new(into, self.ui.last_template, self.settings.graph_size, self.settings.graph_precision);
            self.ui.dialog = Some(Dialog::NewGraph(dialog));
            return iced::widget::operation::focus(crate::view::dialogs::new_graph_name_id());
        }
        let Some(Dialog::NewGraph(dialog)) = &mut self.ui.dialog else { return Task::none() };
        match message {
            NewGraphMessage::Open(_) => {}
            NewGraphMessage::Name(name) => dialog.name = name,
            NewGraphMessage::Template(template) => dialog.choose(template),
            NewGraphMessage::Into(into) => dialog.into = into,
            NewGraphMessage::Width(width) => {
                // What is set by hand stays, whatever template is chosen after.
                dialog.before = None;
                dialog.size[0] = width.clamp(16, 8192);
                if dialog.linked {
                    dialog.size[1] = dialog.size[0];
                }
            }
            NewGraphMessage::Height(height) => {
                dialog.before = None;
                dialog.size[1] = height.clamp(16, 8192);
                if dialog.linked {
                    dialog.size[0] = dialog.size[1];
                }
            }
            NewGraphMessage::Link(linked) => {
                dialog.before = None;
                dialog.linked = linked;
                if linked {
                    dialog.size[1] = dialog.size[0];
                }
            }
            NewGraphMessage::Precision(precision) => {
                dialog.before = None;
                dialog.precision = precision;
            }
            NewGraphMessage::Layout(layout) => self.ui.template_layout = layout,
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
                let picture = crate::view::view3d::picture(self, &mut pipeline, &gpu, [PICTURE, PICTURE], false);
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

/// The dialog of a new graph.
#[derive(Clone, Debug, PartialEq)]
pub struct NewGraphDialog {
    /// The package the graph goes into (its key in the Explorer); none: a package of its own.
    pub into: Option<u64>,
    /// The name of the graph (and of the package, until it is saved).
    pub name: String,
    pub template: Template,
    /// How wide and how high its images are, and whether the one follows the other.
    pub size: [u32; 2],
    pub linked: bool,
    pub precision: Precision,
    /// What the size, its link and the precision were before the template chosen set them
    /// (a panorama's): they come back when another is chosen.
    pub before: Option<([u32; 2], bool, Precision)>,
}

impl NewGraphDialog {
    /// The dialog of a graph of `template` (into the package open as `into`, or a new one), its
    /// images `size` and `precision` unless the template needs others.
    pub fn new(into: Option<u64>, template: Template, size: [u32; 2], precision: Precision) -> NewGraphDialog {
        let mut dialog = NewGraphDialog { into, name: "Graph".into(), template, size, linked: size[0] == size[1], precision, before: None };
        dialog.choose(template);
        dialog
    }

    /// Choose `template`: what it needs of the size and the precision is set (a panorama is
    /// twice as wide as high, in 32-bit floats), and what was there before comes back with
    /// another.
    pub fn choose(&mut self, template: Template) {
        if let Some((size, linked, precision)) = self.before.take() {
            (self.size, self.linked, self.precision) = (size, linked, precision);
        }
        let fitted = template.fit(self.size, self.precision);
        if fitted != (self.size, self.precision) {
            self.before = Some((self.size, self.linked, self.precision));
            (self.size, self.precision) = fitted;
            self.linked = fitted.0[0] == fitted.0[1];
        }
        self.template = template;
    }
}

#[derive(Clone, Debug)]
pub enum NewGraphMessage {
    /// Open the dialog: for a package of its own, or for a graph of the package open as the key.
    Open(Option<u64>),
    Name(String),
    Template(Template),
    /// The package the graph goes into: a new one, or the one open as the key.
    Into(Option<u64>),
    Width(u32),
    Height(u32),
    Link(bool),
    Precision(Precision),
    /// The templates as pictures, or as a list.
    Layout(home_screen::Layout),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every example of Learn has its picture, as wide as the cards show it on a screen of three
    /// times the pixels, and 10 high for 16 wide as they are; every template its picture, square,
    /// and its card the same on the Home screen and in the window of a new graph.
    #[test]
    fn every_lesson_and_template_has_its_picture() {
        for example in Example::ALL {
            let (_, bytes) = LESSON_PICTURES.iter().find(|(of, _)| *of == example).expect("a picture of every example");
            let picture = ::image::load_from_memory(bytes).expect("a picture");
            assert_eq!((picture.width(), picture.height()), (512, 320), "{example:?}");
        }
        for template in Template::ALL {
            let (_, bytes) = TEMPLATE_PICTURES.iter().find(|(of, _)| *of == template).expect("a picture of every template");
            let picture = ::image::load_from_memory(bytes).expect("a picture");
            assert_eq!((picture.width(), picture.height()), (384, 384), "{template:?}");
        }
        let lists = HomeLists::new();
        assert_eq!(lists.lessons.iter().filter(|l| l.picture.is_some()).count(), Example::ALL.len());
        assert_eq!(lists.templates.len(), Template::ALL.len());
        for start in &lists.start {
            let (_, card) = lists.templates.iter().find(|(t, _)| t.id() == start.id).expect("a template");
            assert_eq!(card, start, "the same card");
        }
    }
}
