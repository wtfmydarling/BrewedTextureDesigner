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

use iced::{Task, Vector};

use brewed_texture_core::package::{self, Flat, Library, Missing, MissingKind, Package, PackageEdit, ResourceKind};
use brewed_texture_core::project::{self, ViewState};
use brewed_texture_core::{
    Document, Edit, Graph, GraphId, GraphRef, Instance, Interface, NodeId, NodeKind, ParamValue,
};

use super::{App, Collected, ContextMenu, Dialog, DragPayload, ExplorerMessage, Message, MissingFiles, PendingAction, blocking};

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

/// Copy each file of `plan` (where it is, where its copy goes), making the folders; a file is
/// copied only where nothing is (a plan leaves out the names taken).
pub fn copy_files(plan: &[(String, String)]) -> Collected {
    let mut out = Collected::default();
    for (from, to) in plan {
        let target = Path::new(to);
        let made = target.parent().map_or(Ok(()), std::fs::create_dir_all).map_err(|e| e.to_string());
        let copied = made.and_then(|()| {
            if target.exists() {
                return Err("there is a file of that name there".to_string());
            }
            std::fs::copy(from, target).map(|_| ()).map_err(|e| e.to_string())
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

    fn package_doc_mut(&mut self, key: u64) -> Option<&mut Document> {
        if key == self.doc_key {
            Some(&mut self.doc)
        } else {
            self.shelf.iter_mut().find(|p| p.key == key).map(|p| &mut p.doc)
        }
    }

    fn all_docs(&self) -> impl Iterator<Item = &Document> {
        std::iter::once(&self.doc).chain(self.shelf.iter().map(|p| &p.doc))
    }

    /// The package open in the Explorer that is the file at `path`.
    fn open_key(&self, path: &str) -> Option<u64> {
        self.packages().into_iter().find(|(_, d)| d.key().as_deref() == Some(path)).map(|(k, _)| k)
    }

    fn next_key(&mut self) -> u64 {
        self.next_key += 1;
        self.next_key
    }

    /// Where the view of the graph being edited is.
    fn view_state(&self) -> ViewState {
        ViewState {
            preview: self.ui.preview.node,
            graph_pan: Some([self.ui.graph_view.pan.x, self.ui.graph_view.pan.y]),
            graph_zoom: Some(self.ui.graph_view.zoom),
        }
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
    fn add_package(
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
        let before = std::mem::replace(&mut self.doc, Document::new(package, path.clone()));
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

    /// Open the package file at `path` in the Explorer (or edit it, when it is open already),
    /// and say what it names that is not there.
    pub(super) fn open_package(&mut self, path: PathBuf) {
        let key = clean(&path);
        if let Some(open) = self.open_key(&key) {
            self.activate(open, None);
            return;
        }
        match project::load(Path::new(&key)) {
            Ok(loaded) => {
                let notes = loaded.notes.clone();
                self.add_package(loaded.package, Some(PathBuf::from(&key)), loaded.views, loaded.open);
                let mut msg = format!("Opened {key}");
                if !notes.is_empty() {
                    msg.push_str(&format!(" ({})", notes.join("; ")));
                }
                self.set_status(msg, false);
                self.ask_for_missing();
            }
            Err(e) => self.set_status(e, true),
        }
    }

    /// Open the file at `path` in place of the package being edited (the unsaved changes
    /// dialog has been answered).
    pub(super) fn open_path(&mut self, path: PathBuf) {
        let key = clean(&path);
        match project::load(Path::new(&key)) {
            Ok(loaded) => {
                let notes = loaded.notes.clone();
                // Open elsewhere in the Explorer already: that one goes, this one takes its place.
                if let Some(open) = self.open_key(&key).filter(|k| *k != self.doc_key) {
                    self.shelf.retain(|p| p.key != open);
                }
                self.replace_package(loaded.package, Some(PathBuf::from(&key)), loaded.views, loaded.open);
                let mut msg = format!("Opened {key}");
                if !notes.is_empty() {
                    msg.push_str(&format!(" ({})", notes.join("; ")));
                }
                self.set_status(msg, false);
                self.ask_for_missing();
            }
            Err(e) => self.set_status(e, true),
        }
    }

    /// A new, empty package in the Explorer.
    pub(super) fn new_package(&mut self) {
        let package = Package::new("Graph", Graph::default());
        let open = package.first();
        self.add_package(package, None, HashMap::new(), open);
    }

    /// The demo, as a package of its own in the Explorer.
    pub(super) fn open_demo(&mut self) {
        let (package, preview) = brewed_texture_core::demo::package();
        let open = package.first();
        let views = [(open, ViewState { preview, ..ViewState::default() })].into_iter().collect();
        self.add_package(package, None, views, open);
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
            let package = Package::new("Graph", Graph::default());
            let open = package.first();
            self.replace_package(package, None, HashMap::new(), open);
        }
        self.views.retain(|(k, _), _| *k != key);
        self.ui.explorer_collapsed.remove(&key);
        // A package still used by the others is read from its file again.
        self.load_dependencies();
    }

    /// The packages with unsaved changes, in the Explorer's order.
    pub(super) fn dirty_packages(&self) -> Vec<u64> {
        self.packages().into_iter().filter(|(_, d)| d.is_dirty()).map(|(k, _)| k).collect()
    }

    /// Where the view of each graph of the package being edited is, to save with it.
    pub(super) fn views_to_save(&self) -> HashMap<GraphId, ViewState> {
        let mut views: HashMap<GraphId, ViewState> =
            self.views.iter().filter(|((key, _), _)| *key == self.doc_key).map(|((_, g), v)| (*g, v.clone())).collect();
        views.insert(self.doc.current(), self.view_state());
        views
    }

    /// The package being edited was saved at `path`: whatever was read of that file is its
    /// copy now.
    pub(super) fn saved_as(&mut self, path: &Path) {
        let key = path.display().to_string();
        self.forget_file(&key);
        // Another package open in the Explorer from the same file is this one now.
        let doc_key = self.doc_key;
        self.shelf.retain(|p| p.key == doc_key || p.doc.key().as_deref() != Some(key.as_str()));
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
        let Some(source) = self.package_doc(key) else { return };
        let Some(entry) = source.package().graph(graph) else { return };
        let package = if key == self.doc_key {
            if graph == self.doc.current() {
                self.set_status("A graph cannot use itself", true);
                return;
            }
            None
        } else {
            match source.key() {
                Some(path) => Some(path),
                None => {
                    let title = source.title();
                    self.set_status(format!("Save {title} first: another package finds its graphs by its file"), true);
                    return;
                }
            }
        };
        let instance = Instance {
            graph: GraphRef { package, graph, name: entry.name.clone() },
            interface: Interface::of(&entry.graph),
        };
        if self.apply(Edit::AddInstance { instance, pos }) {
            self.doc.seal();
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
                let before = std::mem::replace(&mut self.doc, Document::new(package, Some(PathBuf::from(&path))));
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
    pub(super) fn make_subgraph(&mut self) {
        let ids = self.selected_ids();
        if ids.is_empty() {
            self.set_status("Select the nodes to put in a graph of their own", false);
            return;
        }
        self.close_popups();
        let name = self.doc.package().unique_graph_name(&format!("{} Part", self.doc.entry().name));
        match self.doc.apply_package(PackageEdit::MakeSubgraph { graph: self.doc.current(), nodes: ids.clone(), name })
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
        let folder = Path::new(&path).parent().filter(|d| d.is_dir()).map(PathBuf::from);
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

    // ------------------------------------------------------------------ the Explorer

    pub(super) fn explorer_message(&mut self, msg: ExplorerMessage) -> Task<Message> {
        match msg {
            ExplorerMessage::Toggle(key) => {
                if !self.ui.explorer_collapsed.remove(&key) {
                    self.ui.explorer_collapsed.insert(key);
                }
            }
            ExplorerMessage::OpenGraph(key, graph) => {
                self.close_popups();
                self.activate(key, Some(graph));
            }
            ExplorerMessage::NewGraph(key) => {
                self.close_popups();
                let Some(doc) = self.package_doc_mut(key) else { return Task::none() };
                match doc.apply_package(PackageEdit::AddGraph { name: "Graph".into(), graph: Graph::default() }) {
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
                return Task::perform(
                    blocking(move || {
                        let mut dialog = rfd::FileDialog::new()
                            .set_title("Link files to the package")
                            .add_filter("Images, vector graphics and fonts", &extensions);
                        for kind in ResourceKind::ALL {
                            dialog = dialog.add_filter(kind.name(), kind.extensions());
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
    pub(super) fn add_package_payload(&mut self, payload: DragPayload, pos: [f32; 2]) -> bool {
        match payload {
            DragPayload::Graph { key, graph, .. } => {
                self.add_graph_node(key, graph, pos);
                true
            }
            DragPayload::Resource { path, kind } => {
                self.add_resource_node(path, kind, Some(pos));
                true
            }
            _ => false,
        }
    }
}
