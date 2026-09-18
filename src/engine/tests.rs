//! GPU integration tests: build small graphs, evaluate them on the real GPU and
//! check pixel values. Skipped automatically when no adapter is available.

use super::*;
use crate::graph::{DataType, ParamValue};

const RES: u32 = 256;

fn engine() -> Option<Engine> {
    match Gpu::headless() {
        Ok(gpu) => Some(Engine::new(gpu, None)),
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
    e.evaluate(g);
    if let Some(err) = &e.state(id).unwrap().error {
        panic!("node error: {err}");
    }
    read_node(e, id).expect("node image")
}

#[test]
fn uniform_invert_blend_levels() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let c = g.add_node(NodeKind::UniformColor, [0.0, 0.0]);
    set(&mut g, c, "color", ParamValue::Color([0.25, 0.5, 0.75, 1.0]));
    let inv = g.add_node(NodeKind::Invert, [0.0, 0.0]);
    g.connect(c, "o:output", inv, "i:input").unwrap();

    let data = eval_read(&mut e, &g, c);
    assert_eq!(pixel(&data, 17, 99), [0.25, 0.5, 0.75, 1.0]);
    let data = eval_read(&mut e, &g, inv);
    assert_eq!(pixel(&data, 3, 3), [0.75, 0.5, 0.25, 1.0]);

    // Multiply blend: 0.5 * 0.5 = 0.25
    let half = g.add_node(NodeKind::UniformColor, [0.0, 0.0]);
    set(&mut g, half, "color", ParamValue::Color([0.5, 0.5, 0.5, 1.0]));
    let blend = g.add_node(NodeKind::Blend, [0.0, 0.0]);
    set(&mut g, blend, "mode", ParamValue::Enum(3));
    g.connect(half, "o:output", blend, "i:foreground").unwrap();
    g.connect(half, "o:output", blend, "i:background").unwrap();
    let data = eval_read(&mut e, &g, blend);
    assert_eq!(pixel(&data, 100, 100), [0.25, 0.25, 0.25, 1.0]);

    // Levels: input range [0.25, 0.75] maps 0.5 -> 0.5; gamma 2 maps 0.25 -> 0.5.
    let lv = g.add_node(NodeKind::Levels, [0.0, 0.0]);
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
    let noise = g.add_node(NodeKind::Noise, [0.0, 0.0]);
    let t = g.add_node(NodeKind::Transform2D, [0.0, 0.0]);
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
    let noise = g.add_node(NodeKind::Noise, [0.0, 0.0]);
    set(&mut g, noise, "type", ParamValue::Enum(5)); // white noise
    let blur = g.add_node(NodeKind::Blur, [0.0, 0.0]);
    set(&mut g, blur, "radius", ParamValue::Float(100.0)); // exercises the pyramid path
    g.connect(noise, "o:output", blur, "i:input").unwrap();
    let src = eval_read(&mut e, &g, noise);
    let out = eval_read(&mut e, &g, blur);
    let mean = |d: &[f32]| d.iter().step_by(4).map(|v| *v as f64).sum::<f64>() / (RES * RES) as f64;
    let (a, b) = (mean(&src), mean(&out));
    assert!((a - b).abs() < 0.01, "mean {a} vs {b}");
    // And it is actually smooth.
    let var =
        |d: &[f32], m: f64| d.iter().step_by(4).map(|v| (*v as f64 - m).powi(2)).sum::<f64>() / (RES * RES) as f64;
    assert!(var(&out, b) < var(&src, a) * 0.05);
}

#[test]
fn output_format_single_channel() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let c = g.add_node(NodeKind::UniformColor, [0.0, 0.0]);
    set(&mut g, c, "color", ParamValue::Color([0.25, 0.5, 0.75, 0.5]));
    let o = g.add_node(NodeKind::Output, [0.0, 0.0]);
    g.connect(c, "o:output", o, "i:input").unwrap();
    set(&mut g, o, "format", ParamValue::Enum(4)); // G
    let data = eval_read(&mut e, &g, o);
    assert_eq!(pixel(&data, 5, 5), [0.5, 0.5, 0.5, 1.0]);
    set(&mut g, o, "format", ParamValue::Enum(2)); // RG
    let data = eval_read(&mut e, &g, o);
    assert_eq!(pixel(&data, 5, 5), [0.25, 0.5, 0.0, 1.0]);
}

#[test]
fn math_and_inputs_drive_exposed_parameters() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let input = g.add_input(DataType::Float, "brightness");
    g.input_mut(input).unwrap().value = Value::Float(3.0);
    let math = g.add_node(NodeKind::Math, [0.0, 0.0]);
    set(&mut g, math, "expression", ParamValue::Text("brightness / 4 - x".into()));
    g.node_mut(math).unwrap().var_values.insert("x".into(), 0.5);
    let color = g.add_node(NodeKind::UniformColor, [0.0, 0.0]);
    g.set_exposed(color, "color", true);
    g.connect(math, "o:result", color, "p:color").unwrap();
    // brightness is auto-bound by name: 3/4 - 0.5 = 0.25 -> gray color
    let data = eval_read(&mut e, &g, color);
    assert_eq!(pixel(&data, 1, 1), [0.25, 0.25, 0.25, 1.0]);
    assert_eq!(e.value(math), Some(Value::Float(0.25)));

    // A broken expression reports an error instead of crashing, and the driven
    // parameter falls back to its own value.
    set(&mut g, math, "expression", ParamValue::Text("sin(".into()));
    e.evaluate(&g);
    assert!(e.state(math).unwrap().error.as_deref().unwrap().contains("incorrect"));
    let data = read_node(&e, color).unwrap();
    assert_eq!(pixel(&data, 1, 1), [0.5, 0.5, 0.5, 1.0]);
}

#[test]
fn shape_and_pixel_math() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let shape = g.add_node(NodeKind::Shape, [0.0, 0.0]);
    let data = eval_read(&mut e, &g, shape);
    assert!(pixel(&data, RES / 2, RES / 2)[0] > 0.99);
    assert!(pixel(&data, 2, 2)[0] < 0.01);

    let pm = g.add_node(NodeKind::PixelMath, [0.0, 0.0]);
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
    let a = g.add_node(NodeKind::Noise, [0.0, 0.0]);
    let b = g.add_node(NodeKind::Levels, [0.0, 0.0]);
    let c = g.add_node(NodeKind::Invert, [0.0, 0.0]);
    let d = g.add_node(NodeKind::Shape, [0.0, 0.0]);
    g.connect(a, "o:output", b, "i:input").unwrap();
    g.connect(b, "o:output", c, "i:input").unwrap();
    e.evaluate(&g);
    assert_eq!(e.report.recomputed, 4);
    e.evaluate(&g);
    assert_eq!(e.report.recomputed, 0);
    set(&mut g, b, "gamma", ParamValue::Float(2.0));
    e.evaluate(&g);
    assert_eq!(e.report.recomputed, 2); // levels + invert, not noise or shape
    g.resolution = [128, 128];
    e.evaluate(&g);
    assert_eq!(e.report.recomputed, 4);
    let _ = d;
}

#[test]
fn every_image_node_runs_with_defaults() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let src = g.add_node(NodeKind::Noise, [0.0, 0.0]);
    let mut ids = Vec::new();
    for d in crate::graph::defs::DEFS.iter().filter(|d| d.has_image) {
        let id = g.add_node(d.kind, [0.0, 0.0]);
        for s in d.image_inputs {
            let _ = g.connect(src, "o:output", id, &crate::graph::image_socket_key(s.key));
        }
        ids.push((id, d.name));
    }
    // Pixel Math default expression "u * v" and every tiling mode.
    for tiling in 0..4 {
        for (id, _) in &ids {
            set(&mut g, *id, "tiling", ParamValue::Enum(tiling));
        }
        e.evaluate(&g);
        for (id, name) in &ids {
            let st = e.state(*id).unwrap();
            if *name == "Bitmap" {
                assert!(st.error.is_some()); // no file selected
                continue;
            }
            assert!(st.error.is_none(), "{name}: {:?}", st.error);
            assert!(st.image.is_some(), "{name}");
        }
    }
    assert!(e.gpu.take_errors().is_empty());
}

#[test]
fn bitmap_node_decodes_and_resamples() {
    let Some(mut e) = engine() else { return };
    let dir = std::env::temp_dir().join(format!("ferrotex_bitmap_{}", std::process::id()));
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
    let b = g.add_node(NodeKind::Bitmap, [0.0, 0.0]);
    set(&mut g, b, "tiling", ParamValue::Enum(0)); // clamp at the borders
    set(&mut g, b, "path", ParamValue::Text(path.display().to_string()));
    // Decoding happens on a worker thread; keep evaluating until it lands.
    let mut ready = false;
    for _ in 0..300 {
        e.evaluate(&g);
        let st = e.state(b).unwrap();
        assert!(st.error.is_none(), "{:?}", st.error);
        if st.warning.is_none() {
            ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(ready, "bitmap never finished loading");
    let data = read_node(&e, b).unwrap();
    // Centers of texel (1, 0) (green) and (2, 1) (yellow) in the 256x256 output.
    let px = |tx: f32, ty: f32| pixel(&data, ((tx + 0.5) / 4.0 * RES as f32) as u32, ((ty + 0.5) / 2.0 * RES as f32) as u32);
    let green = px(1.0, 0.0);
    let yellow = px(2.0, 1.0);
    assert!(close(green[0], 0.0, 0.02) && close(green[1], 1.0, 0.02), "{green:?}");
    assert!(close(yellow[0], 1.0, 0.02) && close(yellow[1], 1.0, 0.02) && close(yellow[2], 0.0, 0.02), "{yellow:?}");

    // A missing file is an error on the node, not a crash.
    set(&mut g, b, "path", ParamValue::Text(dir.join("missing.png").display().to_string()));
    let mut error = None;
    for _ in 0..300 {
        e.evaluate(&g);
        error = e.state(b).unwrap().error.clone();
        if error.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(error.as_deref().is_some_and(|m| m.contains("Cannot open")), "{error:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
