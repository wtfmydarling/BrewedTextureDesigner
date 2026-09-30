//! The packages open in the Explorer, the packages whose graphs they use, and graphs used as
//! nodes: which graph is edited, what the engine computes of it (the graph with every graph it
//! uses laid out in it), opening, closing and saving packages, and the files a package cannot
//! find.
//!
//! The package being edited is `App::doc`; the others open in the Explorer wait on
//! `App::shelf`. Packages whose graphs are used but that are not open are read from their files
//! into `App::deps`, as the Explorer of the program this one follows does: they are not shown
//! until a graph of theirs is opened ("Open Reference").

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use iced::{Task, Vector};

use brewed_texture_core::files::Held;
use brewed_texture_core::package::{self, Flat, Library, Missing, MissingKind, Package, PackageEdit, ResourceKind};
use brewed_texture_core::project::{self, ViewState};
use brewed_texture_core::{
    Document, Edit, GraphId, GraphRef, Instance, Interface, NodeId, NodeKind, ParamKind, ParamValue,
};

use super::{App, Collected, ContextMenu, Dialog, DragPayload, ExplorerMessage, FileTarget, Fold, Message, MissingFiles, PendingAction, blocking};

/// A package open in the Explorer, other than the one being edited.
pub struct OpenPackage {
    /// Its place in the Explorer: packages are listed in the order they were opened.
    pub key: u64,
    pub doc: Document,
}

/// The packages the graphs of a flattened graph are found in (see [`package::flatten`]).
pub struct Shelf<'a> {
    /// The package being flattened when it has no file.
    root: Option<&'a Package>,
    /// Packages by the path of their file.
    open: Vec<(String, &'a Package)>,
    deps: &'a HashMap<String, Package>,
}

impl<'a> Shelf<'a> {
    fn new(
        root: Option<&'a Package>,
        docs: impl Iterator<Item = &'a Document>,
        deps: &'a HashMap<String, Package>,
    ) -> Shelf<'a> {
        let open = docs.filter_map(|d| Some((d.key()?, d.package()))).collect();
        Shelf { root, open, deps }
    }
}

impl Library for Shelf<'_> {
    fn package(&self, path: Option<&str>) -> Option<&Package> {
        match path {
            None => self.root,
            Some(p) => self.open.iter().find(|(k, _)| k == p).map(|(_, package)| *package).or_else(|| self.deps.get(p)),
        }
    }
}

/// Read `paths` for the package open as `key`, on a thread of its own: to import them into it,
/// or (`embed`) to hold in it the files it links to.
fn read_files(key: u64, paths: Vec<String>, embed: bool) -> Task<Message> {
    if paths.is_empty() {
        return Task::none();
    }
    Task::perform(
        blocking(move || {
            let mut files = Vec::new();
            let mut failed = Vec::new();
            for path in paths {
                match brewed_texture_core::files::read_disk(&path) {
                    // Hashed here: not on the thread that draws the window.
                    Ok(bytes) => files.push((path.clone(), Held::new(&path, Arc::new(bytes)))),
                    Err(e) => failed.push(format!("{} ({e})", package::file_name(&path))),
                }
            }
            (files, failed)
        }),
        move |read| {
            let (files, failed) = read.unwrap_or_else(|| (Vec::new(), vec!["the files".into()]));
            Message::Explorer(ExplorerMessage::FilesRead { key, embed, files, failed })
        },
    )
}

/// Read the file at `path` to be held by a package, in the background (hashed there too: not
/// on the thread that draws the window), and say what came of it as `then` does.
pub(super) fn read_held(path: String, then: impl FnOnce(Result<Held, String>) -> Message + Send + 'static) -> Task<Message> {
    Task::perform(
        blocking(move || match brewed_texture_core::files::read_disk(&path) {
            Ok(bytes) => Ok(Held::new(&path, Arc::new(bytes))),
            Err(e) => Err(format!("Cannot read {}: {e}", package::file_name(&path))),
        }),
        move |read| then(read.unwrap_or_else(|| Err("The file could not be read".into()))),
    )
}

/// Keep `held` for whatever reads it (see [`brewed_texture_core::files`]): the name it is read
/// by. A node that reads it makes the package hold it.
pub(super) fn hold(held: Held) -> String {
    let (path, bytes, name) = held.into_parts();
    brewed_texture_core::files::keep(&path, &bytes, &name);
    path
}

/// Copy each file of `plan` (where it is, where its copy goes), making the folders; a file is
/// copied only where nothing is (a plan leaves out the names taken). Each copy is whole and on
/// the disk before the package says it is where it is: saved then, the package never names a
/// copy a power cut left half written.
pub fn copy_files(plan: &[(String, String)]) -> Collected {
    let mut out = Collected::default();
    for (from, to) in plan {
        let target = Path::new(to);
        let made = target.parent().map_or(Ok(()), std::fs::create_dir_all).map_err(|e| e.to_string());
        let copied = made.and_then(|()| {
            if target.exists() {
                return Err("there is a file of that name there".to_string());
            }
            brewed_texture_core::files::replace(target, |beside| std::fs::copy(from, beside).map(drop))
        });
        match copied {
            Ok(()) => out.copied.push((from.clone(), to.clone())),
            Err(e) => out.failed.push(format!("{} ({e})", package::file_name(from))),
        }
    }
    out
}

/// A path as the packages say it: absolute, without `.` and `..`.
pub fn clean(path: &Path) -> String {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| path.to_path_buf(), |d| d.join(path))
    };
    package::clean_path(&path).display().to_string()
}

/// A package being saved (see [`App::save_package`]): where, the package as it is written,
/// what was to follow the save, where to save it again once this is done (a save asked for
/// meanwhile), and whether the file is to be opened then (and in place of the package being
/// edited, or not): it was asked for while it was being written.
#[derive(Debug)]
pub struct Saving {
    path: String,
    package: Package,
    then: Option<PendingAction>,
    again: Option<PathBuf>,
    open: Option<bool>,
}

/// The package files the graphs of a package use, each read (or why it could not be).
pub(crate) type Dependencies = Vec<(String, Result<Package, String>)>;

/// A package file read, with the package files its graphs use (see [`App::open_file`]).
#[derive(Debug)]
pub struct Opened {
    loaded: project::Loaded,
    deps: Dependencies,
}

impl Opened {
    /// The package read, and the package files its graphs use.
    pub(crate) fn into_parts(self) -> (project::Loaded, Dependencies) {
        (self.loaded, self.deps)
    }
}

/// The package file at `file`, and the package files its graphs use, as deep as they use
/// others, but those in `known` (open, or read already).
pub(crate) fn read_with_dependencies(file: &str, mut known: HashSet<String>) -> Result<Opened, String> {
    let loaded = project::load(Path::new(file))?;
    known.insert(file.to_string());
    let mut wanted: Vec<String> = loaded.package.dependencies().into_iter().collect();
    let mut deps = Vec::new();
    while let Some(dep) = wanted.pop() {
        if !known.insert(dep.clone()) {
            continue;
        }
        let read = project::load(Path::new(&dep)).map(|l| l.package);
        if let Ok(package) = &read {
            wanted.extend(package.dependencies());
        }
        deps.push((dep, read));
    }
    Ok(Opened { loaded, deps })
}

impl App {
    // ------------------------------------------------------------------ the open packages

    /// Every package open in the Explorer, in the order they were opened: its key and itself.
    pub fn packages(&self) -> Vec<(u64, &Document)> {
        let mut all: Vec<(u64, &Document)> = self.shelf.iter().map(|p| (p.key, &p.doc)).collect();
        all.push((self.doc_key, &self.doc));
        all.sort_by_key(|(key, _)| *key);
        all
    }

    /// The package open in the Explorer as `key`.
    pub fn package_doc(&self, key: u64) -> Option<&Document> {
        if key == self.doc_key { Some(&self.doc) } else { self.shelf.iter().find(|p| p.key == key).map(|p| &p.doc) }
    }

    pub(super) fn package_doc_mut(&mut self, key: u64) -> Option<&mut Document> {
        if key == self.doc_key {
            Some(&mut self.doc)
        } else {
            self.shelf.iter_mut().find(|p| p.key == key).map(|p| &mut p.doc)
        }
    }

    pub(super) fn all_docs(&self) -> impl Iterator<Item = &Document> {
        std::iter::once(&self.doc).chain(self.shelf.iter().map(|p| &p.doc))
    }

    /// The package open in the Explorer that is the file at `path`.
    pub(super) fn open_key(&self, path: &str) -> Option<u64> {
        self.packages().into_iter().find(|(_, d)| d.key().as_deref() == Some(path)).map(|(k, _)| k)
    }

    fn next_key(&mut self) -> u64 {
        self.next_key += 1;
        self.next_key
    }

    /// Where the view of the graph being edited is.
    pub(super) fn view_state(&self) -> ViewState {
        ViewState {
            preview: self.ui.preview.node,
            graph_pan: Some([self.ui.graph_view.pan.x, self.ui.graph_view.pan.y]),
            graph_zoom: Some(self.ui.graph_view.zoom),
        }
    }

    /// Undo or redo went back to the graph a change was made in, from `graph`, whose view was
    /// `view`: the view of that graph is kept for it, and the one of the graph gone back to is
    /// where it was, as when a graph is opened.
    pub(super) fn stepped_from(&mut self, graph: GraphId, view: ViewState) {
        if self.doc.current() == graph {
            return;
        }
        self.views.insert((self.doc_key, graph), view);
        let view = self.views.get(&(self.doc_key, self.doc.current())).cloned();
        self.restore_view(view);
    }

    /// Put the views where they were for the graph being edited, or frame it when it has not
    /// been shown yet.
    fn restore_view(&mut self, view: Option<ViewState>) {
        let view = view.unwrap_or_default();
        let preview = view.preview.or_else(|| self.graph().nodes_of(NodeKind::OUTPUT).next().map(|n| n.id));
        self.after_document_change(preview);
        if let (Some(pan), Some(zoom)) = (view.graph_pan, view.graph_zoom) {
            self.ui.graph_view.pan = Vector::new(pan[0], pan[1]);
            self.ui.graph_view.zoom = zoom.clamp(0.1, 3.0);
            self.ui.frame_request = false;
        }
    }

    /// Edit the graph `graph` (the package's own when `None`) of the package open as `key`.
    pub(super) fn activate(&mut self, key: u64, graph: Option<GraphId>) {
        let same = key == self.doc_key && graph.is_none_or(|g| g == self.doc.current());
        if same || self.package_doc(key).is_none() {
            return;
        }
        let view = self.view_state();
        self.views.insert((self.doc_key, self.doc.current()), view);
        if key != self.doc_key {
            let i = self.shelf.iter().position(|p| p.key == key).expect("an open package");
            let next = self.shelf.swap_remove(i);
            let before = std::mem::replace(&mut self.doc, next.doc);
            self.shelf.push(OpenPackage { key: self.doc_key, doc: before });
            self.doc_key = key;
        }
        if let Some(graph) = graph {
            self.doc.open_graph(graph);
        }
        self.doc.seal();
        let view = self.views.get(&(self.doc_key, self.doc.current())).cloned();
        self.restore_view(view);
    }

    /// Make the package being edited `package`, the file at `path` (`None`: untitled), in
    /// place of what it was: a new package of the Explorer.
    pub(super) fn replace_package(
        &mut self,
        package: Package,
        path: Option<PathBuf>,
        views: HashMap<GraphId, ViewState>,
        open: GraphId,
    ) {
        self.views.retain(|(key, _), _| *key != self.doc_key);
        self.doc_key = self.next_key();
        if let Some(path) = &path {
            self.forget_file(&path.display().to_string());
        }
        self.doc.replace(package, path);
        self.doc.open_graph(open);
        for (graph, view) in views {
            self.views.insert((self.doc_key, graph), view);
        }
        let view = self.views.get(&(self.doc_key, self.doc.current())).cloned();
        self.restore_view(view);
        self.load_dependencies();
    }

    /// Add `package` to the Explorer and edit it. A package that is untitled and untouched
    /// (the one the program opens with, a new one) makes room for it.
    pub(super) fn add_package(
        &mut self,
        package: Package,
        path: Option<PathBuf>,
        views: HashMap<GraphId, ViewState>,
        open: GraphId,
    ) {
        let untouched = self.doc.path().is_none() && !self.doc.is_dirty() && !self.doc.can_undo();
        if untouched {
            self.replace_package(package, path, views, open);
            return;
        }
        let view = self.view_state();
        self.views.insert((self.doc_key, self.doc.current()), view);
        let key = self.next_key();
        let doc = self.document(package, path.clone());
        let before = std::mem::replace(&mut self.doc, doc);
        self.shelf.push(OpenPackage { key: self.doc_key, doc: before });
        self.doc_key = key;
        if let Some(path) = &path {
            self.forget_file(&path.display().to_string());
        }
        self.doc.open_graph(open);
        for (graph, view) in views {
            self.views.insert((key, graph), view);
        }
        let view = self.views.get(&(key, self.doc.current())).cloned();
        self.restore_view(view);
        self.load_dependencies();
    }

    /// A package is open from `path` now: the copy read as a dependency goes, and a file that
    /// could not be read is tried again.
    fn forget_file(&mut self, path: &str) {
        self.deps.remove(path);
        self.unreadable.remove(path);
    }

    /// Open the package file at `path`: in the Explorer (or edit it, when it is open already),
    /// or in place of the package being edited (`replace`: the unsaved changes dialog has been
    /// answered). It is read off the UI thread, with the package files its graphs use that are
    /// not read yet: a large one takes a while (see [`App::opened`]).
    pub(crate) fn open_file(&mut self, path: PathBuf, replace: bool) -> Task<Message> {
        let file = clean(&path);
        if !replace && let Some(open) = self.open_key(&file) {
            self.activate(open, None);
            return Task::none();
        }
        // Being written: opened once it is.
        if let Some(saving) = self.ui.saving.values_mut().find(|s| crate::workspace::same_file(&s.path, &file)) {
            saving.open = Some(replace);
            return Task::none();
        }
        if !self.ui.opening.insert(file.clone()) {
            return Task::none();
        }
        self.set_status(format!("Opening {file}…"), false);
        let known: HashSet<String> =
            self.all_docs().filter_map(Document::key).chain(self.deps.keys().cloned()).chain(self.unreadable.iter().cloned()).collect();
        let read = file.clone();
        self.in_background(
            move || read_with_dependencies(&read, known).map(Arc::new),
            move |done| Message::Opened(file.clone(), replace, done.unwrap_or_else(|| Err(format!("{file} was not opened: what read it stopped")))),
        )
    }

    /// A package file was read (see [`App::open_file`]): the packages it uses are kept, and it
    /// is opened; what it names that is not there is asked for.
    pub(super) fn opened(&mut self, file: String, replace: bool, result: Result<Arc<Opened>, String>) {
        self.ui.opening.remove(&file);
        let Opened { loaded, deps } = match result.map(Arc::try_unwrap) {
            Ok(Ok(opened)) => opened,
            Ok(Err(_)) => return,
            Err(e) => {
                self.set_status(e, true);
                return;
            }
        };
        self.take_dependencies(deps);
        let notes = loaded.notes.clone();
        let path = Some(PathBuf::from(&file));
        if let Some(picture) = &loaded.picture {
            self.pictures.insert(file.clone(), Arc::new(picture.clone()));
        }
        // Opened: in place of a package that is blank (as the program starts on Home), and in
        // the editor.
        let replace = replace || self.blank();
        self.ui.home = None;
        if replace {
            // Open elsewhere in the Explorer already: that one goes, this one takes its place.
            if let Some(open) = self.open_key(&file).filter(|k| *k != self.doc_key) {
                self.shelf.retain(|p| p.key != open);
            }
            self.replace_package(loaded.package, path, loaded.views, loaded.open);
        } else if let Some(open) = self.open_key(&file) {
            // Opened meanwhile.
            self.activate(open, None);
            return;
        } else {
            self.add_package(loaded.package, path, loaded.views, loaded.open);
        }
        crate::workspace::remember(&mut self.ui.recent, &file);
        let mut msg = format!("Opened {file}");
        if !notes.is_empty() {
            msg.push_str(&format!(" ({})", notes.join("; ")));
        }
        self.set_status(msg, false);
        self.ask_for_missing();
    }

    /// Keep the package files `deps` read, which graphs use (but those open, or kept already).
    pub(crate) fn take_dependencies(&mut self, deps: Dependencies) {
        let open: HashSet<String> = self.all_docs().filter_map(Document::key).collect();
        for (dep, read) in deps {
            if open.contains(&dep) || self.deps.contains_key(&dep) || self.unreadable.contains(&dep) {
                continue;
            }
            match read {
                Ok(package) => {
                    self.deps.insert(dep, package);
                }
                Err(e) => {
                    log::warn!("a package that is used cannot be read: {e}");
                    self.unreadable.insert(dep);
                }
            }
        }
    }

    /// Save the package open as `key` at `path`, off the UI thread (a package that holds large
    /// files takes a while), as it is now: an edit made meanwhile is unsaved still once it is
    /// written. What was to follow the save (`ui.after_save`) follows it, if it goes well. A
    /// package being saved is saved again once that is done.
    pub(crate) fn save_package(&mut self, key: u64, path: PathBuf) -> Task<Message> {
        let path = PathBuf::from(clean(&path));
        let then = self.ui.after_save.take();
        if let Some(saving) = self.ui.saving.get_mut(&key) {
            saving.again = Some(path);
            if then.is_some() {
                saving.then = then;
            }
            return Task::none();
        }
        let Some(doc) = self.package_doc(key) else { return Task::none() };
        let (package, current) = (doc.package().clone(), doc.current());
        let views = self.views_of(key);
        // (Its picture, for the Home screen.)
        let picture = self.picture_for(key, &path.display().to_string());
        self.set_status(format!("Saving {}…", path.display()), false);
        let (written, to) = (package.clone(), path.clone());
        self.ui.saving.insert(key, Saving { path: path.display().to_string(), package, then, again: None, open: None });
        // Its earlier versions (see `versions`): the file it writes over first, if they have not
        // got it, then it as it is saved. A version that cannot be kept is said, the save done.
        let versions = self.versions_kept();
        self.in_background(
            move || -> Result<Option<String>, String> {
                let mut unkept = versions.as_ref().and_then(|(v, keep)| v.before_writing(&to, *keep).err());
                project::save_with_picture(&to, &written, &views, current, picture.as_ref().map(|p| p.as_slice()))?;
                if let Some((v, keep)) = &versions {
                    unkept = unkept.or(v.keep(&to, &written, &views, current, brewed_texture_core::versions::Kind::Saved, *keep).err());
                }
                Ok(unkept)
            },
            move |done| Message::Saved(key, path.clone(), done.unwrap_or_else(|| Err("The package was not saved: what wrote it stopped".into()))),
        )
    }

    /// A package was written, or could not be (see [`App::save_package`]): saved if it is as it
    /// was written; what was to follow the save follows. What waited for the packages being
    /// saved goes on once none is.
    pub(super) fn saved(&mut self, key: u64, path: PathBuf, result: Result<Option<String>, String>) -> Task<Message> {
        let Some(saving) = self.ui.saving.remove(&key) else { return Task::none() };
        let task = match result {
            Ok(unkept) => {
                match unkept {
                    None => self.set_status(format!("Saved {}", path.display()), false),
                    Some(e) => self.set_status(format!("Saved {}, but its version was not kept: {e}", path.display()), true),
                }
                // Into a library folder: the Library shows it as it is now.
                let library = if self.in_library(&path) { self.look_at_library() } else { Task::none() };
                if let Some(doc) = self.package_doc_mut(key) {
                    let same = *doc.package() == saving.package;
                    doc.saved_at(path.clone(), same);
                }
                self.saved_as(key, &path);
                self.forget_saved();
                let after = match saving.again {
                    Some(again) => {
                        self.ui.after_save = saving.then;
                        let task = self.save_package(key, again);
                        // (Opened once it is written again.)
                        if let Some(next) = self.ui.saving.get_mut(&key) {
                            next.open = next.open.or(saving.open);
                        }
                        task
                    }
                    None => {
                        let then = self.continue_after(saving.then);
                        match saving.open {
                            Some(replace) => Task::batch([then, self.open_file(path, replace)]),
                            None => then,
                        }
                    }
                };
                Task::batch([after, library])
            }
            // What was to follow the save (quitting, closing the package) does not: a save
            // that works later is only a save. The file is opened as it is, if it is there.
            Err(e) => {
                self.set_status(e, true);
                match saving.open {
                    Some(replace) => self.open_file(path, replace),
                    None => Task::none(),
                }
            }
        };
        match self.ui.after_saving.take() {
            Some(action) if self.ui.saving.is_empty() => Task::batch([task, self.guarded(action)]),
            waiting => {
                self.ui.after_saving = waiting;
                task
            }
        }
    }

    /// A new, empty package in the Explorer.
    #[cfg(any(test, feature = "dev-tools"))]
    pub(super) fn new_package(&mut self) {
        let package = Package::new("Graph", self.new_graph());
        let open = package.first();
        self.add_package(package, None, HashMap::new(), open);
    }

    /// The demo, as a package of its own in the Explorer.
    pub(super) fn open_demo(&mut self) {
        let _ = self.home_message(super::home::HomeMessage::Example(brewed_texture_core::templates::Example::BrickWall));
    }

    /// Close the package open as `key` (its unsaved changes go). The last one leaves an empty
    /// package in its place.
    pub(super) fn close_package(&mut self, key: u64) {
        if key != self.doc_key {
            self.shelf.retain(|p| p.key != key);
        } else if let Some(next) = self.packages().into_iter().map(|(k, _)| k).find(|k| *k != key) {
            self.activate(next, None);
            self.shelf.retain(|p| p.key != key);
        } else {
            let package = Package::new("Graph", self.new_graph());
            let open = package.first();
            self.replace_package(package, None, HashMap::new(), open);
        }
        self.views.retain(|(k, _), _| *k != key);
        self.ui.explorer_folded.retain(|f| match f {
            Fold::Package(k) | Fold::Resources(k) | Fold::Kind(k, _) => *k != key,
        });
        // A package still used by the others is read from its file again.
        self.load_dependencies();
        // Its unsaved changes are gone: nothing of it is recovered.
        self.forget_saved();
    }

    /// The packages with unsaved changes, in the Explorer's order.
    pub(super) fn dirty_packages(&self) -> Vec<u64> {
        self.packages().into_iter().filter(|(_, d)| d.is_dirty()).map(|(k, _)| k).collect()
    }

    /// Where the view of each graph of the package open as `key` is.
    pub(super) fn views_of(&self, key: u64) -> HashMap<GraphId, ViewState> {
        let mut views: HashMap<GraphId, ViewState> =
            self.views.iter().filter(|((k, _), _)| *k == key).map(|((_, g), v)| (*g, v.clone())).collect();
        if key == self.doc_key {
            views.insert(self.doc.current(), self.view_state());
        }
        views
    }

    /// The package open as `key` was saved at `path`: whatever was read of that file is its
    /// copy now.
    fn saved_as(&mut self, key: u64, path: &Path) {
        let file = path.display().to_string();
        crate::workspace::remember(&mut self.ui.recent, &file);
        self.forget_file(&file);
        // Another package open in the Explorer from the same file is this one now.
        self.shelf.retain(|p| p.key == key || p.doc.key().as_deref() != Some(file.as_str()));
    }

    // ------------------------------------------------------------------ graphs used as nodes

    /// Read the package files the graphs of the open packages use, as deep as they use others.
    /// Those that cannot be read are asked for when their user opens (see
    /// [`App::ask_for_missing`]).
    pub(super) fn load_dependencies(&mut self) {
        for _ in 0..32 {
            let open: HashSet<String> = self.all_docs().filter_map(Document::key).collect();
            let wanted: BTreeSet<String> = self
                .all_docs()
                .map(Document::package)
                .chain(self.deps.values())
                .flat_map(Package::dependencies)
                .filter(|p| !open.contains(p) && !self.deps.contains_key(p) && !self.unreadable.contains(p))
                .collect();
            if wanted.is_empty() {
                return;
            }
            for path in wanted {
                match project::load(Path::new(&path)) {
                    Ok(loaded) => {
                        self.deps.insert(path, loaded.package);
                    }
                    Err(e) => {
                        log::warn!("a package that is used cannot be read: {e}");
                        self.unreadable.insert(path);
                    }
                }
            }
        }
    }

    /// Bring the nodes that use graphs, in every open package, in line with those graphs.
    pub(super) fn sync_instances(&mut self) {
        {
            let lib = Shelf::new(None, self.shelf.iter().map(|p| &p.doc), &self.deps);
            self.doc.sync_instances(&lib);
        }
        for i in 0..self.shelf.len() {
            let (before, rest) = self.shelf.split_at_mut(i);
            let (item, after) = rest.split_first_mut().expect("in the shelf");
            let others = before.iter().chain(after.iter()).map(|p| &p.doc).chain(std::iter::once(&self.doc));
            let lib = Shelf::new(None, others, &self.deps);
            item.doc.sync_instances(&lib);
        }
    }

    /// The graph being edited with every graph it uses laid out in it: what the engine
    /// computes.
    pub(super) fn flatten(&self) -> Flat {
        let own = self.doc.key();
        let docs = self.shelf.iter().map(|p| &p.doc).chain(own.is_some().then_some(&self.doc));
        let lib = Shelf::new(Some(self.doc.package()), docs, &self.deps);
        package::flatten(&lib, own.as_deref(), self.doc.current()).unwrap_or_default()
    }

    /// Add a node that uses the graph `graph` of the package open as `key`, at `pos`.
    pub(super) fn add_graph_node(&mut self, key: u64, graph: GraphId, pos: [f32; 2]) {
        if let Some(why) = self.graph_refusal(key, graph) {
            self.set_status(why, true);
            return;
        }
        let Some(source) = self.package_doc(key) else { return };
        let Some(entry) = source.package().graph(graph) else { return };
        let package = if key == self.doc_key { None } else { source.key() };
        let instance = Instance::new(GraphRef { package, graph, name: entry.name.clone() }, Interface::of(&entry.graph));
        if self.apply(Edit::AddInstance { instance, pos }) {
            self.doc.seal();
        }
    }

    /// Why the graph `graph` of the package open as `key` cannot be used in the graph being
    /// edited, if it cannot: it is that graph; it uses that graph (a graph cannot use a graph
    /// that uses it, however deep); or it is of another package that has no file to be found by.
    /// Said while it is dragged over the graph, before it is dropped.
    pub fn graph_refusal(&self, key: u64, graph: GraphId) -> Option<String> {
        let source = self.package_doc(key)?;
        let entry = source.package().graph(graph)?;
        let package = if key == self.doc_key {
            None
        } else {
            match source.key() {
                Some(path) => Some(path),
                None => {
                    let title = source.title();
                    return Some(format!("Save {title} first: another package finds its graphs by its file"));
                }
            }
        };
        self.use_refusal(&GraphRef { package, graph, name: entry.name.clone() })
    }

    /// Why the graph `r` says (of the package being edited, or of the package at its path)
    /// cannot be used in the graph being edited, if it cannot: it is that graph, or it uses it,
    /// however deep. A graph that cannot be found has nothing to refuse.
    pub fn use_refusal(&self, r: &GraphRef) -> Option<String> {
        let own = self.doc.key();
        let here = self.doc.entry().name.clone();
        // (A package's own file is the package itself.)
        let package = r.package.clone().filter(|p| Some(p) != own.as_ref());
        let docs = self.shelf.iter().map(|p| &p.doc).chain(own.is_some().then_some(&self.doc));
        let lib = Shelf::new(Some(self.doc.package()), docs, &self.deps);
        let from = package.clone().or_else(|| own.clone());
        let entry = lib.package(from.as_deref())?.find(r)?;
        if package.is_none() && entry.id == self.doc.current() {
            return Some(format!("'{here}' cannot use itself"));
        }
        package::uses_graph(&lib, from.as_deref(), entry.id, (own.as_deref(), self.doc.current()))
            .then(|| format!("'{}' uses '{here}': a graph cannot use a graph that uses it", entry.name))
    }

    /// The files a field takes: their extensions, lower case (none: any).
    pub fn takes(&self, target: &FileTarget) -> &'static [&'static str] {
        match target {
            FileTarget::Param(node, key) => match self.graph().node(*node).and_then(|n| n.param_def(key)).map(|pd| pd.kind) {
                Some(ParamKind::File { extensions }) => extensions,
                Some(ParamKind::Image { .. }) => brewed_texture_core::nodes::PICTURE_FILES,
                _ => &[],
            },
            FileTarget::Variable(_) | FileTarget::VariableDefault(_) => brewed_texture_core::nodes::PICTURE_FILES,
        }
    }

    /// The file a field reads (empty: none).
    pub fn file_of(&self, target: &FileTarget) -> &str {
        let graph = self.graph();
        let image = |name: &str| graph.variable(name).and_then(|v| v.image.as_ref());
        match target {
            FileTarget::Param(node, key) => graph.node(*node).map_or("", |n| n.text(key)),
            FileTarget::Variable(name) => image(name).map_or("", |i| i.picture.as_str()),
            FileTarget::VariableDefault(name) => image(name).map_or("", |i| i.default.as_str()),
        }
    }

    /// Make a field read the file at `path`, or none, as one step to undo. A file the package
    /// did not use before becomes one of its files (see the Explorer's Resources).
    pub(super) fn set_file(&mut self, target: FileTarget, path: Option<String>) -> Task<Message> {
        let picture = path.unwrap_or_default();
        let edit = match target {
            FileTarget::Param(node, key) => Edit::SetParam { node, key, value: ParamValue::Text(picture) },
            FileTarget::Variable(name) => Edit::SetVariablePicture { name, picture, default: false },
            FileTarget::VariableDefault(name) => Edit::SetVariablePicture { name, picture, default: true },
        };
        self.apply(edit);
        self.doc.seal();
        // A new file is looked at now, for its picture.
        if self.new_files() { self.look_at_files() } else { Task::none() }
    }

    /// Why what is dragged cannot be dropped on the graph being edited, if it cannot.
    pub fn drop_refusal(&self, payload: &DragPayload) -> Option<String> {
        match payload {
            DragPayload::Graph { key, graph, .. } => self.graph_refusal(*key, *graph),
            // (A package not read yet has nothing to refuse until it is.)
            DragPayload::Library { package, graph, name } => match self.open_key(package) {
                Some(key) => self.graph_refusal(key, *graph),
                None => self.use_refusal(&GraphRef { package: Some(package.clone()), graph: *graph, name: name.clone() }),
            },
            _ => None,
        }
    }

    /// Open the graph a node uses: in its package, which the Explorer opens if it has to.
    pub(super) fn open_reference(&mut self, node: NodeId) {
        let Some(r) = self.graph().node(node).and_then(|n| n.instance.as_deref()).map(|i| i.graph.clone()) else {
            return;
        };
        let path = match &r.package {
            None => {
                match self.doc.package().find(&r).map(|g| g.id) {
                    Some(id) => self.activate(self.doc_key, Some(id)),
                    None => self.set_status(format!("The package has no graph '{}'", r.name), true),
                }
                return;
            }
            Some(path) => path.clone(),
        };
        if let Some(key) = self.open_key(&path) {
            let id = self.package_doc(key).and_then(|d| d.package().find(&r)).map(|g| g.id);
            self.activate(key, id);
            if id.is_none() {
                self.set_status(format!("{} has no graph '{}'", package::file_name(&path), r.name), true);
            }
            return;
        }
        let loaded = match self.deps.remove(&path) {
            Some(package) => Ok((package, HashMap::new())),
            None => project::load(Path::new(&path)).map(|l| (l.package, l.views)),
        };
        match loaded {
            Ok((package, views)) => {
                let id = package.find(&r).map(|g| g.id);
                let open = id.unwrap_or_else(|| package.first());
                // Added, never in place of the package the node is in.
                let view = self.view_state();
                self.views.insert((self.doc_key, self.doc.current()), view);
                let key = self.next_key();
                let doc = self.document(package, Some(PathBuf::from(&path)));
                let before = std::mem::replace(&mut self.doc, doc);
                self.shelf.push(OpenPackage { key: self.doc_key, doc: before });
                self.doc_key = key;
                self.doc.open_graph(open);
                for (graph, view) in views {
                    self.views.insert((key, graph), view);
                }
                let view = self.views.get(&(key, open)).cloned();
                self.restore_view(view);
                self.load_dependencies();
                if id.is_none() {
                    self.set_status(format!("{} has no graph '{}'", package::file_name(&path), r.name), true);
                }
            }
            Err(e) => self.set_status(e, true),
        }
    }

    /// Put the selected nodes in a graph of their own, used in their place.
    pub(super) fn graph_from_selection(&mut self) {
        let ids = self.selected_ids();
        if ids.is_empty() {
            self.set_status("Select the nodes to put in a graph of their own", false);
            return;
        }
        self.close_popups();
        let name = self.doc.package().unique_graph_name("New Graph");
        match self.doc.apply_package(PackageEdit::GraphFromSelection { graph: self.doc.current(), nodes: ids.clone(), name })
        {
            Ok(result) => {
                self.ui.selection = result.created.iter().copied().collect();
                self.ui.selected_link = None;
                let name =
                    result.graph.and_then(|g| self.doc.package().graph(g)).map(|g| g.name.clone()).unwrap_or_default();
                self.set_status(
                    format!(
                        "{} node(s) are the graph '{name}' now: double-click it in the Explorer to edit it",
                        ids.len()
                    ),
                    false,
                );
            }
            Err(e) => self.set_status(e, true),
        }
    }

    // ------------------------------------------------------------------ missing files

    /// Ask for the files the package being edited names and that are not there: its own files
    /// and the packages whose graphs it uses.
    pub(super) fn ask_for_missing(&mut self) {
        let items = self.doc.package().missing();
        if !items.is_empty() {
            self.ui.dialog = Some(Dialog::Missing(MissingFiles { key: self.doc_key, items, note: None }));
        }
    }

    /// Copy the files the package open as `key` uses that are not beside it into
    /// `<its name>_files` beside it, on a thread of its own (see [`copy_files`]); the package
    /// uses the copies once they are made. A package without a file has nowhere to put them.
    fn collect_files(&mut self, key: u64) -> Task<Message> {
        let Some(doc) = self.package_doc(key) else { return Task::none() };
        let Some(file) = doc.path().cloned() else {
            let title = doc.title();
            self.set_status(format!("Save {title} first: its files are collected beside its file"), true);
            return Task::none();
        };
        let home = file.parent().map(Path::to_path_buf).unwrap_or_default();
        let stem = file.file_stem().map_or_else(|| "package".to_string(), |s| s.to_string_lossy().to_string());
        let folder = home.join(format!("{stem}_files"));
        let plan = package::collect_plan(&doc.package().all_files(), &home, &folder, Path::exists);
        if plan.is_empty() {
            let title = doc.title();
            self.set_status(format!("The files of {title} are beside it already"), false);
            return Task::none();
        }
        Task::perform(blocking(move || copy_files(&plan)), move |collected| {
            Message::Explorer(ExplorerMessage::FilesCollected(key, collected.unwrap_or_default()))
        })
    }

    /// The files of a package were copied: it uses the copies from now on (one step to undo),
    /// and what could not be copied is said.
    fn files_collected(&mut self, key: u64, collected: Collected) {
        let count = collected.copied.len();
        if let Some(doc) = self.package_doc_mut(key)
            && count > 0
            && let Err(e) = doc.apply_package(PackageEdit::Relocate(collected.copied.clone()))
        {
            self.set_status(e, true);
            return;
        }
        let folder = collected.copied.first().and_then(|(_, to)| Path::new(to).parent().map(|d| d.display().to_string()));
        let mut said = match folder {
            Some(folder) => format!("Copied {count} file(s) into {folder}: save the package to keep using them"),
            None => "No file was copied".to_string(),
        };
        if !collected.failed.is_empty() {
            said = format!("{said}; not copied: {}", collected.failed.join(", "));
        }
        self.set_status(said, !collected.failed.is_empty());
    }

    /// The file dialog to find a file that is missing.
    fn locate(&self, key: u64, path: String) -> Task<Message> {
        let what = match &self.ui.dialog {
            Some(Dialog::Missing(m)) => m.items.iter().find(|i| i.path == path).map(|i| i.what),
            _ => None,
        };
        let what = what.or_else(|| ResourceKind::of_path(&path).map(MissingKind::Resource));
        let (label, extensions): (String, Vec<&'static str>) = match what {
            Some(MissingKind::Package) | None => {
                (format!("{} package", brewed_texture_core::APP_NAME), vec![project::FILE_EXTENSION])
            }
            Some(MissingKind::Resource(kind)) => (kind.name().to_string(), kind.extensions().to_vec()),
        };
        // Where it was, else where dialogs open.
        let was = Path::new(&path).parent().filter(|d| d.is_dir()).map(PathBuf::from);
        let folder = was.or_else(|| self.dialog_folder(self.package_doc(key).and_then(|d| d.path()).map(PathBuf::as_path), false));
        let title = format!("Where is {}?", package::file_name(&path));
        Task::perform(
            blocking(move || {
                let mut dialog = rfd::FileDialog::new().set_title(title).add_filter(label, &extensions);
                if let Some(folder) = folder {
                    dialog = dialog.set_directory(folder);
                }
                dialog.pick_file()
            }),
            move |p| Message::Explorer(ExplorerMessage::Located(key, path.clone(), p.flatten())),
        )
    }

    /// The missing file `from` of the package open as `key` is at `to`: the package says so
    /// from now on (an undo step). A package must have the graphs that are used of it. Other
    /// files missing from the same folder are looked for next to it.
    fn located(&mut self, key: u64, from: String, to: PathBuf) {
        let to = clean(&to);
        let item = match &self.ui.dialog {
            Some(Dialog::Missing(m)) => m.items.iter().find(|i| i.path == from).cloned(),
            _ => None,
        };
        if let Some(Missing { what: MissingKind::Package, graphs, .. }) = &item {
            let wrong = match project::load(Path::new(&to)) {
                Ok(loaded) => {
                    let lacks: Vec<&String> =
                        graphs.iter().filter(|name| !loaded.package.graphs.iter().any(|g| &g.name == *name)).collect();
                    (!lacks.is_empty()).then(|| {
                        let names: Vec<&str> = lacks.iter().map(|s| s.as_str()).collect();
                        format!(
                            "{} has no graph {}: it is not the package that was there",
                            package::file_name(&to),
                            names.join(", ")
                        )
                    })
                }
                Err(e) => Some(e),
            };
            if let Some(note) = wrong {
                if let Some(Dialog::Missing(m)) = &mut self.ui.dialog {
                    m.note = Some(note);
                }
                return;
            }
        }
        let mut found = vec![(from.clone(), to.clone())];
        // The others that were in the same folder as this one may have moved with it.
        let (old_dir, new_dir) =
            (Path::new(&from).parent().map(PathBuf::from), Path::new(&to).parent().map(PathBuf::from));
        if let (Some(old_dir), Some(new_dir), Some(Dialog::Missing(m))) = (old_dir, new_dir, &self.ui.dialog) {
            for other in m.items.iter().filter(|i| i.path != from) {
                let p = Path::new(&other.path);
                if p.parent() == Some(old_dir.as_path())
                    && let Some(name) = p.file_name()
                    && new_dir.join(name).is_file()
                {
                    found.push((other.path.clone(), new_dir.join(name).display().to_string()));
                }
            }
        }
        let Some(doc) = self.package_doc_mut(key) else { return };
        let count = found.len();
        for (from, to) in &found {
            if let Err(e) = doc.apply_package(PackageEdit::Relink { from: from.clone(), to: to.clone() }) {
                log::warn!("relinking {from}: {e}");
            }
        }
        for (_, to) in &found {
            self.unreadable.remove(to);
        }
        if let Some(Dialog::Missing(m)) = &mut self.ui.dialog {
            m.items.retain(|i| !found.iter().any(|(from, _)| *from == i.path));
            m.note = (count > 1).then(|| format!("{} more found in the same folder", count - 1));
            if m.items.is_empty() {
                self.ui.dialog = None;
            }
        }
        self.load_dependencies();
        self.set_status(format!("Relinked {count} file(s)"), false);
    }

    // ------------------------------------------------------------------ the files packages use

    /// The files the open packages use, each once, and their kinds.
    pub(super) fn used_files(&self) -> Vec<(String, ResourceKind)> {
        let mut seen = HashSet::new();
        self.all_docs()
            .flat_map(|d| d.package().resources.iter())
            .filter(|r| seen.insert(r.path.clone()))
            .map(|r| (r.path.clone(), r.kind))
            .collect()
    }

    /// Do the open packages use files that were not there when the files were looked at last?
    pub(super) fn new_files(&self) -> bool {
        let looked: HashSet<&String> = self.res.looked.iter().collect();
        self.used_files().iter().any(|(p, _)| !looked.contains(p))
    }

    /// Look at the files the open packages use, on a thread of its own (see
    /// [`crate::previews`]); unless they are being looked at.
    pub(super) fn look_at_files(&mut self) -> Task<Message> {
        if self.res.looking {
            return Task::none();
        }
        let paths: Vec<String> = self.used_files().into_iter().map(|(p, _)| p).collect();
        self.res.looked = paths.clone();
        if paths.is_empty() {
            self.res.files.clear();
            return Task::none();
        }
        self.res.looking = true;
        Task::perform(blocking(move || crate::previews::look(&paths)), |looks| Message::FilesLooked(looks.unwrap_or_default()))
    }

    /// What looking at the files found: a picture is made of each that is there and changed
    /// since its last one (a file on disk edited elsewhere), in the background, a few at a time;
    /// and a file edited elsewhere, or back where it was, is read again, so that what reads it
    /// shows it as it is now.
    pub(super) fn files_looked(&mut self, looks: Vec<crate::previews::Look>) -> Task<Message> {
        self.res.looking = false;
        let kinds: HashMap<String, ResourceKind> = self.used_files().into_iter().collect();
        self.res.files.retain(|path, _| kinds.contains_key(path));
        let mut changed = Vec::new();
        for look in looks {
            let Some(kind) = kinds.get(&look.path).copied() else { continue };
            // (A file not looked at before was read as it is.)
            let known = self.res.files.contains_key(&look.path);
            let state = self.res.files.entry(look.path.clone()).or_default();
            if known && look.there && (state.modified != look.modified || !state.there) {
                changed.push(look.path.clone());
            }
            (state.there, state.modified, state.bytes) = (look.there, look.modified, look.bytes);
            if state.wants_preview() {
                state.making = true;
                self.res.previews_waiting.push_back((look.path, kind, look.modified));
            }
        }
        for path in changed {
            self.send(brewed_texture_engine::Request::ReloadFile(path));
        }
        self.make_previews()
    }

    /// Start making the pictures waiting, as many at a time as [`crate::previews::AT_ONCE`].
    fn make_previews(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        while self.res.previews_making < crate::previews::AT_ONCE
            && let Some((path, kind, modified)) = self.res.previews_waiting.pop_front()
        {
            // (A file no package uses any more is not pictured.)
            if !self.res.files.contains_key(&path) {
                continue;
            }
            self.res.previews_making += 1;
            let made = path.clone();
            tasks.push(Task::perform(blocking(move || crate::previews::make(&made, kind)), move |made| {
                let made = made.unwrap_or_else(|| Err("The picture could not be made".into()));
                Message::PreviewMade(path.clone(), modified, made)
            }));
        }
        Task::batch(tasks)
    }

    /// A picture of a file was made (of the file as it was when it changed at `of`), or not:
    /// the next one waiting is started.
    pub(super) fn preview_made(
        &mut self,
        path: String,
        of: Option<std::time::SystemTime>,
        made: Result<(Vec<u8>, u32, u32, String), String>,
    ) -> Task<Message> {
        self.res.previews_making = self.res.previews_making.saturating_sub(1);
        if let Some(state) = self.res.files.get_mut(&path) {
            state.making = false;
            match made {
                Ok((rgba, w, h, about)) => {
                    let image = iced::widget::image::Handle::from_rgba(w, h, rgba);
                    state.preview = Some(crate::previews::Preview { image, about, of });
                    state.failed = None;
                }
                Err(e) => state.failed = Some((of, e)),
            }
        }
        self.make_previews()
    }

    // ------------------------------------------------------------------ the Explorer

    pub(super) fn explorer_message(&mut self, msg: ExplorerMessage) -> Task<Message> {
        match msg {
            ExplorerMessage::Toggle(fold) => {
                if !self.ui.explorer_folded.remove(&fold) {
                    self.ui.explorer_folded.insert(fold);
                }
            }
            ExplorerMessage::OpenGraph(key, graph) => {
                self.close_popups();
                self.activate(key, Some(graph));
            }
            #[cfg(test)]
            ExplorerMessage::NewGraph(key) => {
                self.close_popups();
                let graph = self.new_graph();
                let Some(doc) = self.package_doc_mut(key) else { return Task::none() };
                match doc.apply_package(PackageEdit::AddGraph { name: "Graph".into(), graph }) {
                    Ok(result) => {
                        if let Some(graph) = result.graph {
                            self.activate(key, Some(graph));
                            self.ui.renaming_graph =
                                self.package_doc(key).map(|d| (key, graph, d.entry().name.clone()));
                            return iced::widget::operation::focus(crate::view::explorer::rename_id());
                        }
                    }
                    Err(e) => self.set_status(e, true),
                }
            }
            ExplorerMessage::DuplicateGraph(key, graph) => {
                self.close_popups();
                if let Some(doc) = self.package_doc_mut(key)
                    && let Err(e) = doc.apply_package(PackageEdit::DuplicateGraph(graph))
                {
                    self.set_status(e, true);
                }
            }
            ExplorerMessage::DeleteGraph(key, graph) => {
                self.close_popups();
                let before = (self.doc_key, self.doc.current());
                let Some(doc) = self.package_doc_mut(key) else { return Task::none() };
                match doc.apply_package(PackageEdit::RemoveGraph(graph)) {
                    Ok(result) => {
                        if let Some(notice) = result.notice {
                            self.set_status(notice, true);
                        }
                        // The graph being edited went: the package's first is edited now.
                        if before.0 == key && before.1 == graph {
                            let view = self.views.get(&(self.doc_key, self.doc.current())).cloned();
                            self.restore_view(view);
                        }
                        self.views.remove(&(key, graph));
                    }
                    Err(e) => self.set_status(e, true),
                }
            }
            ExplorerMessage::StartRename(key, graph) => {
                self.close_popups();
                let name = self.package_doc(key).and_then(|d| d.package().graph(graph)).map(|g| g.name.clone());
                if let Some(name) = name {
                    self.ui.renaming_graph = Some((key, graph, name));
                    return iced::widget::operation::focus(crate::view::explorer::rename_id());
                }
            }
            ExplorerMessage::RenameText(text) => {
                if let Some((_, _, name)) = &mut self.ui.renaming_graph {
                    *name = text;
                }
            }
            ExplorerMessage::CommitRename => {
                if let Some((key, graph, name)) = self.ui.renaming_graph.take()
                    && let Some(doc) = self.package_doc_mut(key)
                    && let Err(e) = doc.apply_package(PackageEdit::RenameGraph { id: graph, name })
                {
                    self.set_status(e, true);
                }
            }
            ExplorerMessage::StartRenameFile(key, path) => {
                self.close_popups();
                let name = self.package_doc(key).and_then(|d| d.package().resources.iter().find(|r| r.path == path).map(|r| r.name.clone()));
                if let Some(name) = name {
                    self.ui.renaming_file = Some((key, path, name));
                    return iced::widget::operation::focus(crate::view::explorer::rename_id());
                }
            }
            ExplorerMessage::RenameFileText(text) => {
                if let Some((_, _, name)) = &mut self.ui.renaming_file {
                    *name = text;
                }
            }
            ExplorerMessage::CommitRenameFile => {
                if let Some((key, path, name)) = self.ui.renaming_file.take()
                    && let Some(doc) = self.package_doc_mut(key)
                    && let Err(e) = doc.apply_package(PackageEdit::RenameResource { path, name })
                {
                    self.set_status(e, true);
                }
            }
            ExplorerMessage::Save(key) => {
                self.activate(key, None);
                return self.update_nested(Message::Save);
            }
            ExplorerMessage::SaveAs(key) => {
                self.activate(key, None);
                return self.update_nested(Message::SaveAs);
            }
            ExplorerMessage::Close(key) => return self.guarded(PendingAction::Close(key)),
            ExplorerMessage::LinkFiles(key) => {
                self.close_popups();
                let extensions: Vec<&'static str> =
                    ResourceKind::ALL.iter().flat_map(|k| k.extensions().iter().copied()).collect();
                let folder = self.dialog_folder(self.package_doc(key).and_then(|d| d.path()).map(PathBuf::as_path), false);
                return Task::perform(
                    blocking(move || {
                        let mut dialog = rfd::FileDialog::new()
                            .set_title("Link files to the package")
                            .add_filter("Images, vector graphics and fonts", &extensions);
                        for kind in ResourceKind::ALL {
                            dialog = dialog.add_filter(kind.name(), kind.extensions());
                        }
                        if let Some(folder) = folder {
                            dialog = dialog.set_directory(folder);
                        }
                        dialog.pick_files()
                    }),
                    move |files| {
                        Message::Explorer(ExplorerMessage::FilesPicked(key, files.flatten().unwrap_or_default()))
                    },
                );
            }
            ExplorerMessage::FilesPicked(key, files) => {
                let paths: Vec<String> = files.iter().map(|p| clean(p)).collect();
                if paths.is_empty() {
                    return Task::none();
                }
                if let Some(doc) = self.package_doc_mut(key) {
                    match doc.apply_package(PackageEdit::AddResources(paths)) {
                        Ok(result) => {
                            if let Some(n) = result.notice {
                                self.set_status(n, true);
                            }
                        }
                        Err(e) => self.set_status(e, true),
                    }
                }
            }
            ExplorerMessage::ImportFiles(key) => {
                self.close_popups();
                let extensions: Vec<&'static str> =
                    ResourceKind::ALL.iter().flat_map(|k| k.extensions().iter().copied()).collect();
                let folder = self.dialog_folder(self.package_doc(key).and_then(|d| d.path()).map(PathBuf::as_path), false);
                return Task::perform(
                    blocking(move || {
                        let mut dialog = rfd::FileDialog::new()
                            .set_title("Import files into the package")
                            .add_filter("Images, vector graphics and fonts", &extensions);
                        for kind in ResourceKind::ALL {
                            dialog = dialog.add_filter(kind.name(), kind.extensions());
                        }
                        if let Some(folder) = folder {
                            dialog = dialog.set_directory(folder);
                        }
                        dialog.pick_files()
                    }),
                    move |files| {
                        Message::Explorer(ExplorerMessage::ImportPicked(key, files.flatten().unwrap_or_default()))
                    },
                );
            }
            ExplorerMessage::ImportPicked(key, files) => {
                let paths: Vec<String> = files.iter().map(|p| clean(p)).collect();
                return read_files(key, paths, false);
            }
            ExplorerMessage::Embed(key, paths) => {
                self.close_popups();
                return read_files(key, paths, true);
            }
            ExplorerMessage::FilesRead { key, embed, files, failed } => {
                let count = files.len();
                let edit = if embed { PackageEdit::Embed(files) } else { PackageEdit::Import(files) };
                let applied = match self.package_doc_mut(key) {
                    Some(doc) if count > 0 => doc.apply_package(edit).map(Some),
                    _ => Ok(None),
                };
                let mut said = match applied {
                    Ok(Some(result)) => result.notice.unwrap_or_else(|| {
                        let what = if embed { "is held by" } else { "went into" };
                        format!("{count} file(s): each {what} the package, and saved in its file")
                    }),
                    Ok(None) => String::new(),
                    Err(e) => e,
                };
                if !failed.is_empty() {
                    said = format!("{said}{}could not be read: {}", if said.is_empty() { "" } else { "; " }, failed.join(", "));
                }
                if !said.is_empty() {
                    self.set_status(said, !failed.is_empty());
                }
            }
            ExplorerMessage::SaveResourceAs(key, path) => {
                self.close_popups();
                let name = package::file_name(&path);
                let folder = self.dialog_folder(self.package_doc(key).and_then(|d| d.path()).map(PathBuf::as_path), true);
                return Task::perform(
                    blocking(move || {
                        let mut dialog = rfd::FileDialog::new().set_title("Save the file, and link to it").set_file_name(name);
                        if let Some(folder) = folder {
                            dialog = dialog.set_directory(folder);
                        }
                        dialog.save_file()
                    }),
                    move |to| Message::Explorer(ExplorerMessage::ResourceSavePicked(key, path.clone(), to.flatten())),
                );
            }
            ExplorerMessage::ResourceSavePicked(key, path, Some(to)) => {
                let Some(bytes) = brewed_texture_core::files::embedded(&path) else {
                    self.set_status(format!("{} is not in its package", package::file_name(&path)), true);
                    return Task::none();
                };
                let to = clean(&to);
                return Task::perform(
                    blocking({
                        let to = to.clone();
                        move || brewed_texture_core::files::write_whole(Path::new(&to), &bytes)
                    }),
                    move |written| {
                        let result = written.unwrap_or_else(|| Err("The file could not be written".into())).map(|()| to.clone());
                        Message::Explorer(ExplorerMessage::ResourceSaved(key, path.clone(), result))
                    },
                );
            }
            ExplorerMessage::ResourceSavePicked(_, _, None) => {}
            ExplorerMessage::ResourceSaved(key, from, Ok(to)) => {
                if let Some(doc) = self.package_doc_mut(key) {
                    match doc.apply_package(PackageEdit::Relink { from, to: to.clone() }) {
                        Ok(_) => self.set_status(format!("Saved as {to}: the package links to it now"), false),
                        Err(e) => self.set_status(e, true),
                    }
                }
            }
            ExplorerMessage::ResourceSaved(_, _, Err(e)) => self.set_status(e, true),
            ExplorerMessage::RemoveUnused(key) => {
                self.close_popups();
                if let Some(doc) = self.package_doc_mut(key) {
                    match doc.apply_package(PackageEdit::RemoveUnused) {
                        Ok(result) => {
                            let said = result.notice.unwrap_or_else(|| "Every file of the package is read by a node".into());
                            self.set_status(said, false);
                        }
                        Err(e) => self.set_status(e, true),
                    }
                }
            }
            ExplorerMessage::RemoveResource(key, path) => {
                self.close_popups();
                if let Some(doc) = self.package_doc_mut(key)
                    && let Err(e) = doc.apply_package(PackageEdit::RemoveResource(path))
                {
                    self.set_status(e, true);
                }
            }
            ExplorerMessage::Locate(key, path) => {
                self.close_popups();
                return self.locate(key, path);
            }
            ExplorerMessage::CollectFiles(key) => {
                self.close_popups();
                return self.collect_files(key);
            }
            ExplorerMessage::FilesCollected(key, collected) => self.files_collected(key, collected),
            ExplorerMessage::Located(key, from, Some(to)) => self.located(key, from, to),
            ExplorerMessage::Located(_, _, None) => {}
            ExplorerMessage::Context(target) => {
                self.ui.menu = None;
                self.ui.add_menu = None;
                self.ui.choice_menu = None;
                self.ui.context = Some(ContextMenu { at: self.ui.cursor, target });
            }
            ExplorerMessage::AddResourceNode(path, kind, pos) => self.add_resource_node(path, kind, pos),
        }
        Task::none()
    }

    /// Add the node that reads the file at `path`.
    pub(super) fn add_resource_node(&mut self, path: String, kind: ResourceKind, pos: Option<[f32; 2]>) {
        let pos = pos.unwrap_or_else(|| {
            let c = self.ui.graph_view.center();
            [c[0] - 80.0, c[1] - 40.0]
        });
        let (node, key) = match kind {
            ResourceKind::Bitmap => (NodeKind::BITMAP, "path"),
            ResourceKind::Svg => (NodeKind::SVG, "path"),
            ResourceKind::Font => (NodeKind::TEXT, "font"),
        };
        let edit = Edit::AddNodeWith { kind: node, pos, params: vec![(key.into(), ParamValue::Text(path))] };
        if self.apply(edit) {
            self.doc.seal();
            let created = self.ui.selection.first().copied();
            self.set_preview(created);
        }
    }

    /// What a panel item dragged onto the graph becomes.
    pub(super) fn add_package_payload(&mut self, payload: DragPayload, pos: [f32; 2]) -> Task<Message> {
        match payload {
            DragPayload::Graph { key, graph, .. } => self.add_graph_node(key, graph, pos),
            DragPayload::Resource { path, kind, .. } => self.add_resource_node(path, kind, Some(pos)),
            DragPayload::Library { package, graph, name } => return self.add_library_graph(package, graph, name, pos),
            _ => {}
        }
        Task::none()
    }
}
