//! Graphs used as nodes.
//!
//! A node of [`NodeKind::GRAPH_INSTANCE`] stands for a graph, of its own package or of another
//! package file, and makes what that graph makes. Its sockets and parameters are the graph's:
//! an input for each Image Input node, an output for each Output node, a parameter for each
//! input value. The node keeps a copy of them, its [`Interface`]: its sockets, links and
//! values stay while the graph cannot be found (a package file that was moved), and the front
//! end brings the copy up to date whenever the graph changes (see [`Interface::of`] and
//! [`crate::Node::set_interface`]).
//!
//! The engine never sees such a node: before a graph is computed, every graph it uses is laid
//! out in it, node by node (see [`crate::package::flatten`]).

use serde::{Deserialize, Serialize};

use crate::graph::{Graph, Node};
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

/// An image input or output of a graph.
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
}

impl InstanceParam {
    /// The parameter's description, as a node type's parameters have one. Numbers are held to
    /// the range the input value has in the graph, as the graph's own value is.
    pub fn def(&self) -> ParamDef<'_> {
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

    /// Make what a file says agree with itself, as the graph's input value does: values of the
    /// type of `value`, finite, within the range.
    pub fn normalize(&mut self) {
        let mut var = crate::graph::Variable {
            name: self.name.clone(),
            ty: self.value.data_type(),
            value: self.value,
            default: self.default,
            min: self.min,
            max: self.max,
            description: String::new(),
        };
        var.normalize();
        (self.value, self.default, self.min, self.max) = (var.value, var.default, var.min, var.max);
        if self.label.trim().is_empty() {
            self.label = label_of(&self.name);
        }
    }
}

/// What a graph is as a node: its image inputs, its outputs and its input values.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Interface {
    #[serde(default)]
    pub inputs: Vec<Port>,
    #[serde(default)]
    pub outputs: Vec<Port>,
    #[serde(default)]
    pub params: Vec<InstanceParam>,
}

/// The Image Inputs or the Outputs (`kind`) of `graph` that are its sockets as a node, in the
/// order of the sockets: of the graph from top to bottom, so that moving them there orders the
/// sockets. Of nodes of one name, the upper one is the socket.
pub fn port_nodes(graph: &Graph, kind: NodeKind) -> Vec<&Node> {
    let mut nodes: Vec<&Node> = graph.nodes_of(kind).collect();
    nodes.sort_by(|a, b| a.pos[1].total_cmp(&b.pos[1]).then(a.pos[0].total_cmp(&b.pos[0])).then(a.id.cmp(&b.id)));
    let mut names = std::collections::HashSet::new();
    nodes.retain(|n| names.insert(n.text("name")));
    nodes
}

impl Interface {
    /// What `graph` is as a node: its inputs and outputs as [`port_nodes`] has them, and the
    /// parameters in the order of the input values.
    pub fn of(graph: &Graph) -> Interface {
        let ports = |kind: NodeKind| {
            port_nodes(graph, kind)
                .into_iter()
                .map(|node| Port {
                    name: node.text("name").to_string(),
                    kind: (kind == NodeKind::IMAGE_INPUT).then(|| image_input_kind(node)),
                })
                .collect()
        };
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
            })
            .collect();
        Interface { inputs: ports(NodeKind::IMAGE_INPUT), outputs: ports(NodeKind::OUTPUT), params }
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

/// What an Image Input node takes.
pub fn image_input_kind(node: &Node) -> ImageKind {
    if node.enum_value("kind") == 1 { ImageKind::Color } else { ImageKind::Gray }
}

/// A graph used as a node: which graph, and what it was as a node when it was last seen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Instance {
    pub graph: GraphRef,
    #[serde(default)]
    pub interface: Interface,
    /// The picture each image input of the node takes while nothing arrives at it (by the
    /// input's name): a file, as a Bitmap reads one. Without one, the input takes the graph's
    /// own (see [`crate::nodes::IMAGE_INPUT`]).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
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
        let low = g.add_node(NodeKind::IMAGE_INPUT, [0.0, 300.0]);
        let high = g.add_node(NodeKind::IMAGE_INPUT, [0.0, 100.0]);
        g.node_mut(high).unwrap().param_mut("kind").unwrap().value = ParamValue::Enum(1);
        g.add_node(NodeKind::OUTPUT, [400.0, 0.0]);
        let width = g.add_node(NodeKind::FLOAT_INPUT, [0.0, 0.0]);
        g.bind_variable(width, "brick_width").unwrap();
        let interface = Interface::of(&g);
        // Top to bottom, names made unique as the nodes were added.
        let names: Vec<&str> = interface.inputs.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, [g.node(high).unwrap().text("name"), g.node(low).unwrap().text("name")]);
        assert_ne!(names[0], names[1]);
        assert_eq!(interface.inputs[0].kind, Some(ImageKind::Color));
        assert_eq!(interface.inputs[1].kind, Some(ImageKind::Gray));
        assert_eq!(interface.outputs.len(), 1);
        let param = interface.param("brick_width").unwrap();
        assert_eq!(param.label, "Brick width");
        assert_eq!(param.value.data_type(), DataType::Float);
        let def = param.def();
        assert_eq!(def.key, "brick_width");
        assert_eq!(def.limits, Some((0.0, 1.0)), "held to the range of the input value");
        assert!(def.can_expose());
    }
}
