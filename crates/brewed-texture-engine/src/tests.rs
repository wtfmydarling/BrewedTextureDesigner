//! GPU integration tests: build small graphs, evaluate them on the real GPU and check
//! pixel values. Skipped automatically when no adapter is available.

use brewed_texture_core::graph::{Graph, NodeId};
use brewed_texture_core::{Gradient, NodeKind, ParamValue, Value};

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
    // Without tiling the uncovered area shows the matte color: as gray on a grayscale image,
    // which has no alpha...
    set(&mut g, t, "tiling", ParamValue::Enum(0));
    set(&mut g, t, "matte", ParamValue::Color([0.5, 0.5, 0.5, 0.0]));
    let out = eval_read(&mut e, &g, t);
    assert_eq!(pixel(&out, 10, 10), [0.5, 0.5, 0.5, 1.0]);
    // ... and as it is, transparent, on a color image.
    let color = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
    g.connect(color, "o:output", t, "i:input").unwrap();
    let out = eval_read(&mut e, &g, t);
    assert_eq!(pixel(&out, 10, 10), [0.5, 0.5, 0.5, 0.0]);
}

#[test]
fn blur_preserves_mean_with_tiling() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    set(&mut g, noise, "type", ParamValue::Enum(brewed_texture_core::nodes::NOISE_WHITE));
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
            let _ = g.connect(src, "o:output", id, &brewed_texture_core::graph::input_socket_key(s.key));
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
fn readbacks_and_preview() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let ramp = g.add_node(NodeKind::GRADIENT, [0.0, 0.0]);
    evaluate_blocking(&mut e, &g);
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
    let dir = std::env::temp_dir().join(format!("brewed_bitmap_{}", std::process::id()));
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
    let (mut g, _) = brewed_texture_core::demo::graph();
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

/// The occlusion of the demo wall at 1024 px, to look at (`BREWED_UI_SNAPSHOTS=<folder>`).
#[test]
#[ignore = "a picture to look at"]
fn ao_of_the_demo_as_a_picture() {
    let Some(mut e) = engine() else { return };
    let Some(dir) = std::env::var_os("BREWED_UI_SNAPSHOTS") else { return };
    let (mut g, _) = brewed_texture_core::demo::graph();
    g.resolution = [1024, 1024];
    let ao = g.nodes_of(NodeKind::AMBIENT_OCCLUSION).next().unwrap().id;
    let out = eval_read(&mut e, &g, ao);
    let px: Vec<u8> = out
        .chunks(4)
        .flat_map(|p| {
            let v = (p[0].clamp(0.0, 1.0) * 255.0).round() as u8;
            [v, v, v, 255]
        })
        .collect();
    image::RgbaImage::from_raw(1024, 1024, px).unwrap().save(std::path::Path::new(&dir).join("ao_demo.png")).unwrap();
}

/// Bilinear read of the red channel of a `size`×`size` output at a point in pixels.
fn sample_at(data: &[f32], size: u32, x: f32, y: f32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let at = |dx: u32, dy: u32| {
        let i = (((y0 as u32 + dy) * size + x0 as u32 + dx) * 4) as usize;
        data[i]
    };
    let top = at(0, 0) * (1.0 - fx) + at(1, 0) * fx;
    let bottom = at(0, 1) * (1.0 - fx) + at(1, 1) * fx;
    top * (1.0 - fy) + bottom * fy
}

/// Occlusion around a round plateau: the same all the way round at any distance from its
/// wall (no rays from the directions it is sampled in), and lighter the further from the
/// wall, without rings (no layers from the steps along each direction).
#[test]
fn occlusion_is_smooth_and_the_same_in_every_direction() {
    let Some(mut e) = engine() else { return };
    // Fine enough that the disc itself is round at the pixel scale the occlusion is read at.
    const SIZE: u32 = 1024;
    let mut g = graph();
    g.resolution = [SIZE, SIZE];
    let disc = g.add_node(NodeKind::SHAPE, [0.0, 0.0]);
    let ao = g.add_node(NodeKind::AMBIENT_OCCLUSION, [0.0, 0.0]);
    g.connect(disc, "o:output", ao, "i:input").unwrap();
    set(&mut g, ao, "radius", ParamValue::Float(96.0));
    set(&mut g, ao, "height", ParamValue::Float(0.1));
    let out = eval_read(&mut e, &g, ao);
    let sample = |x: f32, y: f32| sample_at(&out, SIZE, x, y);
    let c = SIZE as f32 / 2.0;
    let edge = 0.3 * SIZE as f32;
    let reach = 96.0 / 1024.0 * SIZE as f32;
    // All the way round, a few distances out from the wall.
    for out_by in [0.15, 0.35, 0.6] {
        let r = edge + 1.5 + reach * out_by;
        let ring: Vec<f32> = (0..144).map(|i| i as f32 / 144.0 * std::f32::consts::TAU).map(|a| sample(c + r * a.cos(), c + r * a.sin())).collect();
        let (lo, hi) = ring.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        // The directions it used to be read in left 0.07 here.
        assert!(hi - lo < 0.03, "{} px out: from {lo} to {hi} round the plateau", r - edge);
        assert!(lo < 0.97, "{} px out: {lo} is not occluded at all", r - edge);
    }
    // Out from the wall, pixel by pixel: never darker further away.
    let line: Vec<f32> = (0..(reach as usize)).map(|i| sample(c + edge + 1.5 + i as f32, c + 0.37)).collect();
    for (i, w) in line.windows(2).enumerate() {
        assert!(w[1] >= w[0] - 0.004, "{} px out: {} after {}", i + 1, w[1], w[0]);
    }
}

/// Every look of the noise node: each type, and each style of the types that have them,
/// set on `noise` in turn; `f` gets its name.
fn each_noise_look(g: &mut Graph, noise: NodeId, mut f: impl FnMut(&mut Graph, &str)) {
    for (t, name) in brewed_texture_core::nodes::NOISE_TYPES.iter().enumerate() {
        set(g, noise, "type", ParamValue::Enum(t as u32));
        f(g, name);
    }
}

/// Every look of the noise node with its defaults, each tiled 2×2 so seams would show
/// (`BREWED_UI_SNAPSHOTS=<folder>`, writes `noises.png` and `noise_<look>.png`).
#[test]
#[ignore = "a picture to look at"]
fn noises_as_a_picture() {
    let Some(mut e) = engine() else { return };
    let Some(dir) = std::env::var_os("BREWED_UI_SNAPSHOTS") else { return };
    const SIZE: u32 = 512;
    const COLUMNS: u32 = 5;
    let mut g = graph();
    g.resolution = [SIZE, SIZE];
    let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    let mut looks = Vec::new();
    each_noise_look(&mut g, noise, |g, name| looks.push((name.to_string(), eval_read(&mut e, g, noise))));
    let rows = (looks.len() as u32).div_ceil(COLUMNS);
    let (w, h) = (COLUMNS * SIZE, rows * SIZE);
    let mut sheet = vec![0u8; (w * h * 4) as usize];
    for (t, (name, out)) in looks.iter().enumerate() {
        let t = t as u32;
        let own: Vec<u8> = out.chunks(4).flat_map(|p| { let v = (p[0].clamp(0.0, 1.0) * 255.0).round() as u8; [v, v, v, 255] }).collect();
        let file = format!("noise_{}.png", name.to_lowercase().replace(' ', "_"));
        image::RgbaImage::from_raw(SIZE, SIZE, own).unwrap().save(std::path::Path::new(&dir).join(file)).unwrap();
        let (ox, oy) = ((t % COLUMNS) * SIZE, (t / COLUMNS) * SIZE);
        for y in 0..SIZE {
            for x in 0..SIZE {
                // Half size, twice in each direction.
                let (sx, sy) = ((x * 2) % SIZE, (y * 2) % SIZE);
                let v = (0..4).map(|k| out[(((sy + k / 2) % SIZE * SIZE + (sx + k % 2) % SIZE) * 4) as usize]).sum::<f32>() / 4.0;
                let v = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                let i = (((oy + y) * w + ox + x) * 4) as usize;
                sheet[i..i + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
    }
    image::RgbaImage::from_raw(w, h, sheet).unwrap().save(std::path::Path::new(&dir).join("noises.png")).unwrap();
}

/// How long every look of the noise takes at 2048×2048 (prints; for tuning).
#[test]
#[ignore = "a measurement"]
fn noise_costs() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    g.resolution = [2048, 2048];
    let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    each_noise_look(&mut g, noise, |g, name| {
        evaluate_blocking(&mut e, g);
        let runs = 10;
        let start = std::time::Instant::now();
        for seed in 1..=runs {
            set(g, noise, "seed", ParamValue::Int(seed));
            evaluate_blocking(&mut e, g);
        }
        println!("{name:>18}: {:6.2} ms", start.elapsed().as_secs_f64() * 1000.0 / runs as f64);
    });
}

/// How every noise uses the range from black to white with its defaults (prints; for tuning):
/// its mean, its spread, and how much of it is cut off at black and at white.
#[test]
#[ignore = "a measurement"]
fn noise_levels() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    g.resolution = [512, 512];
    let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    each_noise_look(&mut g, noise, |g, name| {
        let px: Vec<f64> = eval_read(&mut e, g, noise).iter().step_by(4).map(|v| *v as f64).collect();
        let n = px.len() as f64;
        let m = px.iter().sum::<f64>() / n;
        let sd = (px.iter().map(|v| (v - m).powi(2)).sum::<f64>() / n).sqrt();
        let black = px.iter().filter(|v| **v <= 0.001).count() as f64 / n * 100.0;
        let white = px.iter().filter(|v| **v >= 0.999).count() as f64 / n * 100.0;
        println!("{name:>12}: mean {m:.3}  sd {sd:.3}  black {black:5.1}%  white {white:5.1}%");
    });
}

/// Every image of the demo repeats without a seam: across each edge, neighbouring pixels
/// differ no more than neighbouring pixels do anywhere else.
#[test]
fn the_demo_tiles() {
    let Some(mut e) = engine() else { return };
    const SIZE: u32 = 512;
    let (mut g, _) = brewed_texture_core::demo::graph();
    g.resolution = [SIZE, SIZE];
    evaluate_blocking(&mut e, &g);
    for n in g.nodes.iter().filter(|n| n.def().has_image()) {
        let (_, _, px) = e.read_image(n.id).expect("an image");
        let (across, inside) = seams(&px, SIZE);
        assert!(across <= inside * 1.25 + 1e-3, "{}: {across} across the edges, at most {inside} anywhere else", n.def().name);
    }
}

/// How much neighbouring pixels of a `size`×`size` image differ, on average along a line
/// between two rows or two columns: the most across its edges (as if it were tiled), and
/// the most between any other two rows or columns. An image with sharp features has lines
/// that cross many of them, so a seam is only one if it stands out from all of those.
fn seams(px: &[f32], size: u32) -> (f32, f32) {
    let at = |x: u32, y: u32, c: u32| px[((y * size + x) * 4 + c) as usize];
    let step = |a: (u32, u32), b: (u32, u32)| (0..3).map(|c| (at(a.0, a.1, c) - at(b.0, b.1, c)).abs()).sum::<f32>();
    // Between column (row) j and the next one, wrapping around after the last.
    let line = |j: u32| {
        let k = (j + 1) % size;
        let (mut columns, mut rows) = (0.0, 0.0);
        for i in 0..size {
            columns += step((j, i), (k, i));
            rows += step((i, j), (i, k));
        }
        (columns / size as f32).max(rows / size as f32)
    };
    let inside = (0..size - 1).map(line).fold(0.0, f32::max);
    (line(size - 1), inside)
}

/// Every look of the noise tiles, and none of them comes out flat.
#[test]
fn every_noise_tiles() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    each_noise_look(&mut g, noise, |g, name| {
        let px = eval_read(&mut e, g, noise);
        let m = mean(&px);
        let spread = px.iter().step_by(4).map(|v| (*v as f64 - m).powi(2)).sum::<f64>() / (RES * RES) as f64;
        assert!(spread.sqrt() > 0.05, "{name} is flat: {m} ± {}", spread.sqrt());
        let (across, inside) = seams(&px, RES);
        eprintln!("{name:>18}: {across:.4} across the edges, at most {inside:.4} anywhere else");
        assert!(across <= inside * 1.25 + 1e-3, "{name}: {across} across the edges, at most {inside} anywhere else");
    });
}

/// The pictures of a kind of node's options: one per option, each what a node of the kind
/// makes with it as it is added, drawn once and the same whatever any node of the graph is
/// set to.
#[test]
fn pictures_of_the_options() {
    let Some(mut e) = engine() else { return };
    let types = brewed_texture_core::nodes::NOISE_TYPES.len();
    let choices = e.render_choices(NodeKind::NOISE, "type").expect("pictures");
    assert_eq!(choices.pictures.len(), types);
    for i in 0..types {
        assert_eq!(choices.pictures[i].len(), (choices.size * choices.size * 4) as usize);
        for j in 0..i {
            assert_ne!(choices.pictures[i], choices.pictures[j], "options {i} and {j} look the same");
        }
    }
    // The first is what a noise node makes as it is added: its own thumbnail (to a step of
    // 8 bits here and there: the picture is drawn in 32 bits, the node stored in 16).
    assert_eq!(crate::engine::CHOICE_RES, RES);
    let mut g = graph();
    let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    evaluate_blocking(&mut e, &g);
    let thumb = e.render_thumbnails().into_iter().find(|t| t.node == noise).expect("a thumbnail");
    let off = thumb.rgba.iter().zip(&choices.pictures[0]).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
    assert!(off <= 1, "the picture is {off} off the thumbnail");
    // Nothing a node is set to changes them: asking again gives the same pictures.
    set(&mut g, noise, "seed", ParamValue::Int(5));
    set(&mut g, noise, "scale", ParamValue::Int(3));
    evaluate_blocking(&mut e, &g);
    let again = e.render_choices(NodeKind::NOISE, "type").expect("pictures");
    assert!(std::sync::Arc::ptr_eq(&again.pictures, &choices.pictures), "drawn again for nothing");
    // Only choices have pictures, and only choices of nodes that draw from their parameters.
    assert!(e.render_choices(NodeKind::NOISE, "scale").is_none());
    assert!(e.render_choices(NodeKind::NOISE, "nothing").is_none());
    assert!(e.render_choices(NodeKind::BITMAP, "tiling").is_none());
    assert!(e.render_choices(NodeKind::SHAPE, "shape").is_some_and(|c| c.pictures.len() == brewed_texture_core::nodes::SHAPES.len()));
    assert!(e.gpu.take_errors().is_empty());
}

/// An image nothing reads is let go of once its thumbnail is drawn: a node no other node
/// takes and the 2D view does not show needs no more than its thumbnail. It is computed again
/// only when it changes or something comes to read it.
#[test]
fn images_nothing_reads_are_let_go_of() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let alone = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    let read = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    let blur = g.add_node(NodeKind::BLUR, [0.0, 0.0]);
    g.connect(read, "o:output", blur, "i:input").unwrap();
    evaluate_blocking(&mut e, &g);
    let thumbs = e.render_thumbnails();
    let before = e.memory();
    e.release_unread(&g);
    assert!(e.image(alone).is_none() && e.released(alone), "nothing reads it");
    assert!(e.image(read).is_some(), "the blur reads it");
    // The blur's own image is read by nothing either: its thumbnail is all that is left.
    assert!(e.image(blur).is_none());
    assert!(thumbs.iter().any(|t| t.node == alone), "its thumbnail was drawn first");
    assert_eq!(before - e.memory(), 2 * RES as u64 * RES as u64 * 2, "two grayscale images of 2 bytes a pixel");

    // Another node's change does not compute it again.
    set(&mut g, read, "seed", ParamValue::Int(4));
    evaluate_blocking(&mut e, &g);
    assert!(e.image(alone).is_none(), "computed for nothing");
    // Its own change does, for its thumbnail, and then it is let go of again.
    set(&mut g, alone, "seed", ParamValue::Int(4));
    evaluate_blocking(&mut e, &g);
    assert_eq!(e.report.recomputed, 1);
    assert!(e.render_thumbnails().iter().any(|t| t.node == alone));
    e.release_unread(&g);
    assert!(e.image(alone).is_none());
    // Shown in the 2D view, it is computed and kept.
    e.set_preview(Some(alone));
    evaluate_blocking(&mut e, &g);
    assert!(e.image(alone).is_some());
    e.render_thumbnails();
    e.release_unread(&g);
    assert!(e.image(alone).is_some(), "the 2D view reads it");
    // Read by another node, it is too.
    e.set_preview(None);
    let levels = g.add_node(NodeKind::LEVELS, [0.0, 0.0]);
    g.connect(alone, "o:output", levels, "i:input").unwrap();
    evaluate_blocking(&mut e, &g);
    e.render_thumbnails();
    e.release_unread(&g);
    assert!(e.image(alone).is_some(), "the levels read it");
}

/// What the demo takes in GPU memory at 2048×2048, as the app shows it (prints).
#[test]
#[ignore = "a measurement"]
fn demo_memory() {
    let Some(mut e) = engine() else { return };
    let (mut g, preview) = brewed_texture_core::demo::graph();
    g.resolution = [2048, 2048];
    evaluate_blocking(&mut e, &g);
    let outputs: Vec<NodeId> = g.nodes_of(NodeKind::OUTPUT).filter(|n| n.enum_value("usage") != 0).map(|n| n.id).collect();
    e.prepare_preview(preview);
    e.prepare_materials(&outputs);
    e.render_thumbnails();
    e.wait_idle();
    let mb = |b: u64| b as f64 / (1024.0 * 1024.0);
    println!("demo at 2048x2048: {:.0} MB, of which {:.0} MB scratch images", mb(e.memory()), mb(e.gpu.pool_bytes()));
    e.gpu.trim_pool();
    e.gpu.trim_pool();
    println!("without scratch images: {:.0} MB", mb(e.memory()));
}

/// Grayscale images are stored in one channel, and what arrives at an input of the other
/// kind is converted: color at a grayscale input is its luminance, grayscale at a color
/// input is gray in every channel.
#[test]
fn grayscale_and_color_images() {
    use brewed_texture_core::nodes::ImageKind;
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    let color = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
    set(&mut g, color, "color", ParamValue::Color([0.2, 0.6, 0.9, 1.0]));
    // Color into a grayscale input.
    let gray = g.add_node(NodeKind::GRAYSCALE, [0.0, 0.0]);
    g.connect(color, "o:output", gray, "i:input").unwrap();
    let scan = g.add_node(NodeKind::HISTOGRAM_SCAN, [0.0, 0.0]);
    set(&mut g, scan, "contrast", ParamValue::Float(0.0));
    g.connect(color, "o:output", scan, "i:input").unwrap();
    // Grayscale into a color input.
    let hsl = g.add_node(NodeKind::HSL, [0.0, 0.0]);
    g.connect(noise, "o:output", hsl, "i:input").unwrap();
    // A filter is what it is given.
    let blur = g.add_node(NodeKind::BLUR, [0.0, 0.0]);
    g.connect(noise, "o:output", blur, "i:input").unwrap();
    evaluate_blocking(&mut e, &g);

    let kind = |e: &Engine, id| e.image(id).map(|i| i.kind());
    assert_eq!(kind(&e, noise), Some(ImageKind::Gray));
    assert_eq!(kind(&e, blur), Some(ImageKind::Gray));
    assert_eq!(kind(&e, gray), Some(ImageKind::Gray));
    assert_eq!(kind(&e, color), Some(ImageKind::Color));
    assert_eq!(kind(&e, hsl), Some(ImageKind::Color));
    assert!(e.image(noise).unwrap().is_gray() && e.image(noise).unwrap().bytes() == u64::from(RES * RES * 2), "two bytes a pixel");
    // A uniform color is one pixel, and reads as the whole texture.
    assert_eq!(e.image(color).unwrap().size(), [1, 1]);
    let px = e.read_image(color).unwrap();
    assert_eq!((px.0, px.1), (RES, RES));
    let [r, g2, b, a] = pixel(&px.2, 200, 100);
    assert!(close(r, 0.2, 1e-3) && close(g2, 0.6, 1e-3) && close(b, 0.9, 1e-3) && a == 1.0, "{:?}", pixel(&px.2, 200, 100));

    let luminance = 0.2126 * 0.2 + 0.7152 * 0.6 + 0.0722 * 0.9;
    let out = e.read_image(gray).unwrap().2;
    assert!(close(pixel(&out, 7, 7)[0], luminance, 2e-3), "{:?}", pixel(&out, 7, 7));
    let noise_px = e.read_image(noise).unwrap().2;
    let hsl_px = e.read_image(hsl).unwrap().2;
    let [r, g2, b, a] = pixel(&hsl_px, 31, 17);
    assert!(close(r, pixel(&noise_px, 31, 17)[0], 2e-3) && close(r, g2, 2e-3) && close(g2, b, 2e-3) && a == 1.0);
    // The report tells the interface what each node makes.
    assert_eq!(e.reports()[&hsl].kind, Some(ImageKind::Color));
    assert!(e.gpu.take_errors().is_empty());
}

/// An output shows what arrives at it, as its format shows it, and keeps no image of its
/// own; so does a bypassed node, which passes on what arrives at it.
#[test]
fn outputs_and_bypassed_nodes_share_their_input() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    let invert = g.add_node(NodeKind::INVERT, [0.0, 0.0]);
    g.connect(noise, "o:output", invert, "i:input").unwrap();
    let out = g.add_node(NodeKind::OUTPUT, [0.0, 0.0]);
    g.connect(invert, "o:output", out, "i:input").unwrap();
    evaluate_blocking(&mut e, &g);
    let texture = |e: &Engine, id| e.image(id).map(|i| i.texture.clone());
    assert_eq!(texture(&e, out), texture(&e, invert), "the output is its input");
    let memory = e.memory();

    g.node_mut(invert).unwrap().bypass = true;
    evaluate_blocking(&mut e, &g);
    assert_eq!(texture(&e, invert), texture(&e, noise), "a bypassed node is its input");
    assert_eq!(texture(&e, out), texture(&e, noise));
    assert!(e.memory() < memory, "{} is not less than {memory}", e.memory());
    // Back again: the node makes an image of its own, and does not write into its input's.
    let before = e.read_image(noise).unwrap().2;
    g.node_mut(invert).unwrap().bypass = false;
    evaluate_blocking(&mut e, &g);
    assert_ne!(texture(&e, invert), texture(&e, noise));
    assert_eq!(e.read_image(noise).unwrap().2, before, "the input was written into");
    let inverted = e.read_image(out).unwrap().2;
    assert!(close(pixel(&inverted, 9, 9)[0], 1.0 - pixel(&before, 9, 9)[0], 2e-3));
}

/// The demo at a texture size of 512: what it takes in GPU memory with what the views
/// show, which was 2.4 times as much before grayscale images, outputs without copies of
/// their own, 8-bit copies for the views and one-pixel uniform colors.
#[test]
fn the_demo_takes_a_quarter_of_what_it_did() {
    let Some(mut e) = engine() else { return };
    let (mut g, preview) = brewed_texture_core::demo::graph();
    g.resolution = [512, 512];
    evaluate_blocking(&mut e, &g);
    let outputs: Vec<NodeId> = g.nodes_of(NodeKind::OUTPUT).filter(|n| n.enum_value("usage") != 0).map(|n| n.id).collect();
    e.prepare_preview(preview);
    e.prepare_materials(&outputs);
    // As the engine is once nothing changes for a moment: without its scratch images.
    e.gpu.clear_pool();
    let mb = e.memory() as f64 / (1024.0 * 1024.0);
    // 802 MB at 2048 was 50 MB at 512; 219 MB at 2048 is 13.7 at 512.
    assert!(mb < 14.0, "the demo takes {mb:.1} MB at 512");
}
