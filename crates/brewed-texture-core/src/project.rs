//! Project files (`.brewtex`, JSON). Files of the earlier versions of the program open too:
//! those of the version before are the same inside, those of the first one (`.ftx`) are
//! converted. Either way they are saved as a new file, never over the original.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json, json};

use crate::graph::{Graph, NodeId};

pub const FILE_EXTENSION: &str = "brewtex";
/// The extensions of the files of earlier versions, which open as well.
pub const EARLIER_EXTENSIONS: [&str; 2] = ["tessera", "ftx"];
const FORMAT: &str = "brewed-texture-designer";
/// The name the version before gave the same format.
const EARLIER_FORMAT: &str = "tessera";
/// How the files of the first version say what they are.
const FIRST_VERSION_APP: &str = "ferrotex";
const VERSION: u32 = 2;

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
    /// The file was in an older format and was converted: it should be saved as a new file
    /// rather than overwritten.
    pub converted: bool,
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
    let first_version = json.get("app").and_then(Json::as_str) == Some(FIRST_VERSION_APP);
    let mut converted = first_version;
    let file: ProjectFile = if first_version {
        notes.push("Converted from the file format of the first version (.ftx)".into());
        let migrated = migrate_first_version(json, &mut notes)?;
        serde_json::from_value(migrated).map_err(|e| format!("Cannot convert the old file: {e}"))?
    } else {
        match json.get("format").and_then(Json::as_str) {
            Some(FORMAT) => {}
            // The same format under its earlier name: it opens as it is, and is saved as a
            // new file.
            Some(EARLIER_FORMAT) => {
                converted = true;
                notes.push("Opened a file of the previous version; it is saved as a new .brewtex file".into());
            }
            _ => return Err(format!("This is not a {} project", crate::APP_NAME)),
        }
        let version = json.get("version").and_then(Json::as_u64).unwrap_or(0);
        if version > VERSION as u64 {
            notes.push(format!("The file was written by a newer version (format {version}); some data may be lost"));
        }
        serde_json::from_value(json).map_err(|e| format!("Cannot read the project: {e}"))?
    };
    let mut graph = file.graph;
    let links_before = graph.links.len();
    graph.normalize();
    if graph.links.len() < links_before {
        notes.push(format!("{} invalid link(s) were removed", links_before - graph.links.len()));
    }
    let mut view = file.view;
    view.preview = view.preview.filter(|id| graph.contains(*id));
    Ok(Loaded { graph, view, notes, converted })
}

// ---------------------------------------------------------------------------- migration

fn legacy_kind(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "Math" => "math",
        "ComposeVec2" => "compose_vec2",
        "ComposeColor" => "compose_color",
        "UniformColor" => "uniform_color",
        "Shape" => "shape",
        "TileGenerator" => "tile_generator",
        "Gradient" => "gradient",
        "Noise" => "noise",
        "Bitmap" => "bitmap",
        "Transform2D" => "transform_2d",
        "Blend" => "blend",
        "Blur" => "blur",
        "Sharpen" => "sharpen",
        "Warp" => "warp",
        "Mirror" => "mirror",
        "NormalMap" => "normal",
        "PixelMath" => "pixel_math",
        "Levels" => "levels",
        "Invert" => "invert",
        "Grayscale" => "grayscale",
        "Hsl" => "hsl",
        "GradientMap" => "gradient_map",
        "ChannelShuffle" => "channel_shuffle",
        "Posterize" => "posterize",
        "Output" => "output",
        _ => return None,
    })
}

fn legacy_input_kind(ty: &str) -> &'static str {
    match ty {
        "Int" => "int_input",
        "Bool" => "bool_input",
        "Vec2" => "vec2_input",
        "Color" => "color_input",
        _ => "float_input",
    }
}

/// Stored value of a parameter in the old `params` array.
fn legacy_param<'a>(node: &'a Json, key: &str) -> Option<&'a Json> {
    node.get("params")?.as_array()?.iter().find(|p| p.get("key").and_then(Json::as_str) == Some(key))?.get("value")
}

fn legacy_color(node: &Json, key: &str, fallback: [f64; 4]) -> Json {
    legacy_param(node, key).and_then(|v| v.get("Color")).cloned().unwrap_or_else(|| json!(fallback))
}

fn legacy_float(node: &Json, key: &str, fallback: f64) -> f64 {
    legacy_param(node, key).and_then(|v| v.get("Float")).and_then(Json::as_f64).unwrap_or(fallback)
}

fn migrate_first_version(mut json: Json, notes: &mut Vec<String>) -> Result<Json, String> {
    let graph = json.get_mut("graph").and_then(Json::as_object_mut).ok_or("The old file has no graph")?;
    let inputs: Vec<Json> = graph.remove("inputs").and_then(|v| v.as_array().cloned()).unwrap_or_default();
    let input_by_id =
        |id: &Json| inputs.iter().find(|i| i.get("id") == Some(id)).cloned().ok_or("missing graph input".to_string());

    // Variables from the old graph inputs.
    let variables: Vec<Json> = inputs
        .iter()
        .map(|i| {
            let mut v = Map::new();
            for key in ["name", "ty", "value", "default", "min", "max", "description"] {
                if let Some(x) = i.get(key) {
                    v.insert(key.into(), x.clone());
                }
            }
            Json::Object(v)
        })
        .collect();
    graph.insert("variables".into(), Json::Array(variables));

    let nodes = graph.get_mut("nodes").and_then(Json::as_array_mut).ok_or("The old file has no nodes")?;
    let mut renamed_links: Vec<(u64, &str, &str)> = Vec::new();
    for node in nodes.iter_mut() {
        let kind = node.get("kind").and_then(Json::as_str).unwrap_or("").to_string();
        let id = node.get("id").and_then(Json::as_u64).unwrap_or(0);
        let obj = node.as_object_mut().ok_or("A node is not an object")?;
        if kind == "Input" {
            let input = obj.remove("input_ref").filter(|r| !r.is_null()).map(|r| input_by_id(&r));
            let (new_kind, name) = match input {
                Some(Ok(input)) => (
                    legacy_input_kind(input.get("ty").and_then(Json::as_str).unwrap_or("Float")),
                    input.get("name").and_then(Json::as_str).unwrap_or("value").to_string(),
                ),
                _ => {
                    notes.push("An Input node read a deleted graph input; it now has its own variable".into());
                    ("float_input", "value".to_string())
                }
            };
            obj.insert("kind".into(), json!(new_kind));
            obj.insert("variable".into(), json!(name));
            continue;
        }
        obj.remove("input_ref");
        let new_kind = legacy_kind(&kind).ok_or_else(|| format!("Unknown node type '{kind}' in the old file"))?;
        obj.insert("kind".into(), json!(new_kind));
        match new_kind {
            "tile_generator" => {
                // "Row Offset" became "Offset" (and gained a vertical variant).
                if let Some(params) = obj.get_mut("params").and_then(Json::as_array_mut) {
                    for p in params.iter_mut() {
                        if p.get("key").and_then(Json::as_str) == Some("row_offset") {
                            p["key"] = json!("offset");
                        }
                    }
                }
                renamed_links.push((id, "p:row_offset", "p:offset"));
            }
            "gradient_map" => {
                // Two to four fixed colors became a gradient with any number of stops.
                let snapshot = Json::Object(obj.clone());
                let stops =
                    legacy_param(&snapshot, "stops").and_then(|v| v.get("Enum")).and_then(Json::as_u64).unwrap_or(1);
                let a = legacy_color(&snapshot, "color_a", [0.0, 0.0, 0.0, 1.0]);
                let b = legacy_color(&snapshot, "color_b", [0.5, 0.5, 0.5, 1.0]);
                let c = legacy_color(&snapshot, "color_c", [0.75, 0.75, 0.75, 1.0]);
                let d = legacy_color(&snapshot, "color_d", [1.0, 1.0, 1.0, 1.0]);
                let pb = legacy_float(&snapshot, "pos_b", 0.5);
                let pc = legacy_float(&snapshot, "pos_c", 0.75);
                let mut list = vec![json!({"pos": 0.0, "color": a})];
                if stops >= 1 {
                    list.push(json!({"pos": pb.min(pc), "color": b}));
                }
                if stops >= 2 {
                    list.push(json!({"pos": pb.max(pc), "color": c}));
                }
                list.push(json!({"pos": 1.0, "color": d}));
                let exposed_colors =
                    ["color_a", "color_b", "color_c", "color_d", "pos_b", "pos_c", "stops"].iter().any(|k| {
                        snapshot["params"].as_array().is_some_and(|ps| {
                            ps.iter().any(|p| {
                                p.get("key").and_then(Json::as_str) == Some(*k)
                                    && p.get("exposed").and_then(Json::as_bool) == Some(true)
                            })
                        })
                    });
                if exposed_colors {
                    notes.push("Gradient Map colors can no longer be exposed; their links were removed".into());
                }
                let mut params: Vec<Json> = snapshot["params"]
                    .as_array()
                    .map(|ps| {
                        ps.iter().filter(|p| p.get("key").and_then(Json::as_str) == Some("tiling")).cloned().collect()
                    })
                    .unwrap_or_default();
                params.push(json!({"key": "gradient", "value": {"Gradient": {"stops": list}}}));
                obj.insert("params".into(), Json::Array(params));
            }
            _ => {}
        }
    }
    if let Some(links) = graph.get_mut("links").and_then(Json::as_array_mut) {
        for l in links.iter_mut() {
            let to = l.get("to").and_then(Json::as_u64);
            for (id, old, new) in &renamed_links {
                if to == Some(*id) && l.get("to_socket").and_then(Json::as_str) == Some(old) {
                    l["to_socket"] = json!(new);
                }
            }
        }
    }
    let preview = json.get("preview").cloned().unwrap_or(Json::Null);
    let graph = json.get("graph").cloned().unwrap_or(Json::Null);
    Ok(json!({"format": FORMAT, "version": VERSION, "graph": graph, "view": {"preview": preview}}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NodeKind, ParamValue, Value};

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

    /// A file of the previous version is the same format under another name: it opens as it
    /// is, and is saved as a new file rather than over the original.
    #[test]
    fn files_of_the_previous_version_open() {
        let (graph, _) = crate::demo::brick_wall();
        let json = to_json(&graph, &ViewState::default()).unwrap().replace("\"brewed-texture-designer\"", "\"tessera\"");
        let loaded = from_json(&json).expect("the previous version's file");
        assert_eq!(loaded.graph, graph);
        assert!(loaded.converted);
        assert!(from_json(&json.replace("\"tessera\"", "\"something else\"")).is_err());
    }

    #[test]
    fn migrates_first_version_files() {
        let old = r#"{"app":"ferrotex","version":1,"preview":4,"graph":{
            "nodes":[
              {"id":1,"kind":"Input","pos":[0,0],"params":[{"key":"tiling","value":{"Enum":3},"exposed":false}],"input_ref":10},
              {"id":2,"kind":"TileGenerator","pos":[100,0],"params":[
                  {"key":"tiling","value":{"Enum":3},"exposed":false},
                  {"key":"row_offset","value":{"Float":0.5},"exposed":true}]},
              {"id":3,"kind":"GradientMap","pos":[200,0],"params":[
                  {"key":"tiling","value":{"Enum":3},"exposed":false},
                  {"key":"stops","value":{"Enum":1},"exposed":false},
                  {"key":"color_a","value":{"Color":[0,0,0,1]},"exposed":false},
                  {"key":"color_b","value":{"Color":[1,0,0,1]},"exposed":true},
                  {"key":"pos_b","value":{"Float":0.3},"exposed":false},
                  {"key":"color_d","value":{"Color":[1,1,1,1]},"exposed":false}]},
              {"id":4,"kind":"Output","pos":[300,0],"params":[{"key":"name","value":{"Text":"basecolor"},"exposed":false}]}
            ],
            "links":[
              {"from":1,"from_socket":"o:value","to":2,"to_socket":"p:row_offset"},
              {"from":2,"from_socket":"o:output","to":3,"to_socket":"i:input"},
              {"from":3,"from_socket":"o:output","to":4,"to_socket":"i:input"}
            ],
            "inputs":[{"id":10,"name":"shift","description":"brick shift","ty":"Float",
                       "value":{"Float":0.5},"default":{"Float":0.5},"min":{"Float":0.0},"max":{"Float":1.0}}],
            "resolution":[1024,1024],"next_id":11}}"#;
        let loaded = from_json(old).unwrap();
        let g = &loaded.graph;
        assert!(!loaded.notes.is_empty());
        assert_eq!(g.node(NodeId(1)).unwrap().kind, NodeKind::FLOAT_INPUT);
        assert_eq!(g.node(NodeId(1)).unwrap().variable.as_deref(), Some("shift"));
        assert_eq!(g.variable("shift").unwrap().description, "brick shift");
        assert_eq!(g.variable("shift").unwrap().value, Value::Float(0.5));
        let tiles = g.node(NodeId(2)).unwrap();
        assert!(tiles.is_exposed("offset"));
        assert!(g.link_into(NodeId(2), "p:offset").is_some(), "link follows the renamed parameter");
        let gm = g.node(NodeId(3)).unwrap();
        let ParamValue::Gradient(grad) = gm.value("gradient").unwrap() else { panic!("no gradient") };
        assert_eq!(grad.stops.len(), 3);
        assert_eq!(grad.stops[1].color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(g.links.len(), 3);
        assert_eq!(loaded.view.preview, Some(NodeId(4)));
        assert_eq!(g.resolution, [1024, 1024]);
    }
}
