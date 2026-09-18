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
        if *kind == NodeKind::TileGenerator
            && let Some(n) = g.node_mut(id)
        {
            n.param_mut("random_rotation").unwrap().value = ParamValue::Float(45.0);
            n.param_mut("random_scale").unwrap().value = ParamValue::Float(0.5);
            n.param_mut("random_luminance").unwrap().value = ParamValue::Float(0.5);
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

fn time_ms<T>(runs: usize, mut f: impl FnMut() -> T) -> f64 {
    let _ = f(); // warm-up
    let start = std::time::Instant::now();
    for _ in 0..runs {
        std::hint::black_box(f());
    }
    start.elapsed().as_secs_f64() * 1000.0 / runs as f64
}

/// Measure GPU graph evaluation and the SIMD CPU kernels against scalar code.
pub fn bench() -> Result<(), String> {
    use crate::graph::Value;
    let gpu = Gpu::headless()?;
    println!("GPU: {}", gpu.adapter_info);
    println!("CPU threads: {}", rayon::current_num_threads());
    let mut engine = Engine::new(gpu, None);
    let (mut g, _) = crate::demo::brick_wall();
    g.resolution = [2048, 2048];
    let wait = |e: &Engine| {
        let _ = e.gpu.device.poll(eframe::wgpu::PollType::wait_indefinitely());
    };

    let t = std::time::Instant::now();
    engine.evaluate(&g);
    wait(&engine);
    println!(
        "\nGraph: {} nodes ({} image nodes) at 2048x2048, rgba16float",
        g.nodes.len(),
        engine.report.image_nodes
    );
    println!("  first evaluation (includes shader compilation): {:8.1} ms", t.elapsed().as_secs_f64() * 1000.0);

    let full = time_ms(10, || {
        engine.reset();
        engine.evaluate(&g);
        wait(&engine);
    });
    println!("  full re-evaluation of every node:               {full:8.1} ms");

    // Changing the 'mortar' input re-runs everything downstream of the tile size.
    let mortar = g.inputs.iter().find(|i| i.name == "mortar").map(|i| i.id).ok_or("demo input missing")?;
    let mut k = 0;
    let incremental = time_ms(20, || {
        k += 1;
        if let Some(i) = g.input_mut(mortar) {
            i.value = Value::Float(0.03 + (k % 10) as f32 * 0.01);
        }
        engine.evaluate(&g);
        wait(&engine);
        engine.report.recomputed
    });
    println!(
        "  slider change (mortar, {} nodes recomputed):    {incremental:8.1} ms",
        engine.report.recomputed
    );
    let idle = time_ms(50, || {
        engine.evaluate(&g);
        wait(&engine);
    });
    println!("  idle frame (nothing changed):                   {idle:8.3} ms");

    // CPU kernels on one 2048x2048 RGBA image.
    let n = 2048 * 2048 * 4;
    let mut state = 0x9e37_79b9u32;
    let src: Vec<f32> = (0..n)
        .map(|_| {
            state = crate::expr::builtins::pcg_hash(state);
            state as f32 / u32::MAX as f32
        })
        .collect();
    println!("\nCPU kernels on a 2048x2048 RGBA image (SIMD + all cores vs scalar single thread):");

    let mut dst = vec![0u8; n];
    let simd_q = time_ms(10, || crate::simd::quantize_u8(&src, &mut dst));
    let scalar_q = time_ms(3, || {
        for (d, s) in dst.iter_mut().zip(&src) {
            *d = (s.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
    });
    println!("  quantize f32 -> u8:    {simd_q:7.2} ms  vs {scalar_q:7.2} ms  ({:.1}x)", scalar_q / simd_q);

    let mut half = vec![0u16; n];
    crate::simd::f32_to_f16(&src, &mut half);
    let mut back = vec![0f32; n];
    let simd_h = time_ms(10, || crate::simd::f16_to_f32(&half, &mut back));
    let scalar_h = time_ms(3, || {
        for (d, s) in back.iter_mut().zip(&half) {
            *d = half::f16::from_bits(*s).to_f32_const();
        }
    });
    println!("  half -> f32:           {simd_h:7.2} ms  vs {scalar_h:7.2} ms  ({:.1}x)", scalar_h / simd_h);

    let simd_s = time_ms(10, || crate::simd::image_stats(&src, 2048, 2048));
    let scalar_s = time_ms(3, || {
        let mut hist = [[0u32; 256]; 5];
        let (mut mn, mut mx, mut sum) = ([f32::MAX; 4], [f32::MIN; 4], [0f64; 4]);
        for px in src.chunks_exact(4) {
            for c in 0..4 {
                hist[c][((px[c] * 256.0) as usize).min(255)] += 1;
                mn[c] = mn[c].min(px[c]);
                mx[c] = mx[c].max(px[c]);
                sum[c] += px[c] as f64;
            }
            let l = 0.2126 * px[0] + 0.7152 * px[1] + 0.0722 * px[2];
            hist[4][((l * 256.0) as usize).min(255)] += 1;
        }
        (hist, mn, mx, sum)
    });
    println!("  histogram + min/max/mean: {simd_s:4.2} ms  vs {scalar_s:7.2} ms  ({:.1}x)", scalar_s / simd_s);
    Ok(())
}
