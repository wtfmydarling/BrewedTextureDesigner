//! Command line helpers that run without a window (used for verification).

use std::path::Path;

use crate::engine::{Engine, read_node};
use crate::gpu::Gpu;
use crate::graph::{Graph, NodeId, NodeKind, ParamValue};

/// Graph with one node of every image kind (fed by a noise and a shape) so all
/// kernels can be looked at side by side.
pub fn gallery_graph() -> (Graph, Vec<(NodeId, String)>) {
    let mut g = Graph { resolution: [512, 512], ..Graph::default() };
    let noise = g.add_node(NodeKind::Noise, [0.0, 0.0]);
    let shape = g.add_node(NodeKind::Shape, [0.0, 0.0]);
    let mut out = vec![(noise, "Noise".to_string()), (shape, "Shape".to_string())];
    for d in crate::graph::defs::DEFS.iter().filter(|d| d.has_image) {
        if matches!(d.kind, NodeKind::Noise | NodeKind::Shape | NodeKind::Bitmap) {
            continue;
        }
        let id = g.add_node(d.kind, [0.0, 0.0]);
        for (i, s) in d.image_inputs.iter().enumerate() {
            let src = if i == 0 { noise } else { shape };
            let _ = g.connect(src, "o:output", id, &crate::graph::image_socket_key(s.key));
        }
        out.push((id, d.name.to_string()));
    }
    // A few variations that are interesting to look at.
    let variants: &[(NodeKind, &str, &str, ParamValue)] = &[
        (NodeKind::Shape, "Star", "shape", ParamValue::Enum(4)),
        (NodeKind::Shape, "Polygon", "shape", ParamValue::Enum(3)),
        (NodeKind::Shape, "Ring", "shape", ParamValue::Enum(5)),
        (NodeKind::Shape, "Paraboloid", "shape", ParamValue::Enum(8)),
        (NodeKind::Noise, "Cells", "type", ParamValue::Enum(3)),
        (NodeKind::Noise, "Cell Edges", "type", ParamValue::Enum(4)),
        (NodeKind::Noise, "Ridged", "type", ParamValue::Enum(2)),
        (NodeKind::Gradient, "Radial", "type", ParamValue::Enum(2)),
        (NodeKind::Gradient, "Angular", "type", ParamValue::Enum(3)),
        (NodeKind::TileGenerator, "Tiles Disc", "pattern", ParamValue::Enum(0)),
        (NodeKind::TileGenerator, "Tiles Star", "pattern", ParamValue::Enum(4)),
    ];
    for (kind, label, key, value) in variants {
        let id = g.add_node(*kind, [0.0, 0.0]);
        if let Some(p) = g.node_mut(id).and_then(|n| n.param_mut(key)) {
            p.value = value.clone();
        }
        if *kind == NodeKind::TileGenerator {
            if let Some(n) = g.node_mut(id) {
                n.param_mut("random_rotation").unwrap().value = ParamValue::Float(45.0);
                n.param_mut("random_scale").unwrap().value = ParamValue::Float(0.5);
                n.param_mut("random_luminance").unwrap().value = ParamValue::Float(0.5);
            }
        }
        out.push((id, label.to_string()));
    }
    let transform = g.add_node(NodeKind::Transform2D, [0.0, 0.0]);
    let _ = g.connect(shape, "o:output", transform, "i:input");
    if let Some(n) = g.node_mut(transform) {
        n.param_mut("rotation").unwrap().value = ParamValue::Float(30.0);
        n.param_mut("scale").unwrap().value = ParamValue::Vec2([0.5, 0.8]);
        n.param_mut("offset").unwrap().value = ParamValue::Vec2([0.5, 0.25]);
    }
    out.push((transform, "Transform rot/scale".to_string()));
    (g, out)
}

/// Evaluate `graph` headlessly and write the listed nodes into one grid image.
pub fn contact_sheet(graph: &Graph, nodes: &[(NodeId, String)], path: &Path) -> Result<(), String> {
    let gpu = Gpu::headless()?;
    println!("GPU: {}", gpu.adapter_info);
    let mut engine = Engine::new(gpu, None);
    let start = std::time::Instant::now();
    engine.evaluate(graph);
    let _ = engine.gpu.device.poll(eframe::wgpu::PollType::wait_indefinitely());
    println!("evaluated {} image nodes in {:.1} ms", engine.report.image_nodes, start.elapsed().as_secs_f32() * 1000.0);

    let cell = 256u32;
    let cols = 6u32;
    let rows = (nodes.len() as u32).div_ceil(cols);
    let mut sheet = image::RgbaImage::from_pixel(cols * cell, rows * cell, image::Rgba([40, 40, 40, 255]));
    for (i, (id, label)) in nodes.iter().enumerate() {
        if let Some(err) = engine.state(*id).and_then(|s| s.error.clone()) {
            println!("  #{i:02} {label}: ERROR {err}");
        } else {
            println!("  #{i:02} {label}");
        }
        let Some(data) = read_node(&engine, *id) else { continue };
        let img = engine.image(*id).expect("image");
        let (w, h) = (img.width(), img.height());
        let mut q = vec![0u8; data.len()];
        crate::simd::quantize_u8(&data, &mut q);
        let Some(buf) = image::RgbaImage::from_raw(w, h, q) else { continue };
        let small = image::imageops::resize(&buf, cell - 4, cell - 4, image::imageops::FilterType::Triangle);
        let (x, y) = ((i as u32 % cols) * cell + 2, (i as u32 / cols) * cell + 2);
        image::imageops::overlay(&mut sheet, &small, x as i64, y as i64);
    }
    sheet.save(path).map_err(|e| e.to_string())?;
    println!("wrote {}", path.display());
    Ok(())
}
