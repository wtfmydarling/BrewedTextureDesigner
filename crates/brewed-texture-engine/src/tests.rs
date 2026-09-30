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

/// An output writes the channels of its format at its bit depth: one channel for a single
/// one, two for RG, four for RGB and RGBA; 8 bits hold a value to the nearest of 255 steps,
/// where 16 keep it.
#[test]
fn outputs_write_their_channels_at_their_bit_depth() {
    use wgpu::TextureFormat as F;
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let c = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
    set(&mut g, c, "color", ParamValue::Color([0.25, 0.6, 0.75, 0.4]));
    let o = g.add_node(NodeKind::OUTPUT, [0.0, 0.0]);
    g.connect(c, "o:output", o, "i:input").unwrap();
    // A uniform color is one pixel, and so is what an output writes of it: that pixel, as
    // the output keeps it.
    let written = |e: &mut Engine, g: &Graph| {
        evaluate_blocking(e, g);
        let image = e.image(o).expect("the output's image");
        assert_eq!(image.size(), [1, 1]);
        let px = e.gpu.read_image_blocking(image).unwrap();
        ([px[0], px[1], px[2], px[3]], image.format())
    };
    let step = |v: f32| (v * 255.0).round() / 255.0;
    set(&mut g, o, "format", ParamValue::Enum(4)); // G
    assert_eq!(written(&mut e, &g), ([step(0.6), step(0.6), step(0.6), 1.0], F::R8Unorm));
    set(&mut g, o, "format", ParamValue::Enum(2)); // RG
    assert_eq!(written(&mut e, &g), ([step(0.25), step(0.6), 0.0, 1.0], F::Rg8Unorm));
    set(&mut g, o, "format", ParamValue::Enum(1)); // RGB
    assert_eq!(written(&mut e, &g), ([step(0.25), step(0.6), step(0.75), 1.0], F::Rgba8Unorm));
    set(&mut g, o, "bit_depth", ParamValue::Enum(1)); // 16 bit
    let (px, format) = written(&mut e, &g);
    assert_eq!(format, F::Rgba16Float);
    for (v, w) in px.iter().zip([0.25, 0.6, 0.75, 1.0]) {
        assert!(close(*v, w, 1e-3), "{px:?}");
    }
    // Spread over the texture size, as it is exported.
    let data = eval_read(&mut e, &g, o);
    assert!(close(pixel(&data, 5, 5)[1], 0.6, 1e-3));
    // What arrives gray is one channel in RGB too, as precisely as it arrives: in half floats,
    // and in floats from a node that keeps its image in floats.
    let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    g.connect(noise, "o:output", o, "i:input").unwrap();
    evaluate_blocking(&mut e, &g);
    assert_eq!(e.image(o).unwrap().format(), F::R16Float);
    set(&mut g, noise, "precision", ParamValue::Enum(3));
    evaluate_blocking(&mut e, &g);
    assert_eq!(e.image(o).unwrap().format(), F::R32Float);
    set(&mut g, noise, "precision", ParamValue::Enum(0));
    assert_eq!(e.image(o).unwrap().size(), [RES, RES]);
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

/// Pixel Math makes a color image of a list of values: red, green, blue and alpha, those not
/// given none and opaque; one value stays a grayscale image.
#[test]
fn pixel_math_makes_colors_of_lists() {
    use crate::gpu::{COLOR_FORMAT, GRAY_FORMAT};
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let pm = g.add_node(NodeKind::PIXEL_MATH, [0.0, 0.0]);
    set(&mut g, pm, "expression", ParamValue::Text("u, v, 0.25".into()));
    let rgb = eval_read(&mut e, &g, pm);
    assert_eq!(e.image(pm).unwrap().format(), COLOR_FORMAT);
    let (x, y) = (RES / 4, 3 * RES / 4);
    let (u, v) = ((x as f32 + 0.5) / RES as f32, (y as f32 + 0.5) / RES as f32);
    let p = pixel(&rgb, x, y);
    assert!(close(p[0], u, 2e-3) && close(p[1], v, 2e-3) && close(p[2], 0.25, 1e-3) && p[3] == 1.0, "{p:?}");
    set(&mut g, pm, "expression", ParamValue::Text("1, 0.5, 0, 0.5".into()));
    let p = pixel(&eval_read(&mut e, &g, pm), 9, 9);
    assert_eq!(p, [1.0, 0.5, 0.0, 0.5]);
    set(&mut g, pm, "expression", ParamValue::Text("u".into()));
    let _ = eval_read(&mut e, &g, pm);
    assert_eq!(e.image(pm).unwrap().format(), GRAY_FORMAT);
    assert!(e.gpu.take_errors().is_empty());
}

/// Pixel Math reads an image anywhere with `sample`: mirrored across, and a neighbour's
/// difference (an edge), each pixel what the image has where it says; a value not connected
/// to an image reads the same everywhere.
#[test]
fn pixel_math_samples_images_anywhere() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let ramp = g.add_node(NodeKind::GRADIENT, [0.0, 0.0]);
    let pm = g.add_node(NodeKind::PIXEL_MATH, [0.0, 0.0]);
    set(&mut g, pm, "expression", ParamValue::Text("sample(img, 1 - u, v)".into()));
    g.connect(ramp, "o:output", pm, "v:img").unwrap();
    let before = eval_read(&mut e, &g, ramp);
    let mirrored = eval_read(&mut e, &g, pm);
    for x in [0, 3, RES / 4, RES / 2 + 7, RES - 1] {
        let (got, want) = (pixel(&mirrored, x, 11)[0], pixel(&before, RES - 1 - x, 11)[0]);
        assert!(close(got, want, 2e-3), "x {x}: {got}, not {want}");
    }
    // One pixel to the right, less this one: the ramp's step everywhere but at its seam.
    set(&mut g, pm, "expression", ParamValue::Text("(sample(img, u + 1 / width, v) - img) * width".into()));
    let slope = eval_read(&mut e, &g, pm);
    assert!(close(pixel(&slope, RES / 3, 40)[0], 1.0, 0.05), "{}", pixel(&slope, RES / 3, 40)[0]);
    // A value is itself wherever it is read.
    set(&mut g, pm, "expression", ParamValue::Text("sample(k, 0.9, 0.1)".into()));
    g.node_mut(pm).unwrap().var_values.insert("k".into(), 0.375);
    let value = eval_read(&mut e, &g, pm);
    assert!(close(pixel(&value, 5, 5)[0], 0.375, 1e-3));
    assert!(e.gpu.take_errors().is_empty());
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
            // (They read files, and none is selected.)
            if *name == "Bitmap" || *name == "SVG" {
                assert!(st.error.is_some(), "no file selected");
                continue;
            }
            assert!(st.error.is_none(), "{name}: {:?}", st.error);
            assert!(ev.node(*id).unwrap().error().is_none(), "{name}");
            assert!(e.image(*id).is_some(), "{name}");
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

/// A Curve maps each value through its curve: the straight line leaves an image as it is, a
/// curve through a point lifts the middle as far as it says; on a color image, only the
/// channels chosen change.
#[test]
fn curve_maps_values() {
    use brewed_texture_core::Curve;
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let ramp = g.add_node(NodeKind::GRADIENT, [0.0, 0.0]);
    let curve = g.add_node(NodeKind::CURVE, [0.0, 0.0]);
    g.connect(ramp, "o:output", curve, "i:input").unwrap();
    let before = eval_read(&mut e, &g, ramp);
    let straight = eval_read(&mut e, &g, curve);
    assert!(before.iter().zip(&straight).step_by(4).all(|(a, b)| close(*a, *b, 0.002)));
    let lifted = Curve::from_points(&[[0.0, 0.0], [0.5, 0.8], [1.0, 1.0]]);
    set(&mut g, curve, "curve", ParamValue::Curve(lifted.clone()));
    let after = eval_read(&mut e, &g, curve);
    for x in [0, RES / 4, RES / 2, RES - 1] {
        let (v, got) = (pixel(&before, x, 9)[0], pixel(&after, x, 9)[0]);
        assert!(close(got, lifted.sample(v), 0.003), "{v} → {got}, not {}", lifted.sample(v));
    }
    // On a color image, the red only.
    let color = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
    set(&mut g, color, "color", ParamValue::Color([0.5, 0.5, 0.5, 1.0]));
    g.connect(color, "o:output", curve, "i:input").unwrap();
    set(&mut g, curve, "channels", ParamValue::Enum(1));
    let p = pixel(&eval_read(&mut e, &g, curve), 3, 3);
    assert!(close(p[0], 0.8, 0.003) && close(p[1], 0.5, 0.002) && close(p[2], 0.5, 0.002), "{p:?}");
    assert!(e.gpu.take_errors().is_empty());
}

/// Gradient Dynamic reads its colors along a line of an image: a value picks the color at its
/// place along the row, or along the column at Position, or where a Position Map says.
#[test]
fn gradient_dynamic_reads_colors_from_an_image() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    // The gradient: a ramp from black on the left to white on the right.
    let ramp = g.add_node(NodeKind::GRADIENT, [0.0, 0.0]);
    let quarter = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
    set(&mut g, quarter, "color", ParamValue::Color([0.25, 0.25, 0.25, 1.0]));
    let dynamic = g.add_node(NodeKind::GRADIENT_DYNAMIC, [0.0, 0.0]);
    g.connect(quarter, "o:output", dynamic, "i:input").unwrap();
    g.connect(ramp, "o:output", dynamic, "i:gradient").unwrap();
    let along = pixel(&eval_read(&mut e, &g, dynamic), 40, 70);
    assert!(close(along[0], 0.25, 0.01) && close(along[2], 0.25, 0.01), "{along:?}");
    // Down a column: the column at Position, whatever the value.
    set(&mut g, dynamic, "orientation", ParamValue::Enum(1));
    set(&mut g, dynamic, "position", ParamValue::Float(0.75));
    let down = pixel(&eval_read(&mut e, &g, dynamic), 40, 70);
    assert!(close(down[0], 0.75, 0.01), "{down:?}");
    // The column a Position Map says: here, as the map is.
    let map = g.add_node(NodeKind::GRADIENT, [0.0, 0.0]);
    set(&mut g, map, "angle", ParamValue::Float(90.0));
    g.connect(map, "o:output", dynamic, "i:position_map").unwrap();
    let mapped = eval_read(&mut e, &g, dynamic);
    let map_values = eval_read(&mut e, &g, map);
    for (x, y) in [(10, 10), (100, 50), (200, 240), (128, 128)] {
        let (got, want) = (pixel(&mapped, x, y)[0], pixel(&map_values, x, y)[0]);
        assert!(close(got, want, 0.01), "({x}, {y}): {got}, not {want}");
    }
    assert!(e.gpu.take_errors().is_empty());
}

/// RGBA Merge packs grayscale maps into the channels of one image (occlusion, roughness and
/// metallic, as engines take them): each channel its map, or its value with none; a color
/// image at a channel is its luminance.
#[test]
fn rgba_merge_packs_channels() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let gray = |g: &mut Graph, v: f32| {
        let id = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
        set(g, id, "color", ParamValue::Color([v, v, v, 1.0]));
        let to_gray = g.add_node(NodeKind::GRAYSCALE, [0.0, 0.0]);
        g.connect(id, "o:output", to_gray, "i:input").unwrap();
        to_gray
    };
    let (occlusion, roughness) = (gray(&mut g, 0.25), gray(&mut g, 0.5));
    let merge = g.add_node(NodeKind::RGBA_MERGE, [0.0, 0.0]);
    g.connect(occlusion, "o:output", merge, "i:red").unwrap();
    g.connect(roughness, "o:output", merge, "i:green").unwrap();
    set(&mut g, merge, "blue_value", ParamValue::Float(0.75));
    let packed = eval_read(&mut e, &g, merge);
    let p = pixel(&packed, 7, 200);
    assert!(close(p[0], 0.25, 0.002) && close(p[1], 0.5, 0.002) && close(p[2], 0.75, 0.002) && close(p[3], 1.0, 0.002), "{p:?}");
    // A color image at Alpha: its luminance.
    let red = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
    set(&mut g, red, "color", ParamValue::Color([1.0, 0.0, 0.0, 1.0]));
    g.connect(red, "o:output", merge, "i:alpha").unwrap();
    let p = pixel(&eval_read(&mut e, &g, merge), 7, 200);
    assert!(close(p[3], 0.2126, 0.002), "{p:?}");
    assert!(e.gpu.take_errors().is_empty());
}

/// Tile Sampler: squares in every cell, tiling; tiles that stay only where the Mask Map says,
/// shrink where the Scale Map is dark, take the color of the Color Map or colors of their own,
/// one of several patterns each; a pattern much larger than its tiles read from its mips (no
/// shimmer); and tiles stacked one over another rather than merged.
#[test]
fn tile_sampler_places_patterns_by_maps() {
    use crate::gpu::{COLOR_FORMAT, GRAY_FORMAT};
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let t = g.add_node(NodeKind::TILE_SAMPLER, [0.0, 0.0]);
    let covered = |px: &[f32], x: std::ops::Range<u32>| {
        let n = (0..RES).flat_map(|y| x.clone().map(move |x| (x, y))).filter(|&(x, y)| pixel(px, x, y)[0] > 0.5).count();
        n as f32 / (x.len() as u32 * RES) as f32
    };
    // Squares of 0.8 of their cells: 64 % of the image, and it tiles.
    let plain = eval_read(&mut e, &g, t);
    assert_eq!(e.image(t).unwrap().format(), GRAY_FORMAT);
    assert!((covered(&plain, 0..RES) - 0.64).abs() < 0.03, "{}", covered(&plain, 0..RES));
    let (across, inside) = seams(&plain, RES);
    assert!(across <= inside * 1.25 + 1e-3, "{across} across the edges, at most {inside} anywhere else");

    // A mask map dark on the left, light on the right: the tiles stay on the right only, or,
    // inverted, on the left.
    let ramp = g.add_node(NodeKind::GRADIENT, [0.0, 0.0]);
    g.connect(ramp, "o:output", t, "i:mask_map").unwrap();
    let masked = eval_read(&mut e, &g, t);
    assert!(covered(&masked, 0..RES / 2) < 0.01 && covered(&masked, RES / 2..RES) > 0.6, "{} {}", covered(&masked, 0..RES / 2), covered(&masked, RES / 2..RES));
    set(&mut g, t, "mask_invert", ParamValue::Bool(true));
    let inverted = eval_read(&mut e, &g, t);
    assert!(covered(&inverted, 0..RES / 2) > 0.6 && covered(&inverted, RES / 2..RES) < 0.01);
    g.disconnect(t, "i:mask_map");

    // The same ramp as the scale: small tiles on the left, nearly whole ones on the right (the
    // two columns there scaled by 0.81 and 0.94: 49 % of it).
    g.connect(ramp, "o:output", t, "i:scale_map").unwrap();
    let scaled = eval_read(&mut e, &g, t);
    let (left, right) = (covered(&scaled, 0..RES / 4), covered(&scaled, 3 * RES / 4..RES));
    assert!(left < 0.05 && (right - 0.49).abs() < 0.04, "{left} on the left, {right} on the right");
    g.disconnect(t, "i:scale_map");

    // The color of a color map, or colors of their own.
    let red = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
    set(&mut g, red, "color", ParamValue::Color([1.0, 0.1, 0.0, 1.0]));
    g.connect(red, "o:output", t, "i:color_map").unwrap();
    set(&mut g, t, "color", ParamValue::Enum(2));
    let colored = eval_read(&mut e, &g, t);
    assert_eq!(e.image(t).unwrap().format(), COLOR_FORMAT);
    let middle = RES / 16;
    let c = pixel(&colored, middle, middle);
    assert!(close(c[0], 1.0, 0.01) && close(c[1], 0.1, 0.01) && close(c[2], 0.0, 0.01), "{c:?}");
    set(&mut g, t, "color", ParamValue::Enum(1));
    let own = eval_read(&mut e, &g, t);
    let colors: std::collections::BTreeSet<[u32; 3]> =
        (0..8).map(|k| pixel(&own, middle + k * RES / 8, middle)).map(|c| [c[0], c[1], c[2]].map(|v| (v * 20.0) as u32)).collect();
    assert!(colors.len() >= 4, "{colors:?}");
    set(&mut g, t, "color", ParamValue::Enum(0));

    // Two patterns, one of them in each tile.
    let (white, dark) = (g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]), g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]));
    set(&mut g, white, "color", ParamValue::Color([1.0; 4]));
    set(&mut g, dark, "color", ParamValue::Color([0.25, 0.25, 0.25, 1.0]));
    g.connect(white, "o:output", t, "i:pattern_1").unwrap();
    g.connect(dark, "o:output", t, "i:pattern_3").unwrap();
    set(&mut g, t, "pattern", ParamValue::Enum(brewed_texture_core::nodes::TILE_PATTERN_IMAGE));
    let two = eval_read(&mut e, &g, t);
    let centers: Vec<f32> = (0..64).map(|k| pixel(&two, middle + (k % 8) * RES / 8, middle + (k / 8) * RES / 8)[0]).collect();
    assert!(centers.iter().all(|v| close(*v, 1.0, 0.01) || close(*v, 0.25, 0.01)), "{centers:?}");
    assert!(centers.iter().any(|v| close(*v, 1.0, 0.01)) && centers.iter().any(|v| close(*v, 0.25, 0.01)));
    g.disconnect(t, "i:pattern_3");

    // A fine checker as the pattern of tiles 8 pixels wide: each tile the gray it averages to,
    // not a shimmer of its black and white pixels.
    let checker = g.add_node(NodeKind::TILE_GENERATOR, [0.0, 0.0]);
    for (key, value) in [("x_amount", 64), ("y_amount", 64)] {
        set(&mut g, checker, key, ParamValue::Int(value));
    }
    set(&mut g, checker, "size", ParamValue::Vec2([0.5, 1.0]));
    g.connect(checker, "o:output", t, "i:pattern_1").unwrap();
    for (key, value) in [("x_amount", 32), ("y_amount", 32)] {
        set(&mut g, t, key, ParamValue::Int(value));
    }
    set(&mut g, t, "scale", ParamValue::Float(1.0));
    let fine = eval_read(&mut e, &g, t);
    let inner: Vec<f32> =
        (0..RES).flat_map(|y| (0..RES).map(move |x| (x, y))).filter(|(x, y)| (x % 8).abs_diff(4) < 3 && (y % 8).abs_diff(4) < 3).map(|(x, y)| pixel(&fine, x, y)[0]).collect();
    let m = inner.iter().sum::<f32>() / inner.len() as f32;
    let spread = (inner.iter().map(|v| (v - m).powi(2)).sum::<f32>() / inner.len() as f32).sqrt();
    assert!((m - 0.5).abs() < 0.1 && spread < 0.1, "tiles of {m} ± {spread}");
    g.disconnect(t, "i:pattern_1");

    // Squares larger than their cells, of random luminance: merged, the brighter shows where
    // they overlap; stacked, the one on top, which is sometimes the darker.
    set(&mut g, t, "pattern", ParamValue::Enum(1));
    set(&mut g, t, "scale", ParamValue::Float(1.6));
    set(&mut g, t, "random_luminance", ParamValue::Float(1.0));
    let merged = eval_read(&mut e, &g, t);
    set(&mut g, t, "blend_mode", ParamValue::Enum(2));
    let stacked = eval_read(&mut e, &g, t);
    // Where two neighbours both cover a pixel whole (on the line between their cells, level
    // with their middles): merged, the brighter; stacked, one of the two, often the darker.
    let cell = RES / 32;
    let (mut darker, mut checked) = (0, 0);
    for (x, y) in (0..32).flat_map(|i| (0..32).map(move |j| (i * cell, j * cell + cell / 2))) {
        let (m, s) = (pixel(&merged, x, y)[0], pixel(&stacked, x, y)[0]);
        let (left, right) = (pixel(&merged, (x + RES - cell / 2) % RES, y)[0], pixel(&merged, x + cell / 2, y)[0]);
        assert!(close(m, left.max(right), 0.01), "merged: {m}, of {left} and {right}");
        assert!(close(s, left, 0.01) || close(s, right, 0.01), "stacked: {s}, of {left} and {right}");
        checked += 1;
        darker += usize::from(s < m - 0.05);
    }
    assert!(darker > checked / 5, "{darker} of {checked} places where a tile lies over a brighter one");
    assert!(e.gpu.take_errors().is_empty());
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

/// A graph used as a node computes what its nodes compute in the graph that uses it: laid out
/// there (see `brewed_texture_core::package::flatten`), pixel for pixel, the node showing its
/// first output. What arrives at a grayscale Image Input is made grayscale; an input nothing
/// arrives at is its default color.
#[test]
fn a_graph_used_as_a_node_computes_what_its_nodes_do() {
    use brewed_texture_core::package::{Library, Package, flatten};
    use brewed_texture_core::{GraphRef, Instance, Interface};
    use std::sync::Arc;

    struct One(Package);
    impl Library for One {
        fn package(&self, path: Option<&str>) -> Option<&Package> {
            path.is_none().then_some(&self.0)
        }
    }

    let Some(mut e) = engine() else { return };
    // The graph used: blurs what arrives, by an input value.
    let mut soft = graph();
    let input = soft.add_node(NodeKind::IMAGE_INPUT, [0.0, 0.0]);
    let blur = soft.add_node(NodeKind::BLUR, [200.0, 0.0]);
    let out = soft.add_node(NodeKind::OUTPUT, [400.0, 0.0]);
    let radius = soft.add_node(NodeKind::FLOAT_INPUT, [0.0, 200.0]);
    soft.bind_variable(radius, "radius").unwrap();
    soft.variable_mut("radius").unwrap().max = Value::Float(40.0);
    soft.set_exposed(blur, "radius", true).unwrap();
    soft.connect(input, "o:output", blur, "i:input").unwrap();
    soft.connect(radius, "o:value", blur, "p:radius").unwrap();
    soft.connect(blur, "o:output", out, "i:input").unwrap();

    // The graph that uses it: a noise at its input, its radius at 12.
    let mut package = Package::new("Main", graph());
    let used = package.add_graph("Soft", soft);
    let main = package.first();
    let interface = Interface::of(&package.graph(used).unwrap().graph);
    let instance = Instance::new(GraphRef { package: None, graph: used, name: "Soft".into() }, interface);
    let g = Arc::make_mut(&mut package.graph_mut(main).unwrap().graph);
    let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    let node = g.add_instance(instance.clone(), [200.0, 0.0]);
    let alone = g.add_instance(instance, [200.0, 300.0]);
    g.node_mut(node).unwrap().param_mut("radius").unwrap().value = ParamValue::Float(12.0);
    g.set_exposed(node, "input", true).unwrap();
    g.connect(noise, "o:output", node, "p:input").unwrap();
    let flat = flatten(&One(package), None, main).unwrap();
    assert!(flat.problems.is_empty(), "{:?}", flat.problems);
    let got = eval_read(&mut e, &flat.graph, node);

    // The same nodes, in one graph.
    let mut same = graph();
    let n = same.add_node(NodeKind::NOISE, [0.0, 0.0]);
    let gray = same.add_node(NodeKind::GRAYSCALE, [0.0, 0.0]);
    let b = same.add_node(NodeKind::BLUR, [0.0, 0.0]);
    set(&mut same, b, "radius", ParamValue::Float(12.0));
    same.connect(n, "o:output", gray, "i:input").unwrap();
    same.connect(gray, "o:output", b, "i:input").unwrap();
    let want = eval_read(&mut e, &same, b);
    assert_eq!(got, want);

    // Nothing arrives at the other: it blurs the input's default gray (0.5), which stays 0.5.
    let flat_again = eval_read(&mut e, &flat.graph, alone);
    assert!(flat_again.chunks(4).all(|p| close(p[0], 0.5, 1e-3)), "{:?}", pixel(&flat_again, 7, 7));
}

/// A graph that mixes much of what a graph made of some of its nodes has to keep as it was:
/// color arriving at grayscale inputs, one image going to sockets of both kinds, a channel of a
/// color image read, an input value read by name and through a socket, images read at every
/// scale and anywhere (see `nodes_made_a_graph_of_their_own_compute_what_they_did`). With
/// its nodes by name.
fn mixed() -> (Graph, Vec<(&'static str, NodeId)>) {
    let mut g = graph();
    let mut at = 0.0;
    let mut add = |g: &mut Graph, kind: NodeKind| {
        at += 120.0;
        g.add_node(kind, [at, (at * 7.0) % 500.0])
    };
    let link = |g: &mut Graph, a: NodeId, sa: &str, b: NodeId, sb: &str| {
        g.connect(a, sa, b, sb).unwrap_or_else(|e| panic!("{sa} -> {sb}: {e}"));
    };
    let noise = add(&mut g, NodeKind::NOISE);
    let color = add(&mut g, NodeKind::GRADIENT_MAP);
    link(&mut g, noise, "o:output", color, "i:input");
    let warm = add(&mut g, NodeKind::HSL);
    set(&mut g, warm, "hue", ParamValue::Float(40.0));
    link(&mut g, color, "o:output", warm, "i:input");
    // One color image into a grayscale mask and into a filter of either kind.
    let blend = add(&mut g, NodeKind::BLEND);
    set(&mut g, blend, "mode", ParamValue::Enum(1));
    link(&mut g, noise, "o:output", blend, "i:foreground");
    link(&mut g, color, "o:output", blend, "i:background");
    link(&mut g, warm, "o:output", blend, "i:mask");
    let blur = add(&mut g, NodeKind::BLUR);
    link(&mut g, warm, "o:output", blur, "i:input");
    // The red of a color image.
    let red = add(&mut g, NodeKind::GRADIENT_MAP);
    set(&mut g, red, "channel", ParamValue::Enum(1));
    link(&mut g, blur, "o:output", red, "i:input");
    // A color height.
    let normal = add(&mut g, NodeKind::NORMAL);
    link(&mut g, blend, "o:output", normal, "i:input");
    // An input value, read by name by an expression and through a socket by a filter.
    let amount = add(&mut g, NodeKind::FLOAT_INPUT);
    g.bind_variable(amount, "amount").unwrap();
    g.variable_mut("amount").unwrap().value = Value::Float(0.3);
    let pm = add(&mut g, NodeKind::PIXEL_MATH);
    set(&mut g, pm, "expression", ParamValue::Text("sample(img, v, u) * (1 - amount) + h * amount".into()));
    link(&mut g, blend, "o:output", pm, "v:img");
    link(&mut g, noise, "o:output", pm, "v:h");
    let levels = add(&mut g, NodeKind::LEVELS);
    g.set_exposed(levels, "out_high", true).unwrap();
    link(&mut g, amount, "o:value", levels, "p:out_high");
    link(&mut g, pm, "o:output", levels, "i:input");
    // Read at every scale: a blur by a map, and tiles of a pattern.
    let soft = add(&mut g, NodeKind::NON_UNIFORM_BLUR);
    link(&mut g, levels, "o:output", soft, "i:input");
    link(&mut g, noise, "o:output", soft, "i:map");
    let tiles = add(&mut g, NodeKind::TILE_SAMPLER);
    set(&mut g, tiles, "pattern", ParamValue::Enum(brewed_texture_core::nodes::TILE_PATTERN_IMAGE));
    set(&mut g, tiles, "random_rotation", ParamValue::Float(90.0));
    link(&mut g, red, "o:output", tiles, "i:pattern_1");
    link(&mut g, soft, "o:output", tiles, "i:scale_map");
    for (from, name) in [(normal, "normal"), (tiles, "tiles"), (soft, "soft"), (red, "red"), (blend, "blend")] {
        let out = add(&mut g, NodeKind::OUTPUT);
        set(&mut g, out, "name", ParamValue::Text(name.into()));
        link(&mut g, from, "o:output", out, "i:input");
    }
    let named = vec![
        ("noise", noise),
        ("color", color),
        ("warm", warm),
        ("blend", blend),
        ("blur", blur),
        ("red", red),
        ("normal", normal),
        ("amount", amount),
        ("pm", pm),
        ("levels", levels),
        ("soft", soft),
        ("tiles", tiles),
    ];
    (g, named)
}

/// Nodes put in a graph of their own compute what they did (see
/// `brewed_texture_core::package::graph_from_selection`): every output of the demo and of a graph that
/// mixes much (see `mixed`), pixel for pixel, whichever nodes go, and whichever of those go in
/// a graph of their own again: nodes chosen for what they take (a color image into a mask and
/// into a filter of either kind, into a mask alone, the red of a color image, an input value
/// read by name and through a socket, inside and outside), and at random.
#[test]
fn nodes_made_a_graph_of_their_own_compute_what_they_did() {
    use brewed_texture_core::package::{Library, Package, PackageEdit, flatten};

    struct One(Package);
    impl Library for One {
        fn package(&self, path: Option<&str>) -> Option<&Package> {
            path.is_none().then_some(&self.0)
        }
    }
    /// The images the outputs of the graph `id` write, by their names.
    fn written(e: &mut Engine, package: &Package, id: brewed_texture_core::GraphId) -> Vec<(String, Vec<f32>)> {
        let flat = flatten(&One(package.clone()), None, id).unwrap();
        assert!(flat.problems.is_empty(), "{:?}", flat.problems);
        let ev = evaluate_blocking(e, &flat.graph);
        for n in &flat.graph.nodes {
            let error = ev.node(n.id).and_then(|x| x.error()).map(str::to_string);
            if let Some(err) = error.or_else(|| e.state(n.id).and_then(|s| s.error.clone())) {
                panic!("{}: {err}", n.def().name);
            }
        }
        let mut out: Vec<(String, Vec<f32>)> = flat
            .graph
            .nodes_of(NodeKind::OUTPUT)
            .map(|o| (o.text("name").to_string(), e.read_image(o.id).expect("written").2))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }
    fn differ(want: &[(String, Vec<f32>)], got: &[(String, Vec<f32>)]) -> Option<String> {
        if want.len() != got.len() {
            return Some(format!("{} outputs, not {}", got.len(), want.len()));
        }
        for ((name, a), (_, b)) in want.iter().zip(got) {
            let off = a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max);
            // Half floats (colors) round a value a float's rounding moved to the next step.
            if off > 2e-3 || a.len() != b.len() {
                return Some(format!("{name} is off by {off}"));
            }
        }
        None
    }

    let Some(mut e) = engine() else { return };
    let (mut demo, _) = brewed_texture_core::demo::graph();
    demo.resolution = [RES, RES];
    let mut rng = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };
    let (mixed, named) = mixed();
    let id = |name: &str| named.iter().find(|(n, _)| *n == name).unwrap().1;
    let chosen_for_what_they_take: Vec<Vec<NodeId>> = [
        &["blend", "blur", "red"][..],
        &["blend"],
        &["normal"],
        &["red"],
        &["pm", "levels"],
        &["amount", "pm", "levels"],
        &["soft", "tiles"],
        &["color", "warm", "blend", "blur"],
    ]
    .iter()
    .map(|names| names.iter().map(|n| id(n)).collect())
    .collect();
    for (name, mut graph, fixed) in [("demo", demo, Vec::new()), ("mixed", mixed, chosen_for_what_they_take)] {
        // Written as floats: what a graph computes, not 8 bits of it.
        let outputs: Vec<NodeId> = graph.nodes_of(NodeKind::OUTPUT).map(|n| n.id).collect();
        for o in outputs {
            set(&mut graph, o, "bit_depth", ParamValue::Enum(1));
        }
        let package = Package::new(name, graph);
        let main = package.first();
        let want = written(&mut e, &package, main);
        // Image nodes chosen at random, as many as `share` tenths of them, and half the value
        // nodes that only they read, until they can go (they cannot where a node is between
        // them).
        let mut make = |p: &Package, graph: brewed_texture_core::GraphId, share: u64| {
            let g = &p.graph(graph).unwrap().graph;
            let movable: Vec<&brewed_texture_core::graph::Node> =
                g.nodes.iter().filter(|n| n.kind != NodeKind::OUTPUT && n.kind != NodeKind::IMAGE_INPUT).collect();
            (0..60).find_map(|_| {
                let mut chosen: Vec<&brewed_texture_core::graph::Node> =
                    movable.iter().copied().filter(|n| n.def().has_image() && next() % 10 < share).collect();
                for n in movable.iter().filter(|n| !n.def().has_image()) {
                    let mut readers = g.links.iter().filter(|l| l.from == n.id).map(|l| l.to);
                    if readers.all(|r| chosen.iter().any(|c| c.id == r)) && next() % 2 == 0 {
                        chosen.push(n);
                    }
                }
                let mut made = p.clone();
                let ids = chosen.iter().map(|n| n.id).collect();
                let result = PackageEdit::GraphFromSelection { graph, nodes: ids, name: "Part".into() }.apply(&mut made).ok()?;
                let kinds: Vec<&str> = chosen.iter().map(|n| n.def().name).collect();
                Some((made, result.graph.unwrap(), kinds))
            })
        };
        for chosen in fixed {
            let mut p = package.clone();
            let kinds: Vec<&str> = chosen.iter().map(|id| p.graph(main).unwrap().graph.node(*id).unwrap().def().name).collect();
            let edit = PackageEdit::GraphFromSelection { graph: main, nodes: chosen, name: "Part".into() };
            edit.apply(&mut p).unwrap_or_else(|e| panic!("{kinds:?}: {e}"));
            if let Some(off) = differ(&want, &written(&mut e, &p, main)) {
                panic!("{name}: {off}, with {kinds:?} in a graph of their own");
            }
        }
        let mut made = 0;
        for round in 0..12 {
            let share = [3, 5, 7][round % 3];
            let (p, sub, kinds) = make(&package, main, share).unwrap_or_else(|| panic!("{name}: no nodes could go"));
            if let Some(off) = differ(&want, &written(&mut e, &p, main)) {
                panic!("{name}: {off}, with {kinds:?} in a graph of their own");
            }
            // Some of the new graph's nodes in a graph of their own again.
            if let Some((p, _, deeper)) = make(&p, sub, 5) {
                made += 1;
                if let Some(off) = differ(&want, &written(&mut e, &p, main)) {
                    panic!("{name}: {off}, with {kinds:?} in a graph of their own, and {deeper:?} of them deeper");
                }
            }
        }
        assert!(made >= 6, "{name}: {made} graphs made in graphs made");
    }
    assert!(e.gpu.take_errors().is_empty());
}

/// The SVG and Text nodes draw sharp pictures at the texture's size: an SVG file in its colors
/// or as how much of each pixel it covers, and text in the font built in, both wrapping around
/// where the texture tiles.
#[test]
fn svg_and_text_nodes() {
    let Some(mut e) = engine() else { return };
    let dir = std::env::temp_dir().join(format!("brewed-vector-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("half.svg");
    // A red square on the left half of a picture twice as wide as it is high.
    std::fs::write(&file, r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><rect width="100" height="100" fill="#ff0000"/></svg>"##)
        .unwrap();
    let mut g = graph();
    let svg = g.add_node(NodeKind::SVG, [0.0, 0.0]);
    set(&mut g, svg, "path", ParamValue::Text(file.display().to_string()));
    set(&mut g, svg, "fit", ParamValue::Enum(2));
    let data = eval_read(&mut e, &g, svg);
    assert_eq!(pixel(&data, RES / 4, RES / 2), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(pixel(&data, 3 * RES / 4, RES / 2)[3], 0.0);
    // As a mask: white where it is, black elsewhere, grayscale.
    set(&mut g, svg, "output", ParamValue::Enum(1));
    let data = eval_read(&mut e, &g, svg);
    assert_eq!(pixel(&data, RES / 4, 7), [1.0, 1.0, 1.0, 1.0]);
    assert_eq!(pixel(&data, 3 * RES / 4, 7), [0.0, 0.0, 0.0, 1.0]);
    assert!(e.image(svg).unwrap().is_gray());
    // Moved a quarter to the right with no tiling, the right half is covered and nothing wraps.
    set(&mut g, svg, "tiling", ParamValue::Enum(0));
    set(&mut g, svg, "offset", ParamValue::Vec2([0.5, 0.0]));
    let data = eval_read(&mut e, &g, svg);
    assert_eq!(pixel(&data, 3 * RES / 4, 7)[0], 1.0);
    assert_eq!(pixel(&data, RES / 4, 7)[0], 0.0);

    let text = g.add_node(NodeKind::TEXT, [0.0, 0.0]);
    let data = eval_read(&mut e, &g, text);
    let covered = mean(&data);
    assert!(covered > 0.01, "the letters are there: {covered}");
    // Half a texture over, where the texture tiles: all of it still there, what left the right
    // edge back at the left.
    set(&mut g, text, "offset", ParamValue::Vec2([0.5, 0.0]));
    let moved = mean(&eval_read(&mut e, &g, text));
    assert!((moved - covered).abs() < 0.02 * covered, "{moved} against {covered}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Distances by jump flooding are the distances: from a dot, a pixel's value falls off as its
/// true distance from the dot does, across the edges where the texture tiles; the signed field
/// is a half at the edge of a disc; a bevel rises from the edge to its top over its width.
#[test]
fn distance_fields_and_bevels() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    // A disc of radius 1/8 in the middle.
    let disc = g.add_node(NodeKind::SHAPE, [0.0, 0.0]);
    set(&mut g, disc, "size", ParamValue::Vec2([0.25, 0.25]));
    set(&mut g, disc, "softness", ParamValue::Float(0.0));
    let dist = g.add_node(NodeKind::DISTANCE, [0.0, 0.0]);
    g.connect(disc, "o:output", dist, "i:mask").unwrap();
    // 64 px of 1024 is 16 px of 256.
    let data = eval_read(&mut e, &g, dist);
    let r = RES as f32 / 8.0;
    let c = RES as f32 / 2.0;
    assert_eq!(pixel(&data, RES / 2, RES / 2)[0], 1.0, "on the shape");
    for x in [c + r + 4.0, c + r + 8.0, c + r + 12.0] {
        let expected = 1.0 - (x - (c + r)) / 16.0;
        let got = pixel(&data, x as u32, RES / 2)[0];
        assert!(close(got, expected, 0.08), "at {x}: {got}, not {expected}");
    }
    assert_eq!(pixel(&data, 3, 3)[0], 0.0, "farther than the distance");
    // Tiling: half a disc at the left edge (the disc itself not wrapping) has distances across
    // it, on the right.
    set(&mut g, disc, "offset", ParamValue::Vec2([-0.5, 0.0]));
    set(&mut g, disc, "tiling", ParamValue::Enum(0));
    let data = eval_read(&mut e, &g, dist);
    let right = pixel(&data, RES - 3, RES / 2)[0];
    assert!(right > 0.7, "across the edge: {right}");
    set(&mut g, dist, "tiling", ParamValue::Enum(0));
    let data = eval_read(&mut e, &g, dist);
    assert_eq!(pixel(&data, RES - 3, RES / 2)[0], 0.0, "not across an edge that does not tile");
    set(&mut g, disc, "offset", ParamValue::Vec2([0.0, 0.0]));
    set(&mut g, disc, "tiling", ParamValue::Enum(3));
    set(&mut g, dist, "tiling", ParamValue::Enum(3));
    // Signed: a half at the edge, more inside, less outside.
    set(&mut g, dist, "mode", ParamValue::Enum(2));
    let data = eval_read(&mut e, &g, dist);
    let edge = pixel(&data, (c + r) as u32, RES / 2)[0];
    assert!(close(edge, 0.5, 0.06), "at the edge: {edge}");
    assert!(pixel(&data, RES / 2, RES / 2)[0] > 0.9);
    assert!(pixel(&data, 3, 3)[0] < 0.1);
    // Into the shapes: 0 at the edge, rising inward.
    set(&mut g, dist, "mode", ParamValue::Enum(1));
    let data = eval_read(&mut e, &g, dist);
    assert_eq!(pixel(&data, 3, 3)[0], 0.0);
    assert!(pixel(&data, RES / 2, RES / 2)[0] > 0.99);
    // A distance larger than anything (typed, or driven by a value) reaches across the image.
    set(&mut g, dist, "mode", ParamValue::Enum(0));
    set(&mut g, dist, "distance", ParamValue::Float(1e12));
    let data = eval_read(&mut e, &g, dist);
    assert_eq!(pixel(&data, RES / 2, RES / 2)[0], 1.0);
    assert!(pixel(&data, 3, 3)[0] > 0.99, "as near as the shape, next to that distance");
    set(&mut g, dist, "distance", ParamValue::Float(64.0));

    // A bevel: 0 outside, the top in the middle, the slope in between in its profile.
    let bevel = g.add_node(NodeKind::BEVEL, [0.0, 0.0]);
    g.connect(disc, "o:output", bevel, "i:mask").unwrap();
    set(&mut g, bevel, "profile", ParamValue::Enum(0));
    let data = eval_read(&mut e, &g, bevel);
    // 24 px of 1024 is 6 px of 256: halfway up 3.5 px in (a half pixel from the edge).
    let half = pixel(&data, (c + r - 3.5) as u32, RES / 2)[0];
    assert!(close(half, 0.5, 0.12), "halfway up: {half}");
    assert_eq!(pixel(&data, RES / 2, RES / 2)[0], 1.0);
    assert_eq!(pixel(&data, 3, 3)[0], 0.0);
    // Outward: the shape at the top, the slope around it.
    set(&mut g, bevel, "direction", ParamValue::Enum(1));
    let data = eval_read(&mut e, &g, bevel);
    assert_eq!(pixel(&data, RES / 2, RES / 2)[0], 1.0);
    let out = pixel(&data, (c + r + 3.5) as u32, RES / 2)[0];
    assert!(close(out, 0.5, 0.12), "halfway down: {out}");
    assert!(e.gpu.take_errors().is_empty());
}

/// Height Blend shows the higher map; Normal Combine of a flat base is the detail, and of a flat
/// detail the base; Non-uniform Blur leaves alone what its map says not to blur and smooths the
/// rest; Make It Tile makes an image that does not tile, tile.
#[test]
fn height_blend_normal_combine_blur_by_map_and_tiling() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let uniform = |g: &mut Graph, v: f32| {
        let id = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
        set(g, id, "color", ParamValue::Color([v, v, v, 1.0]));
        id
    };
    let (high, low) = (uniform(&mut g, 0.6), uniform(&mut g, 0.4));
    let hb = g.add_node(NodeKind::HEIGHT_BLEND, [0.0, 0.0]);
    g.connect(high, "o:output", hb, "i:top").unwrap();
    g.connect(low, "o:output", hb, "i:bottom").unwrap();
    set(&mut g, hb, "contrast", ParamValue::Float(1.0));
    assert!(close(pixel(&eval_read(&mut e, &g, hb), 5, 5)[0], 0.6, 1e-3));
    set(&mut g, hb, "offset", ParamValue::Float(-0.3));
    assert!(close(pixel(&eval_read(&mut e, &g, hb), 5, 5)[0], 0.4, 1e-3), "lowered below the bottom one");
    set(&mut g, hb, "output", ParamValue::Enum(1));
    assert!(close(pixel(&eval_read(&mut e, &g, hb), 5, 5)[0], 0.0, 1e-3), "the top one does not show");

    // A tilted detail on a flat base is the detail (UDN keeps the base's height, which flattens
    // it a little); a flat detail leaves the base as it is.
    let flat = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
    set(&mut g, flat, "color", ParamValue::Color([0.5, 0.5, 1.0, 1.0]));
    let n = [0.3f32, -0.2, 0.0];
    let nz = (1.0 - n[0] * n[0] - n[1] * n[1]).sqrt();
    let tilted = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
    set(&mut g, tilted, "color", ParamValue::Color([n[0] * 0.5 + 0.5, n[1] * 0.5 + 0.5, nz * 0.5 + 0.5, 1.0]));
    let nc = g.add_node(NodeKind::NORMAL_COMBINE, [0.0, 0.0]);
    g.connect(flat, "o:output", nc, "i:base").unwrap();
    g.connect(tilted, "o:output", nc, "i:detail").unwrap();
    let udn = {
        let l = (n[0] * n[0] + n[1] * n[1] + 1.0).sqrt();
        [n[0] / l, n[1] / l, 1.0 / l]
    };
    for technique in 0..3 {
        set(&mut g, nc, "technique", ParamValue::Enum(technique));
        let got = pixel(&eval_read(&mut e, &g, nc), 3, 3);
        let expected = if technique == 2 { udn } else { [n[0], n[1], nz] };
        for c in 0..3 {
            let want = expected[c] * 0.5 + 0.5;
            assert!(close(got[c], want, 4e-3), "technique {technique}, channel {c}: {} not {want}", got[c]);
        }
    }
    g.connect(tilted, "o:output", nc, "i:base").unwrap();
    g.connect(flat, "o:output", nc, "i:detail").unwrap();
    set(&mut g, nc, "technique", ParamValue::Enum(0));
    let got = pixel(&eval_read(&mut e, &g, nc), 3, 3);
    assert!(close(got[0], n[0] * 0.5 + 0.5, 4e-3) && close(got[1], n[1] * 0.5 + 0.5, 4e-3), "{got:?}");

    // Blurred by a map: nothing where it is black, the noise smoother where it is white.
    let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    let black = uniform(&mut g, 0.0);
    let nb = g.add_node(NodeKind::NON_UNIFORM_BLUR, [0.0, 0.0]);
    g.connect(noise, "o:output", nb, "i:input").unwrap();
    g.connect(black, "o:output", nb, "i:map").unwrap();
    let source = eval_read(&mut e, &g, noise);
    let unblurred = eval_read(&mut e, &g, nb);
    assert!(source.iter().zip(&unblurred).all(|(a, b)| close(*a, *b, 2e-3)), "black blurs nothing");
    g.disconnect(nb, "i:map");
    // (32 px of 256: wider than the noise's grain.)
    set(&mut g, nb, "radius", ParamValue::Float(128.0));
    let blurred = eval_read(&mut e, &g, nb);
    let variance = |d: &[f32]| {
        let m = mean(d);
        d.iter().step_by(4).map(|v| (*v as f64 - m).powi(2)).sum::<f64>() / (d.len() / 4) as f64
    };
    assert!(variance(&blurred) < 0.5 * variance(&source), "{} against {}", variance(&blurred), variance(&source));

    // A gradient from left to right does not tile; made to, it does.
    let ramp = g.add_node(NodeKind::GRADIENT, [0.0, 0.0]);
    set(&mut g, ramp, "tiling", ParamValue::Enum(0));
    let tile = g.add_node(NodeKind::MAKE_IT_TILE, [0.0, 0.0]);
    g.connect(ramp, "o:output", tile, "i:input").unwrap();
    let (before, _) = seams(&eval_read(&mut e, &g, ramp), RES);
    let (after, inside) = seams(&eval_read(&mut e, &g, tile), RES);
    assert!(before > 0.5, "the ramp does not tile: {before}");
    assert!(after < 0.05 && after <= inside * 4.0 + 0.01, "made to tile: {after} (inside {inside})");
    assert!(e.gpu.take_errors().is_empty());
}

/// How many shapes `mask` has: regions of it connected through their four neighbours (across
/// the edges when it `tiles`), counted on the CPU.
fn shapes_of(mask: &[f32], size: u32, tiles: bool) -> usize {
    let inside = |i: usize| mask[i * 4] >= 0.5;
    let n = (size * size) as usize;
    let mut seen = vec![false; n];
    let mut count = 0;
    for start in 0..n {
        if seen[start] || !inside(start) {
            continue;
        }
        count += 1;
        seen[start] = true;
        let mut stack = vec![start];
        while let Some(i) = stack.pop() {
            let (x, y) = ((i as u32 % size) as i64, (i as u32 / size) as i64);
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (mut nx, mut ny) = (x + dx, y + dy);
                if tiles {
                    nx = nx.rem_euclid(size as i64);
                    ny = ny.rem_euclid(size as i64);
                } else if nx < 0 || ny < 0 || nx >= size as i64 || ny >= size as i64 {
                    continue;
                }
                let j = (ny * size as i64 + nx) as usize;
                if !seen[j] && inside(j) {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
    }
    count
}

/// Whether every two neighbours inside the shapes of `mask` have the same value in `filled`
/// (their shape's), and how many values there are: `None` when two neighbours differ.
fn one_value_a_shape(mask: &[f32], filled: &[f32], size: u32, tiles: bool) -> Option<usize> {
    let at = |d: &[f32], x: u32, y: u32| d[((y * size + x) * 4) as usize];
    let mut values = std::collections::BTreeSet::new();
    for y in 0..size {
        for x in 0..size {
            if at(mask, x, y) < 0.5 {
                continue;
            }
            values.insert(at(filled, x, y).to_bits());
            for (nx, ny) in [(x + 1, y), (x, y + 1)] {
                let (nx, ny) = if tiles { (nx % size, ny % size) } else if nx >= size || ny >= size { continue } else { (nx, ny) };
                if at(mask, nx, ny) >= 0.5 && at(filled, nx, ny) != at(filled, x, y) {
                    return None;
                }
            }
        }
    }
    Some(values.len())
}

/// Flood Fill gives each shape one value: the bricks of the tile generator, a U whose arms meet
/// through its bottom (and, where the texture tiles, across the edges), a snake of thin
/// stripes turning at their ends.
#[test]
fn flood_fill_gives_each_shape_one_value() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let tiles = g.add_node(NodeKind::TILE_GENERATOR, [0.0, 0.0]);
    let fill = g.add_node(NodeKind::FLOOD_FILL, [0.0, 0.0]);
    g.connect(tiles, "o:output", fill, "i:mask").unwrap();
    let mask = eval_read(&mut e, &g, tiles);
    let filled = eval_read(&mut e, &g, fill);
    let shapes = one_value_a_shape(&mask, &filled, RES, true).expect("one value a brick");
    assert_eq!(shapes, shapes_of(&mask, RES, true), "a value for each brick");
    assert!(shapes > 40, "the bricks are told apart: {shapes}");
    assert!(e.image(fill).unwrap().is_gray());
    set(&mut g, fill, "output", ParamValue::Enum(1));
    let colored = eval_read(&mut e, &g, fill);
    assert!(!e.image(fill).unwrap().is_gray());
    assert!(one_value_a_shape(&mask, &colored, RES, true).is_some());

    let shape = |g: &mut Graph, expression: &str| {
        let pm = g.add_node(NodeKind::PIXEL_MATH, [0.0, 0.0]);
        set(g, pm, "expression", ParamValue::Text(expression.into()));
        pm
    };
    // A U, its arms meeting only at the bottom.
    let u_shape = shape(&mut g, "max(step(0.75, v), max(1 - step(0.25, u), step(0.75, u))) * step(0.1, v)");
    g.connect(u_shape, "o:output", fill, "i:mask").unwrap();
    set(&mut g, fill, "tiling", ParamValue::Enum(0));
    let mask = eval_read(&mut e, &g, u_shape);
    let filled = eval_read(&mut e, &g, fill);
    assert_eq!(one_value_a_shape(&mask, &filled, RES, false), Some(1), "a U is one shape");
    // A snake: stripes 2 px wide, 6 px apart, joined at the top and the bottom in turn.
    let snake = shape(
        &mut g,
        "max(1 - step(2, mod(px, 8)), \
         step(8, mod(px, 16)) * (1 - step(2, py)) + (1 - step(8, mod(px, 16))) * step(height - 2, py)) * (1 - step(width - 8, px))",
    );
    g.connect(snake, "o:output", fill, "i:mask").unwrap();
    let mask = eval_read(&mut e, &g, snake);
    let filled = eval_read(&mut e, &g, fill);
    assert_eq!(one_value_a_shape(&mask, &filled, RES, false), Some(1), "the snake is one shape");
    // A spiral band, four turns of it, thin and curved all along.
    let spiral = shape(&mut g, "(1 - step(0.3, fract(hypot(u - 0.5, v - 0.5) * 10 - atan2(v - 0.5, u - 0.5) / tau))) * (1 - step(0.45, hypot(u - 0.5, v - 0.5)))");
    g.connect(spiral, "o:output", fill, "i:mask").unwrap();
    let mask = eval_read(&mut e, &g, spiral);
    let filled = eval_read(&mut e, &g, fill);
    // (Where the circle cuts it, the tip of its tail falls apart in a few slivers of their own.)
    assert_eq!(one_value_a_shape(&mask, &filled, RES, false), Some(shapes_of(&mask, RES, false)), "the spiral is one shape");
    assert!(e.gpu.take_errors().is_empty());
}

/// A gentle slope, a two-hundredth of the range across the image: steps of half floats in a
/// graph of half precision (as a new graph has it), all its values in one of full precision:
/// in the node's image, in what an output of 16 bits writes and in the file it exports, and
/// through Non-uniform Blur and the Tile Sampler, which read their inputs at every scale in
/// the precision they have. Ambient Occlusion reads it filtered, from a copy in half floats;
/// the 2D view shows a copy it can filter. A 16-bit grayscale file keeps all its steps too.
/// A node can keep its image in floats in a graph of half precision, and in bytes.
#[test]
fn full_precision_keeps_gentle_slopes() {
    use brewed_texture_core::Precision;
    use wgpu::TextureFormat as F;
    let Some(mut e) = engine() else { return };
    assert_eq!(Graph::default().precision, Precision::Half);
    let mut g = graph();
    let slope = g.add_node(NodeKind::PIXEL_MATH, [0.0, 0.0]);
    set(&mut g, slope, "expression", ParamValue::Text("0.5 + u * 0.005".into()));
    let out = g.add_node(NodeKind::OUTPUT, [0.0, 0.0]);
    set(&mut g, out, "bit_depth", ParamValue::Enum(1));
    g.connect(slope, "o:output", out, "i:input").unwrap();
    let ao = g.add_node(NodeKind::AMBIENT_OCCLUSION, [0.0, 0.0]);
    g.connect(slope, "o:output", ao, "i:input").unwrap();
    let values = |d: &[f32]| d.iter().step_by(4).map(|v| v.to_bits()).collect::<std::collections::BTreeSet<_>>().len();
    let half = values(&eval_read(&mut e, &g, slope));
    assert!(half < 20, "half floats have few steps there: {half}");
    g.precision = Precision::Full;
    let full = values(&eval_read(&mut e, &g, slope));
    assert!(full > 200, "floats have them all: {full}");
    assert_eq!(e.image(slope).unwrap().format(), F::R32Float);
    assert_eq!(e.image(out).unwrap().format(), F::R32Float);
    // The 16-bit file has the steps too.
    let stored = e.read_stored(out).unwrap();
    let dir = std::env::temp_dir().join(format!("brewed-precision-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("slope.png");
    crate::export::write_stored(&file, &stored, 0, crate::export::DEPTH_16).unwrap();
    let read = image::open(&file).unwrap().into_luma16();
    let steps = read.pixels().map(|p| p.0[0]).collect::<std::collections::BTreeSet<_>>().len();
    assert!(steps > 200, "16 bits of it: {steps} steps");
    // Filtered from a copy it can filter, and shown through one.
    let _ = eval_read(&mut e, &g, ao);
    assert!(e.gpu.take_errors().is_empty());
    // Not blurred where the map is black: the slope as it is, every value. Blurred, a slope
    // still, as straight as floats are (half floats would bend it by their steps, 1/4096).
    let original = eval_read(&mut e, &g, slope);
    let blur = g.add_node(NodeKind::NON_UNIFORM_BLUR, [0.0, 0.0]);
    let map = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
    set(&mut g, map, "color", ParamValue::Color([0.0, 0.0, 0.0, 1.0]));
    g.connect(slope, "o:output", blur, "i:input").unwrap();
    g.connect(map, "o:output", blur, "i:map").unwrap();
    assert_eq!(eval_read(&mut e, &g, blur), original);
    set(&mut g, map, "color", ParamValue::Color([1.0; 4]));
    set(&mut g, blur, "radius", ParamValue::Float(8.0));
    let blurred = eval_read(&mut e, &g, blur);
    let row = |d: &[f32], x: u32| pixel(d, x, RES / 2)[0];
    let bend = (RES / 4..3 * RES / 4).map(|x| (row(&blurred, x + 1) - 2.0 * row(&blurred, x) + row(&blurred, x - 1)).abs());
    let bend = bend.fold(0.0, f32::max);
    assert!(bend < 2e-6, "bent by {bend}");
    assert!(values(&blurred) > 200, "{}", values(&blurred));
    // The slope as the pattern of one tile as large as the image: the slope as it is.
    let tiles = g.add_node(NodeKind::TILE_SAMPLER, [0.0, 0.0]);
    set(&mut g, tiles, "pattern", ParamValue::Enum(brewed_texture_core::nodes::TILE_PATTERN_IMAGE));
    set(&mut g, tiles, "x_amount", ParamValue::Int(1));
    set(&mut g, tiles, "y_amount", ParamValue::Int(1));
    set(&mut g, tiles, "scale", ParamValue::Float(1.0));
    g.connect(slope, "o:output", tiles, "i:pattern_1").unwrap();
    let tiled = eval_read(&mut e, &g, tiles);
    let off = tiled.iter().zip(&original).step_by(4).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
    assert!(off < 1e-6, "{off} off");
    assert!(e.gpu.take_errors().is_empty());
    e.set_keep(crate::engine::Keep::Read);
    let shown = e.prepare_preview(Some(slope)).flatten().expect("shown");
    assert_eq!(shown.texture.format(), F::R16Float);
    assert!(shown.texture.mip_level_count() > 1);
    e.set_keep(crate::engine::Keep::All);
    // A 16-bit grayscale file: all its steps in a graph of full precision.
    let gradient = image::ImageBuffer::<image::Luma<u16>, Vec<u16>>::from_fn(RES, RES, |x, _| image::Luma([32768 + x as u16 * 2]));
    let heights = dir.join("heights.png");
    gradient.save(&heights).unwrap();
    let bitmap = g.add_node(NodeKind::BITMAP, [0.0, 0.0]);
    set(&mut g, bitmap, "path", ParamValue::Text(heights.display().to_string()));
    let read = values(&eval_read(&mut e, &g, bitmap));
    assert!(read > 200, "{read} values of 256");
    let _ = std::fs::remove_dir_all(&dir);
    // Its own precision, whatever the graph's: floats in a graph of half floats, and bytes.
    g.precision = Precision::Half;
    set(&mut g, slope, "precision", ParamValue::Enum(3));
    assert!(values(&eval_read(&mut e, &g, slope)) > 200);
    assert_eq!(e.image(slope).unwrap().format(), F::R32Float);
    set(&mut g, slope, "precision", ParamValue::Enum(1));
    let bytes = eval_read(&mut e, &g, slope);
    assert_eq!(e.image(slope).unwrap().format(), F::R8Unorm);
    assert!(values(&bytes) <= 3, "{}", values(&bytes));
    assert!(e.gpu.take_errors().is_empty());
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

    // The same picture held by a package (see `brewed_texture_core::files`): the same pixels.
    let bytes = std::sync::Arc::new(std::fs::read(&path).unwrap());
    let held = brewed_texture_core::files::embedded_path("tex.png", &bytes);
    brewed_texture_core::files::keep(&held, &bytes, "held.png");
    set(&mut g, b, "path", ParamValue::Text(held));
    assert_eq!(eval_read(&mut e, &g, b), data);

    // A missing file is an error on the node, not a crash.
    set(&mut g, b, "path", ParamValue::Text(dir.join("missing.png").display().to_string()));
    evaluate_blocking(&mut e, &g);
    let error = e.state(b).unwrap().error.clone();
    assert!(error.as_deref().is_some_and(|m| m.contains("Cannot open")), "{error:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn files_no_node_reads_leave_the_gpu() {
    let Some(mut e) = engine() else { return };
    let dir = std::env::temp_dir().join(format!("btd-files-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (a, b) = (dir.join("a.png"), dir.join("b.png"));
    image::RgbaImage::from_pixel(64, 32, image::Rgba([200, 10, 10, 255])).save(&a).unwrap();
    image::RgbaImage::from_pixel(32, 64, image::Rgba([10, 200, 10, 255])).save(&b).unwrap();
    let mut g = graph();
    let n = g.add_node(NodeKind::BITMAP, [0.0, 0.0]);
    set(&mut g, n, "path", ParamValue::Text(a.display().to_string()));
    evaluate_blocking(&mut e, &g);
    let one = e.memory_report().files;
    assert!(one > 0);
    // Another file: the first is read no more, and goes.
    set(&mut g, n, "path", ParamValue::Text(b.display().to_string()));
    evaluate_blocking(&mut e, &g);
    assert_eq!(e.memory_report().files, one);
    // No Bitmap node, no file.
    g.remove_nodes(&[n]);
    evaluate_blocking(&mut e, &g);
    assert_eq!(e.memory_report().files, 0);
    // A file whose node went while it was decoded is not uploaded when it arrives.
    let n = g.add_node(NodeKind::BITMAP, [0.0, 0.0]);
    set(&mut g, n, "path", ParamValue::Text(a.display().to_string()));
    e.evaluate(&g, &brewed_texture_core::eval::evaluate(&g));
    assert!(e.files_loading());
    g.remove_nodes(&[n]);
    e.evaluate(&g, &brewed_texture_core::eval::evaluate(&g));
    assert!(!e.files_loading());
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_millis(500) {
        assert!(!e.receive_files());
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(e.memory_report().files, 0);
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
    use brewed_texture_core::nodes::{NOISE_TYPES, NOISE_WEAVE, WEAVES};
    for (t, name) in NOISE_TYPES.iter().enumerate() {
        set(g, noise, "type", ParamValue::Enum(t as u32));
        if t as u32 != NOISE_WEAVE {
            f(g, name);
            continue;
        }
        // Each weave repeats after a different number of threads.
        for (w, weave) in WEAVES.iter().enumerate() {
            set(g, noise, "weave", ParamValue::Enum(w as u32));
            f(g, &format!("{name} {weave}"));
        }
        set(g, noise, "weave", ParamValue::Enum(0));
    }
}

/// The shader's names for the types of noise are the node's, in the same order (the last,
/// white noise, is what any other number draws).
#[test]
fn the_shader_knows_every_noise_by_its_number() {
    let source = include_str!("gpu/shaders/noise.wgsl");
    let types = brewed_texture_core::nodes::NOISE_TYPES;
    let mut known = 0;
    for line in source.lines() {
        let Some(rest) = line.strip_prefix("const ") else { continue };
        let Some((name, value)) = rest.split_once(": u32 = ") else { continue };
        let Some(Ok(n)) = value.strip_suffix("u;").map(str::parse::<usize>) else { continue };
        if name.starts_with("BASIS_") || name.chars().any(|c| c.is_ascii_lowercase()) {
            continue;
        }
        assert_eq!(types[n].to_uppercase().replace(' ', "_"), name, "the shader's {name} is {n}");
        known += 1;
    }
    assert_eq!(known, types.len() - 1, "the shader draws every type but white noise by its number");
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

/// Kept as the app keeps them, an image lives while something reads it: a node no other node
/// takes and no view shows keeps only its thumbnail, an image read only inside an evaluation
/// goes back to the pool once it is read, what the last change read from outside itself
/// stays for the next change of the same kind, and once the engine settles only what the
/// views show stays.
#[test]
fn images_live_while_something_reads_them() {
    let Some(mut e) = engine() else { return };
    e.set_keep(crate::engine::Keep::Read);
    let mut g = graph();
    let alone = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    let blur = g.add_node(NodeKind::BLUR, [0.0, 0.0]);
    let out = g.add_node(NodeKind::OUTPUT, [0.0, 0.0]);
    g.connect(noise, "o:output", blur, "i:input").unwrap();
    g.connect(blur, "o:output", out, "i:input").unwrap();
    // The 3D view uses the output.
    e.set_materials(&[out]);
    evaluate_blocking(&mut e, &g);
    let thumbs = e.render_thumbnails();
    for id in [alone, noise, blur, out] {
        assert!(thumbs.iter().any(|t| t.node == id), "every node drew its thumbnail");
    }
    // Only what the output writes stays, with the mips the 3D view samples it with: a gray
    // 8-bit image, one byte a pixel.
    assert!(e.image(alone).is_none() && e.image(noise).is_none() && e.image(blur).is_none());
    let shown = e.image(out).expect("the output's image").clone();
    assert_eq!(shown.format(), wgpu::TextureFormat::R8Unorm);
    assert!(shown.texture.mip_level_count() > 1);
    e.gpu.clear_pool();
    assert_eq!(e.memory(), shown.bytes(), "one image of one byte a pixel, and its mips");

    // A change of the noise computes it, the blur and the output again; the noise goes back
    // to the pool once the blur has read it.
    set(&mut g, noise, "seed", ParamValue::Int(3));
    evaluate_blocking(&mut e, &g);
    assert_eq!(e.report.recomputed, 3);
    assert_eq!(e.render_thumbnails().len(), 3, "the noise, the blur and the output changed");
    assert!(e.image(noise).is_none());
    // A change of the blur reads the noise: it is computed again, as it was (its version
    // stays, so nothing else is), and stays for the next change of the blur.
    let version = e.version(noise);
    set(&mut g, blur, "radius", ParamValue::Float(3.0));
    evaluate_blocking(&mut e, &g);
    assert_eq!(e.report.recomputed, 3);
    assert_eq!(e.version(noise), version);
    assert!(e.image(noise).is_some());
    let thumbs = e.render_thumbnails();
    assert!(thumbs.iter().all(|t| t.node != noise), "no new thumbnail of what did not change");
    set(&mut g, blur, "radius", ParamValue::Float(4.0));
    evaluate_blocking(&mut e, &g);
    assert_eq!(e.report.recomputed, 2, "the noise was there");
    assert_eq!(e.render_thumbnails().len(), 2, "the blur and the output changed");
    // Shown in the 2D view, a node is computed and stays.
    e.set_preview(Some(alone));
    evaluate_blocking(&mut e, &g);
    assert_eq!(e.report.recomputed, 1);
    assert!(e.image(alone).is_some());
    assert!(e.render_thumbnails().is_empty(), "nothing changed");
    // Nothing changes for a while: what the next change of the blur would have read goes.
    e.settle();
    assert!(e.image(noise).is_none());
    assert!(e.image(alone).is_some() && e.image(out).is_some());
    let memory = e.memory_report();
    assert_eq!((memory.shown_images, memory.kept, memory.scratch), (2, 0, 0));
    // Deleted, a node lets go of its image.
    g.remove_nodes(&[alone]);
    e.set_preview(None);
    evaluate_blocking(&mut e, &g);
    assert!(e.image(alone).is_none() && e.state(alone).is_none());
}

/// A view samples the node image it was given until it is given another, which the engine
/// cannot know it has been yet: let go of, that texture never goes back to the pool, where
/// another image could take it and be drawn in the view in its place.
#[test]
fn a_texture_a_view_was_given_is_not_taken_by_another_image() {
    let Some(mut e) = engine() else { return };
    e.set_keep(crate::engine::Keep::Read);
    let mut g = graph();
    let a = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    let b = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    set(&mut g, b, "seed", ParamValue::Int(7));
    e.set_preview(Some(a));
    evaluate_blocking(&mut e, &g);
    let shown = e.prepare_preview(Some(a)).flatten().expect("the 2D view shows a");
    assert!(shown.texture.mip_level_count() > 1, "with the mips the view samples it with");
    // The view moves on to b, and a is let go of.
    e.set_preview(Some(b));
    evaluate_blocking(&mut e, &g);
    let next = e.prepare_preview(Some(b)).flatten().expect("the 2D view shows b");
    assert!(e.image(a).is_none());
    assert!(next.texture != shown.texture);
    // Nothing after takes it either.
    for seed in 0..3 {
        set(&mut g, b, "seed", ParamValue::Int(seed));
        evaluate_blocking(&mut e, &g);
    }
    let [w, h] = [shown.texture.width(), shown.texture.height()];
    let taken: Vec<_> = (0..40).map(|_| e.gpu.take_scratch_with_mips(w, h, shown.texture.format())).collect();
    assert!(taken.iter().all(|t| t.texture != shown.texture));
}

/// The app's engine and views, as they run a graph: the 2D view shows one node, the 3D view
/// the outputs with a usage, and each round goes as the engine thread goes (see
/// `service::run`). The views hold what they were last given, as the app does.
struct App {
    e: Engine,
    g: Graph,
    outputs: Vec<NodeId>,
    preview: Option<NodeId>,
    shown: Option<crate::engine::DisplayImage>,
    materials: Vec<crate::engine::DisplayImage>,
    /// The memory the last round worked in: what stays, and the scratch images of its
    /// evaluation (at most what was alive at once), and how long it took.
    at_work: (u64, f64),
}

impl App {
    fn demo(e: Engine, resolution: u32) -> App {
        let (mut g, preview) = brewed_texture_core::demo::graph();
        g.resolution = [resolution, resolution];
        let outputs = g.nodes_of(NodeKind::OUTPUT).filter(|n| n.enum_value("usage") != 0).map(|n| n.id).collect();
        let mut app = App { e, g, outputs, preview, shown: None, materials: Vec::new(), at_work: (0, 0.0) };
        app.e.set_keep(crate::engine::Keep::Read);
        app.round(true);
        app
    }

    /// One round of the engine thread: an evaluation when the graph changed or what the views
    /// read is not there, then what the views are given and the thumbnails. Nothing waits
    /// after it (a single change, a node shown), so the scratch images go.
    fn round(&mut self, changed: bool) {
        self.e.set_preview(self.preview);
        self.e.set_materials(&self.outputs);
        let read: Vec<NodeId> = self.preview.iter().chain(&self.outputs).copied().collect();
        let started = std::time::Instant::now();
        if changed || self.e.missing(&read) {
            evaluate_blocking(&mut self.e, &self.g);
        }
        if let Some(shown) = self.e.prepare_preview(self.preview) {
            self.shown = shown;
        }
        if let Some(materials) = self.e.prepare_materials(&self.outputs) {
            self.materials = materials;
        }
        self.e.sweep();
        self.e.render_thumbnails();
        self.at_work = (self.e.memory(), started.elapsed().as_secs_f64() * 1000.0);
        self.e.gpu.clear_pool();
    }

    /// A double click on a node: the 2D view shows it.
    fn show(&mut self, node: NodeId) {
        self.preview = Some(node);
        self.round(false);
    }

    /// An input value set, as a slider sets it.
    fn set_value(&mut self, name: &str, value: f32) {
        let v = self.g.variables.iter().position(|v| v.name == name).unwrap();
        self.g.variables[v].value = Value::Float(value);
        self.round(true);
    }

    /// Half a second with nothing to do.
    fn wait(&mut self) {
        self.e.settle();
    }
}

/// Showing one node after another in the 2D view leaves nothing behind, and takes nothing
/// more than it shows: as soon as a node is shown, what stays is what the outputs write for
/// the 3D view and the image of that node, whatever was shown or changed before. (The chain
/// up to the node is computed again, in scratch images that go at once.)
#[test]
fn showing_node_after_node_leaves_nothing_behind() {
    let Some(e) = engine() else { return };
    let mut app = App::demo(e, 256);
    app.wait();
    let outputs = app.e.memory_report().shown;
    app.set_value("wear", 0.7);
    app.wait();
    let image_nodes: Vec<NodeId> = app.g.nodes.iter().filter(|n| n.def().has_image()).map(|n| n.id).collect();
    let mut first = Vec::new();
    for round in 0..2 {
        for &node in &image_nodes {
            app.show(node);
            let m = app.e.memory_report();
            // What cannot be filtered is shown through a copy that can (half floats, with mips).
            let own = if app.outputs.contains(&node) {
                0
            } else {
                let image = app.e.image(node).expect("the node shown");
                let shown = &app.shown.as_ref().expect("shown").texture;
                image.bytes() + if *shown == image.texture { 0 } else { crate::gpu::texture_bytes(shown) }
            };
            let name = app.g.node(node).unwrap().def().name;
            assert_eq!((m.shown, m.kept, m.scratch), (outputs + own, 0, 0), "{name}");
            assert!(app.shown.as_ref().is_some_and(|d| d.node == node), "{name} is shown");
            if round == 0 {
                first.push(m);
            } else {
                assert_eq!(m, first[image_nodes.iter().position(|n| *n == node).unwrap()], "{name}, the second time");
            }
        }
    }
}

/// What the demo takes while an input value is dragged, and once nothing changes (prints).
#[test]
#[ignore = "a measurement"]
fn demo_memory_while_changing() {
    let Some(mut e) = engine() else { return };
    let (mut g, preview) = brewed_texture_core::demo::graph();
    g.resolution = [2048, 2048];
    let outputs: Vec<NodeId> = g.nodes_of(NodeKind::OUTPUT).filter(|n| n.enum_value("usage") != 0).map(|n| n.id).collect();
    e.set_keep(crate::engine::Keep::Read);
    e.set_preview(preview);
    e.set_materials(&outputs);
    let mb = |b: u64| b as f64 / (1024.0 * 1024.0);
    let show = |e: &Engine, g: &Graph, what: &str| {
        println!("{what}: {:.0} MB, of which {:.0} MB scratch", mb(e.memory()), mb(e.gpu.pool_bytes()));
        for n in &g.nodes {
            if let Some(img) = e.image(n.id) {
                println!("  kept  {:<18} {:?} x{}: {:.1} MB", n.def().name, img.format(), img.texture.mip_level_count(), mb(img.bytes()));
            }
        }
        for img in e.gpu.pooled() {
            println!("  pool  {:?} {:?} x{}: {:.1} MB", img.size(), img.format(), img.texture.mip_level_count(), mb(img.bytes()));
        }
    };
    evaluate_blocking(&mut e, &g);
    e.prepare_preview(preview);
    e.prepare_materials(&outputs);
    e.render_thumbnails();
    e.settle();
    show(&e, &g, "opened");
    let wear = g.variables.iter().position(|v| v.name == "wear").unwrap();
    for keep_pool in [true, false, true, false] {
        let mut total = 0.0;
        for step in 0..20 {
            g.variables[wear].value = Value::Float(0.5 + step as f32 * 0.01);
            if !keep_pool {
                e.gpu.clear_pool();
            }
            let started = std::time::Instant::now();
            evaluate_blocking(&mut e, &g);
            e.prepare_preview(preview);
            e.prepare_materials(&outputs);
            e.render_thumbnails();
            e.wait_idle();
            if step > 0 {
                total += started.elapsed().as_secs_f64() * 1000.0;
            }
        }
        println!("keep pool {keep_pool}: {} recomputed, {:.2} ms a step", e.report.recomputed, total / 19.0);
    }
    show(&e, &g, "after dragging wear");
    e.settle();
    show(&e, &g, "half a second later");
}

/// Double clicks on every node of the demo at 2048×2048, as `showing_node_after_node_leaves_nothing_behind`
/// makes them, with the memory the engine counts after each and, where the backend can say
/// it (DX12), what its allocator holds (prints).
#[test]
#[ignore = "a measurement"]
fn demo_memory_while_showing_nodes() {
    let gpu = Gpu::headless_on(wgpu::Backends::DX12).or_else(|_| Gpu::headless());
    let Ok(gpu) = gpu else { return };
    let mut app = App::demo(Engine::new(gpu), 2048);
    let mb = |b: u64| b as f64 / (1024.0 * 1024.0);
    let allocated = |app: &App| {
        app.e.wait_idle();
        app.e.gpu.device.generate_allocator_report().map(|r| (mb(r.total_allocated_bytes), mb(r.total_reserved_bytes)))
    };
    let line = |app: &App, what: &str| {
        let m = app.e.memory_report();
        let gpu = allocated(app).map_or(String::new(), |(a, r)| format!(", the allocator {a:.0} MB ({r:.0} reserved)"));
        println!("{what:<28} {:>4.0} MB: shown {:.0} ({}), kept {:.0}, scratch {:.0}{gpu}", mb(m.total()), mb(m.shown), m.shown_images, mb(m.kept), mb(m.scratch));
    };
    println!("{}", app.e.gpu.adapter_info);
    app.wait();
    line(&app, "opened");
    app.set_value("wear", 0.7);
    line(&app, "wear 0.7");
    app.wait();
    line(&app, "  half a second later");
    let image_nodes: Vec<NodeId> = app.g.nodes.iter().filter(|n| n.def().has_image()).map(|n| n.id).collect();
    for round in 0..2 {
        for &node in &image_nodes {
            let name = app.g.node(node).unwrap().def().name;
            app.show(node);
            if round == 0 {
                line(&app, name);
                println!("  {:<26} at most {:.0} MB for {:.1} ms", "at work", mb(app.at_work.0), app.at_work.1);
            }
            app.wait();
            if round == 0 {
                line(&app, "  half a second later");
            }
        }
        line(&app, &format!("after round {}", round + 1));
    }
    let basecolor = app.outputs[0];
    app.show(basecolor);
    app.wait();
    line(&app, "back to basecolor");
}

/// What a step of a dragged input value of the demo costs at 2048×2048, on Vulkan (what the
/// app runs on here) and DX12: with the scratch images and what the step reads kept from the
/// step before, without the scratch images, and without either (prints).
#[test]
#[ignore = "a measurement"]
fn drag_step_costs() {
    for backend in [wgpu::Backends::VULKAN, wgpu::Backends::DX12] {
        let Ok(gpu) = Gpu::headless_on(backend) else { continue };
        println!("{}", gpu.adapter_info);
        let mut app = App::demo(Engine::new(gpu), 2048);
        app.wait();
        let mb = |b: u64| b as f64 / (1024.0 * 1024.0);
        let all = u64::MAX;
        let mb_ = 1024 * 1024;
        // What the engine keeps while a slider is dragged (six grayscale images, 48 MB at
        // 2048), against keeping all of them, less and none.
        let engine = 48 * mb_;
        let budgets = [(all, true), (engine, true), (24 * mb_, true), (0, true), (0, false), (engine, true)];
        for (budget, keep_read) in budgets {
            let mut times = Vec::new();
            let mut memory = 0;
            for step in 0..40 {
                let v = app.g.variables.iter().position(|v| v.name == "wear").unwrap();
                app.g.variables[v].value = Value::Float(0.5 + (step % 20) as f32 * 0.01);
                let started = std::time::Instant::now();
                app.e.set_preview(app.preview);
                app.e.set_materials(&app.outputs);
                evaluate_blocking(&mut app.e, &app.g);
                if let Some(shown) = app.e.prepare_preview(app.preview) {
                    app.shown = shown;
                }
                if let Some(materials) = app.e.prepare_materials(&app.outputs) {
                    app.materials = materials;
                }
                app.e.sweep();
                app.e.render_thumbnails();
                app.e.wait_idle();
                app.e.gpu.trim_pool(budget);
                if !keep_read {
                    app.e.settle();
                }
                memory = memory.max(app.e.memory());
                if step > 0 {
                    times.push(started.elapsed().as_secs_f64() * 1000.0);
                }
            }
            let average = times.iter().sum::<f64>() / times.len() as f64;
            let worst = times.iter().copied().fold(0.0, f64::max);
            let kept = if budget == all { "all".to_string() } else { format!("{} MB", budget / mb_) };
            println!(
                "  scratch kept {kept:<6} read kept {keep_read:<5}: {average:5.2} ms a step, {worst:5.2} at worst, \
                 {:.0} MB between steps",
                mb(memory)
            );
            app.wait();
        }
    }
}

/// What a new 2048×2048 image costs, by its format: made, written once and let go of, on
/// Vulkan and DX12 (prints).
#[test]
#[ignore = "a measurement"]
fn image_allocation_costs() {
    use wgpu::TextureFormat as F;
    for backend in [wgpu::Backends::VULKAN, wgpu::Backends::DX12] {
        let Ok(gpu) = Gpu::headless_on(backend) else { continue };
        println!("{}", gpu.adapter_info);
        let formats =
            [(F::R8Unorm, 1), (F::R16Float, 1), (F::Rgba8Unorm, 1), (F::Rgba16Float, 1), (F::Rgba16Float, 12)];
        for (format, mips) in formats {
            let mut made = 0.0;
            let mut reused = 0.0;
            let kept = gpu.create_image_raw_for_tests(2048, 2048, mips, format);
            for i in 0..21 {
                let started = std::time::Instant::now();
                let image = gpu.create_image_raw_for_tests(2048, 2048, mips, format);
                let mut encoder = gpu.encoder();
                gpu.encode_clear(&mut encoder, &image.level(0));
                gpu.submit(encoder);
                drop(image);
                let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
                if i > 0 {
                    made += started.elapsed().as_secs_f64() * 1000.0;
                }
                let started = std::time::Instant::now();
                let mut encoder = gpu.encoder();
                gpu.encode_clear(&mut encoder, &kept.level(0));
                gpu.submit(encoder);
                let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
                if i > 0 {
                    reused += started.elapsed().as_secs_f64() * 1000.0;
                }
            }
            let mb = kept.bytes() as f64 / (1024.0 * 1024.0);
            println!("  {format:?} x{mips} ({mb:.1} MB): new {:.2} ms, reused {:.2} ms", made / 20.0, reused / 20.0);
        }
    }
}

/// The engine thread, as the app runs it, while an input value of the demo is dragged faster
/// than it computes: the memory it reports as it goes, the most of it, and what it settles
/// on (in MB).
fn drag_through_the_service(resolution: u32, steps: usize) -> Option<(Vec<f64>, f64)> {
    use crate::service::{EngineService, Event, Request};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    let gpu = Gpu::headless().ok()?;
    let reports: Arc<Mutex<Vec<crate::engine::Memory>>> = Arc::default();
    let finished = Arc::new(Mutex::new(0u64));
    let (sink, done) = (reports.clone(), finished.clone());
    let emit: crate::service::Emit = Arc::new(move |event| match event {
        Event::Memory(m) => sink.lock().unwrap().push(m),
        Event::Finished { revision, .. } => *done.lock().unwrap() = revision,
        _ => {}
    });
    let service = EngineService::spawn(gpu.device.clone(), gpu.queue.clone(), gpu.adapter_info.clone(), emit);
    let (mut g, preview) = brewed_texture_core::demo::graph();
    g.resolution = [resolution, resolution];
    let outputs: Vec<NodeId> =
        g.nodes_of(NodeKind::OUTPUT).filter(|n| n.enum_value("usage") != 0).map(|n| n.id).collect();
    let wait_for = |revision: u64| {
        let started = std::time::Instant::now();
        while *finished.lock().unwrap() < revision && started.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(1));
        }
    };
    let send = |g: &Graph, revision: u64| {
        let eval = brewed_texture_core::eval::evaluate(g);
        service.send(Request::Evaluate { revision, graph: Arc::new(g.clone()), eval: Arc::new(eval) });
    };
    service.send(Request::Preview(preview));
    service.send(Request::Materials(outputs));
    send(&g, 1);
    wait_for(1);
    // Settled, and then dragged: a step every millisecond, far faster than a step computes.
    std::thread::sleep(Duration::from_millis(700));
    let wear = g.variables.iter().position(|v| v.name == "wear").unwrap();
    let from = reports.lock().unwrap().len();
    for step in 0..steps {
        g.variables[wear].value = Value::Float(0.5 + step as f32 * 0.005);
        send(&g, 2 + step as u64);
        std::thread::sleep(Duration::from_millis(1));
    }
    wait_for(1 + steps as u64);
    std::thread::sleep(Duration::from_millis(700));
    drop(service);
    let mb = |m: &crate::engine::Memory| m.total() as f64 / (1024.0 * 1024.0);
    let reports = reports.lock().unwrap();
    let during: Vec<f64> = reports[from..reports.len() - 1].iter().map(mb).collect();
    Some((during, mb(reports.last().unwrap())))
}

/// While an input value is dragged, the engine keeps no more than the views' images, what the
/// next step reads and its budget of scratch images (six grayscale images of the texture
/// size); once the drag is over it settles on the views' images again.
#[test]
fn dragging_keeps_memory_to_its_budget() {
    let Some((during, settled)) = drag_through_the_service(512, 40) else { return };
    // At 512: 4 MB the maps with their mips, 0.5 the two images the next step reads, 3 of
    // scratch images at most (and 0.1 of thumbnails, gradients).
    let most = during.iter().copied().fold(0.0, f64::max);
    assert!(most < 7.8, "{most:.1} MB while dragging: {during:.1?}");
    assert!(settled < 4.1, "settled on {settled:.1} MB");
}

/// The same at 2048×2048, as the app opens the demo (prints).
#[test]
#[ignore = "a measurement"]
fn demo_memory_through_the_service() {
    let Some((during, settled)) = drag_through_the_service(2048, 60) else { return };
    println!("while dragging: {during:.0?}");
    println!("the most: {:.0} MB; settled: {settled:.0} MB", during.iter().copied().fold(0.0, f64::max));
}

/// Values worth trying for a parameter: its default, the ends of its slider, and what can be
/// typed past them (or what its limits hold it to); for a choice, every option and ones a file
/// could hold that no option is.
fn edge_values(pd: &brewed_texture_core::nodes::ParamDef) -> Vec<ParamValue> {
    use brewed_texture_core::nodes::ParamKind;
    let limit = |v: f32| match pd.limits {
        Some((lo, hi)) => v.clamp(lo, hi),
        None => v,
    };
    let mut out = Vec::new();
    match pd.kind {
        ParamKind::Float { default, min, max, .. } => {
            let span = (max - min).abs().max(1.0);
            for v in [default, min, max, 0.0, -1.0, 1.0, max + span * 10.0, min - span * 10.0, 1e6, -1e6, 1e-6] {
                out.push(ParamValue::Float(limit(v)));
            }
        }
        ParamKind::Int { default, min, max } => {
            for v in [
                default as i64,
                min as i64,
                max as i64,
                0,
                -1,
                max as i64 * 10 + 10,
                min as i64 - 100,
                100_000,
                i32::MAX as i64,
                i32::MIN as i64 + 1,
            ] {
                out.push(ParamValue::Int(limit(v as f32) as i64 as i32));
            }
        }
        ParamKind::Vec2 { default, min, max } => {
            let span = (max - min).abs().max(1.0);
            for v in [
                default,
                [min, min],
                [max, max],
                [0.0, 0.0],
                [max + span * 10.0; 2],
                [min - span * 10.0; 2],
                [1e6, -1e6],
            ] {
                out.push(ParamValue::Vec2([limit(v[0]), limit(v[1])]));
            }
        }
        ParamKind::Color { default } => {
            for v in [default, [0.0; 4], [1.0; 4], [-10.0, -10.0, -10.0, -1.0], [10.0; 4], [1e6; 4]] {
                out.push(ParamValue::Color(v));
            }
        }
        ParamKind::Enum { default, options, .. } => {
            out.extend((0..options.len() as u32).map(ParamValue::Enum));
            for v in [default, options.len() as u32, 1000, u32::MAX] {
                out.push(ParamValue::Enum(v));
            }
        }
        ParamKind::Bool { .. } => out.extend([ParamValue::Bool(false), ParamValue::Bool(true)]),
        ParamKind::Text { .. } => out.extend([ParamValue::Text(String::new()), ParamValue::Text("x".repeat(10_000))]),
        ParamKind::File { .. } => out.extend([
            ParamValue::Text(String::new()),
            ParamValue::Text("does/not/exist.png".into()),
            ParamValue::Text(std::env::temp_dir().display().to_string()),
            ParamValue::Text(file!().into()),
        ]),
        ParamKind::Expression { language, .. } => {
            use brewed_texture_core::expr::Language;
            let texts: &[&str] = if language == Language::Splats {
                &[
                    "size = 0.1",
                    "x = 1/0",
                    "size = sqrt(-1)",
                    "size = 1e30",
                    "size = 100, 0.001",
                    "size = -1",
                    "value = log(0); angle = 1e30",
                    "color = 1/0, 0, 0",
                    "image = 1/0; draw = rand(1) < 0.5",
                    "x = ",
                    "size = a",
                ]
            } else if language == Language::Pixel {
                &[
                    "u",
                    "0",
                    "1/0",
                    "sqrt(-1)",
                    "log(0)",
                    "u*1e30*1e30",
                    "((((((((u))))))))",
                    "u +",
                    "unknown_function(u)",
                    "a",
                ]
            } else {
                &["0", "1/0", "sqrt(-1)", "1e40*1e40", "1 +", "x", "pow(2, 1000)"]
            };
            out.extend(texts.iter().map(|t| ParamValue::Text((*t).into())));
        }
        _ => {}
    }
    out
}

/// One variant of a node under stress: evaluated twice (the first time builds its pipelines),
/// then what went wrong, if anything. `at_4k` scales its time from the resolution of `g` to
/// 4096×4096. Returns the engine to go on with (a new one after a panic).
fn stress(
    mut e: Engine,
    g: &Graph,
    id: NodeId,
    what: &str,
    findings: &mut Vec<String>,
    times: &mut Vec<(f64, String)>,
) -> Engine {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let pixels = f64::from(g.resolution[0]) * f64::from(g.resolution[1]);
    let result = catch_unwind(AssertUnwindSafe(|| {
        evaluate_blocking(&mut e, g);
        // Computed once more, alone: bypassed and back gives it a new key, and what it reads
        // is there.
        let mut changed = g.clone();
        if let Some(n) = changed.node_mut(id) {
            n.bypass = !n.bypass;
        }
        evaluate_blocking(&mut e, &changed);
        let t = std::time::Instant::now();
        evaluate_blocking(&mut e, g);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let nans = e.read_image(id).ok().map(|(_, _, px)| px.iter().filter(|v| !v.is_finite()).count());
        (ms, nans)
    }));
    match result {
        Err(panic) => {
            let message = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            findings.push(format!("PANIC {what}: {message}"));
            return engine().expect("the GPU is still there");
        }
        Ok((ms, nans)) => {
            let errors = e.gpu.take_errors();
            if !errors.is_empty() {
                findings.push(format!("GPU ERROR {what}: {}", errors.join(" | ")));
            }
            if let Some(n) = nans.filter(|n| *n > 0) {
                findings.push(format!("NaN {what}: {n} values"));
            }
            // (What a small image takes is mostly the cost of a submission, which does not grow.)
            if pixels >= 512.0 * 512.0 {
                times.push((ms * 4096.0 * 4096.0 / pixels, what.to_string()));
            }
        }
    }
    e
}

/// A node of `kind` alone with what it reads: grayscale or color images at its inputs.
fn node_with_inputs(kind: NodeKind, color: bool, resolution: [u32; 2]) -> (Graph, NodeId) {
    let mut g = Graph { resolution, ..Graph::default() };
    let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    let src = if color {
        let hsl = g.add_node(NodeKind::HSL, [0.0, 0.0]);
        g.connect(noise, "o:output", hsl, "i:input").unwrap();
        hsl
    } else {
        noise
    };
    let id = g.add_node(kind, [0.0, 0.0]);
    for sd in kind.def().inputs {
        let _ = g.connect(src, "o:output", id, &brewed_texture_core::graph::input_socket_key(sd.key));
    }
    (g, id)
}

/// Every parameter of every image node at its edges, one at a time from the node as it is
/// added, with grayscale and with color images arriving, at 1024×1024; then every node with
/// all its parameters at random edges at once, at sizes that are not powers of two. Prints
/// what panics, makes a GPU error or a NaN, and the slowest, with the time each would take
/// at 4096×4096 (a graphics driver resets a device that works on one thing for about 2 s).
#[test]
#[ignore = "a stress test"]
fn every_parameter_at_its_edges() {
    let Some(mut e) = engine() else { return };
    let mut findings: Vec<String> = Vec::new();
    let mut times: Vec<(f64, String)> = Vec::new();
    let started = std::time::Instant::now();
    let kinds: Vec<NodeKind> = NodeKind::ALL.iter().copied().filter(|k| k.def().has_image()).collect();
    for &kind in &kinds {
        let d = kind.def();
        for color in [false, true] {
            if d.inputs.is_empty() && color {
                continue;
            }
            let (g, id) = node_with_inputs(kind, color, [1024, 1024]);
            for pd in d.params {
                for value in edge_values(pd) {
                    let mut g = g.clone();
                    let Some(param) = g.node_mut(id).and_then(|n| n.param_mut(pd.key)) else { continue };
                    param.value = value.clone();
                    let what =
                        format!("{} ({}) {} = {:?}", d.name, if color { "color" } else { "gray" }, pd.key, value);
                    e = stress(e, &g, id, &what, &mut findings, &mut times);
                }
            }
        }
    }
    let one_at_a_time = times.len();
    // All at once, at random, at odd sizes.
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for &kind in &kinds {
        let d = kind.def();
        for round in 0..12 {
            let resolution = [[777, 333], [16, 16], [1000, 16], [333, 1024]][round % 4];
            let (mut g, id) = node_with_inputs(kind, round % 2 == 1, resolution);
            let mut chosen = Vec::new();
            for pd in d.params {
                let values = edge_values(pd);
                if values.is_empty() {
                    continue;
                }
                let v = values[(next() % values.len() as u64) as usize].clone();
                chosen.push(format!("{}={:?}", pd.key, v));
                if let Some(param) = g.node_mut(id).and_then(|n| n.param_mut(pd.key)) {
                    param.value = v;
                }
            }
            let what = format!("{} at {resolution:?} with {}", d.name, chosen.join(", "));
            e = stress(e, &g, id, &what, &mut findings, &mut times);
        }
    }
    println!(
        "{} evaluations ({one_at_a_time} one parameter at a time) in {:.1} s, {} findings",
        times.len(),
        started.elapsed().as_secs_f64(),
        findings.len()
    );
    for f in &findings {
        println!("{}", f.chars().take(300).collect::<String>());
    }
    times.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!("the slowest, as they would take at 4096x4096:");
    for (ms, what) in times.iter().take(25) {
        println!("  {:7.0} ms  {}", ms, what.chars().take(200).collect::<String>());
    }
}

/// What the demo takes in GPU memory at 2048×2048, as the app shows it (prints).
#[test]
#[ignore = "a measurement"]
fn demo_memory() {
    let Some(mut e) = engine() else { return };
    let (mut g, preview) = brewed_texture_core::demo::graph();
    g.resolution = [2048, 2048];
    let outputs: Vec<NodeId> = g.nodes_of(NodeKind::OUTPUT).filter(|n| n.enum_value("usage") != 0).map(|n| n.id).collect();
    e.set_keep(crate::engine::Keep::Read);
    e.set_preview(preview);
    e.set_materials(&outputs);
    evaluate_blocking(&mut e, &g);
    e.prepare_preview(preview);
    e.prepare_materials(&outputs);
    e.render_thumbnails();
    e.wait_idle();
    let mb = |b: u64| b as f64 / (1024.0 * 1024.0);
    println!("demo at 2048x2048: {:.0} MB, of which {:.0} MB scratch images", mb(e.memory()), mb(e.gpu.pool_bytes()));
    e.settle();
    println!("settled: {:.0} MB", mb(e.memory()));
    // The images that stay.
    for n in &g.nodes {
        if let Some(img) = e.image(n.id) {
            let name = if n.kind == NodeKind::OUTPUT { n.text("name").to_string() } else { n.def().name.to_string() };
            println!("  {name:<20} {:?}, {} mips: {:.1} MB", img.format(), img.texture.mip_level_count(), mb(img.bytes()));
        }
    }
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

/// A bypassed node passes on what arrives at it and keeps no image of its own; an output
/// keeps the image it writes.
#[test]
fn bypassed_nodes_share_their_input() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    let invert = g.add_node(NodeKind::INVERT, [0.0, 0.0]);
    g.connect(noise, "o:output", invert, "i:input").unwrap();
    let out = g.add_node(NodeKind::OUTPUT, [0.0, 0.0]);
    g.connect(invert, "o:output", out, "i:input").unwrap();
    evaluate_blocking(&mut e, &g);
    let texture = |e: &Engine, id| e.image(id).map(|i| i.texture.clone());
    assert!(texture(&e, out).is_some() && texture(&e, out) != texture(&e, invert), "the output writes an image of its own");
    // (Without the scratch images, which go once nothing changes.)
    e.gpu.clear_pool();
    let memory = e.memory();

    g.node_mut(invert).unwrap().bypass = true;
    evaluate_blocking(&mut e, &g);
    assert_eq!(texture(&e, invert), texture(&e, noise), "a bypassed node is its input");
    e.gpu.clear_pool();
    assert!(e.memory() < memory, "{} is not less than {memory}", e.memory());
    // Back again: the node makes an image of its own, and does not write into its input's.
    let before = e.read_image(noise).unwrap().2;
    g.node_mut(invert).unwrap().bypass = false;
    evaluate_blocking(&mut e, &g);
    assert_ne!(texture(&e, invert), texture(&e, noise));
    assert_eq!(e.read_image(noise).unwrap().2, before, "the input was written into");
    // The output writes 8 bits: to the nearest of 255 steps.
    let inverted = e.read_image(out).unwrap().2;
    assert!(close(pixel(&inverted, 9, 9)[0], 1.0 - pixel(&before, 9, 9)[0], 3e-3));
}

/// Kept as the app keeps them, once nothing changes the demo keeps the images its outputs
/// write for the views and nothing else, the same whatever was changed before: each in its
/// output's channels and at its bit depth, with its mips.
#[test]
fn the_demo_settles_on_what_its_outputs_write() {
    use wgpu::TextureFormat as F;
    let Some(mut e) = engine() else { return };
    let (mut g, preview) = brewed_texture_core::demo::graph();
    g.resolution = [512, 512];
    let outputs: Vec<NodeId> = g.nodes_of(NodeKind::OUTPUT).filter(|n| n.enum_value("usage") != 0).map(|n| n.id).collect();
    // As the app runs it.
    e.set_keep(crate::engine::Keep::Read);
    e.set_preview(preview);
    e.set_materials(&outputs);
    evaluate_blocking(&mut e, &g);
    assert!(e.prepare_preview(preview).is_some());
    assert!(e.prepare_materials(&outputs).is_some_and(|m| m.len() == outputs.len()));
    e.settle();
    let settled = e.memory_report();
    assert_eq!((settled.shown_images, settled.kept, settled.scratch), (outputs.len(), 0, 0));
    // Base color and normal in RGB of 8 bits (kept with alpha), the height in one channel of
    // 16, roughness and occlusion in one of 8.
    let formats: Vec<F> = outputs.iter().map(|o| e.image(*o).unwrap().format()).collect();
    assert_eq!(formats, [F::Rgba8Unorm, F::Rgba8Unorm, F::R16Float, F::R8Unorm, F::R8Unorm]);
    let mb = settled.total() as f64 / (1024.0 * 1024.0);
    // 64 MB at 2048: 48 of maps, 16 of mips.
    assert!(mb < 4.1, "the demo takes {mb:.1} MB at 512");
    // Dragging an input value keeps what the next step reads; settled again, the same.
    let wear = g.variables.iter().position(|v| v.name == "wear").unwrap();
    for value in [0.6, 0.65, 0.55] {
        g.variables[wear].value = Value::Float(value);
        evaluate_blocking(&mut e, &g);
        e.prepare_preview(preview);
        e.prepare_materials(&outputs);
    }
    assert!(e.memory_report().kept > 0);
    e.settle();
    assert_eq!(e.memory_report(), settled);
}

/// What wgpu's allocator holds of the GPU's memory once nine demos in one graph are computed,
/// on each backend it reports on (prints).
#[test]
#[ignore = "a measurement"]
fn memory_the_allocator_keeps() {
    let (demo, preview) = brewed_texture_core::demo::graph();
    let mut g = demo.clone();
    let ids: Vec<NodeId> = demo.nodes.iter().map(|n| n.id).collect();
    for k in 1..=8 {
        g.paste(&demo.copy_nodes(&ids), [0.0, 900.0 * k as f32]);
    }
    let mb = |b: u64| b as f64 / (1024.0 * 1024.0);
    for backends in [wgpu::Backends::DX12, wgpu::Backends::VULKAN, wgpu::Backends::METAL] {
        let Ok(gpu) = Gpu::headless_on(backends) else { continue };
        let mut e = Engine::new(gpu);
        e.set_keep(crate::engine::Keep::Read);
        e.set_preview(preview);
        let held = |e: &Engine| {
            let report = e.gpu.device.generate_allocator_report()?;
            Some((mb(report.total_allocated_bytes), mb(report.total_reserved_bytes)))
        };
        let before = held(&e);
        evaluate_blocking(&mut e, &g);
        let _ = e.render_thumbnails();
        let computed = held(&e);
        e.settle();
        e.wait_idle();
        let settled = held(&e);
        let name = &e.gpu.adapter_info;
        println!("{name}, MB allocated and reserved: {before:.1?} before, {computed:.1?} computed");
        println!("{name}, MB allocated and reserved: {settled:.1?} settled");
    }
}

/// The demo nine times in one graph, "mortar" dragged through the engine thread as the app
/// drags it, then nothing for a while: to watch the process's memory from outside (prints its
/// id, and when each part starts).
#[test]
#[ignore = "a measurement"]
fn a_large_graph_dragged_through_the_service() {
    use crate::service::{EngineService, Event, Request};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    let Ok(gpu) = Gpu::headless() else { return };
    println!("pid {} on {}", std::process::id(), gpu.adapter_info);
    let finished = Arc::new(Mutex::new(0u64));
    let done = finished.clone();
    let emit: crate::service::Emit = Arc::new(move |event| {
        if let Event::Finished { revision, .. } = event {
            *done.lock().unwrap() = revision;
        }
    });
    let service = EngineService::spawn(gpu.device.clone(), gpu.queue.clone(), gpu.adapter_info.clone(), emit);
    let (demo, preview) = brewed_texture_core::demo::graph();
    let materials: Vec<NodeId> =
        demo.nodes_of(NodeKind::OUTPUT).filter(|n| n.enum_value("usage") != 0).map(|n| n.id).collect();
    let mut g = demo.clone();
    let ids: Vec<NodeId> = demo.nodes.iter().map(|n| n.id).collect();
    // (BREWED_COPIES copies of the demo besides it, 8 if it does not say.)
    let copies: u32 = std::env::var("BREWED_COPIES").ok().and_then(|c| c.parse().ok()).unwrap_or(8);
    for k in 1..=copies {
        g.paste(&demo.copy_nodes(&ids), [0.0, 900.0 * k as f32]);
    }
    let send = |g: &Graph, revision: u64| {
        let eval = brewed_texture_core::eval::evaluate(g);
        service.send(Request::Evaluate { revision, graph: Arc::new(g.clone()), eval: Arc::new(eval) });
    };
    let wait_for = |revision: u64| {
        while *finished.lock().unwrap() < revision {
            std::thread::sleep(Duration::from_millis(1));
        }
    };
    let started = Instant::now();
    let say = |what: &str| println!("{:6.1} s {what}", started.elapsed().as_secs_f64());
    std::thread::sleep(Duration::from_secs(3));
    say("computing");
    service.send(Request::Preview(preview));
    service.send(Request::Materials(materials));
    send(&g, 1);
    wait_for(1);
    say("computed");
    std::thread::sleep(Duration::from_secs(4));
    say("dragging");
    let mortar = g.variables.iter().position(|v| v.name == "mortar").unwrap();
    for step in 0..=60u64 {
        g.variables[mortar].value = Value::Float(0.02 + 0.001 * step as f32);
        send(&g, 2 + step);
        std::thread::sleep(Duration::from_millis(16));
    }
    wait_for(62);
    say("dragged");
    std::thread::sleep(Duration::from_secs(8));
    say("done");
}

/// Pictures larger than a texture can be are refused before they are decoded, saying why; a
/// file that is no picture says so.
#[test]
fn pictures_too_large_are_refused_at_once() {
    let mut png = Vec::new();
    image::GrayImage::new(8193, 1).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let error = crate::open_picture(&png, "wide.png").unwrap_err();
    assert_eq!(error, "'wide.png' is larger than 8192 × 8192 pixels");
    assert!(crate::open_picture(b"not a picture", "notes.png").unwrap_err().starts_with("Cannot open 'notes.png'"));
    let picture = crate::open_picture(&png[..], "wide").unwrap_err();
    assert!(picture.contains("larger than"), "by what it starts with: {picture}");
}

/// The largest picture a Bitmap reads, of 32-bit floats, is read (it takes 1 GB decoded).
#[test]
#[ignore = "takes 2 GB of memory"]
fn the_largest_pictures_are_read() {
    let dir = std::env::temp_dir().join(format!("btd-large-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("large.exr");
    exr::prelude::write_rgba_file(&path, 8192, 8192, |x, y| (x as f32 / 8192.0, y as f32 / 8192.0, 0.5f32, 1.0f32)).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let picture = crate::open_picture(&bytes, "large.exr").unwrap();
    assert_eq!((picture.width(), picture.height()), (8192, 8192));
    let _ = std::fs::remove_dir_all(&dir);
}

/// An Image Input nothing arrives at shows its picture, on the GPU as a Bitmap of the picture
/// would: the one the node that uses its graph gives it, else the graph's; made grayscale for an
/// input that takes grayscale, as it is for one that takes color. Without a picture, its color:
/// its luminance, in a grayscale image, for an input that takes grayscale.
#[test]
fn image_inputs_show_their_pictures_as_bitmaps_would() {
    use brewed_texture_core::package::{Library, Package, flatten};
    use brewed_texture_core::{GraphRef, Instance, Interface};
    use std::sync::Arc;

    struct One(Package);
    impl Library for One {
        fn package(&self, path: Option<&str>) -> Option<&Package> {
            path.is_none().then_some(&self.0)
        }
    }

    let Some(mut e) = engine() else { return };
    let dir = std::env::temp_dir().join(format!("btd-input-pictures-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let across = dir.join("across.png");
    image::RgbaImage::from_fn(64, 64, |x, y| image::Rgba([(x * 4) as u8, 255 - (y * 4) as u8, 90, 255])).save(&across).unwrap();
    let plain = dir.join("plain.png");
    image::RgbaImage::from_pixel(8, 8, image::Rgba([40, 160, 220, 255])).save(&plain).unwrap();
    let (across, plain) = (across.display().to_string(), plain.display().to_string());

    // A graph that gives out what arrives at its input (grayscale), whose picture is plain.
    let mut shows = graph();
    let input = shows.add_node(NodeKind::IMAGE_INPUT, [0.0, 0.0]);
    let out = shows.add_node(NodeKind::OUTPUT, [300.0, 0.0]);
    shows.connect(input, "o:output", out, "i:input").unwrap();
    let name = shows.node(input).unwrap().variable.clone().unwrap();
    shows.variable_mut(&name).unwrap().image.as_mut().unwrap().picture = plain.clone();
    let mut package = Package::new("Main", graph());
    let used = package.add_graph("Shows", shows);
    let main = package.first();
    let interface = Interface::of(&package.graph(used).unwrap().graph);
    let instance = Instance::new(GraphRef { package: None, graph: used, name: "Shows".into() }, interface);
    let g = Arc::make_mut(&mut package.graph_mut(main).unwrap().graph);
    let own = g.add_instance(instance.clone(), [0.0, 0.0]);
    let given = g.add_instance(instance, [0.0, 300.0]);
    set(g, given, &name, ParamValue::Text(across.clone()));

    // Each picture as a Bitmap reads it, made grayscale or not.
    let read = |e: &mut Engine, path: &str, gray: bool| {
        let mut g = graph();
        let b = g.add_node(NodeKind::BITMAP, [0.0, 0.0]);
        set(&mut g, b, "path", ParamValue::Text(path.to_string()));
        if !gray {
            return eval_read(e, &g, b);
        }
        let c = g.add_node(NodeKind::GRAYSCALE, [0.0, 0.0]);
        g.connect(b, "o:output", c, "i:input").unwrap();
        eval_read(e, &g, c)
    };
    let flat = flatten(&One(package.clone()), None, main).unwrap();
    assert!(flat.problems.is_empty(), "{:?}", flat.problems);
    assert_eq!(eval_read(&mut e, &flat.graph, own), read(&mut e, &plain, true), "the graph's picture");
    assert_eq!(eval_read(&mut e, &flat.graph, given), read(&mut e, &across, true), "the node's picture");
    // An input that takes color: the pictures as they are.
    let s = Arc::make_mut(&mut package.graph_mut(used).unwrap().graph);
    s.variable_mut(&name).unwrap().image.as_mut().unwrap().kind = brewed_texture_core::nodes::ImageKind::Color;
    let flat = flatten(&One(package.clone()), None, main).unwrap();
    assert_eq!(eval_read(&mut e, &flat.graph, own), read(&mut e, &plain, false));
    assert_eq!(eval_read(&mut e, &flat.graph, given), read(&mut e, &across, false));
    // No picture: its color, as it is, or its luminance in a grayscale image of one pixel.
    let s = Arc::make_mut(&mut package.graph_mut(used).unwrap().graph);
    let var = s.variable_mut(&name).unwrap();
    var.value = brewed_texture_core::Value::Color([0.2, 0.4, 0.6, 1.0]);
    var.image.as_mut().unwrap().picture.clear();
    let flat = flatten(&One(package.clone()), None, main).unwrap();
    let shown = pixel(&eval_read(&mut e, &flat.graph, own), 0, 0);
    assert!(shown.iter().zip([0.2, 0.4, 0.6, 1.0]).all(|(a, b)| close(*a, b, 1e-3)), "its color: {shown:?}");
    let s = Arc::make_mut(&mut package.graph_mut(used).unwrap().graph);
    s.variable_mut(&name).unwrap().image.as_mut().unwrap().kind = brewed_texture_core::nodes::ImageKind::Gray;
    let flat = flatten(&One(package), None, main).unwrap();
    evaluate_blocking(&mut e, &flat.graph);
    let kind = e.reports().get(&own).and_then(|r| r.kind);
    assert_eq!(kind, Some(brewed_texture_core::nodes::ImageKind::Gray), "grayscale");
    let (_, _, data) = e.read_image(own).unwrap();
    let l = 0.2126 * 0.2 + 0.7152 * 0.4 + 0.0722 * 0.6;
    assert!(close(data[0], l, 2e-3), "its luminance: {:?}", &data[..4]);
    assert!(e.gpu.take_errors().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// What is not a number in Pixel Math (1/0 of nothing, the root of a negative, the log of 0) is
/// none, and what is infinite the largest a half float holds: nothing spreads from it through
/// what reads it.
#[test]
fn pixel_math_makes_no_nan() {
    let Some(mut e) = engine() else { return };
    // (What overflows as it is computed is infinite or not a number, as the GPU has it: finite
    // here, whichever.)
    let cases = [
        ("sqrt(-1)", Some(0.0)),
        ("log(0)", Some(-65504.0)),
        ("1 / (u - u)", Some(65504.0)),
        ("0 * (1 / (u - u))", Some(0.0)),
        ("u * 1e30 * 1e30 + 1", None),
    ];
    for (expression, want) in cases {
        let mut g = graph();
        let pm = g.add_node(NodeKind::PIXEL_MATH, [0.0, 0.0]);
        set(&mut g, pm, "expression", ParamValue::Text(expression.into()));
        let blur = g.add_node(NodeKind::BLUR, [0.0, 0.0]);
        g.connect(pm, "o:output", blur, "i:input").unwrap();
        let data = eval_read(&mut e, &g, blur);
        assert!(data.iter().all(|v| v.is_finite()), "{expression}: not finite");
        let shown = pixel(&eval_read(&mut e, &g, pm), RES / 2, RES / 2)[0];
        assert!(shown.is_finite() && want.is_none_or(|want| close(shown, want, 1.0)), "{expression}: {shown}, not {want:?}");
    }
    assert!(e.gpu.take_errors().is_empty());
}


/// The deepest expressions Pixel Math reads (chains of as many operations as an expression
/// can have, negations as deep as they nest, `fract` of `fract`…, a `max` of more than a
/// thousand values) are shaders the GPU takes, and compute what the CPU does.
#[test]
fn the_deepest_pixel_math_is_a_shader() {
    let Some(mut e) = engine() else { return };
    let longest = brewed_texture_core::expr::parser::MAX_TREE - 1;
    let cases = [
        "u+".repeat(longest) + "u",
        "u<".repeat(longest) + "u",
        "-".repeat(94) + "u",
        "(u+".repeat(40) + &"u*".repeat(80) + "u" + &")".repeat(40),
        "fract(".repeat(40) + "u * 3" + &")".repeat(40),
        "max(".to_string() + &"u, ".repeat(1300) + "0.75)",
    ];
    let (x, y) = (RES / 3, RES / 5);
    let u = (x as f32 + 0.5) / RES as f32;
    for expression in cases {
        let mut g = graph();
        let pm = g.add_node(NodeKind::PIXEL_MATH, [0.0, 0.0]);
        set(&mut g, pm, "expression", ParamValue::Text(expression.clone()));
        let want = brewed_texture_core::expr::compile(&expression).unwrap().eval(&[f64::from(u)]) as f32;
        let got = pixel(&eval_read(&mut e, &g, pm), x, y)[0];
        assert!(close(got, want, 1e-2 * want.abs().max(1.0)), "{}…: {got}, not {want}", &expression[..24]);
    }
    assert!(e.gpu.take_errors().is_empty());
}

// ------------------------------------------------------------------ FX-Map

const SQUARE: u32 = 1;
const PATTERN_IMAGE: u32 = brewed_texture_core::nodes::TILE_PATTERN_IMAGE;
const MAX: u32 = 0;
const ADD: u32 = 1;
const STACK: u32 = 2;
const SUBTRACT: u32 = 3;
const MIN: u32 = 4;
const MULTIPLY: u32 = 5;

/// An FX-Map running `program`, `count` splats a cell at the depths `from` to `depth`, each a
/// square blending by `blend`.
fn fx_map(g: &mut Graph, program: &str, count: i32, (from, depth): (i32, i32), blend: u32) -> NodeId {
    let fx = g.add_node(NodeKind::FX_MAP, [0.0, 0.0]);
    set(g, fx, "program", ParamValue::Text(program.into()));
    set(g, fx, "count", ParamValue::Int(count));
    set(g, fx, "from_depth", ParamValue::Int(from));
    set(g, fx, "depth", ParamValue::Int(depth));
    set(g, fx, "pattern", ParamValue::Enum(SQUARE));
    set(g, fx, "blend", ParamValue::Enum(blend));
    fx
}

/// A grayscale Uniform Color of `v`.
fn gray_color(g: &mut Graph, v: f32) -> NodeId {
    let c = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
    set(g, c, "color", ParamValue::Color([v, v, v, 1.0]));
    set(g, c, "output", ParamValue::Enum(1));
    c
}

/// Splats go where the program says, as big and as turned as it says; at depth 2 one in each of
/// the 4 × 4 cells.
#[test]
fn fx_map_draws_where_its_program_says() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    // A quarter of the image wide (64 px), its middle a quarter across and halfway down.
    let fx = fx_map(&mut g, "x = 0.25; y = 0.5; size = 0.25", 1, (0, 0), MAX);
    let px = eval_read(&mut e, &g, fx);
    for (x, y, v) in [(64, 128, 1.0), (36, 100, 1.0), (91, 155, 1.0), (100, 128, 0.0), (64, 164, 0.0), (27, 128, 0.0), (192, 128, 0.0)] {
        assert!(close(pixel(&px, x, y)[0], v, 1e-3), "({x}, {y}): {:?}", pixel(&px, x, y));
    }
    assert_eq!(e.image(fx).unwrap().format(), wgpu::TextureFormat::R16Float, "a grayscale image");
    // Turned by 45 degrees: a diamond reaching 45 px along the axes, not the corners.
    set(&mut g, fx, "program", ParamValue::Text("x = 0.25; y = 0.5; size = 0.25; angle = 45".into()));
    let px = eval_read(&mut e, &g, fx);
    assert!(close(pixel(&px, 64 + 40, 128)[0], 1.0, 1e-3));
    assert!(close(pixel(&px, 64 + 28, 128 + 28)[0], 0.0, 1e-3));
    // Width and height of their own.
    set(&mut g, fx, "program", ParamValue::Text("x = 0.25; y = 0.5; size = 0.25, 0.125".into()));
    let px = eval_read(&mut e, &g, fx);
    assert!(close(pixel(&px, 64 + 28, 128)[0], 1.0, 1e-3));
    assert!(close(pixel(&px, 64, 128 + 20)[0], 0.0, 1e-3));
    // In the middle of each of 16 cells, half as big as a cell.
    set(&mut g, fx, "program", ParamValue::Text("size = 0.5".into()));
    set(&mut g, fx, "depth", ParamValue::Int(2));
    set(&mut g, fx, "from_depth", ParamValue::Int(2));
    let px = eval_read(&mut e, &g, fx);
    for cell in 0..4 {
        let c = 32 + 64 * cell;
        assert!(close(pixel(&px, c, 32 + 64 * ((cell + 1) % 4))[0], 1.0, 1e-3), "cell {cell}");
        assert!(close(pixel(&px, 64 * cell + 4, 64 * cell + 4)[0], 0.0, 1e-3), "the corner of cell {cell}");
    }
    // What the program reads of each: here, a brighter splat a row further down.
    set(&mut g, fx, "program", ParamValue::Text("size = 0.5; value = (row + 1) / cells".into()));
    let px = eval_read(&mut e, &g, fx);
    for row in 0..4 {
        assert!(close(pixel(&px, 96, 32 + 64 * row)[0], (row + 1) as f32 / 4.0, 1e-3), "row {row}");
    }
    // The depths from From Depth to Depth, the finer over the coarser.
    set(&mut g, fx, "program", ParamValue::Text("size = 0.5; value = (depth + 1) / 4".into()));
    set(&mut g, fx, "from_depth", ParamValue::Int(0));
    set(&mut g, fx, "blend", ParamValue::Enum(STACK));
    let px = eval_read(&mut e, &g, fx);
    assert!(close(pixel(&px, 32, 32)[0], 0.75, 1e-3), "depth 2 over the rest: {:?}", pixel(&px, 32, 32));
    assert!(close(pixel(&px, 64 + 4, 64 + 4)[0], 0.5, 1e-3), "depth 1 where depth 2 is not");
    assert!(close(pixel(&px, 128, 128)[0], 0.25, 1e-3), "depth 0 alone in the middle");
    assert!(e.gpu.take_errors().is_empty());
}

/// A splat over an edge the image tiles on comes back on the other side; where it does not
/// tile, it ends at the edge. The FX-Map as it is added tiles.
#[test]
fn fx_map_splats_wrap_where_the_image_tiles() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let fx = fx_map(&mut g, "x = 0; y = 0.5; size = 0.25", 1, (0, 0), MAX);
    let px = eval_read(&mut e, &g, fx);
    assert!(close(pixel(&px, 4, 128)[0], 1.0, 1e-3));
    assert!(close(pixel(&px, 250, 128)[0], 1.0, 1e-3), "across the edge");
    assert!(close(pixel(&px, 250, 250)[0], 0.0, 1e-3));
    // At a corner: in all four.
    set(&mut g, fx, "program", ParamValue::Text("x = 1; y = 1; size = 0.25".into()));
    let px = eval_read(&mut e, &g, fx);
    for (x, y) in [(4, 4), (250, 4), (4, 250), (250, 250)] {
        assert!(close(pixel(&px, x, y)[0], 1.0, 1e-3), "({x}, {y})");
    }
    set(&mut g, fx, "tiling", ParamValue::Enum(0));
    let px = eval_read(&mut e, &g, fx);
    assert!(close(pixel(&px, 250, 250)[0], 1.0, 1e-3));
    assert!(close(pixel(&px, 4, 4)[0], 0.0, 1e-3), "no tiling: no copy");
    // As it is added.
    let fx = g.add_node(NodeKind::FX_MAP, [0.0, 0.0]);
    let px = eval_read(&mut e, &g, fx);
    let (across, inside) = seams(&px, RES);
    assert!(across <= inside * 1.25 + 1e-3, "{across} across the edges, at most {inside} anywhere else");
    assert!(mean(&px) > 0.05, "it draws: {}", mean(&px));
    // The same splats every time; others with another seed.
    assert_eq!(eval_read(&mut e, &g, fx), px);
    set(&mut g, fx, "seed", ParamValue::Int(7));
    assert_ne!(eval_read(&mut e, &g, fx), px);
    assert!(e.gpu.take_errors().is_empty());
}

/// How a splat combines with what is under it: the brighter wins, they add up, the later lies
/// over the earlier, it takes itself away, the darker wins, it multiplies.
#[test]
fn fx_map_blends_as_it_is_told() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let value_at_middle = |e: &mut Engine, g: &Graph, fx: NodeId| {
        let px = eval_read(e, g, fx);
        (pixel(&px, 128, 128)[0], pixel(&px, 10, 10)[0])
    };
    // Two splats, the second darker.
    let program = "size = 0.25; value = 0.75 - 0.5 * i";
    let fx = fx_map(&mut g, program, 2, (0, 0), MAX);
    assert_eq!(value_at_middle(&mut e, &g, fx), (0.75, 0.0));
    set(&mut g, fx, "blend", ParamValue::Enum(STACK));
    assert_eq!(value_at_middle(&mut e, &g, fx), (0.25, 0.0));
    set(&mut g, fx, "blend", ParamValue::Enum(ADD));
    assert_eq!(value_at_middle(&mut e, &g, fx), (1.0, 0.0));
    // Over a background.
    let background = gray_color(&mut g, 0.5);
    g.connect(background, "o:output", fx, "i:background").unwrap();
    set(&mut g, fx, "program", ParamValue::Text("size = 0.25; value = 0.25".into()));
    set(&mut g, fx, "count", ParamValue::Int(1));
    assert_eq!(value_at_middle(&mut e, &g, fx), (0.75, 0.5));
    set(&mut g, fx, "blend", ParamValue::Enum(SUBTRACT));
    assert_eq!(value_at_middle(&mut e, &g, fx), (0.25, 0.5));
    set(&mut g, fx, "blend", ParamValue::Enum(MIN));
    assert_eq!(value_at_middle(&mut e, &g, fx), (0.25, 0.5));
    set(&mut g, fx, "blend", ParamValue::Enum(MULTIPLY));
    assert_eq!(value_at_middle(&mut e, &g, fx), (0.125, 0.5));
    set(&mut g, fx, "blend", ParamValue::Enum(MAX));
    assert_eq!(value_at_middle(&mut e, &g, fx), (0.5, 0.5));
    // Opacity: half of it over what is there.
    set(&mut g, fx, "blend", ParamValue::Enum(STACK));
    set(&mut g, fx, "program", ParamValue::Text("size = 0.25; value = 1; opacity = 0.5".into()));
    assert_eq!(value_at_middle(&mut e, &g, fx), (0.75, 0.5));
    // Left out.
    set(&mut g, fx, "program", ParamValue::Text("size = 0.25; value = 1; draw = 0".into()));
    assert_eq!(value_at_middle(&mut e, &g, fx), (0.5, 0.5));
    assert!(e.gpu.take_errors().is_empty());
}

/// Colors make a color image (over opaque black); so do patterns in color, drawn as they
/// are; a grayscale pattern covers as much as it is bright.
#[test]
fn fx_map_colors_and_patterns() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let fx = fx_map(&mut g, "size = 0.25; color = 1, 0.5, 0", 1, (0, 0), MAX);
    let px = eval_read(&mut e, &g, fx);
    assert_eq!(pixel(&px, 128, 128), [1.0, 0.5, 0.0, 1.0]);
    assert_eq!(pixel(&px, 10, 10), [0.0, 0.0, 0.0, 1.0]);
    // A grayscale pattern: as much as it is bright.
    set(&mut g, fx, "program", ParamValue::Text("size = 0.25".into()));
    set(&mut g, fx, "pattern", ParamValue::Enum(PATTERN_IMAGE));
    let pattern = gray_color(&mut g, 0.8);
    g.connect(pattern, "o:output", fx, "i:pattern_1").unwrap();
    let px = eval_read(&mut e, &g, fx);
    assert!(close(pixel(&px, 128, 128)[0], 0.8, 1e-3), "{:?}", pixel(&px, 128, 128));
    assert!(close(pixel(&px, 128 + 36, 128)[0], 0.0, 1e-3));
    // In color, as it is; the second pattern for image = 2, none where nothing arrives.
    let colored = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
    set(&mut g, colored, "color", ParamValue::Color([0.2, 0.4, 0.6, 1.0]));
    g.connect(colored, "o:output", fx, "i:pattern_2").unwrap();
    set(&mut g, fx, "program", ParamValue::Text("size = 0.25; image = 2".into()));
    let px = eval_read(&mut e, &g, fx);
    let c = pixel(&px, 128, 128);
    assert!(close(c[0], 0.2, 2e-3) && close(c[1], 0.4, 2e-3) && close(c[2], 0.6, 2e-3), "{c:?}");
    set(&mut g, fx, "program", ParamValue::Text("size = 0.25; image = 3".into()));
    let px = eval_read(&mut e, &g, fx);
    assert_eq!(pixel(&px, 128, 128), [0.0, 0.0, 0.0, 1.0]);
    assert!(e.gpu.take_errors().is_empty());
}

/// The program reads images connected to its names (at the middle of the cell, or anywhere
/// with sample) and the input values of the graph it names.
#[test]
fn fx_map_reads_images_and_values() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let fx = fx_map(&mut g, "size = 0.25; value = m", 1, (0, 0), MAX);
    let m = gray_color(&mut g, 0.6);
    let socket = brewed_texture_core::graph::var_socket_key("m");
    g.connect(m, "o:output", fx, &socket).unwrap();
    let px = eval_read(&mut e, &g, fx);
    assert!(close(pixel(&px, 128, 128)[0], 0.6, 1e-3), "{:?}", pixel(&px, 128, 128));
    // Anywhere: u of Pixel Math, three quarters across.
    let across = g.add_node(NodeKind::PIXEL_MATH, [0.0, 0.0]);
    set(&mut g, across, "expression", ParamValue::Text("u".into()));
    set(&mut g, fx, "program", ParamValue::Text("size = 0.25; value = sample(m, 0.75, 0.5)".into()));
    g.connect(across, "o:output", fx, &socket).unwrap();
    let px = eval_read(&mut e, &g, fx);
    assert!(close(pixel(&px, 128, 128)[0], 0.75, 2e-3), "{:?}", pixel(&px, 128, 128));
    // An input value of the graph, by its name.
    let input = g.add_node(NodeKind::FLOAT_INPUT, [0.0, 0.0]);
    g.bind_variable(input, "k").unwrap();
    g.variable_mut("k").unwrap().value = Value::Float(0.7);
    set(&mut g, fx, "program", ParamValue::Text("size = 0.25; value = k".into()));
    let px = eval_read(&mut e, &g, fx);
    assert!(close(pixel(&px, 128, 128)[0], 0.7, 1e-3), "{:?}", pixel(&px, 128, 128));
    assert!(e.gpu.take_errors().is_empty());
}

/// In a graph of floats of 32 bits (which cannot be blended on every device) it draws the
/// same; a wrong program and too many splats say what is wrong.
#[test]
fn fx_map_in_full_precision_and_errors() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    g.precision = brewed_texture_core::Precision::Full;
    let fx = fx_map(&mut g, "x = 0.25; y = 0.5; size = 0.25; value = 0.3", 1, (0, 0), MAX);
    let px = eval_read(&mut e, &g, fx);
    assert_eq!(e.image(fx).unwrap().format(), wgpu::TextureFormat::R32Float);
    assert!(close(pixel(&px, 64, 128)[0], 0.3, 1e-3));
    assert!(close(pixel(&px, 192, 128)[0], 0.0, 1e-3));
    set(&mut g, fx, "program", ParamValue::Text("x = ".into()));
    let ev = evaluate_blocking(&mut e, &g);
    let error = ev.node(fx).and_then(|n| n.error()).map(str::to_string).or_else(|| e.state(fx).unwrap().error.clone());
    assert!(error.as_deref().is_some_and(|m| m.contains("program is incorrect")), "{error:?}");
    set(&mut g, fx, "program", ParamValue::Text("size = 0.1".into()));
    set(&mut g, fx, "count", ParamValue::Int(4096));
    set(&mut g, fx, "depth", ParamValue::Int(12));
    evaluate_blocking(&mut e, &g);
    let error = e.state(fx).unwrap().error.clone();
    assert!(error.as_deref().is_some_and(|m| m.contains("too many")), "{error:?}");
    assert!(e.gpu.take_errors().is_empty());
}

/// How long an FX-Map takes (prints; for tuning): splats of every size, a few and many.
#[test]
#[ignore = "a measurement"]
fn fx_map_costs() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    g.resolution = [2048, 2048];
    let fx = g.add_node(NodeKind::FX_MAP, [0.0, 0.0]);
    let start = std::time::Instant::now();
    evaluate_blocking(&mut e, &g);
    println!("the first, its program built: {:7.2} ms", start.elapsed().as_secs_f64() * 1000.0);
    for (count, from, depth) in [(64, 0, 0), (1000, 0, 0), (1, 0, 8), (16, 6, 6), (1, 0, 10), (4, 10, 10)] {
        set(&mut g, fx, "count", ParamValue::Int(count));
        set(&mut g, fx, "from_depth", ParamValue::Int(from));
        set(&mut g, fx, "depth", ParamValue::Int(depth));
        evaluate_blocking(&mut e, &g);
        let runs = 5;
        let start = std::time::Instant::now();
        for seed in 1..=runs {
            set(&mut g, fx, "seed", ParamValue::Int(seed));
            evaluate_blocking(&mut e, &g);
        }
        let splats: u64 = (from..=depth).map(|d| (count as u64) << (2 * d)).sum();
        println!("{splats:>8} splats ({count} a cell, depths {from} to {depth}): {:7.2} ms", start.elapsed().as_secs_f64() * 1000.0 / runs as f64);
    }
}

/// What FX-Maps make of a few programs, as pictures (`BREWED_UI_SNAPSHOTS=<folder>`, writes
/// `fx_map_<name>.png`): scattered discs, bricks, clouds, confetti, a map of sizes.
#[test]
#[ignore = "a picture to look at"]
fn fx_maps_as_pictures() {
    let Some(mut e) = engine() else { return };
    let Some(dir) = std::env::var_os("BREWED_UI_SNAPSHOTS") else { return };
    const SIZE: u32 = 512;
    // Name, program, count, depths, pattern, blend, background.
    type Look = (&'static str, &'static str, i32, (i32, i32), u32, u32, Option<f32>);
    let looks: &[Look] = &[
        ("as_added", brewed_texture_core::nodes::DEFAULT_FX_MAP_PROGRAM, 64, (0, 0), 0, MAX, None),
        (
            "bricks",
            "# Two bricks a cell, the lower row half a brick along.\ny = 0.25 + 0.5 * i\nx = 0.5 * i\nsize = 0.94, 0.44\nvalue = 0.45 + 0.55 * rand(1)",
            2,
            (3, 3),
            2,
            MAX,
            None,
        ),
        ("clouds", "size = 1.4\nvalue = (rand(1) - 0.5) * 0.6^depth", 1, (0, 8), 12, ADD, Some(0.5)),
        (
            "confetti",
            "x = rand(1); y = rand(2)\nsize = 0.02 + 0.03 * rand(3), 0.01 + 0.01 * rand(4)\nangle = 360 * rand(5)\ncolor = rand(6), rand(7), rand(8)",
            600,
            (0, 0),
            1,
            STACK,
            None,
        ),
        ("dots", "size = 0.9 * rand(1)^0.5\nvalue = 0.3 + 0.7 * rand(2)\ndraw = rand(3) < 0.8", 1, (5, 5), 0, MAX, None),
    ];
    for (name, program, count, depths, pattern, blend, background) in looks {
        let mut g = graph();
        g.resolution = [SIZE, SIZE];
        let fx = fx_map(&mut g, program, *count, *depths, *blend);
        set(&mut g, fx, "pattern", ParamValue::Enum(*pattern));
        if let Some(v) = background {
            let b = gray_color(&mut g, *v);
            g.connect(b, "o:output", fx, "i:background").unwrap();
        }
        let out = eval_read(&mut e, &g, fx);
        let bytes: Vec<u8> = out.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8).collect();
        let gray = e.image(fx).unwrap().is_gray();
        let rgba: Vec<u8> = bytes.chunks(4).flat_map(|p| if gray { [p[0], p[0], p[0], 255] } else { [p[0], p[1], p[2], 255] }).collect();
        let file = std::path::Path::new(&dir).join(format!("fx_map_{name}.png"));
        image::RgbaImage::from_raw(SIZE, SIZE, rgba).unwrap().save(file).unwrap();
    }
}

/// Pixel Math with vectors: the channels of an image (`img.rgb`), vectors made and worked on
/// component by component, a value given a name before it is read, and random numbers of each
/// pixel.
#[test]
fn pixel_math_with_vectors_and_names() {
    use crate::gpu::{COLOR_FORMAT, GRAY_FORMAT};
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let pm = g.add_node(NodeKind::PIXEL_MATH, [0.0, 0.0]);
    let colored = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
    set(&mut g, colored, "color", ParamValue::Color([0.2, 0.4, 0.6, 1.0]));
    set(&mut g, pm, "expression", ParamValue::Text("img.bgr * 0.5".into()));
    g.connect(colored, "o:output", pm, &brewed_texture_core::graph::var_socket_key("img")).unwrap();
    let px = eval_read(&mut e, &g, pm);
    assert_eq!(e.image(pm).unwrap().format(), COLOR_FORMAT);
    let p = pixel(&px, 50, 60);
    assert!(close(p[0], 0.3, 1e-3) && close(p[1], 0.2, 1e-3) && close(p[2], 0.1, 1e-3) && p[3] == 1.0, "{p:?}");
    // A channel read anywhere.
    set(&mut g, pm, "expression", ParamValue::Text("sample(img, 0.5, 0.5).g".into()));
    let p = pixel(&eval_read(&mut e, &g, pm), 3, 3);
    assert!(close(p[0], 0.4, 1e-3), "{p:?}");
    assert_eq!(e.image(pm).unwrap().format(), GRAY_FORMAT);
    // A name given a vector, then the value: the normal of a dome, how much it faces up.
    set(&mut g, pm, "expression", ParamValue::Text("n = vec3(u - 0.5, v - 0.5, 0.5)\n# how much it faces up\nnormalize(n).z".into()));
    let px = eval_read(&mut e, &g, pm);
    let at = |x: u32, y: u32| {
        let (u, v) = ((x as f32 + 0.5) / RES as f32 - 0.5, (y as f32 + 0.5) / RES as f32 - 0.5);
        0.5 / (u * u + v * v + 0.25).sqrt()
    };
    for (x, y) in [(128, 128), (0, 0), (40, 200)] {
        assert!(close(pixel(&px, x, y)[0], at(x, y), 2e-3), "({x}, {y}): {} not {}", pixel(&px, x, y)[0], at(x, y));
    }
    // A list of a vector and a number: red and green from the vector, blue.
    set(&mut g, pm, "expression", ParamValue::Text("vec2(0.25, 0.5), 0.75".into()));
    assert_eq!(pixel(&eval_read(&mut e, &g, pm), 7, 7), [0.25, 0.5, 0.75, 1.0]);
    // Random numbers of each pixel: the same every time, spread from 0 to 1.
    set(&mut g, pm, "expression", ParamValue::Text("rand(1)".into()));
    let noise = eval_read(&mut e, &g, pm);
    assert_eq!(eval_read(&mut e, &g, pm), noise);
    assert!((mean(&noise) - 0.5).abs() < 0.02, "{}", mean(&noise));
    assert_ne!(pixel(&noise, 1, 1), pixel(&noise, 2, 1));
    assert!(e.gpu.take_errors().is_empty());
}

/// An FX-Map program holds vectors: of its own names, and as the values of its lists.
#[test]
fn fx_map_programs_hold_vectors() {
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let fx = fx_map(&mut g, "p = vec2(0.25, 0.5)\nx = p.x; y = p.y\nsize = vec2(0.25, 0.25)\ncolor = vec3(1, 0.5, 0)", 1, (0, 0), MAX);
    let px = eval_read(&mut e, &g, fx);
    assert_eq!(pixel(&px, 64, 128), [1.0, 0.5, 0.0, 1.0]);
    assert_eq!(pixel(&px, 192, 128), [0.0, 0.0, 0.0, 1.0]);
    assert!(e.gpu.take_errors().is_empty());
}

/// What the Home screen's Learn opens, and the templates of new graphs, are computed with no
/// error, and their outputs are not flat.
#[test]
fn examples_and_templates_are_computed() {
    use brewed_texture_core::templates::{Example, Template};
    let Some(mut e) = engine() else { return };
    let mut graphs = vec![("PBR".to_string(), Template::Pbr.graph(256, brewed_texture_core::Precision::Half))];
    for example in [Example::FxMaps, Example::PixelMath] {
        let (package, _) = example.package();
        graphs.extend(package.graphs.iter().map(|g| (g.name.clone(), Graph::clone(&g.graph))));
    }
    for (name, mut g) in graphs {
        g.resolution = [256, 256];
        let ev = evaluate_blocking(&mut e, &g);
        for n in &g.nodes {
            assert!(ev.node(n.id).and_then(|n| n.error()).is_none(), "{name}: {}", n.def().name);
            assert!(e.state(n.id).is_none_or(|s| s.error.is_none()), "{name}: {} {:?}", n.def().name, e.state(n.id).and_then(|s| s.error.clone()));
        }
        if name != "PBR" {
            for out in g.nodes_of(NodeKind::OUTPUT) {
                let px = e.read_image(out.id).expect("an image").2;
                let m = mean(&px);
                let spread = (px.iter().step_by(4).map(|v| (*v as f64 - m).powi(2)).sum::<f64>() / (px.len() / 4) as f64).sqrt();
                assert!(spread > 0.01, "{name}: {} is flat", out.text("name"));
            }
        }
    }
    assert!(e.gpu.take_errors().is_empty());
}

/// A file packed of channels of several images (an export preset's ORM, mask map…): as big as
/// the texture, a uniform color's one pixel read everywhere, a channel turned over where it is
/// to be, a value where there is no image.
#[test]
fn channels_of_images_are_packed_into_one_file() {
    use crate::export::{DEPTH_8, Packed, write_packed};
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let gray = |g: &mut Graph, v: f32| {
        let c = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, 0.0]);
        set(g, c, "color", ParamValue::Color([v, v, v, 1.0]));
        set(g, c, "output", ParamValue::Enum(1));
        let out = g.add_node(NodeKind::OUTPUT, [0.0, 0.0]);
        g.connect(c, "o:output", out, "i:input").unwrap();
        out
    };
    // Not 0.5: 1 − 0.5 is 127.5 of 255, between two bytes, and GPUs round its last bit apart.
    let (a, b) = (gray(&mut g, 0.2), gray(&mut g, 0.4));
    let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    let out = g.add_node(NodeKind::OUTPUT, [0.0, 0.0]);
    g.connect(noise, "o:output", out, "i:input").unwrap();
    evaluate_blocking(&mut e, &g);
    let images: std::collections::HashMap<_, _> = [a, b, out].into_iter().map(|n| (n, e.read_own(n).unwrap())).collect();
    assert_eq!((images[&a].width, images[&a].height), (1, 1), "a uniform color's one pixel");
    let dir = std::env::temp_dir().join(format!("brewed-pack-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("orm.png");
    let pack = vec![
        Packed::Channel { node: a, channel: 0, invert: false },
        Packed::Channel { node: b, channel: 0, invert: true },
        Packed::Value(0.8),
        Packed::Channel { node: out, channel: 0, invert: false },
    ];
    write_packed(&file, &pack, &images, [RES, RES], DEPTH_8).unwrap();
    let png = image::open(&file).unwrap().to_rgba8();
    assert_eq!(png.dimensions(), (RES, RES), "as big as the texture");
    let noise_px = e.read_image(out).unwrap().2;
    for (x, y) in [(0, 0), (100, 37), (RES - 1, RES - 1)] {
        let p = png.get_pixel(x, y).0;
        for (c, want) in [51, 153, 204].into_iter().enumerate() {
            assert!((i32::from(p[c]) - want).abs() <= 1, "channel {c} at ({x}, {y}): {} not {want}", p[c]);
        }
        let want = (pixel(&noise_px, x, y)[0] * 255.0).round() as i32;
        assert!((i32::from(p[3]) - want).abs() <= 1, "the noise in alpha at ({x}, {y}): {} not {want}", p[3]);
    }
    // Three channels: RGB; one: gray.
    write_packed(&file, &pack[..3], &images, [RES, RES], DEPTH_8).unwrap();
    assert_eq!(image::open(&file).unwrap().color(), image::ColorType::Rgb8);
    write_packed(&file, &pack[..1], &images, [RES, RES], DEPTH_8).unwrap();
    assert_eq!(image::open(&file).unwrap().color(), image::ColorType::L8);
    // Nothing but uniform colors: the texture's size still.
    write_packed(&file, &pack[..2], &images, [RES, RES], DEPTH_8).unwrap();
    assert_eq!(image::open(&file).unwrap().to_rgb8().dimensions(), (RES, RES));
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a preset writes of an Output is what arrives at it, as the graph computes it: not the
/// Output's own image, which is in the channels and at the bit depth of its own file (here its
/// alpha, of 8 bits).
#[test]
fn presets_write_what_arrives_at_outputs() {
    use crate::export::{DEPTH_16, Packed, nodes_of, read_what_arrives, write_packed};
    let Some(mut e) = engine() else { return };
    let mut g = graph();
    let noise = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    let out = g.add_node(NodeKind::OUTPUT, [0.0, 0.0]);
    g.connect(noise, "o:output", out, "i:input").unwrap();
    set(&mut g, out, "format", ParamValue::Enum(6));
    evaluate_blocking(&mut e, &g);
    let dir = std::env::temp_dir().join(format!("brewed-arrives-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut job = crate::ExportJob {
        node: out,
        path: dir.join("height.png"),
        format: 0,
        bit_depth: DEPTH_16,
        pack: vec![Packed::Channel { node: out, channel: 0, invert: false }],
    };
    read_what_arrives(&mut job, &g);
    assert_eq!(nodes_of(&job), [noise]);
    let images = [(noise, e.read_own(noise).unwrap())].into_iter().collect();
    write_packed(&job.path, &job.pack, &images, [RES, RES], job.bit_depth).unwrap();
    let png = image::open(&job.path).unwrap();
    assert_eq!(png.color(), image::ColorType::L16, "gray, of 16 bits");
    let png = png.to_luma16();
    let noise_px = e.read_image(noise).unwrap().2;
    let mut finer = false;
    for (x, y) in [(3, 5), (100, 37), (180, 90), (RES - 1, RES - 2)] {
        let v = pixel(&noise_px, x, y)[0];
        let written = f32::from(png.get_pixel(x, y).0[0]) / 65535.0;
        assert!((written - v).abs() < 1e-3, "({x}, {y}): {written} for {v}");
        // Between the steps of 8 bits.
        finer |= (written * 255.0 - (written * 255.0).round()).abs() > 0.01;
    }
    assert!(finer, "16 bits of what arrives, not the Output's 8");
    let _ = std::fs::remove_dir_all(&dir);
}
