//! Project files (`.brewtex`, JSON): a package (see [`crate::package`]).
//!
//! A file holds the package's graphs, each with where its view was, the graph that was being
//! edited and the files the package uses. Paths are written relative to the file wherever
//! they can be (see [`crate::package::relative_path`]): a project moved or copied with its
//! files keeps finding them. Files of version 1 held one graph; they are read as a package of
//! that graph.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::graph::{Graph, NodeId};
use crate::instance::GraphId;
use crate::package::{GraphEntry, Package, Resource, relative_path, resolve_path};

pub const FILE_EXTENSION: &str = "brewtex";
const FORMAT: &str = "brewed-texture-designer";
/// 3: the formats of an Output are those of [`crate::nodes::OUTPUT_DEPTHS`] (in 2, "32 bit
/// float" was the third, which 16 bits float is now).
const VERSION: u32 = 3;

/// View settings stored with a graph (not part of undo).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<NodeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_pan: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_zoom: Option<f32>,
}

impl ViewState {
    fn is_empty(&self) -> bool {
        *self == ViewState::default()
    }
}

#[derive(Serialize, Deserialize)]
struct FileGraph {
    id: GraphId,
    name: String,
    graph: Graph,
    #[serde(default, skip_serializing_if = "ViewState::is_empty")]
    view: ViewState,
}

#[derive(Serialize)]
struct FileGraphOut<'a> {
    id: GraphId,
    name: &'a str,
    graph: &'a Graph,
    #[serde(skip_serializing_if = "ViewState::is_empty")]
    view: ViewState,
}

#[derive(Deserialize)]
struct ProjectFile {
    #[serde(default)]
    graphs: Vec<FileGraph>,
    #[serde(default)]
    resources: Vec<Resource>,
    #[serde(default)]
    open: Option<GraphId>,
    /// The id the next graph gets (see [`Package::next_graph`]).
    #[serde(default)]
    next_graph: u64,
    /// Version 1: the one graph, and where its view was.
    #[serde(default)]
    graph: Option<Graph>,
    #[serde(default)]
    view: ViewState,
}

#[derive(Serialize)]
struct ProjectOut<'a> {
    format: &'a str,
    version: u32,
    graphs: Vec<FileGraphOut<'a>>,
    #[serde(skip_serializing_if = "<[Resource]>::is_empty")]
    resources: &'a [Resource],
    open: GraphId,
    next_graph: u64,
}

/// A loaded project and what was adjusted while loading it.
pub struct Loaded {
    pub package: Package,
    /// Where the view of each graph was.
    pub views: HashMap<GraphId, ViewState>,
    /// The graph that was being edited.
    pub open: GraphId,
    pub notes: Vec<String>,
}

impl Loaded {
    /// The view of the graph that was being edited.
    pub fn view(&self) -> ViewState {
        self.views.get(&self.open).cloned().unwrap_or_default()
    }
}

/// The file of `package`, which is (or will be) in `dir`: paths relative to `dir`.
pub fn to_json(package: &Package, views: &HashMap<GraphId, ViewState>, open: GraphId, dir: Option<&Path>) -> Result<String, String> {
    let mut package = package.clone();
    if let Some(dir) = dir {
        package.map_paths(|p| relative_path(p, dir));
    }
    let graphs = package
        .graphs
        .iter()
        .map(|g| FileGraphOut { id: g.id, name: &g.name, graph: &g.graph, view: views.get(&g.id).cloned().unwrap_or_default() })
        .collect();
    let file = ProjectOut {
        format: FORMAT,
        version: VERSION,
        graphs,
        resources: &package.resources,
        open,
        next_graph: package.next_graph(),
    };
    serde_json::to_string_pretty(&file).map_err(|e| e.to_string())
}

pub fn save(path: &Path, package: &Package, views: &HashMap<GraphId, ViewState>, open: GraphId) -> Result<(), String> {
    let json = to_json(package, views, open, path.parent())?;
    // Write next to the target and rename, so a crash never leaves half a file.
    let tmp = path.with_extension("tmp-save");
    std::fs::write(&tmp, json).map_err(|e| format!("Cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Cannot write {}: {e}", path.display())
    })
}

pub fn load(path: &Path) -> Result<Loaded, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    // A picture, or anything else that is not text, is not a project either.
    let text = String::from_utf8(bytes).map_err(|_| format!("This is not a {} project", crate::APP_NAME))?;
    if text.trim().is_empty() {
        return Err(format!("{} is empty", path.display()));
    }
    let name = path.file_stem().map_or_else(|| "Graph".to_string(), |s| s.to_string_lossy().to_string());
    from_json(&text, path.parent(), &name)
}

/// Read a project that is in `dir`: its relative paths are taken from there. `name` is what
/// the one graph of a file of version 1 is called.
pub fn from_json(text: &str, dir: Option<&Path>, name: &str) -> Result<Loaded, String> {
    let json: Json = serde_json::from_str(text).map_err(|e| format!("Not a valid project file: {e}"))?;
    let mut notes = Vec::new();
    if json.get("format").and_then(Json::as_str) != Some(FORMAT) {
        return Err(format!("This is not a {} project", crate::APP_NAME));
    }
    let version = json.get("version").and_then(Json::as_u64).unwrap_or(0);
    if version > VERSION as u64 {
        notes.push(format!("The file was written by a newer version (format {version}); some data may be lost"));
    }
    let file: ProjectFile = serde_json::from_value(json).map_err(|e| format!("Cannot read the project: {e}"))?;
    let mut views = HashMap::new();
    let mut entries: Vec<GraphEntry> = if !file.graphs.is_empty() {
        file.graphs
            .into_iter()
            .map(|g| {
                views.entry(g.id).or_insert(g.view);
                GraphEntry { id: g.id, name: g.name, graph: Arc::new(g.graph) }
            })
            .collect()
    } else if let Some(graph) = file.graph {
        views.insert(GraphId(1), file.view);
        vec![GraphEntry { id: GraphId(1), name: name.to_string(), graph: Arc::new(graph) }]
    } else {
        return Err("The project has no graph".into());
    };
    for entry in &mut entries {
        let graph = Arc::make_mut(&mut entry.graph);
        if version < 3 {
            for node in graph.nodes.iter_mut().filter(|n| n.kind == crate::NodeKind::OUTPUT) {
                if let Some(slot) = node.param_mut("bit_depth")
                    && slot.value == crate::ParamValue::Enum(2)
                {
                    slot.value = crate::ParamValue::Enum(crate::nodes::DEPTH_32F);
                }
            }
        }
        let links_before = graph.links.len();
        graph.normalize();
        if graph.links.len() < links_before {
            notes.push(format!("{}: {} invalid link(s) were removed", entry.name, links_before - graph.links.len()));
        }
    }
    let mut package = Package::from_parts(entries, file.resources, file.next_graph);
    if let Some(dir) = dir {
        package.map_paths(|p| resolve_path(p, dir));
    }
    views.retain(|id, _| package.graph(*id).is_some());
    for (id, view) in &mut views {
        let graph = &package.graph(*id).expect("kept above").graph;
        view.preview = view.preview.filter(|n| graph.contains(*n));
    }
    let open = file.open.filter(|id| package.graph(*id).is_some()).unwrap_or_else(|| package.first());
    Ok(Loaded { package, views, open, notes })
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

    #[test]
    fn roundtrip() {
        let (mut package, views, open) = demo();
        let half = Graph { precision: crate::Precision::Half, ..Graph::default() };
        let second = package.add_graph("Second", half);
        let json = to_json(&package, &views, second, None).unwrap();
        let loaded = from_json(&json, None, "x").unwrap();
        assert_eq!(loaded.package, package);
        assert_eq!(loaded.views[&open], views[&open]);
        assert_eq!(loaded.open, second);
        assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
        assert!(from_json("{}", None, "x").is_err());
        assert!(from_json("not json", None, "x").is_err());
    }

    /// An Output of 32 bits float in a file of version 2 is one in this version too, where the
    /// third format is 16 bits float; its other formats are what they were.
    #[test]
    fn outputs_of_version_2_keep_their_format() {
        use crate::{NodeKind, ParamValue};
        let mut g = Graph::default();
        let depths = [0, 1, 2];
        for (i, depth) in depths.iter().enumerate() {
            let out = g.add_node(NodeKind::OUTPUT, [0.0, 200.0 * i as f32]);
            g.node_mut(out).unwrap().param_mut("bit_depth").unwrap().value = ParamValue::Enum(*depth);
        }
        let package = Package::new("Old", g);
        let json = to_json(&package, &HashMap::new(), package.first(), None).unwrap();
        let old = json.replace(&format!("\"version\": {VERSION}"), "\"version\": 2");
        assert_ne!(old, json);
        let depths_of = |text: &str| -> Vec<u32> {
            let loaded = from_json(text, None, "x").unwrap();
            loaded.package.graphs[0].graph.nodes.iter().map(|n| n.enum_value("bit_depth")).collect()
        };
        assert_eq!(depths_of(&old), [crate::nodes::DEPTH_8, crate::nodes::DEPTH_16, crate::nodes::DEPTH_32F]);
        assert_eq!(depths_of(&json), [0, 1, 2], "this version's third format stays what it is");
    }

    /// A graph that is gone leaves its id behind: a new graph never takes it, in a file saved
    /// and read again too, and a node that used the graph that is gone does not use it.
    #[test]
    fn ids_of_graphs_that_are_gone_are_not_taken() {
        use crate::package::PackageEdit;
        let mut package = Package::new("Main", Graph::default());
        let gone = package.add_graph("Gone", Graph::default());
        PackageEdit::RemoveGraph(gone).apply(&mut package).unwrap();
        let json = to_json(&package, &HashMap::new(), package.first(), None).unwrap();
        let mut loaded = from_json(&json, None, "x").unwrap().package;
        assert_eq!(loaded, package);
        let new = loaded.add_graph("New", Graph::default());
        assert!(new.0 > gone.0, "{new:?} after {gone:?}");
    }

    /// A file of version 1 is a package of its one graph, named after the file.
    #[test]
    fn a_file_of_one_graph() {
        let (graph, preview) = crate::demo::graph();
        let v1 = serde_json::json!({ "format": FORMAT, "version": 1, "graph": graph, "view": { "preview": preview } });
        let loaded = from_json(&v1.to_string(), None, "wall").unwrap();
        assert_eq!(loaded.package.graphs.len(), 1);
        assert_eq!(loaded.package.graphs[0].name, "wall");
        assert_eq!(*loaded.package.graphs[0].graph, graph);
        assert_eq!(loaded.view().preview, preview);
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
        let instance = crate::instance::Instance {
            graph: crate::instance::GraphRef { package: None, graph: wall, name: package.graphs[0].name.clone() },
            interface: crate::instance::Interface::of(&package.graphs[0].graph),
        };
        let mut g = Graph::default();
        g.add_instance(instance, [0.0, 0.0]);
        let bitmap = g.add_node(crate::NodeKind::BITMAP, [0.0, 300.0]);
        g.node_mut(bitmap).unwrap().param_mut("path").unwrap().value = crate::ParamValue::Text(somewhere(&["tex", "a.png"]));
        package.add_graph("Uses it", g);
        package.sync_resources();
        package.resources.push(Resource { path: somewhere(&["fonts", "b.ttf"]), kind: crate::package::ResourceKind::Font });
        package
    }

    /// Files and pastes mangled at random load whole, or say why they do not.
    #[test]
    fn mangled_files_and_pastes_come_out_whole() {
        use crate::graph::Clipboard;
        let package = a_package();
        let demo = Graph::clone(&package.graphs[0].graph);
        let file = to_json(&package, &HashMap::new(), package.first(), None).unwrap();
        let file: Json = serde_json::from_str(&file).unwrap();
        let ids: Vec<_> = demo.nodes.iter().map(|n| n.id).collect();
        let clip = demo.copy_nodes(&ids).to_text();
        let clip: Json = serde_json::from_str(clip.strip_prefix(Clipboard::MAGIC).unwrap()).unwrap();
        let mut seed = 1u32;
        let (mut loaded, mut pasted) = (0, 0);
        for round in 0..4000 {
            let (mut f, mut c) = (file.clone(), clip.clone());
            // A few parts of each.
            for _ in 0..1 + round % 3 {
                seed = crate::expr::builtins::pcg_hash(seed);
                let mut at = seed as usize % parts(&f);
                mangle(&mut f, &mut at, (seed >> 20) as usize);
                seed = crate::expr::builtins::pcg_hash(seed);
                let mut at = seed as usize % parts(&c);
                mangle(&mut c, &mut at, (seed >> 20) as usize);
            }
            if let Ok(l) = from_json(&f.to_string(), None, "x") {
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
        // Most survive their mangling, so what is checked is graphs, not refusals.
        assert!(loaded > 1000 && pasted > 1000, "{loaded} files and {pasted} pastes of 4000");
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
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"textures/a.png\""), "relative: {text}");
        assert_eq!(load(&path).unwrap().package, package);
        let moved = dir.join("moved");
        std::fs::create_dir_all(moved.join("textures")).unwrap();
        std::fs::copy(&path, moved.join("a.brewtex")).unwrap();
        let there = load(&moved.join("a.brewtex")).unwrap().package;
        let found: Vec<&str> = there.files().map(|(.., p)| p).collect();
        assert_eq!(found, [moved.join("textures").join("a.png").display().to_string()]);
        // What is not a project says so.
        let not = |bytes: &[u8]| {
            std::fs::write(&path, bytes).unwrap();
            load(&path).map(|_| ()).unwrap_err()
        };
        assert_eq!(not(b"\x89PNG\r\n\x1a\n\xff\xfe"), "This is not a Brewed Texture Designer project");
        assert!(not(b"").ends_with("a.brewtex is empty"));
        assert!(not(b" \n").ends_with("a.brewtex is empty"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
