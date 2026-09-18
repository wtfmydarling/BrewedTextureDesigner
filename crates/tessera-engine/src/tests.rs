//! GPU integration tests: build small graphs, evaluate them on the real GPU and check
//! pixel values. Skipped automatically when no adapter is available.

use tessera_core::graph::{Graph, NodeId};
use tessera_core::{Gradient, NodeKind, ParamValue, Value};

use crate::engine::{Engine, evaluate_blocking};
use crate::gpu::Gpu;

const RES: u32 = 256;

fn engine() -> Option<Engine> {
    match Gpu::headless() {
        Ok(gpu) => Some(Engine::new(gpu)),
        Err(e) => {
            eprintln!("skipping GPU test: {e}");
            None
        }
    }
}

fn graph() -> Graph {
    Graph { resolution: [RES, RES], ..Graph::default() }
}

fn set(g: &mut Graph, id: NodeId, key: &str, value: ParamValue) {
    g.node_mut(id).unwrap().param_mut(key).unwrap_or_else(|| panic!("no param {key}")).value = value;
}

fn pixel(data: &[f32], x: u32, y: u32) -> [f32; 4] {
    let i = ((y * RES + x) * 4) as usize;
    [data[i], data[i + 1], data[i + 2], data[i + 3]]
}

fn close(a: f32, b: f32, eps: f32) -> bool {
    (a - b).abs() <= eps
}

fn eval_read(e: &mut Engine, g: &Graph, id: NodeId) -> Vec<f32> {
    let ev = evaluate_blocking(e, g);
    if let Some(err) = ev.node(id).and_then(|n| n.error()) {
        panic!("node error: {err}");
    }
    if let Some(err) = &e.state(id).unwrap().error {
        panic!("node error: {err}");
    }
    e.read_image(id).expect("node image").2
}

fn mean(d: &[f32]) -> f64 {
    d.iter().step_by(4).map(|v| *v as f64).sum::<f64>() / (d.len() / 4) as f64
}

#[test]
fn uniform_invert_blend_levels() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let c = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
    set(&mut g, c, "color", ParamValue::Color([0.25, 0.5, 0.75, 1.0]));
    let inv = g.add_node(NodeKind::INVERT, [0.0, 0.0]);
    g.connect(c, "o:output", inv, "i:input").unwrap();

    let data = eval_read(&mut e, &g, c);
    assert_eq!(pixel(&data, 17, 99), [0.25, 0.5, 0.75, 1.0]);
    let data = eval_read(&mut e, &g, inv);
    assert_eq!(pixel(&data, 3, 3), [0.75, 0.5, 0.25, 1.0]);

    // Multiply blend: 0.5 * 0.5 = 0.25
    let half = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
    set(&mut g, half, "color", ParamValue::Color([0.5, 0.5, 0.5, 1.0]));
    let blend = g.add_node(NodeKind::BLEND, [0.0, 0.0]);
    set(&mut g, blend, "mode", ParamValue::Enum(3));
    g.connect(half, "o:output", blend, "i:foreground").unwrap();
    g.connect(half, "o:output", blend, "i:background").unwrap();
    let data = eval_read(&mut e, &g, blend);
    assert_eq!(pixel(&data, 100, 100), [0.25, 0.25, 0.25, 1.0]);

    // Levels: input range [0.25, 0.75] maps 0.5 -> 0.5; gamma 2 maps 0.25 -> 0.5.
    let lv = g.add_node(NodeKind::LEVELS, [0.0, 0.0]);
    g.connect(half, "o:output", lv, "i:input").unwrap();
    set(&mut g, lv, "in_low", ParamValue::Float(0.25));
    set(&mut g, lv, "in_high", ParamValue::Float(0.75));
    let data = eval_read(&mut e, &g, lv);
    assert!(close(pixel(&data, 0, 0)[0], 0.5, 1e-3));
    set(&mut g, lv, "in_low", ParamValue::Float(0.0));
    set(&mut g, lv, "in_high", ParamValue::Float(1.0));
    set(&mut g, lv, "gamma", ParamValue::Float(2.0));
    g.connect(blend, "o:output", lv, "i:input").unwrap(); // 0.25 input
    let data = eval_read(&mut e, &g, lv);
    assert!(close(pixel(&data, 0, 0)[0], 0.5, 1e-3), "{:?}", pixel(&data, 0, 0));
}

#[test]
fn transform_identity_and_wrapping_offset() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    let t = g.add_node(NodeKind::TRANSFORM_2D, [0.0, 0.0]);
    g.connect(noise, "o:output", t, "i:input").unwrap();
    let src = eval_read(&mut e, &g, noise);
    let out = eval_read(&mut e, &g, t);
    for (a, b) in src.iter().zip(&out) {
        assert!(close(*a, *b, 2e-3));
    }
    // Offset by a quarter of the image = 64 pixels, wrapping around (H and V tiling).
    set(&mut g, t, "offset", ParamValue::Vec2([0.25, 0.0]));
    let out = eval_read(&mut e, &g, t);
    for y in (0..RES).step_by(37) {
        for x in (0..RES).step_by(13) {
            let sx = (x + RES - 64) % RES;
            let a = pixel(&out, x, y)[0];
            let b = pixel(&src, sx, y)[0];
            assert!(close(a, b, 2e-3), "({x},{y}): {a} vs {b}");
        }
    }
    // Without tiling the uncovered area shows the matte color (transparent).
    set(&mut g, t, "tiling", ParamValue::Enum(0));
    let out = eval_read(&mut e, &g, t);
    assert_eq!(pixel(&out, 10, 10), [0.0, 0.0, 0.0, 0.0]);
}

#[test]
fn blur_preserves_mean_with_tiling() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    set(&mut g, noise, "type", ParamValue::Enum(5)); // white noise
    let blur = g.add_node(NodeKind::BLUR, [0.0, 0.0]);
    set(&mut g, blur, "radius", ParamValue::Float(100.0)); // exercises the pyramid path
    g.connect(noise, "o:output", blur, "i:input").unwrap();
    let src = eval_read(&mut e, &g, noise);
    let out = eval_read(&mut e, &g, blur);
    let (a, b) = (mean(&src), mean(&out));
    assert!((a - b).abs() < 0.01, "mean {a} vs {b}");
    let var =
        |d: &[f32], m: f64| d.iter().step_by(4).map(|v| (*v as f64 - m).powi(2)).sum::<f64>() / (RES * RES) as f64;
    assert!(var(&out, b) < var(&src, a) * 0.05);

    // Directional blur also keeps the mean (it wraps around the tiling borders).
    let dir = g.add_node(NodeKind::DIRECTIONAL_BLUR, [0.0, 0.0]);
    g.connect(noise, "o:output", dir, "i:input").unwrap();
    let out = eval_read(&mut e, &g, dir);
    assert!((mean(&out) - a).abs() < 0.01);
}

#[test]
fn output_format_single_channel() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let c = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
    set(&mut g, c, "color", ParamValue::Color([0.25, 0.5, 0.75, 0.5]));
    let o = g.add_node(NodeKind::OUTPUT, [0.0, 0.0]);
    g.connect(c, "o:output", o, "i:input").unwrap();
    set(&mut g, o, "format", ParamValue::Enum(4)); // G
    let data = eval_read(&mut e, &g, o);
    assert_eq!(pixel(&data, 5, 5), [0.5, 0.5, 0.5, 1.0]);
    set(&mut g, o, "format", ParamValue::Enum(2)); // RG
    let data = eval_read(&mut e, &g, o);
    assert_eq!(pixel(&data, 5, 5), [0.25, 0.5, 0.0, 1.0]);
}

#[test]
fn variables_and_math_drive_exposed_parameters() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let input = g.add_node(NodeKind::FLOAT_INPUT, [0.0, 0.0]);
    g.bind_variable(input, "brightness").unwrap();
    g.variable_mut("brightness").unwrap().max = Value::Float(4.0);
    g.variable_mut("brightness").unwrap().value = Value::Float(3.0);
    let math = g.add_node(NodeKind::MATH, [0.0, 0.0]);
    set(&mut g, math, "expression", ParamValue::Text("brightness / 4 - x".into()));
    g.node_mut(math).unwrap().var_values.insert("x".into(), 0.5);
    let color = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
    g.set_exposed(color, "color", true).unwrap();
    g.connect(math, "o:result", color, "p:color").unwrap();
    // brightness is auto-bound by name: 3/4 - 0.5 = 0.25 -> gray color
    let data = eval_read(&mut e, &g, color);
    assert_eq!(pixel(&data, 1, 1), [0.25, 0.25, 0.25, 1.0]);

    // A broken expression reports an error instead of crashing, and the driven parameter
    // falls back to its own value.
    set(&mut g, math, "expression", ParamValue::Text("sin(".into()));
    let ev = evaluate_blocking(&mut e, &g);
    assert!(ev.node(math).unwrap().error().unwrap().contains("incorrect"));
    let data = e.read_image(color).unwrap().2;
    assert_eq!(pixel(&data, 1, 1), [0.5, 0.5, 0.5, 1.0]);
}

#[test]
fn shape_and_pixel_math() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let shape = g.add_node(NodeKind::SHAPE, [0.0, 0.0]);
    let data = eval_read(&mut e, &g, shape);
    assert!(pixel(&data, RES / 2, RES / 2)[0] > 0.99);
    assert!(pixel(&data, 2, 2)[0] < 0.01);

    let pm = g.add_node(NodeKind::PIXEL_MATH, [0.0, 0.0]);
    set(&mut g, pm, "expression", ParamValue::Text("u * k + img".into()));
    g.node_mut(pm).unwrap().var_values.insert("k".into(), 0.5);
    g.connect(shape, "o:output", pm, "v:img").unwrap();
    let data = eval_read(&mut e, &g, pm);
    let x = 64u32;
    let expected = (x as f32 + 0.5) / RES as f32 * 0.5;
    assert!(close(pixel(&data, x, 3)[0], expected, 2e-3));
    assert!(close(pixel(&data, RES / 2, RES / 2)[0], 1.0 + 0.5 * (RES / 2) as f32 / RES as f32, 1e-2));
}

#[test]
fn evaluation_is_incremental() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let a = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    let b = g.add_node(NodeKind::LEVELS, [0.0, 0.0]);
    let c = g.add_node(NodeKind::INVERT, [0.0, 0.0]);
    g.add_node(NodeKind::SHAPE, [0.0, 0.0]);
    g.connect(a, "o:output", b, "i:input").unwrap();
    g.connect(b, "o:output", c, "i:input").unwrap();
    evaluate_blocking(&mut e, &g);
    assert_eq!(e.report.recomputed, 4);
    evaluate_blocking(&mut e, &g);
    assert_eq!(e.report.recomputed, 0);
    set(&mut g, b, "gamma", ParamValue::Float(2.0));
    evaluate_blocking(&mut e, &g);
    assert_eq!(e.report.recomputed, 2); // levels + invert, not noise or shape
    g.resolution = [128, 128];
    evaluate_blocking(&mut e, &g);
    assert_eq!(e.report.recomputed, 4);
    // Thumbnails only for images that changed since the last ones.
    assert_eq!(e.render_thumbnails().len(), 4);
    assert!(e.render_thumbnails().is_empty());
    set(&mut g, c, "invert_alpha", ParamValue::Bool(true));
    evaluate_blocking(&mut e, &g);
    let thumbs = e.render_thumbnails();
    assert_eq!(thumbs.len(), 1);
    assert_eq!(thumbs[0].rgba.len(), (thumbs[0].size * thumbs[0].size * 4) as usize);
}

#[test]
fn every_image_node_runs_with_defaults_in_every_tiling_mode() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let src = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    let mut ids = Vec::new();
    for kind in NodeKind::ALL.iter().filter(|k| k.def().has_image()) {
        let id = g.add_node(*kind, [0.0, 0.0]);
        for s in kind.def().inputs {
            let _ = g.connect(src, "o:output", id, &tessera_core::graph::input_socket_key(s.key));
        }
        ids.push((id, kind.def().name));
    }
    for tiling in 0..4 {
        for (id, _) in &ids {
            set(&mut g, *id, "tiling", ParamValue::Enum(tiling));
        }
        let ev = evaluate_blocking(&mut e, &g);
        for (id, name) in &ids {
            let st = e.state(*id).unwrap();
            if *name == "Bitmap" {
                assert!(st.error.is_some(), "no file selected");
                continue;
            }
            assert!(st.error.is_none(), "{name}: {:?}", st.error);
            assert!(ev.node(*id).unwrap().error().is_none(), "{name}");
            assert!(st.image.is_some(), "{name}");
        }
    }
    assert!(e.gpu.take_errors().is_empty());
}

#[test]
fn tile_offset_moves_the_random_variation() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let t = g.add_node(NodeKind::TILE_GENERATOR, [0.0, 0.0]);
    set(&mut g, t, "random_luminance", ParamValue::Float(1.0));
    set(&mut g, t, "x_amount", ParamValue::Int(4));
    set(&mut g, t, "y_amount", ParamValue::Int(4));
    let base = eval_read(&mut e, &g, t);
    set(&mut g, t, "offset", ParamValue::Float(1.0));
    let shifted = eval_read(&mut e, &g, t);
    // Row 0 has no shift; row 1 is shifted by one whole tile: the tile under a pixel of row
    // 1 now shows the variation of its left neighbour.
    let (x, y0, y1) = (RES / 8 * 3, RES / 8, RES / 8 * 3);
    assert_eq!(pixel(&base, x, y0), pixel(&shifted, x, y0));
    assert_ne!(pixel(&base, x, y1), pixel(&shifted, x, y1), "offset 1 must not look like offset 0");
    assert_eq!(pixel(&shifted, x, y1), pixel(&base, x - RES / 4, y1));
    // Vertical offset shifts columns instead of rows.
    set(&mut g, t, "vertical_offset", ParamValue::Bool(true));
    let vertical = eval_read(&mut e, &g, t);
    assert_eq!(pixel(&vertical, x - RES / 4, y0), pixel(&base, x - RES / 4, y0), "column 0 is not shifted");
    assert_eq!(pixel(&vertical, x, y1), pixel(&base, x, y1 - RES / 4));
}

#[test]
fn gradient_map_with_many_stops() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let ramp = g.add_node(NodeKind::GRADIENT, [0.0, 0.0]);
    let gm = g.add_node(NodeKind::GRADIENT_MAP, [0.0, 0.0]);
    g.connect(ramp, "o:output", gm, "i:input").unwrap();
    let stops: Vec<(f32, [f32; 4])> =
        (0..7).map(|i| (i as f32 / 6.0, [(i % 2) as f32, (i % 3) as f32 / 2.0, i as f32 / 6.0, 1.0])).collect();
    let gradient = Gradient::from_pairs(&stops);
    set(&mut g, gm, "gradient", ParamValue::Gradient(gradient.clone()));
    let src = eval_read(&mut e, &g, ramp);
    let out = eval_read(&mut e, &g, gm);
    for x in (0..RES).step_by(17) {
        let t = pixel(&src, x, 5)[0];
        let expected = gradient.sample(t);
        let got = pixel(&out, x, 5);
        for c in 0..3 {
            assert!(close(got[c], expected[c], 4e-3), "x {x}: {got:?} vs {expected:?}");
        }
    }
}

#[test]
fn histogram_scan_thresholds() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let ramp = g.add_node(NodeKind::GRADIENT, [0.0, 0.0]);
    let hs = g.add_node(NodeKind::HISTOGRAM_SCAN, [0.0, 0.0]);
    g.connect(ramp, "o:output", hs, "i:input").unwrap();
    set(&mut g, hs, "contrast", ParamValue::Float(1.0));
    set(&mut g, hs, "position", ParamValue::Float(0.5));
    let out = eval_read(&mut e, &g, hs);
    assert_eq!(pixel(&out, 10, 0)[0], 0.0);
    assert_eq!(pixel(&out, RES - 10, 0)[0], 1.0);
}

#[test]
fn height_effects_on_flat_input_are_neutral() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let flat = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
    let checks = [(NodeKind::AMBIENT_OCCLUSION, 1.0), (NodeKind::CURVATURE, 0.5), (NodeKind::EDGE_DETECT, 0.0)];
    for (kind, expected) in checks {
        let id = g.add_node(kind, [0.0, 0.0]);
        g.connect(flat, "o:output", id, "i:input").unwrap();
        let out = eval_read(&mut e, &g, id);
        assert!(close(pixel(&out, 40, 70)[0], expected, 1e-3), "{}: {:?}", kind.def().name, pixel(&out, 40, 70));
    }
    // A hole with steep walls: occluded next to the wall, open in the middle.
    let shape = g.add_node(NodeKind::SHAPE, [0.0, 0.0]);
    set(&mut g, shape, "invert", ParamValue::Bool(true)); // disc -> hole of radius 0.3
    let ao = g.add_node(NodeKind::AMBIENT_OCCLUSION, [0.0, 0.0]);
    g.connect(shape, "o:output", ao, "i:input").unwrap();
    let out = eval_read(&mut e, &g, ao);
    let near_wall = pixel(&out, RES / 2 + (0.29 * RES as f32) as u32, RES / 2)[0];
    assert!(near_wall < 0.9, "{near_wall}");
    assert!(pixel(&out, RES / 2, RES / 2)[0] > 0.99);
}

#[test]
fn bypass_passes_the_input_through() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    let inv = g.add_node(NodeKind::INVERT, [0.0, 0.0]);
    g.connect(noise, "o:output", inv, "i:input").unwrap();
    g.node_mut(inv).unwrap().bypass = true;
    let src = eval_read(&mut e, &g, noise);
    let out = eval_read(&mut e, &g, inv);
    assert_eq!(pixel(&src, 33, 44), pixel(&out, 33, 44));
}

#[test]
fn readbacks_preview_and_line_sampling() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let ramp = g.add_node(NodeKind::GRADIENT, [0.0, 0.0]);
    evaluate_blocking(&mut e, &g);
    let samples = e.sample_line(ramp, [0.0, 0.5], [1.0, 0.5], 5).unwrap();
    assert_eq!(samples.len(), 5);
    assert!(samples[0][0] < 0.05 && samples[4][0] > 0.95, "{samples:?}");
    assert!(close(samples[2][0], 0.5, 0.02));
    let p = e.probe(ramp, [RES / 2, 3]).unwrap();
    assert!(close(p[0], 0.5, 0.01));
    let stats = e.stats(ramp).unwrap();
    assert!(close(stats.mean[0], 0.5, 0.01));
    let preview = e.prepare_preview(Some(ramp)).expect("changed").expect("image");
    assert_eq!(preview.size, [RES, RES]);
    assert_eq!(preview.texture.mip_level_count(), 9);
    assert!(e.prepare_preview(Some(ramp)).is_none(), "unchanged");
    assert!(matches!(e.prepare_preview(None), Some(None)));
}

#[test]
fn bitmap_node_decodes_and_resamples() {
    let Some(mut e) = engine() else { return };
    let dir = std::env::temp_dir().join(format!("tessera_bitmap_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tex.png");
    // 4x2 image: distinct colors per texel.
    let colors: [[u8; 4]; 8] = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 255, 255],
        [0, 0, 0, 255],
        [128, 128, 128, 255],
        [255, 255, 0, 255],
        [0, 255, 255, 255],
    ];
    let raw: Vec<u8> = colors.iter().flatten().copied().collect();
    image::RgbaImage::from_raw(4, 2, raw).unwrap().save(&path).unwrap();

    let mut g = graph();
    let b = g.add_node(NodeKind::BITMAP, [0.0, 0.0]);
    set(&mut g, b, "tiling", ParamValue::Enum(0)); // clamp at the borders
    set(&mut g, b, "path", ParamValue::Text(path.display().to_string()));
    let data = eval_read(&mut e, &g, b);
    let px =
        |tx: f32, ty: f32| pixel(&data, ((tx + 0.5) / 4.0 * RES as f32) as u32, ((ty + 0.5) / 2.0 * RES as f32) as u32);
    let green = px(1.0, 0.0);
    let yellow = px(2.0, 1.0);
    assert!(close(green[0], 0.0, 0.02) && close(green[1], 1.0, 0.02), "{green:?}");
    assert!(close(yellow[0], 1.0, 0.02) && close(yellow[1], 1.0, 0.02) && close(yellow[2], 0.0, 0.02), "{yellow:?}");

    // A missing file is an error on the node, not a crash.
    set(&mut g, b, "path", ParamValue::Text(dir.join("missing.png").display().to_string()));
    evaluate_blocking(&mut e, &g);
    let error = e.state(b).unwrap().error.clone();
    assert!(error.as_deref().is_some_and(|m| m.contains("Cannot open")), "{error:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn demo_graph_evaluates_without_errors() {
    let Some(mut e) = engine() else { return };
    let (mut g, _) = tessera_core::demo::brick_wall();
    g.resolution = [512, 512];
    let ev = evaluate_blocking(&mut e, &g);
    for n in &g.nodes {
        assert!(ev.node(n.id).unwrap().error().is_none(), "{}", n.def().name);
        if n.def().has_image() {
            let st = e.state(n.id).unwrap();
            assert!(st.error.is_none(), "{}: {:?}", n.def().name, st.error);
        }
    }
    assert!(e.gpu.take_errors().is_empty());
}
