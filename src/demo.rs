//! The graph Ferrotex opens with: a procedural brick wall that exercises inputs,
//! Math, exposed parameters, Transform 2D, Blend, Levels, Blur and the outputs.

use crate::graph::{DataType, Graph, NodeId, NodeKind, ParamValue, Value};

fn set(g: &mut Graph, id: NodeId, key: &str, value: ParamValue) {
    if let Some(slot) = g.node_mut(id).and_then(|n| n.param_mut(key)) {
        slot.value = value;
    }
}

fn link(g: &mut Graph, from: NodeId, out: &str, to: NodeId, input: &str) {
    if let Err(e) = g.connect(from, out, to, input) {
        log::error!("demo graph: {e}");
    }
}

/// Returns the graph and the node to preview first.
pub fn brick_wall() -> (Graph, NodeId) {
    let mut g = Graph::default();

    // Graph inputs.
    let bricks_x = g.add_input(DataType::Int, "bricks_x");
    let bricks_y = g.add_input(DataType::Int, "bricks_y");
    let mortar = g.add_input(DataType::Float, "mortar");
    let wear = g.add_input(DataType::Float, "wear");
    let angle = g.add_input(DataType::Float, "angle");
    let tint = g.add_input(DataType::Color, "tint");
    let setup = |g: &mut Graph, id, value: Value, min: Value, max: Value, desc: &str| {
        if let Some(i) = g.input_mut(id) {
            i.value = value;
            i.default = value;
            i.min = min;
            i.max = max;
            i.description = desc.to_string();
        }
    };
    setup(&mut g, bricks_x, Value::Int(4), Value::Int(1), Value::Int(16), "Bricks per row");
    setup(&mut g, bricks_y, Value::Int(8), Value::Int(1), Value::Int(32), "Number of rows");
    setup(&mut g, mortar, Value::Float(0.06), Value::Float(0.0), Value::Float(0.25), "Gap between bricks");
    setup(&mut g, wear, Value::Float(0.55), Value::Float(0.0), Value::Float(1.0), "How much dirt is blended in");
    setup(&mut g, angle, Value::Float(30.0), Value::Float(-180.0), Value::Float(180.0), "Rotation of the dirt layer");
    let brick = Value::Color([0.62, 0.26, 0.16, 1.0]);
    setup(&mut g, tint, brick, Value::Color([0.0; 4]), Value::Color([1.0; 4]), "Brick color");

    // Column 0: input nodes.
    let in_x = g.add_input_node(bricks_x, [-560.0, -40.0]);
    let in_y = g.add_input_node(bricks_y, [-560.0, 40.0]);
    let in_mortar = g.add_input_node(mortar, [-560.0, 140.0]);
    let in_wear = g.add_input_node(wear, [-560.0, 380.0]);
    let in_angle = g.add_input_node(angle, [-560.0, 560.0]);
    let in_tint = g.add_input_node(tint, [720.0, 470.0]);

    // Mortar gap -> tile size (float broadcasts into the Vec2 size parameter).
    let size_math = g.add_node(NodeKind::Math, [-330.0, 140.0]);
    set(&mut g, size_math, "expression", ParamValue::Text("1 - 2*mortar".into()));
    link(&mut g, in_mortar, "o:value", size_math, "v:mortar");

    // Bricks.
    let tiles = g.add_node(NodeKind::TileGenerator, [-100.0, -60.0]);
    set(&mut g, tiles, "pattern", ParamValue::Enum(2)); // rounded square
    set(&mut g, tiles, "row_offset", ParamValue::Float(0.5));
    set(&mut g, tiles, "corner_radius", ParamValue::Float(0.15));
    set(&mut g, tiles, "softness", ParamValue::Float(0.12));
    set(&mut g, tiles, "random_luminance", ParamValue::Float(0.35));
    set(&mut g, tiles, "seed", ParamValue::Int(7));
    for key in ["x_amount", "y_amount", "size"] {
        g.set_exposed(tiles, key, true);
    }
    link(&mut g, in_x, "o:value", tiles, "p:x_amount");
    link(&mut g, in_y, "o:value", tiles, "p:y_amount");
    link(&mut g, size_math, "o:result", tiles, "p:size");

    // Dirt: rotated noise.
    let noise = g.add_node(NodeKind::Noise, [-330.0, 420.0]);
    set(&mut g, noise, "scale", ParamValue::Int(6));
    set(&mut g, noise, "octaves", ParamValue::Int(7));
    set(&mut g, noise, "roughness", ParamValue::Float(0.55));
    let rotate_math = g.add_node(NodeKind::Math, [-330.0, 640.0]);
    set(&mut g, rotate_math, "expression", ParamValue::Text("angle + 5*sin(angle/10)".into()));
    link(&mut g, in_angle, "o:value", rotate_math, "v:angle");
    let transform = g.add_node(NodeKind::Transform2D, [-100.0, 420.0]);
    set(&mut g, transform, "scale", ParamValue::Vec2([1.0, 1.0]));
    g.set_exposed(transform, "rotation", true);
    link(&mut g, noise, "o:output", transform, "i:input");
    link(&mut g, rotate_math, "o:result", transform, "p:rotation");

    // Height = bricks multiplied with dirt, then levels + a touch of blur.
    let blend = g.add_node(NodeKind::Blend, [130.0, 120.0]);
    set(&mut g, blend, "mode", ParamValue::Enum(3)); // multiply
    g.set_exposed(blend, "opacity", true);
    link(&mut g, transform, "o:output", blend, "i:foreground");
    link(&mut g, tiles, "o:output", blend, "i:background");
    link(&mut g, in_wear, "o:value", blend, "p:opacity");

    let levels = g.add_node(NodeKind::Levels, [360.0, 120.0]);
    set(&mut g, levels, "in_low", ParamValue::Float(0.04));
    set(&mut g, levels, "in_high", ParamValue::Float(0.85));
    set(&mut g, levels, "gamma", ParamValue::Float(0.9));
    link(&mut g, blend, "o:output", levels, "i:input");

    let blur = g.add_node(NodeKind::Blur, [590.0, 120.0]);
    set(&mut g, blur, "radius", ParamValue::Float(1.5));
    link(&mut g, levels, "o:output", blur, "i:input");

    // Base color via a gradient map whose middle color is the tint input.
    let gmap = g.add_node(NodeKind::GradientMap, [940.0, 290.0]);
    set(&mut g, gmap, "stops", ParamValue::Enum(2));
    set(&mut g, gmap, "color_a", ParamValue::Color([0.12, 0.11, 0.1, 1.0]));
    set(&mut g, gmap, "pos_b", ParamValue::Float(0.35));
    set(&mut g, gmap, "color_c", ParamValue::Color([0.78, 0.45, 0.3, 1.0]));
    set(&mut g, gmap, "pos_c", ParamValue::Float(0.8));
    set(&mut g, gmap, "color_d", ParamValue::Color([0.92, 0.72, 0.55, 1.0]));
    g.set_exposed(gmap, "color_b", true);
    link(&mut g, blur, "o:output", gmap, "i:input");
    link(&mut g, in_tint, "o:value", gmap, "p:color_b");

    let normal = g.add_node(NodeKind::NormalMap, [940.0, -80.0]);
    set(&mut g, normal, "intensity", ParamValue::Float(6.0));
    link(&mut g, blur, "o:output", normal, "i:input");

    // Outputs.
    let out_color = g.add_node(NodeKind::Output, [1200.0, 290.0]);
    set(&mut g, out_color, "name", ParamValue::Text("basecolor".into()));
    set(&mut g, out_color, "format", ParamValue::Enum(1)); // RGB
    link(&mut g, gmap, "o:output", out_color, "i:input");

    let out_normal = g.add_node(NodeKind::Output, [1200.0, -80.0]);
    set(&mut g, out_normal, "name", ParamValue::Text("normal".into()));
    set(&mut g, out_normal, "format", ParamValue::Enum(1));
    link(&mut g, normal, "o:output", out_normal, "i:input");

    let out_height = g.add_node(NodeKind::Output, [940.0, 640.0]);
    set(&mut g, out_height, "name", ParamValue::Text("height".into()));
    set(&mut g, out_height, "format", ParamValue::Enum(3)); // R
    set(&mut g, out_height, "bit_depth", ParamValue::Enum(1));
    link(&mut g, blur, "o:output", out_height, "i:input");

    (g, out_color)
}

#[cfg(test)]
mod tests {
    #[test]
    fn demo_graph_is_valid() {
        let (g, preview) = super::brick_wall();
        assert!(g.node(preview).is_some());
        let (_, cyclic) = g.topo_order();
        assert!(cyclic.is_empty());
        assert!(g.links.len() >= 20, "links: {}", g.links.len());
    }
}
