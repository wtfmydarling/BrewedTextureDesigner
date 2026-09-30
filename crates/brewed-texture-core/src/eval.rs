//! CPU evaluation of a graph: value nodes (inputs, Math, Compose …), the effective
//! parameters of every node (exposed parameters replaced by the values driving them)
//! and the messages that can be derived without a GPU.
//!
//! This is cheap (no pixels involved), so front ends run it synchronously after every
//! edit and the engine receives its result together with the graph.

use std::collections::{BTreeMap, HashMap};

use crate::curve::Curve;
use crate::expr;
use crate::gradient::Gradient;
use crate::graph::{Graph, Node, NodeId, param_socket_key, var_socket_key};
use crate::nodes::{NodeKind, ParamKind, Role};
use crate::types::{DataType, ParamValue, Value};

/// Effective parameter values of a node: stored values with driven overrides.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Params {
    pub values: Vec<(String, ParamValue)>,
}

fn finite(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

impl Params {
    pub fn get(&self, key: &str) -> Option<&ParamValue> {
        self.values.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    pub fn f(&self, key: &str) -> f32 {
        finite(self.get(key).map_or(0.0, ParamValue::as_f32))
    }
    pub fn i(&self, key: &str) -> i32 {
        match self.get(key) {
            Some(ParamValue::Int(v)) => *v,
            Some(other) => other.as_f32() as i32,
            None => 0,
        }
    }
    pub fn u(&self, key: &str) -> u32 {
        match self.get(key) {
            Some(ParamValue::Enum(v)) => *v,
            Some(ParamValue::Bool(b)) => u32::from(*b),
            Some(other) => other.as_f32().max(0.0) as u32,
            None => 0,
        }
    }
    pub fn b(&self, key: &str) -> bool {
        matches!(self.get(key), Some(ParamValue::Bool(true)))
    }
    pub fn v2(&self, key: &str) -> [f32; 2] {
        match self.get(key) {
            Some(ParamValue::Vec2(v)) => v.map(finite),
            _ => [0.0; 2],
        }
    }
    pub fn color(&self, key: &str) -> [f32; 4] {
        match self.get(key) {
            Some(ParamValue::Color(v)) => v.map(finite),
            _ => [0.0, 0.0, 0.0, 1.0],
        }
    }
    pub fn text(&self, key: &str) -> &str {
        match self.get(key) {
            Some(ParamValue::Text(s)) => s,
            _ => "",
        }
    }
    pub fn gradient(&self, key: &str) -> Gradient {
        match self.get(key) {
            Some(ParamValue::Gradient(g)) => g.clone(),
            _ => Gradient::default(),
        }
    }
    pub fn curve(&self, key: &str) -> Curve {
        match self.get(key) {
            Some(ParamValue::Curve(c)) => c.clone(),
            _ => Curve::default(),
        }
    }
    /// Tiling bits (see [`crate::Tiling`]).
    pub fn tiling(&self) -> u32 {
        self.u("tiling").min(3)
    }
    pub fn hash_into<H: std::hash::Hasher>(&self, h: &mut H) {
        use std::hash::Hash;
        for (k, v) in &self.values {
            k.hash(h);
            v.hash_into(h);
        }
    }
}

/// Where an expression variable gets its value from.
#[derive(Clone, Debug, PartialEq)]
pub enum VarSource {
    /// A link from a value node.
    Link(NodeId),
    /// A graph variable with the same name (auto-bind).
    Variable(String),
    /// The node's own value for the variable.
    Local,
    /// A per-pixel built-in of Pixel Math (u, v, px …).
    Builtin,
    /// A link from an image (Pixel Math).
    Image(NodeId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct VarInfo {
    pub name: String,
    pub value: f64,
    pub source: VarSource,
}

/// Evaluation result of one node.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeEval {
    /// Output of value nodes.
    pub value: Option<Value>,
    pub params: Params,
    /// Exposed parameters and the value driving them.
    pub driven: BTreeMap<String, ParamValue>,
    /// Exposed parameters without a (usable) link: they keep their stored value.
    pub unconnected: Vec<String>,
    /// Variables of Math / Pixel Math expressions.
    pub vars: Vec<VarInfo>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl NodeEval {
    pub fn error(&self) -> Option<&str> {
        self.errors.first().map(String::as_str)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Evaluation {
    pub nodes: HashMap<NodeId, NodeEval>,
    /// Topological order of all nodes that are not part of a cycle.
    pub order: Vec<NodeId>,
    pub cyclic: Vec<NodeId>,
}

impl Evaluation {
    pub fn node(&self, id: NodeId) -> Option<&NodeEval> {
        self.nodes.get(&id)
    }

    pub fn value(&self, id: NodeId) -> Option<Value> {
        self.nodes.get(&id).and_then(|n| n.value)
    }

    pub fn params(&self, id: NodeId) -> Option<&Params> {
        self.nodes.get(&id).map(|n| &n.params)
    }
}

/// Evaluate every value node and the effective parameters of every node.
pub fn evaluate(graph: &Graph) -> Evaluation {
    let (order, cyclic) = graph.topo_order();
    let mut out = Evaluation { nodes: HashMap::with_capacity(graph.nodes.len()), order, cyclic };
    for &id in &out.cyclic {
        let node = graph.node(id).expect("cyclic ids come from the graph");
        let mut e = NodeEval { params: stored_params(node), ..NodeEval::default() };
        e.errors.push("The node is part of a cycle".into());
        out.nodes.insert(id, e);
    }
    let order = out.order.clone();
    for id in order {
        let Some(node) = graph.node(id) else { continue };
        let e = evaluate_node(graph, node, &out);
        out.nodes.insert(id, e);
    }
    out
}

fn stored_params(node: &Node) -> Params {
    Params { values: node.params.iter().map(|s| (s.key.clone(), s.value.clone())).collect() }
}

/// Value arriving at `socket` of `node`, if something is connected. The inner `None`
/// means the source has no value (for example because of an error upstream).
fn linked_value(graph: &Graph, results: &Evaluation, node: NodeId, socket: &str) -> Option<(NodeId, Option<Value>)> {
    let link = graph.link_into(node, socket)?;
    Some((link.from, results.value(link.from)))
}

fn evaluate_node(graph: &Graph, node: &Node, results: &Evaluation) -> NodeEval {
    let d = node.def();
    let mut e = NodeEval::default();

    // Effective parameters.
    let mut values = Vec::with_capacity(node.params.len());
    for pd in node.param_defs() {
        let Some(slot) = node.param(pd.key) else { continue };
        let mut value = slot.value.clone();
        // What arrives at the socket of an image input is an image: laid out, it is what the
        // Image Inputs of the graph show (see `crate::package::flatten`).
        let image = matches!(pd.kind, ParamKind::Image { .. });
        if slot.exposed && pd.can_expose() && node.param_visible(&pd) && image {
            if graph.link_into(node.id, &param_socket_key(pd.key)).is_none() {
                e.unconnected.push(pd.key.to_string());
                e.warnings.push(format!("'{}' is exposed but not connected; using its own value", pd.label));
            }
        } else if slot.exposed && pd.can_expose() && node.param_visible(&pd) {
            match linked_value(graph, results, node.id, &param_socket_key(pd.key)) {
                Some((_, Some(v))) => match slot.value.from_driven(&v, pd.enum_options().len()) {
                    Some(pv) => {
                        e.driven.insert(pd.key.to_string(), pv.clone());
                        value = pv;
                    }
                    None => {
                        e.unconnected.push(pd.key.to_string());
                        e.warnings.push(format!("'{}' receives an unusable value ({})", pd.label, v.display()));
                    }
                },
                Some((from, None)) => {
                    e.unconnected.push(pd.key.to_string());
                    let src = graph.node(from).map_or("?".to_string(), |n| n.def().name.to_string());
                    e.warnings.push(format!("'{}' gets no value from {src}; using its own value", pd.label));
                }
                None => {
                    e.unconnected.push(pd.key.to_string());
                    e.warnings.push(format!("'{}' is exposed but not connected; using its own value", pd.label));
                }
            }
        }
        values.push((pd.key.to_string(), value));
    }
    e.params = Params { values };

    // Unconnected required inputs.
    for s in d.inputs.iter().filter(|s| !s.optional) {
        if graph.link_into(node.id, &crate::graph::input_socket_key(s.key)).is_none() {
            e.warnings.push(format!("Input '{}' is not connected", s.label));
        }
    }
    // Outputs of one name: the graph used as a node has one output of it (the upper one's).
    if node.kind == NodeKind::OUTPUT {
        let name = node.text("name");
        if graph.nodes_of(NodeKind::OUTPUT).any(|o| o.id != node.id && o.text("name") == name) {
            e.warnings.push(format!("Another Output is named '{name}': give each output a name of its own"));
        }
    }

    match d.role {
        Role::Variable(ty) => match node.variable.as_deref().and_then(|n| graph.variable(n)) {
            Some(var) if var.ty == ty => e.value = Some(var.value),
            Some(var) => e.errors.push(format!("'{}' is a {} variable", var.name, var.ty.name())),
            None => e.errors.push("The variable this node reads does not exist".into()),
        },
        Role::Value => e.value = evaluate_value(graph, node, results, &mut e),
        Role::Image | Role::Output => {
            if d.is_pixel_expression() {
                pixel_vars(graph, node, results, &mut e);
            }
        }
        // What a graph used as a node makes is laid out in the graph before it is computed
        // (see [`crate::package::flatten`]): here it only has its parameters.
        Role::Instance => {}
    }
    if let Some(v) = e.value
        && !v.is_finite()
    {
        e.warnings.push(format!("The result is not a finite number ({})", v.display()));
    }
    e
}

/// Scalar value of an expression variable that is not linked to an image.
fn scalar_var(graph: &Graph, node: &Node, results: &Evaluation, name: &str) -> (f64, VarSource) {
    if let Some((from, value)) = linked_value(graph, results, node.id, &var_socket_key(name))
        && let Some(v) = value.and_then(|v| v.as_f64())
    {
        return (v, VarSource::Link(from));
    }
    if node.bool_value("auto_bind")
        && let Some(var) = graph.variable(name).filter(|v| v.ty != DataType::Image)
        && let Some(v) = var.value.as_f64()
    {
        return (v, VarSource::Variable(var.name.clone()));
    }
    (node.var_values.get(name).copied().unwrap_or(0.0) as f64, VarSource::Local)
}

fn evaluate_value(graph: &Graph, node: &Node, results: &Evaluation, e: &mut NodeEval) -> Option<Value> {
    let p = &e.params;
    match node.kind {
        k if k == crate::NodeKind::MATH => {
            let compiled = expr::compile_cached(node.expression());
            match &*compiled {
                Ok(program) => {
                    let mut args = Vec::with_capacity(program.vars.len());
                    for name in &program.vars {
                        let (v, source) = scalar_var(graph, node, results, name);
                        args.push(v);
                        e.vars.push(VarInfo { name: name.clone(), value: v, source });
                    }
                    if program.components() > 1 {
                        e.errors.push("Math gives one number: a vector, or a list of values, makes a color in Pixel Math".into());
                        return None;
                    }
                    Some(Value::Float(program.eval(&args) as f32))
                }
                Err(err) => {
                    for name in node.expression_vars() {
                        let (v, source) = scalar_var(graph, node, results, &name);
                        e.vars.push(VarInfo { name, value: v, source });
                    }
                    e.errors.push(format!("The expression is incorrect: {}", err.message));
                    None
                }
            }
        }
        k if k == crate::NodeKind::COMPOSE_VEC2 => Some(Value::Vec2([p.f("x"), p.f("y")])),
        k if k == crate::NodeKind::COMPOSE_COLOR => Some(Value::Color([p.f("r"), p.f("g"), p.f("b"), p.f("a")])),
        _ => {
            e.errors.push(format!("No evaluator for value node '{}'", node.def().name));
            None
        }
    }
}

/// Classify the variables of a Pixel Math expression or an FX-Map program.
fn pixel_vars(graph: &Graph, node: &Node, results: &Evaluation, e: &mut NodeEval) {
    let language = node.def().expression_language().unwrap_or(expr::Language::Pixel);
    let names = match language.reads(node.expression()) {
        Ok(vars) => vars,
        Err(message) => {
            e.errors.push(message);
            node.expression_vars()
        }
    };
    for name in names {
        if language.builtins().contains(&name.as_str()) {
            e.vars.push(VarInfo { name, value: 0.0, source: VarSource::Builtin });
            continue;
        }
        let image_link = graph
            .link_into(node.id, &var_socket_key(&name))
            .filter(|l| graph.output_socket(l.from, &l.from_socket).is_some_and(|s| s.ty == DataType::Image));
        if let Some(link) = image_link {
            e.vars.push(VarInfo { name, value: 0.0, source: VarSource::Image(link.from) });
        } else {
            let (value, source) = scalar_var(graph, node, results, &name);
            e.vars.push(VarInfo { name, value, source });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeKind;

    fn set(g: &mut Graph, id: NodeId, key: &str, value: ParamValue) {
        g.node_mut(id).unwrap().param_mut(key).unwrap().value = value;
    }

    #[test]
    fn math_with_links_variables_and_locals() {
        let mut g = Graph::default();
        let m = g.add_node(NodeKind::MATH, [0.0, 0.0]);
        set(&mut g, m, "expression", ParamValue::Text("a * 2 + b + speed".into()));
        let a = g.add_node(NodeKind::FLOAT_INPUT, [0.0, 0.0]);
        let a_name = g.node(a).unwrap().variable.clone().unwrap();
        g.variable_mut(&a_name).unwrap().value = Value::Float(3.0);
        g.connect(a, "o:value", m, "v:a").unwrap();
        // 'speed' auto-binds to a variable of that name.
        let s = g.add_node(NodeKind::FLOAT_INPUT, [0.0, 0.0]);
        g.bind_variable(s, "speed").unwrap();
        g.variable_mut("speed").unwrap().value = Value::Float(0.5);
        g.node_mut(m).unwrap().var_values.insert("b".into(), 10.0);
        let ev = evaluate(&g);
        let r = ev.node(m).unwrap();
        assert_eq!(r.value, Some(Value::Float(16.5)));
        assert_eq!(r.vars[0].source, VarSource::Link(a));
        assert_eq!(r.vars[1].source, VarSource::Local);
        assert_eq!(r.vars[2].source, VarSource::Variable("speed".into()));
        assert!(r.errors.is_empty());
    }

    #[test]
    fn broken_expressions_are_reported_not_fatal() {
        let mut g = Graph::default();
        let m = g.add_node(NodeKind::MATH, [0.0, 0.0]);
        set(&mut g, m, "expression", ParamValue::Text("sin(x".into()));
        let blur = g.add_node(NodeKind::BLUR, [0.0, 0.0]);
        g.set_exposed(blur, "radius", true).unwrap();
        g.connect(m, "o:result", blur, "p:radius").unwrap();
        let ev = evaluate(&g);
        assert!(ev.node(m).unwrap().error().unwrap().contains("incorrect"));
        let b = ev.node(blur).unwrap();
        assert_eq!(b.params.f("radius"), 8.0, "falls back to the stored value");
        assert!(b.unconnected.contains(&"radius".to_string()));
        assert!(!b.warnings.is_empty());
        // A list of values is a color of Pixel Math: Math says it gives one.
        set(&mut g, m, "expression", ParamValue::Text("x, 2".into()));
        let ev = evaluate(&g);
        assert!(ev.node(m).unwrap().error().unwrap().contains("Math gives one number"));
        // Pixel Math: gray for one value, color for a list.
        let pm = g.add_node(NodeKind::PIXEL_MATH, [0.0, 0.0]);
        let kind = |g: &Graph| crate::nodes::image_kind(NodeKind::PIXEL_MATH, &stored_params(g.node(pm).unwrap()), &[]);
        assert_eq!(kind(&g), crate::nodes::ImageKind::Gray);
        set(&mut g, pm, "expression", ParamValue::Text("u, v, 0.5".into()));
        assert_eq!(kind(&g), crate::nodes::ImageKind::Color);
    }

    #[test]
    fn driven_parameters_and_enum_clamping() {
        let mut g = Graph::default();
        let c = g.add_node(NodeKind::INT_INPUT, [0.0, 0.0]);
        let name = g.node(c).unwrap().variable.clone().unwrap();
        g.variable_mut(&name).unwrap().value = Value::Int(99);
        let shape = g.add_node(NodeKind::SHAPE, [0.0, 0.0]);
        g.set_exposed(shape, "shape", true).unwrap();
        g.connect(c, "o:value", shape, "p:shape").unwrap();
        let ev = evaluate(&g);
        let e = ev.node(shape).unwrap();
        assert_eq!(e.driven.get("shape"), Some(&ParamValue::Enum(crate::nodes::SHAPES.len() as u32 - 1)));
        assert!(e.unconnected.is_empty());
    }

    #[test]
    fn exposed_unconnected_uses_stored_value_with_warning() {
        let mut g = Graph::default();
        let b = g.add_node(NodeKind::BLUR, [0.0, 0.0]);
        g.set_exposed(b, "radius", true).unwrap();
        set(&mut g, b, "radius", ParamValue::Float(3.0));
        let ev = evaluate(&g);
        let e = ev.node(b).unwrap();
        assert_eq!(e.params.f("radius"), 3.0);
        assert!(e.warnings.iter().any(|w| w.contains("not connected")));
    }

    #[test]
    fn pixel_math_variables_are_classified() {
        let mut g = Graph::default();
        let pm = g.add_node(NodeKind::PIXEL_MATH, [0.0, 0.0]);
        set(&mut g, pm, "expression", ParamValue::Text("h * u + k".into()));
        let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
        g.connect(noise, "o:output", pm, "v:h").unwrap();
        let ev = evaluate(&g);
        let vars = &ev.node(pm).unwrap().vars;
        assert_eq!(vars[0].source, VarSource::Image(noise));
        assert_eq!(vars[1].source, VarSource::Builtin);
        assert_eq!(vars[2].source, VarSource::Local);
    }

    #[test]
    fn cycles_are_errors() {
        let mut g = Graph::default();
        let a = g.add_node(NodeKind::INVERT, [0.0, 0.0]);
        let b = g.add_node(NodeKind::INVERT, [0.0, 0.0]);
        g.connect(a, "o:output", b, "i:input").unwrap();
        // Bypass the cycle check to simulate a corrupt file.
        g.links.push(crate::Link { from: b, from_socket: "o:output".into(), to: a, to_socket: "i:input".into() });
        let ev = evaluate(&g);
        assert!(ev.node(a).unwrap().error().unwrap().contains("cycle"));
        assert_eq!(ev.cyclic.len(), 2);
    }
}
