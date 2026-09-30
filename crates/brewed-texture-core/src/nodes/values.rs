//! Inputs & values: graph variables, Math and composition of vectors / colors.

use super::*;

pub const DEFAULT_MATH_EXPRESSION: &str = "(1+x)*sin(myVar)-2.4/cos(myVar2)";

const fn input_node(
    id: &'static str,
    name: &'static str,
    ty: DataType,
    outputs: &'static [SocketDef],
    description: &'static str,
) -> NodeDef {
    NodeDef {
        id,
        name,
        category: Category::Values,
        description,
        keywords: "input parameter variable exposed graph",
        inputs: NO_SOCKETS,
        outputs,
        params: &[],
        role: Role::Variable(ty),
    }
}

const FLOAT_OUT: &[SocketDef] = &[value("value", "Value", DataType::Float)];
const INT_OUT: &[SocketDef] = &[value("value", "Value", DataType::Int)];
const BOOL_OUT: &[SocketDef] = &[value("value", "Value", DataType::Bool)];
const VEC2_OUT: &[SocketDef] = &[value("value", "Value", DataType::Vec2)];
const COLOR_OUT: &[SocketDef] = &[value("value", "Value", DataType::Color)];

pub static FLOAT_INPUT: NodeDef = input_node(
    "float_input",
    "Float Input",
    DataType::Float,
    FLOAT_OUT,
    "A graph variable (shown in Input Values). Input nodes with the same name share one value.",
);
pub static INT_INPUT: NodeDef = input_node(
    "int_input",
    "Integer Input",
    DataType::Int,
    INT_OUT,
    "An integer graph variable (shown in Input Values). Input nodes with the same name share one value.",
);
pub static BOOL_INPUT: NodeDef = input_node(
    "bool_input",
    "Boolean Input",
    DataType::Bool,
    BOOL_OUT,
    "A boolean graph variable (shown in Input Values). Input nodes with the same name share one value.",
);
pub static VEC2_INPUT: NodeDef = input_node(
    "vec2_input",
    "Vec2 Input",
    DataType::Vec2,
    VEC2_OUT,
    "A 2D vector graph variable (shown in Input Values). Input nodes with the same name share one value.",
);
pub static COLOR_INPUT: NodeDef = input_node(
    "color_input",
    "Color Input",
    DataType::Color,
    COLOR_OUT,
    "A color graph variable (shown in Input Values). Input nodes with the same name share one value.",
);

pub static MATH: NodeDef = NodeDef {
    id: "math",
    name: "Math",
    category: Category::Values,
    description: "Evaluates a scalar expression. Every word that is not a function or a constant \
                  becomes an input on the left.",
    keywords: "expression formula calculate function scalar",
    inputs: NO_SOCKETS,
    outputs: &[value("result", "Result", DataType::Float)],
    params: &[
        expression("expression", "Expression", DEFAULT_MATH_EXPRESSION, false),
        boolean("auto_bind", "Auto-bind", true)
            .fixed()
            .tip("Unconnected variables named like a graph variable take that variable's value."),
    ],
    role: Role::Value,
};

pub static COMPOSE_VEC2: NodeDef = NodeDef {
    id: "compose_vec2",
    name: "Compose Vec2",
    category: Category::Values,
    description: "Builds a 2D vector from two numbers.",
    keywords: "vector combine make",
    inputs: NO_SOCKETS,
    outputs: &[value("value", "Vec2", DataType::Vec2)],
    params: &[float("x", "X", 0.0, -1.0, 1.0).exposed(), float("y", "Y", 0.0, -1.0, 1.0).exposed()],
    role: Role::Value,
};

pub static COMPOSE_COLOR: NodeDef = NodeDef {
    id: "compose_color",
    name: "Compose Color",
    category: Category::Values,
    description: "Builds an RGBA color from four numbers.",
    keywords: "rgba combine make",
    inputs: NO_SOCKETS,
    outputs: &[value("value", "Color", DataType::Color)],
    params: &[
        float("r", "Red", 0.5, 0.0, 1.0).exposed(),
        float("g", "Green", 0.5, 0.0, 1.0).exposed(),
        float("b", "Blue", 0.5, 0.0, 1.0).exposed(),
        float("a", "Alpha", 1.0, 0.0, 1.0),
    ],
    role: Role::Value,
};

pub static IMAGE_INPUT: NodeDef = NodeDef {
    id: "image_input",
    name: "Image Input",
    category: Category::Values,
    description: "An image the graph takes, shown in Input Values; Image Inputs of one name share it. Where the \
                  graph is used as a node, it is a parameter of that node: a picture, or, exposed, a socket.",
    keywords: "input image graph instance socket parameter picture",
    inputs: NO_SOCKETS,
    outputs: IMAGE_OUT,
    params: &[],
    role: Role::Image,
};

/// A graph used as a node. Its sockets and parameters are not those of a node type but those
/// of the graph (see [`crate::instance`]).
pub static GRAPH_INSTANCE: NodeDef = NodeDef {
    id: "graph_instance",
    name: "Graph",
    category: Category::Graphs,
    description: "A graph used as a node: its outputs and its input values (its image inputs among them) \
                  are the node's. Changed where it is, it changes everywhere it is used.",
    keywords: "instance graph package reference",
    inputs: NO_SOCKETS,
    outputs: NO_SOCKETS,
    params: &[],
    role: Role::Instance,
};
