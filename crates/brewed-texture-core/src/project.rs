//! Package files (`.brewtex`): a package (see [`crate::package`]), its graphs, the files it holds
//! and how the editor showed it, in one file any program can read. `docs/package-format.md`
//! says it for them; in short, a ZIP archive (see [`crate::archive`]) of
//!
//! - `mimetype`, the first entry, stored: [`MEDIA_TYPE`], which tells the file by its first
//!   bytes;
//! - `package.json`: what the package is (see [`Manifest`]);
//! - `graphs/<id>.json`: each graph (see [`Graph`]);
//! - `files/<sha256>.<ext>`: each file the package holds, its bytes as they are (what the
//!   package calls it is in `package.json`);
//! - `editor.json`: how the editor showed it: the graph edited, where each graph's view was;
//! - whatever else other programs put there, written back as it was.
//!
//! Paths to files outside are written relative to the package wherever they can be (see
//! [`crate::package::relative_path`]): a package moved with its files keeps finding them.
//!
//! The format is extended as glTF is: a reader passes over fields it does not know; `extras`
//! hold what other programs keep with the package, a graph, a node or a file (kept as they
//! are); and what a reader could not pass over is named in `extensions_required`, which a
//! reader that does not know it refuses to read rather than lose.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io::{BufReader, BufWriter, Cursor, Read, Seek, Write};
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json};

use crate::archive::{self, Archive, Method, Prepared, Writer};
use crate::graph::{Graph, NodeId};
use crate::instance::GraphId;
use crate::package::{GraphEntry, Package, Resource, ResourceKind, relative_path, resolve_path};

pub const FILE_EXTENSION: &str = "brewtex";
/// What `mimetype` holds: the media type of a package file.
pub const MEDIA_TYPE: &str = "application/vnd.brewed-texture.package+zip";
/// What `package.json` says it is.
pub const FORMAT: &str = "brewed-texture-package";
/// The version of the format: made greater only when a reader of an earlier one would read a
/// file wrongly (what it can pass over, it does).
pub const VERSION: u32 = 1;
/// The extensions of the format this program reads (see the module): none yet.
const EXTENSIONS: &[&str] = &[];

const MIMETYPE: &str = "mimetype";
const MANIFEST: &str = "package.json";
const EDITOR: &str = "editor.json";
/// A snapshot's (see [`crate::recovery`]): what it says of its package.
const RECOVERY: &str = "recovery.json";
/// A picture of the package, for lists of packages (the Home screen's): of the first output of
/// the graph that was being edited, as it was when the package was saved.
const PICTURE: &str = "thumbnail.png";
/// The largest picture read: a larger one is no picture for a list.
const MAX_PICTURE: u64 = 4 << 20;
const GRAPHS: &str = "graphs/";
const FILES: &str = "files/";

/// The most the JSON of a package or of a graph may take, and what other programs put in a
/// package, each and in all: more is not read (it would only fill the memory).
const MAX_JSON: u64 = 256 << 20;
const MAX_OTHER: u64 = 64 << 20;
const MAX_OTHERS: u64 = 256 << 20;

/// Where the editor showed a graph (not part of undo). A file keeps only the node the 2D view
/// showed: where the graph view was, and how near, is the editor's while it runs (a package
/// opens with its graph shown whole), and moving it is no change of the package.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewState {
    /// The node the 2D view showed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<NodeId>,
    #[serde(skip)]
    pub graph_pan: Option<[f32; 2]>,
    #[serde(skip)]
    pub graph_zoom: Option<f32>,
}

/// What a snapshot of a package with unsaved changes says of the package (see
/// [`crate::recovery`] and [`save_snapshot`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Unsaved {
    /// The file the package is saved as, if it is.
    pub path: Option<String>,
    /// What it is called where it is listed: its file's name, or its first graph's.
    pub title: String,
    /// When the snapshot was made, in seconds since 1970.
    pub time: u64,
}

/// What `package.json` says: what the file is, the package's graphs (their ids and names:
/// each is `graphs/<id>.json`), the files it uses (each as its nodes say it, and what the
/// package calls it), the id the next graph gets, the extensions of the format it uses, and what
/// other programs keep with it. Nothing is said twice: what can be worked out (where a file or a
/// graph is in the archive, what kind of file a file is, the packages the graphs use) is not
/// said. What a reader can do without, it takes as it can: a file of the package that cannot
/// be read is left out, saying so; the rest is as it is by default.
#[derive(Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    #[serde(default, skip_serializing_if = "String::is_empty", deserialize_with = "lenient")]
    generator: String,
    graphs: Vec<GraphItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    files: Vec<Json>,
    #[serde(default, deserialize_with = "lenient")]
    next_graph: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty", deserialize_with = "lenient")]
    extensions_used: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    extensions_required: Vec<String>,
    #[serde(default, skip_serializing_if = "Map::is_empty", deserialize_with = "lenient")]
    extras: Map<String, Json>,
}

#[derive(Serialize, Deserialize)]
struct GraphItem {
    id: GraphId,
    name: String,
}

/// A file a package uses: as its nodes say it (`embedded:<sha256>.<ext>` for a file the package
/// holds, else where it is on disk), and what the package calls it.
#[derive(Serialize, Deserialize)]
struct FileItem {
    file: String,
    #[serde(default, skip_serializing_if = "String::is_empty", deserialize_with = "lenient")]
    name: String,
    #[serde(default, skip_serializing_if = "Map::is_empty", deserialize_with = "lenient")]
    extras: Map<String, Json>,
}

/// What a field says, or, when it says what cannot be taken, as it is by default: for what a
/// reader can do without (not worth refusing a package over).
fn lenient<'de, D: serde::Deserializer<'de>, T: serde::de::DeserializeOwned + Default>(d: D) -> Result<T, D::Error> {
    Ok(serde_json::from_value(Json::deserialize(d)?).unwrap_or_default())
}

/// The entry of a package's file that holds the graph `id`.
fn graph_entry(id: GraphId) -> String {
    format!("{GRAPHS}{}.json", id.0)
}

/// What `editor.json` says: the graph being edited, and where each graph was shown.
#[derive(Serialize, Deserialize)]
struct Editor {
    open: GraphId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    views: Vec<GraphView>,
}

#[derive(Serialize, Deserialize)]
struct GraphView {
    graph: GraphId,
    #[serde(flatten)]
    view: ViewState,
}

/// A package read, and what was adjusted while reading it.
#[derive(Debug)]
pub struct Loaded {
    pub package: Package,
    /// Where the view of each graph was.
    pub views: HashMap<GraphId, ViewState>,
    /// The graph that was being edited.
    pub open: GraphId,
    pub notes: Vec<String>,
    /// What a snapshot of unsaved changes says of its package (see [`save_snapshot`]).
    pub unsaved: Option<Unsaved>,
    /// Its picture, a PNG (see [`save_with_picture`]).
    pub picture: Option<Vec<u8>>,
}

impl Loaded {
    /// The view of the graph that was being edited.
    pub fn view(&self) -> ViewState {
        self.views.get(&self.open).cloned().unwrap_or_default()
    }
}

/// Whether the files a package holds are written in its file, or kept apart (a snapshot's are,
/// see [`crate::recovery`]).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Held {
    In,
    Apart,
}

/// Save `package` as the file at `path`: whatever stops the program, the system or the power
/// meanwhile, the file is the one before or this one, whole (see [`crate::files::replace`]).
pub fn save(path: &Path, package: &Package, views: &HashMap<GraphId, ViewState>, open: GraphId) -> Result<(), String> {
    save_with_picture(path, package, views, open, None)
}

/// [`save`], with a picture of the package (a PNG), which lists of packages show.
pub fn save_with_picture(
    path: &Path,
    package: &Package,
    views: &HashMap<GraphId, ViewState>,
    open: GraphId,
    picture: Option<&[u8]>,
) -> Result<(), String> {
    write_file(path, package, views, open, path.parent(), Held::In, None, picture)
}

/// The picture of the package file at `path`, if it has one (see [`save_with_picture`]): read
/// without the rest of the package.
pub fn read_picture(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = Archive::new(BufReader::new(file)).ok()?;
    archive.entry(MANIFEST)?;
    archive.read(PICTURE, MAX_PICTURE).ok()
}

/// Save a snapshot of `package`, which has unsaved changes (see [`crate::recovery`]), as the
/// file at `path`: its paths as they are (the snapshot is elsewhere), the files it holds kept
/// apart, and what `unsaved` says of it.
pub fn save_snapshot(
    path: &Path,
    package: &Package,
    views: &HashMap<GraphId, ViewState>,
    open: GraphId,
    unsaved: &Unsaved,
) -> Result<(), String> {
    write_file(path, package, views, open, None, Held::Apart, Some(unsaved), None)
}

/// The file of `package`, which is (or will be) in `dir`: paths relative to `dir`.
pub fn to_bytes(package: &Package, views: &HashMap<GraphId, ViewState>, open: GraphId, dir: Option<&Path>) -> Result<Vec<u8>, String> {
    write(Vec::new(), package, views, open, dir, Held::In, None, None)
}

#[allow(clippy::too_many_arguments)]
fn write_file(
    path: &Path,
    package: &Package,
    views: &HashMap<GraphId, ViewState>,
    open: GraphId,
    dir: Option<&Path>,
    held: Held,
    unsaved: Option<&Unsaved>,
    picture: Option<&[u8]>,
) -> Result<(), String> {
    crate::files::replace(path, |beside| -> Result<(), String> {
        let file = std::fs::File::create(beside).map_err(|e| e.to_string())?;
        let out = write(BufWriter::new(file), package, views, open, dir, held, unsaved, picture)?;
        out.into_inner().map(drop).map_err(|e| e.error().to_string())
    })
}

/// JSON as a package file writes it, which reads back as it was (else the file is not written:
/// a file that cannot be read is worse than one not saved).
fn json<T: Serialize + for<'de> Deserialize<'de>>(value: &T, what: &str) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| format!("{what} cannot be written: {e}"))?;
    serde_json::from_slice::<T>(&bytes).map_err(|e| format!("{what} would not read back: {e}"))?;
    Ok(bytes)
}

#[allow(clippy::too_many_arguments)]
fn write<W: Write>(
    out: W,
    package: &Package,
    views: &HashMap<GraphId, ViewState>,
    open: GraphId,
    dir: Option<&Path>,
    held: Held,
    unsaved: Option<&Unsaved>,
    picture: Option<&[u8]>,
) -> Result<W, String> {
    let mut package = package.clone();
    if let Some(dir) = dir {
        package.map_paths(|p| relative_path(p, dir));
    }
    // The entries of graphs and of the files the package holds (each once), in order.
    let graph_entries: Vec<String> = package.graphs.iter().map(|g| graph_entry(g.id)).collect();
    let file_entries: Vec<Option<String>> =
        package.resources.iter().map(|r| crate::files::embedded_file(&r.path).map(|f| format!("{FILES}{f}"))).collect();
    let manifest = manifest(&package)?;
    let editor = Editor {
        open,
        views: package
            .graphs
            .iter()
            .filter_map(|g| views.get(&g.id).filter(|v| **v != ViewState::default()).map(|v| GraphView { graph: g.id, view: v.clone() }))
            .collect(),
    };
    // The entries, in order; made ready on every core (the graphs written, what deflates
    // deflated), then written one after the other.
    let mut parts: Vec<(&str, Part<'_>)> = vec![
        (MIMETYPE, Part::Bytes(Cow::Borrowed(MEDIA_TYPE.as_bytes()), Method::Stored)),
        (MANIFEST, Part::Bytes(Cow::Owned(json(&manifest, "The package")?), Method::Deflated)),
        (EDITOR, Part::Bytes(Cow::Owned(json(&editor, "The editor's views")?), Method::Deflated)),
    ];
    if let Some(unsaved) = unsaved {
        parts.push((RECOVERY, Part::Bytes(Cow::Owned(json(unsaved, "The snapshot")?), Method::Deflated)));
    }
    // (A PNG is compressed already.)
    if let Some(picture) = picture {
        parts.push((PICTURE, Part::Bytes(Cow::Borrowed(picture), Method::Stored)));
    }
    parts.extend(package.graphs.iter().zip(&graph_entries).map(|(g, entry)| (entry.as_str(), Part::Graph(g))));
    if held == Held::In {
        let mut written = HashSet::new();
        for (r, entry) in package.resources.iter().zip(&file_entries) {
            let (Some(entry), Some(bytes)) = (entry, &r.data) else { continue };
            if written.insert(entry.as_str()) {
                // Pictures are compressed already; vector graphics and fonts deflate well.
                let method = if r.kind == ResourceKind::Bitmap { Method::Stored } else { Method::Deflated };
                parts.push((entry, Part::Bytes(Cow::Borrowed(bytes), method)));
            }
        }
    }
    parts.extend(package.other_entries.iter().map(|(name, bytes)| (name.as_str(), Part::Bytes(Cow::Borrowed(bytes), Method::Deflated))));
    let ready: Vec<(&str, Result<Prepared<'_>, String>)> = {
        use rayon::prelude::*;
        parts.into_par_iter().map(|(name, part)| (name, part.prepare())).collect()
    };
    let io = |e: std::io::Error| e.to_string();
    let mut zip = Writer::new(out);
    for (name, prepared) in ready {
        zip.add_prepared(name, prepared?).map_err(io)?;
    }
    zip.finish().map_err(io)
}

/// What `package.json` says of `package`, its paths as they are.
fn manifest(package: &Package) -> Result<Manifest, String> {
    let files = package
        .resources
        .iter()
        .map(|r| serde_json::to_value(FileItem { file: r.path.clone(), name: r.name.clone(), extras: r.extras.clone() }))
        .collect::<Result<_, _>>()
        .map_err(|e| format!("The package cannot be written: {e}"))?;
    Ok(Manifest {
        format: FORMAT.into(),
        version: VERSION,
        generator: format!("{} {}", crate::APP_NAME, env!("CARGO_PKG_VERSION")),
        graphs: package.graphs.iter().map(|g| GraphItem { id: g.id, name: g.name.clone() }).collect(),
        files,
        next_graph: package.next_graph(),
        extensions_used: Vec::new(),
        extensions_required: Vec::new(),
        extras: package.extras.clone(),
    })
}

/// What a package holds, as a hash: of its `package.json` and its graphs as a file says them
/// (its paths as they are), and of what other programs keep in it; not of where the editor
/// showed it, its picture, nor what a snapshot says of it. A package saved again as it was has
/// the same (see [`crate::versions`]), whatever its view did meanwhile.
pub fn fingerprint(package: &Package) -> Result<String, String> {
    let mut parts = vec![json(&manifest(package)?, "The package")?];
    for g in &package.graphs {
        parts.push(json(&*g.graph, &format!("The graph '{}'", g.name))?);
    }
    for (name, bytes) in &package.other_entries {
        parts.push(name.as_bytes().to_vec());
        parts.push(bytes.to_vec());
    }
    Ok(hash_of(&parts))
}

/// [`fingerprint`] of the package file at `path` (one written with its paths as they are, as a
/// snapshot is): of its entries as they are.
pub fn fingerprint_file(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let mut archive = Archive::new(BufReader::new(file))?;
    let bytes = archive.read(MANIFEST, MAX_JSON)?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| format!("The package cannot be read: {e}"))?;
    let mut parts = vec![bytes];
    for item in &manifest.graphs {
        parts.push(archive.read(&graph_entry(item.id), MAX_JSON)?);
    }
    let others: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).filter(|n| is_other(n)).collect();
    for name in others {
        let bytes = archive.read(&name, MAX_OTHER)?;
        parts.push(name.into_bytes());
        parts.push(bytes);
    }
    Ok(hash_of(&parts))
}

/// The hash of `parts` one after the other, each told apart by its length.
fn hash_of(parts: &[Vec<u8>]) -> String {
    let mut all = Vec::with_capacity(parts.iter().map(|p| p.len() + 8).sum());
    for part in parts {
        all.extend_from_slice(&(part.len() as u64).to_le_bytes());
        all.extend_from_slice(part);
    }
    crate::files::sha256(&all)
}

/// Is `name` an entry other programs put in a package, kept as it is: none the format names,
/// nor the folders some tools list (their entries' names make them).
fn is_other(name: &str) -> bool {
    let ours = [MIMETYPE, MANIFEST, EDITOR, RECOVERY, PICTURE].contains(&name) || name.starts_with(GRAPHS) || name.starts_with(FILES);
    !ours && !(name.ends_with('/') && archive::is_entry_name(name.trim_end_matches('/')))
}

/// An entry of a package file, to be made ready (see [`archive::Prepared`]).
enum Part<'a> {
    Bytes(Cow<'a, [u8]>, Method),
    /// A graph, as JSON.
    Graph(&'a GraphEntry),
}

impl<'a> Part<'a> {
    fn prepare(self) -> Result<Prepared<'a>, String> {
        let (bytes, method) = match self {
            Part::Bytes(bytes, method) => (bytes, method),
            Part::Graph(g) => (Cow::Owned(json(&*g.graph, &format!("The graph '{}'", g.name))?), Method::Deflated),
        };
        Prepared::new(bytes, method).map_err(|e| e.to_string())
    }
}

/// Read the package file at `path`.
pub fn load(path: &Path) -> Result<Loaded, String> {
    let cannot = |e: String| format!("Cannot read {}: {e}", path.display());
    // (A package names the packages it uses, which could be anything: a device is read forever.)
    let meta = std::fs::metadata(path).map_err(|e| cannot(e.to_string()))?;
    if !meta.is_file() {
        return Err(cannot("it is not a file".into()));
    }
    if meta.len() == 0 {
        return Err(format!("{} is empty", path.display()));
    }
    let file = std::fs::File::open(path).map_err(|e| cannot(e.to_string()))?;
    read_from(BufReader::new(file), path.parent())
}

/// A graph of a package file, as a library shows it (see [`read_graphs`]).
#[derive(Clone, Debug, PartialEq)]
pub struct GraphSummary {
    pub id: GraphId,
    pub name: String,
    pub attributes: crate::graph::GraphAttributes,
    /// How many Output nodes it has: what a node that uses it gives.
    pub outputs: usize,
}

/// The graphs of the package file at `path`: their names and what they say of themselves,
/// read without the files the package holds (a library looks at many packages).
pub fn read_graphs(path: &Path) -> Result<Vec<GraphSummary>, String> {
    let cannot = |e: String| format!("Cannot read {}: {e}", path.display());
    let file = std::fs::File::open(path).map_err(|e| cannot(e.to_string()))?;
    let mut archive = Archive::new(BufReader::new(file)).map_err(|e| if e.contains("not an archive") { not_a_package() } else { e })?;
    if archive.entry(MANIFEST).is_none() {
        return Err(not_a_package());
    }
    let manifest: Manifest =
        serde_json::from_slice(&archive.read(MANIFEST, MAX_JSON)?).map_err(|e| format!("The package cannot be read: {e}"))?;
    if manifest.format != FORMAT {
        return Err(not_a_package());
    }
    if manifest.version > VERSION {
        return Err(format!("{} was made by a newer version of {}", path.display(), crate::APP_NAME));
    }
    let mut graphs = Vec::with_capacity(manifest.graphs.len());
    for item in &manifest.graphs {
        let bytes = archive.read(&graph_entry(item.id), MAX_JSON).map_err(|e| format!("The graph '{}' cannot be read: {e}", item.name))?;
        let graph: Graph = serde_json::from_slice(&bytes).map_err(|e| format!("The graph '{}' cannot be read: {e}", item.name))?;
        let outputs = graph.nodes_of(crate::NodeKind::OUTPUT).count();
        graphs.push(GraphSummary { id: item.id, name: item.name.clone(), attributes: graph.attributes, outputs });
    }
    Ok(graphs)
}

/// The files the package file at `path` uses, as its nodes say them (`embedded:<sha256>.<ext>`
/// for one it holds): read without the rest of the package.
pub fn read_files(path: &Path) -> Result<Vec<String>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let mut archive = Archive::new(BufReader::new(file))?;
    let manifest: Manifest =
        serde_json::from_slice(&archive.read(MANIFEST, MAX_JSON)?).map_err(|e| format!("The package cannot be read: {e}"))?;
    Ok(manifest.files.into_iter().filter_map(|f| serde_json::from_value::<FileItem>(f).ok()).map(|f| f.file).collect())
}

/// Read a package file of `bytes`, which is in `dir`: its relative paths are taken from there.
pub fn from_bytes(bytes: &[u8], dir: Option<&Path>) -> Result<Loaded, String> {
    read_from(Cursor::new(bytes), dir)
}

/// What a snapshot at `path` says of its package (see [`save_snapshot`]), read without reading
/// the package.
pub fn read_unsaved(path: &Path) -> Option<Unsaved> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = Archive::new(BufReader::new(file)).ok()?;
    serde_json::from_slice(&archive.read(RECOVERY, MAX_JSON).ok()?).ok()
}

fn not_a_package() -> String {
    format!("This is not a {} package", crate::APP_NAME)
}

/// Read a package file from `source`, which is in `dir`.
pub fn read_from<R: Read + Seek>(source: R, dir: Option<&Path>) -> Result<Loaded, String> {
    let mut archive = Archive::new(source).map_err(|e| if e.contains("not an archive") { not_a_package() } else { e })?;
    if archive.entry(MIMETYPE).is_some() && archive.read(MIMETYPE, 1024)? != MEDIA_TYPE.as_bytes() {
        return Err(not_a_package());
    }
    if archive.entry(MANIFEST).is_none() {
        return Err(not_a_package());
    }
    let manifest: Manifest =
        serde_json::from_slice(&archive.read(MANIFEST, MAX_JSON)?).map_err(|e| format!("The package cannot be read: {e}"))?;
    if manifest.format != FORMAT {
        return Err(not_a_package());
    }
    if manifest.version > VERSION {
        return Err(format!(
            "The package was made by a newer version of {} (format {}): update the program to open it",
            crate::APP_NAME,
            manifest.version
        ));
    }
    let unknown: Vec<&str> =
        manifest.extensions_required.iter().map(String::as_str).filter(|e| !EXTENSIONS.contains(e)).collect();
    if !unknown.is_empty() {
        return Err(format!("The package needs what this version does not read: {}", unknown.join(", ")));
    }
    let mut notes = Vec::new();
    let mut entries = Vec::new();
    for item in &manifest.graphs {
        let bytes = archive.read(&graph_entry(item.id), MAX_JSON).map_err(|e| format!("The graph '{}' cannot be read: {e}", item.name))?;
        let mut graph: Graph =
            serde_json::from_slice(&bytes).map_err(|e| format!("The graph '{}' cannot be read: {e}", item.name))?;
        let links = graph.links.len();
        graph.normalize();
        if graph.links.len() < links {
            notes.push(format!("{}: {} link(s) that led nowhere were removed", item.name, links - graph.links.len()));
        }
        entries.push(GraphEntry { id: item.id, name: item.name.clone(), graph: Arc::new(graph) });
    }
    if entries.is_empty() {
        return Err("The package has no graph".into());
    }
    let unsaved: Option<Unsaved> = match archive.entry(RECOVERY) {
        Some(_) => serde_json::from_slice(&archive.read(RECOVERY, MAX_JSON)?).ok(),
        None => None,
    };
    let mut resources = Vec::new();
    let mut missing = 0;
    // (Files of the same bytes are one entry, read once.)
    let mut read: HashMap<String, Arc<Vec<u8>>> = HashMap::new();
    for value in manifest.files {
        let item = match serde_json::from_value::<FileItem>(value) {
            Ok(item) => item,
            Err(e) => {
                notes.push(format!("A file of the package cannot be read, and was left out: {e}"));
                continue;
            }
        };
        let Some(kind) = ResourceKind::of_path(&item.file) else {
            notes.push(format!("'{}' is no file the package can use, and was left out", item.file));
            continue;
        };
        let mut data = None;
        if let Some(entry) = crate::files::embedded_file(&item.file).map(|f| format!("{FILES}{f}")) {
            match (read.get(&entry), archive.entry(&entry)) {
                (Some(bytes), _) => data = Some(bytes.clone()),
                (None, Some(_)) => match archive.read(&entry, crate::files::MAX_FILE) {
                    Ok(bytes) => {
                        let bytes = Arc::new(bytes);
                        read.insert(entry, bytes.clone());
                        data = Some(bytes);
                    }
                    Err(e) => notes.push(e),
                },
                // (A snapshot holds them apart.)
                (None, None) if unsaved.is_none() => missing += 1,
                (None, None) => {}
            }
        }
        resources.push(Resource { path: item.file, kind, name: item.name, data, extras: item.extras });
    }
    drop(read);
    if missing > 0 {
        notes.push(format!("{missing} file(s) the package holds are not in its file"));
    }
    // What other programs put in the package, as it is; not what no entry may be named (it
    // could not be written back).
    let others: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).filter(|n| is_other(n)).collect();
    let mut kept = 0;
    let mut other_entries = Vec::new();
    for name in others {
        if !archive::is_entry_name(&name) {
            notes.push(format!("Not kept: '{name}', which no entry may be named"));
            continue;
        }
        match archive.read(&name, MAX_OTHER.min(MAX_OTHERS - kept)) {
            Ok(bytes) => {
                kept += bytes.len() as u64;
                other_entries.push((name, Arc::new(bytes)));
            }
            Err(e) => notes.push(format!("Not kept: {e}")),
        }
    }
    let picture = if archive.entry(PICTURE).is_some() { archive.read(PICTURE, MAX_PICTURE).ok() } else { None };
    let editor: Option<Editor> = match archive.entry(EDITOR) {
        Some(_) => archive.read(EDITOR, MAX_JSON).ok().and_then(|b| serde_json::from_slice(&b).ok()),
        None => None,
    };
    let mut package = Package::from_parts(entries, resources, manifest.next_graph);
    package.extras = manifest.extras;
    package.other_entries = other_entries;
    if let Some(dir) = dir {
        package.map_paths(|p| resolve_path(p, dir));
    }
    let lost = package.resources.iter().filter(|r| crate::files::is_embedded(&r.path) && r.data.is_none()).count();
    if lost > missing && unsaved.is_none() {
        notes.push(format!("{} file(s) the package holds are not what their names say, and were left out", lost - missing));
    }
    let mut views = HashMap::new();
    let mut open = package.first();
    if let Some(editor) = editor {
        for v in editor.views {
            if let Some(entry) = package.graph(v.graph) {
                let mut view = v.view;
                view.preview = view.preview.filter(|n| entry.graph.contains(*n));
                views.insert(v.graph, view);
            }
        }
        if package.graph(editor.open).is_some() {
            open = editor.open;
        }
    }
    Ok(Loaded { package, views, open, notes, unsaved, picture })
}

/// The entries of a package file as the format names them, for what checks it (tests).
#[cfg(test)]
fn entry_names(bytes: &[u8]) -> Vec<String> {
    Archive::new(Cursor::new(bytes)).unwrap().entries().iter().map(|e| e.name.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo() -> (Package, HashMap<GraphId, ViewState>, GraphId) {
        let (package, preview) = crate::demo::package();
        let open = package.first();
        let views = [(open, ViewState { preview, graph_pan: Some([1.0, 2.0]), graph_zoom: Some(0.8) })].into_iter().collect();
        (package, views, open)
    }

    /// A package's fingerprint is that of its file, whatever the view did; a node moved, or
    /// renamed, is another.
    #[test]
    fn fingerprints_say_what_a_package_holds() {
        let (mut package, views, open) = demo();
        let dir = std::env::temp_dir().join(format!("brewed-fingerprint-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.brewtex");
        let unsaved = Unsaved { path: None, title: "a".into(), time: 1 };
        save_snapshot(&file, &package, &views, open, &unsaved).unwrap();
        let first = fingerprint(&package).unwrap();
        assert_eq!(fingerprint_file(&file).unwrap(), first, "the package's is its file's");
        let elsewhere = [(open, ViewState { graph_pan: Some([500.0, 9.0]), graph_zoom: Some(2.0), ..views[&open].clone() })].into_iter().collect();
        save_snapshot(&file, &package, &elsewhere, open, &Unsaved { time: 2, ..unsaved.clone() }).unwrap();
        assert_eq!(fingerprint_file(&file).unwrap(), first, "the view moved: the same package");
        let graph = Arc::make_mut(&mut package.graphs[0].graph);
        graph.nodes[0].pos[0] += 1.0;
        assert_ne!(fingerprint(&package).unwrap(), first, "a node moved: another");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A package is read as it was written: its graphs, the files it holds and those it links
    /// to, the node each view showed (not where the view was, nor how near: the editor's), the
    /// graph edited; written twice, it is the same bytes; its entries are those the format
    /// names, `mimetype` first.
    #[test]
    fn roundtrip() {
        let (mut package, views, open) = demo();
        let half = Graph { precision: crate::Precision::Half, ..Graph::default() };
        let second = package.add_graph("Second", half);
        let png = Arc::new(b"\x89PNG not really".to_vec());
        let font = Arc::new(vec![7u8; 3000]);
        let font_bytes = font.clone();
        package.resources.push(Resource::embedded("bricks.png", ResourceKind::Bitmap, png.clone()));
        package.resources.push(Resource::embedded("C:/fonts/Шрифт.ttf", ResourceKind::Font, font.clone()));
        package.resources.push(Resource::linked(somewhere(&["tex", "linked.png"]), ResourceKind::Bitmap));
        let bytes = to_bytes(&package, &views, second, None).unwrap();
        let loaded = from_bytes(&bytes, None).unwrap();
        assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
        assert_eq!(loaded.package, package);
        assert_eq!(loaded.views[&open], ViewState { preview: views[&open].preview, ..ViewState::default() });
        assert_eq!(loaded.open, second);
        let held: Vec<_> = loaded.package.resources.iter().filter_map(|r| r.data.clone()).collect();
        assert_eq!(held, [png.clone(), font], "the bytes of the files it holds");
        assert_eq!(to_bytes(&package, &views, second, None).unwrap(), bytes, "the same bytes");
        let names = entry_names(&bytes);
        assert_eq!(names[..3], ["mimetype", "package.json", "editor.json"]);
        assert!(names.contains(&"graphs/1.json".to_string()) && names.contains(&"graphs/2.json".to_string()));
        // Each file it holds, by its bytes and extension; what the package calls it, beside.
        let sha = crate::files::sha256(&png);
        assert!(names.contains(&format!("files/{sha}.png")), "{names:?}");
        assert!(names.contains(&format!("files/{}.ttf", crate::files::sha256(&font_bytes))), "{names:?}");
        assert_eq!(names.len(), 7);
        let manifest: Json = serde_json::from_slice(&Archive::new(Cursor::new(&bytes)).unwrap().read(MANIFEST, 1 << 20).unwrap()).unwrap();
        let files: Vec<(&str, &str)> =
            manifest["files"].as_array().unwrap().iter().map(|f| (f["file"].as_str().unwrap(), f["name"].as_str().unwrap())).collect();
        assert_eq!(files[0], (format!("embedded:{sha}.png").as_str(), "bricks.png"));
        assert_eq!(files[1].1, "Шрифт.ttf");
        assert!(manifest.get("packages").is_none() && manifest["graphs"][0].get("entry").is_none(), "nothing said twice");
        assert!(from_bytes(b"not a zip", None).err().unwrap().contains("not a Brewed Texture Designer package"));
    }

    /// What is not a package says so; one made by a newer version, or needing what this version
    /// does not read, says so and is not read (rather than read wrongly and saved without it).
    #[test]
    fn what_is_not_a_package_says_so() {
        let (package, views, open) = demo();
        let bytes = to_bytes(&package, &views, open, None).unwrap();
        let with_manifest = |edit: &dyn Fn(&mut Json)| {
            let mut archive = Archive::new(Cursor::new(bytes.clone())).unwrap();
            let names: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).collect();
            let mut zip = Writer::new(Vec::new());
            for name in names {
                let mut content = archive.read(&name, 1 << 30).unwrap();
                if name == MANIFEST {
                    let mut manifest: Json = serde_json::from_slice(&content).unwrap();
                    edit(&mut manifest);
                    content = serde_json::to_vec(&manifest).unwrap();
                }
                zip.add(&name, &content, Method::Deflated).unwrap();
            }
            from_bytes(&zip.finish().unwrap(), None)
        };
        assert!(with_manifest(&|_| {}).is_ok());
        let newer = with_manifest(&|m| m["version"] = (VERSION + 1).into()).err().unwrap();
        assert!(newer.contains("newer version"), "{newer}");
        let needs = with_manifest(&|m| m["extensions_required"] = serde_json::json!(["x_terrain"])).err().unwrap();
        assert!(needs.contains("x_terrain"), "{needs}");
        assert!(with_manifest(&|m| m["extensions_used"] = serde_json::json!(["x_hint"])).is_ok(), "used, not needed: read");
        assert!(with_manifest(&|m| m["format"] = "something-else".into()).err().unwrap().contains("not a"));
        // Another archive.
        let mut zip = Writer::new(Vec::new());
        zip.add("mimetype", b"application/epub+zip", Method::Stored).unwrap();
        zip.add("package.json", b"{}", Method::Stored).unwrap();
        assert!(from_bytes(&zip.finish().unwrap(), None).err().unwrap().contains("not a"));
    }

    /// What other programs keep with a package, a graph, a node or a file, and the entries they
    /// put in its file, are written back as they were.
    #[test]
    fn what_other_programs_keep_is_kept() {
        let (mut package, views, open) = demo();
        package.extras.insert("studio".into(), serde_json::json!({ "asset": "wall_03", "lod": [0, 1] }));
        let g = Arc::make_mut(&mut package.graphs[0].graph);
        g.extras.insert("reviewed".into(), true.into());
        g.nodes[0].extras.insert("note".into(), "keep me".into());
        package.resources.push(Resource::embedded("a.png", ResourceKind::Bitmap, Arc::new(vec![1, 2, 3])));
        package.resources.last_mut().unwrap().extras.insert("source".into(), "scan 12".into());
        package.other_entries.push(("x-studio/review.txt".into(), Arc::new(b"approved".to_vec())));
        let loaded = from_bytes(&to_bytes(&package, &views, open, None).unwrap(), None).unwrap();
        assert_eq!(loaded.package, package);
        assert_eq!(loaded.package.other_entries, package.other_entries);
        assert_eq!(loaded.package.graphs[0].graph.nodes[0].extras["note"], "keep me");
    }

    /// What some tools put in an archive that is no entry of a package, the folders they list
    /// and names no entry may have, is not kept (the second saying so): the package is saved
    /// all the same.
    #[test]
    fn folders_and_odd_names_of_other_tools_are_not_kept() {
        let (package, views, open) = demo();
        let bytes = to_bytes(&package, &views, open, None).unwrap();
        // Written under names as long, then named as those tools name them.
        let mut archive = Archive::new(Cursor::new(bytes)).unwrap();
        let mut zip = Writer::new(Vec::new());
        for e in archive.entries().to_vec() {
            let content = archive.read(&e.name, 1 << 30).unwrap();
            zip.add(&e.name, &content, Method::Stored).unwrap();
        }
        zip.add("x-studioX", b"", Method::Stored).unwrap();
        zip.add("x/aa/y", b"odd", Method::Stored).unwrap();
        zip.add("x-studio/notes.txt", b"kept", Method::Stored).unwrap();
        let mut bytes = zip.finish().unwrap();
        for (from, to) in [(&b"x-studioX"[..], &b"x-studio/"[..]), (b"x/aa/y", b"x/../y")] {
            let places: Vec<usize> = bytes.windows(from.len()).enumerate().filter(|(_, w)| *w == from).map(|(i, _)| i).collect();
            assert_eq!(places.len(), 2, "the entry and the directory name it");
            for at in places {
                bytes[at..at + to.len()].copy_from_slice(to);
            }
        }
        let loaded = from_bytes(&bytes, None).unwrap();
        let kept: Vec<&str> = loaded.package.other_entries.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(kept, ["x-studio/notes.txt"]);
        assert!(loaded.notes.iter().any(|n| n.contains("'x/../y'")), "{:?}", loaded.notes);
        let again = from_bytes(&to_bytes(&loaded.package, &loaded.views, loaded.open, None).unwrap(), None).unwrap();
        assert_eq!(again.package.other_entries, loaded.package.other_entries);
    }

    /// A file of the same bytes, whatever it was called, is one file of the package, held
    /// once: taken in again, it is the one there, as it is called there.
    #[test]
    fn the_same_bytes_are_held_once() {
        use crate::package::PackageEdit;
        let (mut package, views, open) = demo();
        let bytes = Arc::new(b"the same picture, twice".to_vec());
        let held = |name: &str| (name.to_string(), crate::files::Held::new(name, bytes.clone()));
        PackageEdit::Import(vec![held("C:/a/bricks.png"), held("C:/b/wall.png")]).apply(&mut package).unwrap();
        let names: Vec<&str> = package.resources.iter().map(|r| r.name()).collect();
        assert_eq!(names, ["bricks.png"]);
        let file = to_bytes(&package, &views, open, None).unwrap();
        assert_eq!(entry_names(&file).iter().filter(|n| n.starts_with(FILES)).count(), 1);
        let loaded = from_bytes(&file, None).unwrap();
        assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
        assert_eq!(loaded.package.resources.len(), 1);
        assert_eq!(loaded.package.resources[0].data.as_deref(), Some(&*bytes));
    }

    /// What a package calls a file is its own to say, and to change: not what the file holds,
    /// nor how nodes read it. Names are each their own; one a file cannot take is refused, and
    /// one a file said that another has, made its own on reading.
    #[test]
    fn files_are_renamed_in_the_package() {
        use crate::package::PackageEdit;
        let (mut package, views, open) = demo();
        let held = |name: &str, bytes: &[u8]| (name.to_string(), crate::files::Held::new(name, Arc::new(bytes.to_vec())));
        PackageEdit::Import(vec![held("C:/a/bricks.png", b"one"), held("C:/b/bricks.png", b"two")]).apply(&mut package).unwrap();
        let names: Vec<&str> = package.resources.iter().map(|r| r.name()).collect();
        assert_eq!(names, ["bricks.png", "bricks 2.png"], "names each their own");
        let second = package.resources[1].path.clone();
        PackageEdit::RenameResource { path: second.clone(), name: " Old wall.png ".into() }.apply(&mut package).unwrap();
        assert_eq!(package.resources[1].name, "Old wall.png");
        assert!(PackageEdit::RenameResource { path: second.clone(), name: "bricks.png".into() }.apply(&mut package).is_err());
        assert!(PackageEdit::RenameResource { path: second.clone(), name: "  ".into() }.apply(&mut package).is_err());
        assert_eq!(package.resources[1].path, second, "the file is the same");
        let loaded = from_bytes(&to_bytes(&package, &views, open, None).unwrap(), None).unwrap();
        assert_eq!(loaded.package.resources[1].name, "Old wall.png");
        // A linked file is called as its file, until renamed; moved, as its new file.
        PackageEdit::AddResources(vec!["C:/x/stone.png".into()]).apply(&mut package).unwrap();
        PackageEdit::Relink { from: "C:/x/stone.png".into(), to: "C:/y/rock.png".into() }.apply(&mut package).unwrap();
        assert!(package.resources.iter().any(|r| r.path == "C:/y/rock.png" && r.name == "rock.png"));
        PackageEdit::RenameResource { path: "C:/y/rock.png".into(), name: "Cliff".into() }.apply(&mut package).unwrap();
        PackageEdit::Relink { from: "C:/y/rock.png".into(), to: "C:/z/rock2.png".into() }.apply(&mut package).unwrap();
        assert!(package.resources.iter().any(|r| r.path == "C:/z/rock2.png" && r.name == "Cliff"), "a name given stays");
    }

    /// What a package's file says that a reader can do without, a reader takes as it can: a
    /// file of the package it cannot read is left out, saying so; a count or a note it cannot
    /// read is as it is by default. The package opens.
    #[test]
    fn what_can_be_done_without_does_not_keep_a_package_shut() {
        let (package, views, open) = demo();
        let bytes = to_bytes(&package, &views, open, None).unwrap();
        let mut archive = Archive::new(Cursor::new(bytes.clone())).unwrap();
        let names: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).collect();
        let mut zip = Writer::new(Vec::new());
        for name in names {
            let mut content = archive.read(&name, 1 << 30).unwrap();
            if name == MANIFEST {
                let mut manifest: Json = serde_json::from_slice(&content).unwrap();
                manifest["next_graph"] = serde_json::json!(-1);
                manifest["generator"] = serde_json::json!({ "what": "not text" });
                manifest["extras"] = serde_json::json!("not an object");
                manifest["files"] = serde_json::json!([
                    { "file": 5 },
                    { "file": "C:/tex/a.png", "name": 7, "size": -1 },
                    { "file": "C:/tex/readme.txt" }
                ]);
                content = serde_json::to_vec(&manifest).unwrap();
            }
            zip.add(&name, &content, Method::Deflated).unwrap();
        }
        let loaded = from_bytes(&zip.finish().unwrap(), None).unwrap();
        assert_eq!(loaded.package.graphs.len(), package.graphs.len());
        let files: Vec<(&str, &str)> = loaded.package.resources.iter().map(|r| (r.path.as_str(), r.name())).collect();
        assert_eq!(files, [("C:/tex/a.png", "a.png")]);
        assert_eq!(loaded.notes.len(), 2, "{:?}", loaded.notes);
        assert!(loaded.package.next_graph() > package.graphs.iter().map(|g| g.id.0).max().unwrap());
    }

    /// A file a package holds that is not what its name says (made so elsewhere) is left out,
    /// saying so; one its file lacks is missing, saying so: the package opens either way.
    #[test]
    fn files_that_are_not_what_they_say_are_left_out() {
        let (mut package, views, open) = demo();
        // (Bytes nothing in this run holds: what holds them would give them back by their name.)
        let picture = Arc::new(format!("the picture of {:?}", std::time::SystemTime::now()).into_bytes());
        let path = crate::files::embedded_path("a.png", &picture);
        package.resources.push(Resource { path, kind: ResourceKind::Bitmap, name: "a.png".into(), data: Some(picture), extras: Map::new() });
        let bytes = to_bytes(&package, &views, open, None).unwrap();
        /// What an entry, by its name, becomes (none: it goes).
        type Change<'a> = &'a dyn Fn(&str, Vec<u8>) -> Option<Vec<u8>>;
        let rewritten = |change: Change| {
            let mut archive = Archive::new(Cursor::new(bytes.clone())).unwrap();
            let names: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).collect();
            let mut zip = Writer::new(Vec::new());
            for name in names {
                let content = archive.read(&name, 1 << 30).unwrap();
                if let Some(content) = change(&name, content) {
                    zip.add(&name, &content, Method::Stored).unwrap();
                }
            }
            from_bytes(&zip.finish().unwrap(), None).unwrap()
        };
        let forged = rewritten(&|name, content| Some(if name.starts_with(FILES) { b"another".to_vec() } else { content }));
        assert!(forged.package.resources.iter().any(|r| r.name == "a.png" && r.data.is_none()));
        assert!(forged.notes.iter().any(|n| n.contains("not what their names say")), "{:?}", forged.notes);
        let lacking = rewritten(&|name, content| (!name.starts_with(FILES)).then_some(content));
        assert!(lacking.notes.iter().any(|n| n.contains("not in its file")), "{:?}", lacking.notes);
    }

    /// A graph that is gone leaves its id behind: a new graph never takes it, in a file saved
    /// and read again too, and a node that used the graph that is gone does not use it.
    #[test]
    fn ids_of_graphs_that_are_gone_are_not_taken() {
        use crate::package::PackageEdit;
        let mut package = Package::new("Main", Graph::default());
        let gone = package.add_graph("Gone", Graph::default());
        PackageEdit::RemoveGraph(gone).apply(&mut package).unwrap();
        let bytes = to_bytes(&package, &HashMap::new(), package.first(), None).unwrap();
        let mut loaded = from_bytes(&bytes, None).unwrap().package;
        assert_eq!(loaded, package);
        let new = loaded.add_graph("New", Graph::default());
        assert!(new.0 > gone.0, "{new:?} after {gone:?}");
    }

    /// What is wrong with a graph, if anything: what the editor and the engine count on.
    fn whole(g: &Graph) -> Result<(), String> {
        let mut ids = std::collections::HashSet::new();
        for n in &g.nodes {
            if !ids.insert(n.id) || n.id.0 >= g.next_id || n.id.is_inner() {
                return Err(format!("node id {:?} taken twice, or not below {}", n.id, g.next_id));
            }
            if !n.pos.iter().all(|p| p.abs() <= crate::graph::MAX_COORD) {
                return Err(format!("{} at {:?}", n.def().name, n.pos));
            }
            if (n.kind == crate::NodeKind::GRAPH_INSTANCE) != n.instance.is_some() {
                return Err(format!("{} with {:?}", n.def().name, n.instance));
            }
        }
        for c in &g.comments {
            if !ids.insert(NodeId(c.id.0)) || c.id.0 >= g.next_id {
                return Err(format!("comment id {:?} taken twice, or not below {}", c.id, g.next_id));
            }
            let place = c.pos.iter().chain(&c.size).all(|v| v.abs() <= crate::graph::MAX_COORD);
            if !place || !c.color.iter().all(|v| (0.0..=1.0).contains(v)) || c.size[0] < 0.0 || c.size[1] < 0.0 {
                return Err(format!("a comment at {:?}, {:?} large, of {:?}", c.pos, c.size, c.color));
            }
        }
        for v in &g.variables {
            let values = [v.value, v.default, v.min, v.max];
            if !values.iter().all(crate::types::Value::is_finite) {
                return Err(format!("the input value {} holds {values:?}", v.name));
            }
        }
        let mut inputs = std::collections::HashSet::new();
        for l in &g.links {
            if !g.contains(l.from) || !g.contains(l.to) || !inputs.insert((l.to, l.to_socket.clone())) {
                return Err(format!("a link {l:?}"));
            }
        }
        Ok(())
    }

    /// Mangle the part `at` of a value (counted depth first), `how` says how: a number far
    /// out, or of another kind; a string odd; an array emptied, doubled or turned around; a
    /// field gone.
    fn mangle(v: &mut Json, at: &mut usize, how: usize) {
        let here = *at == 0;
        *at = at.wrapping_sub(1);
        match v {
            Json::Number(_) if here => {
                let far = [1e39, -1e39, 3e38, -3e38, 1e30, 0.0, -0.0, 1e-40, 4294967296.0, -1.0, 0.5];
                *v = serde_json::json!(far[how % far.len()]);
            }
            Json::String(text) if here => {
                let odd = ["", " ", "a b", "i:input", "o:output", "../x", "\u{0}", "ы"];
                *text = match how % 9 {
                    8 => "ы".repeat(5000),
                    k => odd[k].to_string(),
                };
            }
            Json::Bool(b) if here => *b = !*b,
            Json::Array(items) if here => match how % 3 {
                0 => items.clear(),
                1 => items.extend(items.clone()),
                _ => items.reverse(),
            },
            Json::Object(fields) if here => {
                if let Some(key) = fields.keys().nth(how % fields.len().max(1)).cloned() {
                    fields.remove(&key);
                }
            }
            Json::Array(items) => items.iter_mut().for_each(|i| mangle(i, at, how)),
            Json::Object(fields) => fields.values_mut().for_each(|f| mangle(f, at, how)),
            _ => {}
        }
    }

    /// How many parts a value has, itself included.
    fn parts(v: &Json) -> usize {
        1 + match v {
            Json::Array(items) => items.iter().map(parts).sum(),
            Json::Object(fields) => fields.values().map(parts).sum(),
            _ => 0,
        }
    }

    /// An absolute path, on every system.
    fn somewhere(parts: &[&str]) -> String {
        let mut path = std::env::temp_dir().join("brewed_fixture");
        for p in parts {
            path.push(p);
        }
        path.display().to_string()
    }

    /// A package of the demo and a graph that uses it, with a file and a font.
    fn a_package() -> Package {
        let (mut package, _) = crate::demo::package();
        let wall = package.first();
        let instance = crate::instance::Instance::new(
            crate::instance::GraphRef { package: None, graph: wall, name: package.graphs[0].name.clone() },
            crate::instance::Interface::of(&package.graphs[0].graph),
        );
        let mut g = Graph::default();
        g.add_instance(instance, [0.0, 0.0]);
        let bitmap = g.add_node(crate::NodeKind::BITMAP, [0.0, 300.0]);
        g.node_mut(bitmap).unwrap().param_mut("path").unwrap().value = crate::ParamValue::Text(somewhere(&["tex", "a.png"]));
        package.add_graph("Uses it", g);
        package.sync_resources();
        package.resources.push(Resource::linked(somewhere(&["fonts", "b.ttf"]), crate::package::ResourceKind::Font));
        package
    }

    /// Files and pastes mangled at random load whole, or say why they do not: what a package's
    /// file and each of its graphs say, and the clipboard.
    #[test]
    fn mangled_files_and_pastes_come_out_whole() {
        use crate::graph::Clipboard;
        let package = a_package();
        let demo = Graph::clone(&package.graphs[0].graph);
        let bytes = to_bytes(&package, &HashMap::new(), package.first(), None).unwrap();
        let mut archive = Archive::new(Cursor::new(bytes)).unwrap();
        let names: Vec<String> = archive.entries().iter().map(|e| e.name.clone()).collect();
        let contents: Vec<(String, Vec<u8>)> = names.into_iter().map(|n| (n.clone(), archive.read(&n, 1 << 30).unwrap())).collect();
        let jsons: Vec<(usize, Json)> = contents
            .iter()
            .enumerate()
            .filter(|(_, (n, _))| n.ends_with(".json"))
            .map(|(i, (_, c))| (i, serde_json::from_slice(c).unwrap()))
            .collect();
        let ids: Vec<_> = demo.nodes.iter().map(|n| n.id).collect();
        let clip = demo.copy_nodes(&ids).to_text();
        let clip: Json = serde_json::from_str(clip.strip_prefix(Clipboard::MAGIC).unwrap()).unwrap();
        let mut seed = 1u32;
        let (mut loaded, mut pasted) = (0, 0);
        for round in 0..4000 {
            let mut files = jsons.clone();
            let mut c = clip.clone();
            // A few parts of each.
            for _ in 0..1 + round % 3 {
                seed = crate::expr::builtins::pcg_hash(seed);
                let which = seed as usize % files.len();
                let f = &mut files[which].1;
                let mut at = (seed >> 8) as usize % parts(f);
                mangle(f, &mut at, (seed >> 20) as usize);
                seed = crate::expr::builtins::pcg_hash(seed);
                let mut at = seed as usize % parts(&c);
                mangle(&mut c, &mut at, (seed >> 20) as usize);
            }
            let mut zip = Writer::new(Vec::new());
            for (i, (name, content)) in contents.iter().enumerate() {
                match files.iter().find(|(k, _)| *k == i) {
                    Some((_, json)) => zip.add(name, json.to_string().as_bytes(), Method::Stored).unwrap(),
                    None => zip.add(name, content, Method::Stored).unwrap(),
                }
            }
            if let Ok(l) = from_bytes(&zip.finish().unwrap(), None) {
                loaded += 1;
                let p = &l.package;
                assert!(!p.graphs.is_empty(), "file of round {round}: no graph");
                assert!(p.graph(l.open).is_some(), "file of round {round}: open {:?}", l.open);
                let mut names = std::collections::HashSet::new();
                for g in &p.graphs {
                    assert!(names.insert(g.name.clone()) && !g.name.trim().is_empty(), "file of round {round}: {:?}", g.name);
                    whole(&g.graph).unwrap_or_else(|e| panic!("file of round {round}: {e}"));
                }
            }
            if let Some(clip) = Clipboard::from_text(&format!("{}{c}", Clipboard::MAGIC)) {
                let mut g = demo.clone();
                g.paste(&clip, [40.0, 40.0]);
                pasted += 1;
                whole(&g).unwrap_or_else(|e| panic!("paste of round {round}: {e}"));
            }
        }
        // Many survive their mangling, so what is checked is graphs, not refusals. (A value made
        // no value at all refuses its node, and a paste of it: rather than set it back unsaid.)
        assert!(loaded > 1000 && pasted > 500, "{loaded} files and {pasted} pastes of 4000");
    }

    #[test]
    fn save_and_load_file() {
        let dir = std::env::temp_dir().join(format!("brewed_project_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("textures")).unwrap();
        let path = dir.join("a.brewtex");
        // Files next to the project are written relative to it, and found again from wherever
        // it is.
        let mut package = a_package();
        let texture = dir.join("textures").join("a.png").display().to_string();
        package = {
            let mut p = package;
            p.map_paths(|x| if x.ends_with("a.png") { texture.clone() } else { x.to_string() });
            p
        };
        save(&path, &package, &HashMap::new(), package.first()).unwrap();
        let mut archive = Archive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let manifest = String::from_utf8(archive.read(MANIFEST, 1 << 20).unwrap()).unwrap();
        assert!(manifest.contains("\"textures/a.png\""), "relative: {manifest}");
        assert_eq!(load(&path).unwrap().package, package);
        let moved = dir.join("moved");
        std::fs::create_dir_all(moved.join("textures")).unwrap();
        std::fs::copy(&path, moved.join("a.brewtex")).unwrap();
        let there = load(&moved.join("a.brewtex")).unwrap().package;
        let found: Vec<&str> = there.files().map(|(.., p)| p).collect();
        assert_eq!(found, [moved.join("textures").join("a.png").display().to_string()]);
        // What is not a package says so.
        let not = |bytes: &[u8]| {
            std::fs::write(&path, bytes).unwrap();
            load(&path).map(|_| ()).unwrap_err()
        };
        assert_eq!(not(b"\x89PNG\r\n\x1a\n\xff\xfe"), "This is not a Brewed Texture Designer package");
        assert!(not(b"").ends_with("a.brewtex is empty"));
        assert!(load(&dir).unwrap_err().contains("not a file"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What a graph says of itself is kept with it, and read with the names of the graphs of a
    /// package without the files it holds (as a library reads many).
    #[test]
    fn graphs_are_read_with_what_they_say_of_themselves() {
        use crate::graph::GraphAttributes;
        let dir = std::env::temp_dir().join(format!("brewed_graph_list_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rust.brewtex");
        let mut rust = Graph::default();
        rust.add_node(crate::NodeKind::OUTPUT, [0.0, 0.0]);
        rust.attributes = GraphAttributes { description: "Orange rust".into(), category: "Metals/Rust".into(), hidden: false };
        let mut package = Package::new("Rust", rust);
        let mut helper = Graph::default();
        helper.attributes.hidden = true;
        let helper = package.add_graph("Helper", helper);
        save(&path, &package, &HashMap::new(), package.first()).unwrap();
        assert_eq!(load(&path).unwrap().package, package);
        let graphs = read_graphs(&path).unwrap();
        assert_eq!(graphs.len(), 2);
        assert_eq!((graphs[0].name.as_str(), graphs[0].outputs, graphs[0].attributes.category.as_str()), ("Rust", 1, "Metals/Rust"));
        assert_eq!((graphs[1].id, graphs[1].outputs, graphs[1].attributes.hidden), (helper, 0, true));
        // Said only where they are not as by default.
        let mut archive = Archive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let helper_json = String::from_utf8(archive.read(&graph_entry(helper), 1 << 20).unwrap()).unwrap();
        assert!(helper_json.contains("\"hidden\": true") && !helper_json.contains("description"), "{helper_json}");
        assert!(read_graphs(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A snapshot holds its package and what it says of it, not the files the package holds
    /// (kept apart), and is read as a package all the same.
    #[test]
    fn snapshots_hold_what_they_say() {
        let (mut package, views, open) = demo();
        package.resources.push(Resource::embedded("a.png", ResourceKind::Bitmap, Arc::new(vec![9; 100])));
        let dir = std::env::temp_dir().join(format!("brewed_snapshot_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("1.brewtex");
        let unsaved = Unsaved { path: Some("C:/work/a.brewtex".into()), title: "a.brewtex".into(), time: 42 };
        save_snapshot(&file, &package, &views, open, &unsaved).unwrap();
        assert_eq!(read_unsaved(&file), Some(unsaved.clone()));
        let names = entry_names(&std::fs::read(&file).unwrap());
        assert!(names.contains(&RECOVERY.to_string()) && !names.iter().any(|n| n.starts_with(FILES)), "{names:?}");
        let loaded = load(&file).unwrap();
        assert_eq!(loaded.unsaved, Some(unsaved));
        assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
