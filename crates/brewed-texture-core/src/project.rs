//! Project files (`.brewtex`, JSON).

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::graph::{Graph, NodeId};

pub const FILE_EXTENSION: &str = "brewtex";
const FORMAT: &str = "brewed-texture-designer";
const VERSION: u32 = 1;

/// View settings stored with the document (not part of undo).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<NodeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_pan: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_zoom: Option<f32>,
}

#[derive(Serialize, Deserialize)]
struct ProjectFile {
    format: String,
    version: u32,
    graph: Graph,
    #[serde(default)]
    view: ViewState,
}

/// A loaded project and what was adjusted while loading it.
pub struct Loaded {
    pub graph: Graph,
    pub view: ViewState,
    pub notes: Vec<String>,
}

pub fn to_json(graph: &Graph, view: &ViewState) -> Result<String, String> {
    let file = ProjectFile { format: FORMAT.into(), version: VERSION, graph: graph.clone(), view: view.clone() };
    serde_json::to_string_pretty(&file).map_err(|e| e.to_string())
}

pub fn save(path: &Path, graph: &Graph, view: &ViewState) -> Result<(), String> {
    let json = to_json(graph, view)?;
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
    from_json(&text)
}

pub fn from_json(text: &str) -> Result<Loaded, String> {
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
    let mut graph = file.graph;
    let links_before = graph.links.len();
    graph.normalize();
    if graph.links.len() < links_before {
        notes.push(format!("{} invalid link(s) were removed", links_before - graph.links.len()));
    }
    let mut view = file.view;
    view.preview = view.preview.filter(|id| graph.contains(*id));
    Ok(Loaded { graph, view, notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let (graph, preview) = crate::demo::graph();
        let view = ViewState { preview, graph_pan: Some([1.0, 2.0]), graph_zoom: Some(0.8) };
        let json = to_json(&graph, &view).unwrap();
        let loaded = from_json(&json).unwrap();
        assert_eq!(loaded.graph, graph);
        assert_eq!(loaded.view, view);
        assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
        assert!(from_json("{}").is_err());
        assert!(from_json("not json").is_err());
    }

    /// What is wrong with a graph, if anything: what the editor and the engine count on.
    fn whole(g: &Graph) -> Result<(), String> {
        let mut ids = std::collections::HashSet::new();
        for n in &g.nodes {
            if !ids.insert(n.id) || n.id.0 >= g.next_id {
                return Err(format!("node id {:?} taken twice, or not below {}", n.id, g.next_id));
            }
            if !n.pos.iter().all(|p| p.abs() <= crate::graph::MAX_COORD) {
                return Err(format!("{} at {:?}", n.def().name, n.pos));
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

    /// Files and pastes mangled at random load whole, or say why they do not.
    #[test]
    fn mangled_files_and_pastes_come_out_whole() {
        use crate::graph::Clipboard;
        let (demo, preview) = crate::demo::graph();
        let file = to_json(&demo, &ViewState { preview, ..Default::default() }).unwrap();
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
            if let Ok(l) = from_json(&f.to_string()) {
                loaded += 1;
                whole(&l.graph).unwrap_or_else(|e| panic!("file of round {round}: {e}"));
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
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.brewtex");
        let (graph, _) = crate::demo::graph();
        save(&path, &graph, &ViewState::default()).unwrap();
        assert_eq!(load(&path).unwrap().graph, graph);
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
