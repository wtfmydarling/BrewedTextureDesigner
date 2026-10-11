//! The graph document: nodes, links and graph variables.
//!
//! Plain data (serializable, cheap to clone for undo). GPU state lives in the engine and
//! is keyed by [`NodeId`]. Front ends never mutate a [`Graph`] directly: they submit
//! [`crate::Edit`]s to a [`crate::Document`].

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::instance::{Instance, InstanceParam, Interface};
use crate::nodes::{FX_MAP_BUILTINS, ImageKind, NodeDef, NodeKind, PIXEL_MATH_BUILTINS, ParamDef, ParamKind, Slot, TILING_PARAM};
use crate::types::{DataType, ParamValue, Tiling, Value, can_connect};
use crate::{Curve, Gradient};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CommentId(pub u64);

/// How far from the origin anything in a graph is, at most, in graph units: far beyond any
/// graph (the demo is 2000 wide), and near enough that a view of it never goes past what a
/// float holds, whatever a file says or an edit does.
pub const MAX_COORD: f32 = 1e7;

/// The ids of nodes, comments and graphs are below this: far beyond what a graph or a package
/// makes (an id a microsecond for eight years), below the ids of the nodes laid out from graphs
/// used as nodes (see [`crate::package::flatten`]), and with room to count on. An id a file
/// says at or above it is made anew as the file is read.
pub const MAX_ID: u64 = 1 << 48;

/// A coordinate within [`MAX_COORD`] (0 when it is not a number).
pub fn coordinate(v: f32) -> f32 {
    if v.is_finite() { v.clamp(-MAX_COORD, MAX_COORD) } else { 0.0 }
}

/// A titled frame around a group of nodes, to say what they are for. It is moved with
/// the nodes it holds, and it is only ever decoration: nothing in the graph depends on it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Comment {
    pub id: CommentId,
    /// Its title.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// What the nodes it frames do, said under its title (as a frame of Substance Designer
    /// says it): wrapped to its width.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(rename = "position")]
    pub pos: [f32; 2],
    pub size: [f32; 2],
    /// The color it is drawn in, like any other color in the graph.
    #[serde(default = "Comment::gray")]
    pub color: [f32; 4],
}

impl Comment {
    /// The color a comment starts with: a plain gray, until it is given another.
    pub fn gray() -> [f32; 4] {
        [0.49, 0.49, 0.49, 1.0]
    }
    /// Smallest a comment can be dragged to, in graph units: still large enough to grab
    /// its title bar and its corner.
    pub const MIN_SIZE: [f32; 2] = [120.0, 80.0];

    /// Keep what a file says within what can be drawn: a place, a size between the smallest
    /// and [`MAX_COORD`], and a color.
    pub fn normalize(&mut self) {
        self.pos = self.pos.map(coordinate);
        for (size, min) in self.size.iter_mut().zip(Comment::MIN_SIZE) {
            *size = if size.is_finite() { size.clamp(min, MAX_COORD) } else { min };
        }
        if !self.color.iter().all(|c| c.is_finite()) {
            self.color = Comment::gray();
        }
        self.color = self.color.map(|c| c.clamp(0.0, 1.0));
    }

    /// Does this comment hold a node of size `size` at `pos`? True when the whole node is
    /// inside, so that moving the comment never tears a node away from its neighbours.
    pub fn holds(&self, pos: [f32; 2], size: [f32; 2]) -> bool {
        pos[0] >= self.pos[0]
            && pos[1] >= self.pos[1]
            && pos[0] + size[0] <= self.pos[0] + self.size[0]
            && pos[1] + size[1] <= self.pos[1] + self.size[1]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParamSlot {
    pub key: String,
    pub value: ParamValue,
    pub exposed: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// A node of a graph. A file says it as [`NodeFile`] does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(into = "NodeFile", try_from = "NodeFile")]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub pos: [f32; 2],
    pub params: Vec<ParamSlot>,
    /// Input nodes: name of the graph variable they read.
    pub variable: Option<String>,
    /// Values of expression variables that are not connected.
    pub var_values: BTreeMap<String, f32>,
    /// Image nodes: pass the first input through unchanged.
    pub bypass: bool,
    /// A graph used as a node: which graph, and what it is as a node (see
    /// [`crate::instance`]).
    pub instance: Option<Box<Instance>>,
    /// What other programs keep with the node (see [`crate::project`]): kept as it is.
    pub extras: serde_json::Map<String, serde_json::Value>,
}

/// A node as a file says it: what it is (`type`), where, its parameters by key, each a typed
/// value (see [`FileValue`]), those exposed as sockets listed apart; what an Input node reads
/// (`input_value`), the values of an expression's variables nothing is connected to
/// (`locals`), whether it is bypassed, the graph it uses (`uses`, see
/// [`crate::instance::Instance`]) and what other programs keep with it (`extras`). What is as it
/// is by default is not said.
#[derive(Serialize, Deserialize)]
struct NodeFile {
    id: NodeId,
    #[serde(rename = "type")]
    kind: String,
    position: [f32; 2],
    #[serde(default, skip_serializing_if = "ParamsFile::is_empty")]
    params: ParamsFile,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    exposed: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    input_value: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    locals: BTreeMap<String, f32>,
    #[serde(default, skip_serializing_if = "is_false")]
    bypass: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    uses: Option<Box<Instance>>,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    extras: serde_json::Map<String, serde_json::Value>,
}

impl From<Node> for NodeFile {
    fn from(node: Node) -> NodeFile {
        let exposed = node.params.iter().filter(|p| p.exposed).map(|p| p.key.clone()).collect();
        let params = node.params.iter().map(|p| (p.key.clone(), FileValue::of(&p.value, node.param_def(&p.key)))).collect();
        NodeFile {
            id: node.id,
            kind: node.kind.id().to_string(),
            position: node.pos,
            params: ParamsFile(params),
            exposed,
            input_value: node.variable,
            locals: node.var_values,
            bypass: node.bypass,
            uses: node.instance,
            extras: node.extras,
        }
    }
}

impl TryFrom<NodeFile> for Node {
    type Error = String;

    fn try_from(file: NodeFile) -> Result<Node, String> {
        let kind = NodeKind::from_id(&file.kind)
            .ok_or_else(|| format!("unknown node type '{}' (made by a newer version of the program?)", file.kind))?;
        let def = |key: &str| match &file.uses {
            Some(instance) => instance.interface.param(key).map(InstanceParam::def),
            None => kind.def().param(key).copied(),
        };
        let mut params = Vec::with_capacity(file.params.0.len());
        for (key, value) in file.params.0 {
            // (A parameter the node does not have goes: see `Node::normalize`.)
            let value = match value.into_value(def(&key)) {
                None => continue,
                Some(Ok(value)) => value,
                Some(Err(id)) => {
                    let node = kind.def().name;
                    return Err(format!(
                        "the {key} of a {node} is '{id}', which this version does not know (made by a newer version of the program?)"
                    ));
                }
            };
            params.push(ParamSlot { exposed: file.exposed.contains(&key), key, value });
        }
        Ok(Node {
            id: file.id,
            kind,
            pos: file.position,
            params,
            variable: file.input_value,
            var_values: file.locals,
            bypass: file.bypass,
            instance: file.uses,
            extras: file.extras,
        })
    }
}

/// A parameter's value as a file says it: by its type, `{"float": 0.5}`, `{"int": 4}`,
/// `{"bool": true}`, `{"vec2": [x, y]}`, `{"color": [r, g, b, a]}`, `{"choice": "opengl"}` (the
/// option's id, see [`crate::nodes::ParamDef::choice_id`]), `{"text": "…"}`, `{"gradient": {…}}`
/// or `{"curve": {…}}`.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum FileValue {
    Float(f32),
    Int(i32),
    Bool(bool),
    Vec2([f32; 2]),
    Color([f32; 4]),
    Choice(String),
    Text(String),
    Gradient(Gradient),
    Curve(Curve),
}

impl FileValue {
    /// `value`, of the parameter `def` describes.
    fn of(value: &ParamValue, def: Option<ParamDef<'_>>) -> FileValue {
        match value {
            ParamValue::Float(v) => FileValue::Float(*v),
            ParamValue::Int(v) => FileValue::Int(*v),
            ParamValue::Bool(v) => FileValue::Bool(*v),
            ParamValue::Vec2(v) => FileValue::Vec2(*v),
            ParamValue::Color(v) => FileValue::Color(*v),
            // (A choice holds one of its options, see `ParamDef::sanitize`: else the graph does
            // not read back, and is not written, see `crate::project`.)
            ParamValue::Enum(option) => FileValue::Choice(def.and_then(|d| d.choice_id(*option)).unwrap_or_default().to_string()),
            ParamValue::Text(v) => FileValue::Text(v.clone()),
            ParamValue::Gradient(v) => FileValue::Gradient(v.clone()),
            ParamValue::Curve(v) => FileValue::Curve(v.clone()),
        }
    }

    /// What it is of the parameter `def` describes: none for a choice of a parameter that is
    /// no choice, or none at all; the id itself for an option the choice does not have.
    fn into_value(self, def: Option<ParamDef<'_>>) -> Option<Result<ParamValue, String>> {
        Some(Ok(match self {
            FileValue::Float(v) => ParamValue::Float(v),
            FileValue::Int(v) => ParamValue::Int(v),
            FileValue::Bool(v) => ParamValue::Bool(v),
            FileValue::Vec2(v) => ParamValue::Vec2(v),
            FileValue::Color(v) => ParamValue::Color(v),
            FileValue::Choice(id) => {
                let def = def.filter(|d| matches!(d.kind, ParamKind::Enum { .. }))?;
                return Some(def.choice_of(&id).map(ParamValue::Enum).ok_or(id));
            }
            FileValue::Text(v) => ParamValue::Text(v),
            FileValue::Gradient(v) => ParamValue::Gradient(v),
            FileValue::Curve(v) => ParamValue::Curve(v),
        }))
    }
}

/// Parameters as a file says them: an object of values by key, in their order.
#[derive(Default)]
struct ParamsFile(Vec<(String, FileValue)>);

impl ParamsFile {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Serialize for ParamsFile {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = s.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for ParamsFile {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<ParamsFile, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = ParamsFile;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("parameters by key")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<ParamsFile, A::Error> {
                let mut params = Vec::with_capacity(map.size_hint().unwrap_or(0).min(256));
                while let Some((key, value)) = map.next_entry::<String, FileValue>()? {
                    params.push((key, value));
                }
                Ok(ParamsFile(params))
            }
        }
        d.deserialize_map(Visitor)
    }
}

impl Node {
    pub fn new(id: NodeId, kind: NodeKind, pos: [f32; 2]) -> Node {
        let params = kind
            .def()
            .all_params()
            .map(|p| ParamSlot { key: p.key.to_string(), value: p.default_value(), exposed: p.exposed_by_default })
            .collect();
        Node {
            id,
            kind,
            pos: pos.map(coordinate),
            params,
            variable: None,
            var_values: BTreeMap::new(),
            bypass: false,
            instance: None,
            extras: serde_json::Map::new(),
        }
    }

    /// A node that uses a graph: its parameters are the graph's input values, at the values
    /// they have in the graph.
    pub fn of_graph(id: NodeId, instance: Instance, pos: [f32; 2]) -> Node {
        let params = instance
            .interface
            .params
            .iter()
            .map(|p| ParamSlot { key: p.name.clone(), value: p.start_value(), exposed: false })
            .collect();
        Node {
            id,
            kind: NodeKind::GRAPH_INSTANCE,
            pos: pos.map(coordinate),
            params,
            variable: None,
            var_values: BTreeMap::new(),
            bypass: false,
            instance: Some(Box::new(instance)),
            extras: serde_json::Map::new(),
        }
    }

    pub fn def(&self) -> &'static NodeDef {
        self.kind.def()
    }

    /// The description of the parameter `key`: of the node type, or of the input value of the
    /// graph the node uses.
    pub fn param_def(&self, key: &str) -> Option<ParamDef<'_>> {
        match &self.instance {
            Some(instance) => instance.interface.param(key).map(InstanceParam::def),
            None => self.def().param(key).copied(),
        }
    }

    /// The descriptions of all the node's parameters, in order: the tiling first for a node
    /// type, the input values of its graph for a graph used as a node.
    pub fn param_defs(&self) -> Vec<ParamDef<'_>> {
        match &self.instance {
            Some(instance) => instance.interface.params.iter().map(InstanceParam::def).collect(),
            None => self.def().all_params().copied().collect(),
        }
    }

    /// Bring a node that uses a graph in line with what the graph is now: its name, and its
    /// sockets and parameters. Parameters that are gone go, new ones come with their value in
    /// the graph, and the others keep theirs (held to their new range). Links to sockets that
    /// are gone wait for them (see [`Graph::prune_links`]). True if anything changed.
    pub fn set_interface(&mut self, name: &str, interface: Interface) -> bool {
        let Some(instance) = &mut self.instance else { return false };
        if instance.interface == interface && instance.graph.name == name {
            return false;
        }
        let old = std::mem::take(&mut self.params);
        self.params = interface
            .params
            .iter()
            .map(|p| {
                let def = p.def();
                match old.iter().find(|s| s.key == p.name) {
                    Some(slot) if def.accepts(&slot.value) => {
                        ParamSlot { key: p.name.clone(), value: def.sanitize(slot.value.clone()), exposed: slot.exposed }
                    }
                    other => ParamSlot {
                        key: p.name.clone(),
                        value: p.start_value(),
                        exposed: other.is_some_and(|s| s.exposed),
                    },
                }
            })
            .collect();
        instance.graph.name = name.to_string();
        instance.interface = interface;
        true
    }

    pub fn param(&self, key: &str) -> Option<&ParamSlot> {
        self.params.iter().find(|p| p.key == key)
    }

    pub fn param_mut(&mut self, key: &str) -> Option<&mut ParamSlot> {
        self.params.iter_mut().find(|p| p.key == key)
    }

    pub fn value(&self, key: &str) -> Option<&ParamValue> {
        self.param(key).map(|p| &p.value)
    }

    pub fn text(&self, key: &str) -> &str {
        match self.value(key) {
            Some(ParamValue::Text(s)) => s,
            _ => "",
        }
    }

    pub fn enum_value(&self, key: &str) -> u32 {
        match self.value(key) {
            Some(ParamValue::Enum(v)) => *v,
            _ => 0,
        }
    }

    pub fn bool_value(&self, key: &str) -> bool {
        matches!(self.value(key), Some(ParamValue::Bool(true)))
    }

    pub fn is_exposed(&self, key: &str) -> bool {
        self.param(key).is_some_and(|p| p.exposed)
    }

    pub fn tiling(&self) -> Tiling {
        Tiling::from_index(self.enum_value(TILING_PARAM.key))
    }

    /// Input nodes read a graph variable of this type (an Image Input, an image input).
    pub fn variable_type(&self) -> Option<DataType> {
        self.def().variable_type()
    }

    pub fn is_expression_node(&self) -> bool {
        self.def().expression_param().is_some()
    }

    pub fn expression(&self) -> &str {
        self.def().expression_param().map_or("", |p| self.text(p.key))
    }

    /// Free variables of the Math / Pixel Math expression or the FX-Map program (stable while
    /// the user types: see [`crate::expr::Language::inputs_lenient`]). Built-ins are not listed.
    pub fn expression_vars(&self) -> Vec<String> {
        match self.def().expression_language() {
            Some(language) => language.inputs_lenient(self.expression()),
            None => Vec::new(),
        }
    }

    /// Is the parameter, a color, a level of gray now (see [`ParamDef::gray_if`])? Not while
    /// what decides is exposed: then it is known only once evaluated.
    pub fn param_gray(&self, pd: &ParamDef) -> bool {
        pd.gray_if.is_some_and(|(key, values)| !self.is_exposed(key) && values.contains(&self.enum_value(key)))
    }

    /// Is the parameter currently relevant (see [`ParamDef::visible_if`])?
    pub fn param_visible(&self, pd: &ParamDef) -> bool {
        match pd.visible_if {
            None => true,
            Some((key, values)) => self.is_exposed(key) || values.contains(&self.enum_value(key)),
        }
    }

    /// A Uniform Color that makes grayscale of a "color" past a color's channels (a file of
    /// before colors were held from 0 to 1: an index of refraction of 1.5 as a level of gray) is
    /// the Uniform Value of that number: what it made, it makes. Not one driven by its socket.
    fn value_of_a_color(&mut self) {
        if self.kind != NodeKind::UNIFORM_COLOR || self.enum_value("output") != 1 {
            return;
        }
        let Some(slot) = self.param("color").filter(|s| !s.exposed) else { return };
        let ParamValue::Color(c) = slot.value else { return };
        if !c[..3].iter().all(|v| v.is_finite()) || c[..3].iter().all(|v| (0.0..=1.0).contains(v)) {
            return;
        }
        let value = if c[0] == c[1] && c[1] == c[2] { c[0] } else { 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2] };
        self.kind = NodeKind::UNIFORM_VALUE;
        self.params.retain(|s| s.key != "color" && s.key != "output");
        self.params.push(ParamSlot { key: "value".into(), value: ParamValue::Float(value), exposed: false });
    }

    /// Bring the parameter list in line with the node definition (after loading a file
    /// written by another version, or hand-edited JSON).
    pub fn normalize(&mut self) {
        self.value_of_a_color();
        if self.kind != NodeKind::GRAPH_INSTANCE {
            self.instance = None;
        }
        if let Some(instance) = &mut self.instance {
            instance.interface.normalize();
        }
        let old = std::mem::take(&mut self.params);
        let params: Vec<ParamSlot> = {
            let defs: Vec<ParamDef<'_>> = match &self.instance {
                Some(instance) => instance.interface.params.iter().map(InstanceParam::def).collect(),
                None => self.kind.def().all_params().copied().collect(),
            };
            let value = |p: &ParamDef<'_>| match &self.instance {
                Some(instance) => instance.interface.param(p.key).map_or_else(|| p.default_value(), InstanceParam::start_value),
                None => p.default_value(),
            };
            defs.iter()
                .map(|p| match old.iter().find(|s| s.key == p.key) {
                    Some(slot) => ParamSlot {
                        key: p.key.to_string(),
                        value: p.sanitize(slot.value.clone()),
                        exposed: slot.exposed && p.can_expose(),
                    },
                    None => ParamSlot { key: p.key.to_string(), value: value(p), exposed: p.exposed_by_default },
                })
                .collect()
        };
        self.params = params;
        if self.variable_type().is_none() {
            self.variable = None;
        }
        if !self.is_expression_node() {
            self.var_values.clear();
        }
        self.var_values.retain(|_, v| v.is_finite());
        if !self.def().has_image() || self.def().inputs.is_empty() {
            self.bypass = false;
        }
        self.pos = self.pos.map(coordinate);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Link {
    pub from: NodeId,
    pub from_socket: String,
    pub to: NodeId,
    pub to_socket: String,
}

/// An input value of the graph: a value shared by every Input node with the same name, or
/// the picture shared by every Image Input of the name, edited in the Input Values panel.
/// Input values exist exactly as long as a node reads them. Used as a node in another graph,
/// the graph has a parameter for each (see [`crate::instance`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: DataType,
    /// Its value, and the value it has by default. Of an image input: its color while it has
    /// no picture (a setting of it, the same in both).
    pub value: Value,
    pub default: Value,
    pub min: Value,
    pub max: Value,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Of an image input: its picture, the picture it has by default and what it takes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageInput>,
}

/// The picture of an image input of a graph: what the graph sees there while nothing arrives.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageInput {
    /// What it takes: an image that arrives is made grayscale or color.
    pub kind: ImageKind,
    /// A file, as a Bitmap reads one (empty: none; the graph sees the input's color).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub picture: String,
    /// The picture it has by default.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default: String,
}

/// The color of an image input without a picture, when nothing says another.
pub const IMAGE_INPUT_COLOR: [f32; 4] = [0.5, 0.5, 0.5, 1.0];

impl Variable {
    pub fn new(name: String, ty: DataType) -> Variable {
        let (value, min, max) = match ty {
            DataType::Float => (Value::Float(0.5), Value::Float(0.0), Value::Float(1.0)),
            DataType::Int => (Value::Int(4), Value::Int(0), Value::Int(16)),
            DataType::Bool => (Value::Bool(false), Value::Bool(false), Value::Bool(true)),
            DataType::Vec2 => (Value::Vec2([0.0, 0.0]), Value::Vec2([-1.0, -1.0]), Value::Vec2([1.0, 1.0])),
            DataType::Image => (Value::Color(IMAGE_INPUT_COLOR), Value::Color([0.0; 4]), Value::Color([1.0; 4])),
            DataType::Color => (Value::Color([0.8, 0.5, 0.2, 1.0]), Value::Color([0.0; 4]), Value::Color([1.0; 4])),
        };
        let image = (ty == DataType::Image)
            .then(|| ImageInput { kind: ImageKind::Gray, picture: String::new(), default: String::new() });
        Variable { name, ty, value, default: value, min, max, description: String::new(), image }
    }

    /// Numeric variables have a min / max range.
    pub fn has_range(&self) -> bool {
        matches!(self.ty, DataType::Float | DataType::Int | DataType::Vec2)
    }

    /// The type of [`Variable::value`]: the variable's, or of an image input, a color.
    pub fn value_type(&self) -> DataType {
        if self.ty == DataType::Image { DataType::Color } else { self.ty }
    }

    /// Is it what it is by default: its value, or of an image input, its picture?
    pub fn is_default(&self) -> bool {
        match &self.image {
            Some(image) => image.picture == image.default,
            None => self.value == self.default,
        }
    }

    /// Make it what it is by default. True if that changed it.
    pub fn reset(&mut self) -> bool {
        if self.is_default() {
            return false;
        }
        match &mut self.image {
            Some(image) => image.picture = image.default.clone(),
            None => self.value = self.default,
        }
        true
    }

    /// Make what it is now what it is by default. True if that changed it.
    pub fn make_default(&mut self) -> bool {
        if self.is_default() {
            return false;
        }
        match &mut self.image {
            Some(image) => image.default = image.picture.clone(),
            None => self.default = self.value,
        }
        true
    }

    /// Make every stored value match `ty` and keep value / default inside [min, max].
    pub fn normalize(&mut self) {
        let fresh = Variable::new(self.name.clone(), self.ty);
        let ty = self.value_type();
        let fix = |v: &Value, fallback: Value| match v.convert(ty) {
            Some(c) if c.is_finite() => c,
            _ => fallback,
        };
        self.value = fix(&self.value, fresh.value);
        self.default = fix(&self.default, fresh.default);
        if self.has_range() {
            self.min = fix(&self.min, fresh.min);
            self.max = fix(&self.max, fresh.max);
        } else {
            self.min = fresh.min;
            self.max = fresh.max;
        }
        self.value = self.value.clamped(&self.min, &self.max);
        self.default = self.default.clamped(&self.min, &self.max);
        match (self.ty, &mut self.image) {
            (DataType::Image, Some(image)) => {
                // (The color is a setting: the same by default.)
                self.default = self.value;
                image.picture = image.picture.trim().to_string();
                image.default = image.default.trim().to_string();
            }
            (DataType::Image, image) => *image = fresh.image,
            (_, image) => *image = None,
        }
    }
}

/// Default name stem of a new variable of type `ty`.
pub fn variable_base_name(ty: DataType) -> &'static str {
    match ty {
        DataType::Float => "value",
        DataType::Int => "count",
        DataType::Bool => "toggle",
        DataType::Vec2 => "vector",
        DataType::Color => "color",
        DataType::Image => "input",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketKind {
    /// A fixed input of the node type.
    Input,
    /// An exposed parameter.
    Param,
    /// A free variable of an expression.
    Variable,
    Output,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SocketInfo {
    pub key: String,
    pub label: String,
    pub ty: DataType,
    /// For image sockets, which kind of image (see [`Slot`]).
    pub slot: Slot,
    pub kind: SocketKind,
}

/// One end of a (possibly unfinished) link.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SocketRef {
    pub node: NodeId,
    pub key: String,
    pub output: bool,
}

/// Nodes and links copied to the clipboard, with the variables their Input nodes use.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clipboard {
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
    #[serde(default)]
    pub variables: Vec<Variable>,
    /// The file of the package the nodes were copied from: pasted in another package, the
    /// nodes that use graphs of their own package find them there (see
    /// [`Clipboard::use_package`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// Which package the nodes were copied from, when it has no file: another package cannot
    /// find its graphs (a number the program makes up for the package, see
    /// [`Clipboard::uses_graphs_of_its_package`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<u64>,
}

impl Clipboard {
    pub const MAGIC: &'static str = "brewed-texture-nodes:";

    pub fn to_text(&self) -> String {
        format!("{}{}", Clipboard::MAGIC, serde_json::to_string(self).unwrap_or_default())
    }

    pub fn from_text(text: &str) -> Option<Clipboard> {
        serde_json::from_str(text.trim().strip_prefix(Clipboard::MAGIC)?).ok()
    }

    /// Do nodes use graphs of the package they were copied from?
    pub fn uses_graphs_of_its_package(&self) -> bool {
        self.nodes.iter().any(|n| n.instance.as_ref().is_some_and(|i| i.graph.package.is_none()))
    }

    /// Have the nodes that use graphs of the package they were copied from say which file
    /// that is: they are pasted in another package.
    pub fn use_package(&mut self) {
        let Some(path) = self.package.clone() else { return };
        for node in &mut self.nodes {
            if let Some(instance) = &mut node.instance
                && instance.graph.package.is_none()
            {
                instance.graph.package = Some(path.clone());
            }
        }
    }

    /// Top-left corner of the copied nodes.
    pub fn origin(&self) -> Option<[f32; 2]> {
        let mut it = self.nodes.iter().map(|n| n.pos);
        let first = it.next()?;
        Some(it.fold(first, |a, p| [a[0].min(p[0]), a[1].min(p[1])]))
    }
}

/// How precisely images are kept, a channel of a pixel in: a byte (8 bits, 256 steps from 0 to
/// 1, nothing beyond them), a half float (16 bits: 11 bits of precision, steps of 1/2048 near 1
/// and finer below, values beyond 0 and 1 kept; what a game needs of a height), or a float (32
/// bits: 24 bits of precision, twice the memory). A graph keeps its images in one, half floats
/// unless it says otherwise, and a node can keep its own in another (its "precision"
/// parameter, see [`crate::nodes::PRECISION_PARAM`]), as each node of Substance Designer has
/// its output format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Precision {
    /// `unorm8` in a file: the texture format.
    #[serde(rename = "unorm8")]
    Bits8,
    #[default]
    #[serde(rename = "float16")]
    Half,
    #[serde(rename = "float32")]
    Full,
}

impl Precision {
    pub const ALL: [Precision; 3] = [Precision::Bits8, Precision::Half, Precision::Full];

    pub fn name(self) -> &'static str {
        match self {
            Precision::Bits8 => "8 bits",
            Precision::Half => "16 bits float",
            Precision::Full => "32 bits float",
        }
    }

    /// The precision of a node whose "precision" parameter is `choice` (see
    /// [`crate::nodes::PRECISION_PARAM`]) in a graph of precision `graph`.
    pub fn of_node(choice: u32, graph: Precision) -> Precision {
        match choice {
            1 => Precision::Bits8,
            2 => Precision::Half,
            3 => Precision::Full,
            _ => graph,
        }
    }

    fn is_half(&self) -> bool {
        *self == Precision::Half
    }
}

impl std::fmt::Display for Precision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// What a graph says of itself, for the Library of the packages that have it (see
/// `library` in the designer): as Substance Designer's graph attributes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphAttributes {
    /// What it makes, a sentence or two: the tip of it in the Library, and of the nodes that use
    /// it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Where the Library shows it: a category, or categories in categories (`Noises/Grunge`);
    /// none, under the name of its package.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub category: String,
    /// Not shown in the Library (a graph the others use, not one to use itself).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

impl GraphAttributes {
    pub fn is_empty(&self) -> bool {
        *self == GraphAttributes::default()
    }
}

/// How the 3D view shows the graph: on which model (by its id, `sphere`, `matball` …), how far
/// apart its height map moves the surface, how many times its texture repeats across the
/// model. What it does not say, the view has as it starts. Kept with the graph: a change of it
/// is a step of undo.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<f32>,
}

impl Scene {
    pub fn is_empty(&self) -> bool {
        *self == Scene::default()
    }

    /// Held to what a view takes: a model's id of a few plain characters, a height from 0 to 1,
    /// a repeat from a quarter to 32 (what a file says past them is held to them).
    pub fn sanitized(self) -> Scene {
        Scene {
            model: self.model.map(|m| m.chars().filter(|c| !c.is_control()).take(64).collect::<String>().trim().to_string()).filter(|m| !m.is_empty()),
            height: self.height.filter(|h| h.is_finite()).map(|h| h.clamp(0.0, 1.0)),
            repeat: self.repeat.filter(|r| r.is_finite()).map(|r| r.clamp(0.25, 32.0)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub resolution: [u32; 2],
    /// How precisely its images are kept (see [`Precision`]).
    #[serde(default, skip_serializing_if = "Precision::is_half")]
    pub precision: Precision,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(rename = "input_values", default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<Variable>,
    /// Frames grouping nodes, drawn behind them (see [`Comment`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<Comment>,
    /// The id the next node or comment gets.
    pub next_id: u64,
    /// What it says of itself, for the Library.
    #[serde(default, skip_serializing_if = "GraphAttributes::is_empty")]
    pub attributes: GraphAttributes,
    /// How the 3D view shows it (a file that says it otherwise than a scene says it is read as
    /// saying nothing of it: not a graph refused over a view).
    #[serde(default, skip_serializing_if = "Scene::is_empty", deserialize_with = "crate::project::lenient")]
    pub scene: Scene,
    /// What other programs keep with the graph (see [`crate::project`]): kept as it is.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extras: serde_json::Map<String, serde_json::Value>,
}

impl Default for Graph {
    fn default() -> Self {
        Graph {
            nodes: Vec::new(),
            links: Vec::new(),
            variables: Vec::new(),
            comments: Vec::new(),
            resolution: [2048, 2048],
            precision: Precision::Half,
            next_id: 1,
            attributes: GraphAttributes::default(),
            scene: Scene::default(),
            extras: serde_json::Map::new(),
        }
    }
}

pub const MIN_RESOLUTION: u32 = 16;
pub const MAX_RESOLUTION: u32 = 8192;

pub fn input_socket_key(key: &str) -> String {
    format!("i:{key}")
}
pub fn param_socket_key(key: &str) -> String {
    format!("p:{key}")
}
pub fn var_socket_key(name: &str) -> String {
    format!("v:{name}")
}
pub fn output_socket_key(key: &str) -> String {
    format!("o:{key}")
}

/// Turn arbitrary text into an identifier usable in expressions.
pub fn sanitize_identifier(text: &str) -> String {
    let mut out: String =
        text.trim().chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if out.is_empty() {
        out.push_str("value");
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    let reserved = [crate::nodes::MATH_BUILTINS, PIXEL_MATH_BUILTINS, FX_MAP_BUILTINS];
    if crate::expr::builtins::is_builtin(&out) || reserved.iter().any(|names| names.contains(&out.as_str())) {
        out.push('_');
    }
    out
}

fn unique_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_string();
    }
    let stem = base.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end_matches('_');
    let stem = if stem.is_empty() { base } else { stem };
    (1..).map(|i| format!("{stem}_{i}")).find(|n| !taken(n)).unwrap_or_else(|| base.to_string())
}

impl Graph {
    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.iter().any(|n| n.id == id)
    }

    pub fn comment(&self, id: CommentId) -> Option<&Comment> {
        self.comments.iter().find(|c| c.id == id)
    }

    pub fn comment_mut(&mut self, id: CommentId) -> Option<&mut Comment> {
        self.comments.iter_mut().find(|c| c.id == id)
    }

    /// Add a comment frame; it is drawn behind the nodes it covers.
    pub fn add_comment(&mut self, text: String, pos: [f32; 2], size: [f32; 2]) -> CommentId {
        let id = CommentId(self.alloc_id());
        let mut comment = Comment { id, text, description: String::new(), pos, size, color: Comment::gray() };
        comment.normalize();
        self.comments.push(comment);
        id
    }

    /// Add a node with default parameters. Output nodes get a unique name, Input nodes
    /// a fresh variable.
    pub fn add_node(&mut self, kind: NodeKind, pos: [f32; 2]) -> NodeId {
        let id = NodeId(self.alloc_id());
        let mut node = Node::new(id, kind, pos);
        if kind == NodeKind::OUTPUT {
            let name = self.unique_output_name("output");
            if let Some(p) = node.param_mut("name") {
                p.value = ParamValue::Text(name);
            }
        }
        if let Some(ty) = node.variable_type() {
            node.variable = Some(self.unique_variable_name(variable_base_name(ty)));
        }
        self.nodes.push(node);
        self.sync_variables();
        id
    }

    /// Add an Input node reading the existing variable `name`.
    pub fn add_variable_node(&mut self, name: &str, pos: [f32; 2]) -> Option<NodeId> {
        let ty = self.variable(name)?.ty;
        let kind = NodeKind::input_for(ty)?;
        let id = NodeId(self.alloc_id());
        let mut node = Node::new(id, kind, pos);
        node.variable = Some(name.to_string());
        self.nodes.push(node);
        Some(id)
    }

    pub fn remove_nodes(&mut self, ids: &[NodeId]) {
        let set: HashSet<NodeId> = ids.iter().copied().collect();
        self.nodes.retain(|n| !set.contains(&n.id));
        self.links.retain(|l| !set.contains(&l.from) && !set.contains(&l.to));
        self.sync_variables();
    }

    /// A name no Output of the graph has: the name of an output of the graph used as a node.
    pub fn unique_output_name(&self, base: &str) -> String {
        unique_name(base, |name| self.nodes.iter().any(|n| n.kind == NodeKind::OUTPUT && n.text("name") == name))
    }

    /// Add a node that uses a graph (see [`crate::instance`]).
    pub fn add_instance(&mut self, instance: Instance, pos: [f32; 2]) -> NodeId {
        let id = NodeId(self.alloc_id());
        self.nodes.push(Node::of_graph(id, instance, pos));
        id
    }

    // ------------------------------------------------------------------ variables

    pub fn variable(&self, name: &str) -> Option<&Variable> {
        self.variables.iter().find(|v| v.name == name)
    }

    pub fn variable_mut(&mut self, name: &str) -> Option<&mut Variable> {
        self.variables.iter_mut().find(|v| v.name == name)
    }

    /// Input nodes that read the variable `name`.
    pub fn variable_users(&self, name: &str) -> impl Iterator<Item = &Node> {
        self.nodes.iter().filter(move |n| n.variable.as_deref() == Some(name))
    }

    pub fn unique_variable_name(&self, base: &str) -> String {
        let base = sanitize_identifier(base);
        unique_name(&base, |name| self.variable(name).is_some() || self.variable_users(name).next().is_some())
    }

    /// Restore the variable invariants: every Input node has a valid name, a variable of
    /// its type exists for it, and variables no node uses are gone.
    pub fn sync_variables(&mut self) {
        for i in 0..self.nodes.len() {
            let Some(ty) = self.nodes[i].variable_type() else { continue };
            let wanted = self.nodes[i].variable.as_deref().map(sanitize_identifier);
            let name = match wanted {
                Some(name) if self.variable(&name).is_none_or(|v| v.ty == ty) => name,
                // Missing name, or the name belongs to a variable of another type.
                Some(name) => self.unique_variable_name(&name),
                None => self.unique_variable_name(variable_base_name(ty)),
            };
            if self.variable(&name).is_none() {
                self.variables.push(Variable::new(name.clone(), ty));
            }
            self.nodes[i].variable = Some(name);
        }
        let used: HashSet<&str> = self.nodes.iter().filter_map(|n| n.variable.as_deref()).collect();
        self.variables.retain(|v| used.contains(v.name.as_str()));
    }

    /// Rename a variable and every Input node reading it.
    pub fn rename_variable(&mut self, from: &str, to: &str) -> Result<String, String> {
        let to = sanitize_identifier(to);
        if to == from {
            return Ok(to);
        }
        if self.variable(from).is_none() {
            return Err(format!("There is no variable named '{from}'"));
        }
        if self.variable(&to).is_some() {
            return Err(format!(
                "A variable named '{to}' already exists. To share it, give an Input node that name in its properties."
            ));
        }
        for n in self.nodes.iter_mut().filter(|n| n.variable.as_deref() == Some(from)) {
            n.variable = Some(to.clone());
        }
        if let Some(v) = self.variable_mut(from) {
            v.name = to.clone();
        }
        Ok(to)
    }

    /// Point an Input node at the variable `name`. An existing variable is shared (it
    /// must have the node's type); a new name gets a copy of the settings the node had.
    pub fn bind_variable(&mut self, node: NodeId, name: &str) -> Result<String, String> {
        let name = sanitize_identifier(name);
        let n = self.node(node).ok_or("The node does not exist")?;
        let ty = n.variable_type().ok_or("Only Input nodes read variables")?;
        if n.variable.as_deref() == Some(name.as_str()) {
            return Ok(name);
        }
        match self.variable(&name) {
            Some(v) if v.ty != ty => {
                return Err(format!("'{name}' is a {} variable, this node is a {} input", v.ty.name(), ty.name()));
            }
            Some(_) => {}
            None => {
                let mut var = n
                    .variable
                    .as_deref()
                    .and_then(|old| self.variable(old))
                    .cloned()
                    .unwrap_or_else(|| Variable::new(name.clone(), ty));
                var.name = name.clone();
                self.variables.push(var);
            }
        }
        if let Some(n) = self.node_mut(node) {
            n.variable = Some(name.clone());
        }
        self.sync_variables();
        Ok(name)
    }

    // ------------------------------------------------------------------ sockets

    /// Input sockets of `node`: fixed inputs, exposed (and currently visible)
    /// parameters, then expression variables.
    pub fn input_sockets(&self, node: &Node) -> Vec<SocketInfo> {
        let d = node.def();
        let mut out: Vec<SocketInfo> = match &node.instance {
            // The image inputs of the graph the node uses.
            Some(instance) => instance
                .interface
                .inputs
                .iter()
                .map(|p| SocketInfo {
                    key: input_socket_key(&p.name),
                    label: p.name.clone(),
                    ty: DataType::Image,
                    slot: match p.kind {
                        Some(ImageKind::Gray) => Slot::Gray,
                        Some(ImageKind::Color) => Slot::Color,
                        None => Slot::Any,
                    },
                    kind: SocketKind::Input,
                })
                .collect(),
            None => d
                .inputs
                .iter()
                .map(|s| SocketInfo {
                    key: input_socket_key(s.key),
                    label: s.label.to_string(),
                    ty: s.ty,
                    slot: s.slot,
                    kind: SocketKind::Input,
                })
                .collect(),
        };
        for pd in node.param_defs() {
            let Some(slot) = node.param(pd.key) else { continue };
            if !slot.exposed || !pd.can_expose() || !node.param_visible(&pd) {
                continue;
            }
            if let Some((ty, takes)) = pd.socket(&slot.value) {
                out.push(SocketInfo {
                    key: param_socket_key(pd.key),
                    label: pd.label.to_string(),
                    ty,
                    slot: takes,
                    kind: SocketKind::Param,
                });
            }
        }
        // Pixel Math reads a channel of each image: color, which a grayscale image is too.
        let (var_ty, var_slot) = if d.is_pixel_expression() { (DataType::Image, Slot::Color) } else { (DataType::Float, Slot::Any) };
        for v in node.expression_vars() {
            out.push(SocketInfo { key: var_socket_key(&v), label: v.clone(), ty: var_ty, slot: var_slot, kind: SocketKind::Variable });
        }
        out
    }

    pub fn output_sockets(&self, node: &Node) -> Vec<SocketInfo> {
        if let Some(instance) = &node.instance {
            // The outputs of the graph the node uses.
            return instance
                .interface
                .outputs
                .iter()
                .map(|p| SocketInfo {
                    key: output_socket_key(&p.name),
                    label: p.name.clone(),
                    ty: DataType::Image,
                    slot: Slot::Any,
                    kind: SocketKind::Output,
                })
                .collect();
        }
        node.def()
            .outputs
            .iter()
            .map(|s| SocketInfo {
                key: output_socket_key(s.key),
                label: s.label.to_string(),
                ty: s.ty,
                slot: s.slot,
                kind: SocketKind::Output,
            })
            .collect()
    }

    pub fn input_socket(&self, node: NodeId, key: &str) -> Option<SocketInfo> {
        let n = self.node(node)?;
        self.input_sockets(n).into_iter().find(|s| s.key == key)
    }

    pub fn output_socket(&self, node: NodeId, key: &str) -> Option<SocketInfo> {
        let n = self.node(node)?;
        self.output_sockets(n).into_iter().find(|s| s.key == key)
    }

    pub fn socket(&self, s: &SocketRef) -> Option<SocketInfo> {
        if s.output { self.output_socket(s.node, &s.key) } else { self.input_socket(s.node, &s.key) }
    }

    // ------------------------------------------------------------------ links

    pub fn link_into(&self, node: NodeId, socket: &str) -> Option<&Link> {
        self.links.iter().find(|l| l.to == node && l.to_socket == socket)
    }

    /// Is `candidate` (transitively) upstream of `node`?
    pub fn depends_on(&self, node: NodeId, candidate: NodeId) -> bool {
        self.upstream(node).contains(&candidate)
    }

    /// `node` and every node it depends on, through the live links: what cannot take what it
    /// makes (that would be a cycle).
    pub fn upstream(&self, node: NodeId) -> HashSet<NodeId> {
        self.reached(node, true)
    }

    /// `node` and every node that depends on it: what cannot give it anything.
    pub fn downstream(&self, node: NodeId) -> HashSet<NodeId> {
        self.reached(node, false)
    }

    fn reached(&self, node: NodeId, up: bool) -> HashSet<NodeId> {
        let mut next: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for l in self.live_links() {
            let (from, to) = if up { (l.to, l.from) } else { (l.from, l.to) };
            next.entry(from).or_default().push(to);
        }
        let mut seen = HashSet::from([node]);
        let mut stack = vec![node];
        while let Some(n) = stack.pop() {
            for m in next.get(&n).into_iter().flatten() {
                if seen.insert(*m) {
                    stack.push(*m);
                }
            }
        }
        seen
    }

    /// Why a connection would be refused, or `Ok` if it is valid.
    pub fn check_connection(&self, from: NodeId, from_socket: &str, to: NodeId, to_socket: &str) -> Result<(), String> {
        if from == to {
            return Err("A node cannot be connected to itself".into());
        }
        let out = self.output_socket(from, from_socket).ok_or("Unknown output socket")?;
        let inp = self.input_socket(to, to_socket).ok_or("Unknown input socket")?;
        if !can_connect(out.ty, inp.ty) {
            return Err(format!("Cannot connect {} to {}", out.ty.name(), inp.ty.name()));
        }
        if self.depends_on(from, to) {
            return Err("This connection would create a cycle".into());
        }
        Ok(())
    }

    /// Connect an output to an input, replacing the input's previous link.
    pub fn connect(&mut self, from: NodeId, from_socket: &str, to: NodeId, to_socket: &str) -> Result<(), String> {
        self.check_connection(from, from_socket, to, to_socket)?;
        self.links.retain(|l| !(l.to == to && l.to_socket == to_socket));
        self.links.push(Link { from, from_socket: from_socket.to_string(), to, to_socket: to_socket.to_string() });
        Ok(())
    }

    /// Connect two socket ends in whichever order they were picked.
    pub fn connect_refs(&mut self, a: &SocketRef, b: &SocketRef) -> Result<(), String> {
        let (out, inp) = match (a.output, b.output) {
            (true, false) => (a, b),
            (false, true) => (b, a),
            _ => return Err("Connect an output to an input".into()),
        };
        self.connect(out.node, &out.key, inp.node, &inp.key)
    }

    pub fn disconnect(&mut self, to: NodeId, to_socket: &str) -> bool {
        let before = self.links.len();
        self.links.retain(|l| !(l.to == to && l.to_socket == to_socket));
        before != self.links.len()
    }

    pub fn set_exposed(&mut self, node: NodeId, key: &str, exposed: bool) -> Result<(), String> {
        let n = self.node(node).ok_or("The node does not exist")?;
        let pd = n.param_def(key).ok_or("Unknown parameter")?;
        if exposed && !pd.can_expose() {
            return Err(format!("'{}' cannot be exposed", pd.label));
        }
        if let Some(slot) = self.node_mut(node).and_then(|n| n.param_mut(key)) {
            slot.exposed = exposed;
        }
        if !exposed {
            self.disconnect(node, &param_socket_key(key));
        }
        Ok(())
    }

    /// Remove links that became invalid (deleted nodes, un-exposed parameters, type
    /// changes). Links into sockets that are only temporarily missing stay dormant: an
    /// expression variable that disappears while the expression is being typed, or an
    /// exposed parameter hidden by another parameter's value. They come back with the
    /// socket.
    pub fn prune_links(&mut self) {
        let mut keep: Vec<Link> = Vec::with_capacity(self.links.len());
        for l in &self.links {
            let (Some(from), Some(to)) = (self.node(l.from), self.node(l.to)) else { continue };
            let out = self.output_sockets(from).into_iter().find(|s| s.key == l.from_socket);
            let inp = self.input_sockets(to).into_iter().find(|s| s.key == l.to_socket);
            let ok = match (out, inp) {
                (Some(out), Some(inp)) => can_connect(out.ty, inp.ty),
                (out, inp) => {
                    // An output of a graph used as a node that its graph does not have now.
                    let out_waits = out.is_some() || (from.instance.is_some() && l.from_socket.starts_with("o:"));
                    out_waits && (inp.is_some() || dormant_input(to, &l.to_socket))
                }
            };
            if ok && !keep.iter().any(|k| k.to == l.to && k.to_socket == l.to_socket) {
                keep.push(l.clone());
            }
        }
        self.links = keep;
    }

    /// Is the link currently attached to a visible socket (dormant links are not)?
    pub fn is_link_live(&self, link: &Link) -> bool {
        self.node(link.to).is_some_and(|n| has_input_socket(n, &link.to_socket))
            && self.node(link.from).is_some_and(|n| match &n.instance {
                // The outputs of a node type are always there; a graph's may not be now.
                Some(instance) => {
                    link.from_socket.strip_prefix("o:").is_some_and(|k| instance.interface.outputs.iter().any(|p| p.name == k))
                }
                None => true,
            })
    }

    /// Links attached to visible sockets. Dormant links (kept for an expression variable
    /// that is being retyped, or a hidden exposed parameter) carry nothing: they must not
    /// count as dependencies (cycles, evaluation order).
    pub fn live_links(&self) -> impl Iterator<Item = &Link> {
        self.links.iter().filter(|l| self.is_link_live(l))
    }

    /// Topological order (sources first) and the nodes caught in cycles, if any.
    pub fn topo_order(&self) -> (Vec<NodeId>, Vec<NodeId>) {
        let mut indegree: HashMap<NodeId, usize> = self.nodes.iter().map(|n| (n.id, 0)).collect();
        let mut children: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for l in self.live_links() {
            if indegree.contains_key(&l.from) && indegree.contains_key(&l.to) {
                *indegree.get_mut(&l.to).expect("checked") += 1;
                children.entry(l.from).or_default().push(l.to);
            }
        }
        let mut queue: VecDeque<NodeId> = self.nodes.iter().map(|n| n.id).filter(|id| indegree[id] == 0).collect();
        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(id) = queue.pop_front() {
            order.push(id);
            if let Some(ch) = children.get(&id) {
                for c in ch {
                    let d = indegree.get_mut(c).expect("child exists");
                    *d -= 1;
                    if *d == 0 {
                        queue.push_back(*c);
                    }
                }
            }
        }
        let placed: HashSet<NodeId> = order.iter().copied().collect();
        let cyclic = self.nodes.iter().map(|n| n.id).filter(|id| !placed.contains(id)).collect();
        (order, cyclic)
    }

    /// Repair a graph that was loaded from disk.
    pub fn normalize(&mut self) {
        self.scene = std::mem::take(&mut self.scene).sanitized();
        let mut seen = HashSet::new();
        // A graph used as a node says which graph; one that does not is nothing.
        self.nodes.retain(|n| seen.insert(n.id) && (n.kind != NodeKind::GRAPH_INSTANCE || n.instance.is_some()));
        for n in &mut self.nodes {
            n.normalize();
        }
        let mut seen = HashSet::new();
        self.comments.retain(|c| seen.insert(c.id));
        for c in &mut self.comments {
            c.normalize();
        }
        let mut names = HashSet::new();
        self.variables.retain(|v| names.insert(v.name.clone()));
        for v in &mut self.variables {
            v.normalize();
        }
        // Nodes and comments take their ids from the same count. Ids no graph makes (a file
        // can say anything) are made anew, and the links say so.
        let ids = self.nodes.iter().map(|n| n.id.0).chain(self.comments.iter().map(|c| c.id.0));
        let mut max_id = ids.filter(|id| *id < MAX_ID).max().unwrap_or(0);
        let mut renamed = HashMap::new();
        for node in self.nodes.iter_mut().filter(|n| n.id.0 >= MAX_ID) {
            max_id += 1;
            renamed.insert(node.id, NodeId(max_id));
            node.id = NodeId(max_id);
        }
        for comment in self.comments.iter_mut().filter(|c| c.id.0 >= MAX_ID) {
            max_id += 1;
            comment.id = CommentId(max_id);
        }
        for link in &mut self.links {
            link.from = renamed.get(&link.from).copied().unwrap_or(link.from);
            link.to = renamed.get(&link.to).copied().unwrap_or(link.to);
        }
        // (A count past the ids there are is none: it counts on from the last.)
        self.next_id = if self.next_id < MAX_ID { self.next_id.max(max_id + 1) } else { max_id + 1 };
        self.resolution = self.resolution.map(|r| r.clamp(MIN_RESOLUTION, MAX_RESOLUTION));
        self.sync_variables();
        self.prune_links();
    }

    // ------------------------------------------------------------------ clipboard

    pub fn copy_nodes(&self, ids: &[NodeId]) -> Clipboard {
        let set: HashSet<NodeId> = ids.iter().copied().collect();
        let nodes: Vec<Node> = self.nodes.iter().filter(|n| set.contains(&n.id)).cloned().collect();
        let links = self.links.iter().filter(|l| set.contains(&l.from) && set.contains(&l.to)).cloned().collect();
        let mut variables: Vec<Variable> = Vec::new();
        for name in nodes.iter().filter_map(|n| n.variable.as_deref()) {
            if !variables.iter().any(|v| v.name == name)
                && let Some(v) = self.variable(name)
            {
                variables.push(v.clone());
            }
        }
        Clipboard { nodes, links, variables, package: None, origin: None }
    }

    /// Paste nodes, offsetting their positions by `offset`. Variables that already exist
    /// with the same type are shared; conflicting names get a fresh variable. Returns the
    /// new node ids.
    pub fn paste(&mut self, clip: &Clipboard, offset: [f32; 2]) -> Vec<NodeId> {
        let mut var_map: HashMap<String, String> = HashMap::new();
        for v in &clip.variables {
            let target = match self.variable(&v.name) {
                Some(existing) if existing.ty == v.ty => v.name.clone(),
                // (A clipboard can hold anything another program put there.)
                Some(_) => {
                    let name = self.unique_variable_name(&v.name);
                    let mut pasted = Variable { name: name.clone(), ..v.clone() };
                    pasted.normalize();
                    self.variables.push(pasted);
                    name
                }
                None => {
                    let mut pasted = v.clone();
                    pasted.normalize();
                    self.variables.push(pasted);
                    v.name.clone()
                }
            };
            var_map.insert(v.name.clone(), target);
        }
        let mut node_map: HashMap<NodeId, NodeId> = HashMap::new();
        let mut new_ids = Vec::new();
        for n in &clip.nodes {
            let mut copy = n.clone();
            copy.id = NodeId(self.alloc_id());
            copy.pos = [n.pos[0] + offset[0], n.pos[1] + offset[1]];
            copy.variable = n.variable.as_ref().map(|v| var_map.get(v).cloned().unwrap_or_else(|| v.clone()));
            copy.normalize();
            if copy.kind == NodeKind::OUTPUT {
                let name = self.unique_output_name(copy.text("name"));
                if let Some(p) = copy.param_mut("name") {
                    p.value = ParamValue::Text(name);
                }
            }
            node_map.insert(n.id, copy.id);
            new_ids.push(copy.id);
            self.nodes.push(copy);
        }
        for l in &clip.links {
            if let (Some(&from), Some(&to)) = (node_map.get(&l.from), node_map.get(&l.to)) {
                self.links.push(Link { from, from_socket: l.from_socket.clone(), to, to_socket: l.to_socket.clone() });
            }
        }
        self.sync_variables();
        self.prune_links();
        new_ids
    }

    // ------------------------------------------------------------------ queries

    /// Graph-space bounding box of the given nodes, from their positions and `size`.
    pub fn bounds(&self, ids: Option<&[NodeId]>, size: impl Fn(&Node) -> [f32; 2]) -> Option<[f32; 4]> {
        let mut out: Option<[f32; 4]> = None;
        let mut grow = |r: [f32; 4]| {
            out = Some(match out {
                None => r,
                Some(b) => [b[0].min(r[0]), b[1].min(r[1]), b[2].max(r[2]), b[3].max(r[3])],
            });
        };
        for n in &self.nodes {
            if ids.is_some_and(|ids| !ids.contains(&n.id)) {
                continue;
            }
            let s = size(n);
            grow([n.pos[0], n.pos[1], n.pos[0] + s[0], n.pos[1] + s[1]]);
        }
        // Comments belong to the graph too: framing everything shows them whole.
        if ids.is_none() {
            for c in &self.comments {
                grow([c.pos[0], c.pos[1], c.pos[0] + c.size[0], c.pos[1] + c.size[1]]);
            }
        }
        out
    }

    /// Nodes of a kind (e.g. all outputs).
    pub fn nodes_of(&self, kind: NodeKind) -> impl Iterator<Item = &Node> {
        self.nodes.iter().filter(move |n| n.kind == kind)
    }

    /// The description of a parameter of `node` (see [`Node::param_def`]).
    pub fn param_def(&self, node: NodeId, key: &str) -> Option<ParamDef<'_>> {
        self.node(node)?.param_def(key)
    }

    /// The nodes that use a graph (see [`crate::instance`]).
    pub fn instances(&self) -> impl Iterator<Item = &Node> {
        self.nodes.iter().filter(|n| n.instance.is_some())
    }
}

/// Whether a link to the input `key` of `to`, which is not there now, waits for it rather
/// than going: a variable of an expression being typed, a parameter hidden by another's value,
/// or a socket of a graph used as a node that the graph does not have now (its package file
/// cannot be found, or it changed).
fn dormant_input(to: &Node, key: &str) -> bool {
    if key.starts_with("v:") {
        return to.is_expression_node();
    }
    if key.starts_with("i:") {
        return to.instance.is_some();
    }
    let Some(key) = key.strip_prefix("p:") else { return false };
    match &to.instance {
        Some(instance) => instance.interface.param(key).is_none(),
        // Exposed but hidden by `visible_if`.
        None => to.def().param(key).is_some_and(|pd| to.is_exposed(key) && !to.param_visible(pd)),
    }
}

/// Does `node` have the input socket `key` now, as [`Graph::input_sockets`] lists them? (Asked
/// of every link again and again: without making the list.)
fn has_input_socket(node: &Node, key: &str) -> bool {
    if let Some(k) = key.strip_prefix("i:") {
        return match &node.instance {
            Some(instance) => instance.interface.inputs.iter().any(|p| p.name == k),
            None => node.def().inputs.iter().any(|s| s.key == k),
        };
    }
    if let Some(k) = key.strip_prefix("p:") {
        let (Some(pd), Some(slot)) = (node.param_def(k), node.param(k)) else { return false };
        return slot.exposed && pd.can_expose() && node.param_visible(&pd) && pd.socket(&slot.value).is_some();
    }
    key.strip_prefix("v:").is_some_and(|k| node.expression_vars().iter().any(|v| v == k))
}

/// Is `kind` a value-producing parameter kind that the UI edits with sliders?
pub fn is_numeric_param(kind: &ParamKind) -> bool {
    matches!(kind, ParamKind::Float { .. } | ParamKind::Int { .. } | ParamKind::Vec2 { .. })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(g: &mut Graph, id: NodeId, key: &str, value: ParamValue) {
        g.node_mut(id).unwrap().param_mut(key).unwrap().value = value;
    }

    #[test]
    fn connect_rules_and_cycles() {
        let mut g = Graph::default();
        let a = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
        let b = g.add_node(NodeKind::INVERT, [0.0, 0.0]);
        let c = g.add_node(NodeKind::BLUR, [0.0, 0.0]);
        let m = g.add_node(NodeKind::MATH, [0.0, 0.0]);
        assert!(g.connect(a, "o:output", b, "i:input").is_ok());
        assert!(g.connect(b, "o:output", c, "i:input").is_ok());
        assert!(g.connect(c, "o:output", b, "i:input").unwrap_err().contains("cycle"));
        assert!(g.connect(a, "o:output", m, "v:x").is_err());
        assert!(g.connect(m, "o:result", c, "p:radius").is_err());
        g.set_exposed(c, "radius", true).unwrap();
        assert!(g.connect(m, "o:result", c, "p:radius").is_ok());
        g.set_exposed(c, "radius", false).unwrap();
        assert!(g.link_into(c, "p:radius").is_none());
        let (order, cyclic) = g.topo_order();
        assert!(cyclic.is_empty());
        let pos = |id| order.iter().position(|x| *x == id).unwrap();
        assert!(pos(a) < pos(b) && pos(b) < pos(c));
    }

    #[test]
    fn tiling_is_not_exposable() {
        let mut g = Graph::default();
        let n = g.add_node(NodeKind::BLUR, [0.0, 0.0]);
        assert!(g.set_exposed(n, "tiling", true).is_err());
        assert!(!g.node(n).unwrap().is_exposed("tiling"));
    }

    #[test]
    fn hidden_parameters_have_no_socket_but_keep_their_link() {
        let mut g = Graph::default();
        let shape = g.add_node(NodeKind::SHAPE, [0.0, 0.0]);
        let count = g.add_node(NodeKind::INT_INPUT, [0.0, 0.0]);
        set(&mut g, shape, "shape", ParamValue::Enum(crate::nodes::SHAPE_POLYGON));
        g.set_exposed(shape, "sides", true).unwrap();
        g.connect(count, "o:value", shape, "p:sides").unwrap();
        // Disc has no sides: the socket disappears, the link stays dormant.
        set(&mut g, shape, "shape", ParamValue::Enum(0));
        g.prune_links();
        assert!(g.input_socket(shape, "p:sides").is_none());
        assert_eq!(g.links.len(), 1);
        assert!(!g.is_link_live(&g.links[0]));
        // Back to a polygon: the socket and the link are back.
        set(&mut g, shape, "shape", ParamValue::Enum(crate::nodes::SHAPE_STAR));
        g.prune_links();
        assert!(g.input_socket(shape, "p:sides").is_some());
        assert!(g.is_link_live(&g.links[0]));
        // If the shape type itself is exposed, every shape parameter stays available.
        set(&mut g, shape, "shape", ParamValue::Enum(0));
        g.set_exposed(shape, "shape", true).unwrap();
        assert!(g.input_socket(shape, "p:sides").is_some());
    }

    #[test]
    fn math_sockets_follow_the_expression() {
        let mut g = Graph::default();
        let m = g.add_node(NodeKind::MATH, [0.0, 0.0]);
        let keys: Vec<String> = g.input_sockets(g.node(m).unwrap()).into_iter().map(|s| s.key).collect();
        assert_eq!(keys, vec!["v:x", "v:myVar", "v:myVar2"]);
        let i = g.add_node(NodeKind::FLOAT_INPUT, [0.0, 0.0]);
        g.connect(i, "o:value", m, "v:myVar").unwrap();
        set(&mut g, m, "expression", ParamValue::Text("x + ".into()));
        g.prune_links();
        assert_eq!(g.links.len(), 1);
        set(&mut g, m, "expression", ParamValue::Text("x + myVar".into()));
        assert!(g.link_into(m, "v:myVar").is_some());
    }

    /// Whether a node has an input socket is what the list of its sockets says, for every node
    /// type with every parameter exposed (those hidden by another's value have none), for
    /// expression variables, and for a graph used as a node.
    #[test]
    fn a_socket_is_there_as_the_list_of_them_says() {
        let agree = |n: &Node, g: &Graph| {
            let listed: Vec<String> = g.input_sockets(n).into_iter().map(|s| s.key).collect();
            for key in &listed {
                assert!(has_input_socket(n, key), "{} lists {key}", n.def().name);
            }
            let asked = n
                .param_defs()
                .into_iter()
                .map(|p| param_socket_key(p.key))
                .chain(n.def().inputs.iter().map(|s| input_socket_key(s.key)))
                .chain(["i:nope", "p:nope", "v:nope", "o:output", "nope"].map(String::from));
            for key in asked {
                assert_eq!(has_input_socket(n, &key), listed.contains(&key), "{} {key}", n.def().name);
            }
        };
        let mut g = Graph::default();
        for kind in NodeKind::ALL {
            let id = g.add_node(*kind, [0.0, 0.0]);
            let node = g.node_mut(id).unwrap();
            for slot in &mut node.params {
                slot.exposed = true;
            }
        }
        let math = g.nodes_of(NodeKind::MATH).next().unwrap().id;
        g.node_mut(math).unwrap().param_mut("expression").unwrap().value = ParamValue::Text("a * b + c".into());
        for n in &g.nodes {
            agree(n, &g);
        }
        // A graph used as a node, its image input exposed, a value not.
        let mut sub = Graph::default();
        sub.add_node(NodeKind::IMAGE_INPUT, [0.0, 0.0]);
        sub.add_node(NodeKind::FLOAT_INPUT, [0.0, 100.0]);
        let r = crate::instance::GraphRef { package: None, graph: crate::instance::GraphId(2), name: "Sub".into() };
        let used = g.add_instance(Instance::new(r, Interface::of(&sub)), [0.0, 0.0]);
        g.set_exposed(used, "input", true).unwrap();
        agree(g.node(used).unwrap(), &g);
        assert!(has_input_socket(g.node(used).unwrap(), "p:input") && !has_input_socket(g.node(used).unwrap(), "p:value"));
    }

    #[test]
    fn dormant_links_are_not_dependencies() {
        // Math a reads Math b through "x"; then "x" leaves a's expression: the link stays
        // (dormant, for when "x" is typed again) but a no longer depends on b.
        let mut g = Graph::default();
        let a = g.add_node(NodeKind::MATH, [0.0, 0.0]);
        let b = g.add_node(NodeKind::MATH, [0.0, 0.0]);
        set(&mut g, a, "expression", ParamValue::Text("x".into()));
        set(&mut g, b, "expression", ParamValue::Text("y".into()));
        g.connect(b, "o:result", a, "v:x").unwrap();
        set(&mut g, a, "expression", ParamValue::Text("1".into()));
        g.prune_links();
        assert_eq!(g.links.len(), 1, "kept, dormant");
        assert!(!g.depends_on(a, b));
        // So a can feed b without a cycle.
        g.connect(a, "o:result", b, "v:y").unwrap();
        let (order, cyclic) = g.topo_order();
        assert!(cyclic.is_empty(), "{cyclic:?}");
        assert_eq!(order.len(), 2);
    }

    #[test]
    fn variables_live_as_long_as_their_nodes() {
        let mut g = Graph::default();
        let a = g.add_node(NodeKind::FLOAT_INPUT, [0.0, 0.0]);
        let name = g.node(a).unwrap().variable.clone().unwrap();
        assert_eq!(name, "value");
        assert_eq!(g.variables.len(), 1);
        g.variable_mut(&name).unwrap().value = Value::Float(0.25);
        // A second node with the same name shares the value.
        let b = g.add_variable_node(&name, [0.0, 0.0]).unwrap();
        assert_eq!(g.variables.len(), 1);
        assert_eq!(g.variable_users(&name).count(), 2);
        g.remove_nodes(&[a]);
        assert_eq!(g.variable(&name).unwrap().value, Value::Float(0.25));
        g.remove_nodes(&[b]);
        assert!(g.variables.is_empty(), "the variable disappears with its last node");
        // A new input gets a fresh name.
        let c = g.add_node(NodeKind::FLOAT_INPUT, [0.0, 0.0]);
        let d = g.add_node(NodeKind::FLOAT_INPUT, [0.0, 0.0]);
        assert_ne!(g.node(c).unwrap().variable, g.node(d).unwrap().variable);
    }

    #[test]
    fn binding_and_renaming_variables() {
        let mut g = Graph::default();
        let f = g.add_node(NodeKind::FLOAT_INPUT, [0.0, 0.0]);
        let col = g.add_node(NodeKind::COLOR_INPUT, [0.0, 0.0]);
        g.variable_mut("value").unwrap().max = Value::Float(10.0);
        // Renaming a node to a new name keeps its settings.
        assert_eq!(g.bind_variable(f, "rough ness").unwrap(), "rough_ness");
        assert_eq!(g.variable("rough_ness").unwrap().max, Value::Float(10.0));
        assert!(g.variable("value").is_none());
        // Type conflicts are refused.
        assert!(g.bind_variable(col, "rough_ness").is_err());
        // Renaming the variable renames all users.
        let f2 = g.add_variable_node("rough_ness", [0.0, 0.0]).unwrap();
        g.rename_variable("rough_ness", "roughness").unwrap();
        assert_eq!(g.node(f).unwrap().variable.as_deref(), Some("roughness"));
        assert_eq!(g.node(f2).unwrap().variable.as_deref(), Some("roughness"));
        assert!(g.rename_variable("roughness", "color").is_err(), "name taken");
        assert_eq!(sanitize_identifier("sin"), "sin_");
        assert_eq!(sanitize_identifier("u"), "u_");
    }

    #[test]
    fn loading_fixes_type_conflicts() {
        let mut g = Graph::default();
        let a = g.add_node(NodeKind::FLOAT_INPUT, [0.0, 0.0]);
        let b = g.add_node(NodeKind::COLOR_INPUT, [0.0, 0.0]);
        g.node_mut(b).unwrap().variable = g.node(a).unwrap().variable.clone();
        g.normalize();
        assert_ne!(g.node(a).unwrap().variable, g.node(b).unwrap().variable);
        assert_eq!(g.variables.len(), 2);
    }

    #[test]
    fn copy_paste_remaps_ids_and_shares_variables() {
        let mut g = Graph::default();
        let a = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
        let b = g.add_node(NodeKind::LEVELS, [100.0, 0.0]);
        let v = g.add_node(NodeKind::FLOAT_INPUT, [0.0, 50.0]);
        g.connect(a, "o:output", b, "i:input").unwrap();
        let clip = g.copy_nodes(&[a, b, v]);
        let text = clip.to_text();
        let clip = Clipboard::from_text(&text).unwrap();
        assert_eq!(clip.origin(), Some([0.0, 0.0]));
        let new = g.paste(&clip, [10.0, 10.0]);
        assert_eq!(new.len(), 3);
        assert_eq!(g.nodes.len(), 6);
        assert_eq!(g.links.len(), 2);
        assert_eq!(g.variables.len(), 1, "pasted input shares the variable");
        assert!(g.link_into(new[1], "i:input").is_some_and(|l| l.from == new[0]));
    }

    #[test]
    fn serialization_roundtrip() {
        let mut g = Graph::default();
        let a = g.add_node(NodeKind::SHAPE, [1.0, 2.0]);
        let o = g.add_node(NodeKind::OUTPUT, [3.0, 4.0]);
        g.connect(a, "o:output", o, "i:input").unwrap();
        g.add_node(NodeKind::VEC2_INPUT, [0.0, 0.0]);
        g.add_node(NodeKind::GRADIENT_MAP, [0.0, 0.0]);
        let json = serde_json::to_string(&g).unwrap();
        let mut back: Graph = serde_json::from_str(&json).unwrap();
        back.normalize();
        assert_eq!(g, back);
    }

    /// A file says an option of a choice by its id, not its place. An option this version does
    /// not have (of a newer one) keeps the graph from being read, saying which; a choice of a
    /// parameter the node does not have, or that is no choice, goes as that parameter does.
    #[test]
    fn choices_are_said_by_their_ids() {
        let mut g = Graph::default();
        let normal = g.add_node(NodeKind::NORMAL, [0.0, 0.0]);
        set(&mut g, normal, "format", ParamValue::Enum(1));
        set(&mut g, normal, "intensity", ParamValue::Float(7.25));
        let json = serde_json::to_string(&g).unwrap();
        assert!(json.contains(r#""format":{"choice":"directx"}"#) && json.contains(r#""tiling":{"choice":"both"}"#), "{json}");
        let back: Graph = serde_json::from_str(&json).unwrap();
        assert_eq!(back.node(normal).unwrap().enum_value("format"), 1);
        let unknown = json.replace("directx", "sideways");
        let e = serde_json::from_str::<Graph>(&unknown).unwrap_err().to_string();
        assert!(e.contains("'sideways'") && e.contains("newer version"), "{e}");
        let mut odd: serde_json::Value = serde_json::from_str(&json).unwrap();
        let params = &mut odd["nodes"][0]["params"];
        params["intensity"] = serde_json::json!({ "choice": "x" });
        params["nothing"] = serde_json::json!({ "choice": "x" });
        let mut back: Graph = serde_json::from_value(odd).unwrap();
        back.normalize();
        let node = back.node(normal).unwrap();
        assert_eq!(node.value("intensity"), Some(&node.def().param("intensity").unwrap().default_value()));
        assert!(node.value("nothing").is_none() && node.enum_value("format") == 1);
    }
}
