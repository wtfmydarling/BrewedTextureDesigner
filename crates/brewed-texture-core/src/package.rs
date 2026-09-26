//! A package: what a project file holds. Graphs, any of which can be used as a node in the
//! others (see [`crate::instance`]) and in the graphs of other packages, and the files they
//! use (images, vector graphics, fonts): linked where they are on disk, or held by the package
//! itself (embedded, see [`crate::files`]).
//!
//! In memory every path is absolute. A file stores them relative to itself wherever it can,
//! so that a project moved or copied with its files keeps finding them (see
//! [`relative_path`] and [`crate::project`]).
//!
//! Before a graph is computed, [`flatten`] lays out in it every graph it uses: the engine
//! only ever sees nodes of the node types.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::eval::{self, NodeEval, Params, VarSource};
use crate::graph::{Graph, IMAGE_INPUT_COLOR, Link, Node, NodeId, input_socket_key, output_socket_key, param_socket_key};
use crate::instance::{GraphId, GraphRef, Instance, Interface, Port, port_nodes};
use crate::nodes::{ImageKind, NodeKind, ParamKind, Role, Slot};
use crate::types::{DataType, ParamValue, Value};

// ---------------------------------------------------------------------------- package

/// A graph of a package.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphEntry {
    pub id: GraphId,
    /// Unique in the package: what the Explorer and the nodes that use the graph call it.
    pub name: String,
    /// Shared with the undo steps that have it as it is (see [`crate::Document`]): an edit
    /// copies only the graph it changes.
    pub graph: Arc<Graph>,
}

/// What a file of a package is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    /// An image (Bitmap node).
    Bitmap,
    /// Vector graphics (SVG node).
    Svg,
    /// A font (Text node).
    Font,
}

impl ResourceKind {
    pub const ALL: [ResourceKind; 3] = [ResourceKind::Bitmap, ResourceKind::Svg, ResourceKind::Font];

    pub fn name(self) -> &'static str {
        match self {
            ResourceKind::Bitmap => "Bitmap",
            ResourceKind::Svg => "Vector graphics",
            ResourceKind::Font => "Font",
        }
    }

    /// The file extensions of the kind, lower case.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            ResourceKind::Bitmap => crate::nodes::PICTURE_FILES,
            ResourceKind::Svg => &["svg"],
            ResourceKind::Font => &["ttf", "otf", "ttc"],
        }
    }

    /// What the file at `path` is, by its extension.
    pub fn of_path(path: &str) -> Option<ResourceKind> {
        let ext = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
        ResourceKind::ALL.into_iter().find(|k| k.extensions().contains(&ext.as_str()))
    }
}

/// A file a package uses: linked where it is on disk, or held by the package (embedded).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Resource {
    /// Where it is on disk; for an embedded file, the name the package holds it by (see
    /// [`crate::files`]).
    pub path: String,
    pub kind: ResourceKind,
    /// The file itself, when the package holds it: written into the package's file.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "crate::files::base64_bytes")]
    pub data: Option<Arc<Vec<u8>>>,
}

/// Two resources are the same when they say the same file: the name of an embedded one says
/// its bytes.
impl PartialEq for Resource {
    fn eq(&self, other: &Resource) -> bool {
        self.path == other.path && self.kind == other.kind && self.data.is_some() == other.data.is_some()
    }
}

impl Eq for Resource {}

impl Resource {
    /// A file linked where it is on disk.
    pub fn linked(path: String, kind: ResourceKind) -> Resource {
        Resource { path, kind, data: None }
    }

    /// A file the package holds, `bytes`, that came as `name`: its embedded name, which
    /// [`crate::files`] keeps its bytes by.
    pub fn embedded(name: &str, kind: ResourceKind, bytes: Arc<Vec<u8>>) -> Resource {
        let path = crate::files::embedded_path(name, &bytes);
        crate::files::keep(&path, &bytes);
        Resource { path, kind, data: Some(bytes) }
    }

    /// Its file name, as the Explorer shows it.
    pub fn name(&self) -> String {
        file_name(&self.path)
    }

    /// Does the package hold the file itself?
    pub fn is_embedded(&self) -> bool {
        self.data.is_some()
    }
}

/// The name of the file at `path`.
pub fn file_name(path: &str) -> String {
    Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().to_string())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Package {
    /// At least one.
    pub graphs: Vec<GraphEntry>,
    pub resources: Vec<Resource>,
    /// The id the next graph gets. Never one a graph had: a node that used a graph that is
    /// gone does not use another in its place.
    next_graph: u64,
}

impl Package {
    /// A package with one graph.
    pub fn new(name: &str, graph: Graph) -> Package {
        let mut package = Package { graphs: Vec::new(), resources: Vec::new(), next_graph: 1 };
        package.add_graph(name, graph);
        package.sync_resources();
        package
    }

    /// Put together from what a file says (`next_graph`: the id the next graph gets, see
    /// [`Package::next_graph`]); [`Package::normalize`] makes it whole.
    pub fn from_parts(graphs: Vec<GraphEntry>, resources: Vec<Resource>, next_graph: u64) -> Package {
        let mut package = Package { graphs, resources, next_graph };
        package.normalize();
        package
    }

    /// The id the next graph gets: above the ids of the graphs that are, and of those that
    /// were (a file keeps it).
    pub fn next_graph(&self) -> u64 {
        self.next_graph
    }

    pub fn graph(&self, id: GraphId) -> Option<&GraphEntry> {
        self.graphs.iter().find(|g| g.id == id)
    }

    pub fn graph_mut(&mut self, id: GraphId) -> Option<&mut GraphEntry> {
        self.graphs.iter_mut().find(|g| g.id == id)
    }

    pub fn first(&self) -> GraphId {
        self.graphs.first().map_or(GraphId(1), |g| g.id)
    }

    /// The graph a node uses: by its id, or else by its name.
    pub fn find(&self, r: &GraphRef) -> Option<&GraphEntry> {
        self.graph(r.graph).or_else(|| self.graphs.iter().find(|g| g.name == r.name))
    }

    pub fn unique_graph_name(&self, base: &str) -> String {
        let base = base.trim();
        let base = if base.is_empty() { "Graph" } else { base };
        if !self.graphs.iter().any(|g| g.name == base) {
            return base.to_string();
        }
        let stem = base.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end();
        let stem = if stem.is_empty() { base } else { stem };
        (2..).map(|i| format!("{stem} {i}")).find(|n| !self.graphs.iter().any(|g| &g.name == n)).expect("a free name")
    }

    /// Add a graph, named `name` or, when a graph has that name, after it.
    pub fn add_graph(&mut self, name: &str, graph: Graph) -> GraphId {
        let id = GraphId(self.next_graph);
        self.next_graph += 1;
        let name = self.unique_graph_name(name);
        self.graphs.push(GraphEntry { id, name, graph: Arc::new(graph) });
        id
    }

    /// Every file the graphs of the package read (a Bitmap's image, a font, the picture of an
    /// image input…): the graph, what reads it, and the path.
    pub fn files(&self) -> impl Iterator<Item = (GraphId, Reader<'_>, &str)> {
        self.graphs.iter().flat_map(|g| {
            let nodes = g.graph.nodes.iter().flat_map(move |n| {
                n.param_defs()
                    .into_iter()
                    .filter(|p| matches!(p.kind, ParamKind::File { .. } | ParamKind::Image { .. }))
                    .map(move |p| (g.id, Reader::Node(n), n.text(p.key)))
            });
            // The pictures of its image inputs, and those they have by default.
            let inputs = g.graph.variables.iter().flat_map(move |v| {
                let pictures = v.image.as_ref().map(|i| [i.picture.as_str(), i.default.as_str()]);
                pictures.into_iter().flatten().map(move |path| (g.id, Reader::Input(v.name.as_str()), path))
            });
            nodes.chain(inputs).filter(|(.., path)| !path.trim().is_empty())
        })
    }

    /// The files of the package: those it lists, and every file a node reads, which it lists
    /// from then on (an embedded one with its bytes: a node pasted from another package brings
    /// the file it reads). True if a file was added.
    pub fn sync_resources(&mut self) -> bool {
        let used: Vec<String> = self.files().map(|(.., path)| path.to_string()).collect();
        let mut added = false;
        for path in used {
            if self.resources.iter().any(|r| r.path == path) {
                continue;
            }
            let Some(kind) = ResourceKind::of_path(&path) else { continue };
            let data = crate::files::is_embedded(&path).then(|| crate::files::embedded(&path)).flatten();
            self.resources.push(Resource { path, kind, data });
            added = true;
        }
        added
    }

    /// The files of the package no node reads.
    pub fn unused_resources(&self) -> Vec<&Resource> {
        let read: HashSet<&str> = self.files().map(|(.., path)| path).collect();
        self.resources.iter().filter(|r| !read.contains(r.path.as_str())).collect()
    }

    /// Every file the package uses: those it lists and those its nodes read, each once, in
    /// that order.
    pub fn all_files(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        self.resources
            .iter()
            .map(|r| r.path.clone())
            .chain(self.files().map(|(.., path)| path.to_string()))
            .filter(|p| seen.insert(p.clone()))
            .collect()
    }

    /// The package files whose graphs the nodes of this one use.
    pub fn dependencies(&self) -> BTreeSet<String> {
        self.graphs
            .iter()
            .flat_map(|g| g.graph.instances())
            .filter_map(|n| n.instance.as_ref()?.graph.package.clone())
            .collect()
    }

    /// Change every path of the package: of its files, of the files its graphs read and of the
    /// packages whose graphs it uses.
    pub fn map_paths(&mut self, f: impl Fn(&str) -> String) {
        let map = |path: &mut String| {
            if !path.trim().is_empty() {
                *path = f(path);
            }
        };
        for r in &mut self.resources {
            r.path = f(&r.path);
        }
        for entry in &mut self.graphs {
            let graph = Arc::make_mut(&mut entry.graph);
            for node in &mut graph.nodes {
                let files: Vec<String> = node
                    .param_defs()
                    .iter()
                    .filter(|p| matches!(p.kind, ParamKind::File { .. } | ParamKind::Image { .. }))
                    .map(|p| p.key.to_string())
                    .collect();
                for key in files {
                    if let Some(slot) = node.param_mut(&key)
                        && let ParamValue::Text(path) = &mut slot.value
                    {
                        map(path);
                    }
                }
                if let Some(path) = node.instance.as_mut().and_then(|i| i.graph.package.as_mut()) {
                    *path = f(path);
                }
            }
            for image in graph.variables.iter_mut().filter_map(|v| v.image.as_mut()) {
                map(&mut image.picture);
                map(&mut image.default);
            }
        }
    }

    /// The files the package names that are not there: its own files, the files its nodes
    /// read, and the packages whose graphs it uses.
    pub fn missing(&self) -> Vec<Missing> {
        let mut out: Vec<Missing> = Vec::new();
        let mut seen = HashSet::new();
        for r in &self.resources {
            if seen.insert(r.path.clone()) && r.data.is_none() && !crate::files::is_there(&r.path) {
                out.push(Missing { path: r.path.clone(), what: MissingKind::Resource(r.kind), graphs: Vec::new() });
            }
        }
        for (.., path) in self.files() {
            if seen.insert(path.to_string())
                && !crate::files::is_there(path)
                && let Some(kind) = ResourceKind::of_path(path)
            {
                out.push(Missing { path: path.to_string(), what: MissingKind::Resource(kind), graphs: Vec::new() });
            }
        }
        for node in self.graphs.iter().flat_map(|g| g.graph.instances()) {
            let Some(r) = node.instance.as_deref().map(|i| &i.graph) else { continue };
            let Some(path) = &r.package else { continue };
            if Path::new(path).is_file() {
                continue;
            }
            match out.iter_mut().find(|m| &m.path == path) {
                Some(m) => {
                    if !m.graphs.contains(&r.name) {
                        m.graphs.push(r.name.clone());
                    }
                }
                None => {
                    out.push(Missing { path: path.clone(), what: MissingKind::Package, graphs: vec![r.name.clone()] })
                }
            }
        }
        out
    }

    /// Make what a file says whole: at least one graph, graphs of their own ids and names, and
    /// the files the nodes read listed.
    pub fn normalize(&mut self) {
        let mut ids = HashSet::new();
        self.graphs.retain(|g| ids.insert(g.id));
        if self.graphs.is_empty() {
            self.graphs.push(GraphEntry { id: GraphId(1), name: "Graph".into(), graph: Arc::new(Graph::default()) });
        }
        let mut names = HashSet::new();
        for entry in &mut self.graphs {
            let mut name = entry.name.trim().to_string();
            if name.is_empty() {
                name = "Graph".into();
            }
            if !names.insert(name.clone()) {
                let stem = name.clone();
                name = (2..).map(|i| format!("{stem} {i}")).find(|n| !names.contains(n)).expect("a free name");
                names.insert(name.clone());
            }
            entry.name = name;
        }
        self.next_graph = self.next_graph.max(self.graphs.iter().map(|g| g.id.0).max().unwrap_or(0) + 1);
        // The files the package holds: kept for whatever reads them, under the name their bytes
        // give them (a file edited by hand could say another).
        for r in &mut self.resources {
            match &r.data {
                Some(bytes) => {
                    let named = crate::files::embedded_path(&r.path, bytes);
                    if named != r.path {
                        r.data = None;
                    } else {
                        crate::files::keep(&r.path, bytes);
                    }
                }
                None if crate::files::is_embedded(&r.path) => r.data = crate::files::embedded(&r.path),
                None => {}
            }
        }
        let mut paths = HashSet::new();
        self.resources.retain(|r| !r.path.trim().is_empty() && paths.insert(r.path.clone()));
        self.sync_resources();
    }
}

/// What reads a file of a package (see [`Package::files`]).
#[derive(Clone, Copy, Debug)]
pub enum Reader<'a> {
    /// A parameter of a node: the file of a Bitmap, the picture a node gives the image input
    /// of the graph it uses…
    Node(&'a Node),
    /// An image input of the graph, by its name: its picture, or the one it has by default.
    Input(&'a str),
}

/// A file a package names that is not there.
#[derive(Clone, Debug, PartialEq)]
pub struct Missing {
    pub path: String,
    pub what: MissingKind,
    /// For a package: the graphs of it that are used.
    pub graphs: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingKind {
    Resource(ResourceKind),
    Package,
}

// ---------------------------------------------------------------------------- edits

/// A change of a package as a whole (a change inside one of its graphs is an [`crate::Edit`]).
#[derive(Clone, Debug, PartialEq)]
pub enum PackageEdit {
    /// A new graph, named `name` or after it.
    AddGraph {
        name: String,
        graph: Graph,
    },
    RemoveGraph(GraphId),
    RenameGraph {
        id: GraphId,
        name: String,
    },
    DuplicateGraph(GraphId),
    /// Link files to the package.
    AddResources(Vec<String>),
    /// Take a file out of the package; the nodes that read it keep reading it.
    RemoveResource(String),
    /// A file was moved: every file of the package, every node and every graph used from
    /// another package that said `from` says `to`.
    Relink {
        from: String,
        to: String,
    },
    /// Files were copied (their files collected, see [`collect_plan`]): every path that said
    /// the first of a pair says the second.
    Relocate(Vec<(String, String)>),
    /// Hold linked files in the package itself (each file of the package and what it holds,
    /// read by whoever asks): every node that read one reads it there.
    Embed(Vec<(String, Arc<Vec<u8>>)>),
    /// Hold files in the package itself (the name each came as, and what it holds).
    Import(Vec<(String, Arc<Vec<u8>>)>),
    /// Take out the files no node reads.
    RemoveUnused,
    /// Put nodes of a graph in a graph of their own, and use that graph in their place (see
    /// [`graph_from_selection`]).
    GraphFromSelection {
        graph: GraphId,
        nodes: Vec<NodeId>,
        name: String,
    },
}

/// What a package edit did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PackageResult {
    pub changed: bool,
    /// The graph it made.
    pub graph: Option<GraphId>,
    /// The nodes it made, in the graph it changed.
    pub created: Vec<NodeId>,
    pub notice: Option<String>,
}

impl PackageEdit {
    pub fn label(&self) -> &'static str {
        match self {
            PackageEdit::AddGraph { .. } => "New Graph",
            PackageEdit::RemoveGraph(_) => "Delete Graph",
            PackageEdit::RenameGraph { .. } => "Rename Graph",
            PackageEdit::DuplicateGraph(_) => "Duplicate Graph",
            PackageEdit::AddResources(_) => "Link Files",
            PackageEdit::RemoveResource(_) => "Remove File",
            PackageEdit::Relink { .. } => "Relink",
            PackageEdit::Relocate(_) => "Collect Files",
            PackageEdit::Embed(_) => "Embed Files",
            PackageEdit::Import(_) => "Import Files",
            PackageEdit::RemoveUnused => "Remove Unused Files",
            PackageEdit::GraphFromSelection { .. } => "Create Graph from Selection",
        }
    }

    /// Validate and apply. On error the package is left unchanged.
    pub fn apply(self, package: &mut Package) -> Result<PackageResult, String> {
        let changed = PackageResult { changed: true, ..Default::default() };
        Ok(match self {
            PackageEdit::AddGraph { name, graph } => {
                let id = package.add_graph(&name, graph);
                package.sync_resources();
                PackageResult { graph: Some(id), ..changed }
            }
            PackageEdit::RemoveGraph(id) => {
                let name = package.graph(id).ok_or("The graph no longer exists")?.name.clone();
                if package.graphs.len() == 1 {
                    return Err("A package has at least one graph".into());
                }
                package.graphs.retain(|g| g.id != id);
                // Nodes that used it show nothing now; they say so.
                let users = package
                    .graphs
                    .iter()
                    .flat_map(|g| g.graph.instances())
                    .filter(|n| n.instance.as_deref().is_some_and(|i| i.graph.package.is_none() && i.graph.graph == id))
                    .count();
                let notice = (users > 0).then(|| format!("{users} node(s) used '{name}': they make nothing now"));
                PackageResult { notice, ..changed }
            }
            PackageEdit::RenameGraph { id, name } => {
                let current = package.graph(id).ok_or("The graph no longer exists")?.name.clone();
                let name = name.trim().to_string();
                if name.is_empty() {
                    return Err("A graph needs a name".into());
                }
                if name == current {
                    return Ok(PackageResult::default());
                }
                if package.graphs.iter().any(|g| g.id != id && g.name == name) {
                    return Err(format!("The package has a graph named '{name}' already"));
                }
                package.graph_mut(id).expect("checked").name = name;
                changed
            }
            PackageEdit::DuplicateGraph(id) => {
                let entry = package.graph(id).ok_or("The graph no longer exists")?;
                let (name, graph) = (entry.name.clone(), Graph::clone(&entry.graph));
                let new = package.add_graph(&name, graph);
                PackageResult { graph: Some(new), ..changed }
            }
            PackageEdit::AddResources(paths) => {
                let mut added = 0;
                let mut unknown = Vec::new();
                for path in paths {
                    if package.resources.iter().any(|r| r.path == path) {
                        continue;
                    }
                    match ResourceKind::of_path(&path) {
                        Some(kind) => {
                            package.resources.push(Resource::linked(path, kind));
                            added += 1;
                        }
                        None => unknown.push(file_name(&path)),
                    }
                }
                if added == 0 && unknown.is_empty() {
                    return Ok(PackageResult::default());
                }
                if added == 0 {
                    return Err(format!("Not a file the package can use: {}", unknown.join(", ")));
                }
                let notice =
                    (!unknown.is_empty()).then(|| format!("Not files the package can use: {}", unknown.join(", ")));
                PackageResult { notice, ..changed }
            }
            PackageEdit::RemoveResource(path) => {
                let before = package.resources.len();
                if package.files().any(|(.., p)| p == path) {
                    return Err(format!("The graphs read {}: give them another file first", file_name(&path)));
                }
                package.resources.retain(|r| r.path != path);
                if package.resources.len() == before { PackageResult::default() } else { changed }
            }
            PackageEdit::Relink { from, to } => {
                if from == to {
                    return Ok(PackageResult::default());
                }
                package.map_paths(|p| if p == from { to.clone() } else { p.to_string() });
                for r in package.resources.iter_mut().filter(|r| r.path == to && !crate::files::is_embedded(&to)) {
                    r.data = None;
                }
                let mut seen = HashSet::new();
                package.resources.retain(|r| seen.insert(r.path.clone()));
                changed
            }
            PackageEdit::Embed(files) => {
                let mut embedded = 0;
                for (path, bytes) in files {
                    let Some(r) = package.resources.iter().find(|r| r.path == path && r.data.is_none()) else { continue };
                    let held = Resource::embedded(&path, r.kind, bytes);
                    let new = held.path.clone();
                    package.map_paths(|p| if p == path { new.clone() } else { p.to_string() });
                    if let Some(r) = package.resources.iter_mut().find(|r| r.path == new) {
                        *r = held;
                    }
                    embedded += 1;
                }
                if embedded == 0 {
                    return Ok(PackageResult::default());
                }
                let mut seen = HashSet::new();
                package.resources.retain(|r| seen.insert(r.path.clone()));
                changed
            }
            PackageEdit::Import(files) => {
                let mut added = 0;
                let mut unknown = Vec::new();
                for (name, bytes) in files {
                    let Some(kind) = ResourceKind::of_path(&name) else {
                        unknown.push(file_name(&name));
                        continue;
                    };
                    let held = Resource::embedded(&name, kind, bytes);
                    if !package.resources.iter().any(|r| r.path == held.path) {
                        package.resources.push(held);
                        added += 1;
                    }
                }
                if added == 0 && unknown.is_empty() {
                    return Ok(PackageResult::default());
                }
                if added == 0 {
                    return Err(format!("Not a file the package can use: {}", unknown.join(", ")));
                }
                let notice =
                    (!unknown.is_empty()).then(|| format!("Not files the package can use: {}", unknown.join(", ")));
                PackageResult { notice, ..changed }
            }
            PackageEdit::RemoveUnused => {
                let unused: HashSet<String> = package.unused_resources().into_iter().map(|r| r.path.clone()).collect();
                if unused.is_empty() {
                    return Ok(PackageResult::default());
                }
                package.resources.retain(|r| !unused.contains(&r.path));
                PackageResult { notice: Some(format!("{} file(s) no node read are gone", unused.len())), ..changed }
            }
            PackageEdit::Relocate(moves) => {
                let moves: HashMap<String, String> = moves.into_iter().filter(|(from, to)| from != to).collect();
                if moves.is_empty() {
                    return Ok(PackageResult::default());
                }
                package.map_paths(|p| moves.get(p).cloned().unwrap_or_else(|| p.to_string()));
                let mut seen = HashSet::new();
                package.resources.retain(|r| seen.insert(r.path.clone()));
                changed
            }
            PackageEdit::GraphFromSelection { graph, nodes, name } => {
                let entry = package.graph(graph).ok_or("The graph no longer exists")?;
                let (inner, mut outer) = graph_from_selection(&entry.graph, &nodes)?;
                // (Made in a copy: the package stays as it is if the node cannot be placed.)
                let mut made = package.clone();
                let sub = made.add_graph(&name, inner);
                let entry = made.graph(sub).expect("just added");
                let instance = Instance::new(GraphRef { package: None, graph: sub, name: entry.name.clone() }, Interface::of(&entry.graph));
                let id = outer.place(instance)?;
                *Arc::make_mut(&mut made.graph_mut(graph).expect("checked").graph) = outer.graph;
                *package = made;
                PackageResult { graph: Some(sub), created: vec![id], ..changed }
            }
        })
    }
}

// ---------------------------------------------------------------------------- graphs of selections

/// What [`graph_from_selection`] leaves of the graph the nodes were taken from: the graph without
/// them, and what to connect to the node that takes their place.
pub struct Remainder {
    pub graph: Graph,
    pos: [f32; 2],
    /// Links into the new node: from a node of the graph, into an input (`i:`) or a parameter
    /// (`p:`) of the new graph.
    into: Vec<(NodeId, String, String)>,
    /// Links out of the new node: from an output of the new graph, into a node of the graph.
    out_of: Vec<(String, NodeId, String)>,
    /// Parameters of the new node driven by the links into it.
    exposed: Vec<String>,
}

impl Remainder {
    /// Put the node that uses the new graph where the nodes were, connected as they were.
    fn place(&mut self, instance: Instance) -> Result<NodeId, String> {
        let id = self.graph.add_instance(instance, self.pos);
        for key in &self.exposed {
            if let Some(slot) = self.graph.node_mut(id).and_then(|n| n.param_mut(key)) {
                slot.exposed = true;
            }
        }
        let cannot = |e: String| format!("The new node cannot be connected as the nodes were: {e}");
        for (from, socket, key) in &self.into {
            self.graph.connect(*from, socket, id, key).map_err(cannot)?;
        }
        for (key, to, socket) in &self.out_of {
            self.graph.connect(id, &output_socket_key(key), *to, socket).map_err(cannot)?;
        }
        Ok(id)
    }
}

/// Put the nodes `ids` of `graph` in a graph of their own. What arrives at them from the
/// other nodes arrives through Image Inputs (images) and input values (values); what leaves
/// them for the other nodes leaves through Outputs. The input values the nodes read go with
/// them, at the values they have; those the other nodes read too stay, and drive the new
/// graph's. Returns the new graph and what is left of `graph`: the same images, pixel for
/// pixel.
pub fn graph_from_selection(graph: &Graph, ids: &[NodeId]) -> Result<(Graph, Remainder), String> {
    let chosen: HashSet<NodeId> = ids.iter().copied().filter(|id| graph.contains(*id)).collect();
    if chosen.is_empty() {
        return Err("Select the nodes to put in a graph of their own".into());
    }
    for id in &chosen {
        let node = graph.node(*id).expect("chosen from the graph");
        if node.kind == NodeKind::OUTPUT || node.kind == NodeKind::IMAGE_INPUT {
            return Err(format!("{} nodes stay in the graph: leave them out", node.def().name));
        }
    }
    let links: Vec<&Link> = graph.live_links().collect();
    // A node between chosen ones (what one of them makes reaches another through it) would be
    // both before and after the node that takes their place.
    let between = nodes_between(graph, &links, &chosen);
    if !between.is_empty() {
        let names: Vec<&str> = between.iter().take(3).map(|n| n.def().name).collect();
        let (is, them) = if between.len() == 1 { ("is", "it") } else { ("are", "them") };
        return Err(format!(
            "{} {is} between the selected nodes: select {them} too, or leave out what comes after {them}",
            names.join(", ")
        ));
    }
    for l in &links {
        if chosen.contains(&l.from) && !chosen.contains(&l.to) {
            let from = graph.output_socket(l.from, &l.from_socket);
            if from.as_ref().is_some_and(|s| s.ty != DataType::Image) {
                let name = graph.node(l.from).map_or_else(String::new, |n| n.def().name.to_string());
                return Err(format!(
                    "A value leaves the selection ({name}): a graph gives images, leave the value nodes out"
                ));
            }
        }
    }

    let mut inner = Graph { resolution: graph.resolution, precision: graph.precision, ..Graph::default() };
    let nodes: Vec<&Node> = graph.nodes.iter().filter(|n| chosen.contains(&n.id)).collect();
    let bounds = |f: fn(&Node) -> f32| {
        nodes
            .iter()
            .map(|n| f(n))
            .fold(None, |a: Option<(f32, f32)>, v| Some(a.map_or((v, v), |(lo, hi)| (lo.min(v), hi.max(v)))))
    };
    let (x0, x1) = bounds(|n| n.pos[0]).expect("nodes");
    let (y0, _) = bounds(|n| n.pos[1]).expect("nodes");
    // The nodes as they are (ids of their own in the new graph, so their links stay as they
    // are), with the input values they read and those their expressions take by name.
    inner.nodes.extend(nodes.iter().map(|n| (*n).clone()));
    inner.next_id = inner.nodes.iter().map(|n| n.id.0).max().unwrap_or(0) + 1;
    let ev = eval::evaluate(graph);
    // The input values an expression reads by their names.
    let reads_by_name = |n: &Node| -> Vec<String> {
        if !n.is_expression_node() || !n.bool_value("auto_bind") {
            return Vec::new();
        }
        let mut vars = n.expression_vars();
        vars.retain(|v| {
            graph.link_into(n.id, &crate::graph::var_socket_key(v)).is_none()
                && graph.variable(v).is_some_and(|v| v.ty != DataType::Image)
        });
        vars
    };
    let mut needed: Vec<String> = Vec::new();
    for v in nodes.iter().filter_map(|n| n.variable.clone()) {
        if !needed.contains(&v) {
            needed.push(v);
        }
    }
    for n in &nodes {
        for v in reads_by_name(n) {
            if !needed.contains(&v) {
                // An Input node keeps it there (input values exist while one reads them).
                needed.push(v.clone());
                let reader = inner.add_node(
                    NodeKind::input_for(graph.variable(&v).expect("checked").ty).expect("a value type"),
                    [x0 - 260.0, y0 + 90.0 * needed.len() as f32],
                );
                inner.node_mut(reader).expect("just added").variable = Some(v);
            }
        }
    }
    for name in &needed {
        if let Some(v) = graph.variable(name)
            && inner.variable(name).is_none()
        {
            inner.variables.push(v.clone());
        }
    }
    for l in &links {
        if chosen.contains(&l.from) && chosen.contains(&l.to) {
            inner.links.push((*l).clone());
        }
    }

    let mut rest = graph.clone();
    rest.remove_nodes(&chosen.iter().copied().collect::<Vec<_>>());
    // The input values the nodes took that the other nodes read too stay, read by an Input node
    // (one of theirs, or a new one), and drive the new graph's.
    let mut kept: Vec<(NodeId, String)> = Vec::new();
    for name in &needed {
        let read = rest.variable_users(name).next().is_some() || rest.nodes.iter().any(|n| reads_by_name(n).contains(name));
        let Some(var) = graph.variable(name).filter(|_| read) else { continue };
        if rest.variable(name).is_none() {
            rest.variables.push(var.clone());
        }
        let theirs = rest.variable_users(name).next().map(|n| n.id);
        let reader = match theirs {
            Some(id) => id,
            None => {
                let at = [x0 - 220.0, y0 + 60.0 * kept.len() as f32];
                rest.add_variable_node(name, at).expect("the variable is there")
            }
        };
        kept.push((reader, name.clone()));
    }
    let mut remainder =
        Remainder { graph: rest, pos: [x0, y0], into: Vec::new(), out_of: Vec::new(), exposed: Vec::new() };
    for (reader, name) in kept {
        remainder.into.push((reader, "o:value".to_string(), param_socket_key(&name)));
        remainder.exposed.push(name);
    }

    // What arrives from outside: one input (or input value) per thing that arrives. An input
    // takes color when any socket what arrives there goes to takes it (or either kind); else it
    // takes grayscale, as the sockets do.
    let takes_color: HashSet<(NodeId, String)> = links
        .iter()
        .filter(|l| !chosen.contains(&l.from) && chosen.contains(&l.to))
        .filter(|l| graph.input_socket(l.to, &l.to_socket).is_some_and(|t| t.ty == DataType::Image && t.slot != Slot::Gray))
        .map(|l| (l.from, l.from_socket.clone()))
        .collect();
    let mut arriving: Vec<((NodeId, String), String)> = Vec::new();
    let mut row = 0.0;
    for l in links.iter().filter(|l| !chosen.contains(&l.from) && chosen.contains(&l.to)) {
        let source = (l.from, l.from_socket.clone());
        let target = graph.input_socket(l.to, &l.to_socket).ok_or("A link goes to a socket that is not there")?;
        let name = match arriving.iter().find(|(s, _)| *s == source) {
            Some((_, name)) => name.clone(),
            None => {
                let from = graph.node(l.from).map_or_else(
                    || "input".to_string(),
                    |n| crate::graph::sanitize_identifier(&n.def().name.to_lowercase()),
                );
                let name = if target.ty == DataType::Image {
                    let name = inner.unique_variable_name(&from);
                    let input = inner.add_node(NodeKind::IMAGE_INPUT, [x0 - 260.0, y0 + row]);
                    inner.node_mut(input).expect("just added").variable = Some(name.clone());
                    inner.sync_variables();
                    // What the input takes: what the sockets take, or what arrives when one
                    // takes either (known only to the engine: color, which gray reads as).
                    let color = takes_color.contains(&source);
                    if let Some(image) = inner.variable_mut(&name).and_then(|v| v.image.as_mut()) {
                        image.kind = if color { ImageKind::Color } else { ImageKind::Gray };
                    }
                    // Exposed on the node that takes the nodes' place: what arrived arrives there.
                    remainder.into.push((l.from, l.from_socket.clone(), param_socket_key(&name)));
                    remainder.exposed.push(name.clone());
                    name
                } else {
                    let ty = graph.output_socket(l.from, &l.from_socket).map_or(target.ty, |s| s.ty);
                    let ty = if DataType::VALUE_TYPES.contains(&ty) { ty } else { DataType::Float };
                    let name = inner.unique_variable_name(&from);
                    let reader = inner.add_node(NodeKind::input_for(ty).expect("a value type"), [x0 - 260.0, y0 + row]);
                    inner.node_mut(reader).expect("just added").variable = Some(name.clone());
                    inner.sync_variables();
                    // The value that arrives now, in the range of the parameter it drives: the new
                    // graph, on its own, is what the nodes were.
                    let range =
                        target.key.strip_prefix("p:").and_then(|key| graph.param_def(l.to, key)).map(|pd| pd.kind);
                    if let Some(var) = inner.variable_mut(&name) {
                        match range {
                            Some(ParamKind::Float { min, max, .. }) if var.ty == DataType::Float => {
                                (var.min, var.max) = (Value::Float(min), Value::Float(max));
                            }
                            Some(ParamKind::Int { min, max, .. }) if var.ty == DataType::Int => {
                                (var.min, var.max) = (Value::Int(min), Value::Int(max));
                            }
                            _ => {}
                        }
                        if let Some(v) = ev.value(l.from).and_then(|v| v.convert(var.ty)).filter(Value::is_finite) {
                            (var.min, var.max) = match (var.min, var.max, v) {
                                (Value::Float(lo), Value::Float(hi), Value::Float(x)) => {
                                    (Value::Float(lo.min(x)), Value::Float(hi.max(x)))
                                }
                                (Value::Int(lo), Value::Int(hi), Value::Int(x)) => {
                                    (Value::Int(lo.min(x)), Value::Int(hi.max(x)))
                                }
                                other => (other.0, other.1),
                            };
                            var.value = v.clamped(&var.min, &var.max);
                            var.default = var.value;
                        }
                    }
                    remainder.into.push((l.from, l.from_socket.clone(), param_socket_key(&name)));
                    remainder.exposed.push(name.clone());
                    name
                };
                row += 140.0;
                arriving.push((source, name.clone()));
                name
            }
        };
        let reader = if target.ty == DataType::Image {
            inner
                .nodes_of(NodeKind::IMAGE_INPUT)
                .find(|n| n.variable.as_deref() == Some(name.as_str()))
                .map(|n| (n.id, "o:output".to_string()))
        } else {
            inner
                .nodes
                .iter()
                .find(|n| n.variable.as_deref() == Some(name.as_str()))
                .map(|n| (n.id, "o:value".to_string()))
        };
        if let Some((from, socket)) = reader {
            let _ = inner.connect(from, &socket, l.to, &l.to_socket);
            // An exposed parameter that was driven from outside stays exposed, now inside.
        }
    }

    // What leaves for outside: one output per thing that leaves.
    let mut leaving: Vec<((NodeId, String), String)> = Vec::new();
    let mut row = 0.0;
    for l in links.iter().filter(|l| chosen.contains(&l.from) && !chosen.contains(&l.to)) {
        let source = (l.from, l.from_socket.clone());
        let name = match leaving.iter().find(|(s, _)| *s == source) {
            Some((_, name)) => name.clone(),
            None => {
                let from = graph.node(l.from).map_or_else(
                    || "output".to_string(),
                    |n| crate::graph::sanitize_identifier(&n.def().name.to_lowercase()),
                );
                let output = inner.add_node(NodeKind::OUTPUT, [x1 + 260.0, y0 + row]);
                let name = inner.unique_output_name(&from);
                inner.node_mut(output).expect("just added").param_mut("name").expect("a name").value =
                    ParamValue::Text(name.clone());
                let _ = inner.connect(l.from, &l.from_socket, output, "i:input");
                row += 220.0;
                leaving.push((source, name.clone()));
                name
            }
        };
        remainder.out_of.push((name, l.to, l.to_socket.clone()));
    }
    inner.sync_variables();
    inner.prune_links();
    Ok((inner, remainder))
}

/// The nodes of `graph` (with its `links`) that are not `chosen` but between chosen ones: what
/// one of those makes reaches another through them.
fn nodes_between<'a>(graph: &'a Graph, links: &[&Link], chosen: &HashSet<NodeId>) -> Vec<&'a Node> {
    let mut after: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    let mut before: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for l in links {
        after.entry(l.from).or_default().push(l.to);
        before.entry(l.to).or_default().push(l.from);
    }
    // The nodes that are not chosen reached from the chosen ones, one way.
    let reached = |next: &HashMap<NodeId, Vec<NodeId>>| {
        let mut seen: HashSet<NodeId> = HashSet::new();
        let mut stack: Vec<NodeId> = chosen.iter().copied().collect();
        while let Some(id) = stack.pop() {
            for n in next.get(&id).into_iter().flatten() {
                if !chosen.contains(n) && seen.insert(*n) {
                    stack.push(*n);
                }
            }
        }
        seen
    };
    let (downstream, upstream) = (reached(&after), reached(&before));
    graph.nodes.iter().filter(|n| downstream.contains(&n.id) && upstream.contains(&n.id)).collect()
}

// ---------------------------------------------------------------------------- flattening

/// Where the graphs the nodes use are found.
pub trait Library {
    /// The package at `path`; `None`: the package being flattened, when it has no file.
    fn package(&self, path: Option<&str>) -> Option<&Package>;
}

/// A graph with every graph it uses laid out in it, node by node: what the engine computes.
#[derive(Clone, Debug, Default)]
pub struct Flat {
    pub graph: Graph,
    /// Why a node that uses a graph makes nothing, or not all it should.
    pub problems: BTreeMap<NodeId, String>,
    /// For each node laid out from a graph, the node of the flattened graph that uses it
    /// (directly or deeper): its problems are that node's.
    pub owners: HashMap<NodeId, NodeId>,
}

/// The socket of the node a graph used as a node becomes in a flattened graph: what it shows
/// (its first output) arrives there.
pub const SHOWN: &str = "shows";

/// How deep graphs may use graphs.
const MAX_DEPTH: usize = 32;
/// Ids of nodes laid out from other graphs (see [`NodeId::is_inner`]).
const INNER: u64 = 1 << 63;

impl NodeId {
    /// Is this a node laid out from a graph used as a node, in a flattened graph (see
    /// [`flatten`])? Such ids never come from a graph of its own.
    pub fn is_inner(self) -> bool {
        self.0 & INNER != 0
    }
}

fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn mix(a: u64, b: u64) -> u64 {
    splitmix(a ^ splitmix(b))
}

/// What a graph used as a node was laid out as, seen from the graph that uses it.
#[derive(Default)]
struct Expanded {
    /// What each output gives: the node (and socket) of the flattened graph.
    outputs: HashMap<String, (NodeId, String)>,
    /// The Image Inputs of each input, in the flattened graph.
    inputs: HashMap<String, Vec<NodeId>>,
    /// What the first output gives: what the node shows.
    first: Option<(NodeId, String)>,
}

struct Builder<'a, L: Library> {
    lib: &'a L,
    flat: Graph,
    problems: BTreeMap<NodeId, String>,
    owners: HashMap<NodeId, NodeId>,
    /// What arrives at each Image Input of a graph used as a node, from the graph that uses it.
    redirects: HashMap<NodeId, (NodeId, String)>,
    /// What each Image Input shows while nothing arrives: what it takes, its picture (empty:
    /// none) and its color (see [`crate::graph::ImageInput`]).
    inputs: HashMap<NodeId, (ImageKind, String, [f32; 4])>,
    /// The graphs being laid out, outermost first: a graph cannot use itself.
    stack: Vec<(Option<String>, GraphId)>,
}

/// Lay out in the graph `id` of the package at `from` every graph it uses, as deep as they
/// go, node by node with the values its nodes give each (see [`Flat`]). `None` when there is
/// no such graph.
pub fn flatten<L: Library>(lib: &L, from: Option<&str>, id: GraphId) -> Option<Flat> {
    let entry = lib.package(from)?.graph(id)?;
    let graph = &entry.graph;
    let mut b = Builder {
        lib,
        flat: Graph {
            resolution: graph.resolution,
            precision: graph.precision,
            variables: graph.variables.clone(),
            ..Graph::default()
        },
        problems: BTreeMap::new(),
        owners: HashMap::new(),
        redirects: HashMap::new(),
        inputs: HashMap::new(),
        stack: vec![(from.map(str::to_string), id)],
    };
    b.expand(graph, from, None, None, None);
    Some(b.finish())
}

impl<L: Library> Builder<'_, L> {
    /// Lay out `graph`, of the package at `from`: at the top (`prefix` is `None`) as it is,
    /// else under the node `owner` of the top, with the input values `values` gives.
    fn expand(
        &mut self,
        graph: &Graph,
        from: Option<&str>,
        prefix: Option<u64>,
        values: Option<&Params>,
        owner: Option<NodeId>,
    ) -> Expanded {
        let set;
        let graph = match values {
            Some(values) => {
                let mut g = graph.clone();
                for var in &mut g.variables {
                    let given = values.get(&var.name);
                    match &mut var.image {
                        // A picture of the node's own; none, the graph's.
                        Some(image) => {
                            if let Some(ParamValue::Text(path)) = given
                                && !path.trim().is_empty()
                            {
                                image.picture = path.trim().to_string();
                            }
                        }
                        None => {
                            if let Some(v) = given.and_then(ParamValue::as_value).and_then(|v| v.convert(var.ty))
                                && v.is_finite()
                            {
                                var.value = v.clamped(&var.min, &var.max);
                            }
                        }
                    }
                }
                set = g;
                &set
            }
            None => graph,
        };
        let ev = eval::evaluate(graph);
        let id_of = |id: NodeId| prefix.map_or(id, |p| NodeId(mix(p, id.0) | INNER));
        let mut out = Expanded::default();
        let mut children: HashMap<NodeId, Expanded> = HashMap::new();
        // The Outputs that are the outputs of the graph used as a node (of two of one name, the
        // upper one, as its interface has them).
        let outputs: HashSet<NodeId> = match prefix {
            Some(_) => port_nodes(graph).iter().map(|n| n.id).collect(),
            None => HashSet::new(),
        };

        for node in &graph.nodes {
            match node.def().role {
                Role::Instance => {
                    let Some(instance) = node.instance.as_deref() else { continue };
                    let top = owner.unwrap_or(node.id);
                    match self.used(from, &instance.graph) {
                        Ok((entry_graph, entry_id, package)) => {
                            let key = (package.clone(), entry_id);
                            if self.stack.contains(&key) || self.stack.len() >= MAX_DEPTH {
                                self.problem(top, prefix, &format!("'{}' uses itself", instance.graph.name));
                                continue;
                            }
                            self.stack.push(key);
                            let child = self.expand(
                                &entry_graph,
                                package.as_deref(),
                                Some(mix(prefix.unwrap_or(0x5EED), node.id.0)),
                                ev.node(node.id).map(|e| &e.params),
                                Some(top),
                            );
                            self.stack.pop();
                            children.insert(node.id, child);
                        }
                        Err(e) => self.problem(top, prefix, &e),
                    }
                }
                // Values: at the top the graph's own nodes read them; inside a graph used as a
                // node they are in the parameters they drive already.
                Role::Value | Role::Variable(_) => {
                    if prefix.is_none() {
                        self.flat.nodes.push(node.clone());
                    }
                }
                // An output of a graph used as a node is where the node's output comes from.
                Role::Output if prefix.is_some() => {}
                Role::Image | Role::Output => {
                    let mut copy = node.clone();
                    if prefix.is_some() {
                        copy.id = id_of(node.id);
                        bake(&mut copy, ev.node(node.id));
                        self.owners.insert(copy.id, owner.expect("inside a graph used as a node"));
                    }
                    // An Image Input: the input value it reads says what it shows.
                    if let Some(name) = node.variable.as_deref().filter(|_| node.kind == NodeKind::IMAGE_INPUT) {
                        let var = graph.variable(name);
                        let image = var.and_then(|v| v.image.as_ref());
                        let color = match var.map(|v| v.value) {
                            Some(Value::Color(c)) => c,
                            _ => IMAGE_INPUT_COLOR,
                        };
                        let shows = (image.map_or(ImageKind::Gray, |i| i.kind), image.map_or_else(String::new, |i| i.picture.clone()), color);
                        self.inputs.insert(copy.id, shows);
                        if prefix.is_some() {
                            out.inputs.entry(name.to_string()).or_default().push(copy.id);
                        }
                    }
                    self.flat.nodes.push(copy);
                }
            }
        }

        for l in graph.live_links() {
            let (Some(a), Some(b)) = (graph.node(l.from), graph.node(l.to)) else { continue };
            if matches!(a.def().role, Role::Value | Role::Variable(_)) {
                // A value into a graph used as a node is in its parameters already.
                if prefix.is_none() && b.def().role != Role::Instance {
                    self.flat.links.push(l.clone());
                }
                continue;
            }
            let source = match a.def().role {
                Role::Instance => {
                    let key = l.from_socket.strip_prefix("o:").unwrap_or(&l.from_socket);
                    children.get(&a.id).and_then(|c| c.outputs.get(key).cloned())
                }
                _ => Some((id_of(a.id), l.from_socket.clone())),
            };
            let Some(source) = source else { continue };
            match b.def().role {
                Role::Instance => {
                    // (An image input is a parameter: exposed, images arrive at its socket.)
                    let key = l.to_socket.strip_prefix("p:").unwrap_or(&l.to_socket);
                    if let Some(inputs) = children.get(&b.id).and_then(|c| c.inputs.get(key)) {
                        for input in inputs {
                            self.redirects.insert(*input, source.clone());
                        }
                    }
                }
                Role::Output if prefix.is_some() => {
                    if outputs.contains(&b.id) {
                        out.outputs.insert(b.text("name").to_string(), source);
                    }
                }
                _ => self.flat.links.push(Link {
                    from: source.0,
                    from_socket: source.1,
                    to: id_of(b.id),
                    to_socket: l.to_socket.clone(),
                }),
            }
        }
        out.first = Interface::of(graph).outputs.iter().find_map(|p| out.outputs.get(&p.name).cloned());

        // At the top, a node that uses a graph shows its first output (that gives anything).
        if prefix.is_none() {
            for node in graph.instances() {
                let Some(first) = children.get(&node.id).and_then(|c| c.first.clone()) else { continue };
                let instance = node.instance.as_deref().expect("an instance");
                let shows = Instance::new(
                    instance.graph.clone(),
                    Interface { inputs: vec![Port { name: SHOWN.into(), kind: None }], ..Interface::default() },
                );
                self.flat.nodes.push(Node::of_graph(node.id, shows, node.pos));
                self.flat.links.push(Link {
                    from: first.0,
                    from_socket: first.1,
                    to: node.id,
                    to_socket: input_socket_key(SHOWN),
                });
            }
        }
        out
    }

    /// The graph `r` names, used in the package at `from`: the graph, its id and its package.
    fn used(&self, from: Option<&str>, r: &GraphRef) -> Result<(Arc<Graph>, GraphId, Option<String>), String> {
        let package = r.package.clone().or_else(|| from.map(str::to_string));
        let Some(found) = self.lib.package(package.as_deref()) else {
            return Err(format!(
                "The package {} of '{}' cannot be found",
                file_name(package.as_deref().unwrap_or("?")),
                r.name
            ));
        };
        let entry = found.find(r).ok_or_else(|| match &r.package {
            Some(path) => format!("{} has no graph '{}'", file_name(path), r.name),
            None => format!("The package has no graph '{}'", r.name),
        })?;
        Ok((entry.graph.clone(), entry.id, package))
    }

    fn problem(&mut self, top: NodeId, prefix: Option<u64>, what: &str) {
        let what = if prefix.is_some() { format!("Inside: {what}") } else { what.to_string() };
        self.problems.entry(top).or_insert(what);
    }

    /// What arrives at the Image Inputs of graphs used as nodes arrives at what reads them: made
    /// grayscale for an input that takes grayscale, as it is for one that takes color (which
    /// reads gray as it is). An Image Input nothing arrives at shows its picture, as a Bitmap
    /// reads it (the one the node that uses its graph gives it, else the graph's), or without
    /// one, its color: every Image Input is what it shows, and the engine never sees one.
    fn finish(mut self) -> Flat {
        let mut shown: Vec<(NodeId, ImageKind, String, [f32; 4])> = self
            .inputs
            .iter()
            .filter(|(id, _)| !self.redirects.contains_key(id))
            .map(|(id, (kind, picture, color))| (*id, *kind, picture.clone(), *color))
            .collect();
        shown.sort_by_key(|(id, ..)| *id);
        for (id, kind, picture, color) in shown {
            let Some(input) = self.flat.node(id) else { continue };
            let gray = kind == ImageKind::Gray;
            let (made, value) = if picture.is_empty() {
                // Its color, as it takes it (a single pixel, as a uniform color is).
                let output = ParamValue::Enum(u32::from(gray));
                (NodeKind::UNIFORM_COLOR, vec![("color", ParamValue::Color(color)), ("output", output)])
            } else {
                (NodeKind::BITMAP, vec![("path", ParamValue::Text(picture))])
            };
            let mut node = Node::new(id, made, input.pos);
            for slot in &mut node.params {
                let given = value.iter().find(|(k, _)| *k == slot.key).map(|(_, v)| v.clone());
                // As the input is: its tiling and its precision.
                if let Some(v) = given.or_else(|| input.value(&slot.key).filter(|v| v.same_kind(&slot.value)).cloned()) {
                    slot.value = v;
                }
            }
            if made == NodeKind::UNIFORM_COLOR || !gray {
                // What the input shows, where it was: what reads it reads that.
                *self.flat.node_mut(id).expect("an Image Input") = node;
            } else {
                // A picture made grayscale where the input was, as what arrives would be. What
                // goes wrong reading it is the input's: of the node that uses its graph, or its own.
                node.id = NodeId(mix(id.0, 0xB17_4A9) | INNER);
                self.owners.insert(node.id, self.owners.get(&id).copied().unwrap_or(id));
                self.redirects.insert(id, (node.id, "o:output".to_string()));
                self.flat.nodes.push(node);
            }
        }
        let gray: HashSet<NodeId> = self
            .redirects
            .keys()
            .copied()
            .filter(|id| self.inputs.get(id).is_some_and(|(kind, ..)| *kind == ImageKind::Gray))
            .collect();
        // Follow an input that passes on what arrives at another, as far as it goes.
        let resolve = |mut source: (NodeId, String), redirects: &HashMap<NodeId, (NodeId, String)>| {
            for _ in 0..MAX_DEPTH * 4 {
                match redirects.get(&source.0) {
                    Some(next) if !gray.contains(&source.0) => source = next.clone(),
                    _ => break,
                }
            }
            source
        };
        for link in &mut self.flat.links {
            let (from, socket) = resolve((link.from, link.from_socket.clone()), &self.redirects);
            (link.from, link.from_socket) = (from, socket);
        }
        for id in &gray {
            let source = resolve(self.redirects[id].clone(), &self.redirects);
            if let Some(node) = self.flat.node_mut(*id) {
                let mut conversion = Node::new(*id, NodeKind::GRAYSCALE, node.pos);
                conversion
                    .params
                    .iter_mut()
                    .filter(|s| s.key == "tiling")
                    .for_each(|s| s.value = node.value("tiling").cloned().unwrap_or(s.value.clone()));
                *node = conversion;
            }
            self.flat.links.push(Link {
                from: source.0,
                from_socket: source.1,
                to: *id,
                to_socket: input_socket_key("input"),
            });
        }
        let passed: HashSet<NodeId> = self.redirects.keys().filter(|id| !gray.contains(id)).copied().collect();
        self.flat.nodes.retain(|n| !passed.contains(&n.id));
        self.flat.links.retain(|l| !passed.contains(&l.to));
        self.flat.next_id = self.flat.nodes.iter().map(|n| n.id.0 & !INNER).max().unwrap_or(0) + 1;
        Flat { graph: self.flat, problems: self.problems, owners: self.owners }
    }
}

/// A node laid out from a graph used as a node takes the values its parameters have there,
/// whatever gave them: they are its own now, and it has no sockets for them.
fn bake(node: &mut Node, ne: Option<&NodeEval>) {
    let Some(ne) = ne else { return };
    for slot in &mut node.params {
        if let Some(v) = ne.params.get(&slot.key) {
            slot.value = v.clone();
        }
        slot.exposed = false;
    }
    if node.def().is_pixel_expression() {
        for var in &ne.vars {
            if !matches!(var.source, VarSource::Image(_) | VarSource::Builtin) {
                node.var_values.insert(var.name.clone(), var.value as f32);
            }
        }
        if let Some(slot) = node.param_mut("auto_bind") {
            slot.value = ParamValue::Bool(false);
        }
    }
}

/// Does the graph `id` of the package at `from` use the graph `target` (of the package at
/// `target.0`), itself or through graphs it uses, as deep as they go? (`None`: the package
/// being edited, when it has no file, as [`Library::package`] has it.) A graph that uses
/// another cannot be used in it.
pub fn uses_graph<L: Library>(lib: &L, from: Option<&str>, id: GraphId, target: (Option<&str>, GraphId)) -> bool {
    let mut seen: HashSet<(Option<String>, GraphId)> = HashSet::new();
    let mut stack = vec![(from.map(str::to_string), id)];
    while let Some((package, id)) = stack.pop() {
        if (package.as_deref(), id) == target {
            return true;
        }
        if !seen.insert((package.clone(), id)) {
            continue;
        }
        let Some(graph) = lib.package(package.as_deref()).and_then(|p| p.graph(id)) else { continue };
        for node in graph.graph.instances() {
            let r = &node.instance.as_deref().expect("an instance").graph;
            let used = r.package.clone().or_else(|| package.clone());
            if let Some(entry) = lib.package(used.as_deref()).and_then(|p| p.find(r)) {
                stack.push((used, entry.id));
            }
        }
    }
    false
}

/// Bring the nodes of `package` (the package at `own`) that use graphs in line with those
/// graphs: of the package itself, or found in `lib`. Nodes whose graph cannot be found stay as
/// they were. True if any changed.
pub fn sync_instances(package: &mut Package, own: Option<&str>, lib: &impl Library) -> bool {
    // What each used graph is now, before anything changes.
    let mut now: HashMap<(Option<String>, GraphId, String), (String, Interface)> = HashMap::new();
    for entry in &package.graphs {
        for node in entry.graph.instances() {
            let r = &node.instance.as_deref().expect("an instance").graph;
            let key = (r.package.clone(), r.graph, r.name.clone());
            if now.contains_key(&key) {
                continue;
            }
            let found = match &r.package {
                Some(path) if Some(path.as_str()) != own => lib.package(Some(path)).and_then(|p| p.find(r)),
                _ => package.find(r),
            };
            if let Some(found) = found {
                now.insert(key, (found.name.clone(), Interface::of(&found.graph)));
            }
        }
    }
    let mut changed = false;
    for entry in &mut package.graphs {
        let stale = entry.graph.instances().any(|n| {
            let i = n.instance.as_deref().expect("an instance");
            now.get(&(i.graph.package.clone(), i.graph.graph, i.graph.name.clone()))
                .is_some_and(|(name, interface)| *name != i.graph.name || *interface != i.interface)
        });
        if !stale {
            continue;
        }
        let graph = Arc::make_mut(&mut entry.graph);
        for node in graph.nodes.iter_mut().filter(|n| n.instance.is_some()) {
            let r = node.instance.as_deref().expect("an instance").graph.clone();
            if let Some((name, interface)) = now.get(&(r.package.clone(), r.graph, r.name.clone())) {
                changed |= node.set_interface(name, interface.clone());
            }
        }
        graph.prune_links();
    }
    changed
}

// ---------------------------------------------------------------------------- paths

fn same_part(a: &Component<'_>, b: &Component<'_>) -> bool {
    if cfg!(windows) {
        a.as_os_str().to_string_lossy().eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
    } else {
        a == b
    }
}

/// Where the files of a package go when they are collected beside it (so that the package and
/// its folder can be moved or passed on together): each file that is not in `home` (the
/// package's folder) or below it, into `folder`, by its name, numbered where another file
/// already has that name there (`bricks (2).png`) or `exists` says one is; the files in `home`
/// stay where they are. Pairs of where each file is and where it goes.
pub fn collect_plan(files: &[String], home: &Path, folder: &Path, exists: impl Fn(&Path) -> bool) -> Vec<(String, String)> {
    let mut taken: HashSet<String> = HashSet::new();
    let mut plan = Vec::new();
    for file in files {
        let path = Path::new(file);
        // (What the package holds is in its file already.)
        if path.starts_with(home) || crate::files::is_embedded(file) {
            continue;
        }
        let name = path.file_name().map_or_else(|| "file".to_string(), |n| n.to_string_lossy().to_string());
        let (stem, ext) = match name.rfind('.') {
            Some(dot) if dot > 0 => (name[..dot].to_string(), name[dot..].to_string()),
            _ => (name.clone(), String::new()),
        };
        // Names are told apart as the systems that do not tell cases apart would.
        let to = (1..)
            .map(|n| if n == 1 { folder.join(&name) } else { folder.join(format!("{stem} ({n}){ext}")) })
            .find(|to| !taken.contains(&to.display().to_string().to_lowercase()) && !exists(to))
            .expect("a free name");
        taken.insert(to.display().to_string().to_lowercase());
        plan.push((file.clone(), to.display().to_string()));
    }
    plan
}

/// `path` as a file in `dir` says it: relative to `dir`, with `/` between its parts on every
/// system, where the two share a root (a drive on Windows); as it is where they do not.
pub fn relative_path(path: &str, dir: &Path) -> String {
    let p = Path::new(path);
    if path.trim().is_empty() || crate::files::is_embedded(path) || !p.is_absolute() || !dir.is_absolute() {
        return path.to_string();
    }
    let parts: Vec<Component<'_>> = p.components().collect();
    let base: Vec<Component<'_>> = dir.components().collect();
    let common = parts.iter().zip(&base).take_while(|(a, b)| same_part(a, b)).count();
    // Nothing in common past the root: another drive, or nothing worth saying relatively.
    let roots = base.iter().take_while(|c| matches!(c, Component::Prefix(_) | Component::RootDir)).count();
    if common < roots.max(1) {
        return path.to_string();
    }
    let mut out: Vec<String> = vec!["..".to_string(); base.len() - common];
    out.extend(parts[common..].iter().map(|c| c.as_os_str().to_string_lossy().to_string()));
    if out.is_empty() {
        return ".".to_string();
    }
    out.join("/")
}

/// A path a file in `dir` says, as a path on this system: a relative one is taken from
/// `dir` (`/` separates its parts on every system; `\\` too on Windows).
pub fn resolve_path(stored: &str, dir: &Path) -> String {
    if stored.trim().is_empty() || crate::files::is_embedded(stored) || Path::new(stored).is_absolute() {
        return stored.to_string();
    }
    let mut out = dir.to_path_buf();
    for part in stored.split(if cfg!(windows) { &['/', '\\'][..] } else { &['/'][..] }) {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            name => out.push(name),
        }
    }
    out.display().to_string()
}

/// `path` with `.` and `..` taken out, without asking the file system (the file may not be
/// there).
pub fn clean_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Packages by path, for flattening.
    struct Shelf(HashMap<Option<String>, Package>);

    impl Library for Shelf {
        fn package(&self, path: Option<&str>) -> Option<&Package> {
            self.0.get(&path.map(str::to_string))
        }
    }

    fn connect(g: &mut Graph, a: NodeId, sa: &str, b: NodeId, sb: &str) {
        g.connect(a, sa, b, sb).unwrap_or_else(|e| panic!("{sa} → {sb}: {e}"));
    }

    /// A graph that blurs what arrives at its input by an input value, and gives it.
    fn blurring() -> Graph {
        let mut g = Graph::default();
        let input = g.add_node(NodeKind::IMAGE_INPUT, [0.0, 0.0]);
        let blur = g.add_node(NodeKind::BLUR, [200.0, 0.0]);
        let out = g.add_node(NodeKind::OUTPUT, [400.0, 0.0]);
        let radius = g.add_node(NodeKind::FLOAT_INPUT, [0.0, 200.0]);
        g.bind_variable(radius, "radius").unwrap();
        g.variable_mut("radius").unwrap().max = Value::Float(50.0);
        g.variable_mut("radius").unwrap().value = Value::Float(3.0);
        g.set_exposed(blur, "radius", true).unwrap();
        connect(&mut g, input, "o:output", blur, "i:input");
        connect(&mut g, radius, "o:value", blur, "p:radius");
        connect(&mut g, blur, "o:output", out, "i:input");
        g
    }

    fn instance_of(package: &Package, id: GraphId, path: Option<&str>) -> Instance {
        let entry = package.graph(id).unwrap();
        Instance::new(GraphRef { package: path.map(str::to_string), graph: id, name: entry.name.clone() }, Interface::of(&entry.graph))
    }

    #[test]
    fn a_graph_used_as_a_node_is_laid_out_with_its_values() {
        let mut package = Package::new("Main", Graph::default());
        let blurring = package.add_graph("Blur it", blurring());
        let main = package.first();
        let instance = instance_of(&package, blurring, None);
        let g = Arc::make_mut(&mut package.graph_mut(main).unwrap().graph);
        let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
        let used = g.add_instance(instance, [200.0, 0.0]);
        let out = g.add_node(NodeKind::OUTPUT, [400.0, 0.0]);
        g.set_exposed(used, "input", true).unwrap();
        connect(g, noise, "o:output", used, "p:input");
        connect(g, used, "o:output", out, "i:input");
        // The instance's radius, driven from the graph that uses it.
        let r = g.add_node(NodeKind::FLOAT_INPUT, [0.0, 300.0]);
        g.bind_variable(r, "softness").unwrap();
        g.variable_mut("softness").unwrap().max = Value::Float(20.0);
        g.variable_mut("softness").unwrap().value = Value::Float(7.0);
        g.set_exposed(used, "radius", true).unwrap();
        connect(g, r, "o:value", used, "p:radius");

        let shelf = Shelf([(None, package)].into_iter().collect());
        let flat = flatten(&shelf, None, main).unwrap();
        assert!(flat.problems.is_empty(), "{:?}", flat.problems);
        let f = &flat.graph;
        let blur = f.nodes_of(NodeKind::BLUR).next().expect("the blur, laid out");
        assert!(blur.id.is_inner());
        assert_eq!(blur.value("radius"), Some(&ParamValue::Float(7.0)), "the value the graph that uses it gives");
        assert!(!blur.is_exposed("radius"));
        // The noise goes into the blur through the input, made grayscale as the input says.
        let gray = f.nodes_of(NodeKind::GRAYSCALE).next().expect("the input, as a conversion");
        assert_eq!(f.link_into(gray.id, "i:input").unwrap().from, noise);
        assert_eq!(f.link_into(blur.id, "i:input").unwrap().from, gray.id);
        // The blur goes to the output of the graph that uses it, and to the node, which shows it.
        assert_eq!(f.link_into(out, "i:input").unwrap().from, blur.id);
        assert_eq!(f.link_into(used, &input_socket_key(SHOWN)).unwrap().from, blur.id);
        assert_eq!(flat.owners.get(&blur.id), Some(&used));
        assert!(f.nodes_of(NodeKind::IMAGE_INPUT).next().is_none());
        // The engine's order has the blur before what reads it.
        let ev = eval::evaluate(f);
        assert!(ev.cyclic.is_empty());
        let at = |id: NodeId| ev.order.iter().position(|x| *x == id).unwrap();
        assert!(
            at(noise) < at(gray.id) && at(gray.id) < at(blur.id) && at(blur.id) < at(out) && at(blur.id) < at(used)
        );
    }

    #[test]
    fn graphs_of_other_packages_missing_ones_and_graphs_that_use_themselves() {
        let lib_path = "/lib/stone.brewtex";
        let library = Package::new("Blur it", blurring());
        let blurring = library.first();
        let mut package = Package::new("Main", Graph::default());
        let main = package.first();
        let g = Arc::make_mut(&mut package.graph_mut(main).unwrap().graph);
        let used = g.add_instance(instance_of(&library, blurring, Some(lib_path)), [0.0, 0.0]);
        let missing = g.add_instance(
            Instance::new(GraphRef { package: Some("/gone.brewtex".into()), graph: GraphId(1), name: "Gone".into() }, Interface::default()),
            [0.0, 300.0],
        );
        // The graph uses itself.
        let itself = g.add_instance(
            Instance::new(GraphRef { package: None, graph: main, name: "Main".into() }, Interface::default()),
            [0.0, 600.0],
        );
        let shelf = Shelf([(None, package), (Some(lib_path.to_string()), library)].into_iter().collect());
        let flat = flatten(&shelf, None, main).unwrap();
        assert!(!flat.problems.contains_key(&used), "{:?}", flat.problems);
        assert!(flat.problems[&missing].contains("cannot be found"));
        assert!(flat.problems[&itself].contains("uses itself"));
        // Nothing arrives at its input: the Image Input is its color.
        assert!(flat.graph.nodes_of(NodeKind::IMAGE_INPUT).next().is_none());
        assert!(flat.graph.nodes_of(NodeKind::UNIFORM_COLOR).next().is_some());
        assert!(flat.graph.nodes_of(NodeKind::BLUR).next().is_some());
    }

    #[test]
    fn nodes_follow_the_graph_they_use() {
        let mut package = Package::new("Main", Graph::default());
        let sub = package.add_graph("Sub", blurring());
        let main = package.first();
        let instance = instance_of(&package, sub, None);
        let g = Arc::make_mut(&mut package.graph_mut(main).unwrap().graph);
        let used = g.add_instance(instance, [0.0, 0.0]);
        let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
        g.set_exposed(used, "input", true).unwrap();
        connect(g, noise, "o:output", used, "p:input");
        g.node_mut(used).unwrap().param_mut("radius").unwrap().value = ParamValue::Float(9.0);
        // The graph loses its input and its radius gets a larger range.
        let s = Arc::make_mut(&mut package.graph_mut(sub).unwrap().graph);
        let input = s.nodes_of(NodeKind::IMAGE_INPUT).next().unwrap().id;
        s.remove_nodes(&[input]);
        s.variable_mut("radius").unwrap().max = Value::Float(5.0);
        package.graph_mut(sub).unwrap().name = "Renamed".into();
        let shelf = Shelf(HashMap::new());
        assert!(sync_instances(&mut package, None, &shelf));
        let g = &package.graph(main).unwrap().graph;
        let node = g.node(used).unwrap();
        assert_eq!(node.instance.as_ref().unwrap().graph.name, "Renamed");
        assert_eq!(node.value("radius"), Some(&ParamValue::Float(5.0)), "held to the new range");
        // The link to the input that is gone waits for it.
        assert!(g.link_into(used, "p:input").is_some());
        assert!(!g.is_link_live(g.link_into(used, "p:input").unwrap()));
        assert!(!sync_instances(&mut package, None, &shelf), "nothing more to do");
    }

    #[test]
    fn nodes_made_a_graph_of_their_own() {
        let (demo, _) = crate::demo::graph();
        let mut package = Package::new("Wall", demo);
        let main = package.first();
        let g = &package.graph(main).unwrap().graph;
        // The bricks worn and colored: from the Blend after the tiles to the Blend of the base
        // color, whatever feeds them and whatever they feed.
        let wear = [NodeKind::BLEND, NodeKind::LEVELS, NodeKind::BLUR, NodeKind::GRADIENT_MAP, NodeKind::HISTOGRAM_SCAN];
        let chosen: Vec<NodeId> = g.nodes.iter().filter(|n| wear.contains(&n.kind)).map(|n| n.id).collect();
        assert_eq!(chosen.len(), 6);
        let before = flatten(&Shelf([(None, package.clone())].into_iter().collect()), None, main).unwrap();
        let result = PackageEdit::GraphFromSelection { graph: main, nodes: chosen.clone(), name: "Wear".into() }
            .apply(&mut package)
            .unwrap();
        let sub = result.graph.unwrap();
        let used = result.created[0];
        let g = &package.graph(main).unwrap().graph;
        assert!(chosen.iter().all(|id| !g.contains(*id)));
        let node = g.node(used).unwrap();
        assert_eq!(node.instance.as_ref().unwrap().graph.graph, sub);
        let s = &package.graph(sub).unwrap().graph;
        assert_eq!(wear.iter().map(|k| s.nodes_of(*k).count()).sum::<usize>(), chosen.len());
        // Laid out again, it computes what it did: the same nodes of the same types, their links
        // going the same way.
        let after = flatten(&Shelf([(None, package.clone())].into_iter().collect()), None, main).unwrap();
        assert!(after.problems.is_empty(), "{:?}", after.problems);
        let kinds = |f: &Flat| {
            let mut k: Vec<&str> = f
                .graph
                .nodes
                .iter()
                .filter(|n| n.def().has_image() && n.def().role != Role::Instance)
                .map(|n| n.kind.id())
                .collect();
            k.sort();
            k
        };
        let mut expected = kinds(&before);
        // Every grayscale input of the new graph converts what arrives.
        let conversions = s.variables.iter().filter(|v| v.image.as_ref().is_some_and(|i| i.kind == ImageKind::Gray)).count();
        expected.extend(std::iter::repeat_n("grayscale", conversions));
        expected.sort();
        assert_eq!(kinds(&after), expected);
        for output in g.nodes_of(NodeKind::OUTPUT) {
            assert!(g.link_into(output.id, "i:input").is_some(), "{} is still connected", output.text("name"));
        }
        // Nothing to put in a graph, or outputs: refused, and the package stays as it is.
        let output = g.nodes_of(NodeKind::OUTPUT).next().unwrap().id;
        let before = package.clone();
        assert!(
            PackageEdit::GraphFromSelection { graph: main, nodes: vec![], name: "x".into() }.apply(&mut package).is_err()
        );
        assert!(
            PackageEdit::GraphFromSelection { graph: main, nodes: vec![output], name: "x".into() }
                .apply(&mut package)
                .is_err()
        );
        assert_eq!(package, before);
    }

    /// An input value the nodes read that other nodes read too stays, and drives the new
    /// graph's: changed there, it changes what the nodes compute. One only they read goes with
    /// them, at the value it has.
    #[test]
    fn input_values_read_inside_and_outside() {
        let mut g = Graph::default();
        let amount = g.add_node(NodeKind::FLOAT_INPUT, [0.0, 0.0]);
        g.bind_variable(amount, "amount").unwrap();
        g.variable_mut("amount").unwrap().value = Value::Float(0.3);
        let pm = g.add_node(NodeKind::PIXEL_MATH, [200.0, 0.0]);
        g.node_mut(pm).unwrap().param_mut("expression").unwrap().value = ParamValue::Text("u * amount".into());
        let levels = g.add_node(NodeKind::LEVELS, [400.0, 0.0]);
        g.set_exposed(levels, "out_high", true).unwrap();
        connect(&mut g, amount, "o:value", levels, "p:out_high");
        connect(&mut g, pm, "o:output", levels, "i:input");
        let out = g.add_node(NodeKind::OUTPUT, [600.0, 0.0]);
        connect(&mut g, levels, "o:output", out, "i:input");
        let package = Package::new("Main", g);
        let main = package.first();
        let flat_pm = |package: &Package| {
            let flat = flatten(&Shelf([(None, package.clone())].into_iter().collect()), None, main).unwrap();
            assert!(flat.problems.is_empty(), "{:?}", flat.problems);
            flat.graph.nodes_of(NodeKind::PIXEL_MATH).next().unwrap().var_values.get("amount").copied()
        };

        // The Pixel Math alone: the Input node stays with the Levels, and drives the new graph's
        // amount.
        let mut p = package.clone();
        let used = PackageEdit::GraphFromSelection { graph: main, nodes: vec![pm], name: "Part".into() }
            .apply(&mut p)
            .unwrap()
            .created[0];
        let g = &p.graph(main).unwrap().graph;
        let link = g.link_into(used, &param_socket_key("amount")).expect("driven from outside");
        assert_eq!(g.node(link.from).unwrap().variable.as_deref(), Some("amount"));
        assert_eq!(flat_pm(&p), Some(0.3));
        Arc::make_mut(&mut p.graph_mut(main).unwrap().graph).variable_mut("amount").unwrap().value = Value::Float(0.8);
        assert_eq!(flat_pm(&p), Some(0.8), "the value changed outside");

        // The Input node, the Pixel Math and the Levels: the value goes with them.
        let mut p = package.clone();
        let used = PackageEdit::GraphFromSelection { graph: main, nodes: vec![amount, pm, levels], name: "Part".into() }
            .apply(&mut p)
            .unwrap()
            .created[0];
        let g = &p.graph(main).unwrap().graph;
        assert!(g.variable("amount").is_none());
        let node = g.node(used).unwrap();
        assert_eq!(node.value("amount"), Some(&ParamValue::Float(0.3)));
        assert!(!node.is_exposed("amount"));
        assert_eq!(flat_pm(&p), Some(0.3));
    }

    /// A node between the chosen ones (what one of them makes reaches another through it)
    /// would be both before and after the node that takes their place: it is asked for, and the
    /// package stays as it is.
    #[test]
    fn a_node_between_the_chosen_ones_is_asked_for() {
        let mut g = Graph::default();
        let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
        let blur = g.add_node(NodeKind::BLUR, [200.0, 0.0]);
        let levels = g.add_node(NodeKind::LEVELS, [400.0, 200.0]);
        let blend = g.add_node(NodeKind::BLEND, [600.0, 0.0]);
        connect(&mut g, noise, "o:output", blur, "i:input");
        connect(&mut g, blur, "o:output", levels, "i:input");
        connect(&mut g, blur, "o:output", blend, "i:background");
        connect(&mut g, levels, "o:output", blend, "i:mask");
        let mut package = Package::new("Main", g);
        let main = package.first();
        let before = package.clone();
        let edit = PackageEdit::GraphFromSelection { graph: main, nodes: vec![blur, blend], name: "Part".into() };
        let e = edit.apply(&mut package).unwrap_err();
        assert!(e.starts_with("Levels is between"), "{e}");
        assert_eq!(package, before);
        // With it, they go.
        let edit = PackageEdit::GraphFromSelection { graph: main, nodes: vec![blur, levels, blend], name: "Part".into() };
        assert!(edit.apply(&mut package).is_ok());
    }

    /// Of two Outputs of one name, the upper one is the output of the graph used as a node,
    /// and both say so; a node that uses a graph shows its first output that gives anything.
    #[test]
    fn outputs_of_one_name_and_the_output_shown() {
        let mut g = Graph::default();
        let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
        let color = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 300.0]);
        let unused = g.add_node(NodeKind::OUTPUT, [400.0, -300.0]);
        let upper = g.add_node(NodeKind::OUTPUT, [400.0, 0.0]);
        let lower = g.add_node(NodeKind::OUTPUT, [400.0, 300.0]);
        for (id, name) in [(unused, "first"), (upper, "twice"), (lower, "twice")] {
            g.node_mut(id).unwrap().param_mut("name").unwrap().value = ParamValue::Text(name.into());
        }
        // The lower one linked first: the order of the links does not choose.
        connect(&mut g, color, "o:output", lower, "i:input");
        connect(&mut g, noise, "o:output", upper, "i:input");
        let ev = eval::evaluate(&g);
        for id in [upper, lower] {
            assert!(ev.node(id).unwrap().warnings.iter().any(|w| w.contains("Another Output is named 'twice'")));
        }
        assert!(ev.node(unused).unwrap().warnings.iter().all(|w| !w.contains("Another Output")));
        let names: Vec<String> = Interface::of(&g).outputs.into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["first", "twice"]);

        let mut package = Package::new("Main", Graph::default());
        let sub = package.add_graph("Sub", g);
        let main = package.first();
        let instance = instance_of(&package, sub, None);
        let m = Arc::make_mut(&mut package.graph_mut(main).unwrap().graph);
        let used = m.add_instance(instance, [0.0, 0.0]);
        let out = m.add_node(NodeKind::OUTPUT, [300.0, 0.0]);
        connect(m, used, &output_socket_key("twice"), out, "i:input");
        let flat = flatten(&Shelf([(None, package)].into_iter().collect()), None, main).unwrap();
        let f = &flat.graph;
        let from = |id: NodeId, socket: &str| f.node(f.link_into(id, socket).unwrap().from).unwrap().kind;
        assert_eq!(from(out, "i:input"), NodeKind::NOISE);
        // The first output gives nothing: the node shows the next.
        assert_eq!(from(used, &input_socket_key(SHOWN)), NodeKind::NOISE);
    }

    /// A graph uses the graphs its nodes use, and those they use, in its package and in
    /// others: none of those can use it.
    #[test]
    fn graphs_used_as_deep_as_they_go() {
        let lib_path = "/lib/stone.brewtex";
        let library = Package::new("Stone", Graph::default());
        let stone = library.first();
        let mut package = Package::new("Wall", Graph::default());
        let wall = package.first();
        let bricks = package.add_graph("Bricks", Graph::default());
        let mortar = package.add_graph("Mortar", Graph::default());
        // The wall uses the bricks, which use the stone of the library, which uses nothing; the
        // mortar is on its own.
        let bricks_node = instance_of(&package, bricks, None);
        Arc::make_mut(&mut package.graph_mut(wall).unwrap().graph).add_instance(bricks_node, [0.0, 0.0]);
        let stone_node = instance_of(&library, stone, Some(lib_path));
        Arc::make_mut(&mut package.graph_mut(bricks).unwrap().graph).add_instance(stone_node, [0.0, 0.0]);
        let shelf = Shelf([(None, package), (Some(lib_path.to_string()), library)].into_iter().collect());
        assert!(uses_graph(&shelf, None, wall, (None, wall)), "itself");
        assert!(uses_graph(&shelf, None, wall, (None, bricks)));
        assert!(uses_graph(&shelf, None, wall, (Some(lib_path), stone)), "through the bricks");
        assert!(!uses_graph(&shelf, None, bricks, (None, wall)));
        assert!(!uses_graph(&shelf, None, wall, (None, mortar)));
        assert!(!uses_graph(&shelf, Some(lib_path), stone, (None, wall)));
    }

    /// Files a package holds itself: imported, or embedded in place of the files they were
    /// linked to (every node that read one reads it there); written into the package's file
    /// and read back; never missing nor collected; brought along by a node pasted into another
    /// package; linked again to a file; and those no node reads taken out.
    #[test]
    fn files_a_package_holds_itself() {
        let dir = std::env::temp_dir().join(format!("brewed_embed_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bricks = dir.join("bricks.png").display().to_string();
        std::fs::write(&bricks, b"not really a png").unwrap();
        let mut g = Graph::default();
        let bitmap = g.add_node(NodeKind::BITMAP, [0.0, 0.0]);
        g.node_mut(bitmap).unwrap().param_mut("path").unwrap().value = ParamValue::Text(bricks.clone());
        let mut package = Package::new("Wall", g);
        let main = package.first();
        assert_eq!(package.resources, [Resource::linked(bricks.clone(), ResourceKind::Bitmap)]);

        // Embedded: the node reads it in the package, by what it holds.
        let bytes = Arc::new(std::fs::read(&bricks).unwrap());
        PackageEdit::Embed(vec![(bricks.clone(), bytes.clone())]).apply(&mut package).unwrap();
        let held = &package.resources[0];
        assert!(held.is_embedded() && crate::files::is_embedded(&held.path) && held.name() == "bricks.png");
        let read_by_node = package.graph(main).unwrap().graph.node(bitmap).unwrap().text("path").to_string();
        assert_eq!(read_by_node, held.path);
        assert_eq!(crate::files::read(&held.path).unwrap(), bytes);
        std::fs::remove_file(&bricks).unwrap();
        assert!(package.missing().is_empty(), "the file on disk is gone, but the package holds it");
        assert!(collect_plan(&package.all_files(), &dir.join("elsewhere"), &dir, |_| false).is_empty());

        // Imported, beside it; written into the file and read back as it was.
        let font = Arc::new(b"a font".to_vec());
        PackageEdit::Import(vec![("C:/fonts/Serif.ttf".into(), font.clone())]).apply(&mut package).unwrap();
        assert_eq!(package.resources.len(), 2);
        let json = crate::project::to_json(&package, &HashMap::new(), main, Some(&dir)).unwrap();
        let loaded = crate::project::from_json(&json, Some(&dir), "x").unwrap().package;
        assert_eq!(loaded, package);
        assert_eq!(loaded.resources[1].data.as_deref(), Some(font.as_ref()));

        // A node pasted into another package brings the file it reads.
        let clip = package.graph(main).unwrap().graph.copy_nodes(&[bitmap]);
        let mut other = Package::new("Other", Graph::default());
        let first = other.first();
        Arc::make_mut(&mut other.graph_mut(first).unwrap().graph).paste(&clip, [0.0, 0.0]);
        assert!(other.sync_resources());
        assert!(other.resources[0].is_embedded() && other.resources[0].path == read_by_node);

        // Linked to a file again: no longer held.
        let copy = dir.join("bricks copy.png").display().to_string();
        PackageEdit::Relink { from: read_by_node.clone(), to: copy.clone() }.apply(&mut package).unwrap();
        let linked = package.resources.iter().find(|r| r.path == copy).unwrap();
        assert!(!linked.is_embedded());

        // The font no node reads goes; the file the Bitmap reads stays.
        let result = PackageEdit::RemoveUnused.apply(&mut package).unwrap();
        assert!(result.notice.is_some());
        assert_eq!(package.resources.len(), 1);
        assert_eq!(package.resources[0].path, copy);
        assert_eq!(PackageEdit::RemoveUnused.apply(&mut package).unwrap(), PackageResult::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn image_input<'g>(g: &'g mut Graph, name: &str) -> &'g mut crate::graph::ImageInput {
        g.variable_mut(name).and_then(|v| v.image.as_mut()).expect("an image input")
    }

    /// An image input is an input value of its graph whose value is a picture: the Image Inputs
    /// of its name show it while nothing arrives (the graph seen alone too), else its color. A
    /// node that uses the graph has a parameter for it: a picture of the node's own (none: the
    /// graph's), or, exposed, a socket, what arrives there coming first. A grayscale input makes
    /// what it shows grayscale; the pictures are files of the package.
    #[test]
    fn image_inputs_are_input_values_of_pictures() {
        use crate::Edit;
        let mut tex = Graph::default();
        let input = tex.add_node(NodeKind::IMAGE_INPUT, [0.0, 0.0]);
        assert_eq!(tex.rename_variable("input", "albedo").unwrap(), "albedo");
        let apply = |g: &mut Graph, edit: Edit| g_apply(g, edit);
        assert!(apply(&mut tex, Edit::SetVariableKind { name: "albedo".into(), kind: ImageKind::Color }));
        assert!(apply(&mut tex, Edit::SetVariablePicture { name: "albedo".into(), picture: "/own.png".into(), default: false }));
        assert!(apply(&mut tex, Edit::MakeVariableDefault("albedo".into())));
        assert!(!apply(&mut tex, Edit::MakeVariableDefault("albedo".into())), "it is its default already");
        let out = tex.add_node(NodeKind::OUTPUT, [300.0, 0.0]);
        connect(&mut tex, input, "o:output", out, "i:input");
        let mut package = Package::new("Main", Graph::default());
        let t = package.add_graph("Tex", tex);
        let main = package.first();
        let laid_out = |package: &Package, id: GraphId| {
            flatten(&Shelf([(None, package.clone())].into_iter().collect()), None, id).unwrap().graph
        };
        let source = |f: &Graph, id: NodeId, socket: &str| f.node(f.link_into(id, socket).unwrap().from).unwrap().clone();

        // Alone: the input is its picture; without one, its color.
        let f = laid_out(&package, t);
        let alone = f.node(input).unwrap();
        assert_eq!((alone.kind, alone.text("path")), (NodeKind::BITMAP, "/own.png"));
        assert_eq!(f.link_into(out, "i:input").unwrap().from, input);
        image_input(graph_of(&mut package, t), "albedo").picture.clear();
        let alone = laid_out(&package, t).node(input).unwrap().clone();
        assert_eq!(alone.kind, NodeKind::UNIFORM_COLOR);
        assert_eq!(alone.value("color"), Some(&ParamValue::Color(crate::graph::IMAGE_INPUT_COLOR)));
        assert!(apply(graph_of(&mut package, t), Edit::ResetVariable("albedo".into())), "back to its picture by default");
        assert_eq!(image_input(graph_of(&mut package, t), "albedo").picture, "/own.png");

        // Used: a parameter, not a socket; nothing of its own shows the graph's picture, a picture
        // of its own, that one.
        let instance = instance_of(&package, t, None);
        let g = Arc::make_mut(&mut package.graph_mut(main).unwrap().graph);
        let used = g.add_instance(instance, [0.0, 0.0]);
        let shown = g.add_node(NodeKind::OUTPUT, [300.0, 0.0]);
        connect(g, used, &output_socket_key("output"), shown, "i:input");
        let node = g.node(used).unwrap();
        assert_eq!(node.value("albedo"), Some(&ParamValue::Text(String::new())));
        assert!(!node.is_exposed("albedo"));
        assert!(g.input_sockets(node).is_empty(), "no socket to plug anything into");
        let bitmap = source(&laid_out(&package, main), shown, "i:input");
        assert_eq!((bitmap.kind, bitmap.text("path")), (NodeKind::BITMAP, "/own.png"));
        let g = Arc::make_mut(&mut package.graph_mut(main).unwrap().graph);
        assert!(apply(g, Edit::SetParam { node: used, key: "albedo".into(), value: ParamValue::Text("/given.png".into()) }));
        assert_eq!(source(&laid_out(&package, main), shown, "i:input").text("path"), "/given.png");
        assert!(package.sync_resources());
        assert!(package.resources.iter().any(|r| r.path == "/given.png"), "a file of the package");

        // Exposed: a socket that takes color, and what arrives comes first; not any more, the
        // node's picture again.
        let g = Arc::make_mut(&mut package.graph_mut(main).unwrap().graph);
        assert!(apply(g, Edit::SetExposed { node: used, key: "albedo".into(), exposed: true }));
        let socket = g.input_socket(used, &param_socket_key("albedo")).expect("a socket");
        assert_eq!((socket.ty, socket.slot), (DataType::Image, Slot::Color));
        let noise = g.add_node(NodeKind::NOISE, [-300.0, 0.0]);
        connect(g, noise, "o:output", used, &param_socket_key("albedo"));
        assert_eq!(source(&laid_out(&package, main), shown, "i:input").id, noise);
        let g = Arc::make_mut(&mut package.graph_mut(main).unwrap().graph);
        assert!(apply(g, Edit::SetExposed { node: used, key: "albedo".into(), exposed: false }));
        assert!(g.link_into(used, &param_socket_key("albedo")).is_none());
        assert_eq!(source(&laid_out(&package, main), shown, "i:input").text("path"), "/given.png");

        // A grayscale input: the picture, made grayscale; what goes wrong reading it is the node's
        // that uses the graph, and seen alone, the input's.
        assert!(apply(graph_of(&mut package, t), Edit::SetVariableKind { name: "albedo".into(), kind: ImageKind::Gray }));
        assert!(sync_instances(&mut package, None, &Shelf(HashMap::new())));
        let f = laid_out(&package, main);
        let gray = source(&f, shown, "i:input");
        assert_eq!(gray.kind, NodeKind::GRAYSCALE);
        let read = source(&f, gray.id, "i:input");
        assert_eq!((read.kind, read.text("path")), (NodeKind::BITMAP, "/given.png"));
        let lib = Shelf([(None, package.clone())].into_iter().collect());
        assert_eq!(flatten(&lib, None, main).unwrap().owners.get(&read.id), Some(&used));
        let alone = flatten(&lib, None, t).unwrap();
        let own = alone.graph.nodes.iter().find(|n| n.kind == NodeKind::BITMAP).expect("the input's own picture");
        assert_eq!((own.text("path"), alone.owners.get(&own.id)), ("/own.png", Some(&input)));
        // Without a picture at all, its color: one pixel of gray, as a uniform color is.
        let g = Arc::make_mut(&mut package.graph_mut(main).unwrap().graph);
        assert!(apply(g, Edit::SetParam { node: used, key: "albedo".into(), value: ParamValue::Text(String::new()) }));
        image_input(graph_of(&mut package, t), "albedo").picture.clear();
        let color = source(&laid_out(&package, main), shown, "i:input");
        assert_eq!((color.kind, color.value("output")), (NodeKind::UNIFORM_COLOR, Some(&ParamValue::Enum(1))));

        // The pictures are files of the package, however the paths change.
        image_input(graph_of(&mut package, t), "albedo").picture = "/b.png".into();
        let g = Arc::make_mut(&mut package.graph_mut(main).unwrap().graph);
        assert!(apply(g, Edit::SetParam { node: used, key: "albedo".into(), value: ParamValue::Text("/c.png".into()) }));
        let mut read: Vec<&str> = package.files().map(|(.., path)| path).collect();
        read.sort();
        assert_eq!(read, ["/b.png", "/c.png", "/own.png"]);
        package.map_paths(|p| format!("/moved{p}"));
        let mut read: Vec<&str> = package.files().map(|(.., path)| path).collect();
        read.sort();
        assert_eq!(read, ["/moved/b.png", "/moved/c.png", "/moved/own.png"]);

        // Renamed, the input is another parameter: the picture given for the old one goes, as what
        // arrived there would.
        graph_of(&mut package, t).rename_variable("albedo", "base").unwrap();
        assert!(sync_instances(&mut package, None, &Shelf(HashMap::new())));
        let node = package.graph(main).unwrap().graph.node(used).unwrap().clone();
        assert!(node.param("albedo").is_none());
        assert_eq!(node.value("base"), Some(&ParamValue::Text(String::new())));
    }

    fn graph_of(package: &mut Package, id: GraphId) -> &mut Graph {
        Arc::make_mut(&mut package.graph_mut(id).unwrap().graph)
    }

    fn g_apply(g: &mut Graph, edit: crate::Edit) -> bool {
        edit.apply(g).unwrap_or_else(|e| panic!("{e}")).changed
    }

    #[test]
    fn paths_relative_to_the_file() {
        let (dir, path, other) = if cfg!(windows) {
            (Path::new(r"C:\work\wall"), r"C:\work\textures\bricks.png", r"D:\stock\sand.png")
        } else {
            (Path::new("/work/wall"), "/work/textures/bricks.png", "/stock/sand.png")
        };
        assert_eq!(relative_path(path, dir), "../textures/bricks.png");
        assert_eq!(resolve_path("../textures/bricks.png", dir), Path::new(path).display().to_string());
        let inside = dir.join("a").join("b.png").display().to_string();
        assert_eq!(relative_path(&inside, dir), "a/b.png");
        assert_eq!(resolve_path("a/b.png", dir), inside);
        if cfg!(windows) {
            // Another drive: nothing to say relatively.
            assert_eq!(relative_path(other, dir), other);
        } else {
            assert_eq!(relative_path(other, dir), "../../stock/sand.png");
        }
        assert_eq!(relative_path("", dir), "");
        assert_eq!(resolve_path("", dir), "");
        assert_eq!(clean_path(Path::new("a/./b/../c")), Path::new("a").join("c"));
    }

    /// Collecting the files of a package: those outside its folder go into the folder asked
    /// for, by their names, numbered where two have one or one is there already; those beside
    /// it stay; and the package then says where they went, in one step.
    #[test]
    fn the_files_of_a_package_are_collected_beside_it() {
        let root = if cfg!(windows) { Path::new(r"C:\work") } else { Path::new("/work") };
        let home = root.join("wall");
        let folder = home.join("wall_files");
        let at = |p: &[&str]| p.iter().fold(root.to_path_buf(), |d, part| d.join(part)).display().to_string();
        let files = vec![
            at(&["stock", "bricks.png"]),
            at(&["other", "bricks.png"]),
            at(&["other", "BRICKS (2).png"]),
            at(&["wall", "moss.png"]),
            at(&["fonts", "title"]),
        ];
        let there_already = folder.join("title");
        let plan = collect_plan(&files, &home, &folder, |p| p == there_already);
        let names: Vec<String> = plan.iter().map(|(_, to)| file_name(to)).collect();
        assert_eq!(names, vec!["bricks.png", "bricks (2).png", "BRICKS (2) (2).png", "title (2)"]);
        assert!(plan.iter().all(|(from, to)| *from != at(&["wall", "moss.png"]) && Path::new(to).starts_with(&folder)));

        // The package says where each went, the files it lists and those its nodes read.
        let mut g = Graph::default();
        let bitmap = g.add_node(NodeKind::BITMAP, [0.0, 0.0]);
        g.node_mut(bitmap).unwrap().param_mut("path").unwrap().value = ParamValue::Text(files[0].clone());
        let mut package = Package::new("Wall", g);
        package.resources.push(Resource::linked(files[1].clone(), ResourceKind::Bitmap));
        package.sync_resources();
        assert_eq!(package.all_files(), vec![files[0].clone(), files[1].clone()]);
        let result = PackageEdit::Relocate(plan[..2].to_vec()).apply(&mut package).unwrap();
        assert!(result.changed);
        let node = package.graphs[0].graph.node(bitmap).unwrap();
        assert_eq!(node.text("path"), plan[0].1);
        assert_eq!(package.all_files(), vec![plan[0].1.clone(), plan[1].1.clone()]);
        assert!(!PackageEdit::Relocate(Vec::new()).apply(&mut package).unwrap().changed);
    }

    #[test]
    fn the_pictures_of_image_inputs_are_files_of_the_package() {
        // A graph whose image input has a picture, and a node that uses it, giving another.
        let mut tex = Graph::default();
        tex.add_node(NodeKind::IMAGE_INPUT, [0.0, 0.0]);
        let image = image_input(&mut tex, "input");
        (image.picture, image.default) = ("/old/a.png".into(), "/old/a.png".into());
        let mut package = Package::new("Main", Graph::default());
        let t = package.add_graph("Tex", tex);
        let main = package.first();
        let instance = instance_of(&package, t, None);
        let used = graph_of(&mut package, main).add_instance(instance, [0.0, 0.0]);
        graph_of(&mut package, main).node_mut(used).unwrap().param_mut("input").unwrap().value = ParamValue::Text("/old/b.png".into());
        package.sync_resources();
        let listed = |p: &Package| {
            let mut paths: Vec<String> = p.resources.iter().map(|r| r.path.clone()).collect();
            paths.sort();
            paths
        };
        assert_eq!(listed(&package), ["/old/a.png", "/old/b.png"]);
        let missing: Vec<String> = package.missing().into_iter().map(|m| m.path).collect();
        assert!(missing.contains(&"/old/a.png".to_string()) && missing.contains(&"/old/b.png".to_string()), "{missing:?}");
        assert!(PackageEdit::RemoveResource("/old/a.png".into()).apply(&mut package.clone()).is_err(), "the input reads it");

        // Moved: the input's picture and its default follow, and so does the node's.
        for (from, to) in [("/old/a.png", "/new/a.png"), ("/old/b.png", "/new/b.png")] {
            PackageEdit::Relink { from: from.into(), to: to.into() }.apply(&mut package).unwrap();
        }
        let image = image_input(graph_of(&mut package, t), "input").clone();
        assert_eq!((image.picture.as_str(), image.default.as_str()), ("/new/a.png", "/new/a.png"));
        assert_eq!(package.graph(main).unwrap().graph.node(used).unwrap().text("input"), "/new/b.png");
        assert_eq!(listed(&package), ["/new/a.png", "/new/b.png"]);

        // Held in the package: what reads them reads them there.
        let bytes = Arc::new(b"a picture".to_vec());
        PackageEdit::Embed(vec![("/new/a.png".into(), bytes.clone()), ("/new/b.png".into(), bytes.clone())]).apply(&mut package).unwrap();
        let held = crate::files::embedded_path("a.png", &bytes);
        let image = image_input(graph_of(&mut package, t), "input").clone();
        assert_eq!((image.picture.as_str(), image.default.as_str()), (held.as_str(), held.as_str()));
        let given = package.graph(main).unwrap().graph.node(used).unwrap().text("input").to_string();
        assert!(crate::files::is_embedded(&given));
        assert!(package.resources.iter().all(|r| r.data.is_some()), "{:?}", package.resources);
        assert!(package.unused_resources().is_empty());

        // Collected beside the package: a picture on disk goes with it.
        image_input(graph_of(&mut package, t), "input").picture = "/far/c.png".into();
        package.sync_resources();
        let home = Path::new(if cfg!(windows) { r"C:\work" } else { "/work" });
        let plan = collect_plan(&package.all_files(), home, &home.join("files"), |_| false);
        assert!(plan.iter().any(|(from, _)| from == "/far/c.png"), "{plan:?}");
        PackageEdit::Relocate(plan.clone()).apply(&mut package).unwrap();
        let to = plan.iter().find(|(from, _)| from == "/far/c.png").map(|(_, to)| to.clone()).unwrap();
        assert_eq!(image_input(graph_of(&mut package, t), "input").picture, to);
    }

    #[test]
    fn relinking_and_the_files_of_a_package() {
        let mut g = Graph::default();
        let bitmap = g.add_node(NodeKind::BITMAP, [0.0, 0.0]);
        g.node_mut(bitmap).unwrap().param_mut("path").unwrap().value = ParamValue::Text("/old/bricks.png".into());
        let mut package = Package::new("Main", g);
        assert_eq!(package.resources, vec![Resource::linked("/old/bricks.png".into(), ResourceKind::Bitmap)]);
        assert!(
            PackageEdit::RemoveResource("/old/bricks.png".into()).apply(&mut package.clone()).is_err(),
            "a node reads it"
        );
        PackageEdit::Relink { from: "/old/bricks.png".into(), to: "/new/bricks.png".into() }
            .apply(&mut package)
            .unwrap();
        let g = &package.graph(package.first()).unwrap().graph;
        assert_eq!(g.node(bitmap).unwrap().text("path"), "/new/bricks.png");
        assert_eq!(package.resources[0].path, "/new/bricks.png");
        let r = PackageEdit::AddResources(vec!["/fonts/a.ttf".into(), "/x.doc".into()]).apply(&mut package).unwrap();
        assert!(r.notice.unwrap().contains("x.doc"));
        assert_eq!(package.resources[1].kind, ResourceKind::Font);
        assert!(package.missing().iter().any(|m| m.path == "/fonts/a.ttf"));
        // Graphs of a package: names of their own, at least one.
        let first = package.first();
        let second = PackageEdit::AddGraph { name: "Main".into(), graph: Graph::default() }
            .apply(&mut package)
            .unwrap()
            .graph
            .unwrap();
        assert_eq!(package.graph(second).unwrap().name, "Main 2");
        assert!(PackageEdit::RenameGraph { id: second, name: "Main".into() }.apply(&mut package.clone()).is_err());
        PackageEdit::RemoveGraph(first).apply(&mut package).unwrap();
        assert!(PackageEdit::RemoveGraph(second).apply(&mut package).is_err(), "the last graph stays");
    }
}
