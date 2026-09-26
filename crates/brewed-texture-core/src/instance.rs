//! Graphs used as nodes.
//!
//! A node of [`NodeKind::GRAPH_INSTANCE`] stands for a graph, of its own package or of another
//! package file, and makes what that graph makes. Its outputs and parameters are the graph's:
//! an output for each Output node, a parameter for each input value, the image inputs among
//! them (a picture of the node's own, or, exposed, a socket images arrive at). The node keeps
//! a copy of them, its [`Interface`]: its sockets, links and values stay while the graph
//! cannot be found (a package file that was moved), and the front end brings the copy up to
//! date whenever the graph changes (see [`Interface::of`] and [`crate::Node::set_interface`]).
//!
//! The engine never sees such a node: before a graph is computed, every graph it uses is laid
//! out in it, node by node (see [`crate::package::flatten`]).

use serde::{Deserialize, Serialize};

use crate::graph::{Graph, ImageInput, Node};
use crate::nodes::{ImageKind, NodeKind, ParamDef, ParamKind};
use crate::types::{ParamValue, Value};

/// A graph's identifier in its package. It stays what it is when the graph is renamed, so the
/// nodes that use the graph keep finding it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GraphId(pub u64);

/// The graph a node uses.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GraphRef {
    /// The package file the graph is in; `None`: the package of the graph the node is in. In
    /// memory the path is absolute; a file says it relative to itself where it can (see
    /// [`crate::project`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    pub graph: GraphId,
    /// The graph's name when the node last saw it: what the node is called while the graph
    /// cannot be found, and what the graph is found by when its package has no graph of its id
    /// any more (a package made anew).
    pub name: String,
}

/// An image socket of a graph used as a node: an output of the graph, or a fixed input (a graph
/// has none; files of version 3 said its image inputs so, see [`crate::project`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Port {
    pub name: String,
    /// What an input takes. An output gives what arrives at it, which only the engine knows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ImageKind>,
}

/// An input value of a graph, as a parameter of the nodes that use the graph.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InstanceParam {
    /// The input value's name: the key of the parameter.
    pub name: String,
    /// What the properties call it.
    pub label: String,
    /// Its value in the graph: what a new node starts with.
    pub value: Value,
    pub default: Value,
    pub min: Value,
    pub max: Value,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// An image input: its picture in the graph, and what it takes (see
    /// [`crate::graph::Variable::image`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageInput>,
}

impl InstanceParam {
    /// The parameter's description, as a node type's parameters have one. Numbers are held to
    /// the range the input value has in the graph, as the graph's own value is.
    pub fn def(&self) -> ParamDef<'_> {
        if let Some(image) = &self.image {
            return ParamDef::new(&self.name, &self.label, ParamKind::Image { kind: image.kind }).tip(&self.description);
        }
        let fallback = ParamKind::Float { default: 0.0, min: 0.0, max: 1.0, log: false, suffix: "" };
        let kind = match (self.default, self.min, self.max) {
            (Value::Float(default), Value::Float(min), Value::Float(max)) => {
                ParamKind::Float { default, min, max, log: false, suffix: "" }
            }
            (Value::Int(default), Value::Int(min), Value::Int(max)) => ParamKind::Int { default, min, max },
            (Value::Bool(default), ..) => ParamKind::Bool { default },
            (Value::Vec2(default), Value::Vec2(min), Value::Vec2(max)) => {
                ParamKind::Vec2 { default, min: min[0].min(min[1]), max: max[0].max(max[1]) }
            }
            (Value::Color(default), ..) => ParamKind::Color { default },
            _ => fallback,
        };
        let mut def = ParamDef::new(&self.name, &self.label, kind).tip(&self.description);
        def.limits = match kind {
            ParamKind::Float { min, max, .. } | ParamKind::Vec2 { min, max, .. } if min <= max => Some((min, max)),
            ParamKind::Int { min, max, .. } if min <= max => Some((min as f32, max as f32)),
            _ => None,
        };
        def
    }

    /// What the parameter of a node that starts using the graph is: the input value's value in
    /// the graph; for an image input, none of its own (the picture the graph has).
    pub fn start_value(&self) -> ParamValue {
        match self.image {
            Some(_) => ParamValue::Text(String::new()),
            None => param_value(self.value),
        }
    }

    /// Make what a file says agree with itself, as the graph's input value does: values of the
    /// type of `value`, finite, within the range.
    pub fn normalize(&mut self) {
        let ty = if self.image.is_some() { crate::types::DataType::Image } else { self.value.data_type() };
        let mut var = crate::graph::Variable {
            name: self.name.clone(),
            ty,
            value: self.value,
            default: self.default,
            min: self.min,
            max: self.max,
            description: String::new(),
            image: self.image.take(),
        };
        var.normalize();
        (self.value, self.default, self.min, self.max, self.image) = (var.value, var.default, var.min, var.max, var.image);
        if self.label.trim().is_empty() {
            self.label = label_of(&self.name);
        }
    }
}

/// What a graph is as a node: its outputs and its input values (its image inputs among them).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Interface {
    #[serde(default)]
    pub inputs: Vec<Port>,
    #[serde(default)]
    pub outputs: Vec<Port>,
    #[serde(default)]
    pub params: Vec<InstanceParam>,
}

/// The Outputs of `graph` that are its outputs as a node, in the order of the sockets: of the
/// graph from top to bottom, so that moving them there orders the sockets. Of Outputs of one
/// name, the upper one is the output.
pub fn port_nodes(graph: &Graph) -> Vec<&Node> {
    let mut nodes: Vec<&Node> = graph.nodes_of(NodeKind::OUTPUT).collect();
    nodes.sort_by(|a, b| a.pos[1].total_cmp(&b.pos[1]).then(a.pos[0].total_cmp(&b.pos[0])).then(a.id.cmp(&b.id)));
    let mut names = std::collections::HashSet::new();
    nodes.retain(|n| names.insert(n.text("name")));
    nodes
}

impl Interface {
    /// What `graph` is as a node: its outputs as [`port_nodes`] has them, and the parameters in
    /// the order of the input values.
    pub fn of(graph: &Graph) -> Interface {
        let outputs = port_nodes(graph).into_iter().map(|node| Port { name: node.text("name").to_string(), kind: None }).collect();
        let params = graph
            .variables
            .iter()
            .map(|v| InstanceParam {
                name: v.name.clone(),
                label: label_of(&v.name),
                value: v.value,
                default: v.default,
                min: v.min,
                max: v.max,
                description: v.description.clone(),
                image: v.image.clone(),
            })
            .collect();
        Interface { inputs: Vec::new(), outputs, params }
    }

    pub fn param(&self, name: &str) -> Option<&InstanceParam> {
        self.params.iter().find(|p| p.name == name)
    }

    /// Make what a file says agree with itself: one socket and one parameter of a name, and
    /// parameters whose values agree with each other.
    pub fn normalize(&mut self) {
        let mut seen = std::collections::HashSet::new();
        self.inputs.retain(|p| seen.insert(p.name.clone()));
        let mut seen = std::collections::HashSet::new();
        self.outputs.retain(|p| seen.insert(p.name.clone()));
        let mut seen = std::collections::HashSet::new();
        self.params.retain(|p| seen.insert(p.name.clone()));
        for p in &mut self.params {
            p.normalize();
        }
    }
}

/// A graph used as a node: which graph, and what it was as a node when it was last seen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Instance {
    pub graph: GraphRef,
    #[serde(default)]
    pub interface: Interface,
    /// What files of the day before version 4 said the node gives its image inputs; read
    /// only, into its parameters (see [`crate::project`]).
    #[serde(default, skip_serializing)]
    pub images: std::collections::BTreeMap<String, String>,
}

impl Instance {
    /// A node that uses the graph `graph`, which is `interface` as a node.
    pub fn new(graph: GraphRef, interface: Interface) -> Instance {
        Instance { graph, interface, images: std::collections::BTreeMap::new() }
    }
}

/// The name of an input value as the properties show it: words, with a capital first.
pub fn label_of(name: &str) -> String {
    let words = name.replace('_', " ");
    let words = words.trim();
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => name.to_string(),
    }
}

/// A value of an input value as the value of a parameter.
pub fn param_value(value: Value) -> ParamValue {
    match value {
        Value::Float(v) => ParamValue::Float(v),
        Value::Int(v) => ParamValue::Int(v),
        Value::Bool(v) => ParamValue::Bool(v),
        Value::Vec2(v) => ParamValue::Vec2(v),
        Value::Color(v) => ParamValue::Color(v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::DataType;

    #[test]
    fn a_graph_as_a_node() {
        let mut g = Graph::default();
        let input = g.add_node(NodeKind::IMAGE_INPUT, [0.0, 300.0]);
        g.bind_variable(input, "mask").unwrap();
        g.variable_mut("mask").unwrap().image.as_mut().unwrap().kind = ImageKind::Color;
        let low = g.add_node(NodeKind::OUTPUT, [400.0, 300.0]);
        let high = g.add_node(NodeKind::OUTPUT, [400.0, 100.0]);
        let width = g.add_node(NodeKind::FLOAT_INPUT, [0.0, 0.0]);
        g.bind_variable(width, "brick_width").unwrap();
        let interface = Interface::of(&g);
        // Outputs top to bottom, names made unique as the nodes were added.
        let names: Vec<&str> = interface.outputs.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, [g.node(high).unwrap().text("name"), g.node(low).unwrap().text("name")]);
        assert_ne!(names[0], names[1]);
        // The image input is a parameter, in the order of the input values: a picture of the
        // node's own (none at first: the graph's), or, exposed, a socket that takes color.
        assert!(interface.inputs.is_empty());
        let names: Vec<&str> = interface.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["mask", "brick_width"]);
        let mask = interface.param("mask").unwrap();
        assert_eq!(mask.start_value(), ParamValue::Text(String::new()));
        let def = mask.def();
        assert!(matches!(def.kind, ParamKind::Image { kind: ImageKind::Color }) && def.can_expose());
        assert_eq!(def.socket(&mask.start_value()), Some((DataType::Image, crate::nodes::Slot::Color)));
        let param = interface.param("brick_width").unwrap();
        assert_eq!(param.label, "Brick width");
        assert_eq!(param.value.data_type(), DataType::Float);
        let def = param.def();
        assert_eq!(def.key, "brick_width");
        assert_eq!(def.limits, Some((0.0, 1.0)), "held to the range of the input value");
        assert!(def.can_expose());
    }
}
