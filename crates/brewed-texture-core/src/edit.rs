//! Every modification of a graph is an [`Edit`]. Front ends describe what the user did;
//! the edit validates it, applies it and keeps the graph's invariants. The
//! [`crate::Document`] records edits for undo / redo.

use crate::gradient::Gradient;
use crate::instance::Instance;
use crate::graph::{Clipboard, CommentId, Graph, Link, MAX_RESOLUTION, MIN_RESOLUTION, NodeId, Precision, SocketRef, coordinate};
use crate::nodes::{NodeKind, Role};
use crate::types::{DataType, ParamValue, Value, can_connect};

#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    AddNode {
        kind: NodeKind,
        pos: [f32; 2],
    },
    /// Add a node with some parameters already set (a Bitmap for a dropped image file).
    AddNodeWith {
        kind: NodeKind,
        pos: [f32; 2],
        params: Vec<(String, ParamValue)>,
    },
    /// Add an Input node reading an existing variable.
    AddVariableNode {
        name: String,
        pos: [f32; 2],
    },
    /// Add a node that uses a graph (see [`crate::instance`]).
    AddInstance {
        instance: Instance,
        pos: [f32; 2],
    },
    /// Add a node and connect it to the socket a link was dragged from.
    AddNodeConnected {
        kind: NodeKind,
        pos: [f32; 2],
        socket: SocketRef,
    },
    RemoveNodes(Vec<NodeId>),
    RemoveLink(Link),
    MoveNodes {
        ids: Vec<NodeId>,
        delta: [f32; 2],
    },
    Connect {
        a: SocketRef,
        b: SocketRef,
    },
    Disconnect {
        node: NodeId,
        socket: String,
    },
    DisconnectInputs(NodeId),
    SetParam {
        node: NodeId,
        key: String,
        value: ParamValue,
    },
    /// The same parameter on several nodes (multi-selection).
    SetParamMany {
        nodes: Vec<NodeId>,
        key: String,
        value: ParamValue,
    },
    SetExposed {
        node: NodeId,
        key: String,
        exposed: bool,
    },
    SetBypass {
        node: NodeId,
        bypass: bool,
    },
    /// Value of an unconnected expression variable.
    SetLocalVar {
        node: NodeId,
        name: String,
        value: f32,
    },
    SetResolution([u32; 2]),
    SetPrecision(Precision),
    /// Paste with the top-left of the copied nodes at `at`.
    Paste {
        clipboard: Clipboard,
        at: [f32; 2],
    },
    Duplicate {
        ids: Vec<NodeId>,
        offset: [f32; 2],
    },
    SetVariable {
        name: String,
        value: Value,
    },
    SetVariableDefault {
        name: String,
        value: Value,
    },
    SetVariableRange {
        name: String,
        min: Value,
        max: Value,
    },
    SetVariableDescription {
        name: String,
        text: String,
    },
    ResetVariable(String),
    RenameVariable {
        from: String,
        to: String,
    },
    /// Point an Input node at another (existing or new) variable.
    BindVariable {
        node: NodeId,
        name: String,
    },
    // ------------------------------------------------------------------- comments
    /// A comment frame around a group of nodes.
    AddComment {
        text: String,
        pos: [f32; 2],
        size: [f32; 2],
    },
    RemoveComments(Vec<CommentId>),
    /// Move a comment, and with it the nodes it holds (the front end knows which, since
    /// it knows how large the nodes are drawn).
    MoveComment {
        id: CommentId,
        delta: [f32; 2],
        nodes: Vec<NodeId>,
    },
    /// Drag a corner of a comment: both its position and its size can change.
    ResizeComment {
        id: CommentId,
        pos: [f32; 2],
        size: [f32; 2],
    },
    SetCommentText {
        id: CommentId,
        text: String,
    },
    SetCommentColor {
        id: CommentId,
        color: [f32; 4],
    },
    /// Several edits as one undo step, applied in order: all of them or none.
    Batch(Vec<Edit>),
}

/// What an edit did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditResult {
    /// Nodes created by the edit (the front end usually selects them).
    pub created: Vec<NodeId>,
    /// False when the edit turned out to be a no-op (e.g. setting the current value).
    pub changed: bool,
    /// Something worth telling the user although the edit succeeded.
    pub notice: Option<String>,
}

impl EditResult {
    fn changed() -> Self {
        EditResult { changed: true, ..Default::default() }
    }
    fn unchanged() -> Self {
        EditResult::default()
    }
    fn created(ids: Vec<NodeId>) -> Self {
        EditResult { created: ids, changed: true, notice: None }
    }
}

/// Consecutive edits with the same key (a slider drag, typing into one field) are merged
/// into one undo step.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MergeKey {
    Param(NodeId, String),
    ParamMany(Vec<NodeId>, String),
    Move(Vec<NodeId>),
    LocalVar(NodeId, String),
    Variable(String, u8),
    Comment(CommentId, u8),
}

/// Would a node of type `kind` accept a link from (`from_output`) or to a socket of
/// type `ty`? Used to filter the node search after dropping a link on empty space.
pub fn accepts_link(kind: NodeKind, ty: DataType, from_output: bool) -> bool {
    let d = kind.def();
    if from_output {
        // The new node needs a compatible input: a fixed one or an expression variable.
        d.inputs.iter().any(|s| can_connect(ty, s.ty))
            || (d.is_pixel_expression() && ty == DataType::Image)
            || (d.expression_param().is_some() && !d.is_pixel_expression() && ty.is_scalar())
    } else {
        d.outputs.iter().any(|s| can_connect(s.ty, ty))
    }
}

fn node_exists(graph: &Graph, id: NodeId) -> Result<(), String> {
    if graph.contains(id) { Ok(()) } else { Err("The node no longer exists".into()) }
}

fn set_param(graph: &mut Graph, node: NodeId, key: &str, value: ParamValue) -> Result<bool, String> {
    let n = graph.node(node).ok_or("The node no longer exists")?;
    let pd = n.param_def(key).ok_or_else(|| format!("Unknown parameter '{key}'"))?;
    if !pd.accepts(&value) {
        return Err(format!("Wrong value type for '{}'", pd.label));
    }
    if n.is_exposed(key) {
        return Err(format!("'{}' is exposed: its value comes from the node connected to its socket", pd.label));
    }
    let value = pd.sanitize(value);
    let slot = graph.node_mut(node).and_then(|n| n.param_mut(key)).ok_or("Missing parameter slot")?;
    if slot.value == value {
        return Ok(false);
    }
    slot.value = value;
    Ok(true)
}

fn variable_mut<'a>(graph: &'a mut Graph, name: &str) -> Result<&'a mut crate::Variable, String> {
    graph.variable_mut(name).ok_or_else(|| format!("There is no variable named '{name}'"))
}

impl Edit {
    /// Short description for the Edit menu ("Undo Move").
    pub fn label(&self) -> &'static str {
        match self {
            Edit::AddNode { .. }
            | Edit::AddNodeWith { .. }
            | Edit::AddVariableNode { .. }
            | Edit::AddInstance { .. }
            | Edit::AddNodeConnected { .. } => "Add Node",
            Edit::RemoveNodes(_) => "Delete",
            Edit::RemoveLink(_) | Edit::Disconnect { .. } | Edit::DisconnectInputs(_) => "Disconnect",
            Edit::MoveNodes { .. } => "Move",
            Edit::Connect { .. } => "Connect",
            Edit::SetParam { .. } | Edit::SetParamMany { .. } => "Change Parameter",
            Edit::SetExposed { exposed: true, .. } => "Expose",
            Edit::SetExposed { exposed: false, .. } => "Unexpose",
            Edit::SetBypass { .. } => "Bypass",
            Edit::SetLocalVar { .. } => "Change Variable",
            Edit::SetResolution(_) => "Change Size",
            Edit::SetPrecision(_) => "Change Precision",
            Edit::Paste { .. } => "Paste",
            Edit::Duplicate { .. } => "Duplicate",
            Edit::SetVariable { .. }
            | Edit::SetVariableDefault { .. }
            | Edit::SetVariableRange { .. }
            | Edit::SetVariableDescription { .. }
            | Edit::ResetVariable(_) => "Change Input Value",
            Edit::RenameVariable { .. } | Edit::BindVariable { .. } => "Rename Input",
            Edit::AddComment { .. } => "Add Comment",
            Edit::RemoveComments(_) => "Delete Comment",
            Edit::MoveComment { .. } => "Move",
            Edit::ResizeComment { .. } => "Resize Comment",
            Edit::SetCommentText { .. } => "Edit Comment",
            Edit::SetCommentColor { .. } => "Color Comment",
            Edit::Batch(edits) => edits.last().map_or("Edit", Edit::label),
        }
    }

    pub fn merge_key(&self) -> Option<MergeKey> {
        Some(match self {
            Edit::SetParam { node, key, value } => {
                // Picking another enum entry or toggling is a discrete action of its own.
                if matches!(value, ParamValue::Enum(_) | ParamValue::Bool(_)) {
                    return None;
                }
                MergeKey::Param(*node, key.clone())
            }
            Edit::SetParamMany { nodes, key, .. } => MergeKey::ParamMany(nodes.clone(), key.clone()),
            Edit::MoveNodes { ids, .. } => MergeKey::Move(ids.clone()),
            Edit::SetLocalVar { node, name, .. } => MergeKey::LocalVar(*node, name.clone()),
            Edit::SetVariable { name, .. } => MergeKey::Variable(name.clone(), 0),
            Edit::SetVariableRange { name, .. } => MergeKey::Variable(name.clone(), 1),
            Edit::SetVariableDescription { name, .. } => MergeKey::Variable(name.clone(), 2),
            Edit::SetVariableDefault { name, .. } => MergeKey::Variable(name.clone(), 3),
            Edit::MoveComment { id, .. } => MergeKey::Comment(*id, 0),
            Edit::ResizeComment { id, .. } => MergeKey::Comment(*id, 1),
            Edit::SetCommentText { id, .. } => MergeKey::Comment(*id, 2),
            _ => return None,
        })
    }

    /// Validate and apply. On error the graph is left unchanged.
    pub fn apply(self, graph: &mut Graph) -> Result<EditResult, String> {
        let result = match self {
            Edit::AddNode { kind, pos } => EditResult::created(vec![graph.add_node(kind, pos)]),
            Edit::AddNodeWith { kind, pos, params } => {
                // Validate first so a bad parameter adds nothing.
                for (key, value) in &params {
                    let pd = kind.def().param(key).ok_or_else(|| format!("{} has no '{key}'", kind.def().name))?;
                    if !pd.accepts(value) {
                        return Err(format!("Wrong value type for '{}'", pd.label));
                    }
                }
                let id = graph.add_node(kind, pos);
                for (key, value) in params {
                    set_param(graph, id, &key, value)?;
                }
                EditResult::created(vec![id])
            }
            Edit::AddVariableNode { name, pos } => {
                let id = graph.add_variable_node(&name, pos).ok_or_else(|| format!("No variable named '{name}'"))?;
                EditResult::created(vec![id])
            }
            Edit::AddInstance { mut instance, pos } => {
                instance.interface.normalize();
                EditResult::created(vec![graph.add_instance(instance, pos)])
            }
            Edit::AddNodeConnected { kind, pos, socket } => {
                let info = graph.socket(&socket).ok_or("The socket the link came from no longer exists")?;
                if !accepts_link(kind, info.ty, socket.output) {
                    return Err(format!("{} cannot be connected to a {} socket", kind.def().name, info.ty.name()));
                }
                let id = graph.add_node(kind, pos);
                if socket.output && kind.def().is_pixel_expression() && info.ty == DataType::Image {
                    // A fresh Pixel Math has no image variable yet: give it one.
                    if let Some(p) = graph.node_mut(id).and_then(|n| n.param_mut("expression")) {
                        p.value = ParamValue::Text("a".into());
                    }
                }
                let node = graph.node(id).cloned().expect("just added");
                let connected = if socket.output {
                    let target = graph.input_sockets(&node).into_iter().find(|s| can_connect(info.ty, s.ty));
                    target.map(|t| graph.connect(socket.node, &socket.key, id, &t.key))
                } else {
                    let source = graph.output_sockets(&node).into_iter().find(|s| can_connect(s.ty, info.ty));
                    source.map(|s| graph.connect(id, &s.key, socket.node, &socket.key))
                };
                let mut r = EditResult::created(vec![id]);
                if let Some(Err(e)) = connected {
                    r.notice = Some(e);
                }
                r
            }
            Edit::RemoveNodes(ids) => {
                if ids.is_empty() || !ids.iter().any(|id| graph.contains(*id)) {
                    return Ok(EditResult::unchanged());
                }
                graph.remove_nodes(&ids);
                EditResult::changed()
            }
            Edit::RemoveLink(link) => {
                let before = graph.links.len();
                graph.links.retain(|l| *l != link);
                if graph.links.len() == before { EditResult::unchanged() } else { EditResult::changed() }
            }
            Edit::MoveNodes { ids, delta } => {
                if !delta.iter().all(|d| d.is_finite()) || delta == [0.0, 0.0] {
                    return Ok(EditResult::unchanged());
                }
                for n in graph.nodes.iter_mut().filter(|n| ids.contains(&n.id)) {
                    n.pos = [coordinate(n.pos[0] + delta[0]), coordinate(n.pos[1] + delta[1])];
                }
                EditResult::changed()
            }
            Edit::Connect { a, b } => {
                let (out, inp) = if a.output { (&a, &b) } else { (&b, &a) };
                if graph.link_into(inp.node, &inp.key).is_some_and(|l| l.from == out.node && l.from_socket == out.key) {
                    return Ok(EditResult::unchanged());
                }
                graph.connect_refs(&a, &b)?;
                EditResult::changed()
            }
            Edit::Disconnect { node, socket } => {
                if graph.disconnect(node, &socket) {
                    EditResult::changed()
                } else {
                    EditResult::unchanged()
                }
            }
            Edit::DisconnectInputs(node) => {
                let before = graph.links.len();
                graph.links.retain(|l| l.to != node);
                if graph.links.len() == before { EditResult::unchanged() } else { EditResult::changed() }
            }
            Edit::SetParam { node, key, value } => {
                if set_param(graph, node, &key, value)? {
                    EditResult::changed()
                } else {
                    EditResult::unchanged()
                }
            }
            Edit::SetParamMany { nodes, key, value } => {
                // Validate everything first so a failure changes nothing.
                for id in &nodes {
                    let n = graph.node(*id).ok_or("A node no longer exists")?;
                    let pd = n.param_def(&key).ok_or_else(|| format!("{} has no '{key}'", n.def().name))?;
                    if !pd.accepts(&value) || n.is_exposed(&key) {
                        return Err(format!("'{}' cannot be set on {}", pd.label, n.def().name));
                    }
                }
                let mut changed = false;
                for id in &nodes {
                    changed |= set_param(graph, *id, &key, value.clone())?;
                }
                if changed { EditResult::changed() } else { EditResult::unchanged() }
            }
            Edit::SetExposed { node, key, exposed } => {
                if graph.node(node).is_some_and(|n| n.is_exposed(&key) == exposed) {
                    return Ok(EditResult::unchanged());
                }
                graph.set_exposed(node, &key, exposed)?;
                EditResult::changed()
            }
            Edit::SetBypass { node, bypass } => {
                let n = graph.node_mut(node).ok_or("The node no longer exists")?;
                if !n.def().has_image() || n.def().inputs.is_empty() || n.kind == NodeKind::OUTPUT {
                    return Err("Only filters with an image input can be bypassed".into());
                }
                if n.bypass == bypass {
                    return Ok(EditResult::unchanged());
                }
                n.bypass = bypass;
                EditResult::changed()
            }
            Edit::SetLocalVar { node, name, value } => {
                let n = graph.node_mut(node).ok_or("The node no longer exists")?;
                if !value.is_finite() {
                    return Err("The value must be a finite number".into());
                }
                if n.var_values.get(&name) == Some(&value) {
                    return Ok(EditResult::unchanged());
                }
                n.var_values.insert(name, value);
                EditResult::changed()
            }
            Edit::SetResolution(res) => {
                let res = res.map(|r| r.clamp(MIN_RESOLUTION, MAX_RESOLUTION));
                if graph.resolution == res {
                    return Ok(EditResult::unchanged());
                }
                graph.resolution = res;
                EditResult::changed()
            }
            Edit::SetPrecision(precision) => {
                if graph.precision == precision {
                    return Ok(EditResult::unchanged());
                }
                graph.precision = precision;
                EditResult::changed()
            }
            Edit::Paste { clipboard, at } => {
                let Some(origin) = clipboard.origin() else { return Ok(EditResult::unchanged()) };
                let ids = graph.paste(&clipboard, [at[0] - origin[0], at[1] - origin[1]]);
                EditResult::created(ids)
            }
            Edit::Duplicate { ids, offset } => {
                for id in &ids {
                    node_exists(graph, *id)?;
                }
                if ids.is_empty() {
                    return Ok(EditResult::unchanged());
                }
                let clip = graph.copy_nodes(&ids);
                EditResult::created(graph.paste(&clip, offset))
            }
            Edit::SetVariable { name, value } => {
                let v = variable_mut(graph, &name)?;
                let converted = value.convert(v.ty).filter(Value::is_finite).ok_or("Wrong value type")?;
                let clamped = converted.clamped(&v.min, &v.max);
                if v.value == clamped {
                    return Ok(EditResult::unchanged());
                }
                v.value = clamped;
                EditResult::changed()
            }
            Edit::SetVariableDefault { name, value } => {
                let v = variable_mut(graph, &name)?;
                let converted = value.convert(v.ty).filter(Value::is_finite).ok_or("Wrong value type")?;
                let clamped = converted.clamped(&v.min, &v.max);
                if v.default == clamped {
                    return Ok(EditResult::unchanged());
                }
                v.default = clamped;
                EditResult::changed()
            }
            Edit::SetVariableRange { name, min, max } => {
                let v = variable_mut(graph, &name)?;
                if !v.has_range() {
                    return Err(format!("{} variables have no range", v.ty.name()));
                }
                let min = min.convert(v.ty).filter(Value::is_finite).ok_or("Wrong value type")?;
                let max = max.convert(v.ty).filter(Value::is_finite).ok_or("Wrong value type")?;
                if v.min == min && v.max == max {
                    return Ok(EditResult::unchanged());
                }
                v.min = min;
                v.max = max;
                v.value = v.value.clamped(&min, &max);
                v.default = v.default.clamped(&min, &max);
                EditResult::changed()
            }
            Edit::SetVariableDescription { name, text } => {
                let v = variable_mut(graph, &name)?;
                if v.description == text {
                    return Ok(EditResult::unchanged());
                }
                v.description = text;
                EditResult::changed()
            }
            Edit::ResetVariable(name) => {
                let v = variable_mut(graph, &name)?;
                if v.value == v.default {
                    return Ok(EditResult::unchanged());
                }
                v.value = v.default;
                EditResult::changed()
            }
            Edit::RenameVariable { from, to } => {
                let new = graph.rename_variable(&from, &to)?;
                if new == from { EditResult::unchanged() } else { EditResult::changed() }
            }
            Edit::BindVariable { node, name } => {
                let old = graph.node(node).and_then(|n| n.variable.clone());
                let new = graph.bind_variable(node, &name)?;
                if old.as_deref() == Some(new.as_str()) { EditResult::unchanged() } else { EditResult::changed() }
            }
            Edit::AddComment { text, pos, size } => {
                if !pos.iter().chain(size.iter()).all(|v| v.is_finite()) {
                    return Err("A comment needs a position and a size".into());
                }
                graph.add_comment(text, pos, size);
                EditResult::changed()
            }
            Edit::RemoveComments(ids) => {
                let before = graph.comments.len();
                graph.comments.retain(|c| !ids.contains(&c.id));
                if graph.comments.len() == before {
                    return Ok(EditResult::unchanged());
                }
                EditResult::changed()
            }
            Edit::MoveComment { id, delta, nodes } => {
                if !delta.iter().all(|d| d.is_finite()) || delta == [0.0, 0.0] {
                    return Ok(EditResult::unchanged());
                }
                let comment = graph.comment_mut(id).ok_or("The comment no longer exists")?;
                comment.pos = [coordinate(comment.pos[0] + delta[0]), coordinate(comment.pos[1] + delta[1])];
                for n in graph.nodes.iter_mut().filter(|n| nodes.contains(&n.id)) {
                    n.pos = [coordinate(n.pos[0] + delta[0]), coordinate(n.pos[1] + delta[1])];
                }
                EditResult::changed()
            }
            Edit::ResizeComment { id, pos, size } => {
                if !pos.iter().chain(size.iter()).all(|v| v.is_finite()) {
                    return Err("A comment needs a position and a size".into());
                }
                let comment = graph.comment_mut(id).ok_or("The comment no longer exists")?;
                let before = (comment.pos, comment.size);
                (comment.pos, comment.size) = (pos, size);
                comment.normalize();
                if (comment.pos, comment.size) == before {
                    return Ok(EditResult::unchanged());
                }
                EditResult::changed()
            }
            Edit::SetCommentText { id, text } => {
                let comment = graph.comment_mut(id).ok_or("The comment no longer exists")?;
                if comment.text == text {
                    return Ok(EditResult::unchanged());
                }
                comment.text = text;
                EditResult::changed()
            }
            Edit::SetCommentColor { id, color } => {
                if !color.iter().all(|c| c.is_finite()) {
                    return Err("That is not a color".into());
                }
                let color = color.map(|c| c.clamp(0.0, 1.0));
                let comment = graph.comment_mut(id).ok_or("The comment no longer exists")?;
                if comment.color == color {
                    return Ok(EditResult::unchanged());
                }
                comment.color = color;
                EditResult::changed()
            }
            Edit::Batch(edits) => {
                // On a copy, so that a failing edit leaves the graph as it was.
                let mut scratch = graph.clone();
                let mut result = EditResult::unchanged();
                for edit in edits {
                    let r = edit.apply(&mut scratch)?;
                    result.changed |= r.changed;
                    result.created.extend(r.created);
                    result.notice = r.notice.or(result.notice);
                }
                if result.changed {
                    *graph = scratch;
                }
                result
            }
        };
        if result.changed {
            graph.prune_links();
        }
        Ok(result)
    }
}

/// Convenience for building edits of gradients (used by the gradient editor).
pub fn gradient_edit(node: NodeId, key: &str, gradient: Gradient) -> Edit {
    Edit::SetParam { node, key: key.to_string(), value: ParamValue::Gradient(gradient.normalized()) }
}

/// Is `kind` an Input node type?
pub fn is_input_kind(kind: NodeKind) -> bool {
    matches!(kind.def().role, Role::Variable(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposed_parameters_are_locked() {
        let mut g = Graph::default();
        let b = g.add_node(NodeKind::BLUR, [0.0, 0.0]);
        Edit::SetExposed { node: b, key: "radius".into(), exposed: true }.apply(&mut g).unwrap();
        let err = Edit::SetParam { node: b, key: "radius".into(), value: ParamValue::Float(3.0) }.apply(&mut g);
        assert!(err.unwrap_err().contains("exposed"));
        assert!(Edit::SetExposed { node: b, key: "tiling".into(), exposed: true }.apply(&mut g).is_err());
    }

    #[test]
    fn set_param_validates_and_detects_no_ops() {
        let mut g = Graph::default();
        let b = g.add_node(NodeKind::BLUR, [0.0, 0.0]);
        let r = Edit::SetParam { node: b, key: "radius".into(), value: ParamValue::Float(8.0) }.apply(&mut g).unwrap();
        assert!(!r.changed);
        assert!(Edit::SetParam { node: b, key: "radius".into(), value: ParamValue::Int(3) }.apply(&mut g).is_err());
        assert!(Edit::SetParam { node: b, key: "nope".into(), value: ParamValue::Float(3.0) }.apply(&mut g).is_err());
        Edit::SetParam { node: b, key: "tiling".into(), value: ParamValue::Enum(0) }.apply(&mut g).unwrap();
        let r = Edit::SetParam { node: b, key: "tiling".into(), value: ParamValue::Enum(99) }.apply(&mut g).unwrap();
        assert!(r.changed);
        assert_eq!(g.node(b).unwrap().enum_value("tiling"), 3, "enum clamped into range");
    }

    #[test]
    fn batches_are_all_or_nothing() {
        let mut g = Graph::default();
        let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
        let blur = g.add_node(NodeKind::BLUR, [0.0, 0.0]);
        let invert = g.add_node(NodeKind::INVERT, [0.0, 0.0]);
        g.connect(noise, "o:output", blur, "i:input").unwrap();
        // Re-route: the link moves from the Blur to the Invert in one edit.
        let old = g.links[0].clone();
        let reroute = Edit::Batch(vec![
            Edit::RemoveLink(old.clone()),
            Edit::Connect {
                a: SocketRef { node: noise, key: "o:output".into(), output: true },
                b: SocketRef { node: invert, key: "i:input".into(), output: false },
            },
        ]);
        assert_eq!(reroute.label(), "Connect");
        assert!(reroute.apply(&mut g).unwrap().changed);
        assert!(g.link_into(blur, "i:input").is_none());
        assert!(g.link_into(invert, "i:input").is_some());
        // A failing edit undoes the ones before it.
        let before = g.links.clone();
        let bad = Edit::Batch(vec![
            Edit::RemoveLink(g.links[0].clone()),
            Edit::SetParam { node: blur, key: "nope".into(), value: ParamValue::Float(1.0) },
        ]);
        assert!(bad.apply(&mut g).is_err());
        assert_eq!(g.links, before);
    }

    #[test]
    fn add_node_with_parameters() {
        let mut g = Graph::default();
        let path = ParamValue::Text("bricks.png".into());
        let r = Edit::AddNodeWith { kind: NodeKind::BITMAP, pos: [0.0, 0.0], params: vec![("path".into(), path)] }.apply(&mut g).unwrap();
        assert_eq!(g.node(r.created[0]).unwrap().text("path"), "bricks.png");
        // A bad parameter adds nothing.
        let bad = Edit::AddNodeWith { kind: NodeKind::BITMAP, pos: [0.0, 0.0], params: vec![("path".into(), ParamValue::Float(1.0))] };
        assert!(bad.apply(&mut g).is_err());
        assert_eq!(g.nodes.len(), 1);
    }

    #[test]
    fn add_connected_node() {
        let mut g = Graph::default();
        let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
        let out = SocketRef { node: noise, key: "o:output".into(), output: true };
        assert!(accepts_link(NodeKind::BLUR, DataType::Image, true));
        assert!(!accepts_link(NodeKind::MATH, DataType::Image, true));
        assert!(accepts_link(NodeKind::PIXEL_MATH, DataType::Image, true));
        let r = Edit::AddNodeConnected { kind: NodeKind::PIXEL_MATH, pos: [0.0, 0.0], socket: out.clone() }
            .apply(&mut g)
            .unwrap();
        let pm = r.created[0];
        assert!(g.link_into(pm, "v:a").is_some_and(|l| l.from == noise));
        assert!(Edit::AddNodeConnected { kind: NodeKind::MATH, pos: [0.0, 0.0], socket: out }.apply(&mut g).is_err());
        // Dragging backwards from an input creates a source.
        let blur = g.add_node(NodeKind::BLUR, [0.0, 0.0]);
        g.set_exposed(blur, "radius", true).unwrap();
        let inp = SocketRef { node: blur, key: "p:radius".into(), output: false };
        let r =
            Edit::AddNodeConnected { kind: NodeKind::FLOAT_INPUT, pos: [0.0, 0.0], socket: inp }.apply(&mut g).unwrap();
        assert!(g.link_into(blur, "p:radius").is_some_and(|l| l.from == r.created[0]));
    }

    #[test]
    fn variable_edits_respect_the_range() {
        let mut g = Graph::default();
        g.add_node(NodeKind::FLOAT_INPUT, [0.0, 0.0]);
        Edit::SetVariable { name: "value".into(), value: Value::Float(7.0) }.apply(&mut g).unwrap();
        assert_eq!(g.variable("value").unwrap().value, Value::Float(1.0));
        Edit::SetVariableRange { name: "value".into(), min: Value::Float(0.0), max: Value::Float(10.0) }
            .apply(&mut g)
            .unwrap();
        Edit::SetVariable { name: "value".into(), value: Value::Int(7) }.apply(&mut g).unwrap();
        assert_eq!(g.variable("value").unwrap().value, Value::Float(7.0));
        Edit::SetVariableDefault { name: "value".into(), value: Value::Float(7.0) }.apply(&mut g).unwrap();
        Edit::SetVariable { name: "value".into(), value: Value::Float(2.0) }.apply(&mut g).unwrap();
        Edit::ResetVariable("value".into()).apply(&mut g).unwrap();
        assert_eq!(g.variable("value").unwrap().value, Value::Float(7.0));
        assert!(Edit::SetVariable { name: "nope".into(), value: Value::Float(1.0) }.apply(&mut g).is_err());
    }
}
