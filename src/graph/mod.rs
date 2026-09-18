//! The node graph document: nodes, links and graph input values.
//!
//! This is plain data (serializable, cheap to clone for undo snapshots). GPU state
//! lives in the engine and is keyed by [`NodeId`].

pub mod defs;
pub mod types;

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

pub use defs::{Category, NodeDef, NodeKind, ParamDef, ParamKind, def};
pub use types::{DataType, ParamValue, Tiling, Value, can_connect};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InputId(pub u64);

/// Built-in per-pixel variables of the Pixel Math node.
pub const PIXEL_MATH_BUILTINS: &[&str] = &["u", "v", "px", "py", "width", "height"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParamSlot {
    pub key: String,
    pub value: ParamValue,
    #[serde(default)]
    pub exposed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub pos: [f32; 2],
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub params: Vec<ParamSlot>,
    /// Graph input read by an [`NodeKind::Input`] node.
    #[serde(default)]
    pub input_ref: Option<InputId>,
    /// Values used by expression variables that are not connected.
    #[serde(default)]
    pub var_values: BTreeMap<String, f32>,
}

impl Node {
    pub fn new(id: NodeId, kind: NodeKind, pos: [f32; 2]) -> Node {
        let params = def(kind)
            .all_params()
            .map(|p| ParamSlot { key: p.key.to_string(), value: p.default_value(), exposed: p.exposed_by_default })
            .collect();
        Node { id, kind, pos, label: String::new(), params, input_ref: None, var_values: BTreeMap::new() }
    }

    pub fn def(&self) -> &'static NodeDef {
        def(self.kind)
    }

    pub fn param(&self, key: &str) -> Option<&ParamSlot> {
        self.params.iter().find(|p| p.key == key)
    }

    pub fn param_mut(&mut self, key: &str) -> Option<&mut ParamSlot> {
        self.params.iter_mut().find(|p| p.key == key)
    }

    pub fn text(&self, key: &str) -> &str {
        match self.param(key).map(|p| &p.value) {
            Some(ParamValue::Text(s)) => s,
            _ => "",
        }
    }

    pub fn enum_value(&self, key: &str) -> u32 {
        match self.param(key).map(|p| &p.value) {
            Some(ParamValue::Enum(v)) => *v,
            _ => 0,
        }
    }

    pub fn bool_value(&self, key: &str) -> bool {
        matches!(self.param(key).map(|p| &p.value), Some(ParamValue::Bool(true)))
    }

    pub fn tiling(&self) -> Tiling {
        Tiling::from_index(self.enum_value(defs::TILING_PARAM.key))
    }

    pub fn is_expression_node(&self) -> bool {
        matches!(self.kind, NodeKind::Math | NodeKind::PixelMath)
    }

    /// Variables of a Math / Pixel Math expression (stable while the user types).
    pub fn expression_vars(&self) -> Vec<String> {
        if !self.is_expression_node() {
            return Vec::new();
        }
        let mut vars = crate::expr::variables_lenient(self.text("expression"));
        if self.kind == NodeKind::PixelMath {
            vars.retain(|v| !PIXEL_MATH_BUILTINS.contains(&v.as_str()));
        }
        vars
    }

    /// Bring the parameter list in line with the node definition (after loading a file
    /// written by another version, or hand-edited JSON).
    pub fn normalize(&mut self) {
        let old = std::mem::take(&mut self.params);
        self.params = self
            .def()
            .all_params()
            .map(|p| {
                let default = p.default_value();
                match old.iter().find(|s| s.key == p.key) {
                    Some(slot) if slot.value.same_kind(&default) => {
                        let mut slot = slot.clone();
                        if let (ParamValue::Enum(v), ParamKind::Enum { options, .. }) = (&mut slot.value, p.kind) {
                            *v = (*v).min(options.len() as u32 - 1);
                        }
                        slot.exposed &= p.can_expose();
                        slot
                    }
                    _ => ParamSlot { key: p.key.to_string(), value: default, exposed: p.exposed_by_default },
                }
            })
            .collect();
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Link {
    pub from: NodeId,
    pub from_socket: String,
    pub to: NodeId,
    pub to_socket: String,
}

/// A value parameter of the whole graph, edited in the Input Values panel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphInput {
    pub id: InputId,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub ty: DataType,
    pub value: Value,
    pub default: Value,
    pub min: Value,
    pub max: Value,
}

impl GraphInput {
    pub fn new(id: InputId, name: String, ty: DataType) -> GraphInput {
        let (value, min, max) = match ty {
            DataType::Float => (Value::Float(0.5), Value::Float(0.0), Value::Float(1.0)),
            DataType::Int => (Value::Int(4), Value::Int(0), Value::Int(16)),
            DataType::Bool => (Value::Bool(false), Value::Bool(false), Value::Bool(true)),
            DataType::Vec2 => (Value::Vec2([0.0, 0.0]), Value::Vec2([-1.0, -1.0]), Value::Vec2([1.0, 1.0])),
            _ => (Value::Color([0.8, 0.5, 0.2, 1.0]), Value::Color([0.0; 4]), Value::Color([1.0; 4])),
        };
        GraphInput { id, name, description: String::new(), ty, value, default: value, min, max }
    }

    pub fn has_range(&self) -> bool {
        matches!(self.ty, DataType::Float | DataType::Int | DataType::Vec2)
    }

    /// Make every stored value match `ty` and keep value/default inside [min, max].
    pub fn normalize(&mut self) {
        let fix = |v: &Value, ty: DataType| v.convert(ty).unwrap_or_else(|| Value::default_for(ty));
        self.value = fix(&self.value, self.ty);
        self.default = fix(&self.default, self.ty);
        self.min = fix(&self.min, self.ty);
        self.max = fix(&self.max, self.ty);
        self.value = self.value.clamped(&self.min, &self.max);
        self.default = self.default.clamped(&self.min, &self.max);
    }

    /// Change the type, converting the current values where possible.
    pub fn set_type(&mut self, ty: DataType) {
        if ty == self.ty {
            return;
        }
        let fresh = GraphInput::new(self.id, self.name.clone(), ty);
        let conv = |v: &Value, fallback: Value| v.convert(ty).unwrap_or(fallback);
        self.value = conv(&self.value, fresh.value);
        self.default = conv(&self.default, fresh.default);
        self.min = if self.has_range() { conv(&self.min, fresh.min) } else { fresh.min };
        self.max = if self.has_range() { conv(&self.max, fresh.max) } else { fresh.max };
        self.ty = ty;
        if matches!(ty, DataType::Bool | DataType::Color) {
            self.min = fresh.min;
            self.max = fresh.max;
        }
        self.normalize();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketKind {
    Image,
    Param,
    Variable,
    Output,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SocketInfo {
    pub key: String,
    pub label: String,
    pub ty: DataType,
    pub kind: SocketKind,
}

/// Nodes and links copied to the clipboard.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Clipboard {
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
    pub inputs: Vec<GraphInput>,
}

impl Clipboard {
    pub const MAGIC: &'static str = "ferrotex-nodes:";
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
    pub inputs: Vec<GraphInput>,
    pub resolution: [u32; 2],
    pub next_id: u64,
}

impl Default for Graph {
    fn default() -> Self {
        Graph { nodes: Vec::new(), links: Vec::new(), inputs: Vec::new(), resolution: [2048, 2048], next_id: 1 }
    }
}

pub fn image_socket_key(key: &str) -> String {
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
        out.push_str("input");
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    if crate::expr::builtins::is_builtin(&out) {
        out.push('_');
    }
    out
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

    pub fn add_node(&mut self, kind: NodeKind, pos: [f32; 2]) -> NodeId {
        let id = NodeId(self.alloc_id());
        let mut node = Node::new(id, kind, pos);
        if kind == NodeKind::Output {
            let name = self.unique_output_name("output");
            if let Some(p) = node.param_mut("name") {
                p.value = ParamValue::Text(name);
            }
        }
        self.nodes.push(node);
        id
    }

    pub fn add_input_node(&mut self, input: InputId, pos: [f32; 2]) -> NodeId {
        let id = self.add_node(NodeKind::Input, pos);
        if let Some(n) = self.node_mut(id) {
            n.input_ref = Some(input);
        }
        id
    }

    pub fn remove_nodes(&mut self, ids: &[NodeId]) {
        let set: HashSet<NodeId> = ids.iter().copied().collect();
        self.nodes.retain(|n| !set.contains(&n.id));
        self.links.retain(|l| !set.contains(&l.from) && !set.contains(&l.to));
    }

    /// Move nodes to the end of the list so they draw on top.
    pub fn bring_to_front(&mut self, ids: &[NodeId]) {
        let set: HashSet<NodeId> = ids.iter().copied().collect();
        let (mut front, back): (Vec<Node>, Vec<Node>) =
            std::mem::take(&mut self.nodes).into_iter().partition(|n| !set.contains(&n.id));
        front.extend(back);
        self.nodes = front;
    }

    // ------------------------------------------------------------------ inputs

    pub fn input(&self, id: InputId) -> Option<&GraphInput> {
        self.inputs.iter().find(|i| i.id == id)
    }

    pub fn input_mut(&mut self, id: InputId) -> Option<&mut GraphInput> {
        self.inputs.iter_mut().find(|i| i.id == id)
    }

    pub fn input_by_name(&self, name: &str) -> Option<&GraphInput> {
        self.inputs.iter().find(|i| i.name == name)
    }

    pub fn unique_input_name(&self, base: &str, except: Option<InputId>) -> String {
        let base = sanitize_identifier(base);
        let taken = |name: &str| self.inputs.iter().any(|i| i.name == name && Some(i.id) != except);
        if !taken(&base) {
            return base;
        }
        let stem = base.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end_matches('_');
        let stem = if stem.is_empty() { "input" } else { stem };
        (1..).map(|i| format!("{stem}_{i}")).find(|n| !taken(n)).unwrap_or(base)
    }

    pub fn unique_output_name(&self, base: &str) -> String {
        let taken = |name: &str| self.nodes.iter().any(|n| n.kind == NodeKind::Output && n.text("name") == name);
        if !taken(base) {
            return base.to_string();
        }
        (1..).map(|i| format!("{base}_{i}")).find(|n| !taken(n)).unwrap_or_else(|| base.to_string())
    }

    pub fn add_input(&mut self, ty: DataType, base_name: &str) -> InputId {
        let id = InputId(self.alloc_id());
        let name = self.unique_input_name(base_name, None);
        self.inputs.push(GraphInput::new(id, name, ty));
        id
    }

    /// Remove a graph input. Input nodes reading it stay in the graph and report an error.
    pub fn remove_input(&mut self, id: InputId) {
        self.inputs.retain(|i| i.id != id);
    }

    // ------------------------------------------------------------------ sockets

    pub fn input_sockets(&self, node: &Node) -> Vec<SocketInfo> {
        let d = node.def();
        let mut out: Vec<SocketInfo> = d
            .image_inputs
            .iter()
            .map(|s| SocketInfo {
                key: image_socket_key(s.key),
                label: s.label.to_string(),
                ty: s.ty,
                kind: SocketKind::Image,
            })
            .collect();
        for slot in &node.params {
            if !slot.exposed {
                continue;
            }
            if let (Some(pd), Some(ty)) = (d.param(&slot.key), slot.value.socket_type()) {
                out.push(SocketInfo {
                    key: param_socket_key(&slot.key),
                    label: pd.label.to_string(),
                    ty,
                    kind: SocketKind::Param,
                });
            }
        }
        let var_ty = if node.kind == NodeKind::PixelMath { DataType::Image } else { DataType::Float };
        for v in node.expression_vars() {
            out.push(SocketInfo { key: var_socket_key(&v), label: v.clone(), ty: var_ty, kind: SocketKind::Variable });
        }
        out
    }

    pub fn output_sockets(&self, node: &Node) -> Vec<SocketInfo> {
        let d = node.def();
        d.outputs
            .iter()
            .map(|s| {
                let ty = if node.kind == NodeKind::Input {
                    node.input_ref.and_then(|i| self.input(i)).map_or(DataType::Float, |i| i.ty)
                } else {
                    s.ty
                };
                SocketInfo { key: output_socket_key(s.key), label: s.label.to_string(), ty, kind: SocketKind::Output }
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

    // ------------------------------------------------------------------ links

    pub fn link_into(&self, node: NodeId, socket: &str) -> Option<&Link> {
        self.links.iter().find(|l| l.to == node && l.to_socket == socket)
    }

    /// Is `candidate` (transitively) upstream of `node`?
    pub fn depends_on(&self, node: NodeId, candidate: NodeId) -> bool {
        let mut stack = vec![node];
        let mut seen = HashSet::new();
        while let Some(n) = stack.pop() {
            if n == candidate {
                return true;
            }
            if !seen.insert(n) {
                continue;
            }
            stack.extend(self.links.iter().filter(|l| l.to == n).map(|l| l.from));
        }
        false
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

    pub fn disconnect(&mut self, to: NodeId, to_socket: &str) -> bool {
        let before = self.links.len();
        self.links.retain(|l| !(l.to == to && l.to_socket == to_socket));
        before != self.links.len()
    }

    pub fn set_exposed(&mut self, node: NodeId, key: &str, exposed: bool) {
        let can = self.node(node).and_then(|n| n.def().param(key)).is_some_and(|p| p.can_expose());
        if let Some(slot) = self.node_mut(node).and_then(|n| n.param_mut(key)) {
            slot.exposed = exposed && can;
        }
        if !exposed {
            self.disconnect(node, &param_socket_key(key));
        }
    }

    /// Remove links that became invalid (deleted nodes, un-exposed parameters, type
    /// changes). Links into expression variables that temporarily disappeared while
    /// the user edits the expression are kept so they come back with the variable.
    pub fn prune_links(&mut self) {
        let mut keep = Vec::with_capacity(self.links.len());
        for l in &self.links {
            let (Some(from), Some(to)) = (self.node(l.from), self.node(l.to)) else { continue };
            let Some(out) = self.output_sockets(from).into_iter().find(|s| s.key == l.from_socket) else { continue };
            let inp = self.input_sockets(to).into_iter().find(|s| s.key == l.to_socket);
            let ok = match inp {
                Some(inp) => can_connect(out.ty, inp.ty),
                None => l.to_socket.starts_with("v:") && to.is_expression_node(),
            };
            if ok && !keep.iter().any(|k: &Link| k.to == l.to && k.to_socket == l.to_socket) {
                keep.push(l.clone());
            }
        }
        self.links = keep;
    }

    /// Topological order (sources first) and the nodes caught in cycles, if any.
    pub fn topo_order(&self) -> (Vec<NodeId>, Vec<NodeId>) {
        let mut indegree: HashMap<NodeId, usize> = self.nodes.iter().map(|n| (n.id, 0)).collect();
        let mut children: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for l in &self.links {
            if indegree.contains_key(&l.from) && indegree.contains_key(&l.to) {
                *indegree.get_mut(&l.to).unwrap() += 1;
                children.entry(l.from).or_default().push(l.to);
            }
        }
        let mut queue: VecDeque<NodeId> = self.nodes.iter().map(|n| n.id).filter(|id| indegree[id] == 0).collect();
        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(id) = queue.pop_front() {
            order.push(id);
            if let Some(ch) = children.get(&id) {
                for c in ch {
                    let d = indegree.get_mut(c).unwrap();
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
        for n in &mut self.nodes {
            n.normalize();
        }
        for i in &mut self.inputs {
            i.normalize();
        }
        let max_id = self.nodes.iter().map(|n| n.id.0).chain(self.inputs.iter().map(|i| i.id.0)).max().unwrap_or(0);
        self.next_id = self.next_id.max(max_id + 1);
        self.resolution = self.resolution.map(|r| r.clamp(16, 8192));
        let mut seen = HashSet::new();
        self.nodes.retain(|n| seen.insert(n.id));
        self.prune_links();
    }

    // ------------------------------------------------------------------ clipboard

    pub fn copy_nodes(&self, ids: &[NodeId]) -> Clipboard {
        let set: HashSet<NodeId> = ids.iter().copied().collect();
        let nodes: Vec<Node> = self.nodes.iter().filter(|n| set.contains(&n.id)).cloned().collect();
        let links = self.links.iter().filter(|l| set.contains(&l.from) && set.contains(&l.to)).cloned().collect();
        let inputs = nodes.iter().filter_map(|n| n.input_ref).filter_map(|i| self.input(i).cloned()).collect();
        Clipboard { nodes, links, inputs }
    }

    /// Paste nodes, offsetting their positions by `offset`. Returns the new node ids.
    pub fn paste(&mut self, clip: &Clipboard, offset: [f32; 2]) -> Vec<NodeId> {
        let mut input_map: HashMap<InputId, InputId> = HashMap::new();
        for inp in &clip.inputs {
            let target = if let Some(existing) = self.input(inp.id).filter(|e| e.name == inp.name && e.ty == inp.ty) {
                existing.id
            } else if let Some(existing) = self.input_by_name(&inp.name).filter(|e| e.ty == inp.ty) {
                existing.id
            } else {
                let id = InputId(self.alloc_id());
                let mut copy = inp.clone();
                copy.id = id;
                copy.name = self.unique_input_name(&inp.name, None);
                self.inputs.push(copy);
                id
            };
            input_map.insert(inp.id, target);
        }
        let mut node_map: HashMap<NodeId, NodeId> = HashMap::new();
        let mut new_ids = Vec::new();
        for n in &clip.nodes {
            let mut copy = n.clone();
            copy.id = NodeId(self.alloc_id());
            copy.pos = [n.pos[0] + offset[0], n.pos[1] + offset[1]];
            copy.input_ref = n.input_ref.map(|i| input_map.get(&i).copied().unwrap_or(i));
            copy.normalize();
            if copy.kind == NodeKind::Output {
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
        self.prune_links();
        new_ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_rules_and_cycles() {
        let mut g = Graph::default();
        let a = g.add_node(NodeKind::UniformColor, [0.0, 0.0]);
        let b = g.add_node(NodeKind::Invert, [0.0, 0.0]);
        let c = g.add_node(NodeKind::Blur, [0.0, 0.0]);
        let m = g.add_node(NodeKind::Math, [0.0, 0.0]);
        assert!(g.connect(a, "o:output", b, "i:input").is_ok());
        assert!(g.connect(b, "o:output", c, "i:input").is_ok());
        // cycle c -> b
        assert!(g.connect(c, "o:output", b, "i:input").unwrap_err().contains("cycle"));
        // type mismatch: image -> math variable
        assert!(g.connect(a, "o:output", m, "v:x").is_err());
        // unexposed param is not a socket
        assert!(g.connect(m, "o:result", c, "p:radius").is_err());
        g.set_exposed(c, "radius", true);
        assert!(g.connect(m, "o:result", c, "p:radius").is_ok());
        // un-exposing removes the link
        g.set_exposed(c, "radius", false);
        assert!(g.link_into(c, "p:radius").is_none());
        let (order, cyclic) = g.topo_order();
        assert!(cyclic.is_empty());
        let pos = |id| order.iter().position(|x| *x == id).unwrap();
        assert!(pos(a) < pos(b) && pos(b) < pos(c));
    }

    #[test]
    fn math_sockets_follow_the_expression() {
        let mut g = Graph::default();
        let m = g.add_node(NodeKind::Math, [0.0, 0.0]);
        let node = g.node(m).unwrap();
        let keys: Vec<String> = g.input_sockets(node).into_iter().map(|s| s.key).collect();
        assert_eq!(keys, vec!["v:x", "v:myVar", "v:myVar2"]);
        let f = g.add_input(DataType::Float, "speed");
        let i = g.add_input_node(f, [0.0, 0.0]);
        g.connect(i, "o:value", m, "v:myVar").unwrap();
        // While typing, the variable disappears: the link is kept dormant.
        g.node_mut(m).unwrap().param_mut("expression").unwrap().value = ParamValue::Text("x + ".into());
        g.prune_links();
        assert_eq!(g.links.len(), 1);
        g.node_mut(m).unwrap().param_mut("expression").unwrap().value = ParamValue::Text("x + myVar".into());
        assert!(g.link_into(m, "v:myVar").is_some());
    }

    #[test]
    fn input_type_change_prunes_links() {
        let mut g = Graph::default();
        let inp = g.add_input(DataType::Float, "f");
        let n = g.add_input_node(inp, [0.0, 0.0]);
        let blur = g.add_node(NodeKind::Blur, [0.0, 0.0]);
        g.set_exposed(blur, "radius", true);
        g.connect(n, "o:value", blur, "p:radius").unwrap();
        g.input_mut(inp).unwrap().set_type(DataType::Color);
        g.prune_links();
        assert!(g.links.is_empty());
    }

    #[test]
    fn copy_paste_remaps_ids() {
        let mut g = Graph::default();
        let a = g.add_node(NodeKind::Noise, [0.0, 0.0]);
        let b = g.add_node(NodeKind::Levels, [100.0, 0.0]);
        g.connect(a, "o:output", b, "i:input").unwrap();
        let clip = g.copy_nodes(&[a, b]);
        let new = g.paste(&clip, [10.0, 10.0]);
        assert_eq!(new.len(), 2);
        assert_eq!(g.nodes.len(), 4);
        assert_eq!(g.links.len(), 2);
        assert!(g.link_into(new[1], "i:input").is_some_and(|l| l.from == new[0]));
    }

    #[test]
    fn serialization_roundtrip() {
        let mut g = Graph::default();
        let a = g.add_node(NodeKind::Shape, [1.0, 2.0]);
        let o = g.add_node(NodeKind::Output, [3.0, 4.0]);
        g.connect(a, "o:output", o, "i:input").unwrap();
        g.add_input(DataType::Vec2, "offset");
        let json = serde_json::to_string(&g).unwrap();
        let mut back: Graph = serde_json::from_str(&json).unwrap();
        back.normalize();
        assert_eq!(g, back);
    }

    #[test]
    fn identifiers() {
        assert_eq!(sanitize_identifier("my value"), "my_value");
        assert_eq!(sanitize_identifier("2x"), "_2x");
        assert_eq!(sanitize_identifier("sin"), "sin_");
        let mut g = Graph::default();
        g.add_input(DataType::Float, "a");
        assert_eq!(g.unique_input_name("a", None), "a_1");
    }
}
