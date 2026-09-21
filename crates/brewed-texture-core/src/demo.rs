//! The graph the app opens with: a procedural brick wall that exercises variables, Math,
//! exposed parameters, Transform 2D, Blend, Levels, Blur, Gradient Map, Histogram Scan,
//! Ambient Occlusion, Pixel Math and several outputs for the 3D view.

use crate::gradient::Gradient;
use crate::graph::{Graph, NodeId};
use crate::nodes::NodeKind;
use crate::types::{ParamValue, Value};

fn set(g: &mut Graph, id: NodeId, key: &str, value: ParamValue) {
    if let Some(slot) = g.node_mut(id).and_then(|n| n.param_mut(key)) {
        slot.value = value;
    }
}

fn expose(g: &mut Graph, id: NodeId, key: &str) {
    if let Err(e) = g.set_exposed(id, key, true) {
        log::error!("demo graph: {e}");
    }
}

fn link(g: &mut Graph, from: NodeId, out: &str, to: NodeId, input: &str) {
    if let Err(e) = g.connect(from, out, to, input) {
        log::error!("demo graph: {e}");
    }
}

/// An Input node for a new variable with the given settings; `range` is (value, min, max).
fn variable(
    g: &mut Graph,
    kind: NodeKind,
    name: &str,
    range: (Value, Value, Value),
    desc: &str,
    pos: [f32; 2],
) -> NodeId {
    let (value, min, max) = range;
    let id = g.add_node(kind, pos);
    let name = g.bind_variable(id, name).unwrap_or_else(|_| name.to_string());
    if let Some(v) = g.variable_mut(&name) {
        v.value = value;
        v.default = value;
        v.min = min;
        v.max = max;
        v.description = desc.to_string();
    }
    id
}

fn output(g: &mut Graph, pos: [f32; 2], name: &str, format: u32, usage: u32) -> NodeId {
    let id = g.add_node(NodeKind::OUTPUT, pos);
    set(g, id, "name", ParamValue::Text(name.into()));
    set(g, id, "format", ParamValue::Enum(format));
    set(g, id, "usage", ParamValue::Enum(usage));
    id
}

/// Returns the graph and the node to show in the 2D view first.
pub fn brick_wall() -> (Graph, NodeId) {
    let mut g = Graph::default();
    use Value::{Color, Float, Int};

    // Graph variables (Input Values panel).
    let in_x =
        variable(&mut g, NodeKind::INT_INPUT, "bricks_x", (Int(4), Int(1), Int(16)), "Bricks per row", [-520.0, -60.0]);
    let in_y =
        variable(&mut g, NodeKind::INT_INPUT, "bricks_y", (Int(8), Int(1), Int(32)), "Number of rows", [-520.0, 20.0]);
    let in_mortar = variable(
        &mut g,
        NodeKind::FLOAT_INPUT,
        "mortar",
        (Float(0.06), Float(0.0), Float(0.25)),
        "Gap between bricks",
        [-520.0, 130.0],
    );
    let in_wear = variable(
        &mut g,
        NodeKind::FLOAT_INPUT,
        "wear",
        (Float(0.55), Float(0.0), Float(1.0)),
        "How much dirt is blended in",
        [-520.0, 340.0],
    );
    let in_angle = variable(
        &mut g,
        NodeKind::FLOAT_INPUT,
        "angle",
        (Float(30.0), Float(-180.0), Float(180.0)),
        "Rotation of the dirt layer",
        [-520.0, 540.0],
    );
    let brick = Color([0.62, 0.26, 0.16, 1.0]);
    let in_tint = variable(
        &mut g,
        NodeKind::COLOR_INPUT,
        "tint",
        (brick, Color([0.0; 4]), Color([1.0; 4])),
        "Brick color",
        [560.0, 780.0],
    );

    // Mortar gap -> tile size (a float broadcasts into the Vec2 size parameter).
    let size_math = g.add_node(NodeKind::MATH, [-320.0, 130.0]);
    set(&mut g, size_math, "expression", ParamValue::Text("1 - 2*mortar".into()));
    link(&mut g, in_mortar, "o:value", size_math, "v:mortar");

    // Bricks.
    let tiles = g.add_node(NodeKind::TILE_GENERATOR, [-60.0, -80.0]);
    set(&mut g, tiles, "pattern", ParamValue::Enum(crate::nodes::SHAPE_ROUNDED));
    set(&mut g, tiles, "offset", ParamValue::Float(0.5));
    set(&mut g, tiles, "corner_radius", ParamValue::Float(0.15));
    set(&mut g, tiles, "softness", ParamValue::Float(0.12));
    set(&mut g, tiles, "random_luminance", ParamValue::Float(0.35));
    set(&mut g, tiles, "seed", ParamValue::Int(7));
    for key in ["x_amount", "y_amount", "size"] {
        expose(&mut g, tiles, key);
    }
    link(&mut g, in_x, "o:value", tiles, "p:x_amount");
    link(&mut g, in_y, "o:value", tiles, "p:y_amount");
    link(&mut g, size_math, "o:result", tiles, "p:size");

    // Dirt: rotated noise.
    let noise = g.add_node(NodeKind::NOISE, [-320.0, 360.0]);
    set(&mut g, noise, "scale", ParamValue::Int(6));
    set(&mut g, noise, "octaves", ParamValue::Int(7));
    set(&mut g, noise, "roughness", ParamValue::Float(0.55));
    let rotate_math = g.add_node(NodeKind::MATH, [-320.0, 580.0]);
    set(&mut g, rotate_math, "expression", ParamValue::Text("angle + 5*sin(angle/10)".into()));
    link(&mut g, in_angle, "o:value", rotate_math, "v:angle");
    let transform = g.add_node(NodeKind::TRANSFORM_2D, [-60.0, 360.0]);
    expose(&mut g, transform, "rotation");
    link(&mut g, noise, "o:output", transform, "i:input");
    link(&mut g, rotate_math, "o:result", transform, "p:rotation");

    // Height = bricks multiplied with dirt, then levels and a touch of blur.
    let dirt = g.add_node(NodeKind::BLEND, [160.0, 120.0]);
    set(&mut g, dirt, "mode", ParamValue::Enum(3)); // multiply
    expose(&mut g, dirt, "opacity");
    link(&mut g, transform, "o:output", dirt, "i:foreground");
    link(&mut g, tiles, "o:output", dirt, "i:background");
    link(&mut g, in_wear, "o:value", dirt, "p:opacity");

    let levels = g.add_node(NodeKind::LEVELS, [360.0, 120.0]);
    set(&mut g, levels, "in_low", ParamValue::Float(0.04));
    set(&mut g, levels, "in_high", ParamValue::Float(0.85));
    set(&mut g, levels, "gamma", ParamValue::Float(0.9));
    link(&mut g, dirt, "o:output", levels, "i:input");

    let height = g.add_node(NodeKind::BLUR, [560.0, 120.0]);
    set(&mut g, height, "radius", ParamValue::Float(1.5));
    link(&mut g, levels, "o:output", height, "i:input");

    // Base color: a brightness ramp, tinted where there are bricks (the mortar stays gray).
    let ramp = g.add_node(NodeKind::GRADIENT_MAP, [760.0, 300.0]);
    let gradient = Gradient::from_pairs(&[
        (0.0, [0.26, 0.25, 0.24, 1.0]),
        (0.3, [0.52, 0.5, 0.47, 1.0]),
        (0.55, [0.86, 0.84, 0.8, 1.0]),
        (1.0, [1.0, 0.98, 0.95, 1.0]),
    ]);
    set(&mut g, ramp, "gradient", ParamValue::Gradient(gradient));
    link(&mut g, height, "o:output", ramp, "i:input");

    let mask = g.add_node(NodeKind::HISTOGRAM_SCAN, [760.0, 540.0]);
    set(&mut g, mask, "position", ParamValue::Float(0.3));
    set(&mut g, mask, "contrast", ParamValue::Float(0.85));
    link(&mut g, height, "o:output", mask, "i:input");

    let tint = g.add_node(NodeKind::UNIFORM_COLOR, [760.0, 780.0]);
    expose(&mut g, tint, "color");
    link(&mut g, in_tint, "o:value", tint, "p:color");

    let color = g.add_node(NodeKind::BLEND, [980.0, 300.0]);
    set(&mut g, color, "mode", ParamValue::Enum(3)); // multiply
    link(&mut g, tint, "o:output", color, "i:foreground");
    link(&mut g, ramp, "o:output", color, "i:background");
    link(&mut g, mask, "o:output", color, "i:mask");

    // Normal, ambient occlusion and roughness from the height.
    let normal = g.add_node(NodeKind::NORMAL, [760.0, -120.0]);
    set(&mut g, normal, "intensity", ParamValue::Float(6.0));
    link(&mut g, height, "o:output", normal, "i:input");

    let ao = g.add_node(NodeKind::AMBIENT_OCCLUSION, [980.0, 780.0]);
    set(&mut g, ao, "radius", ParamValue::Float(32.0));
    set(&mut g, ao, "height", ParamValue::Float(0.06));
    link(&mut g, height, "o:output", ao, "i:input");

    let rough = g.add_node(NodeKind::PIXEL_MATH, [980.0, 540.0]);
    set(&mut g, rough, "expression", ParamValue::Text("0.35 + 0.55*(1 - h)^2".into()));
    link(&mut g, height, "o:output", rough, "v:h");

    // Outputs (format: 1 = RGB, 3 = R).
    let out_color = output(&mut g, [1200.0, 300.0], "basecolor", 1, crate::nodes::USAGE_BASE_COLOR);
    link(&mut g, color, "o:output", out_color, "i:input");
    let out_normal = output(&mut g, [1200.0, -120.0], "normal", 1, crate::nodes::USAGE_NORMAL);
    link(&mut g, normal, "o:output", out_normal, "i:input");
    let out_height = output(&mut g, [1200.0, 90.0], "height", 3, crate::nodes::USAGE_HEIGHT);
    set(&mut g, out_height, "bit_depth", ParamValue::Enum(1));
    link(&mut g, height, "o:output", out_height, "i:input");
    let out_rough = output(&mut g, [1200.0, 540.0], "roughness", 3, crate::nodes::USAGE_ROUGHNESS);
    link(&mut g, rough, "o:output", out_rough, "i:input");
    let out_ao = output(&mut g, [1200.0, 780.0], "ambient_occlusion", 3, crate::nodes::USAGE_AO);
    link(&mut g, ao, "o:output", out_ao, "i:input");

    // Two comments, which say what the halves of the graph are for — and show what a
    // comment is.
    let shape = g.add_comment("The shape of the bricks".into(), [-580.0, -180.0], [1250.0, 1000.0]);
    if let Some(c) = g.comment_mut(shape) {
        c.color = [0.28, 0.51, 0.80, 1.0];
    }
    let maps = g.add_comment("Maps for the 3D view".into(), [700.0, -180.0], [680.0, 1180.0]);
    if let Some(c) = g.comment_mut(maps) {
        c.color = [0.29, 0.64, 0.44, 1.0];
    }

    g.normalize();
    (g, out_color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_graph_is_valid() {
        let (g, preview) = brick_wall();
        assert!(g.node(preview).is_some());
        let (_, cyclic) = g.topo_order();
        assert!(cyclic.is_empty());
        for out in g.nodes_of(NodeKind::OUTPUT) {
            assert!(g.link_into(out.id, "i:input").is_some(), "output {} is not connected", out.text("name"));
        }
        assert_eq!(g.variables.len(), 6);
        assert_eq!(g.links.len(), 26);
        let ev = crate::eval::evaluate(&g);
        for n in &g.nodes {
            let e = ev.node(n.id).unwrap();
            assert!(e.errors.is_empty(), "{}: {:?}", n.def().name, e.errors);
            assert!(e.warnings.is_empty(), "{}: {:?}", n.def().name, e.warnings);
        }
        // The size is driven through Math: 1 - 2 * 0.06.
        let tiles = g.nodes_of(NodeKind::TILE_GENERATOR).next().unwrap().id;
        assert_eq!(ev.node(tiles).unwrap().driven.get("size"), Some(&ParamValue::Vec2([0.88, 0.88])));
    }
}
