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
    let text = std::fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
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
        let (graph, preview) = crate::demo::brick_wall();
        let view = ViewState { preview: Some(preview), graph_pan: Some([1.0, 2.0]), graph_zoom: Some(0.8) };
        let json = to_json(&graph, &view).unwrap();
        let loaded = from_json(&json).unwrap();
        assert_eq!(loaded.graph, graph);
        assert_eq!(loaded.view, view);
        assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
        assert!(from_json("{}").is_err());
        assert!(from_json("not json").is_err());
    }

    #[test]
    fn save_and_load_file() {
        let dir = std::env::temp_dir().join(format!("brewed_project_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.brewtex");
        let (graph, _) = crate::demo::brick_wall();
        save(&path, &graph, &ViewState::default()).unwrap();
        assert_eq!(load(&path).unwrap().graph, graph);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
