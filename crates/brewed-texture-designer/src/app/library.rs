//! The graphs of the library folders (Settings → Library), as Substance Designer's library
//! paths: every package file in the folders, as deep as they go, offers its graphs in the
//! Library panel, under the categories they say (Graph → Category; none: the name of the
//! package), as nodes to drag onto the graph. A graph says what it makes in its description,
//! and may keep out of the Library (Show in Library off: one the others use).
//!
//! The folders are looked at in the background: when the program starts, when the settings
//! change them, when the window is come back to, after a package is saved into one of them,
//! and when the Library is told to (its ⟳). A package file is read again only where its size
//! or time changed, and only its graphs are read, not the files it holds.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use brewed_texture_core::project::{self, GraphSummary};
use brewed_texture_core::{Document, Edit, GraphId, GraphRef, Instance, Interface};
use iced::Task;

use super::{App, Message};
use crate::app::packages::Opened;

/// How deep into a library folder packages are looked for, and how many entries of folders are
/// looked at in all (a folder of the whole disk is not read to its end).
const DEPTH: usize = 12;
const MAX_ENTRIES: usize = 50_000;
/// The window come back to looks at the folders again, but not more often than this.
const AGAIN_AFTER: std::time::Duration = std::time::Duration::from_secs(2);

/// A graph of a package in a library folder, as the Library shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct LibraryGraph {
    /// The package file.
    pub package: String,
    pub graph: GraphId,
    pub name: String,
    pub description: String,
    /// What it is shown under: its category, else the name of its package.
    pub category: String,
}

impl LibraryGraph {
    /// Whether it is what `query` (lower case) looks for: every word of it in its name, its
    /// description, its category or the name of its package.
    pub fn matches(&self, query: &str) -> bool {
        let said = format!("{} {} {} {}", self.name, self.description, self.category, self.package).to_lowercase();
        query.split_whitespace().all(|word| said.contains(word))
    }
}

/// A package file looked at: its size and time, and its graphs (or why they cannot be read).
#[derive(Clone, Debug)]
pub struct Looked {
    stamp: (u64, Option<SystemTime>),
    graphs: Result<Vec<GraphSummary>, String>,
}

/// What the library folders hold, as they were looked at last.
#[derive(Debug, Default)]
pub struct Library {
    /// The graphs to show, by category, then by name.
    pub graphs: Vec<LibraryGraph>,
    /// What could not be read: said in the panel.
    pub problems: Vec<String>,
    files: Arc<HashMap<String, Looked>>,
    /// Looking now; and whether to look again once it is done (asked meanwhile).
    looking: bool,
    again: bool,
    last: Option<Instant>,
}

impl Library {
    /// Whether the folders are being looked at.
    pub fn looking(&self) -> bool {
        self.looking
    }
}

/// The size and time of the file at `path`.
fn stamp(meta: &std::fs::Metadata) -> (u64, Option<SystemTime>) {
    (meta.len(), meta.modified().ok())
}

/// Every package file in `folders`, as deep as [`DEPTH`], with its graphs: read again where its
/// size or time is not as `known` has it.
pub fn look(folders: &[String], known: &HashMap<String, Looked>) -> HashMap<String, Looked> {
    let mut found: HashMap<String, Looked> = HashMap::new();
    let mut entries = 0;
    let mut stack: Vec<(std::path::PathBuf, usize)> = folders.iter().map(|f| (Path::new(f).to_path_buf(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        for entry in read.flatten() {
            entries += 1;
            if entries > MAX_ENTRIES {
                return found;
            }
            // (Links are not followed: a link to a folder above would be looked into forever.)
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_dir() {
                if depth < DEPTH {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let package = path.extension().is_some_and(|e| e.eq_ignore_ascii_case(project::FILE_EXTENSION));
            if !kind.is_file() || !package {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let file = path.display().to_string();
            let now = stamp(&meta);
            let looked = match known.get(&file) {
                Some(before) if before.stamp == now => before.clone(),
                _ => Looked { stamp: now, graphs: project::read_graphs(&path) },
            };
            found.insert(file, looked);
        }
    }
    found
}

/// The graphs of `files` to show, by category then by name, and what could not be read.
fn shown(files: &HashMap<String, Looked>) -> (Vec<LibraryGraph>, Vec<String>) {
    let mut graphs = Vec::new();
    let mut problems = Vec::new();
    for (file, looked) in files {
        match &looked.graphs {
            Ok(summaries) => {
                let stem = Path::new(file).file_stem().map_or_else(|| file.clone(), |s| s.to_string_lossy().into_owned());
                for g in summaries.iter().filter(|g| !g.attributes.hidden && g.outputs > 0) {
                    let category = g.attributes.category.trim();
                    graphs.push(LibraryGraph {
                        package: file.clone(),
                        graph: g.id,
                        name: g.name.clone(),
                        description: g.attributes.description.clone(),
                        category: if category.is_empty() { stem.clone() } else { category.to_string() },
                    });
                }
            }
            // (Which file, where the error does not say.)
            Err(e) if e.contains(file.as_str()) => problems.push(e.clone()),
            Err(e) => problems.push(format!("{file}: {e}")),
        }
    }
    graphs.sort_by(|a, b| (a.category.to_lowercase(), a.name.to_lowercase(), &a.package).cmp(&(b.category.to_lowercase(), b.name.to_lowercase(), &b.package)));
    problems.sort();
    (graphs, problems)
}

impl App {
    /// Look at the library folders again, in the background (once what is being looked at is
    /// done, if it is being).
    pub(crate) fn look_at_library(&mut self) -> Task<Message> {
        if self.library.looking {
            self.library.again = true;
            return Task::none();
        }
        let folders = self.settings.library_folders.clone();
        if folders.is_empty() {
            self.library = super::library::Library::default();
            return Task::none();
        }
        self.library.looking = true;
        self.library.last = Some(Instant::now());
        let known = self.library.files.clone();
        self.in_background(move || look(&folders, &known), |found| Message::LibraryLooked(found.map(Arc::new)))
    }

    /// The window was come back to: the folders are looked at again (what was put in them
    /// meanwhile shows), but not more often than every [`AGAIN_AFTER`].
    pub(crate) fn window_come_back(&mut self) -> Task<Message> {
        if self.library.last.is_some_and(|t| t.elapsed() < AGAIN_AFTER) {
            return Task::none();
        }
        self.look_at_library()
    }

    /// The library folders were looked at.
    pub(crate) fn library_looked(&mut self, found: Option<Arc<HashMap<String, Looked>>>) -> Task<Message> {
        self.library.looking = false;
        if let Some(files) = found {
            let (graphs, problems) = shown(&files);
            self.library.graphs = graphs;
            self.library.problems = problems;
            self.library.files = files;
        }
        if std::mem::take(&mut self.library.again) {
            return self.look_at_library();
        }
        Task::none()
    }

    /// Whether `path` is in a library folder (a package saved there shows in the Library).
    pub(crate) fn in_library(&self, path: &Path) -> bool {
        self.settings.library_folders.iter().any(|f| path.starts_with(f))
    }

    /// A graph of the Library dropped on the graph at `pos`: a node that uses it. Its package
    /// is read first if nothing has it (in the background, with the packages it uses).
    pub(crate) fn add_library_graph(&mut self, package: String, graph: GraphId, name: String, pos: [f32; 2]) -> Task<Message> {
        // Open in the Explorer (the one being edited too): used from there.
        if let Some(key) = self.open_key(&package) {
            self.add_graph_node(key, graph, pos);
            return Task::none();
        }
        if self.deps.contains_key(&package) {
            self.add_used_graph(&package, graph, pos);
            return Task::none();
        }
        self.set_status(format!("Reading {package}…"), false);
        let known = self.all_docs().filter_map(Document::key).chain(self.deps.keys().cloned()).collect();
        let file = package.clone();
        // (Added where it was dropped, whichever graph is edited when the package is read.)
        let here = self.here();
        self.in_background(move || super::packages::read_with_dependencies(&file, known), move |read| {
            let read = Message::LibraryRead {
                package: package.clone(),
                graph,
                name: name.clone(),
                pos,
                result: read.unwrap_or_else(|| Err("The package could not be read".into())).map(Arc::new),
            };
            Message::Asked(here, Box::new(read))
        })
    }

    /// The package of a graph of the Library was read (see [`App::add_library_graph`]).
    pub(crate) fn library_read(&mut self, package: String, graph: GraphId, name: String, pos: [f32; 2], result: Result<Arc<Opened>, String>) {
        let opened = match result.map(Arc::try_unwrap) {
            Ok(Ok(opened)) => opened,
            Ok(Err(_)) => return,
            Err(e) => {
                self.set_status(e, true);
                return;
            }
        };
        let (loaded, deps) = opened.into_parts();
        self.take_dependencies(deps);
        if self.open_key(&package).is_none() && !self.deps.contains_key(&package) {
            self.deps.insert(package.clone(), loaded.package);
        }
        self.set_status(format!("'{name}' of {package}"), false);
        match self.open_key(&package) {
            Some(key) => self.add_graph_node(key, graph, pos),
            None => self.add_used_graph(&package, graph, pos),
        }
    }

    /// A node that uses graph `graph` of the package file `package`, read already (not open in
    /// the Explorer), at `pos`.
    fn add_used_graph(&mut self, package: &str, graph: GraphId, pos: [f32; 2]) {
        let Some(entry) = self.deps.get(package).and_then(|p| p.graph(graph)) else {
            self.set_status(format!("{package} has no such graph any more"), true);
            return;
        };
        let r = GraphRef { package: Some(package.to_string()), graph, name: entry.name.clone() };
        if let Some(why) = self.use_refusal(&r) {
            self.set_status(why, true);
            return;
        }
        let instance = Instance::new(r, Interface::of(&entry.graph));
        if self.apply(Edit::AddInstance { instance, pos }) {
            self.doc.seal();
        }
    }
}
